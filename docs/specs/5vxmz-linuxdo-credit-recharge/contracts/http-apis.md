# HTTP APIs

## GET /api/user/recharge/config

- Auth: `hikari_user_session`
- Response `200`:
  - `enabled`: boolean
  - `unitCredits`: `1000`
  - `unitPriceLdc`: `50`
  - `minCredits`: `1000`
  - `maxCredits`: `20000`
  - `creditsStep`: `1000`
  - `defaultCredits`: initial UI value; `1` only when the test-price offer is enabled
  - `minMonths`: `1`
  - `maxMonths`: `12`
  - `quotaDeltaBaseCredits`: quota increment calculation base, currently `1000`
  - `hourlyDeltaPerQuotaUnit`: `20`
  - `dailyDeltaPerQuotaUnit`: `100`
  - `monthlyDeltaPerQuotaUnit`: quota-month increment for one `quotaDeltaBaseCredits`
  - `testPriceEnabled`: boolean; when enabled, this only adds the extra `1 credit × 1 month`
    offer for `1 LDC`
  - `currentMonthStart`: Unix timestamp for current server-local month start in UTC
  - `currentEntitlementCredits`: current month purchased credits
  - `currentEntitlementHourlyDelta`
  - `currentEntitlementDailyDelta`
  - `currentEntitlementMonthlyDelta`
  - `effectiveUntilMonthStart`: latest entitled month start, or `null`

## GET /api/user/billing/summary

- Auth: `hikari_user_session`
- Response `200`:
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

- Semantics:
  - Dedicated to the user-facing `/console/billing` page; it is not an admin ledger export.
  - `composition` only exposes safe user-readable summaries and must not contain admin note,
    actor, internal source id, or raw ledger rows.
  - `timeline[].persistentTotal` represents the long-lived quota baseline the user can treat as
    ongoing access, excluding current-month recharge and one-off month adjustments.
  - `timeline[]` is a natural-month schedule from the previous server-local month through at least
    the next month, and extends through the month after the latest effective recharge month when
    time-limited quota is present.
  - `timeline[].monthlyAdjustments` excludes recharge-derived deltas so the UI can explain month
    adjustments and recharge increments separately.

## POST /api/user/recharge/quote

- Auth: `hikari_user_session`
- Request JSON:
  - `credits`: positive integer, multiple of `1000`
    - normal mode: `1000..=20000`, step `1000`
    - test pricing: additionally allows exactly `1` credit when `months` is exactly `1`
  - `months`: integer in `1..=12`
- Response `200`:
  - `requestedCredits`
  - `requestedMonths`
  - `quoteMonthStart`
  - `remainingDaysInclusive`
  - `unitCredits`
  - `unitPriceCents`
  - `fullMonthHourlyDelta`
  - `fullMonthDailyDelta`
  - `fullMonthMonthlyDelta`
  - `fullMonthMoneyCents`
  - `currentMonthFinalHourlyDelta`
  - `currentMonthFinalDailyDelta`
  - `currentMonthFinalMonthlyDelta`
  - `currentMonthFinalMoneyCents`
  - `fullOrderMoneyCents`
  - `finalOrderMoneyCents`
  - `monthEndClampApplied`
  - `orderName`
  - `schedule[]`: month-by-month final values, full-month baseline, month discount, and clamp reason

## GET /api/user/recharge/orders

- Auth: `hikari_user_session`
- Response `200`: `{ "items": RechargeOrder[] }`
- Semantics:
  - The billing page continues to use this endpoint as the source of recent orders instead of
    embedding order rows into `GET /api/user/billing/summary`.

## GET /api/user/recharge/orders/:out_trade_no

- Auth: `hikari_user_session`
- Response `200`: `RechargeOrder`
- Error:
  - `404` if the order does not belong to current user.

## POST /api/user/recharge/orders

- Auth: `hikari_user_session`
- Request JSON:
  - `credits`: positive integer, multiple of `1000`
    - normal mode: `1000..=20000`, step `1000`
    - test pricing: additionally allows exactly `1` credit when `months` is exactly `1`
  - `months`: integer in `1..=12`
    - test pricing does not allow `1` credit with more than one month
  - `quote`: complete response payload from `POST /api/user/recharge/quote`
