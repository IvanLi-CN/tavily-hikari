# 管理员登录：ForwardAuth + 内置登录可组合启用（#e5g7f）

## Goal

- 提供一个“内置管理员登录”方案（带登录界面），并允许与现有 ForwardAuth 同时启用或单独启用。
- 通过 **两个环境变量**分别控制：
  - 是否启用 ForwardAuth 管理员鉴权
  - 是否启用内置登录（cookie 会话）
- 明确当前仓库的 ForwardAuth 管理员判定口径：以 `FORWARD_AUTH_HEADER` 承载的“用户标识”与 `FORWARD_AUTH_ADMIN_VALUE` 精确匹配（README 示例为邮箱）。

## In / Out

### In

- Backend
  - 新增两项开关型环境变量（布尔值）控制 ForwardAuth / Built-in auth 是否启用。
  - 内置登录：提供登录/登出 API、基于 HttpOnly cookie 的会话、以及统一的管理员判定逻辑（ForwardAuth OR cookie session OR dev-open-admin）。
- Frontend
  - 新增登录界面（用于内置登录）。
  - 当启用内置登录且当前浏览器未登录时，首页展示“管理员登录”按钮。
- Docs
  - README（EN/ZH）补充两种鉴权模式的开关与使用说明。

### Out

- 不做多用户/角色系统（仅管理员单一会话）。
- 不做 OAuth/SSO/2FA。
- 不做跨设备会话管理与审计。

## Acceptance Criteria

- Given `ADMIN_AUTH_FORWARD_ENABLED=true` 且 `ADMIN_AUTH_BUILTIN_ENABLED=false`
  - When 请求携带 `FORWARD_AUTH_HEADER=<FORWARD_AUTH_ADMIN_VALUE>`
  - Then `/admin` 与所有 admin-only API 可访问；首页显示“管理员入口（Admin）”而非“登录”。
  - And 非管理员请求无法访问 admin-only API。
- Given `ADMIN_AUTH_FORWARD_ENABLED=false` 且 `ADMIN_AUTH_BUILTIN_ENABLED=true`
  - When 未登录访问首页
  - Then 首页出现“管理员登录”按钮；点击后可进入登录界面。
  - When 使用正确凭据登录
  - Then 浏览器获得会话 cookie，`/api/profile` 返回 `isAdmin=true`，admin-only API 可访问。
  - When 登出
  - Then cookie 被清除，admin-only API 再次不可访问。
- Given `ADMIN_AUTH_FORWARD_ENABLED=true` 且 `ADMIN_AUTH_BUILTIN_ENABLED=true`
  - Then ForwardAuth 与内置登录任意一条满足均应授予管理员权限（逻辑为 OR）。

## Testing

- Rust：覆盖“启用/禁用开关 + cookie 会话”的权限判定；覆盖登录成功/失败与登出 cookie 行为。
- Web：手动验证首页按钮与登录流程；确认 ForwardAuth-only 场景不出现多余登录按钮。

## Risks / Notes

- 内置登录需要安全的凭据配置（避免默认口令）；cookie 默认 `HttpOnly` + `SameSite=Lax`。
- ForwardAuth 的管理员判定口径来自 README：`FORWARD_AUTH_HEADER`（例 `Remote-Email`）与 `FORWARD_AUTH_ADMIN_VALUE`（例邮箱）精确匹配。
