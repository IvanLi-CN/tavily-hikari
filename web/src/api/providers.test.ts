import { describe, expect, it } from 'bun:test'

import {
  LEGACY_PROVIDER_ID,
  PROVIDER_IDS,
  PROVIDER_META,
  VALIDATE_PROBE_REASON_CODES,
  normalizePayloadProvider,
  parseProviderId,
  parseSerpApiValidateDetail,
  providerLabel,
  providerMeta,
  quotaDisplayKind,
  supportsSyncUsage,
  supportsValidate,
  syncUsageMode,
  validateKeyOutcomeKind,
  type ApiKeyUsageSyncOutcome,
  type ProviderQuotaSyncSnapshot,
  type SummaryProviderCount,
  type SyncUsageErrorBody,
} from './providers'
import type {
  ApiKeyStats,
  RequestLog,
  Summary,
  SummaryWindowMetrics,
  ValidateKeyResult,
} from './runtime'
import { PROVIDER_IDS as barrelProviderIds, providerLabel as barrelProviderLabel } from './index'

// Wire key list pinned from the committed Rust ApiKeyView (src/server/dto.rs:13-36)
// BEFORE plan todo 21. The provider dimension must stay additive: no rename,
// no removal, no camelCase drift on these legacy fields.
const LEGACY_API_KEY_VIEW_KEYS = [
  'id',
  'status',
  'group',
  'registration_ip',
  'registration_region',
  'status_changed_at',
  'last_used_at',
  'deleted_at',
  'quota_limit',
  'quota_remaining',
  'quota_synced_at',
  'total_requests',
  'success_count',
  'error_count',
  'quota_exhausted_count',
  'quarantine',
  'transient_backoff',
]

function legacyApiKeyStats(id: string): ApiKeyStats {
  return {
    id,
    status: 'active',
    group: null,
    registration_ip: null,
    registration_region: null,
    status_changed_at: null,
    last_used_at: null,
    deleted_at: null,
    quota_limit: 5000,
    quota_remaining: 4000,
    quota_synced_at: null,
    total_requests: 10,
    success_count: 9,
    error_count: 1,
    quota_exhausted_count: 0,
    quarantine: null,
    transient_backoff: null,
  }
}

function requestLog(id: number, provider?: string): RequestLog {
  return {
    id,
    key_id: 'key-1',
    auth_token_id: null,
    method: 'POST',
    path: '/api/tavily/search',
    query: null,
    http_status: 200,
    mcp_status: null,
    result_status: 'success',
    created_at: 1_700_000_000,
    error_message: null,
    request_body: null,
    response_body: null,
    forwarded_headers: [],
    dropped_headers: [],
    operationalClass: 'success',
    requestKindProtocolGroup: 'api',
    requestKindBillingGroup: 'billable',
    ...(provider === undefined ? {} : { provider }),
  }
}

function sortedKeys(value: object): string[] {
  return Object.keys(value).sort()
}

describe('provider id union parsing', () => {
  it('parses every known provider id to itself', () => {
    // Given the four wire keys emitted by src/providers/id.rs
    // When/Then each parses to the identical typed id
    for (const id of ['tavily', 'serpapi', 'exa', 'brave']) {
      expect(parseProviderId(id)).toBe(id)
    }
    expect(PROVIDER_IDS).toEqual(['tavily', 'serpapi', 'exa', 'brave'])
  })

  it('falls back to the typed legacy provider for unknown strings without throwing', () => {
    // Given a backend that shipped a provider the frontend does not know yet
    // When/Then the parser returns the typed fallback instead of throwing
    expect(LEGACY_PROVIDER_ID).toBe('tavily')
    expect(parseProviderId('openai')).toBe('tavily')
    expect(parseProviderId('TAVILY')).toBe('tavily')
    expect(parseProviderId('')).toBe('tavily')
    expect(parseProviderId(null)).toBe('tavily')
    expect(parseProviderId(undefined)).toBe('tavily')
    expect(parseProviderId(7)).toBe('tavily')
    expect(parseProviderId({ provider: 'exa' })).toBe('tavily')
    // And the label helper degrades to the raw wire string, never a crash
    expect(providerLabel('openai')).toBe('openai')
    expect(providerLabel('')).toBe('Tavily')
    expect(providerLabel(null)).toBe('Tavily')
    expect(providerLabel(undefined)).toBe('Tavily')
  })

  it('coalesces legacy payloads without a provider field to tavily', () => {
    // Given a pre-todo-10 payload row that carries no provider key at all
    const legacyRow: { id: string; provider?: unknown } = { id: 'key-legacy' }
    // When/Then the normalization helper stamps the legacy provider
    expect(normalizePayloadProvider(legacyRow)).toBe('tavily')
    expect(normalizePayloadProvider({ id: 'x', provider: null })).toBe('tavily')
    expect(normalizePayloadProvider(undefined)).toBe('tavily')
    expect(normalizePayloadProvider({ provider: 'brave' })).toBe('brave')
    expect(normalizePayloadProvider({ provider: 'openai' })).toBe('tavily')
  })
})

