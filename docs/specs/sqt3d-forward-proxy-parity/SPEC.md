# Tavily Hikari 正向代理等价功能对齐（#sqt3d）

## 状态

- Status: 已完成（快车道）
- Created: 2026-03-12
- Last: 2026-03-16

## 背景 / 问题陈述

- 现有 `tavily-hikari` 只能直连 Tavily MCP/HTTP/usage 上游，管理台“代理设置”仍是占位骨架，无法消费订阅型代理节点。
- 主人的实际使用方式是“只提供订阅给程序”，因此缺少订阅解析、share-link 支持、多节点调度与运行态观测时，功能基本不可用。
- 现有请求虽然已有 Tavily API key 亲和与 research request key affinity，但没有“上游 key -> 有限代理节点池”的绑定层，容易造成同一个上游 key 在全量节点间随机跳 IP。
- 不补齐这套能力，会导致 Tavily 链路无法稳定走代理、难以排查节点质量，也无法从管理台完成验证、调参与验收。

## 目标 / 非目标

### Goals

- 为 Tavily 链路引入与参考项目等价的 forward proxy 子系统：订阅导入、节点解析、Xray share-link 支持、多节点调度、验证探测、运行态持久化和统计 API/UI。
- 增加“上游 Tavily API key -> 主备双节点”的代理亲和层，正常情况下让同一个上游 key 只使用有限节点集合，减少 IP 乱跳。
- 仅覆盖 Tavily 出站链路：`/mcp`、`/api/tavily/*`、usage/quota sync、管理员 key 校验与 forward proxy 验证。
- 保持现有 Tavily key 选择、research request key affinity、额度计费、日志与 LinuxDo OAuth 语义不回归。

### Non-goals

- 不让 LinuxDo OAuth token/userinfo 走 forward proxy。
- 不把多个 Tavily keys 额外聚合成新的“上游账号”概念；首版主体固定为 `api_keys.id`。
- 不与生产 Tavily 上游联调；所有验证只使用本地/mock upstream、mock proxy、mock subscription。

## 范围（Scope）

### In scope

- 新增 `forward_proxy_settings`、`forward_proxy_runtime`、`forward_proxy_attempts`、`forward_proxy_weight_hourly`、`forward_proxy_key_affinity` 数据表与相应迁移/索引。
- 新增 forward proxy settings / validate / live stats API 与管理台真实页面。
- 支持原生代理 URL（`http`、`https`、`socks5`、`socks5h`）和 share-link（`vmess`、`vless`、`trojan`、`ss`）解析。
- 通过 `xray` 为 share-link 节点生成本地 socks5 route，并纳入统一调度与统计。
- 实现 subscription refresh、bootstrap probe、penalized recovery probe、多节点 weighted scheduling、`insertDirect` 直连兜底。
- 实现按 `api_keys.id` 的主备双节点亲和、重启恢复、订阅刷新保留、故障切换与备用提升。

### Out of scope

- OAuth 代理化、非 Tavily 出站流量代理化。
- 新增完全不同于参考项目的产品面或复杂运营配置。
- 绕过 mock-only 约束访问生产 Tavily。

## 需求（Requirements）

### MUST

- `GET /api/settings` 返回 `forwardProxy`，并新增 `PUT /api/settings/forward-proxy`、`POST /api/settings/forward-proxy/validate`、`GET /api/stats/forward-proxy`。
- settings 合约保持 `proxyUrls`、`subscriptionUrls`、`subscriptionUpdateIntervalSecs`、`insertDirect` 字段，并返回 `nodes`。
- subscription-only 配置可工作；share-link 节点依赖 Xray 时必须给出明确可诊断错误。
- share-link 中的展示名若来自 URL fragment，必须先做一次性 percent-decoding，再进入 validation、runtime/live stats 与 API/UI 输出链路。
- Tavily `/mcp`、`/api/tavily/search|extract|crawl|map|research`、`/api/tavily/research/:request_id`、管理员 key 校验、quota sync 全部经过 selected forward proxy。
- 同一个上游 key 默认只使用主节点；主节点不可达时切到备用，备用承接成功后提升为主，并补新的备用节点。
- `Direct` 默认只作为备用候选或无健康代理时的兜底主节点。
- 管理台 `/admin/proxy-settings` 不再是占位页，必须能完成 settings 读取、保存、候选验证、节点与 live stats 展示。

### SHOULD

- forward proxy 节点统计包含 `1m/15m/1h/1d/7d` 窗口成功率/平均延迟和 24h weight 变化。
- live stats 额外显示每个节点当前被多少上游 key 作为 `primary` / `secondary` 绑定。
- 非法 share-link、订阅为空、Xray 缺失、probe 超时等错误都能稳定映射成 API 可消费信息。

### COULD

