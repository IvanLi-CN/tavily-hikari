# Admin 仪表盘 49 小时整点请求图表（#h2698）

## 状态

- Status: 已完成
- Created: 2026-04-07
- Last: 2026-04-17

## 背景

- 当前 `/admin/dashboard` 的 `Traffic Trends` 仍是 2 张最近日志 sparkline，只能看到模糊的请求量 / 错误量变化，无法尽早识别 MCP `429`、MCP/API 结构变化与昨日同小时异常。
- 近期 `429` 问题证明：运营需要一块固定在管理员仪表盘首页、能按小时对齐、能立即对比昨日同小时的请求图表，而不是再跳到明细日志里临时筛选。
- 仪表盘已经有 `overview` fetch 与 admin `/api/events` SSE snapshot，同步契约适合继续承载这块小时级图表数据。

## Goals

- 在管理员仪表盘现有 `Traffic Trends` 区域内，用单一 stacked bar 图表面板替换旧 sparkline。
- 后端统一返回最近 49 个**服务器时区对齐**的小时桶，并保证**最新一桶就是当前服务器时区小时进行中**；前端默认展示近 24 小时的 25 个小时桶，并将横轴标签按浏览器本地时间渲染，同时支持与昨日同小时做 signed delta 对比。
- 图表固定支持 4 种视图：
  - 调用结果
  - 调用类型
  - 与昨日对比 · 调用结果
  - 与昨日对比 · 调用类型
- 图表数据通过 `GET /api/dashboard/overview` 与 admin `/api/events` snapshot 共享同一契约，不新增单独 dashboard polling 接口。

## Non-goals

- 不调整 `request_logs` 的长期保留或 GC 策略。
- 不修改 public/user console 页面与 `/mcp` 外部协议。
- 不把调用类型拆到每个单独工具名；v1 只统计 `protocol × billing` 四类。

## 数据契约

### `DashboardHourlyRequestWindow`

- `bucketSeconds = 3600`
- `visibleBuckets = 25`
- `retainedBuckets = 49`
- `buckets[]` 按时间升序排列，最新一桶允许是“当前服务器时区小时进行中”：
  - `bucketStart`
  - `secondarySuccess`
  - `primarySuccess`
  - `secondaryFailure`
  - `primaryFailure429`
  - `primaryFailureOther`
  - `unknown`
  - `mcpNonBillable`
  - `mcpBillable`
  - `apiNonBillable`
  - `apiBillable`

### `GET /api/dashboard/overview`

- 在现有 payload 中新增 `hourlyRequestWindow`。
- 旧 `trend` 字段可保留为兼容字段，但 dashboard 前端不再用它作为主图表来源。

### admin `/api/events` snapshot

- `snapshot.overview.hourlyRequestWindow` 与 `GET /api/dashboard/overview` 完全一致。
- SSE 变更检测必须覆盖小时窗口锚点变化与小时桶内容变化，避免整点翻小时后图表不刷新。

## 统计口径

- 小时桶窗口：
  - 以**服务器时区当前整点**作为当前未封口小时起点，并将该整点换算成 UTC epoch `bucketStart`
  - 返回 `[currentHourStart - 48h, currentHourStart]` 的 49 个小时桶，其中最后一桶就是当前小时
  - 无日志的小时必须零填
- “主要 / 次要”直接复用现有 `request_value_bucket`：
  - `valuable -> primary`
  - `other -> secondary`
  - `unknown -> unknown`
- 调用结果分类：
  - `secondarySuccess` = `other + success`
  - `primarySuccess` = `valuable + success`
  - `secondaryFailure` = `other + (error | quota_exhausted)`
  - `primaryFailure429` = `valuable + failure_kind=upstream_rate_limited_429`
  - `primaryFailureOther` = `valuable + (error | quota_exhausted) - primaryFailure429`
  - `unknown` = `unknown + any result_status`
- 调用类型分类固定为：
  - `mcpNonBillable`
  - `mcpBillable`
  - `apiNonBillable`
  - `apiBillable`
- 与昨日对比：
  - 对最近 25 个小时中的每个当前 bucket，取 `bucketStart - 24h` 的 bucket 做差
  - 当前小时不做分钟截断，直接对比“昨日同小时整桶”
  - delta 图 Y 轴允许正负值

## 展示约束

- `Traffic Trends` 外层 panel、标题区与整体 dashboard 排布保持不变，只替换内部内容。
- 图表默认显示：
  - 结果图：`次要成功 → 主要成功 → 次要失败 → 主要失败·429 → 主要失败·其他 → unknown`
  - 类型图：`MCP 非计费 → MCP 计费 → API 非计费 → API 计费`
