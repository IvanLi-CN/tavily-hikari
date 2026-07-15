# 账户级配额迁移与用户控制台实现状态（#45squ）

## Current Status

- Implementation: 已实现
- Lifecycle: active

## Coverage

- `/console` 提供账户概览、可见充值卡、Token 列表和 token 详情。
- `/console/billing` 保留权益构成、资费规则、自然月时间线、订单明细与购买入口。
- `/console/setup` 集中承载客户端接入指南，支持从 query 参数选择当前 Token 与当前 guide 面板；若缺少 `token`，则默认选择首个启用 Token。
- Token 详情页通过标题区“使用方法”动作跳转到 setup 页面；首页与详情页不再重复嵌入接入指南。
- 概览与 billing 页复用同一份充值配置、报价、订单和创建订单状态。
- Billing 时间线首次布局只同步可见窗口，不覆盖当前月的默认选择；桌面端继续展示相邻三个月份卡片。
- Billing 时间线卡片与导航复用共享 Clay card、pressed 与 button 阴影；当前月只以既有 primary/secondary 材质强化，不再使用孤立色相的悬浮阴影。
- 当前月份选中态使用轻微凸起的 Clay button 层级，紫色仅用于边界、状态文字与选中层次。
- Billing 时间线的月份导航使用 Lucide chevron 图标，并保留禁用态、辅助文本与鼠标悬停反馈。

## References

- `./SPEC.md`
- `../5vxmz-linuxdo-credit-recharge/SPEC.md`
