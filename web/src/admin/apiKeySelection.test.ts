import { describe, expect, it } from 'bun:test'

import { retainVisibleApiKeySelection } from './apiKeySelection'

describe('retainVisibleApiKeySelection', () => {
  it('keeps all selected ids when they remain visible after refresh', () => {
    expect(retainVisibleApiKeySelection(['KjIc', 'detN'], ['KjIc', 'detN', '5RdV'])).toEqual([
      'KjIc',
      'detN',
    ])
  })

  it('drops selected ids that disappear from the refreshed list after delete', () => {
    expect(retainVisibleApiKeySelection(['KjIc', 'detN', '5RdV'], ['detN', '9fbd'])).toEqual([
      'detN',
    ])
  })

  it('clears the selection when a filtered bulk clear removes every selected row from view', () => {
    expect(retainVisibleApiKeySelection(['KjIc', 'detN'], ['9fbd'])).toEqual([])
  })

  it('keeps only the still-visible subset for mixed bulk-action outcomes', () => {
    expect(
      retainVisibleApiKeySelection(
        ['KjIc', 'detN', '5RdV', '9fbd'],
        ['detN', '5RdV', 'extra', '9fbd'],
      ),
    ).toEqual(['detN', '5RdV', '9fbd'])
  })

  it('normalizes blanks and duplicates while preserving original selected order', () => {
    expect(retainVisibleApiKeySelection([' KjIc ', '', 'KjIc', ' detN '], ['detN', 'KjIc'])).toEqual([
      'KjIc',
      'detN',
    ])
  })
})
