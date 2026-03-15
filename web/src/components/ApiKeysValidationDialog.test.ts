import { describe, expect, it } from 'bun:test'

import { assignedProxyMatchToneClass } from './ApiKeysValidationDialog'

describe('ApiKeysValidationDialog assigned proxy match tone', () => {
  it('maps registration IP matches to success text', () => {
    expect(assignedProxyMatchToneClass('registration_ip')).toBe('text-success')
  })

  it('maps same-region matches to info text', () => {
    expect(assignedProxyMatchToneClass('same_region')).toBe('text-info')
  })

  it('maps fallback matches to warning text', () => {
    expect(assignedProxyMatchToneClass('other')).toBe('text-warning')
  })

  it('keeps the default text color when the match kind is absent', () => {
    expect(assignedProxyMatchToneClass(null)).toBe('')
    expect(assignedProxyMatchToneClass(undefined)).toBe('')
  })
})
