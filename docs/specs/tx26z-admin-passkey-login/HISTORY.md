# 管理员 Passkey 登录演进历史（#tx26z）

> 这里记录会影响 Agent 理解“为什么一步步变成现在这样”的关键演进；单次任务流水账不放这里，规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 新增本 spec，用于替代对 ForwardAuth 用户头的公网管理员信任边界，并将 passkey 设为生产主登录方向。
- 采用 WebAuthn challenge 服务端持久化、credential counter 更新和 DB-backed passkey session，避免重启后管理员 session 全部丢失。
- reset URL 仅由本地 CLI 生成；注册成功的 reset token 消费、新 passkey 写入、旧 passkey 与旧 session 撤销必须事务提交，降低 token 重放、并发注册和半完成恢复的风险。
- 管理员密码设置、passkey reset token、credential 与 session 都属于管理员控制面状态；它们需要 HA 同步，写入路径也必须拒绝 standby 本地写，避免 failover 后凭据状态丢失或分叉。
- 单独切换管理员登录 TOTP 要求不能把未持久化的环境变量口令改写成 disabled 状态；持久化设置恢复时只在明确存在 password hash 或 disabled marker 时覆盖口令来源。
- 管理员登录 TOTP 是认证因子，不是充值功能的附属开关；绑定、禁用和状态展示只依赖管理员权限、加密密钥与 dev-open 限制，不能要求启用充值功能。
- 解绑管理员 TOTP 时必须同时关闭“管理员登录要求 TOTP”，避免留下无 TOTP secret 却仍要求登录 TOTP 的锁死状态。
- HA 控制面应用管理员密码设置后必须刷新运行中的内存认证态；否则 standby failover 可能继续接受旧启动口令或错过新设置。
- 管理员登录 TOTP 要求与 TOTP secret 必须作为同一控制面事实同步；secret ciphertext、nonce 与 enabled timestamp 纳入 HA meta allowlist，防止节点只收到 requirement 而无法校验。
- ForwardAuth 不再作为新部署默认管理员边界，但既有完整 header/admin-value 配置需要继续自动启用；示例与文档显式写出 `ADMIN_AUTH_FORWARD_ENABLED=true`，兼顾兼容与新配置可读性。
- Passkey RP 默认推导优先使用浏览器访问的 `EDGEONE_DOMAIN`，没有 EdgeOne 公网域名时才退到 `NODE_PUBLIC_HOST`；非标准公网 origin 仍应显式配置 `ADMIN_PASSKEY_RP_ORIGIN`。
- 显式关闭的管理员认证开关优先级必须高于兼容恢复：`ADMIN_AUTH_FORWARD_ENABLED=false` 不应被 legacy header 配置覆盖，`ADMIN_AUTH_BUILTIN_ENABLED=false` 不应被已持久化的旧密码 hash 重新启用。
- Passkey challenge 虽然是短 TTL ceremony 状态，但仍必须随 HA 控制面同步；否则 start 命中节点 A、finish 命中节点 B 或中途 failover 时会误判 challenge 不存在。
- reset-token recovery 和开启登录 TOTP 都属于安全边界提升动作，必须撤销既有管理员 session；否则旧 cookie 会在有效期内绕过新 passkey/TOTP 状态。
- 内置密码的持久化 hash 必须能独立支撑重启恢复，但显式启动禁用时旧 hash 不得重新启用；删除密码或撤销 passkey 的 fallback 判断必须以运行时可用的登录方式为准，不能只看数据库里是否残留 passkey 行或 password hash。
- `/login` 的登录方式入口不能完全依赖 `/api/profile` 成功返回；profile 临时失败时仍要保留可尝试的 passkey/password 入口，避免 passkey-only 部署被前端 bootstrap 锁死。

## Key Reasons / Replacements

- ForwardAuth 用户头配置错误时会形成可伪造的管理员边界，不适合作为当前公网部署的默认安全方案。
- 内置单密码登录可以作为 break-glass，但不具备 passkey 的设备绑定与抗钓鱼属性。
- reset URL 必须由本地 CLI 生成，避免远程公开重置入口扩大攻击面。
- Challenge 也纳入 HA 同步；短 TTL 只影响保留周期，不改变跨节点 WebAuthn ceremony 对同一 challenge 状态的依赖。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
