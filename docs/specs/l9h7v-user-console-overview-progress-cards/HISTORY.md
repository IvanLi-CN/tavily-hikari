# 用户控制台全宽概览与实时进度卡改版 History

- 2026-06-11: 创建 follow-up spec，冻结 `/console` 全宽 shell、landing 3+4 卡分层、current-period progress 语义、landing overview HTTP/SSE 合同，以及与 `45squ` / `2nx74` / `tz9kq` / `q7u4m` / `3zky1` 的关系。
- 2026-06-11: 完成 landing overview HTTP/SSE、UTC-day rollup、全宽 shell 与 3+4 概览卡实现，并补齐 Storybook 宽屏 / 390px 视觉证据。
- 2026-06-27: 将用户控制台与 token detail 的业务额度文案统一为“每小时业务请求次数限额 / 每日积分限额 / 每月积分限额”，新增 hover/focus/tap 字段解释，并让 `businessCalls1h` 成为小时业务次数的唯一对外语义。
