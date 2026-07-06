# 管理员 Passkey 登录实现状态（#tx26z）

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实，避免这些细节散落到 PR / Git 历史里。

## Current Status

- Implementation: implemented in the active worktree; hinet-lam is still pinned to the temporary `passkey-local` build until a formal release includes this topic
- Lifecycle: active
- Catalog note: Passkey admin login and CLI reset URL.

## Coverage / rollout summary

- 后端新增 WebAuthn passkey authentication / reset registration API，并将 `hikari_admin_passkey_session` 接入管理员鉴权链。
- SQLite 新增管理员 passkey credential、reset token、challenge 与 passkey session 表；admin password settings、credential、reset token、challenge 与 session 纳入 HA 控制面同步基线，支持 start/finish 请求跨节点或中途 failover。
- reset-token recovery 会撤销旧 passkey 凭据、旧 passkey session 和既有内置密码 session；开启“管理员登录要求 TOTP”会撤销既有 passkey session 与内置密码 session，确保新认证要求立即生效。
- passkey 登录在 WebAuthn assertion 成功后先校验登录 TOTP，再更新 credential counter / last-used 和创建 session，避免失败登录污染审计状态。
- CLI 新增 `tavily-hikari admin passkey reset-url --base-url <url>`，直接写入目标 SQLite DB 并输出一次性 reset/enroll URL。
- `/login` 前端新增 passkey 登录按钮与 reset URL 注册流程；reset 注册完成后返回登录页并提示使用新 passkey 登录；`/api/profile` 新增 `passkeyAuthEnabled` capability。
- 内置密码登录保持显式启用的 break-glass 路径；本实现没有恢复 Remote-Email/ForwardAuth 作为生产主登录方案。
- 内置密码可从环境变量或持久化 `admin_password_settings` 恢复；只有持久化 password hash 存在且未禁用时，启动才允许不提供环境变量/显式 hash；删除内置密码或撤销 passkey 时，只有运行时确实可用的 passkey、内置密码或外部管理员登录才允许计为 fallback。
- 当启动配置禁用内置密码登录时，管理员密码更新接口返回冲突错误，不写入一个当前进程无法使用的假成功 password hash。
- `/login` 在 `/api/profile` 临时不可用时仍保留 passkey 与密码尝试入口，避免 passkey-only 部署因 profile bootstrap 失败而隐藏可用登录方式。
- `/api/profile` 对注册开关和 passkey credential 这类可降级字段采用失败即保守关闭，不影响 `adminLoginTotpRequired` 返回，避免 TOTP-required 登录页被非关键元数据故障锁死。
- HA 控制面同步 `admin_password_settings` 后会刷新运行中的内置管理员认证态；当同步结果开启登录 TOTP 时，既有 builtin admin session 会被撤销，避免 standby failover 后沿用旧认证策略。
- passkey 管理员 session 会参与维护操作 actor 归因，审计记录使用稳定的 `admin-passkey:<credential-prefix>` 显示名。
- hinet-lam standby 当前运行
  `/opt/tavily-hikari-standby/releases/20260703113452-passkey-local`；本地 `/health`
  返回 `ok`，`/api/version` 返回 backend `passkey-local` / frontend `0.1.0`，Passkey
  API 路由存在。
- GitHub Release `v0.72.2` 已验证不包含本 topic 的新增 Passkey store/spec/安全设置页文件；
  直接升级到该 release 会让 `/api/admin/passkey/authentication/start` 与 `/login` 回到 404，
  因此不能作为本 topic 的完成态。

## Remaining Gaps

- 浏览器 passkey ceremony 需要在真实 HTTPS origin 上完成最终人工验收；本地自动化覆盖 store/CLI/build/Storybook，不会触发真实安全钥匙或平台认证器。
- 需要先将本 worktree 的 Passkey 实现同步到最新 `main`、通过验证并发布新的正式 release，
  然后才能把 hinet-lam 从 `passkey-local` 升级到正式版本而不丢失 Passkey 能力。
- hinet-lam 仍只有约 `1 GiB` RAM 且未配置 swap；当前临时构建运行与 HA 增量同步正常，但后续初始全量 baseline 或故障恢复前仍应补 swap 或提高内存余量。

## Related Changes

- `src/server/handlers/admin_auth.rs`
- `src/store/key_store_admin_passkeys.rs`
- `src/main.rs`
- `web/src/pages/AdminLogin.tsx`
- `web/src/pages/AdminLogin.stories.tsx`

## References

- `./SPEC.md`
- `./HISTORY.md`
