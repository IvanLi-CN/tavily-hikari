# 管理员 Passkey 登录（#tx26z）

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，关键演进原因见 `./HISTORY.md`。

## 背景 / 问题陈述

- 管理员登录历史上支持 ForwardAuth 与内置单密码登录。
- ForwardAuth 依赖反代注入用户标识头，不适合作为公网后台的唯一信任边界。
- 内置单密码没有设备绑定和抗钓鱼能力，也不适合作为长期生产主路径。
- 项目需要一个由管理员本人设备持有私钥的登录方式，并保留本地运维可恢复能力，避免锁死后台。

## 目标 / 非目标

### Goals

- 支持管理员使用 passkey 登录 `/admin`。
- 支持通过本地 CLI 生成一次性 passkey reset/enroll URL。
- reset URL 成功注册新 passkey 后必须单次消费，且可撤销旧 passkey 与旧 admin session。
- passkey challenge、credential、reset token、admin session 必须服务端持久化。
- 生产认证不得依赖 `FORWARD_AUTH_HEADER` 这类可由错误反代配置伪造的用户头。

### Non-goals

- 不实现多管理员、多角色或用户名密码账号体系。
- 不把 ForwardAuth 重新作为公网管理员登录方案。
- 不提供远程公开 API 来生成 reset URL。
- 不在本主题中实现 LinuxDo OAuth 管理员登录。

## 范围（Scope）

### In scope

- Rust 后端 WebAuthn/passkey 登录和注册 API。
- SQLite schema 与 store API。
- admin session 持久化和 cookie 鉴权接入。
- CLI reset URL 生成工具。
- 前端 `/login` passkey 登录和 reset enrollment UI。
- Storybook 状态入口与视觉证据。

### Out of scope

- EdgeOne/Zero Trust 配置自动化。
- 多账号 passkey 管理 UI。
- 用户侧 passkey 登录。

## 需求（Requirements）

### MUST

- MUST 使用 WebAuthn/passkey 作为管理员生产主登录能力。
- MUST 通过显式 RP ID 与 origin 配置约束 passkey，默认从公开站点配置推导。
- MUST 将 WebAuthn registration/authentication state 存在服务端，并设置 TTL。
- MUST 持久化 credential、credential counter、reset token 与 admin session。
- MUST 在认证成功后按 WebAuthn 结果更新 credential counter。
- MUST 让 reset token 单次可用、过期失效、成功后不可重放。
- MUST 提供 CLI 生成 reset URL，CLI 需要直接访问目标 SQLite DB。
- MUST 保持内置密码登录为显式启用的 break-glass 能力，不作为 passkey 必需依赖。

### SHOULD

- SHOULD 在 reset 注册成功后撤销旧 passkey 与旧 admin session。
- SHOULD 提供清晰的 profile capability 字段，例如 `passkeyAuthEnabled`。
- SHOULD 记录 passkey 注册、登录和 reset 消费的结构化日志，避免写入密钥材料。
- SHOULD 将 passkey 相关 HA 同步纳入控制面数据，避免 standby 切换后锁死。

### COULD

- COULD 后续增加 passkey 管理 UI，用于查看/撤销单个 credential。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 管理员访问 `/login` 时，前端请求 `/api/admin/passkey/authentication/start` 获取 challenge，再调用浏览器 `navigator.credentials.get`，最后提交 `/api/admin/passkey/authentication/finish`。
- 后端验证成功后写入持久化 admin session，并设置 `hikari_admin_passkey_session` HttpOnly cookie。
- 运维人员在服务器上运行 CLI reset-url 子命令，生成带 token 的 URL。
- 管理员打开 reset URL 后，前端请求 registration challenge，浏览器创建 passkey，后端验证并保存 credential，消费 reset token。
- reset 注册成功后，默认撤销旧 passkey 与旧 admin session，并要求管理员使用新 passkey 重新登录。

### Edge cases / errors

