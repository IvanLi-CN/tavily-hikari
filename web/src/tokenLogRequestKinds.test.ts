import { describe, expect, it } from 'bun:test'

import {
  buildRequestKindQuickFilterSelection,
  buildVisibleRequestKindOptions,
  buildTokenLogsPagePath,
  defaultTokenLogRequestKindQuickFilters,
  deriveRequestKindQuickFilters,
  mergeRequestKindCatalog,
  mergeRequestKindOptionsByKey,
  requestKindSelectionsMatch,
  resolveEffectiveRequestKindSelection,
  resolveRequestKindOptionsRefresh,
  resolveManualRequestKindQuickFilters,
  summarizeRequestKindQuickFilters,
  summarizeSelectedRequestKinds,
  tokenLogRequestKindEmptySelectionKey,
  toggleRequestKindSelection,
  uniqueSelectedRequestKinds,
} from './tokenLogRequestKinds'

describe('token log request kind helpers', () => {
  it('deduplicates and canonicalizes request kind selections while preserving order', () => {
    expect(
      uniqueSelectedRequestKinds([
        'api:search',
        ' api:search ',
        '',
        'mcp:search',
        'mcp:raw:/mcp/sse',
        'mcp:tool:acme-lookup',
        'mcp:cancel',
      ]),
    ).toEqual([
      'api:search',
      'mcp:search',
      'mcp:unsupported-path',
      'mcp:third-party-tool',
      'mcp:unknown-method',
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
    expect(toggleRequestKindSelection(['mcp:unsupported-path'], 'mcp:raw:/mcp/sse')).toEqual([])
  })

  it('builds repeated request_kind query params for exact multi-select filters', () => {
    expect(
      buildTokenLogsPagePath({
        tokenId: 'ZjvC',
        page: 2,
        perPage: 50,
        sinceIso: '2026-03-01T00:00:00+08:00',
        untilIso: '2026-04-01T00:00:00+08:00',
        requestKinds: ['api:search', 'mcp:search', 'api:search', 'mcp:raw:/mcp/sse'],
      }),
    ).toBe(
      '/api/tokens/ZjvC/logs/page?page=2&per_page=50&since=2026-03-01T00%3A00%3A00%2B08%3A00&until=2026-04-01T00%3A00%3A00%2B08%3A00&request_kind=api%3Asearch&request_kind=mcp%3Asearch&request_kind=mcp%3Aunsupported-path',
    )
  })

  it('includes operational_class when the token log page uses an outcome filter', () => {
    expect(
      buildTokenLogsPagePath({
        tokenId: 'ZjvC',
        page: 1,
        perPage: 20,
        sinceIso: '2026-03-01T00:00:00+08:00',
        untilIso: '2026-04-01T00:00:00+08:00',
        operationalClass: 'neutral',
        requestKinds: [],
      }),
    ).toBe(
      '/api/tokens/ZjvC/logs/page?page=1&per_page=20&since=2026-03-01T00%3A00%3A00%2B08%3A00&until=2026-04-01T00%3A00%3A00%2B08%3A00&operational_class=neutral',
    )
  })

  it('preserves an active zero-match quick filter as an explicit empty query', () => {
    expect(
      buildTokenLogsPagePath({
        tokenId: 'ZjvC',
        page: 1,
        perPage: 20,
        sinceIso: '2026-03-01T00:00:00+08:00',
        untilIso: '2026-04-01T00:00:00+08:00',
        forceEmptyMatch: true,
        requestKinds: [],
      }),
    ).toBe(
      `/api/tokens/ZjvC/logs/page?page=1&per_page=20&since=2026-03-01T00%3A00%3A00%2B08%3A00&until=2026-04-01T00%3A00%3A00%2B08%3A00&request_kind=${encodeURIComponent(tokenLogRequestKindEmptySelectionKey)}`,
    )
  })

  it('summarizes filter state with labels and selected counts', () => {
    const options = [
      { key: 'api:search', label: 'API | search', protocol_group: 'api', billing_group: 'billable' },
      { key: 'mcp:search', label: 'MCP | search', protocol_group: 'mcp', billing_group: 'billable' },
      { key: 'mcp:batch', label: 'MCP | batch', protocol_group: 'mcp', billing_group: 'billable' },
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

  it('remembers request kind metadata from current and previous options', () => {
    expect(
      mergeRequestKindOptionsByKey(
        {
          'mcp:unsupported-path': {
            key: 'mcp:unsupported-path',
            label: 'MCP | unsupported path',
            protocol_group: 'mcp',
            billing_group: 'non_billable',
          },
        },
        [{ key: 'api:search', label: 'API | search', protocol_group: 'api', billing_group: 'billable' }],
      ),
    ).toEqual({
      'api:search': {
        key: 'api:search',
        label: 'API | search',
        protocol_group: 'api',
        billing_group: 'billable',
      },
      'mcp:unsupported-path': {
        key: 'mcp:unsupported-path',
        label: 'MCP | unsupported path',
        protocol_group: 'mcp',
        billing_group: 'non_billable',
      },
    })
  })

  it('keeps selected request kinds visible even when they drop out of the current window options', () => {
    expect(
      buildVisibleRequestKindOptions(
        ['mcp:unsupported-path', 'api:search'],
        [{ key: 'api:search', label: 'API | search', protocol_group: 'api', billing_group: 'billable' }],
        {
          'mcp:unsupported-path': {
            key: 'mcp:unsupported-path',
            label: 'MCP | unsupported path',
            protocol_group: 'mcp',
            billing_group: 'non_billable',
          },
        },
      ),
    ).toEqual([
      { key: 'api:search', label: 'API | search', protocol_group: 'api', billing_group: 'billable' },
      {
        key: 'mcp:unsupported-path',
        label: 'MCP | unsupported path',
        protocol_group: 'mcp',
        billing_group: 'non_billable',
      },
    ])
  })

  it('merges the canonical request kind catalog with dynamic options for filter menus', () => {
    const merged = mergeRequestKindCatalog([
      {
        key: 'mcp:third-party-tool',
        label: 'MCP | third-party tool',
        protocol_group: 'mcp',
        billing_group: 'non_billable',
        count: 7,
      },
    ])

    expect(merged.some((option) => option.key === 'api:search')).toBe(true)
    expect(merged.some((option) => option.key === 'mcp:tools/list')).toBe(true)
    expect(merged.find((option) => option.key === 'mcp:third-party-tool')).toEqual({
      key: 'mcp:third-party-tool',
      label: 'MCP | third-party tool',
      protocol_group: 'mcp',
      billing_group: 'non_billable',
      count: 7,
    })
  })

  it('builds tri-state quick-filter selections from canonical option groups', () => {
    const options = [
      { key: 'api:search', label: 'API | search', protocol_group: 'api', billing_group: 'billable' },
      { key: 'api:research-result', label: 'API | research result', protocol_group: 'api', billing_group: 'non_billable' },
      { key: 'mcp:search', label: 'MCP | search', protocol_group: 'mcp', billing_group: 'billable' },
      { key: 'mcp:tools/list', label: 'MCP | tools/list', protocol_group: 'mcp', billing_group: 'non_billable' },
    ]

    expect(
      buildRequestKindQuickFilterSelection(options, { billing: 'billable', protocol: 'mcp' }),
    ).toEqual(['mcp:search'])
    expect(
      buildRequestKindQuickFilterSelection(options, { billing: 'non_billable', protocol: 'all' }),
    ).toEqual(['api:research-result', 'mcp:tools/list'])
    expect(
      buildRequestKindQuickFilterSelection(options, defaultTokenLogRequestKindQuickFilters),
    ).toEqual([])
  })

  it('derives quick-filter state only when a selection matches one unique preset', () => {
    const options = [
      { key: 'api:search', label: 'API | search', protocol_group: 'api', billing_group: 'billable' },
      { key: 'api:research-result', label: 'API | research result', protocol_group: 'api', billing_group: 'non_billable' },
      { key: 'mcp:search', label: 'MCP | search', protocol_group: 'mcp', billing_group: 'billable' },
      { key: 'mcp:tools/list', label: 'MCP | tools/list', protocol_group: 'mcp', billing_group: 'non_billable' },
    ]

    expect(deriveRequestKindQuickFilters(['mcp:search'], options)).toEqual({
      billing: 'billable',
      protocol: 'mcp',
    })
    expect(deriveRequestKindQuickFilters(['api:search', 'mcp:search'], options)).toEqual({
      billing: 'billable',
      protocol: 'all',
    })
    expect(deriveRequestKindQuickFilters(['api:search', 'mcp:tools/list'], options)).toEqual(
      defaultTokenLogRequestKindQuickFilters,
    )
  })

  it('compares selections by exact key set and summarizes quick-filter presets', () => {
    expect(requestKindSelectionsMatch(['mcp:search', 'api:search'], ['api:search', 'mcp:search'])).toBe(true)
    expect(requestKindSelectionsMatch(['mcp:search'], ['api:search'])).toBe(false)
    expect(summarizeRequestKindQuickFilters({ billing: 'billable', protocol: 'mcp' })).toBe('Paid + MCP')
    expect(summarizeRequestKindQuickFilters({ billing: 'all', protocol: 'api' })).toBe('API request types')
    expect(summarizeRequestKindQuickFilters(defaultTokenLogRequestKindQuickFilters)).toBe('All request types')
  })

  it('preserves or re-derives quick presets after manual checkbox edits', () => {
    const options = [
      { key: 'api:search', label: 'API | search', protocol_group: 'api', billing_group: 'billable' },
      { key: 'mcp:search', label: 'MCP | search', protocol_group: 'mcp', billing_group: 'billable' },
      { key: 'mcp:tools/list', label: 'MCP | tools/list', protocol_group: 'mcp', billing_group: 'non_billable' },
    ]

    expect(
      resolveManualRequestKindQuickFilters(
        ['mcp:search'],
        { billing: 'billable', protocol: 'mcp' },
        ['mcp:search'],
        options,
      ),
    ).toEqual({ billing: 'billable', protocol: 'mcp' })
    expect(
      resolveManualRequestKindQuickFilters(
        ['mcp:search', 'api:search'],
        { billing: 'billable', protocol: 'mcp' },
        ['mcp:search'],
        options,
      ),
    ).toEqual({ billing: 'billable', protocol: 'all' })
  })

  it('uses the derived quick selection as the effective request kind set while a preset is active', () => {
    expect(
      resolveEffectiveRequestKindSelection(
        ['mcp:search'],
        { billing: 'billable', protocol: 'mcp' },
        ['mcp:search', 'mcp:batch'],
      ),
    ).toEqual(['mcp:search', 'mcp:batch'])
    expect(
      resolveEffectiveRequestKindSelection(
        ['mcp:search'],
        defaultTokenLogRequestKindQuickFilters,
        ['mcp:search', 'mcp:batch'],
      ),
    ).toEqual(['mcp:search'])
    expect(
      resolveManualRequestKindQuickFilters(
        ['mcp:search'],
        defaultTokenLogRequestKindQuickFilters,
        ['api:search'],
        [
          { key: 'api:search', label: 'API | search', protocol_group: 'api', billing_group: 'billable' },
          { key: 'mcp:search', label: 'MCP | search', protocol_group: 'mcp', billing_group: 'billable' },
          { key: 'mcp:tools/list', label: 'MCP | tools/list', protocol_group: 'mcp', billing_group: 'non_billable' },
        ],
      ),
    ).toEqual({ billing: 'billable', protocol: 'mcp' })
  })

  it('detects when refreshed options expand an active quick preset selection', () => {
    const refreshed = resolveRequestKindOptionsRefresh(
      [
        { key: 'mcp:search', label: 'MCP | search', protocol_group: 'mcp', billing_group: 'billable' },
        { key: 'mcp:extract', label: 'MCP | extract', protocol_group: 'mcp', billing_group: 'billable' },
      ],
      ['mcp:search'],
      { billing: 'billable', protocol: 'mcp' },
      ['mcp:search'],
      false,
    )

    expect(refreshed.quickSelection).toEqual(['mcp:search', 'mcp:extract'])
    expect(refreshed.effectiveSelection).toEqual(['mcp:search', 'mcp:extract'])
    expect(refreshed.hasEmptyMatch).toBe(false)
    expect(refreshed.selectionChanged).toBe(true)
  })

  it('preserves a zero-match quick preset when refreshed options still have no matches', () => {
    const refreshed = resolveRequestKindOptionsRefresh(
      [{ key: 'api:search', label: 'API | search', protocol_group: 'api', billing_group: 'billable' }],
      [],
      { billing: 'non_billable', protocol: 'mcp' },
      [],
      true,
    )

    expect(refreshed.quickSelection).toEqual([])
    expect(refreshed.effectiveSelection).toEqual([])
    expect(refreshed.hasEmptyMatch).toBe(true)
    expect(refreshed.selectionChanged).toBe(false)
  })
})
