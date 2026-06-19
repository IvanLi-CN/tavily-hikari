# Admin 用户排行实现状态（#p7n4k）

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与验证事实。

## Current Status

- Implementation: 已实现（本地验证通过）
- Lifecycle: active
- Catalog note: admin rolling user rankings

## Coverage / rollout summary

- 新增独立 `/admin/rankings` 管理台模块，固定展示最近 `24h / 7d / 30d` 三个滚动时间窗。
- 每个时间窗固定两张榜：`成功主要调用` 与 `积分消耗`，每榜最多 `TOP20` 用户，排序为 `value desc, userId asc`。
- 后端新增 `GET /api/users/rankings` 与 `GET /api/users/rankings/events`；HTTP 与 SSE `snapshot` payload 同形，SSE 建连后立即首帧并按 10 秒节奏推送。
- 数据路径扩展为用户级 `primary_success` rollup，并复用 `business_credits` 统计；查询使用 rollup 聚合加 partial bucket 补扫，避免每次刷新回扫 30 天原始日志。
- 页面首屏走 HTTP 快照，后续通过独立 SSE 实时更新；路由与导航作为独立 admin 模块接入，不影响历史 `/admin/users/usage` 与 `/admin/tokens/leaderboard`。
- 前端最终采用 `Apache ECharts + echarts-for-react` 横向柱状图；每张榜为单一 chart surface，用户身份以 `rank + avatar + 单一显示名` 形式内嵌于 chart，不再拆出图外重复身份列，也不再显示 secondary identity。
- 页面主视图已收敛为时间窗切换器驱动的当前窗口双榜，不再把 24h / 7d / 30d 六张榜同时平铺。
- 每张榜的 X 轴按当前窗口当前榜单的最大值自适应，第一名直接占满当前榜单的有效值域，不再固定共享刻度。
- 排行页优先显示服务端返回的真实头像；当 `avatarUrl` 缺失或加载失败时，前端会生成稳定的 mock avatar，避免整榜退化成字母圆牌。
- 排行页已补充 DOM 语义 fallback、实时连接状态、最后更新时间、断连降级提示与显式重试入口。
- Storybook `Admin/Pages/UserRankings` 已覆盖 `Default`、`EmptyState`、`ErrorState`、`ConnectingState`、`Mobile`，默认示例数据为完整 `TOP20`。

## Validation

- `cargo test`
- `cargo clippy -- -D warnings`
- `cd web && bun test src/admin/AdminUserRankingsPage.stories.test.tsx`
- `cd web && bun run build`
- `cd web && bun run build-storybook`
- `node scripts/capture-rankings-evidence.mjs`

## Remaining Gaps

- 当前 worktree 还未进入 PR 创建 / merge-ready 收口，后续需要继续共享 Step 5。
- live 页面当前本地数据库无排行数据，因此 live 截图证明的是最新布局、状态条、空态和移动端堆叠；满数据视觉仍以 Storybook mock 证据为准。

## References

- `./SPEC.md`
- `./HISTORY.md`
