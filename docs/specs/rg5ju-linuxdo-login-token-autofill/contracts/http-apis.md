# HTTP APIs

## LinuxDo OAuth start（GET /auth/linuxdo）

- 范围（Scope）: external
- 变更（Change）: New
- 鉴权（Auth）: none

### 响应

- Success: `302` 重定向到 LinuxDo authorize endpoint，带 `client_id`, `redirect_uri`, `response_type=code`, `scope`, `state`
- Error:
  - `404` 当 OAuth 未启用
  - `500` 生成登录 state 失败

## LinuxDo OAuth callback（GET /auth/linuxdo/callback）

- 范围（Scope）: external
- 变更（Change）: New
- 鉴权（Auth）: none

### Query

- `code`（required）
- `state`（required）

### 响应

- Success: `302 /` 并设置 `hikari_user_session` cookie（HttpOnly, SameSite=Lax）
- Error:
  - `400` 参数缺失/非法 state
  - `401` OAuth 换 token 或 userinfo 失败
  - `500` 本地持久化失败

## User logout（POST /api/user/logout）

- 范围（Scope）: external
- 变更（Change）: New
- 鉴权（Auth）: user session cookie（存在时清理）

### 响应

- Success: `204` 并清理 `hikari_user_session` cookie

## User token（GET /api/user/token）

- 范围（Scope）: external
- 变更（Change）: New
- 鉴权（Auth）: user session cookie

### 响应

- Success:
  - `200`
  - body: `{ "token": "th-<id>-<secret>" }`
- Error:
  - `401` 未登录
  - `404` 用户无绑定 token
  - `409` token 已禁用或软删除（不可用）
  - `500` 查询失败

## Profile 扩展（GET /api/profile）

- 范围（Scope）: external
- 变更（Change）: Modify（向后兼容新增字段）
- 鉴权（Auth）: none

### 新增可选字段

- `userLoggedIn: boolean`
- `userProvider: "linuxdo" | null`
- `userDisplayName: string | null`
