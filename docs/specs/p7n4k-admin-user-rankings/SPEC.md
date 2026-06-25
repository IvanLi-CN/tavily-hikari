# p7n4k · Admin 用户排行

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，关键演进原因见 `./HISTORY.md`。

## Summary

- 新增独立 `/admin/rankings` 管理台模块，以 `24h / 7d / 30d / 主要调用 / 积分 / IP` 六个单选 tab 组织排行视图。
- 排行页 URL 以单一 `tab` 查询参数表达当前视角：`/admin/rankings?tab=last24h|last7d|last30d|primarySuccess|businessCredits|uniqueIp`；缺失或非法值统一回退并规范化为 `last24h`。
- 每个时间窗固定三张榜：`成功主要调用`、`积分消耗` 与 `IP`，各取 `TOP20` 用户，按 `value desc, userId asc` 排序。
- 新增 admin-only HTTP 快照 `GET /api/users/rankings` 与独立 SSE `GET /api/users/rankings/events`；SSE 建连首帧即发 `snapshot`，之后每 10 秒推送。
- 排行身份统一返回 `userId / displayName / username / avatarUrl`；前端展示优先级为 `displayName > username > userId`，优先显示真实头像，缺失或加载失败时回退为稳定 mock 头像。
- 后端通过用户级 rollup、partial bucket 补扫与 10 秒 snapshot cache/singleflight 组合支撑滚动窗口，不回退到“每 10 秒扫 30 天原始日志”。
- 排行 snapshot 由 runtime 持有；Tab 切换、进入用户详情后返回、浏览器前进后退都不得把已有数据清空回首屏 blocking skeleton。

## Scope

- Rust backend
  - 扩展 `account_usage_rollup_buckets.metric_kind`，新增用户级 `primary_success` rollup。
  - `GET /api/users/rankings`
  - `GET /api/users/rankings/events`
  - 用户排行快照缓存、SSE snapshot stream、公开头像安全解析复用。
- Web admin
  - 新增 `rankings` route/nav/module，路径固定 `/admin/rankings`。
  - 新增横向柱状图页面、HTTP 首屏拉取、独立 SSE 活态更新、稳定 mock avatar fallback。
  - 新增 Storybook 页面级 stories 与最小合同测试。
- Docs
  - 本 spec 与 `docs/specs/README.md` 索引登记。

## Data Contract

### `GET /api/users/rankings`

- 返回结构：
  - `generatedAt`
  - `refreshIntervalSecs`
  - `last24h`
  - `last7d`
  - `last30d`
- 每个时间窗固定：
  - `primarySuccessTop`
  - `businessCreditsTop`
  - `uniqueIpTop`
- 每个排行 row 固定：
  - `rank`
  - `value`
  - `user`
    - `userId`
    - `displayName`
    - `username`
    - `avatarUrl`

### `GET /api/users/rankings/events`

- `Content-Type: text/event-stream`
- 建连成功后立即发送：
  - `event: snapshot`
  - `data: <same shape as GET /api/users/rankings>`
- 后续固定每 10 秒推送一次 `snapshot`
- 查询失败时发送：
  - `event: degraded`
  - `data: <error message>`

## Metric Semantics

- `primarySuccessTop`
  - 仅统计 `valuable success / primary_success`
  - 使用滚动窗口，不使用自然日或自然月摘要
- `businessCreditsTop`
  - 仅统计本地 `business_credits` 已记账消耗
  - 不引入上游实扣排行
- `uniqueIpTop`
  - 统计当前滚动时间窗内用户的唯一 `client_ip` 数
  - 仅统计 `request_logs.visibility=visible` 的记录
  - 过滤空 `client_ip`
  - 不要求请求成功，也不要求请求计费
- 用户入榜条件
  - 当前窗口该指标 `value > 0`
  - 不过滤当前 `active` 状态

## UI Constraints