describe('provider meta maps', () => {
  it('covers exactly the four providers with complete metadata', () => {
    // Given the exported meta map
    // Then it holds one entry per provider id and nothing else
    expect(sortedKeys(PROVIDER_META)).toEqual([...PROVIDER_IDS].sort())
    for (const id of PROVIDER_IDS) {
      const meta = PROVIDER_META[id]
      expect(meta.id).toBe(id)
      expect(meta.label.length).toBeGreaterThan(0)
      expect(meta.i18nKey.length).toBeGreaterThan(0)
      expect(meta.validateProbeLabel.length).toBeGreaterThan(0)
      expect(meta.validateProbeI18nKey.length).toBeGreaterThan(0)
      expect(meta.quotaNoteI18nKey.length).toBeGreaterThan(0)
      // Language policy: neutral brand labels only, never a third language.
      expect(providerLabel(id)).toBe(meta.label)
      expect(providerMeta(id)).toBe(meta)
    }
  })

  it('exposes language-neutral brand labels', () => {
    // Given brand proper nouns are identical in en and zh
    // Then the labels stay language-neutral (tokenLogRequestKinds convention)
    expect(PROVIDER_META.tavily.label).toBe('Tavily')
    expect(PROVIDER_META.serpapi.label).toBe('SerpApi')
    expect(PROVIDER_META.exa.label).toBe('Exa')
    expect(PROVIDER_META.brave.label).toBe('Brave')
  })

  it('re-exports the provider surface through the api barrel', () => {
    // Given web/src/api/index.ts is the public barrel
    // Then the provider helpers resolve from it unchanged
    expect(barrelProviderIds).toBe(PROVIDER_IDS)
    expect(barrelProviderLabel('exa')).toBe('Exa')
  })
})

