# Linux.do Credit 额度充值演进历史（#5vxmz）

> 这里记录会影响 Agent 理解“为什么一步步变成现在这样”的关键演进；单次任务流水账不放这里，规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 2026-05-26: 新增本 topic spec，锁定官方 LDC 创建订单、月度额度按服务器本地自然月展开、充值权益只叠加账户月额度。
- 2026-05-27: 增加管理端系统设置开关。充值总开关负责展示与创建订单 gate；非管理员开放开关作为调试保护，允许管理员先自测充值链路再开放给普通用户。

## Key Reasons / Replacements

- 新用户零基线额度和管理员手动加额无法满足用户自助购买；需要支付订单、权益和 quota 解析形成稳定闭环。
- Linux.do Credit 官方 LDC 创建订单要求 Ed25519 签名；异步通知文档未提供平台公钥，因此先按公共通知签名规则与 `Client Secret` 校验。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