- 针对 429/403/5xx 的代理惩罚先保持保守策略，只在统计和惩罚层反映，不强制对非幂等请求做多次转发重试。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 管理员在 `/admin/proxy-settings` 录入 subscription URL、可选手工 proxy URL、刷新周期与 `insertDirect`，保存后服务刷新代理池并对新增节点执行 bootstrap probe。
- 服务定期刷新 subscription，解析出新的节点集合；现有主/备节点仍存在且可用时必须保留，不得因权重波动主动重排。
- Tavily 请求先按现有逻辑选定上游 `api_keys.id`，再在该 key 的主备池中选节点；若主节点 transport/probe 失败，则切到备用。
- share-link 节点通过 Xray 生成本地 socks5 route，后续请求像普通 socks5h 节点一样被选中与统计。
- 管理员可通过 validate API 分别验证单节点和订阅；订阅验证在“至少一个解析出的节点可达”时成功。

### Edge cases / errors

- 没有任何健康代理时，如果 `insertDirect=true` 或没有可解析节点，系统必须回退到 `Direct`，而不是让 Tavily 链路整体不可用。
- 上游 key 返回 432 仅标记 key quota exhausted，不触发代理切换或 key-proxy 重新绑定。
- share-link 解析成功但 Xray 不可执行时，节点不得进入 selectable 集合，且设置页/验证结果要明确提示。
- 订阅刷新全部失败时，已存在的上一版可用节点应继续保留；只有显式保存为新的配置并确认生效时才替换。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                                | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）                                |
| ------------------------------------------- | ------------ | ------------- | -------------- | ------------------------ | --------------- | ------------------- | -------------------------------------------- |
| `GET /api/settings` (`forwardProxy`)        | HTTP API     | external      | Modify         | ./contracts/http-apis.md | backend         | admin web           | 返回 forward proxy settings + nodes          |
| `PUT /api/settings/forward-proxy`           | HTTP API     | external      | New            | ./contracts/http-apis.md | backend         | admin web           | 保存 settings，触发 routes/subscription 同步 |
| `POST /api/settings/forward-proxy/validate` | HTTP API     | external      | New            | ./contracts/http-apis.md | backend         | admin web           | 校验单节点或订阅                             |
| `GET /api/stats/forward-proxy`              | HTTP API     | external      | New            | ./contracts/http-apis.md | backend         | admin web           | live stats + 24h buckets + assignment counts |
| `forward_proxy_*`                           | DB           | internal      | New            | ./contracts/db.md        | backend         | backend             | settings/runtime/attempts/weight/affinity 表 |
| `XRAY_BINARY` / `XRAY_RUNTIME_DIR`          | CLI          | internal      | New            | ./contracts/cli.md       | backend         | deploy/runtime      | share-link 节点运行依赖                      |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/http-apis.md](./contracts/http-apis.md)
- [contracts/cli.md](./contracts/cli.md)
- [contracts/db.md](./contracts/db.md)

## 验收标准（Acceptance Criteria）

- Given 只配置 subscription URL
  When 服务刷新 forward proxy
  Then 能解析出多节点代理池，share-link 节点经 Xray 转为本地 socks5 route，`insertDirect=true` 时可见 `Direct` 节点。

- Given 同一个上游 Tavily API key 连续发起 Tavily 请求
  When 主节点健康
  Then 请求长期命中同一个主节点，不会在全量节点间随机乱跳。

- Given 主节点 transport/probe 失败且备用节点健康
  When 同一个上游 key 再次发起请求
  Then 请求切到备用，备用提升为新主节点，并补齐新的备用节点。

- Given 订阅刷新新增节点但原主/备节点仍存在且健康
  When refresh 完成
  Then 既有 key 的主备绑定保持不变。

- Given 管理员调用 validate API 校验 share-link 或订阅
  When share-link 非法、Xray 缺失、probe timeout、订阅全不可用
  Then API 返回稳定的失败消息，不把坏节点加入可用调度集合。

- Given 管理台打开 `/admin/proxy-settings`
  When settings 与 live stats 已加载
  Then 页面可查看 settings、节点窗口统计、24h weight、主/备绑定计数并执行保存与验证。

- Given 管理员在订阅或手工节点弹窗里拿到长结果内容（长 URL、验证消息、节点统计）
  When 弹窗显示验证结果
  Then footer 操作区必须始终留在可视区域内，内容改由弹窗 body 自身滚动承载，不得把 `取消 / 验证 / 添加或导入` 推出视口。

- Given 管理员校验 subscription URL
  When 弹窗显示订阅校验结果
  Then 原始 subscription URL 只保留在顶部输入框内供复制或继续编辑，结果卡片不得再次回显该 URL。

## 实现前置条件（Definition of Ready / Preconditions）

