/**
 * Provider identity, capability metadata, and additive DTO mirrors for the
 * multi-provider search proxy (plan todo 21).
 *
 * Wire shapes are mirrored field-for-field from the committed Rust sources:
 * - `src/providers/id.rs` — `ProviderId` lowercase serde keys.
 * - `src/server/dto.rs` — `ApiKeyView` (provider, quota_renews_at),
 *   `RequestLogView` (provider), `RequestLogFacetsView` (providers),
 *   `SummaryView` + `SummaryProviderCountView` (provider breakdown).
 * - `src/server/handlers/public.rs` — `SummaryWindowView.providers`.
 * - `src/server/handlers/admin_resources/versions_and_proxy_helpers.rs` —
 *   `CreateKeyRequest.provider`, `ValidateKeysRequest.provider`,
 *   `ValidateKeyResult` (provider, reason).
 * - `src/server/handlers/admin_resources/api_keys_and_logs.rs` — the M13
 *   zero-cost probe endpoints, statuses, reason codes, and the SerpApi
 *   `/account` plan detail string.
 * - `src/server/handlers/admin_auth.rs` — `ProviderQuotaSyncSnapshot`
 *   (source `live`/`passive`), the typed `sync_unsupported` 409 body, and the
 *   legacy Tavily 204 branch.
 *
 * Language policy: labels are language-neutral (brand proper nouns plus
 * technical probe descriptions), following the `tokenLogRequestKinds.ts`
 * catalog convention. Localized copy is NOT stored here; the stable `*I18nKey`
 * references are the registration points for todos 22-24 (en + zh). This module
 * introduces no third language and edits no translation catalog.
 */

/** Closed provider contract: the four lowercase wire keys of `ProviderId`. */
export type ProviderId = 'tavily' | 'serpapi' | 'exa' | 'brave'

/** Stable iteration order for provider selects, badges, and filters. */
export const PROVIDER_IDS: readonly ProviderId[] = ['tavily', 'serpapi', 'exa', 'brave']

/**
 * Legacy rows and payloads without a provider field are Tavily. Mirrors the
 * backend coalesce (`provider TEXT NOT NULL DEFAULT 'tavily'`, v45 DDL) and
 * the sync-usage fall-through for unknown key ids.
 */
export const LEGACY_PROVIDER_ID: ProviderId = 'tavily'

const PROVIDER_ID_SET: ReadonlySet<string> = new Set(PROVIDER_IDS)

function isProviderId(value: unknown): value is ProviderId {
  return typeof value === 'string' && PROVIDER_ID_SET.has(value)
}

/**
 * Tolerant parser for wire values: known ids pass through, everything else
 * (absent, null, future provider, wrong type) coalesces to
 * {@link LEGACY_PROVIDER_ID}. Never throws.
 */
export function parseProviderId(value: unknown): ProviderId {
  return isProviderId(value) ? value : LEGACY_PROVIDER_ID
}

/**
 * Normalize a legacy payload row (`ApiKeyStats`, `RequestLog`, facets, demo
 * fixtures): an absent/null/unknown `provider` field coalesces to `tavily`.
 */
export function normalizePayloadProvider(
  payload: { provider?: unknown } | null | undefined,
): ProviderId {
  return parseProviderId(payload?.provider)
}

/** How a provider quota block is rendered (todos 10/14/19 semantics). */
export type QuotaDisplayKind =
  | 'credits'
  | 'searches_with_renewal'
  | 'harvested_remaining'
  | 'local_counting'

/** Sync-usage behavior: live upstream fetch, passive snapshot echo, or none. */
export type SyncUsageMode = 'live' | 'passive' | 'unsupported'

/** The `source` discriminator of `ProviderQuotaSyncSnapshot`. */
export type ProviderQuotaSource = 'live' | 'passive'

export interface ProviderMeta {
  id: ProviderId
  /** Language-neutral display label (brand proper noun; identical in en/zh). */
  label: string
  /** Stable i18n registration point for localized provider copy (todos 22-24). */
  i18nKey: string
  supportsValidate: boolean
  /** Language-neutral description of the zero-cost probe actually sent (M13). */
  validateProbeLabel: string
  /** Stable i18n registration point for the probe-method dialog copy (M13). */
  validateProbeI18nKey: string
  supportsSyncUsage: boolean
  syncUsageMode: SyncUsageMode
  quotaDisplayKind: QuotaDisplayKind
  /** Stable i18n registration point for the per-provider quota note (todo 22). */
  quotaNoteI18nKey: string
}

