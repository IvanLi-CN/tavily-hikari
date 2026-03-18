import { afterEach, describe, expect, it, mock } from 'bun:test'

import { __testables } from './UserConsole'

const originalFetch = globalThis.fetch

const mcpProbeText: Parameters<typeof __testables.buildMcpProbeStepDefinitions>[0] = {
  steps: {
    mcpPing: 'MCP 服务连通',
    mcpToolsList: 'MCP 工具发现',
    mcpToolCall: 'MCP 工具调用 · {tool}',
  },
  errors: {
    missingAdvertisedTools: 'MCP tools/list 没有返回任何工具',
  },
}

const apiProbeText: Parameters<typeof __testables.buildApiProbeStepDefinitions>[0] = {
  steps: {
    apiSearch: 'Search API',
    apiExtract: 'Extract API',
    apiCrawl: 'Crawl API',
    apiMap: 'Map API',
    apiResearch: 'Research API',
    apiResearchResult: 'Research Result API',
  },
  errors: {
    missingRequestId: 'Missing research request id',
    researchFailed: 'Research failed',
    researchUnexpectedStatus: 'Unexpected research status: {status}',
  },
  researchPendingAccepted: 'Research still processing',
  researchStatus: 'Research status: {status}',
}

afterEach(() => {
  globalThis.fetch = originalFetch
})

function requestUrl(input: RequestInfo | URL): string {
  if (typeof input === 'string') return input
  if (input instanceof URL) return input.toString()
  return input.url
}

describe('UserConsole landing guide helpers', () => {
  it('shows the landing guide only when exactly one token is visible on the merged landing page', () => {
    expect(__testables.shouldRenderLandingGuide({ name: 'landing', section: 'dashboard' }, 1)).toBe(true)
    expect(__testables.shouldRenderLandingGuide({ name: 'landing', section: 'tokens' }, 1)).toBe(true)
    expect(__testables.shouldRenderLandingGuide({ name: 'landing', section: 'tokens' }, 0)).toBe(false)
    expect(__testables.shouldRenderLandingGuide({ name: 'landing', section: 'tokens' }, 2)).toBe(false)
    expect(__testables.shouldRenderLandingGuide({ name: 'token', id: 'a1b2' }, 1)).toBe(false)
  })

  it('prefers the detail token id and otherwise falls back to the single landing token mask', () => {
    expect(__testables.resolveGuideToken({ name: 'token', id: 'a1b2' }, [])).toBe(
      'th-a1b2-************************',
    )
    expect(__testables.resolveGuideToken(
      { name: 'landing', section: 'tokens' },
      [{ tokenId: 'c3d4' } as any],
    )).toBe('th-c3d4-************************')
    expect(__testables.resolveGuideToken(
      { name: 'landing', section: 'dashboard' },
      [{ tokenId: 'a1b2' } as any, { tokenId: 'c3d4' } as any],
    )).toBe('th-xxxx-xxxxxxxxxxxx')
  })
})