- Behavior:
  - Server recomputes the canonical quote and rejects stale/mismatched quote fields.
  - The stored order keeps both the original requested `credits/months` and final `money/hourly/daily/monthly` values for the quote month.
- Response `200`:
  - `order`: `RechargeOrder`
  - `paymentUrl`: Linux.do Credit payment URL
- Error:
  - `400` invalid credits/months
  - `503` recharge not configured

## GET /api/linuxdo-credit/notify

- Auth: Linux.do Credit signed query.
- Query:
  - `pid`, `trade_no`, `out_trade_no`, `type`, `name`, `money`, `trade_status`, `sign`
- Response:
  - `200 text/plain` body `success` when accepted or already applied.
  - `400` when signature, order, status, or amount does not match.
- Behavior:
  - Moves payable orders to `paid` and creates monthly recharge entitlements from the payment
    month in `account_entitlements`, mirrored to the legacy recharge entitlement table.
  - Replayed callbacks update notify audit fields only and must not move `refunding`,
    `refunded`, `refundOnly`, or `expired` orders back to `paid`.
  - If the callback arrives after the quote month has ended, the order becomes `expired` and no entitlement rows are written.

## GET /api/users/:id

- Change: response adds `recharge` and `entitlements` objects.
- Shape:
  - `recharge.currentMonthEntitlementCredits`: current-month Linux.do Credit recharge credits only
  - `recharge.currentMonthEntitlementHourlyDelta`: current-month recharge hourly quota delta
  - `recharge.currentMonthEntitlementDailyDelta`: current-month recharge daily quota delta
  - `recharge.currentMonthEntitlementMonthlyDelta`: current-month recharge monthly quota delta
  - `recharge.effectiveUntilMonthStart`: latest recharge-entitled month start, or `null`
  - `recharge.orders`: recent `RechargeOrder[]`
  - `recharge.entitlements`: recent recharge-sourced entitlement rows
  - `entitlements.currentMonthStart`: current server-local month start
  - `entitlements.currentBaseDelta`: base quota entitlement delta summary
  - `entitlements.currentMonthDelta`: current-month admin/recharge entitlement delta summary
  - `entitlements.currentPermanentDelta`: permanent entitlement delta summary
  - `entitlements.items`: recent account entitlement ledger rows, including admin notes and actor metadata

## GET /api/users/:id/entitlements

- Auth: admin request.
- Query:
  - `scopeKind`: optional `all|base|month|permanent`.
  - `startMonth`: optional Unix timestamp, matched against monthly entitlement target month.
  - `endMonthBefore`: optional exclusive Unix timestamp, matched against monthly entitlement target
    month.
- Response `200`: `{ "items": AdminUserEntitlement[] }`
- Semantics:
  - Base quota rows are included unless `scopeKind` selects another scope.
  - Monthly rows are filtered by target month, not creation time.
  - Permanent rows remain visible unless `scopeKind=base|month`.
  - Rows are returned in target-month-descending audit order.

## POST /api/users/:id/entitlements

- Auth: admin request with master write access.
- Request JSON:
  - `scopeKind`: `base|month|permanent`
  - `monthStart`: required for `month`; ignored for `base` and `permanent`
  - `businessCalls1hDelta`: integer
  - `dailyCreditsDelta`: integer
  - `monthlyCreditsDelta`: integer
  - `backendNote`: optional string
  - `frontendNote`: non-empty string
- Response `201`: created `AdminUserEntitlement`
- Semantics:
  - Base rows adjust the account base quota and are displayed as base quota in user detail.
  - At least one delta must be non-zero.
  - Positive and negative deltas are allowed.
  - Rows are append-only; mistakes are corrected by adding a reverse row.
  - `frontendNote` is admin/API visible only and is not exposed in user console responses.

## GET /api/admin/recharges

- Auth: admin request.
- Query:
  - `user`: optional search across user id/display name/username/order/trade number.
  - `status`: optional `pending|paid|failed|expired|refunding|refunded|refundOnly|all`.
  - `startAt`, `endAt`: optional Unix timestamps matched against order creation time.
  - `sort`: `createdAt|paidAt|refundedAt|status`; default `createdAt`.
  - `order`: `asc|desc`; default `desc`.
  - `view`: `flat|user`; `user` additionally returns user aggregation rows.
  - `page`, `perPage`: flat list pagination.
