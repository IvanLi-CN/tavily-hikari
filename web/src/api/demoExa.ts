/**
 * Browser demo fixtures for the Exa REST facade + local MCP facade
 * (plan todo 25). Ground truth (committed, READ-ONLY):
 * `src/bin/mock_exa.rs` — `/search`, `/contents`, `/answer` success envelopes
 * (per-URL `statuses` included); `src/server/provider_facade.rs` — the T13
 * streaming typed-400 message; `src/server/provider_mcp_exa.rs` — the local
 * MCP catalog (`web_search_exa`, `web_fetch_exa`, `exa_answer`) with its exact
 * inputSchemas and the `{ content, structuredContent }` result envelope.
 * No network access ever runs in demo mode.
 */
import type { DemoMcpToolDefinition } from './demoProviders'

/** `POST /api/exa/search` success body, byte-shape from `handle_search`. */
export function demoExaSearchFixture(): Record<string, unknown> {
  return {
    requestId: 'mock',
    results: [
      {
        title: 'Mock',
        url: 'https://example.com/1',
        text: 'mock text',
        publishedDate: '2026-01-01T00:00:00.000Z',
      },
    ],
    costDollars: { total: 0.007 },
    searchTime: 123.4,
  }
}

/** `POST /api/exa/contents` success body incl. the per-URL statuses envelope. */
export function demoExaContentsFixture(): Record<string, unknown> {
  return {
    results: [{ title: 'Mock', url: 'https://example.com/1', text: 'mock text' }],
    statuses: [{ id: 'https://example.com/1', status: 'success', source: 'cached' }],
  }
}

/** `POST /api/exa/answer` success body. */
export function demoExaAnswerFixture(): Record<string, unknown> {
  return {
    requestId: 'mock',
    answer: 'mock answer',
    citations: [{ url: 'https://example.com/1' }],
    costDollars: { total: 0.005 },
  }
}

/** T13 typed-400 message (`provider_facade.rs EXA_STREAM_REJECT_MESSAGE`). */
export const EXA_STREAM_REJECT_MESSAGE = 'streaming not supported; omit stream or set false'

/**
 * Truthy-`stream` detector mirroring `exa_stream_value_is_truthy`: anything
 * except an explicit `false`/`null` would make Exa answer an SSE stream, which
 * the facade never supports, so every other present value rejects.
 */
export function isDemoExaStreamTruthy(value: unknown): boolean {
  return value !== undefined && value !== false && value !== null
}

/** `/exa/mcp` tool catalog mirroring `EXA_LOCAL_MCP_TOOLS` inputSchemas. */
export const EXA_MCP_TOOLS: DemoMcpToolDefinition[] = [
  {
    name: 'web_search_exa',
    description:
      'Search the web with Exa — live web search returning links, metadata, and optional page contents',
    inputSchema: {
      type: 'object',
      properties: {
        query: { type: 'string', description: 'The search query (required).' },
        numResults: {
          type: 'integer',
          description: 'Number of results to return (1-100).',
          minimum: 1,
          maximum: 100,
        },
        type: { type: 'string', description: 'Search type.', enum: ['neural', 'keyword', 'auto'] },
        category: {
          type: 'string',
          description: 'Optional category filter for results.',
          enum: ['company', 'research paper', 'news', 'github', 'tweet', 'financial report', 'linkedin profile'],
        },
        includeDomains: {
          type: 'array',
          items: { type: 'string' },
          description: 'Only include results from these domains.',
        },
        excludeDomains: {
          type: 'array',
          items: { type: 'string' },
          description: 'Exclude results from these domains.',
        },
        startPublishedDate: {
          type: 'string',
          description: 'ISO 8601 date; only results published at/after this date.',
        },
        endPublishedDate: {
          type: 'string',
          description: 'ISO 8601 date; only results published at/before this date.',
        },
        contents: {
          type: 'object',
          description: 'Request page contents alongside the search results.',
          properties: {
            text: {
              type: ['boolean', 'object'],
              description: 'Include page text content (boolean or a text-options object).',
            },
            highlights: {
              type: ['boolean', 'object'],
              description: 'Include highlight snippets (boolean or a highlights-options object).',
            },
          },
        },
      },
      required: ['query'],
      additionalProperties: false,
    },
  },
  {
    name: 'web_fetch_exa',
    description: 'Fetch clean contents of web pages by URL or Exa document id (exactly one of urls/ids)',
    inputSchema: {
      type: 'object',
      properties: {
        urls: {
          type: 'array',
          items: { type: 'string' },
          minItems: 1,
          maxItems: 100,
          description: 'URLs to fetch contents for. Provide exactly one of urls/ids (XOR — validated locally).',
        },
        ids: {
          type: 'array',
          items: { type: 'string' },
          minItems: 1,
          description:
            'Exa document ids (from earlier search results) to fetch. Provide exactly one of urls/ids (XOR — validated locally).',
        },
      },
      additionalProperties: false,
    },
  },
  {
    name: 'exa_answer',
    description:
      'Answer a question from live web data with citations (Hikari-local tool name; the hosted Exa answering tool is retired)',
    inputSchema: {
      type: 'object',
      properties: {
        query: { type: 'string', description: 'The question to answer from live web data (required).' },
        model: {
          type: 'string',
          description: 'Answer model.',
          enum: ['exa', 'exa-pro', 'exa-research', 'exa-fast'],
        },
      },
      required: ['query'],
      additionalProperties: false,
    },
  },
]

/** Fixture per Exa tool; mirrors each tool's REST wire target. */
export function demoExaMcpToolFixture(name: string): Record<string, unknown> | null {
  if (name === 'web_search_exa') return demoExaSearchFixture()
  if (name === 'web_fetch_exa') return demoExaContentsFixture()
  if (name === 'exa_answer') return demoExaAnswerFixture()
  return null
}