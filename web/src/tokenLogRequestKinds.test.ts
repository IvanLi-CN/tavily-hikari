import { describe, expect, it } from 'bun:test'

import {
  buildVisibleRequestKindOptions,
  buildTokenLogsPagePath,
  mergeRequestKindLabels,
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

  it('remembers request kind labels from options and rendered logs', () => {
    expect(
      mergeRequestKindLabels(
        { 'mcp:raw:/mcp/sse': 'MCP | /mcp/sse' },
        [{ key: 'api:search', label: 'API | search' }],
        [{ request_kind_key: 'mcp:search', request_kind_label: 'MCP | search' }],
      ),
    ).toEqual({
      'api:search': 'API | search',
      'mcp:raw:/mcp/sse': 'MCP | /mcp/sse',
      'mcp:search': 'MCP | search',
    })
  })

  it('keeps selected request kinds visible even when they drop out of the current window options', () => {
    expect(
      buildVisibleRequestKindOptions(
        ['mcp:raw:/mcp/sse', 'api:search'],
        [{ key: 'api:search', label: 'API | search' }],
        { 'mcp:raw:/mcp/sse': 'MCP | /mcp/sse' },
      ),
    ).toEqual([
      { key: 'api:search', label: 'API | search' },
      { key: 'mcp:raw:/mcp/sse', label: 'MCP | /mcp/sse' },
    ])
  })
})