- 前两个绝对图默认全选全部 series。
- 前两个图使用多选显示/隐藏；后两个 delta 图使用单选，并额外提供 `全部`。
- 前端需要记忆上次选中的图表模式与 series 组合，并在下次重新打开管理台时恢复。
- 小时桶统计口径改为按**服务器时区**对齐，但 UI 文案必须明确表达“近 24 小时（共 25 组，含当前小时）”，横轴日期/时间标签按浏览器本地时间显示。
- API / MCP 配色必须复用请求记录界面的语义色族；结果图复用 success / warning / destructive / neutral 语义，不新造一套与现有 UI 脱节的颜色体系。

## 验收标准

- 管理员仪表盘首页能直接看到近 24 小时（共 25 组，含当前小时）的 stacked bar 图表，不再显示旧 sparkline 卡片。
- `/api/dashboard/overview` 与 `/api/events` snapshot 都包含 `hourlyRequestWindow`，且 dashboard 切到该路由后可实时刷新。
- `hourlyRequestWindow.retainedBuckets = 49`、`visibleBuckets = 25` 保持不变，且最新一桶必须等于 `currentHourStart`。
- 小时桶最后一组必须是当前服务器时区小时进行中；横轴标签则按浏览器本地时间展示同一批 bucket。
- 结果图与类型图的默认堆叠顺序、默认可见系列、delta 行为与本 spec 一致。
- 管理台重新打开后，会恢复上一次选中的图表模式与 series 显示状态。
- 当所有可见系列被隐藏时，图表区域显示明确 empty state，而不是坏图或空白画布。
- Storybook 覆盖 4 个图表模式、toggle 行为与空数据场景，并提供最终视觉证据。

## 里程碑

- [x] M1: spec 冻结与索引登记
- [x] M2: 后端 hourly bucket 聚合与 overview/snapshot 扩展
- [x] M3: DashboardOverview 图表模式、图例切换与 i18n
- [x] M4: Storybook / 前端测试 / 后端测试补齐
- [ ] M5: 视觉证据、review-loop 与快车道收敛

## 风险与假设

- 小时图读路径依赖 `dashboard_request_rollup_buckets(bucket_secs=60)`；若后续扩展更多 breakdown，需继续保持 rollup 写入与 bounded rebuild 的幂等性。
- 风险：如果 admin SSE 的变更签名没有覆盖小时锚点，整点切换时图表可能在“无新日志”场景下停留旧窗口。
- 风险：`mcp:batch` 的计费/非计费判定依赖 request body 解析；rollup 写入与 rebuild 必须复用现有 canonicalization 规则，否则会和请求日志页面口径漂移。

## Visual Evidence

- source_type: storybook_canvas
  story_id_or_title: `admin-components-dashboardoverview--default`
  state: `results`
  evidence_note: 验证绝对“调用结果”图默认全选全部结果 series，数据覆盖近 24 小时共 25 组且含当前小时，横轴标签按本地时间显示。
  image:
  ![管理员仪表盘小时图表：调用结果](./assets/dashboard-hourly-results.png)

- source_type: storybook_canvas
  story_id_or_title: `admin-components-dashboardoverview--types-mode`
  state: `types`
  evidence_note: 验证“调用类型”图按 MCP/API 与计费/非计费四类堆叠，复用请求记录界面的协议色族。
  image:
  ![管理员仪表盘小时图表：调用类型](./assets/dashboard-hourly-types.png)

- source_type: storybook_canvas
  story_id_or_title: `admin-components-dashboardoverview--results-delta-mode`
  state: `results-delta`
  evidence_note: 验证“与昨日对比·调用结果”图使用 signed Y 轴，并支持 `全部` 差值堆叠展示。
  image:
  ![管理员仪表盘小时图表：调用结果差值](./assets/dashboard-hourly-results-delta.png)

- source_type: storybook_canvas
  story_id_or_title: `admin-components-dashboardoverview--types-delta-mode`
  state: `types-delta`
  evidence_note: 验证“与昨日对比·调用类型”图支持单选/全部切换，并在 `全部` 下显示类型差值柱状图。
  image:
  ![管理员仪表盘小时图表：调用类型差值](./assets/dashboard-hourly-types-delta.png)

- source_type: storybook_canvas
  story_id_or_title: `admin-components-dashboardoverview--hidden-series-empty`
  state: `empty-selection`
  evidence_note: 验证绝对图在所有系列都被隐藏后呈现明确 empty state，而不是坏图或空白画布。
  image:
  ![管理员仪表盘小时图表：空系列状态](./assets/dashboard-hourly-empty.png)
