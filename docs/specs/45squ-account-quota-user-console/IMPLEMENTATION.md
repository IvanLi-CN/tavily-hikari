# 账户级配额迁移与用户控制台实现状态（#45squ）

## Current Status

- Implementation: 已实现
- Lifecycle: active

## Coverage

- `/console` 提供账户概览、可见充值卡、Token 列表和 token 详情。
- `/console/billing` 保留权益构成、资费规则、自然月时间线、订单明细与购买入口。
- 概览与 billing 页复用同一份充值配置、报价、订单和创建订单状态。
- Billing 时间线首次布局只同步可见窗口，不覆盖当前月的默认选择；桌面端继续展示相邻三个月份卡片。
- Billing 时间线卡片与导航复用共享 Clay card、pressed 与 button 阴影；当前月只以既有 primary/secondary 材质强化，不再使用孤立色相的悬浮阴影。
- 当前月份选中态使用 Clay pressed 凹面层级而非凸起按钮层级，紫色仅用于边界与状态文字。

## References

- `./SPEC.md`
- `../5vxmz-linuxdo-credit-recharge/SPEC.md`
