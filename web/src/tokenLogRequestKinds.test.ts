import { describe, expect, it } from 'bun:test'

import {
  buildTokenLogsPagePath,
  summarizeSelectedRequestKinds,
  toggleRequestKindSelection,
  uniqueSelectedRequestKinds,
} from './tokenLogRequestKinds'

describe('token log request kind helpers', () => {
  it('deduplicates repeated request kind selections while preserving order', () => {
    expect(uniqueSelectedRequestKinds(['api:search', ' api:search ', '', 'mcp:search'])).toEqual([
      'api:search',
      'mcp:search',
    ])
  })

  it('toggles request kind keys for multi-select filters', () => {
    expect(toggleRequestKindSelection(['api:search'], 'mcp:search')).toEqual([
      'api:search',
      'mcp:search',
    ])
    expect(toggleRequestKindSelection(['api:search', 'mcp:search'], 'api:search')).toEqual([
      'mcp:search',
    ])
  })

  it('builds repeated request_kind query params for exact multi-select filters', () => {
    expect(
      buildTokenLogsPagePath({
        tokenId: 'ZjvC',
        page: 2,
        perPage: 50,
        sinceIso: '2026-03-01T00:00:00+08:00',
        untilIso: '2026-04-01T00:00:00+08:00',
        requestKinds: ['api:search', 'mcp:search', 'api:search'],
      }),
    ).toBe(
      '/api/tokens/ZjvC/logs/page?page=2&per_page=50&since=2026-03-01T00%3A00%3A00%2B08%3A00&until=2026-04-01T00%3A00%3A00%2B08%3A00&request_kind=api%3Asearch&request_kind=mcp%3Asearch',
    )
  })

  it('summarizes filter state with labels and selected counts', () => {
    const options = [
      { key: 'api:search', label: 'API | search' },
      { key: 'mcp:search', label: 'MCP | search' },
      { key: 'mcp:batch', label: 'MCP | batch' },
    ]

    expect(summarizeSelectedRequestKinds([], options)).toBe('All request types')
    expect(summarizeSelectedRequestKinds(['api:search'], options)).toBe('API | search')
    expect(summarizeSelectedRequestKinds(['api:search', 'mcp:search'], options)).toBe(
      'API | search + MCP | search',
    )
    expect(
      summarizeSelectedRequestKinds(['api:search', 'mcp:search', 'mcp:batch'], options),
    ).toBe('3 selected')
  })
})