describe('provider capability predicates', () => {
  it('supports validation on all four providers with probe copy', () => {
    // Given M13 zero-cost probes exist for every provider
    // Then supportsValidate is true across the matrix
    for (const id of PROVIDER_IDS) {
      expect(supportsValidate(id)).toBe(true)
      expect(PROVIDER_META[id].supportsValidate).toBe(true)
    }
    // And the probe copy names the endpoint actually probed (M13 dialog requirement)
    expect(PROVIDER_META.tavily.validateProbeLabel).toContain('/usage')
    expect(PROVIDER_META.serpapi.validateProbeLabel).toContain('/account')
    expect(PROVIDER_META.exa.validateProbeLabel).toContain('/contents')
    expect(PROVIDER_META.brave.validateProbeLabel).toContain('/res/v1/web/search')
    // Unknown providers fall through to the legacy tavily branch (backend parity)
    expect(supportsValidate('openai')).toBe(true)
  })

  it('maps sync-usage support and mode per provider', () => {
    // Given admin_auth.rs dispatch: tavily job flow, serpapi live, brave passive, exa 409
    expect(supportsSyncUsage('tavily')).toBe(true)
    expect(supportsSyncUsage('serpapi')).toBe(true)
    expect(supportsSyncUsage('brave')).toBe(true)
    expect(supportsSyncUsage('exa')).toBe(false)
    expect(syncUsageMode('tavily')).toBe('live')
    expect(syncUsageMode('serpapi')).toBe('live')
    expect(syncUsageMode('brave')).toBe('passive')
    expect(syncUsageMode('exa')).toBe('unsupported')
    expect(syncUsageMode('openai')).toBe('live')
  })

  it('maps the quota display kind per provider', () => {
    // Given per-provider quota semantics from todos 10/14/19
    expect(quotaDisplayKind('tavily')).toBe('credits')
    expect(quotaDisplayKind('serpapi')).toBe('searches_with_renewal')
    expect(quotaDisplayKind('brave')).toBe('harvested_remaining')
    expect(quotaDisplayKind('exa')).toBe('local_counting')
    expect(quotaDisplayKind('openai')).toBe('credits')
  })

  it('classifies validate result statuses into summary kinds', () => {
    // Given ValidateKeysSummary counts ok/exhausted/invalid/error separately
    // from the list-level already_exists/duplicate_in_input/pending statuses
    expect(validateKeyOutcomeKind('ok')).toBe('ok')
    expect(validateKeyOutcomeKind('ok_exhausted')).toBe('exhausted')
    expect(validateKeyOutcomeKind('unauthorized')).toBe('invalid')
    expect(validateKeyOutcomeKind('forbidden')).toBe('invalid')
    expect(validateKeyOutcomeKind('invalid')).toBe('invalid')
    expect(validateKeyOutcomeKind('error')).toBe('error')
    expect(validateKeyOutcomeKind('already_exists')).toBe('already_exists')
    expect(validateKeyOutcomeKind('duplicate_in_input')).toBe('duplicate_in_input')
    expect(validateKeyOutcomeKind('pending')).toBe('pending')
    expect(validateKeyOutcomeKind('nonsense')).toBe('unknown')
    expect(validateKeyOutcomeKind(null)).toBe('unknown')
  })
})

describe('mixed-provider payload normalization (QA happy path)', () => {
  it('normalizes a mixed-provider ApiKeyStats fixture including a legacy row', () => {
    // Given one row per provider plus a legacy row without the provider field
    const legacy = legacyApiKeyStats('key-legacy')
    const serpapi: ApiKeyStats = {
      ...legacyApiKeyStats('key-serpapi'),
      provider: 'serpapi',
      quota_limit: 5000,
      quota_remaining: 4321,
      quota_renews_at: 1_792_944_000,
      quota_synced_at: 1_700_000_002,
    }
    const brave: ApiKeyStats = {
      ...legacyApiKeyStats('key-brave'),
      provider: 'brave',
      quota_limit: null,
      quota_remaining: 900,
    }
    const exa: ApiKeyStats = { ...legacyApiKeyStats('key-exa'), provider: 'exa' }
    const rows = [legacy, serpapi, brave, exa]

    // Then every row resolves to a typed provider id and label
    expect(rows.map((row) => normalizePayloadProvider(row))).toEqual([
      'tavily',
      'serpapi',
      'brave',
      'exa',
    ])
    expect(rows.map((row) => providerLabel(row.provider))).toEqual([
      'Tavily',
      'SerpApi',
      'Brave',
      'Exa',
    ])
    // And the additive-only contract holds: the legacy row keeps exactly the
    // pre-todo-21 wire keys while the provider rows add only the new ones.
    expect(sortedKeys(legacy)).toEqual([...LEGACY_API_KEY_VIEW_KEYS].sort())
    expect(sortedKeys(serpapi)).toEqual(
      [...LEGACY_API_KEY_VIEW_KEYS, 'provider', 'quota_renews_at'].sort(),
    )
    expect(quotaDisplayKind(serpapi.provider)).toBe('searches_with_renewal')
    expect(quotaDisplayKind(brave.provider)).toBe('harvested_remaining')
    expect(supportsSyncUsage(exa.provider)).toBe(false)
  })

  it('keeps the additive provider dimension optional on logs and summaries', () => {
    // Given a legacy request log without provider and a stamped one
    expect(requestLog(1).provider).toBeUndefined()
    expect(requestLog(2, 'brave').provider).toBe('brave')
    expect(normalizePayloadProvider(requestLog(1))).toBe('tavily')

    // And a summary/windows payload mirroring the todo-20 additive shape
    const breakdown: SummaryProviderCount = {
      provider: 'serpapi',
      total_requests: 12,
      success_count: 10,
      error_count: 2,
      quota_exhausted_count: 1,
    }
    expect(sortedKeys(breakdown)).toEqual([
      'error_count',
      'provider',
      'quota_exhausted_count',
      'success_count',
      'total_requests',
    ])
    const legacySummary: Summary = {
      total_requests: 12,
      success_count: 10,
      error_count: 2,
      quota_exhausted_count: 1,
      active_keys: 1,
      exhausted_keys: 0,
      quarantined_keys: 0,
      temporary_isolated_keys: 0,
      last_activity: null,
      total_quota_limit: 5000,
      total_quota_remaining: 4000,
    }
    expect(legacySummary.providers).toBeUndefined()
    const segmented: Summary = { ...legacySummary, providers: [breakdown] }
    expect(segmented.providers?.[0]?.provider).toBe('serpapi')
    const window: SummaryWindowMetrics = {
      total_requests: 3,
      success_count: 3,
      error_count: 0,
      quota_exhausted_count: 0,
      valuable_success_count: 3,
      valuable_failure_count: 0,
      other_success_count: 0,
      other_failure_count: 0,
      unknown_count: 0,
      upstream_exhausted_key_count: 0,
      new_keys: 0,
      new_quarantines: 0,
      providers: [{ ...breakdown, provider: 'brave' }],
    }
    expect(window.providers?.[0]?.provider).toBe('brave')
  })
})