/**
 * Per-provider metadata. `Record<ProviderId, ...>` makes an omitted provider a
 * compile error, so the map can never drift from the closed contract.
 */
export const PROVIDER_META: Readonly<Record<ProviderId, ProviderMeta>> = {
  tavily: {
    id: 'tavily',
    label: 'Tavily',
    i18nKey: 'admin.providers.tavily',
    supportsValidate: true,
    validateProbeLabel: 'GET /usage (legacy credit probe)',
    validateProbeI18nKey: 'admin.providers.tavily.validateProbe',
    supportsSyncUsage: true,
    syncUsageMode: 'live',
    quotaDisplayKind: 'credits',
    quotaNoteI18nKey: 'admin.providers.tavily.quotaNote',
  },
  serpapi: {
    id: 'serpapi',
    label: 'SerpApi',
    i18nKey: 'admin.providers.serpapi',
    supportsValidate: true,
    validateProbeLabel: 'GET /account (free, uncounted)',
    validateProbeI18nKey: 'admin.providers.serpapi.validateProbe',
    supportsSyncUsage: true,
    syncUsageMode: 'live',
    quotaDisplayKind: 'searches_with_renewal',
    quotaNoteI18nKey: 'admin.providers.serpapi.quotaNote',
  },
  exa: {
    id: 'exa',
    label: 'Exa',
    i18nKey: 'admin.providers.exa',
    supportsValidate: true,
    validateProbeLabel: 'POST /contents {} (401 vs unbilled 400/422)',
    validateProbeI18nKey: 'admin.providers.exa.validateProbe',
    supportsSyncUsage: false,
    syncUsageMode: 'unsupported',
    quotaDisplayKind: 'local_counting',
    quotaNoteI18nKey: 'admin.providers.exa.quotaNote',
  },
  brave: {
    id: 'brave',
    label: 'Brave',
    i18nKey: 'admin.providers.brave',
    supportsValidate: true,
    validateProbeLabel: 'GET /res/v1/web/search without q (422 discrimination)',
    validateProbeI18nKey: 'admin.providers.brave.validateProbe',
    supportsSyncUsage: true,
    syncUsageMode: 'passive',
    quotaDisplayKind: 'harvested_remaining',
    quotaNoteI18nKey: 'admin.providers.brave.quotaNote',
  },
}

/** Meta of the parsed provider; unknown ids fall through to the Tavily meta. */
export function providerMeta(value: unknown): ProviderMeta {
  return PROVIDER_META[parseProviderId(value)]
}

/**
 * Display label: known providers use the neutral brand label, an unknown
 * non-empty wire string is echoed verbatim (never mislabeled as Tavily, never
 * a crash), and absent values fall back to the legacy label.
 */
export function providerLabel(value: unknown): string {
  if (isProviderId(value)) return PROVIDER_META[value].label
  if (typeof value === 'string' && value.trim().length > 0) return value
  return PROVIDER_META[LEGACY_PROVIDER_ID].label
}

/** All four providers validate; the probe method differs per provider (M13). */
export function supportsValidate(value: unknown): boolean {
  return providerMeta(value).supportsValidate
}

/** Tavily/SerpApi sync live, Brave returns its passive snapshot, Exa none. */
export function supportsSyncUsage(value: unknown): boolean {
  return providerMeta(value).supportsSyncUsage
}

export function syncUsageMode(value: unknown): SyncUsageMode {
  return providerMeta(value).syncUsageMode
}

export function quotaDisplayKind(value: unknown): QuotaDisplayKind {
  return providerMeta(value).quotaDisplayKind
}

/** Per-provider counters shared by `/api/summary` and each summary window. */
export interface SummaryProviderCount {
  provider: string
  total_requests: number
  success_count: number
  error_count: number
  quota_exhausted_count: number
}

/** `POST /api/keys/:id/sync-usage` 200 body for the non-Tavily arms. */
export interface ProviderQuotaSyncSnapshot {
  source: ProviderQuotaSource
  provider: ProviderId
  quota_limit: number | null
  quota_remaining: number | null
  quota_renews_at: number | null
  quota_synced_at: number | null
}

