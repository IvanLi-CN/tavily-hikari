# Admin 用户排行实现状态（#p7n4k）

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与验证事实。

## Current Status

- Implementation: 已实现（本地验证通过）
- Lifecycle: active
- Catalog note: admin rolling user rankings

## Coverage / rollout summary

- 新增独立 `/admin/rankings` 管理台模块，以 `24h / 7d / 30d / 主要调用 / 积分 / IP` 六个单选 tab 组织排行内容区，并把当前视角写入单一 `tab` 查询参数。
- 每个时间窗固定三张榜：`成功主要调用`、`积分消耗` 与 `IP`，每榜最多 `TOP20` 用户，排序为 `value desc, userId asc`。
- 后端新增 `GET /api/users/rankings` 与 `GET /api/users/rankings/events`；HTTP 与 SSE `snapshot` payload 同形，SSE 建连后立即首帧并按 10 秒节奏推送。
- 数据路径扩展为用户级 `primary_success` rollup、`business_credits` 聚合与 `request_logs` 唯一 `client_ip` 统计；查询继续使用 rollup 聚合加 partial bucket 补扫，避免每次刷新回扫 30 天原始日志。
- 页面首屏走 HTTP 快照，后续通过独立 SSE 实时更新；路由与导航作为独立 admin 模块接入，不影响历史 `/admin/users/usage` 与 `/admin/tokens/leaderboard`。
- 排行页当前 tab 由 `AdminDashboardRuntime` 解析/规范化 URL 并受控下发；缺失或非法 `tab` 会统一归一到 `last24h`，并同步修正地址栏。
- 前端最终采用 `Apache ECharts + echarts-for-react` 的 `custom series` 横向排行图；每张榜为单一 chart surface，用户身份以 `rank + avatar + 单一显示名` 形式内嵌于 chart，不再拆出图外重复身份列，也不再显示 secondary identity。
- 旧的 `.admin-ranking-chart-overlay / .admin-ranking-row-label` DOM 覆盖层已移除；当前页面只保留三张 chart canvas 与语义 DOM fallback，不再混用第二套图外身份层。
- 页面主视图已收敛为六个单选 tab 驱动的单一三榜内容区：时间范围 tab 输出 `主要调用 → 积分 → IP`，指标 tab 输出 `最近 24 小时 → 最近 7 天 → 最近 30 天`，两组映射都固定且互斥。
- 每张榜的 X 轴按当前窗口当前榜单的最大值自适应，第一名直接占满当前榜单的有效值域，不再固定共享刻度。
- 排行页优先显示服务端返回的真实头像；当 `avatarUrl` 缺失或加载失败时，前端会生成稳定的 mock avatar，避免整榜退化成字母圆牌。
- 排行页已补充 DOM 语义 fallback、实时连接状态、最后更新时间、断连降级提示与显式重试入口。
- 排行页已补齐行级交互层：任一排行项支持 click 跳转用户详情，hover/focus 时当前三张榜内同用户联动高亮；离开时立即清除。
- runtime 保持最近一次 `rankingsSnapshot`，因此 Tab 切换、浏览器前进后退、从用户详情返回排行时都会先复用现有数据，只在后台 refresh / reconnect，不会回到“从 0 开始”的 blocking skeleton。
- 空态已改为等高内容舞台，不再使用单个普通提示框；三张卡在无数据时保持统一高度。
- loading 期间已改为仅榜单内容区骨架屏；页头状态与六个单选 tab 保持真实文案，不再只显示纯文本 loading。
- 之前出现“两种实现”的根因已确认是证据层级混用：一部分截图来自 story 内容模块，一部分来自真实页面。当前 owner-facing 排行证据已统一收敛为 `web demo` 路由截图；Storybook 仅保留开发验证用途。

## Validation

- `cargo test`
- `cargo clippy -- -D warnings`
- `cd web && bun test src/admin/routes.test.ts`
- `cd web && bun test src/admin/AdminDashboardRuntime.route-switch.test.tsx`
- `cd web && bun test src/admin/AdminUserRankingsPage.render.test.tsx`
- `cd web && bun test src/admin/AdminUserRankingsPage.stories.test.tsx`
- `cd web && bun run build`
- `cd web && bun run build-storybook`
- `LIVE_BASE_URL=http://127.0.0.1:55174 node scripts/capture-rankings-evidence.mjs`

## Visual evidence status

- 视觉证据目标源=`ui_demo`
- 视觉证据=`存在`
- 空白裁剪=`无需裁剪（四张图均因 ambiguous_border fail-open 保持原图）`
- 聊天回图=`已准备`
- 证据落盘=`已落盘`
- 证据绑定sha=`当前 worktree HEAD`

## Remaining Gaps

- 当前 worktree 还未进入 PR 创建 / merge-ready 收口，后续需要继续共享 Step 5。
- owner-facing 排行证据已收敛为 `web demo` 固定状态：桌面浅色、有数据移动端、桌面暗色；旧的 Storybook / live / chrome 中间截图链已清理。

## References

- `./SPEC.md`
- `./HISTORY.md`