describe('validate and sync-usage DTO mirrors', () => {
  it('mirrors the serpapi live sync snapshot field names exactly', () => {
    // Given ProviderQuotaSyncSnapshot from admin_auth.rs:408-416 (snake_case wire)
    const snapshot: ProviderQuotaSyncSnapshot = {
      source: 'live',
      provider: 'serpapi',
      quota_limit: 5000,
      quota_remaining: 4321,
      quota_renews_at: 1_792_944_000,
      quota_synced_at: 1_700_000_002,
    }
    expect(sortedKeys(snapshot)).toEqual([
      'provider',
      'quota_limit',
      'quota_remaining',
      'quota_renews_at',
      'quota_synced_at',
      'source',
    ])
    const outcome: ApiKeyUsageSyncOutcome = { kind: 'snapshot', snapshot }
    expect(outcome.kind).toBe('snapshot')
    // And the tavily legacy branch stays a bodyless 204
    const tavilyOutcome: ApiKeyUsageSyncOutcome = { kind: 'no_content' }
    expect(tavilyOutcome.kind).toBe('no_content')
  })

  it('mirrors the brave passive snapshot and the typed exa 409 body', () => {
    // Given brave echoes the last harvested snapshot without any live call
    const passive: ProviderQuotaSyncSnapshot = {
      source: 'passive',
      provider: 'brave',
      quota_limit: null,
      quota_remaining: 900,
      quota_renews_at: null,
      quota_synced_at: 1_700_000_003,
    }
    expect(passive.source).toBe('passive')
    expect(passive.quota_limit).toBeNull()
    expect(syncUsageMode(passive.provider)).toBe('passive')

    // And exa rejects with a typed 409 carrying the provider echo
    const exaConflict: SyncUsageErrorBody = {
      error: 'sync_unsupported',
      detail: 'exa has no upstream balance API; usage is tracked locally per request',
      provider: 'exa',
    }
    expect(sortedKeys(exaConflict)).toEqual(['detail', 'error', 'provider'])
    expect(supportsSyncUsage(exaConflict.provider)).toBe(false)
    // SerpApi auth failure keeps the typed usage_http error body
    const serpapiUnauthorized: SyncUsageErrorBody = { error: 'usage_http', detail: 'upstream 401' }
    expect(serpapiUnauthorized.provider).toBeUndefined()
  })

  it('mirrors the provider-aware ValidateKeyResult fields', () => {
    // Given ValidateKeyResult from versions_and_proxy_helpers.rs:702-730
    const braveInvalid: ValidateKeyResult = {
      api_key: 'BSA-invalid',
      status: 'invalid',
      provider: 'brave',
      reason: 'SUBSCRIPTION_TOKEN_INVALID',
      detail: 'zero-cost token probe answered 422: {"error":{"code":"SUBSCRIPTION_TOKEN_INVALID"}}',
    }
    expect(braveInvalid.provider).toBe('brave')
    expect(braveInvalid.reason).toBe('SUBSCRIPTION_TOKEN_INVALID')
    expect(validateKeyOutcomeKind(braveInvalid.status)).toBe('invalid')

    const braveValid: ValidateKeyResult = {
      api_key: 'BSA-valid',
      status: 'ok',
      provider: 'brave',
      reason: 'VALIDATION',
    }
    expect(validateKeyOutcomeKind(braveValid.status)).toBe('ok')
    // Brave 402/429 discrimination codes stay typed and enumerable
    expect([...VALIDATE_PROBE_REASON_CODES]).toContain('QUOTA_LIMITED')
    expect([...VALIDATE_PROBE_REASON_CODES]).toContain('RATE_LIMITED')

    // And the exa probe distinguishes 401 from an unbilled validation error
    const exaUnauthorized: ValidateKeyResult = {
      api_key: 'exa-bad',
      status: 'unauthorized',
      provider: 'exa',
      reason: 'UNAUTHORIZED',
    }
    expect(validateKeyOutcomeKind(exaUnauthorized.status)).toBe('invalid')
    const exaValid: ValidateKeyResult = {
      api_key: 'exa-good',
      status: 'ok',
      provider: 'exa',
      reason: 'UNBILLED_VALIDATION_ERROR',
    }
    expect(validateKeyOutcomeKind(exaValid.status)).toBe('ok')
    for (const reason of [
      braveInvalid.reason,
      braveValid.reason,
      exaUnauthorized.reason,
      exaValid.reason,
    ]) {
      expect(VALIDATE_PROBE_REASON_CODES).toContain(reason)
    }

    // And a legacy tavily result keeps the byte-identical shape (no provider echo)
    const legacyTavily: ValidateKeyResult = { api_key: 'tvly-x', status: 'ok' }
    expect(legacyTavily.provider).toBeUndefined()
    expect(legacyTavily.reason).toBeUndefined()
  })

  it('parses the serpapi plan fields out of the validate detail string', () => {
    // Given api_keys_and_logs.rs:385-392 renders the /account plan fields
    const detail =
      'plan_searches_left=4321; total_searches_left=4321; searches_per_month=5000; plan_renewal_date=2026-10-23; account_status=active'
    const parsed = parseSerpApiValidateDetail(detail)
    expect(parsed).toEqual({
      plan_searches_left: '4321',
      total_searches_left: '4321',
      searches_per_month: '5000',
      plan_renewal_date: '2026-10-23',
      account_status: 'active',
    })
    // Missing plan fields stay visibly null instead of being dropped
    const sparse = parseSerpApiValidateDetail(
      'plan_searches_left=null; total_searches_left=10; searches_per_month=null; plan_renewal_date=null; account_status=null',
    )
    expect(sparse).toEqual({
      plan_searches_left: null,
      total_searches_left: '10',
      searches_per_month: null,
      plan_renewal_date: null,
      account_status: null,
    })
    // Non-serpapi details yield null rather than throwing
    expect(parseSerpApiValidateDetail('provider probe failed with 422: {}')).toBeNull()
    expect(parseSerpApiValidateDetail(null)).toBeNull()
    expect(parseSerpApiValidateDetail(undefined)).toBeNull()
  })
})
