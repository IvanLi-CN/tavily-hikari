# 管理员 Passkey 登录演进历史（#tx26z）

> 这里记录会影响 Agent 理解“为什么一步步变成现在这样”的关键演进；单次任务流水账不放这里，规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 新增本 spec，用于替代对 ForwardAuth 用户头的公网管理员信任边界，并将 passkey 设为生产主登录方向。
- 采用 WebAuthn challenge 服务端持久化、credential counter 更新和 DB-backed passkey session，避免重启后管理员 session 全部丢失。
- reset URL 仅由本地 CLI 生成，注册成功后先消费 reset token，再撤销旧 passkey 与旧 passkey session，降低 token 重放和并发注册的风险。
- 管理员密码设置、passkey reset token、credential 与 session 都属于管理员控制面状态；它们需要 HA 同步，写入路径也必须拒绝 standby 本地写，避免 failover 后凭据状态丢失或分叉。
- 单独切换管理员登录 TOTP 要求不能把未持久化的环境变量口令改写成 disabled 状态；持久化设置恢复时只在明确存在 password hash 或 disabled marker 时覆盖口令来源。

## Key Reasons / Replacements

- ForwardAuth 用户头配置错误时会形成可伪造的管理员边界，不适合作为当前公网部署的默认安全方案。
- 内置单密码登录可以作为 break-glass，但不具备 passkey 的设备绑定与抗钓鱼属性。
- reset URL 必须由本地 CLI 生成，避免远程公开重置入口扩大攻击面。
- Challenge 不纳入 HA 同步，因为它是短 TTL ceremony 状态；管理员密码设置、credential、reset token 和 passkey session 纳入控制面同步，支持 standby 接管后的恢复。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