- 页面主视图固定为六个全局单选 tab：`24h / 7d / 30d / 主要调用 / 积分 / IP`，默认选中最近 `24h`。
- 当前视角只以 URL `tab` 为真相源，不再维护会在页面卸载时丢失的本地排行 tab 状态。
- 选中 `24h / 7d / 30d` 时，内容区三张榜顺序固定为 `主要调用 → 积分 → IP`，三张卡共用当前时间窗。
- 选中 `主要调用 / 积分 / IP` 时，内容区三张榜顺序固定为 `最近 24 小时 → 最近 7 天 → 最近 30 天`，三张卡共用当前指标与说明文案。
- 六个 tab 在视觉上作为单行 tab strip 呈现；移动端允许自然换行，但任一时刻只允许一个 active tab。
- 桌面端内容区三栏并排展示三张榜；移动端改为纵向堆叠，不允许横向滚动。
- 图表使用成熟第三方图表库 `Apache ECharts + echarts-for-react`；允许在 ECharts 内通过 `custom series` 统一渲染身份区、柱体与数值，但不允许再叠加图外 DOM 假标签层。
- 每张榜作为单一 chart surface 渲染；身份信息不再拆成图外独立列表，但允许叠加与图内 row 一一对应的透明交互命中层，用于 click / hover / focus 合同。
- 每条 bar 仅展示一份用户身份：`rank + avatar + 单一显示名`，不再重复展示 secondary identity。
- 每条排行项都必须可点击，点击后走 SPA 导航进入 `/admin/users/:id`。
- hover 或 focus 某条排行项时，仅当前可见三张榜里同 `userId` 的条目联动高亮；离开 hover 或 blur 后立即清除。
- 页面需显式展示实时连接状态、最后更新时间与断连后的重试入口；SSE 退化时允许继续展示最近一次成功快照。
- 为避免核心排行信息只存在于 canvas，页面必须补充同榜单同内容的语义 DOM fallback，供辅助技术读取。
- 头像 URL 只使用服务端安全解析后的公开 `avatarUrl`；无头像或加载失败必须回退为稳定 mock 头像，不得整屏退化为字母圆牌。

## Acceptance

- `/admin/rankings` 作为独立 admin 模块出现在导航中，不影响 `/admin/users/usage` 与 `/admin/tokens/leaderboard`；进入页面后 URL 必须显式包含合法 `tab`。
- HTTP 与 SSE `snapshot` payload 结构完全一致。
- 24h / 7d / 30d 三个窗口都返回 `primarySuccessTop`、`businessCreditsTop` 与 `uniqueIpTop` 三榜，且每榜最多 20 行。
- 排序固定为 `value desc, userId asc`。
- SSE 建连后立即收到首帧，并每 10 秒持续推送。
- 点击 `24h / 7d / 30d` 时，三张榜分别展示同一时间窗下的 `主要调用 / 积分 / IP`。
- 点击 `主要调用 / 积分 / IP` 时，三张榜分别展示该指标在 `最近 24 小时 / 最近 7 天 / 最近 30 天` 的排行。
- 点击任一榜单条目进入 `/admin/users/:id` 后返回排行榜，原 `tab` 必须保留，且若 runtime 内已有 snapshot，不得回退到首屏 blocking skeleton。
- hover 或 focus 某条目时，当前 Tab 下另外两张榜中同用户条目同步高亮；移开或失焦后高亮清除。
- `web demo` 页面能证明“6 个单选 tab + URL 视角 + 三榜内容区 + 联动高亮”的桌面三栏、移动端堆叠与暗色桌面布局稳定。

## Visual Evidence

- owner-facing route: `/admin/rankings?demo=true`
- Storybook stories continue to cover internal states and route contracts, but final owner-facing screenshots are unified to `web demo`.

### Web Demo Desktop With Data

- source_type: ui_demo
  route: `/admin/rankings?demo=true&tab=last24h`
  state: `Desktop light theme with default time-window tab`
  evidence_note: 桌面端默认浅色主题，展示 URL 驱动的 `last24h` tab、状态簇，以及同一时间窗下的 `主要调用 / 积分 / IP` 三张榜。
  PR: include

![Web demo desktop](./assets/web-demo-rankings-desktop.png)

### Web Demo Metric Tab With Linked Highlight

- source_type: ui_demo
  route: `/admin/rankings?demo=true&tab=businessCredits`
  state: `Desktop light theme with metric-tab window cards and linked highlight`
  evidence_note: `积分` tab 下三张卡切换为 `最近 24 小时 / 最近 7 天 / 最近 30 天`，并通过 hover 第一名 `Alice Chen` 证明同用户跨三榜联动高亮生效。
  PR: include

![Web demo business credits highlight](./assets/web-demo-rankings-business-credits-highlight.png)

### Web Demo Mobile With Data

- source_type: ui_demo
  route: `/admin/rankings?demo=true&tab=last24h`
  state: `Mobile with populated rankings`
  evidence_note: 移动端 `web demo` 展示 URL 驱动的默认时间 tab 与纵向堆叠三榜，不出现横向滚动或内容串位。
  PR: include

![Web demo mobile](./assets/web-demo-rankings-mobile.png)

### Web Demo Desktop Dark

- source_type: ui_demo
  route: `/admin/rankings?demo=true&tab=last24h`
  state: `Desktop dark theme with populated rankings`
  evidence_note: 暗色桌面版沿用同一六 tab 与三榜结构，只切换共享主题 token，不存在第二套排行壳子。
  PR: include

![Web demo dark desktop](./assets/web-demo-rankings-dark-desktop.png)
