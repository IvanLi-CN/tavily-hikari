/**
 * Browser demo fixtures for the SerpApi facade + hosted-MCP passthrough
 * (plan todo 25). Ground truth (committed mock bin, READ-ONLY):
 * `src/bin/mock_serpapi.rs` — the `/search` success envelope and the `/mcp`
 * tools/list catalog (`search`, `search_table`, `search_dashboard`) with its
 * `{ q }` inputSchema, plus the JSON-string `structuredContent.result`
 * tools/call passthrough shape. No network access ever runs in demo mode.
 */
import type { DemoMcpToolDefinition } from './demoProviders'

/** `/search` success envelope, byte-shape from `search_success_body()`. */
export function demoSerpapiSearchFixture(): Record<string, unknown> {
  return {
    search_metadata: { id: 'mock', status: 'Success' },
    search_parameters: {},
    organic_results: [
      {
        position: 1,
        title: 'Mock Result',
        link: 'https://example.com/1',
        snippet: 'mock snippet',
      },
    ],
  }
}

/** One hosted-MCP tool entry, shape from `mcp_tool()` in the mock bin. */
function serpapiMcpTool(name: string): DemoMcpToolDefinition {
  return {
    name,
    description: `Mock SerpAPI ${name}`,
    inputSchema: {
      type: 'object',
      properties: { q: { type: 'string', description: 'search query' } },
      required: ['q'],
    },
  }
}

/** `/serpapi/mcp` tools/list catalog (plan todo 15 tool names). */
export function demoSerpapiMcpTools(): DemoMcpToolDefinition[] {
  return [serpapiMcpTool('search'), serpapiMcpTool('search_table'), serpapiMcpTool('search_dashboard')]
}

/**
 * `/serpapi/mcp` tools/call result: the passthrough mock returns the search
 * envelope as a JSON string under `structuredContent.result`.
 */
export function demoSerpapiMcpToolResult(): Record<string, unknown> {
  return { structuredContent: { result: JSON.stringify(demoSerpapiSearchFixture()) } }
}