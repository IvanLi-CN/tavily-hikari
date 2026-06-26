# HTTP APIs

## GET `/auth/linuxdo`

- Scope: external
- Change: Keep
- Auth: none

### Response

- `302` 重定向到 LinuxDo authorize endpoint
- Query 包含 `client_id`、`redirect_uri`、`response_type=code`、`scope`、`state`
- Error
  - `404` OAuth 未启用
  - `500` 生成 oauth state 失败

## POST `/auth/linuxdo/finalize`

- Scope: external
- Change: New
- Auth: none
- Content-Type: `application/json`

### Request body

- `code`（required）
- `state`（required）

### Response

- Success
  - `200`
  - body: `{"ok":true,"outcome":"success","redirectTo":"/console"}`
  - headers: 设置 `hikari_user_session` cookie，并清理一次性 oauth binding cookie
- Redirected failure
  - `403`
  - body: `{"ok":false,"outcome":"registration_paused","redirectTo":"/registration-paused"}`
- User-facing recoverable failures
  - `400` + `invalid_state`
  - `403` + `inactive_user`
  - `502` + `upstream_failure`
  - `500` + `server_error`
  - 所有非成功结果都清理一次性 oauth binding cookie

## GET `/auth/linuxdo/callback`

- Scope: external
- Change: Modify
- Auth: none

### Response

- `409`
- 返回诊断性 HTML
- 固定说明：
  - 旧 callback path 不再承担正式登录完成
  - `LINUXDO_OAUTH_REDIRECT_URL` 应配置到 `/console/oauth/linuxdo/callback`
  - 浏览器 callback 页会调用 `POST /auth/linuxdo/finalize`
