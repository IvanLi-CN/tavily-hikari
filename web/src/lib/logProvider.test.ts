import { describe, expect, it } from 'bun:test'

import {
  filterLogsByProvider,
  logProviderAllFilterValue,
  logProviderDisplayLabel,
  mergeProviderFacetOptions,
  providerFacetOptionsFromLogs,
  providerFilterQueryValue,
  resolveLogProvider,
} from './logProvider'

const mixedProviderLogs: ReadonlyArray<{ id: number; provider?: string | null }> = [
  { id: 1, provider: 'brave' },
  { id: 2, provider: 'brave' },
  { id: 3, provider: 'exa' },
  { id: 4, provider: 'serpapi' },
  { id: 5 }, // legacy row: no provider field at all
  { id: 6, provider: 'tavily' },
]

describe('log provider dimension helpers (plan todo 23)', () => {
  it('coalesces absent and unknown payload providers to the legacy Tavily id', () => {
    expect(resolveLogProvider(undefined)).toBe('tavily')
    expect(resolveLogProvider(null)).toBe('tavily')
    expect(resolveLogProvider('')).toBe('tavily')
    expect(resolveLogProvider('openai')).toBe('tavily')
    expect(resolveLogProvider('brave')).toBe('brave')
  })

  it('renders brand labels while echoing unknown raw strings verbatim', () => {
    expect(logProviderDisplayLabel(undefined)).toBe('Tavily')
    expect(logProviderDisplayLabel('serpapi')).toBe('SerpApi')
    expect(logProviderDisplayLabel('exa')).toBe('Exa')
    expect(logProviderDisplayLabel('brave')).toBe('Brave')
    expect(logProviderDisplayLabel('openai')).toBe('openai')
  })

  it('derives provider facet options from mixed logs in stable provider order', () => {
    expect(providerFacetOptionsFromLogs(mixedProviderLogs)).toEqual([
      { value: 'tavily', count: 2 },
      { value: 'serpapi', count: 1 },
      { value: 'exa', count: 1 },
      { value: 'brave', count: 2 },
    ])
  })

  it('prefers todo-20 facet counts and backfills providers that only appear in rows', () => {
    // The facets API returns whole-window counts; a provider present in the
    // current page but absent from a stale facet payload must stay selectable.
    expect(
      mergeProviderFacetOptions([{ value: 'brave', count: 7 }], mixedProviderLogs),
    ).toEqual([
      { value: 'tavily', count: 2 },
      { value: 'serpapi', count: 1 },
      { value: 'exa', count: 1 },
      { value: 'brave', count: 7 },
    ])
    expect(mergeProviderFacetOptions(undefined, mixedProviderLogs)).toEqual(
      providerFacetOptionsFromLogs(mixedProviderLogs),
    )
    expect(mergeProviderFacetOptions([], [])).toEqual([])
  })

  it('filters rows by provider with the legacy NULL row counting as Tavily', () => {
    expect(filterLogsByProvider(mixedProviderLogs, null).map((row) => row.id)).toEqual([
      1, 2, 3, 4, 5, 6,
    ])
    expect(filterLogsByProvider(mixedProviderLogs, 'exa').map((row) => row.id)).toEqual([3])
    expect(filterLogsByProvider(mixedProviderLogs, 'tavily').map((row) => row.id)).toEqual([5, 6])
  })

  it('emits the ?provider= value for server-side filters and omits the all sentinel', () => {
    expect(providerFilterQueryValue('exa')).toBe('exa')
    expect(providerFilterQueryValue(' tavily ')).toBe('tavily')
    expect(providerFilterQueryValue(null)).toBeUndefined()
    expect(providerFilterQueryValue('')).toBeUndefined()
    expect(providerFilterQueryValue(logProviderAllFilterValue)).toBeUndefined()
  })
})