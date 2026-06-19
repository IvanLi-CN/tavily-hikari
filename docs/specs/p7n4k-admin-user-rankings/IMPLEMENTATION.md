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
- 前端最终采用 `chart.js + react-chartjs-2` 横向柱状图；每张榜为单一 chart surface，用户身份以 `rank + avatar fallback + 单一显示名` 形式内嵌于 chart，不再拆出图外重复身份列，也不再显示 secondary identity。
- Storybook `Admin/Pages/UserRankings` 已覆盖 `Default`、`EmptyState`、`ErrorState`，默认示例数据为完整 `TOP20`。

## Validation

- `cargo test`
- `cargo clippy -- -D warnings`
- `cd web && bun test src/admin/AdminUserRankingsPage.stories.test.tsx`
- `cd web && bun run build`
- `cd web && bun run build-storybook`

## Remaining Gaps

- 当前 worktree 还未进入 PR 创建 / merge-ready 收口，后续需要继续共享 Step 5。
- Live admin 视觉证据仍沿用较早一轮截图；若后续在 PR 阶段要求“当前实现完全同款”的 live 页面证据，需要在新预览服务上补一次更新截图。

## References

- `./SPEC.md`
- `./HISTORY.md`
