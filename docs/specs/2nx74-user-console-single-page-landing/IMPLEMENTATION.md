# 用户控制台单页合并实现状态（#2nx74）

## Current Status

- Implementation: 已实现
- Lifecycle: active

## Coverage

- `/console` 保持账户概览、可见充值卡与 Token 列表的单页结构。
- 充值可见时，桌面端复用既有 `has-rail` 双栏布局；小屏保持单列，不创建空白右栏。
- `UserConsole` Storybook 默认、关闭和隐藏充值状态覆盖完整卡片、不可用卡片与无右栏状态。

## References

- `./SPEC.md`
- `../5vxmz-linuxdo-credit-recharge/SPEC.md`
