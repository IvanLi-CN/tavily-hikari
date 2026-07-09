# HTTP APIs

## GET /api/user/dashboard

- Scope: external
- Change: New
- Auth: `hikari_user_session` cookie

### Query

- optional: `today_start`, `today_end`
- contract:
  - must be RFC3339 / ISO8601 datetimes with explicit offset or `Z`
  - must be provided together
  - must describe exactly one natural-day window aligned to local midnight
  - omitted pair falls back to the server-timezone current day window

### Response

- `200`
- body:
  - `requestRate { used, limit, windowMinutes, scope }`
  - `businessCalls1h { totalCount, successCount, failureCount, limit, windowMinutes }`
  - `dailyCreditsUsed`, `dailyCreditsLimit`
  - `monthlyCreditsUsed`, `monthlyCreditsLimit`
  - `dailySuccess`, `dailyFailure`, `monthlySuccess`
  - `lastActivity`

### Semantics

- `dailySuccess` / `dailyFailure`: explicit browser-window today when query params are present; otherwise server timezone current day
- `monthlySuccess`: current UTC month
- `businessCalls1h`: 最近 1 小时实际上游业务请求次数；成功与失败都计入，前置拦截与 `quota_exhausted` 不计入
- `dailyCreditsUsed` / `dailyCreditsLimit`: server timezone natural day
- `monthlyCreditsUsed` / `monthlyCreditsLimit`: current UTC month

### Error

- `401` 未登录
- `404` OAuth 功能未启用

## GET /api/user/billing/summary

- Scope: external
- Change: New
- Auth: `hikari_user_session` cookie

### Response

- `200`
- body:
  - `currentMonthStart`
  - `effectiveUntilMonthStart`
  - `blockAll`
  - `currentTotal { hourly, daily, monthly }`
  - `composition`
    - `baseAccess { hourly, daily, monthly }`
    - `tagAdjustments { hourly, daily, monthly }`
    - `permanentEntitlements { hourly, daily, monthly }`
    - `monthlyAdjustments { hourly, daily, monthly }`
    - `recharge`
      - `credits`
      - `quota { hourly, daily, monthly }`
  - `timeline[]`
    - `monthStart`
    - `isCurrentMonth`
    - `persistentTotal { hourly, daily, monthly }`
    - `monthlyAdjustments { hourly, daily, monthly }`
    - `recharge`
      - `credits`
      - `quota { hourly, daily, monthly }`
    - `effectiveTotal { hourly, daily, monthly }`

### Semantics

- `currentTotal`: 当前生效总额度，已包含基础额度、长期权益、标签增减、当前月调整和当前月充值叠加后的结果；若 `blockAll=true`，前端应按最终有效额度理解为 0。
- `composition`: 只输出用户可理解的安全摘要，不透出 admin note、actor、后台账本原文或内部 source id。
- `timeline[]`: 按服务器本地自然月连续返回，从上一个月开始，至少覆盖到下一个月；若存在时效权益，则继续扩展到最近有效月份的下一个月，保证月历视图始终可对比上月 / 本月 / 下月。
- `timeline[].persistentTotal`: 不依赖当月充值或当月一次性调整、可长期持续理解的额度基线，用于解释“长期/基础部分”。
- `timeline[].monthlyAdjustments`: 当前月或未来月的非充值月度变动汇总（例如管理员追加或扣减的月度额度），不包含 `timeline[].recharge.quota`。
- `timeline[].recharge.credits`: 仅表示当月充值权益对应的 credits 数，不等于总 monthly credits limit。

### Error

- `401` 未登录
- `404` OAuth 功能未启用

## GET /api/user/tokens

- Scope: external
- Change: New
- Auth: `hikari_user_session`

### Query

- optional: `today_start`, `today_end`
- same validation contract as `GET /api/user/dashboard`

### Response

- `200`
- body: `UserTokenSummary[]`
  - `tokenId`, `enabled`, `note`, `lastUsedAt`
  - `requestRate`
  - `businessCalls1h`
  - `dailyCreditsUsed/dailyCreditsLimit`
  - `monthlyCreditsUsed/monthlyCreditsLimit`
  - `dailySuccess`, `dailyFailure`, `monthlySuccess`

## GET /api/user/tokens/:id

- Scope: external
- Change: New
- Auth: `hikari_user_session`

### Query

- optional: `today_start`, `today_end`
- same validation contract as `GET /api/user/dashboard`

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
- 新增 `GET /console/billing` 页面入口，并在用户控制台顶部导航中作为稳定视图存在。
