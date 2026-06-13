# 用户控制台全宽概览与实时进度卡改版 Implementation

## Milestones

- [x] M1: follow-up spec、README 索引与 landing overview 合同冻结
- [x] M2: 后端 current-period overview HTTP / SSE 与 rollup 扩展落地
- [x] M3: 前端全宽 shell、3 张汇总卡、4 张趋势卡与 SSE 降级刷新落地
- [x] M4: Storybook、视觉证据、验证与 review 收敛完成

## Notes

- landing overview 使用专用合同，不改旧 `GET /api/user/dashboard`。
- token detail 维持既有 summary + logs + SSE 结构，只吃新 shell。
- `requestRate` 直接读取内存 rolling `5m` subject 时间戳构造当前窗口序列；`quotaHourly`、`quotaDaily`、`quotaMonthly` 继续走轻量 rollup，并把未来 bucket 保留为 `null`。
- landing SSE 只推 `snapshot` / `ping` 两类事件，服务端最多每 5 秒发一帧；前端仅对 landing overview 做单接口降级刷新。
