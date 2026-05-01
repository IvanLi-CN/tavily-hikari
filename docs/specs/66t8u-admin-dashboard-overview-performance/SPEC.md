# Admin：管理仪表盘总览加载去重与轻量快照（#66t8u）

## 状态

- Status: 已实现（待审查）
- Created: 2026-04-06
- Last: 2026-04-17

## 背景

- 当前 `/admin/dashboard` 首屏会并行触发 `loadData()` 与 dashboard 专用 overview 请求，两条链路都会拉 `summary`，同时还会连带触发 token page、token groups、recent logs、jobs 等额外请求。
- admin SSE `snapshot` 与首屏 HTTP bootstrap 维护了两套相近但不一致的数据拼装逻辑，既增加后端重复查询，也让前端在同一时间窗口里处理多份重叠数据。
- `/api/summary/windows` 需要扫描 `request_logs` 与配额样本；在 dashboard 首屏、SSE `compute_signatures()` 与 `snapshot` 生成阶段重复执行时，会放大停顿感和后台压力。随着日志增长，这条路径已经成为 dashboard 首屏的主要瓶颈。
- dashboard 可见风险区实际只展示前 5 项，但后端此前仍会为 dashboard 拉分页 keys、分页 logs、facets 与更多不需要的全量数据。

## Goals

- 新增 dashboard 专用轻量聚合接口，首屏只请求一份最小可用 overview 快照。
- 让 admin SSE `snapshot` 与 overview HTTP 复用同一套 payload 构造逻辑，减少重复查询与字段漂移。
- 将 dashboard 期间摘要改为读取专用 rollup 表，避免再对 `request_logs` 做大范围实时聚合。
- 让 dashboard 小时图与 `summary_windows` 共用近实时 rollup 数据源，保证当前小时写入后无需等待后台 job 即可反映到 overview 与 SSE snapshot。
- 将 dashboard 风险区所需的 `exhausted keys / recent logs / recent jobs / disabled tokens` 改为后端轻量子集查询，不再走分页 + facets + 全量 token 扫描。
- 保持 dashboard 可见功能和核心数据口径不变：期间摘要、当前状态、风险观察、近期请求、近期任务都继续可用。

## Non-goals

- 不修改 `/api/summary`、`/api/jobs`、`/api/logs`、`/api/keys` 现有对其它页面的语义。
- 不调整 dashboard 卡片视觉结构、文案层级或风险排序逻辑。
- 不修改 `/api/dashboard/overview`、admin SSE `snapshot` 的外部返回 shape。
- 不改变 `summary_windows` 的业务口径；仅将数据来源从原始 `request_logs` 扫描切到 dashboard 专用 rollup。

## 接口与数据契约

### `GET /api/dashboard/overview`

- 仅管理员可访问。
- 返回 dashboard 首屏真正需要的最小快照：
  - `summary`
  - `summaryWindows`
  - `siteStatus`
  - `forwardProxy`
  - `exhaustedKeys`
  - `recentLogs`
  - `recentJobs`
  - `disabledTokens`
  - `tokenCoverage`
- `summary` 与 `summaryWindows` 结构必须继续与既有接口保持一致。
- `tokenCoverage` 仅允许：
  - `ok`
  - `truncated`
  - `error`

### admin SSE `snapshot`

- `snapshot` 必须复用 `GET /api/dashboard/overview` 的同一份 payload 构造逻辑。
- 顶层保留：
  - `summary`
  - `summaryWindows`
  - `siteStatus`
  - `forwardProxy`
- 顶层新增：
  - `exhaustedKeys`
  - `recentLogs`
  - `recentJobs`
  - `disabledTokens`
  - `tokenCoverage`
- 为兼容历史 dashboard 客户端，继续保留 `keys` 与 `logs` 字段，但其内容改为与 `exhaustedKeys` / `recentLogs` 同步的轻量子集，不再返回旧的全量分页载荷。

### dashboard 轻量查询约束

- exhausted keys：
  - 只取 `status=exhausted` 的前 5 项。
  - 不计算 facets。
- recent logs：
  - 直接复用 summary log 视图，仅取前 5 条。
  - 不返回请求体/响应体。
- recent jobs：
  - 仅取最近 5 条任务。
- disabled tokens：
  - 查询 `enabled = 0 AND deleted_at IS NULL`。
  - 实际查询前 6 条，响应只返回前 5 条。
  - 若存在第 6 条，则 `tokenCoverage = truncated`。

