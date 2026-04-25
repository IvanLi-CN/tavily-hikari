import { describe, expect, it } from 'bun:test'

import {
  failureKindGuidance,
  normalizeOperationalClass,
  operationalClassGuidance,
  operationalClassLabel,
  operationalClassTone,
} from './logOperationalClass'

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
})
