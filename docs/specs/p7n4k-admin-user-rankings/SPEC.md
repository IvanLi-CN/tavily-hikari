# p7n4k · Admin 用户排行

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，关键演进原因见 `./HISTORY.md`。

## Summary

- 新增独立 `/admin/rankings` 管理台模块，固定展示最近 `24h / 7d / 30d` 三个滚动时间窗。
- 每个时间窗固定两张榜：`成功主要调用` 与 `积分消耗`，各取 `TOP20` 用户，按 `value desc, userId asc` 排序。
- 新增 admin-only HTTP 快照 `GET /api/users/rankings` 与独立 SSE `GET /api/users/rankings/events`；SSE 建连首帧即发 `snapshot`，之后每 10 秒推送。
- 排行身份统一返回 `userId / displayName / username / avatarUrl`；前端展示优先级为 `displayName > username > userId`，优先显示真实头像，缺失或加载失败时回退为稳定 mock 头像。
- 后端通过用户级 rollup、partial bucket 补扫与 10 秒 snapshot cache/singleflight 组合支撑滚动窗口，不回退到“每 10 秒扫 30 天原始日志”。

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
- 用户入榜条件
  - 当前窗口该指标 `value > 0`
  - 不过滤当前 `active` 状态

## UI Constraints

- 页面主视图固定为“时间窗切换 + 当前时间窗双榜”，默认选中最近 `24h`。
- 桌面端当前时间窗内部双栏并排展示两张榜；移动端改为纵向堆叠，不允许横向滚动。
- 图表使用成熟第三方图表库 `Apache ECharts + echarts-for-react` 的标准横向柱状图配置，不允许业务侧自绘 canvas 装饰。
- 每张榜作为单一 chart surface 渲染；身份信息不再拆成图外独立列表。
- 每条 bar 仅展示一份用户身份：`rank + avatar + 单一显示名`，不再重复展示 secondary identity。
- 页面需显式展示实时连接状态、最后更新时间与断连后的重试入口；SSE 退化时允许继续展示最近一次成功快照。
- 为避免核心排行信息只存在于 canvas，页面必须补充同榜单同内容的语义 DOM fallback，供辅助技术读取。
- 头像 URL 只使用服务端安全解析后的公开 `avatarUrl`；无头像或加载失败必须回退为稳定 mock 头像，不得整屏退化为字母圆牌。

## Acceptance

- `/admin/rankings` 作为独立 admin 模块出现在导航中，不影响 `/admin/users/usage` 与 `/admin/tokens/leaderboard`。
- HTTP 与 SSE `snapshot` payload 结构完全一致。
- 24h / 7d / 30d 三个窗口都返回 `primarySuccessTop` 与 `businessCreditsTop` 两榜，且每榜最多 20 行。
- 排序固定为 `value desc, userId asc`。
- SSE 建连后立即收到首帧，并每 10 秒持续推送。
- Storybook 与真实 admin 页面都能证明“时间窗切换 + 双榜”的桌面双栏与移动端堆叠布局稳定。

## Visual Evidence

- Storybook: `Admin/Pages/UserRankings`
- Story variants:
  - `Default`
  - `EmptyState`
  - `ErrorState`
  - `ConnectingState`
  - `Mobile`
- Live page:
  - `/admin/rankings`

### Storybook Default

- source_type: storybook_canvas
  story_id_or_title: `Admin/Pages/UserRankings`
  state: `Default desktop`
  evidence_note: 默认 24h 视图展示 ECharts 单窗口双榜、TOP20 条形排行、真实头像优先与稳定 mock avatar fallback。
  PR: include

![Storybook default desktop](./assets/storybook-rankings-desktop.png)

### Storybook Mobile

- source_type: storybook_canvas
  story_id_or_title: `Admin/Pages/UserRankings`
  state: `Mobile`
  evidence_note: 移动端保持时间窗切换与双榜纵向堆叠，不出现横向滚动。
  PR: include

![Storybook mobile](./assets/storybook-rankings-mobile.png)

### Live Page Route

- current local capture uses empty live data, so it proves the final layout, status row, empty state, and mobile stacking rather than populated bars.

![Live rankings desktop](./assets/live-rankings-desktop.png)

![Live rankings mobile](./assets/live-rankings-mobile.png)