## 行为约束

- dashboard route 首屏加载拆成两层：
  - shell data：`profile`、`version`
  - module data：dashboard overview
- dashboard route 不再通过通用 `loadData()` 拉 `summary`、token page 或 token groups。
- `loadDashboardOverview()` 必须收敛为单请求模型，只调用 `fetchDashboardOverview()`。
- SSE 正常可用时，dashboard 活态增量更新只依赖 `snapshot`；不得再保留独立的 30 秒 dashboard signals polling。
- SSE 断线或 degraded 后，fallback polling 只能刷新 shell data + overview，不得回退到 `loadData()` 与 `loadDashboardOverview()` 双通路并发。
- 手动刷新 dashboard 时，也只能触发 shell data + overview 的一次刷新。
- `summary_windows` 与 `hourlyRequestWindow` 必须在日志写入的同一事务路径内保持近实时更新；不得依赖单独的异步 materialize job 才能看到当前小时变化。
- `summaryWindows.month` 的**次数类指标**按服务器时区自然月统计；仅 `quota_charge`（本月积分消耗）继续按 UTC 自然月统计，避免与现有积分审计口径漂移。

## 性能约束

- 新增内部表 `dashboard_request_rollup_buckets`：
  - 主键 `(bucket_start, bucket_secs)`
  - `bucket_secs=60` 用于 UTC 分钟桶
  - `bucket_secs=86400` 用于本地日桶
  - 字段至少覆盖 `total/success/error/quota_exhausted`、`valuable/other/unknown` 分类、`mcp/api × billable/non_billable` 分类、`local_estimated_credits`、`updated_at`
- `summary_windows` 与 `hourlyRequestWindow` 读路径必须优先查 rollup，不再重扫 `request_logs`。
- dashboard overview / snapshot 对这两块数据不再使用 2 秒 TTL 缓存；当前小时写入后下一次请求或 snapshot 必须直接可见。
- dashboard HTTP / SSE overview 逻辑不得再触发：
  - logs facets 聚合
  - keys 分页 facets 聚合
  - dashboard 首屏全量 token 扫描
- 首屏进入 dashboard 时，不应再出现两次 `/api/summary` 的并发拉取。
- forward proxy overview / live stats 对 `forward_proxy_attempts` 的 1m / 15m / 1h / 1d / 7d 统计必须使用单次 7d bounded scan 派生所有窗口，避免同一请求重复扫描 attempts 表。
- forward proxy 窗口统计在同一 manager 内使用短 TTL + singleflight 风格缓存，避免管理端刷新周期内 settings/live stats 反复触发同一 7d scan；响应 shape 与统计口径保持不变。
- dashboard 与管理端列表共享 SQLite worker 时，重读接口必须使用有界并发保护，避免 dashboard overview 被其它 admin 慢查询拖入 worker 饱和。
- 全局 request-log catalog 读取已从 shared heavy-read 保护中拆出，并改为 rollup-backed 查询；dashboard overview 不得通过 legacy `/api/logs` 重新引入 request log facets 宽表扫描。

## 验收标准

- `GET /api/dashboard/overview` 未认证返回 `403`，管理员访问返回完整 overview 结构。
- overview 中 `summary`、`summaryWindows` 的值与现有接口保持一致。
- overview 中 `disabledTokens`、`exhaustedKeys`、`recentLogs`、`recentJobs` 都遵守轻量 limit。
- 当禁用 token 数量超过 5 条时，`tokenCoverage = truncated` 且响应只返回前 5 条。
- admin SSE `snapshot` 继续刷新 dashboard 所需字段，但 `keys` / `logs` 兼容别名只承载轻量子集。
- dashboard 首屏不再重复触发 summary/bootstrap 双重加载。
- 新写入一条 request log 后，不等待后台 job，`summaryWindows.today` 与 `summaryWindows.month` 的请求计数和 `local_estimated_credits` 即可反映到 overview / snapshot。
- SSE 正常时，dashboard 不再维持旧的 30 秒 signals polling；SSE 断线后 fallback polling 只刷新 shell data + overview。
- `cargo test`、`cargo clippy -- -D warnings`、`cd web && bun test src/api.test.ts src/admin/dashboardHourlyCharts.test.ts`、`cd web && bun run build`、`cd web && bun run build-storybook` 通过。

## 当前验证记录

