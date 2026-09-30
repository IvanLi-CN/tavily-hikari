import { describe, expect, it } from 'bun:test'

import {
  mergeRequestKindCatalog,
  uniqueSelectedRequestKinds,
  type TokenLogRequestKindOption,
} from './tokenLogRequestKinds'

/**
 * Plan todo 23 coverage pin. Fixture copied VERBATIM from the committed backend
 * static catalog `PROVIDER_REQUEST_KINDS` in `src/analysis.rs:895-912` (also
 * pinned end-to-end by `src/server/tests/provider_read_surface.rs:16-33` and
 * `src/tests/provider_failure_classification.rs`). If the backend adds a
 * provider kind, this fixture and the web catalog must move together.
 */
const PROVIDER_REQUEST_KIND_FIXTURES: ReadonlyArray<{
  key: string
  label: string
  protocolGroup: 'api' | 'mcp'
}> = [
  { key: 'api:serpapi/search', label: 'API | serpapi/search', protocolGroup: 'api' },
  { key: 'api:exa/search', label: 'API | exa/search', protocolGroup: 'api' },
  { key: 'api:exa/contents', label: 'API | exa/contents', protocolGroup: 'api' },
  { key: 'api:exa/answer', label: 'API | exa/answer', protocolGroup: 'api' },
  { key: 'api:brave/web', label: 'API | brave/web', protocolGroup: 'api' },
  { key: 'api:brave/news', label: 'API | brave/news', protocolGroup: 'api' },
  { key: 'api:brave/llm-context', label: 'API | brave/llm-context', protocolGroup: 'api' },
  { key: 'mcp:serpapi/search', label: 'MCP | serpapi/search', protocolGroup: 'mcp' },
  { key: 'mcp:serpapi/search_table', label: 'MCP | serpapi/search_table', protocolGroup: 'mcp' },
  {
    key: 'mcp:serpapi/search_dashboard',
    label: 'MCP | serpapi/search_dashboard',
    protocolGroup: 'mcp',
  },
  { key: 'mcp:brave/web_search', label: 'MCP | brave/web_search', protocolGroup: 'mcp' },
  { key: 'mcp:brave/news_search', label: 'MCP | brave/news_search', protocolGroup: 'mcp' },
  { key: 'mcp:brave/llm_context', label: 'MCP | brave/llm_context', protocolGroup: 'mcp' },
  { key: 'mcp:exa/web_search', label: 'MCP | exa/web_search', protocolGroup: 'mcp' },
  { key: 'mcp:exa/web_fetch', label: 'MCP | exa/web_fetch', protocolGroup: 'mcp' },
  { key: 'mcp:exa/answer', label: 'MCP | exa/answer', protocolGroup: 'mcp' },
]

function catalogByKey(): Map<string, TokenLogRequestKindOption> {
  return new Map(mergeRequestKindCatalog([]).map((option) => [option.key, option]))
}

describe('provider request-kind label coverage (plan todo 23)', () => {
  it('registers every committed backend provider kind with its exact label and M9 group', () => {
    const catalog = catalogByKey()

    for (const fixture of PROVIDER_REQUEST_KIND_FIXTURES) {
      const option = catalog.get(fixture.key)
      expect(option, `catalog missing ${fixture.key}`).toBeDefined()
      expect(option?.label).toBe(fixture.label)
      expect(option?.protocol_group).toBe(fixture.protocolGroup)
      // src/analysis.rs `token_request_kind_billing_group`: facade operations
      // are billable (initialize/ping/tools/list/resources are the only
      // non-billable families and none of the 16 new kinds is one).
      expect(option?.billing_group).toBe('billable')
    }
    expect(PROVIDER_REQUEST_KIND_FIXTURES).toHaveLength(16)
  })

  it('keeps every provider kind selectable instead of normalizing it to unknown-path', () => {
    // Before plan todo 23 the catalog omission made normalizeRequestKindSelectionKey
    // collapse these keys into api:unknown-path / mcp:unknown-method.
    for (const fixture of PROVIDER_REQUEST_KIND_FIXTURES) {
      expect(uniqueSelectedRequestKinds([fixture.key])).toEqual([fixture.key])
    }
  })

  it('keeps the whole canonical catalog renderable (label, protocol, billing group)', () => {
    for (const option of mergeRequestKindCatalog([])) {
      expect(option.label.trim().length).toBeGreaterThan(0)
      expect(option.protocol_group === 'api' || option.protocol_group === 'mcp').toBe(true)
      expect(option.billing_group === 'billable' || option.billing_group === 'non_billable').toBe(
        true,
      )
    }
  })
})