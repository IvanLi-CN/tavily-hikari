/**
 * Browser demo fixtures for the Brave REST facade + local MCP facade
 * (plan todo 25). Ground truth (committed, READ-ONLY):
 * `src/bin/mock_brave.rs` — `/res/v1/web/search` (with the `X-RateLimit-*`
 * month-window headers), `/res/v1/news/search`, `/res/v1/llm/context` success
 * envelopes; `src/server/provider_mcp_brave.rs` — the local MCP catalog
 * (`brave_web_search`, `brave_news_search`, `brave_llm_context`) with its exact
 * inputSchemas. No network access ever runs in demo mode.
 */
import type { DemoMcpToolDefinition } from './demoProviders'

/** `GET /api/brave/res/v1/web/search` success body. */
export function demoBraveWebFixture(): Record<string, unknown> {
  return {
    type: 'search',
    query: { original: 'mock', more_results_available: false },
    web: {
      results: [{ title: 'Mock', url: 'https://example.com/1', description: 'mock desc' }],
    },
  }
}

/** `GET /api/brave/res/v1/news/search` success body. */
export function demoBraveNewsFixture(): Record<string, unknown> {
  return {
    type: 'news',
    results: [{ title: 'Mock News', url: 'https://example.com/n1', description: 'mock' }],
  }
}

/** `GET /api/brave/res/v1/llm/context` success body. */
export function demoBraveLlmContextFixture(): Record<string, unknown> {
  return {
    grounding: {
      generic: [{ url: 'https://example.com/1', title: 'Mock', snippets: ['mock snippet'] }],
    },
    sources: {},
  }
}

/**
 * Passive-harvest demo headers, byte-shape from `rate_limit_headers()` after
 * the first successful web search (monthly remaining = 999).
 */
export const BRAVE_RATE_LIMIT_HEADERS: Record<string, string> = {
  'x-ratelimit-limit': '50, 1000',
  'x-ratelimit-policy': '1;w=1, 1000;w=2592000',
  'x-ratelimit-remaining': '1, 999',
  'x-ratelimit-reset': '1, 2592000',
}

/** `/brave/mcp` tool catalog mirroring `BRAVE_LOCAL_MCP_TOOLS` inputSchemas. */
export const BRAVE_MCP_TOOLS: DemoMcpToolDefinition[] = [
  {
    name: 'brave_web_search',
    description: 'Brave Search API — Web search',
    inputSchema: {
      type: 'object',
      properties: {
        query: { type: 'string', description: 'The search query (required).' },
        country: {
          type: 'string',
          description: 'An ISO 3166-1 alpha-2 country code for market localization.',
        },
        search_lang: {
          type: 'string',
          description: 'An ISO 639-1 language code to prefer for search results.',
        },
        ui_lang: { type: 'string', description: 'An ISO 639-1 language code for UI elements.' },
        count: {
          type: 'integer',
          description: 'Number of results to return (1-20).',
          minimum: 1,
          maximum: 20,
          default: 10,
        },
        offset: {
          type: 'integer',
          description: 'Pagination offset (0-9).',
          minimum: 0,
          maximum: 9,
          default: 0,
        },
        safesearch: {
          type: 'string',
          description: 'Safe-search filtering level.',
          enum: ['off', 'moderate', 'strict'],
          default: 'moderate',
        },
        freshness: {
          type: 'string',
          description: 'Filter results by a discovery-time window (e.g. pd, pw, pm, or a date range).',
        },
        result_filter: {
          type: 'array',
          items: { type: 'string' },
          description: 'Comma/repeat content filters (web, news, videos, faq, discussions, infobox).',
        },
        extra_snippets: {
          type: 'boolean',
          description: 'Include extra result snippets in the response.',
          default: false,
        },
        summary: {
          type: 'boolean',
          description: 'Include a summary of the result content.',
          default: false,
        },
      },
      required: ['query'],
      additionalProperties: false,
    },
  },
  {
    name: 'brave_news_search',
    description: 'Brave Search API — News search',
    inputSchema: {
      type: 'object',
      properties: {
        query: { type: 'string', description: 'The news search query (required).' },
        count: {
          type: 'integer',
          description: 'Number of results to return (1-50).',
          minimum: 1,
          maximum: 50,
          default: 20,
        },
        offset: {
          type: 'integer',
          description: 'Pagination offset (0-9).',
          minimum: 0,
          maximum: 9,
          default: 0,
        },
        freshness: { type: 'string', description: 'Filter results by a discovery-time window.' },
        country: {
          type: 'string',
          description: 'An ISO 3166-1 alpha-2 country code for market localization.',
        },
        search_lang: {
          type: 'string',
          description: 'An ISO 639-1 language code to prefer for results.',
        },
        safesearch: {
          type: 'string',
          description: 'Safe-search filtering level.',
          enum: ['off', 'moderate', 'strict'],
          default: 'moderate',
        },
      },
      required: ['query'],
      additionalProperties: false,
    },
  },
  {
    name: 'brave_llm_context',
    description: 'Brave Search API — LLM context',
    inputSchema: {
      type: 'object',
      properties: {
        query: { type: 'string', description: 'The query to build LLM context for (required).' },
        count: {
          type: 'integer',
          description: 'Number of source results to consider (1-50).',
          minimum: 1,
          maximum: 50,
          default: 20,
        },
        maximum_number_of_urls: {
          type: 'integer',
          description: 'Maximum distinct URLs to include (1-50).',
          minimum: 1,
          maximum: 50,
        },
        maximum_number_of_tokens: {
          type: 'integer',
          description: 'Token budget for the assembled context (1024-32768).',
          minimum: 1024,
          maximum: 32768,
        },
        maximum_number_of_snippets: {
          type: 'integer',
          description: 'Maximum number of snippets to include per source.',
        },
        context_threshold_mode: {
          type: 'string',
          description: 'Relevance-threshold mode used to select context snippets.',
          enum: ['low', 'medium', 'high'],
        },
        freshness: { type: 'string', description: 'Filter results by a discovery-time window.' },
      },
      required: ['query'],
      additionalProperties: false,
    },
  },
]

/** Fixture per Brave tool; mirrors each tool's REST wire target. */
export function demoBraveMcpToolFixture(name: string): Record<string, unknown> | null {
  if (name === 'brave_web_search') return demoBraveWebFixture()
  if (name === 'brave_news_search') return demoBraveNewsFixture()
  if (name === 'brave_llm_context') return demoBraveLlmContextFixture()
  return null
}