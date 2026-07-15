# 上游身份隐私与分段积分对账演进历史（#3s7ku）

> 这里记录影响长期理解的关键演进；规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-07-14: 默认模式锁定为 `accessToken`，不支持原始项目派生或按项目查询用量。
- 2026-07-14: 业务日采用服务器现有业务时区，并固定三个窗口 `00-11 / 11-22 / 22-24`。
- 2026-07-14: 结算只执行一次；Research 最长等待 24 小时，超时后 degraded 结算且不自动复核。
- 2026-07-15: 去掉 API/MCP rebalance 百分比放量控件；新流量是否全量走 rebalance 只由两个开关决定，兼容百分比字段统一归一化为 `0|100`。

## Key Reasons / Replacements

- 本 spec 替代 `34pgu` 的固定项目 UA 条款，UA 改为管理员配置且空值省略。
- 本 spec 替代 `m30lm` 与 `cp8s9` 的 `X-Project-ID` 原样上送条款，但保留本地 routing subject/亲和语义。
- 本 spec 替代 `xm3dh` 中 Rebalance MCP HTTP 固定 UA 的条款；该路径与 REST API 一样不发送 UA。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
