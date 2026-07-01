# HTTP APIs

## GET `/api/user-tags`

- Auth: admin only
- Response `200`
  - `items: AdminUserTagView[]`

## POST `/api/user-tags`

- Auth: admin only
- Body
  - `name: string`
  - `displayName: string`
  - `icon?: string | null`
  - `effectKind: 'quota_delta' | 'block_all'`
  - `businessCalls1hDelta: number`
  - `dailyCreditsDelta: number`
  - `monthlyCreditsDelta: number`
- Notes
  - 仅允许创建 custom tag。
  - 旧 `hourlyAnyDelta/hourlyDelta/dailyDelta/monthlyDelta` 已从对外合同移除。

## PATCH `/api/user-tags/:tagId`

- Auth: admin only
- Body 与创建接口相同，但仅允许更新 custom tag 的展示与额度效果；system tag 仅允许更新 effect 与 delta，不允许修改 `name/displayName/icon/systemKey`。

## DELETE `/api/user-tags/:tagId`

- Auth: admin only
- Response
  - `204` when deleted
  - `400` when the tag is system-defined

## POST `/api/users/:id/tags`

- Auth: admin only
- Body
  - `tagId: string`
- Notes
  - 仅允许手工绑定 custom tag；system tag 绑定由系统同步维护。

## DELETE `/api/users/:id/tags/:tagId`

- Auth: admin only
- Response
  - `204` when unbound
  - `400` when the binding is system-managed

## GET `/api/users`

- Auth: admin only
- Existing response remains paginated.
- Supports optional `tagId` for exact tag-bound user filtering (used by the tag catalog jump-to-users action).
- When `q` and `tagId` are both present, the server applies them conjunctively: fuzzy user search stays scoped to the exact tag-bound subset.
- Each item extends with:
  - `tags: AdminUserTagCompactView[]`

## GET `/api/users/:id`

- Auth: admin only
- Response extends with:
  - `tags: AdminUserTagBindingView[]`
  - `quotaBase: AdminQuotaView`
  - `effectiveQuota: AdminQuotaView`
  - `quotaBreakdown: AdminUserQuotaBreakdownView[]`
  - `entitlements: AdminUserEntitlementsView`
- Notes
  - 自动同步的 LinuxDo 系统标签会像其他 tag 一样出现在 `tags` 与 `quotaBreakdown` 中，并把默认 delta 叠加到 `effectiveQuota`。
  - `effectiveQuota` 继续按“用户基线 + 全部标签 delta + 当前月权益 delta + 长期权益 delta”汇总。
  - 当前月权益在 `quotaBreakdown` 中以 `entitlement_month` 行展示；长期权益以 `entitlement_permanent` 行展示。
  - `quotaBreakdown` 始终包含一条最终 `effective` 行，反映经过 `max(0, value)` 钳制后的最终有效额度。
  - 用户详情 summary / detail 对外只返回：
    - `requestRate`
    - `businessCalls1h`
    - `dailyCreditsUsed` / `dailyCreditsLimit`
    - `monthlyCreditsUsed` / `monthlyCreditsLimit`
  - 不再返回旧 `quotaHourly*`、`quotaDaily*`、`quotaMonthly*`、`hourlyAny*` 平铺字段。

## GET `/api/users/:id/entitlements`

- Auth: admin only
- Query:
  - `scopeKind`: optional `all|month|permanent`
  - `startMonth`: optional Unix timestamp for monthly target-month lower bound
  - `endMonthBefore`: optional Unix timestamp for monthly target-month exclusive upper bound
- Response: `{ items: AdminUserEntitlementView[] }`
- Monthly filters match entitlement target month. Permanent entitlements are visible unless the
  request explicitly selects `month`.

## POST `/api/users/:id/entitlements`

- Auth: admin only with master write access
- Body:
  - `scopeKind: "month" | "permanent"`
  - `monthStart?: number | null`
  - `businessCalls1hDelta: number`
  - `dailyCreditsDelta: number`
  - `monthlyCreditsDelta: number`
  - `backendNote: string`
  - `frontendNote: string`
- Semantics:
  - Writes one append-only unified entitlement row.
  - `monthStart` is required for monthly rows; permanent rows are stored with no target month.
  - Positive and negative deltas are allowed; at least one delta must be non-zero.
  - Both notes are required and admin-visible. `frontendNote` is not exposed in user-console APIs.

## PATCH `/api/users/:id/quota`

- Auth: admin only
- Path unchanged.
- Body shape:
  - `businessCalls1hLimit: number`
  - `dailyCreditsLimit: number`
  - `monthlyCreditsLimit: number`
- Semantics changed:
  - Writes user base quota only.
  - If payload equals current env defaults, server may set `inherits_defaults=1`; otherwise `inherits_defaults=0`.
  - 旧 `hourlyAnyLimit/hourlyLimit/dailyLimit/monthlyLimit` 不再属于对外合同。