- `2026-04-06`：`cargo test --quiet dashboard_overview_` 通过。
- `2026-04-06`：`cargo test --quiet admin_dashboard_sse_snapshot_includes_overview_segments` 通过。
- `2026-04-06`：`cargo test --quiet compute_signatures_tracks_quarantined_key_count` 通过。
- `2026-04-06`：`cargo test admin_dashboard_sse_snapshot_refreshes_when_quota_totals_change -- --nocapture` 通过；期间将 SSE 签名查询进一步瘦身为最小触发集，避免仅为签名轮询拉取完整 logs/token quota。
- `2026-04-06`：`cargo test` 全量通过。
- `2026-04-06`：`cargo clippy -- -D warnings` 通过。
- `2026-04-06`：`cargo fmt` 通过。
- `2026-04-06`：`cd web && bun test src/api.test.ts` 通过。
- `2026-04-06`：`cd web && bun run build` 通过。
- `2026-04-06`：`cd web && bun run build-storybook` 通过。
- `2026-04-06`：使用当前 worktree 的 Storybook 静态预览端口 `127.0.0.1:30020` 打开 `Admin/Components/DashboardOverview/ZhDarkEvidence` iframe，确认 dashboard 总览结构、风险观察与快捷入口在轻量 overview 收敛后保持稳定。
- `2026-04-30`：`cargo test admin_forward_proxy_settings_and_stats_endpoints_work -- --nocapture` 通过，覆盖 forward proxy stats 单次窗口集合查询后的响应结构。
- `2026-05-01`：`cargo test admin_forward_proxy_settings_and_stats_endpoints_work -- --nocapture` 通过，覆盖 forward proxy stats 短 TTL 缓存后的响应结构不变。

## 实现里程碑

- [x] M1: 新增 dashboard 专用 overview 接口并抽出共享 payload 组装逻辑
- [x] M2: 新增 dashboard 专用 rollup 表，并将 `summary_windows` / 小时图切到 rollup 读路径
- [x] M3: dashboard 风险区改走轻量子集查询与 SSE snapshot 复用
- [x] M4: 前端 dashboard 首屏加载去重，移除旧的 signals polling
- [x] M5: Storybook/mock 视觉证据补齐
- [ ] M6: PR 收口与 merge-ready 状态同步

## 风险与开放点

- 本次优化引入 dashboard 专用 rollup 表，后续若要扩展更多 dashboard 维度，需继续维持写入同事务 upsert 与 bounded rebuild 的幂等性。
- `snapshot` 仍保留兼容别名 `keys` / `logs`；若后续确认没有其它消费者，可再做一次协议瘦身。
- dashboard 现有 Storybook 证据主要证明 UI 结构保留，不直接证明网络负载下降；性能收益仍以接口去重、payload 收缩与查询复用为主。

## Visual Evidence

- source_type: `storybook_canvas`; story_id_or_title: `Admin/Components/DashboardOverview/ZhDarkEvidence`; state: `dashboard overview preserved after lightweight bootstrap refactor`; evidence_note: 验证 dashboard 在改为单一 overview bootstrap、SSE 复用 payload 与风险区轻量子集后，今日/本月/当前状态、风险观察与快捷入口仍保持既有可见结构。
  PR: include
  ![管理仪表盘总览轻量快照验收图](./assets/dashboard-overview-performance-proof.png)

## Change log

- 2026-04-06: 初始化 spec，冻结 dashboard overview 轻量聚合接口、SSE payload 复用、`summary_windows` TTL 缓存与前端 dashboard bootstrap 去重的执行合同。
- 2026-04-06: 完成 dashboard overview 聚合接口、SSE snapshot 复用、轻量风险区查询与前端 dashboard route 去重；随后将 SSE 签名轮询进一步收敛为最小触发查询，并补齐 Storybook 静态预览证据。
- 2026-04-17: 将 `summary_windows` 与 dashboard 小时图切到 `dashboard_request_rollup_buckets`，移除 2 秒 freshness 缓存依赖，确保当前小时与本地估算额度可近实时出现在 overview / snapshot。
- 2026-04-30: 将 forward proxy 窗口统计收敛为单次 bounded scan，并补充 admin heavy-read 并发保护，避免线上 SQLite worker 饱和时 dashboard overview 被重读拖慢。
- 2026-05-01: 为 forward proxy 窗口集合查询增加 manager-scoped 短 TTL 缓存，减少同一管理端刷新周期内的重复 7d scan。
