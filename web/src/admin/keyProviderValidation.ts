/**
 * Pure helpers for the per-provider validation dialog results (plan todo 22).
 *
 * Keeps `ApiKeysValidationDialog.tsx` focused on layout while these functions
 * resolve the stable `admin.providers.*` copy, parse the SerpApi `/account`
 * plan detail (todo-21 `parseSerpApiValidateDetail`), and map probe reason
 * codes / Exa verdicts to localized labels.
 */
import { parseProviderId, parseSerpApiValidateDetail, type ProviderId, type SerpApiPlanFields } from '../api'
import type {
  AdminProviderSharedCopy,
  AdminProvidersTranslations,
  ProbeReasonCode,
  SerpApiProviderCopy,
} from '../i18n/adminProviderTranslationTypes'

const PROBE_REASON_CODES: readonly ProbeReasonCode[] = [
  'VALIDATION',
  'SUBSCRIPTION_TOKEN_INVALID',
  'QUOTA_LIMITED',
  'RATE_LIMITED',
  'UNAUTHORIZED',
  'UNBILLED_VALIDATION_ERROR',
]

const PROBE_REASON_CODE_SET: ReadonlySet<string> = new Set(PROBE_REASON_CODES)

export function isProbeReasonCode(value: unknown): value is ProbeReasonCode {
  return typeof value === 'string' && PROBE_REASON_CODE_SET.has(value)
}

export function providerProbeCopy(providers: AdminProvidersTranslations, provider: unknown): string {
  return providers[parseProviderId(provider)].validateProbe
}

export function providerQuotaCopy(providers: AdminProvidersTranslations, provider: unknown): string {
  return providers[parseProviderId(provider)].quotaNote
}

export function providerLabelCopy(providers: AdminProvidersTranslations, provider: unknown): string {
  return providers[parseProviderId(provider)].label
}

export interface SerpApiPlanEntry {
  id: keyof SerpApiPlanFields
  label: string
  value: string
}

const SERPAPI_PLAN_LABELS: Record<keyof SerpApiPlanFields, keyof SerpApiProviderCopy['planFields']> = {
  plan_searches_left: 'planSearchesLeft',
  total_searches_left: 'totalSearchesLeft',
  searches_per_month: 'searchesPerMonth',
  plan_renewal_date: 'planRenewalDate',
  account_status: 'accountStatus',
}

/** All five plan fields in stable order; missing values render as an em dash. */
export function serpApiPlanEntries(
  detail: unknown,
  labels: SerpApiProviderCopy['planFields'],
): SerpApiPlanEntry[] {
  const plan = parseSerpApiValidateDetail(detail)
  if (!plan) return []
  return (Object.keys(SERPAPI_PLAN_LABELS) as Array<keyof SerpApiPlanFields>).map((id) => ({
    id,
    label: labels[SERPAPI_PLAN_LABELS[id]],
    value: plan[id] ?? '—',
  }))
}

/** Localized label for a probe reason code; unknown codes return null. */
export function reasonCodeLabel(shared: AdminProviderSharedCopy, reason: unknown): string | null {
  if (!isProbeReasonCode(reason)) return null
  return shared.reasonCodes[reason]
}

/**
 * Exa has no machine reason on the discrimination path the way Brave does:
 * a 401 is invalid, an unbilled 400/422 is a valid authenticated key.
 */
export function exaProbeVerdict(
  shared: AdminProviderSharedCopy,
  reason: unknown,
  status?: string | null,
): string | null {
  if (reason === 'UNBILLED_VALIDATION_ERROR') return shared.exaVerdictValid
  if (reason === 'UNAUTHORIZED') return shared.exaVerdictInvalid
  const normalizedStatus = (status ?? '').toLowerCase()
  if (normalizedStatus === 'unauthorized' || normalizedStatus === 'forbidden' || normalizedStatus === 'invalid') {
    return shared.exaVerdictInvalid
  }
  return null
}

export function providerIs(provider: unknown, expected: ProviderId): boolean {
  return parseProviderId(provider) === expected
}