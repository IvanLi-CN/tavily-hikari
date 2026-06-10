# Admin 仪表盘期间摘要实现状态（#6p4xz）

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实，避免这些细节散落到 PR / Git 历史里。

## Current Status

- Implementation: 已实现
- Lifecycle: active
- Catalog note: Admin dashboard period summary windows

## Coverage / rollout summary

- 管理端 dashboard 通过 `GET /api/dashboard/overview` 同时读取全站即时摘要、`summaryWindows`、小时请求窗口、站点状态、风险列表与近期事件。
- `summaryWindows.today`、`summaryWindows.yesterday`、`summaryWindows.month` 均由 dashboard request rollup buckets 聚合，避免回退到全量 `request_logs` 扫描。
- `yesterday` 对比窗口使用服务端本地昨日 0 点到昨日同一日内进度的半开区间，窗口时长与 `today` 一致；昨日同刻之后的数据不会进入今日卡片的“较昨日同刻”比较。
- 前端今日卡片主值和比较值继续读取 `summaryWindows`，背景图比较序列读取同一响应里的 `yesterday_start/yesterday_end` 边界，因此修正后主值、delta 与背景图窗口保持同一口径。
- 前端今日明细网格在桌面端固定为两列三行，并把主块剩余高度平均分配到三行小卡；占比 / 新增说明与 delta badge 作为右下角信息组展示，marker 使用轻量半透明样式减少对背景趋势线的遮挡。

## Remaining Gaps

- None

## Related Changes

- None

## References

- `./SPEC.md`
- `./HISTORY.md`
