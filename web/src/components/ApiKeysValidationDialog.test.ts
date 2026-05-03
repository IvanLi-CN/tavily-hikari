import { describe, expect, it } from 'bun:test'

import {
  assignedProxyMatchToneClass,
  computeValidKeys,
  computeValidationCounts,
  type KeysValidationState,
} from './ApiKeysValidationDialog'

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

describe('ApiKeysValidationDialog counts', () => {
  it('counts existing keys separately and excludes them from importable keys', () => {
    const state: KeysValidationState = {
      group: 'default',
      input_lines: 4,
      valid_lines: 4,
      unique_in_input: 3,
      duplicate_in_input: 1,
      checking: false,
      importing: false,
      rows: [
        { api_key: 'tvly-new', status: 'ok', attempts: 1 },
        { api_key: 'tvly-existing', status: 'already_exists', attempts: 0 },
        { api_key: 'tvly-existing', status: 'duplicate_in_input', attempts: 0 },
      ],
    }

    expect(computeValidationCounts(state)).toMatchObject({
      ok: 1,
      existing: 1,
      duplicate: 1,
      checked: 2,
      totalToCheck: 3,
    })
    expect(computeValidKeys(state)).toEqual(['tvly-new'])
  })
})