- 目标/非目标、scope in/out、mock-only 约束已明确
- 对外接口字段已冻结为 `proxyUrls` / `subscriptionUrls` / `subscriptionUpdateIntervalSecs` / `insertDirect`
- 上游 key 亲和主体已锁定为 `api_keys.id`
- 主备双节点 + 数据库持久化策略已确认

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: 代理 URL/订阅/share-link 解析，scheduler weight/penalty，key affinity 主备切换，Xray config 构造。
- Integration tests: mock upstream + mock proxy + mock subscription 下验证 `/mcp`、`/api/tavily/*`、quota sync、管理员 key 校验走代理。
- E2E tests (if applicable): 浏览器打开 `/admin/proxy-settings`，完成 subscription 添加、候选验证与 live stats 查看。
- Storybook: 至少提供 empty / subscription success / subscription failure / overflow proof 四个确定性 stories，其中 overflow proof 必须让弹窗 body 发生滚动而 footer 仍可见。

### UI / Storybook (if applicable)

- Stories to add/update: Admin proxy settings page state（空态、已配置、验证成功/失败、live stats），并补充可直接打开的 forward proxy 弹窗待验证 / 成功 / 失败复现 story。
- Visual regression baseline changes (if any): proxy settings 模块从 placeholder 切换为真实页面。

### Quality checks

- Lint / typecheck / formatting: `cargo fmt`、`cargo clippy -- -D warnings`、`cargo test`、`cd web && bun run build`

## 文档更新（Docs to Update）

- `README.md`: 补充 forward proxy、subscription、Xray 配置与运行说明
- `README.zh-CN.md`: 同步中文说明

## 计划资产（Plan assets）

- Directory: `docs/specs/sqt3d-forward-proxy-parity/assets/`
- In-plan references: `![...](./assets/<file>.png)`
- PR visual evidence source: maintain `## Visual Evidence (PR)` in this spec when PR screenshots are needed.
- If an asset must be used in impl (runtime/test/official docs), list it in `资产晋升（Asset promotion）` and promote it to a stable project path during implementation.

## Visual Evidence (PR)

![Forward proxy subscription dialog success state](./assets/forward-proxy-subscription-dialog-success.png)

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 落地 forward proxy 数据模型、CLI/runtime 配置与 backend settings/validate/stats API
- [x] M2: 落地订阅解析、share-link + Xray route sync、多节点调度与 key 主备亲和
- [x] M3: 将 Tavily 所有目标出站链路接入 selected forward proxy，并补齐 mock-only 后端测试
- [x] M4: 完成 `/admin/proxy-settings` 页面、前端 API 类型与浏览器/构建验收
- [x] M5: 更新 README / SPEC 同步，并完成 fast-flow 所需 review-loop 收敛

## 方案概述（Approach, high-level）

- 复用现有 `TavilyProxy` 作为 Tavily 出站统一入口，在其内部注入 forward proxy manager，而不是让 server handlers 分别拼装代理逻辑。
- 参考 `codex-vibe-monitor` 的 normalized settings、node parsing、XraySupervisor、runtime/attempts/weight 表与 live stats 结构，但按 Tavily-only 场景裁到最小必要实现。
- 上游 key 亲和层叠加在全局 proxy scheduler 之上：全局负责权重/探测，key 层只维护主备绑定与故障切换。
- UI 直接在现有 admin monolith 中接入新的 API 与模块，不再保留 placeholder。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：share-link 与 Xray 运行依赖较重，若 `xray` 不存在会影响一部分节点可用性；需确保错误可见且 direct/manual 节点仍能工作。
- 风险：对非幂等 Tavily 请求进行多节点重试存在重复请求风险，因此 transport 失败切换要谨慎控制重试次数。
- 需要决策的问题：无。
- 假设（需主人确认）：无；关键决策已在计划阶段锁定。

## 变更记录（Change log）

- 2026-03-12: 创建规格，冻结 forward proxy parity、subscription-only、Xray share-link 与上游 key 主备亲和口径。
- 2026-03-15: forward proxy parity 功能与 `/admin/proxy-settings` 收口完成；补齐订阅弹窗 footer 固定、成功/失败/overflow Storybook 复现、视觉证据与 PR-stage review-loop，规格状态切换为已完成（快车道）。
- 2026-03-16: 明确 share-link URL fragment 展示名要做一次性 percent-decoding，避免中文或 emoji 节点名在 validation/live stats 中退化为编码串。

## 参考（References）

- [codex-vibe-monitor forward proxy module](https://github.com/IvanLi-CN/codex-vibe-monitor/blob/main/src/forward_proxy/mod.rs)
- [reqwest Proxy::all](https://docs.rs/reqwest/latest/reqwest/struct.Proxy.html#method.all)
