# 管理员 Passkey 登录实现状态（#tx26z）

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实，避免这些细节散落到 PR / Git 历史里。

## Current Status

- Implementation: implemented in the active worktree; hinet-lam is still pinned to the temporary `passkey-local` build until a formal release includes this topic
- Lifecycle: active
- Catalog note: Passkey admin login and CLI reset URL.

## Coverage / rollout summary

- 后端新增 WebAuthn passkey authentication / reset registration API，并将 `hikari_admin_passkey_session` 接入管理员鉴权链。
- SQLite 新增管理员 passkey credential、reset token、challenge 与 passkey session 表；admin password settings、credential、reset token 与 session 纳入 HA 控制面同步基线，challenge 保持短期本地状态。
- CLI 新增 `tavily-hikari admin passkey reset-url --base-url <url>`，直接写入目标 SQLite DB 并输出一次性 reset/enroll URL。
- `/login` 前端新增 passkey 登录按钮与 reset URL 注册流程；`/api/profile` 新增 `passkeyAuthEnabled` capability。
- 内置密码登录保持显式启用的 break-glass 路径；本实现没有恢复 Remote-Email/ForwardAuth 作为生产主登录方案。
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