- Response `200`:
  - `hasRechargeOrders`: boolean controlling admin navigation visibility.
  - `items`: flat `AdminRechargeOrder[]`.
  - `groups`: user aggregation rows when `view=user`.
  - `total`, `page`, `perPage`.
  - `items[]` include final `moneyCents`, `quoteMonthStart`, final `hourly/daily/monthly` deltas, and `monthEndClampApplied`.

## POST /api/admin/recharges/:out_trade_no/refund

- Auth: admin request.
- Request JSON: `{ "totpCode": "123456" }`.
- Behavior:
  - Rejects `DEV_OPEN_ADMIN`, missing TOTP binding, invalid/locked TOTP, and non-`paid` orders.
  - Calls Linux.do Credit `POST /epay/api.php` with `act=refund`, `pid`, `key`, `trade_no`, `out_trade_no`, `money`.
  - Before the platform call, atomically moves the order from `paid` to `refunding` so duplicate admin requests cannot issue duplicate external refunds.
  - After platform success, persists an external-success marker before final local settlement; a retry of a matching `refunding` order with that marker completes only local settlement and does not call the platform again.
  - On platform success, marks order `refunded`, sets `refundedAt/refundActor/refundPayload`,
    deletes the order's recharge entitlement rows from the unified entitlement table and legacy
    backup table, invalidates quota and records a quota snapshot.
  - On platform failure before completion, moves the order back to `paid` and records the refund error in `lastError`.
- Response `200`: updated `AdminRechargeOrder`.

## POST /api/admin/recharges/:out_trade_no/refund-only

- Auth: admin request.
- Request JSON: `{ "totpCode": "123456" }`.
- Behavior:
  - Same Linux.do Credit full-refund call and TOTP checks as `refund`.
  - On platform success, marks order `refundOnly`, keeps entitlements, records refund fields.
- Response `200`: updated `AdminRechargeOrder`.

## GET /api/admin/totp

- Auth: admin request.
- Response `200`:
  - `enabled`: whether a global admin TOTP secret is bound.
  - `available`: false when recharge is disabled, crypt key is missing, or `DEV_OPEN_ADMIN` is active.
  - `rechargeFeatureEnabled`, `missingCryptoKey`, `lockedUntil`, `issuer`, `accountName`.

## POST /api/admin/totp/setup

- Auth: admin request.
- Preconditions: recharge feature enabled, crypt key present, not `DEV_OPEN_ADMIN`.
- Response `200`: `{ "secret", "otpAuthUrl", "qrPngBase64" }`.
- The setup secret is not persisted until `confirm` succeeds.

## POST /api/admin/totp/confirm

- Auth: admin request.
- Request JSON: `{ "secret": "...", "code": "123456" }`.
- Behavior: verifies the code for the supplied secret, encrypts the secret with
  `LINUXDO_OAUTH_REFRESH_TOKEN_CRYPT_KEY`, and stores it globally.
- Response `200`: TOTP status.

## POST /api/admin/totp/reset

- Auth: admin request.
- Request JSON: `{ "currentCode": "123456", "secret": "...", "code": "654321" }`.
- Behavior: verifies current bound TOTP, verifies the new secret/code pair, then replaces the
  encrypted global secret.
- Response `200`: TOTP status.

## POST /api/admin/totp/disable

- Auth: admin request.
- Request JSON: `{ "totpCode": "123456" }`.
- Behavior: verifies current bound TOTP, then clears the global secret and failure state.
- Response `200`: TOTP status.

## RechargeOrder

- `outTradeNo`
- `status`: `pending|paid|failed|expired|refunding|refunded|refundOnly`
- `credits`
- `months`
- `money`
- `quoteMonthStart`
- `finalMoneyCents`
- `finalHourlyDelta`
- `finalDailyDelta`
- `finalMonthlyDelta`
- `monthEndClampApplied`
- `tradeNo`
- `paymentUrl`
- `createdAt`
- `updatedAt`
- `paidAt`
- `refundedAt`
- `refundActor`
- `lastNotifyAt`
- `lastError`