/** Typed sync-usage error body: Exa 409 `sync_unsupported`, SerpApi 401 `usage_http`. */
export interface SyncUsageErrorBody {
  error: string
  detail?: string
  provider?: ProviderId
}

/**
 * Discriminated sync-usage outcome: Tavily keeps the legacy bodyless 204,
 * SerpApi/Brave return a snapshot.
 */
export type ApiKeyUsageSyncOutcome =
  | { kind: 'no_content' }
  | { kind: 'snapshot'; snapshot: ProviderQuotaSyncSnapshot }

/** Machine-readable probe discrimination codes echoed in `ValidateKeyResult.reason`. */
export type ValidateProbeReasonCode =
  | 'VALIDATION'
  | 'SUBSCRIPTION_TOKEN_INVALID'
  | 'QUOTA_LIMITED'
  | 'RATE_LIMITED'
  | 'UNAUTHORIZED'
  | 'UNBILLED_VALIDATION_ERROR'

export const VALIDATE_PROBE_REASON_CODES: readonly ValidateProbeReasonCode[] = [
  'VALIDATION',
  'SUBSCRIPTION_TOKEN_INVALID',
  'QUOTA_LIMITED',
  'RATE_LIMITED',
  'UNAUTHORIZED',
  'UNBILLED_VALIDATION_ERROR',
]

/** `ValidateKeysSummary` counter kinds plus the list-level statuses. */
export type ValidateKeyOutcomeKind =
  | 'ok'
  | 'exhausted'
  | 'invalid'
  | 'error'
  | 'already_exists'
  | 'duplicate_in_input'
  | 'pending'
  | 'unknown'

const VALIDATE_KEY_OUTCOME_KINDS: Readonly<Record<string, ValidateKeyOutcomeKind>> = {
  ok: 'ok',
  ok_exhausted: 'exhausted',
  unauthorized: 'invalid',
  forbidden: 'invalid',
  invalid: 'invalid',
  error: 'error',
  already_exists: 'already_exists',
  duplicate_in_input: 'duplicate_in_input',
  pending: 'pending',
}

/** Tolerant `ValidateKeyResult.status` classification; unknown values stay `unknown`. */
export function validateKeyOutcomeKind(value: unknown): ValidateKeyOutcomeKind {
  if (typeof value !== 'string') return 'unknown'
  return VALIDATE_KEY_OUTCOME_KINDS[value] ?? 'unknown'
}

/** SerpApi `/account` plan fields rendered into the validate `detail` string. */
export interface SerpApiPlanFields {
  plan_searches_left: string | null
  total_searches_left: string | null
  searches_per_month: string | null
  plan_renewal_date: string | null
  account_status: string | null
}

const SERPAPI_PLAN_FIELDS = [
  'plan_searches_left',
  'total_searches_left',
  'searches_per_month',
  'plan_renewal_date',
  'account_status',
] as const satisfies readonly (keyof SerpApiPlanFields)[]

type SerpApiPlanField = (typeof SERPAPI_PLAN_FIELDS)[number]

const SERPAPI_PLAN_FIELD_SET: ReadonlySet<string> = new Set(SERPAPI_PLAN_FIELDS)

/**
 * Parse the SerpApi plan detail rendered by the `/account` probe. The backend
 * keeps missing fields visibly `null`; anything that is not a plan detail
 * (other providers, probe failures) yields `null` instead of throwing.
 */
export function parseSerpApiValidateDetail(detail: unknown): SerpApiPlanFields | null {
  if (typeof detail !== 'string' || detail.length === 0) return null
  const found = new Map<string, string | null>()
  for (const segment of detail.split(';')) {
    const separator = segment.indexOf('=')
    if (separator <= 0) continue
    const field = segment.slice(0, separator).trim()
    if (!SERPAPI_PLAN_FIELD_SET.has(field)) continue
    const raw = segment.slice(separator + 1).trim()
    found.set(field, raw.length === 0 || raw === 'null' ? null : raw)
  }
  if (found.size === 0) return null
  const planField = (field: SerpApiPlanField): string | null => found.get(field) ?? null
  return {
    plan_searches_left: planField('plan_searches_left'),
    total_searches_left: planField('total_searches_left'),
    searches_per_month: planField('searches_per_month'),
    plan_renewal_date: planField('plan_renewal_date'),
    account_status: planField('account_status'),
  }
}
