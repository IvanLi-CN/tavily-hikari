import { describe, expect, it } from 'bun:test'

import {
  classifyProviderRequestKind,
  failureKindGuidance,
  normalizeOperationalClass,
  operationalClassGuidance,
  operationalClassLabel,
  operationalClassTone,
} from './logOperationalClass'

/**
 * Copied verbatim from the committed Rust catalog `PROVIDER_REQUEST_KINDS`
 * (src/analysis.rs:895-912). The web classification is a pin, not a second
 * source of truth: the backend DTO (`requestKindBillingGroup`) stays
 * authoritative at runtime.
 */
const PROVIDER_KIND_MATRIX: ReadonlyArray<{ key: string; protocolGroup: 'api' | 'mcp' }> = [
  { key: 'api:serpapi/search', protocolGroup: 'api' },
  { key: 'api:exa/search', protocolGroup: 'api' },
  { key: 'api:exa/contents', protocolGroup: 'api' },
  { key: 'api:exa/answer', protocolGroup: 'api' },
  { key: 'api:brave/web', protocolGroup: 'api' },
  { key: 'api:brave/news', protocolGroup: 'api' },
  { key: 'api:brave/llm-context', protocolGroup: 'api' },
  { key: 'mcp:serpapi/search', protocolGroup: 'mcp' },
  { key: 'mcp:serpapi/search_table', protocolGroup: 'mcp' },
  { key: 'mcp:serpapi/search_dashboard', protocolGroup: 'mcp' },
  { key: 'mcp:brave/web_search', protocolGroup: 'mcp' },
  { key: 'mcp:brave/news_search', protocolGroup: 'mcp' },
  { key: 'mcp:brave/llm_context', protocolGroup: 'mcp' },
  { key: 'mcp:exa/web_search', protocolGroup: 'mcp' },
  { key: 'mcp:exa/web_fetch', protocolGroup: 'mcp' },
  { key: 'mcp:exa/answer', protocolGroup: 'mcp' },
]

describe('log operational class helpers', () => {
  it('normalizes known operational classes and maps tones', () => {
    expect(normalizeOperationalClass('neutral')).toBe('neutral')
    expect(operationalClassTone('neutral')).toBe('neutral')
    expect(operationalClassTone('client_error')).toBe('warning')
    expect(operationalClassTone('upstream_error')).toBe('error')
  })

  it('renders localized labels for operational classes', () => {
    expect(operationalClassLabel('success', 'en')).toBe('Success')
    expect(operationalClassLabel('neutral', 'zh')).toBe('中性')
    expect(operationalClassLabel('quota_exhausted', 'en')).toBe('Quota Exhausted')
    expect(operationalClassLabel('quota_exhausted', 'zh')).toBe('限额')
  })

  it('prefers failure-kind guidance and falls back to generic operational guidance', () => {
    expect(failureKindGuidance('mcp_accept_406', 'en')).toContain('Accept')
    expect(operationalClassGuidance('upstream_error', 'upstream_rate_limited_429', 'zh')).toContain(
      '限流',
    )
    expect(operationalClassGuidance('neutral', null, 'en')).toContain('control-plane')
  })

  it('classifies every provider request kind with its Tavily-analogue billing semantics', () => {
    for (const { key, protocolGroup } of PROVIDER_KIND_MATRIX) {
      expect(classifyProviderRequestKind(key), key).toEqual({
        protocolGroup,
        billingGroup: 'billable',
      })
    }
    expect(PROVIDER_KIND_MATRIX).toHaveLength(16)
    // Legacy Tavily kinds keep their own (mixed) catalog classification.
    expect(classifyProviderRequestKind('api:search')).toBeNull()
    expect(classifyProviderRequestKind('mcp:initialize')).toBeNull()
    expect(classifyProviderRequestKind('api:unknown-path')).toBeNull()
    expect(classifyProviderRequestKind('')).toBeNull()
    expect(classifyProviderRequestKind('junk')).toBeNull()
  })

  it('reuses the existing operational-class taxonomy for provider failures', () => {
    // src/tests/provider_failure_classification.rs:486-527 maps every provider
    // failure signal onto the house taxonomy, so no new failure kind or
    // operational class is introduced for the 16 kinds.
    expect(failureKindGuidance('upstream_rate_limited_429', 'en')).toContain('rate limiting')
    expect(failureKindGuidance('upstream_account_deactivated_401', 'zh')).toContain('账户停用')
    expect(failureKindGuidance('upstream_gateway_5xx', 'en')).toContain('gateway')
    expect(operationalClassTone('quota_exhausted')).toBe('warning')
  })
})