- 无 passkey credential 时，普通 passkey 登录 start 返回不可用错误；reset URL 是 bootstrap 入口。
- reset token 过期、已消费或不存在时，前端显示不可继续注册。
- WebAuthn origin/RP ID 不匹配时必须拒绝。
- credential counter 异常时必须拒绝认证，并记录可审计日志。
- 服务重启不能让已持久化的 admin session 全部丢失。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                                          | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）                        |
| ----------------------------------------------------- | ------------ | ------------- | -------------- | ------------------------ | --------------- | ------------------- | ------------------------------------ |
| `/api/admin/passkey/authentication/start`             | HTTP API     | external      | New            | None                     | backend         | web login           | 创建 passkey 登录 challenge          |
| `/api/admin/passkey/authentication/finish`            | HTTP API     | external      | New            | None                     | backend         | web login           | 完成 passkey 登录并设置 admin cookie |
| `/api/admin/passkey/reset/:token/registration/start`  | HTTP API     | external      | New            | None                     | backend         | web reset page      | 创建 reset 注册 challenge            |
| `/api/admin/passkey/reset/:token/registration/finish` | HTTP API     | external      | New            | None                     | backend         | web reset page      | 完成 passkey 注册并消费 reset token  |
| `tavily-hikari admin passkey reset-url`               | CLI          | internal      | New            | None                     | ops             | operator            | 本地生成一次性 reset URL             |
| `/api/profile`                                        | HTTP API     | external      | Modify         | None                     | backend         | public/admin web    | 增加 passkey capability              |

### 契约文档（按 Kind 拆分）

- None

## 验收标准（Acceptance Criteria）

- Given 没有 admin cookie 且没有 ForwardAuth
  When 访问 `/admin`
  Then 请求被拒绝。

- Given 已存在 admin passkey
  When 使用浏览器 passkey 登录成功
  Then `/api/profile` 返回 `isAdmin=true`，并可访问 admin-only API。

- Given CLI 生成 reset URL
  When 首次打开并完成 passkey 注册
  Then token 被消费，新 passkey 可用于登录。

- Given reset URL 已使用或已过期
  When 再次使用同一个 URL
  Then 后端拒绝注册。

- Given 认证返回的 credential counter 不合法
  When 后端完成 authentication finish
  Then 后端拒绝登录且不创建 admin session。

## 验收清单（Acceptance checklist）

- [x] 核心路径的长期行为已被明确描述。
- [x] 关键边界/错误场景已被覆盖。
- [x] 涉及的接口/契约已写清楚或明确为 `None`。
- [x] 相关验收条件已经可以用于实现与 review 对齐。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: store API、token TTL/消费、counter 更新。
- Integration tests: passkey API 的成功、失败、重放拒绝、cookie session。
- E2E tests: 可控浏览器/fixture 验证 reset 页面与登录页面行为。

### UI / Storybook (if applicable)

- Stories to add/update: 登录页 passkey 可用、未配置、reset 注册、token 过期/错误。
- Docs pages / state galleries to add/update: admin login state gallery。
- `play` / interaction coverage to add/update: reset token 错误和登录按钮状态。
- Visual regression baseline changes (if any): passkey login/reset UI 截图。

### Quality checks

- Rust: `cargo fmt`, targeted `cargo test`, `cargo clippy -- -D warnings`。
- Web: `bun run build` and Storybook smoke where feasible.

## Visual Evidence

- Passkey 登录状态：[admin-login-passkey.png](./assets/admin-login-passkey.png)
- Reset 注册状态：[admin-login-reset.png](./assets/admin-login-reset.png)
- Reset URL 注册模式：[admin-login-reset-enrollment.png](./assets/admin-login-reset-enrollment.png)
- Reset 注册完成后重新登录提示：[admin-login-reset-registered.png](./assets/admin-login-reset-registered.png)
- 管理员安全设置页：[admin-security-page.png](./assets/admin-security-page.png)
- 管理员安全操作确认弹窗：[admin-security-confirmation-inline.png](./assets/admin-security-confirmation-inline.png)
- 管理端 TOTP 6 格验证码输入：[admin-totp-six-digit-input.png](./assets/admin-totp-six-digit-input.png)
- 系统设置下的代理设置子菜单：[admin-system-settings-proxy-subnav.png](./assets/admin-system-settings-proxy-subnav.png)

## Related PRs

- None
