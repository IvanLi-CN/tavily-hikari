# 账户级配额迁移与用户控制台实现状态（#45squ）

## Current Status

- Implementation: 已实现
- Lifecycle: active

## Coverage

- `/console` 提供账户概览、可见充值卡、Token 列表和 token 详情。
- `/console/billing` 保留权益构成、资费规则、自然月时间线、订单明细与购买入口。
- 概览与 billing 页复用同一份充值配置、报价、订单和创建订单状态。

## References

- `./SPEC.md`
- `../5vxmz-linuxdo-credit-recharge/SPEC.md`
