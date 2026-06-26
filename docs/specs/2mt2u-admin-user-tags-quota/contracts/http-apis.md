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
  - 旧 `hourlyAnyDelta` 只保留服务端兼容入口，不再属于对外合同。

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
- Notes
  - 自动同步的 LinuxDo 系统标签会像其他 tag 一样出现在 `tags` 与 `quotaBreakdown` 中，并把默认 delta 叠加到 `effectiveQuota`。
  - `quotaBreakdown` 始终包含一条最终 `effective` 行，反映经过 `max(0, value)` 钳制后的最终有效额度。
  - 用户详情 summary / detail 对外只返回：
    - `requestRate`
    - `businessCalls1h`
    - `dailyCreditsUsed` / `dailyCreditsLimit`
    - `monthlyCreditsUsed` / `monthlyCreditsLimit`
  - 不再返回旧 `quotaHourly*`、`quotaDaily*`、`quotaMonthly*`、`hourlyAny*` 平铺字段。

## PATCH `/api/users/:id/quota`

- Auth: admin only
- Path unchanged.
- Body shape:
  - `hourlyLimit: number`
  - `dailyLimit: number`
  - `monthlyLimit: number`
- Semantics changed:
  - Writes user base quota only.
  - If payload equals current env defaults, server may set `inherits_defaults=1`; otherwise `inherits_defaults=0`.
  - 旧 `hourlyAnyLimit` 若被 legacy caller 送入，服务端接受但忽略。
