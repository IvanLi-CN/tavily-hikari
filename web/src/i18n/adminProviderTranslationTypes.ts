/**
 * Translation contract for the multi-provider admin surfaces (plan todo 22).
 *
 * `admin.providers.*` is the stable i18n registration point declared by the
 * todo-21 provider meta (`PROVIDER_META[id].i18nKey`,
 * `.validateProbeI18nKey`, `.quotaNoteI18nKey`). The provider brand labels are
 * proper nouns; the probe/quota/reason copy is operator-facing prose that MUST
 * be localized in both catalogs so the todo-22 dialog and quota panels can
 * state the exact zero-cost probe method (M13) and the passive/live sync
 * semantics without ever leaking an unregistered string.
 */

/** Shared per-provider copy (brand label + stable probe/quota notes). */
export interface ProviderCopy {
  label: string
  /** Localized statement of the exact zero-cost probe actually sent (M13). */
  validateProbe: string
  /** Localized quota-panel note for this provider. */
  quotaNote: string
}

/** SerpApi additionally exposes the `/account` plan fields in the dialog. */
export interface SerpApiProviderCopy extends ProviderCopy {
  planFields: {
    planSearchesLeft: string
    totalSearchesLeft: string
    searchesPerMonth: string
    planRenewalDate: string
    accountStatus: string
  }
}

/** Stable machine-readable probe discrimination codes echoed by the backend. */
export type ProbeReasonCode =
  | 'VALIDATION'
  | 'SUBSCRIPTION_TOKEN_INVALID'
  | 'QUOTA_LIMITED'
  | 'RATE_LIMITED'
  | 'UNAUTHORIZED'
  | 'UNBILLED_VALIDATION_ERROR'

/** Copy shared by every provider surface (table/filter/dialog/quota). */
export interface AdminProviderSharedCopy {
  tableHeader: string
  filterLabel: string
  filterAll: string
  selectLabel: string
  selectBatchHint: string
  quotaUsed: string
  quotaRemaining: string
  quotaRenewalDate: string
  quotaSynced: string
  probeMethodLabel: string
  reasonLabel: string
  exaVerdictValid: string
  exaVerdictInvalid: string
  syncUnsupportedHint: string
  syncPassiveNotice: string
  syncErrorUnsupported: string
  reasonCodes: Record<ProbeReasonCode, string>
}

/** The full `admin.providers` subtree (registered in both catalogs). */
export interface AdminProvidersTranslations {
  tavily: ProviderCopy
  serpapi: SerpApiProviderCopy
  exa: ProviderCopy
  brave: ProviderCopy
  shared: AdminProviderSharedCopy
}