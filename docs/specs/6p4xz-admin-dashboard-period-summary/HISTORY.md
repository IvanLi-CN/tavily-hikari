# Admin 仪表盘期间摘要演进历史（#6p4xz）

> 这里记录会影响 Agent 理解“为什么一步步变成现在这样”的关键演进；单次任务流水账不放这里，规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-04-07: 新增期间摘要 topic spec，锁定管理端 dashboard 顶部摘要区拆分为 `今日`、`本月`、`站点当前状态` 三块。
- 2026-04-07: 将 `上游 Key 耗尽` 从请求级耗尽次数改为窗口内系统自动标记 exhausted 的唯一上游 Key 数，避免与请求日志详情口径混淆。
- 2026-06-09: 修正 `yesterday` 对比窗口边界。旧实现把 `yesterday_end` 设为今天 0 点，实际统计了昨天整天；正确口径应为昨日同一日内进度，且窗口时长与 `today` 一致。

## Key Reasons / Replacements

- “较昨日同刻”是运营对齐窗口，而不是昨天整天基线；窗口边界按同一日内 elapsed duration 对齐，避免 DST 切换日出现一小时偏差。
- Dashboard 卡片背景图、主值和 delta 必须使用同一组窗口边界；边界由后端 `summaryWindows` 返回，前端不自行推导浏览器时区窗口。
- 期间摘要继续使用 dashboard rollup buckets 聚合，修正窗口边界不能回退到扫描全量请求日志。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
