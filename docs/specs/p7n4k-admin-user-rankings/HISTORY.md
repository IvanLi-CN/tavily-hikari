# Admin 用户排行演进历史（#p7n4k）

> 这里记录会影响后续理解“为什么实现收敛到当前形态”的关键演进；规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-06-19: 新增独立 `/admin/rankings` 模块，锁定三个滚动时间窗（`24h / 7d / 30d`）与双榜指标（`primary_success`、`business_credits`）。
- 2026-06-19: 后端合同固定为同形 HTTP 快照 + 独立 SSE；排行 row 固定返回 `rank`、`value`、`userId/displayName/username/avatarUrl`。
- 2026-06-19: 数据路径确定为用户级 rollup + partial bucket 补扫 + 10 秒 snapshot cache/singleflight，避免实时榜单每轮回扫 30 天原始日志。
- 2026-06-19: 前端从“图外身份列 + 图内柱条”的拆分方案收敛到“单一 chart surface”，以更接近常规排行榜图表的呈现方式内嵌 rank、头像 fallback 与单一显示名。
- 2026-06-19: 根据验收反馈，去除 secondary identity 与重复用户信息，Storybook 默认数据扩展为完整 `TOP20`，确保视觉证据与接口合同一致。
- 2026-06-19: 根据 critique 收口，页面从“六张榜同时平铺”调整为“时间窗切换 + 当前窗口双榜”，并新增实时状态条、断连重试与 DOM 语义 fallback。
- 2026-06-19: 根据最新验收反馈，头像策略从“缺失时首字母圆牌”收敛为“真实头像优先，缺失或加载失败时稳定 mock avatar”，避免整榜视觉过假；横轴同步取消共享固定刻度，改为每榜按当前最大值自适应。

## Key Reasons / Replacements

- 用户明确要求“只允许做 charts”，因此最终实现不再保留图表外的独立身份列或重复文本块。
- 用户明确拒绝一个 row 上展示多份身份信息，因此昵称/用户名分层显示被收敛为单一显示名。
- 用户要求 `TOP20` 必须真实可见，因此 Storybook 默认场景必须使用完整 20 行 mock 数据，而不是截断示例。
- critique 指出六榜同时展示的扫描负担过高，且 canvas-only 信息层不利于可访问性，因此最终切到单时间窗主视图，并为榜单补充语义 DOM 同步层。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
