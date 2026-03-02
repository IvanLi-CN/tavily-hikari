# HTTP APIs

## GET /api/user/dashboard

- Scope: external
- Change: New
- Auth: `hikari_user_session` cookie

### Response

- `200`
- body:
  - `hourlyAnyUsed`, `hourlyAnyLimit`
  - `quotaHourlyUsed`, `quotaHourlyLimit`
  - `quotaDailyUsed`, `quotaDailyLimit`
  - `quotaMonthlyUsed`, `quotaMonthlyLimit`
  - `dailySuccess`, `dailyFailure`, `monthlySuccess`
  - `lastActivity`

### Error

- `401` 未登录
- `404` OAuth 功能未启用

## GET /api/user/tokens

- Scope: external
- Change: New
- Auth: `hikari_user_session`

### Response

- `200`
- body: `UserTokenSummary[]`
  - `tokenId`, `enabled`, `note`, `lastUsedAt`
  - `hourlyAnyUsed/hourlyAnyLimit`
  - `quotaHourlyUsed/quotaHourlyLimit`
  - `quotaDailyUsed/quotaDailyLimit`
  - `quotaMonthlyUsed/quotaMonthlyLimit`
  - `dailySuccess`, `dailyFailure`, `monthlySuccess`

## GET /api/user/tokens/:id

- Scope: external
- Change: New
- Auth: `hikari_user_session`

### Response

- `200` `UserTokenSummary`

### Error

- `401` 未登录
- `404` token 不属于当前用户或 OAuth 未启用

## GET /api/user/tokens/:id/secret

- Scope: external
- Change: New
- Auth: `hikari_user_session`

### Response

- `200` `{ "token": "th-<id>-<secret>" }`

### Error

- `401` 未登录
- `404` token 不属于当前用户或不可用

## GET /api/user/tokens/:id/logs?limit=20

- Scope: external
- Change: New
- Auth: `hikari_user_session`

### Response

- `200` `PublicTokenLog[]`（已做敏感字段脱敏）

### Error

- `401` 未登录
- `404` token 不属于当前用户或 OAuth 未启用

## Route changes

- `GET /auth/linuxdo` 生成登录 state 时默认 `redirect_to=/console`。
- `GET /` 当用户 session 有效时返回 `302 /console`。
- 新增 `GET /console` 与 `GET /console/` 页面入口。
