export type SystemSettingsTotpTranslationKey =
  | 'totpTitle'
  | 'totpBoundHint'
  | 'totpUnboundHint'
  | 'totpMissingCryptoKey'
  | 'totpQrAlt'
  | 'totpSetupSecretLabel'
  | 'totpCurrentCodePlaceholder'
  | 'totpConfirmCodePlaceholder'
  | 'totpBindAction'
  | 'totpResetAction'
  | 'totpConfirmAction'
  | 'totpDisableAction'

export type SystemSettingsTrustedClientIpTranslationKey =
  | 'trustedClientIpTitle'
  | 'trustedClientIpConfigure'
  | 'trustedClientIpDialogDescription'
  | 'trustedProxyCidrs'
  | 'trustedClientIpHeaderOrder'
  | 'trustedClientIpHeaderOrderHint'
  | 'observedClientIpTitle'
  | 'observedClientIpDescription'
  | 'observedClientIpNoHeaders'
  | 'observedClientIpNoRequests'
  | 'observedClientIpRequestColumn'

export interface AdminRechargeTranslations {
  title: string
  description: string
  emptyHiddenDescription: string
  searchPlaceholder: string
  allStatuses: string
  orderDesc: string
  orderAsc: string
  viewAriaLabel: string
  flatView: string
  userView: string
  loading: string
  searchLabel: string
  statusFilterLabel: string
  startDateFilterLabel: string
  endDateFilterLabel: string
  sortFilterLabel: string
  orderFilterLabel: string
  groupSummary: string
  groupCredits: string
  summary: Record<'orders' | 'actionable' | 'totpRequired', string>
  status: Record<'pending' | 'paid' | 'failed' | 'refunding' | 'refunded' | 'refundOnly', string>
  statusAction: Record<'pending' | 'failed' | 'refunding' | 'refunded' | 'refundOnly', string>
  amountLdc: string
  orderCredits: string
  table: Record<'user' | 'order' | 'status' | 'amount' | 'createdAt' | 'paidAt' | 'refundedAt' | 'actions', string>
  actions: Record<'refund' | 'refundOnly' | 'previousPage' | 'nextPage' | 'cancel' | 'confirm', string>
  paginationSummary: string
  confirm: Record<'refundTitle' | 'refundOnlyTitle' | 'description' | 'totpLabel' | 'totpPlaceholder', string>
  userDetail: Record<
    | 'title'
    | 'loading'
    | 'description'
    | 'empty'
    | 'baseMonthly'
    | 'tagDelta'
    | 'currentMonthRecharge'
    | 'currentFinal'
    | 'used'
    | 'effectiveUntil'
    | 'effectiveUntilEmpty'
    | 'base'
    | 'tag'
    | 'recharge'
    | 'final'
    | 'monthColumn'
    | 'baseColumn'
    | 'tagColumn'
    | 'rechargeColumn'
    | 'finalColumn'
    | 'usedColumn',
    string
  >
  sort: Record<'createdAt' | 'paidAt' | 'refundedAt' | 'status', string>
}