describe('UserConsole probe step definitions', () => {
  it('keeps MCP ping non-billable so exhausted tokens can still test connectivity', () => {
    const steps = __testables.buildMcpProbeStepDefinitions(mcpProbeText)

    expect(steps[0]?.id).toBe('mcp-ping')
    expect(steps[0]?.billable).toBe(false)
    expect(steps[1]?.id).toBe('mcp-tools-list')
    expect(steps[1]?.billable).toBeUndefined()
  })

  it('canonicalizes advertised MCP tool names before scheduling the sweep', () => {
    expect(__testables.extractAdvertisedMcpTools({
      result: {
        tools: [
          { name: ' tavily_search ' },
          { name: 'tavily-search' },
          { name: 'tavily_map' },
          { name: ' Acme_Lookup ' },
        ],
      },
    })).toEqual(['tavily-search', 'tavily-map', 'Acme_Lookup'])
  })

  it('skips unsupported tools when building the MCP call sweep', async () => {
    const calls: Array<{ url: string, init?: RequestInit }> = []
    globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ url: requestUrl(input), init })
      return new Response(JSON.stringify({ jsonrpc: '2.0', id: 'ok', result: { ok: true } }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    }) as typeof fetch

    const steps = __testables.buildMcpToolCallProbeStepDefinitions(mcpProbeText, [
      ' tavily_search ',
      ' Acme_Lookup ',
    ])

    expect(steps.map((step) => ({ id: step.id, billable: step.billable }))).toEqual([
      { id: 'mcp-tool-call:tavily-search', billable: true },
    ])

    await steps[0]?.run('th-zjvc-secret')

    expect(calls.map((call) => JSON.parse(String(call.init?.body ?? 'null')).params.name)).toEqual([
      'tavily-search',
    ])
  })

  it('executes live MCP probe calls with the expected JSON-RPC payloads', async () => {
    const calls: Array<{ url: string, init?: RequestInit }> = []
    globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ url: requestUrl(input), init })
      const body = JSON.parse(String(init?.body ?? '{}')) as { id?: string; method?: string }
      return new Response(
        JSON.stringify({
          jsonrpc: '2.0',
          id: body.id ?? 'unknown',
          result: body.method === 'tools/list'
            ? {
                tools: [
                  { name: 'tavily_search' },
                  { name: 'tavily_extract' },
                  { name: 'tavily-crawl' },
                  { name: 'tavily_map' },
                  { name: 'tavily_research' },
                ],
              }
            : { ok: true },
        }),
        {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        },
      )
    }) as typeof fetch

    const token = 'th-zjvc-secret'
    const baseSteps = __testables.buildMcpProbeStepDefinitions(mcpProbeText)
    await baseSteps[0]?.run(token)
    const toolsListResult = await baseSteps[1]?.run(token)
    const toolSteps = __testables.buildMcpToolCallProbeStepDefinitions(
      mcpProbeText,
      toolsListResult?.discoveredTools ?? [],
    )

    for (const step of toolSteps) {
      await step.run(token)
    }

    expect(calls).toHaveLength(7)
    expect(calls.every((call) => call.url === '/mcp')).toBe(true)

    const firstHeaders = new Headers(calls[0]?.init?.headers ?? {})
    const secondHeaders = new Headers(calls[1]?.init?.headers ?? {})
    expect(firstHeaders.get('Authorization')).toBe('Bearer th-zjvc-secret')
    expect(secondHeaders.get('Authorization')).toBe('Bearer th-zjvc-secret')

    expect(JSON.parse(String(calls[0]?.init?.body ?? 'null'))).toEqual({
      jsonrpc: '2.0',
      id: 'probe-ping',
      method: 'ping',
      params: {},
    })
    expect(JSON.parse(String(calls[1]?.init?.body ?? 'null'))).toEqual({
      jsonrpc: '2.0',
      id: 'probe-tools-list',
      method: 'tools/list',
      params: {},
    })

    expect(calls.slice(2).map((call) => JSON.parse(String(call.init?.body ?? 'null')))).toEqual([
      {
        jsonrpc: '2.0',
        id: 'probe-tool-call:tavily-search',
        method: 'tools/call',
        params: {
          name: 'tavily-search',
          arguments: {
            query: 'health check',
            search_depth: 'basic',
          },
        },
      },
      {
        jsonrpc: '2.0',
        id: 'probe-tool-call:tavily-extract',
        method: 'tools/call',
        params: {
          name: 'tavily-extract',
          arguments: {
            urls: ['https://example.com'],
          },
        },
      },
      {
        jsonrpc: '2.0',
        id: 'probe-tool-call:tavily-crawl',
        method: 'tools/call',
        params: {
          name: 'tavily-crawl',
          arguments: {
            url: 'https://example.com',
            max_depth: 1,
            limit: 1,
          },
        },
      },
      {
        jsonrpc: '2.0',
        id: 'probe-tool-call:tavily-map',
        method: 'tools/call',
        params: {
          name: 'tavily-map',
          arguments: {
            url: 'https://example.com',
            max_depth: 1,
            limit: 1,
          },
        },
      },
      {
        jsonrpc: '2.0',
        id: 'probe-tool-call:tavily-research',
        method: 'tools/call',
        params: {
          name: 'tavily-research',
          arguments: {
            query: 'health check',
          },
        },
      },
    ])
  })

  it('updates the running MCP progress total after discovering tool call steps', () => {
    const baseSteps = __testables.buildMcpProbeStepDefinitions(mcpProbeText)
    const toolSteps = __testables.buildMcpToolCallProbeStepDefinitions(mcpProbeText, [
      'tavily-search',
      'tavily-extract',
      'tavily-crawl',
      'tavily-map',
      'tavily-research',
    ])

    const stepDefinitions = [...baseSteps, ...toolSteps]
    const nextModel = __testables.nextRunningMcpProbeModel(
      { state: 'running', completed: 2, total: 2 },
      stepDefinitions,
      3,
    )

    expect(nextModel).toEqual({
      state: 'running',
      completed: 3,
      total: 7,
    })
  })

  it('executes every API probe call with the expected endpoint and payload', async () => {
    const calls: Array<{ url: string, init?: RequestInit }> = []
    globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = requestUrl(input)
      calls.push({ url, init })

      if (url === '/api/tavily/research') {
        return new Response(JSON.stringify({ request_id: 'req-health-check' }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        })
      }

      if (url === '/api/tavily/research/req-health-check') {
        return new Response(JSON.stringify({ status: 'completed' }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        })
      }

      return new Response(JSON.stringify({ ok: true }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    }) as typeof fetch

    const steps = __testables.buildApiProbeStepDefinitions(apiProbeText)
    let requestId: string | null = null
    let researchResultDetail: string | null = null

    for (const step of steps) {
      const result = await step.run('th-zjvc-secret', { requestId })
      if (step.id === 'api-research') {
        requestId = result
      }
      if (step.id === 'api-research-result') {
        researchResultDetail = result
      }
    }

    expect(calls).toHaveLength(6)
    expect(calls.map((call) => [call.url, call.init?.method ?? 'GET'])).toEqual([
      ['/api/tavily/search', 'POST'],
      ['/api/tavily/extract', 'POST'],
      ['/api/tavily/crawl', 'POST'],
      ['/api/tavily/map', 'POST'],
      ['/api/tavily/research', 'POST'],
      ['/api/tavily/research/req-health-check', 'GET'],
    ])

    for (const call of calls) {
      const headers = new Headers(call.init?.headers ?? {})
      expect(headers.get('Authorization')).toBe('Bearer th-zjvc-secret')
    }

    expect(JSON.parse(String(calls[0]?.init?.body ?? 'null'))).toEqual({
      query: 'health check',
      max_results: 1,
      search_depth: 'basic',
      include_answer: false,
      include_raw_content: false,
      include_images: false,
    })
    expect(JSON.parse(String(calls[1]?.init?.body ?? 'null'))).toEqual({
      urls: ['https://example.com'],
      include_images: false,
    })
    expect(JSON.parse(String(calls[2]?.init?.body ?? 'null'))).toEqual({
      url: 'https://example.com',
      max_depth: 1,
      limit: 1,
    })
    expect(JSON.parse(String(calls[3]?.init?.body ?? 'null'))).toEqual({
      url: 'https://example.com',
      max_depth: 1,
      limit: 1,
    })
    expect(JSON.parse(String(calls[4]?.init?.body ?? 'null'))).toEqual({
      input: 'health check',
      model: 'mini',
      citation_format: 'numbered',
    })
    expect(researchResultDetail).toBe('Research status: completed')
  })
})
