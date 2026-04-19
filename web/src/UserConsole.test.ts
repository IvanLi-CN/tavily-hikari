import { afterEach, describe, expect, it, mock } from 'bun:test'

import { resolveGuideSamples } from './user-console/guide'
import {
  applyLoggedOutConsoleReset,
  buildApiProbeStepDefinitions,
  buildMcpProbeStepDefinitions,
  buildMcpToolCallProbeStepDefinitions,
  createClearedProbeUiState,
  createMcpProbeIdentityGenerator,
  extractAdvertisedMcpTools,
  isActiveGuideRevealContext,
  isIdentifierLikePropertyName,
  nextRunningMcpProbeModel,
  performUserLogoutFlow,
  resetActiveProbeUiState,
  resolveDetailLogsPushIssueMessage,
  resolveGuideRevealContextKey,
  resolveGuideToken,
  resolveGuideTokenId,
  resolvePostLogoutTarget,
  resolveFallbackLogoutTarget,
  resolveUserConsoleIdentityName,
  resolveUserConsoleProviderLabel,
  resolveUserConsoleView,
  shouldRedirectToLogoutTarget,
  shouldRenderLandingGuide,
  toLoggedOutConsoleProfile,
} from './user-console/runtime'

const originalFetch = globalThis.fetch

const mcpProbeText: Parameters<typeof buildMcpProbeStepDefinitions>[0] = {
  steps: {
    mcpInitialize: 'MCP 会话初始化',
    mcpInitialized: 'MCP initialized 通知',
    mcpPing: 'MCP 服务连通',
    mcpToolsList: 'MCP 工具发现',
    mcpToolCall: '调用 {tool} 工具',
  },
  skippedProbeFixture: '当前本地没有 {tool} 的检测夹具，已跳过。',
  errors: {
    missingAdvertisedTools: 'MCP tools/list 没有返回任何工具',
  },
}

const apiProbeText: Parameters<typeof buildApiProbeStepDefinitions>[0] = {
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

function createMcpProbeContext(overrides: Partial<Parameters<NonNullable<ReturnType<typeof buildMcpProbeStepDefinitions>[number]['run']>>[1]> = {}) {
  return {
    protocolVersion: '2025-03-26',
    sessionId: null,
    clientVersion: '0.29.5-test',
    identity: createMcpProbeIdentityGenerator({
      now: Date.UTC(2026, 2, 27, 8, 15, 42),
      random: () => 0.123456789,
    }),
    ...overrides,
  }
}

describe('UserConsole landing guide helpers', () => {
  it('derives header view, identity, and provider labels from the active session state', () => {
    expect(resolveUserConsoleView({ name: 'landing', section: 'dashboard' })).toBe('dashboard')
    expect(resolveUserConsoleView({ name: 'landing', section: 'tokens' })).toBe('tokens')
    expect(resolveUserConsoleView({ name: 'token', id: 'a1b2' })).toBe('tokenDetail')

    expect(resolveUserConsoleIdentityName({
      displayName: 'ops-admin',
      isAdmin: true,
      forwardAuthEnabled: true,
      builtinAuthEnabled: true,
      allowRegistration: true,
      userLoggedIn: true,
      userProvider: 'linuxdo',
      userDisplayName: 'Ivan',
    })).toBe('Ivan')

    expect(resolveUserConsoleIdentityName({
      displayName: 'dev-mode',
      isAdmin: true,
      forwardAuthEnabled: true,
      builtinAuthEnabled: true,
      allowRegistration: true,
    })).toBe('dev-mode')

    expect(resolveUserConsoleProviderLabel('linuxdo', { linuxdo: 'LinuxDo' })).toBe('LinuxDo')
    expect(resolveUserConsoleProviderLabel(null, { linuxdo: 'LinuxDo' })).toBeNull()
  })

  it('prefers the public home as the logout redirect when it is available', async () => {
    const probe = mock(async () => ({ ok: true, status: 200 }))

    await expect(resolvePostLogoutTarget({
      pathname: '/console',
      search: '',
    } as Pick<Location, 'pathname' | 'search'>, probe)).resolves.toBe('/')
    expect(probe).toHaveBeenCalledWith('/', {
      method: 'HEAD',
      credentials: 'same-origin',
    })
  })

  it('falls back to the active console entry path when the public home is unavailable', async () => {
    const probe = mock(async () => ({ ok: false, status: 404 }))

    await expect(resolvePostLogoutTarget({
      pathname: 'console.html',
      search: '?from=oauth',
    } as Pick<Location, 'pathname' | 'search'>, probe)).resolves.toBe('/console.html?from=oauth')

    expect(resolveFallbackLogoutTarget({
      pathname: '',
      search: '',
    } as Pick<Location, 'pathname' | 'search'>)).toBe('/')
  })

  it('treats a root redirect back to the console entry as a console-only deployment', async () => {
    const probe = mock(async () => ({
      ok: true,
      status: 200,
      redirected: true,
      url: 'https://app.test/console',
    }))

    await expect(resolvePostLogoutTarget({
      pathname: '/console',
      search: '?from=logout',
    } as Pick<Location, 'pathname' | 'search'>, probe)).resolves.toBe('/console?from=logout')
  })

  it('stays on the console entry when the root redirects to an auth page after logout', async () => {
    const probe = mock(async () => ({
      ok: true,
      status: 200,
      redirected: true,
      url: 'https://app.test/auth/linuxdo',
    }))

    await expect(resolvePostLogoutTarget({
      pathname: '/console',
      search: '?from=logout',
    } as Pick<Location, 'pathname' | 'search'>, probe)).resolves.toBe('/console?from=logout')
  })

  it('keeps nested console paths as the logout fallback when the public home is unavailable', async () => {
    const probe = mock(async () => ({ ok: false, status: 404 }))

    await expect(resolvePostLogoutTarget({
      pathname: '/console/tokens/a1b2',
      search: '?from=logout',
    } as Pick<Location, 'pathname' | 'search'>, probe)).resolves.toBe('/console/tokens/a1b2?from=logout')
  })

  it('retries GET when the public home rejects HEAD during logout target probing', async () => {
    const probe = mock(async (_input, init) => {
      if (init?.method === 'HEAD') {
        return {
          ok: false,
          status: 405,
          redirected: false,
          url: 'https://app.test/',
        }
      }

      return {
        ok: true,
        status: 200,
        redirected: false,
        url: 'https://app.test/',
      }
    })

    await expect(resolvePostLogoutTarget({
      pathname: '/console',
      search: '',
    } as Pick<Location, 'pathname' | 'search'>, probe)).resolves.toBe('/')
    expect(probe.mock.calls).toEqual([
      ['/', { method: 'HEAD', credentials: 'same-origin' }],
      ['/', { method: 'GET', credentials: 'same-origin' }],
    ])
  })

  it('retries GET when HEAD fails with a non-405 status but the public home still loads', async () => {
    const probe = mock(async (_input, init) => {
      if (init?.method === 'HEAD') {
        return {
          ok: false,
          status: 403,
          redirected: false,
          url: 'https://app.test/',
        }
      }

      return {
        ok: true,
        status: 200,
        redirected: false,
        url: 'https://app.test/',
      }
    })

    await expect(resolvePostLogoutTarget({
      pathname: '/console',
      search: '',
    } as Pick<Location, 'pathname' | 'search'>, probe)).resolves.toBe('/')
    expect(probe.mock.calls).toEqual([
      ['/', { method: 'HEAD', credentials: 'same-origin' }],
      ['/', { method: 'GET', credentials: 'same-origin' }],
    ])
  })

  it('does not redirect again when logout already landed on the console fallback entry', () => {
    expect(shouldRedirectToLogoutTarget({
      pathname: '/console.html',
      search: '?from=oauth',
    } as Pick<Location, 'pathname' | 'search'>, '/console.html?from=oauth')).toBe(false)

    expect(shouldRedirectToLogoutTarget({
      pathname: '/console/tokens/a1b2',
      search: '?from=logout',
    } as Pick<Location, 'pathname' | 'search'>, '/console/tokens/a1b2?from=logout')).toBe(false)

    expect(shouldRedirectToLogoutTarget({
      pathname: '/console',
      search: '',
    } as Pick<Location, 'pathname' | 'search'>, '/')).toBe(true)
  })

  it('invalidates active probe runs by resetting both probe buttons to idle', () => {
    expect(createClearedProbeUiState(7)).toEqual({
      nextRunId: 8,
      mcpProbe: {
        state: 'idle',
        completed: 0,
        total: 4,
      },
      apiProbe: {
        state: 'idle',
        completed: 0,
        total: 6,
      },
    })
  })

  it('aborts the active probe request when the route resets probe state', () => {
    const abortActiveProbeRun = mock(() => {})

    expect(resetActiveProbeUiState(7, abortActiveProbeRun)).toEqual({
      nextRunId: 8,
      mcpProbe: {
        state: 'idle',
        completed: 0,
        total: 4,
      },
      apiProbe: {
        state: 'idle',
        completed: 0,
        total: 6,
      },
    })
    expect(abortActiveProbeRun).toHaveBeenCalledTimes(1)
  })

  it('clears sensitive console state when profile refresh reports a logged-out session', () => {
    const events: string[] = []

    applyLoggedOutConsoleReset({
      clearSensitiveConsoleState: () => {
        events.push('clear')
      },
      setShowLoggedOutState: (value) => {
        events.push(`show:${String(value)}`)
      },
    })

    expect(events).toEqual(['show:false', 'clear'])
  })

  it('waits for logout success before clearing active console loads', async () => {
    const events: string[] = []
    let resolveLogout: (() => void) | null = null

    const logoutRequest = mock(() => new Promise<void>((resolve) => {
      events.push('logout:start')
      resolveLogout = () => {
        events.push('logout:resolved')
        resolve()
      }
    }))
    const abortActiveConsoleLoads = mock(() => {
      events.push('abort')
    })
    const redirectAfterLogout = mock(async () => {
      events.push('redirect')
    })

    const logoutFlow = performUserLogoutFlow({
      logoutRequest,
      abortActiveConsoleLoads,
      redirectAfterLogout,
    })

    expect(events).toEqual(['logout:start'])
    expect(abortActiveConsoleLoads).not.toHaveBeenCalled()
    expect(redirectAfterLogout).not.toHaveBeenCalled()

    resolveLogout?.()
    await logoutFlow

    expect(events).toEqual([
      'logout:start',
      'logout:resolved',
      'abort',
      'redirect',
    ])
  })

  it('keeps the active console state intact when logout fails', async () => {
    const abortActiveConsoleLoads = mock(() => {})
    const redirectAfterLogout = mock(async () => {})

    await expect(performUserLogoutFlow({
      logoutRequest: async () => {
        throw new Error('temporary failure')
      },
      abortActiveConsoleLoads,
      redirectAfterLogout,
    })).rejects.toThrow('temporary failure')

    expect(abortActiveConsoleLoads).not.toHaveBeenCalled()
    expect(redirectAfterLogout).not.toHaveBeenCalled()
  })

  it('preserves admin affordances when only the user session logs out', () => {
    expect(toLoggedOutConsoleProfile({
      displayName: 'ops-admin',
      isAdmin: true,
      forwardAuthEnabled: true,
      builtinAuthEnabled: false,
      allowRegistration: true,
      userLoggedIn: true,
      userProvider: 'linuxdo',
      userDisplayName: 'Ivan',
      userAvatarUrl: 'https://connect.linux.do/avatar.png',
    })).toEqual({
      displayName: 'ops-admin',
      isAdmin: true,
      forwardAuthEnabled: true,
      builtinAuthEnabled: false,
      allowRegistration: true,
      userLoggedIn: false,
      userProvider: null,
      userDisplayName: null,
      userAvatarUrl: null,
    })
  })

  it('materializes a logged-out placeholder profile when the first profile load returns 401', () => {
    expect(toLoggedOutConsoleProfile(null)).toEqual({
      displayName: null,
      isAdmin: false,
      forwardAuthEnabled: false,
      builtinAuthEnabled: false,
      allowRegistration: false,
      userLoggedIn: false,
      userProvider: null,
      userDisplayName: null,
      userAvatarUrl: null,
    })
  })

  it('maps request-log push issues to the expected bubble copy', () => {
    const copy = {
      ariaLabel: 'status',
      browserUnsupported: 'unsupported',
      reconnecting: 'reconnecting',
      closed: 'closed',
    }

    expect(resolveDetailLogsPushIssueMessage('unsupported', copy)).toBe('unsupported')
    expect(resolveDetailLogsPushIssueMessage('reconnecting', copy)).toBe('reconnecting')
    expect(resolveDetailLogsPushIssueMessage('closed', copy)).toBe('closed')
  })

  it('shows the landing guide only when exactly one token is visible on the merged landing page', () => {
    expect(shouldRenderLandingGuide({ name: 'landing', section: 'dashboard' }, 1)).toBe(true)
    expect(shouldRenderLandingGuide({ name: 'landing', section: 'tokens' }, 1)).toBe(true)
    expect(shouldRenderLandingGuide({ name: 'landing', section: 'tokens' }, 0)).toBe(false)
    expect(shouldRenderLandingGuide({ name: 'landing', section: 'tokens' }, 2)).toBe(false)
    expect(shouldRenderLandingGuide({ name: 'token', id: 'a1b2' }, 1)).toBe(false)
  })

  it('prefers the detail token id and otherwise falls back to the single landing token mask', () => {
    expect(resolveGuideToken({ name: 'token', id: 'a1b2' }, [])).toBe(
      'th-a1b2-************************',
    )
    expect(resolveGuideToken(
      { name: 'landing', section: 'tokens' },
      [{ tokenId: 'c3d4' } as any],
    )).toBe('th-c3d4-************************')
    expect(resolveGuideToken(
      { name: 'landing', section: 'dashboard' },
      [{ tokenId: 'a1b2' } as any, { tokenId: 'c3d4' } as any],
    )).toBe('th-xxxx-xxxxxxxxxxxx')
  })

  it('returns the revealable guide token id only for token detail or single-token landing routes', () => {
    expect(resolveGuideTokenId({ name: 'token', id: 'a1b2' }, [])).toBe('a1b2')
    expect(resolveGuideTokenId(
      { name: 'landing', section: 'tokens' },
      [{ tokenId: 'c3d4' } as any],
    )).toBe('c3d4')
    expect(resolveGuideTokenId(
      { name: 'landing', section: 'dashboard' },
      [{ tokenId: 'a1b2' } as any, { tokenId: 'c3d4' } as any],
    )).toBeNull()
  })

  it('derives a distinct guide reveal context for each route and visible token set', () => {
    expect(resolveGuideRevealContextKey({ name: 'token', id: 'a1b2' }, [])).toBe('token:a1b2')
    expect(resolveGuideRevealContextKey(
      { name: 'landing', section: 'tokens' },
      [{ tokenId: 'c3d4' } as any],
    )).toBe('landing:tokens:c3d4')
    expect(resolveGuideRevealContextKey(
      { name: 'landing', section: 'dashboard' },
      [{ tokenId: 'a1b2' } as any, { tokenId: 'c3d4' } as any],
    )).toBeNull()
  })

  it('renders a revealed guide token only while the reveal context still matches', () => {
    expect(isActiveGuideRevealContext('landing:tokens:a1b2', 'landing:tokens:a1b2')).toBe(true)
    expect(isActiveGuideRevealContext('landing:tokens:a1b2', 'landing:tokens:b2c3')).toBe(false)
    expect(isActiveGuideRevealContext('token:a1b2', null)).toBe(false)
  })

  it('normalizes guide samples so the other tab can render both MCP and API examples', () => {
    expect(resolveGuideSamples({
      title: 'Legacy',
      steps: [],
      sampleTitle: 'Example',
      snippetLanguage: 'bash',
      snippet: 'echo ok',
      reference: { label: 'Docs', url: 'https://example.com' },
    })).toEqual([
      {
        title: 'Example',
        language: 'bash',
        snippet: 'echo ok',
        reference: { label: 'Docs', url: 'https://example.com' },
      },
    ])

    const samples = [
      { title: 'MCP', language: 'json', snippet: '{}' },
      { title: 'API', language: 'bash', snippet: 'curl ...' },
    ]
    expect(resolveGuideSamples({ title: 'Other', steps: [], samples })).toBe(samples)
  })
})

describe('UserConsole probe step definitions', () => {
  it('keeps MCP lifecycle control-plane steps non-billable so exhausted tokens can still test connectivity', () => {
    const steps = buildMcpProbeStepDefinitions(mcpProbeText)

    expect(steps[0]?.id).toBe('mcp-initialize')
    expect(steps[0]?.billable).toBe(false)
    expect(steps[1]?.id).toBe('mcp-initialized')
    expect(steps[1]?.billable).toBe(false)
    expect(steps[2]?.id).toBe('mcp-ping')
    expect(steps[2]?.billable).toBe(false)
    expect(steps[3]?.id).toBe('mcp-tools-list')
    expect(steps[3]?.billable).toBeUndefined()
  })

  it('threads abort signals through MCP probe requests', async () => {
    const fetchMock = mock(async () => {
      return new Response(JSON.stringify({
        jsonrpc: '2.0',
        id: 'req-1',
        result: { ok: true },
      }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    })
    globalThis.fetch = fetchMock as typeof fetch

    const controller = new AbortController()
    const steps = buildMcpProbeStepDefinitions(mcpProbeText)
    await expect(steps[0]?.run('th-zjvc-secret', createMcpProbeContext({ signal: controller.signal }))).resolves.toBeNull()

    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect((fetchMock.mock.calls[0] as [RequestInfo | URL, RequestInit])[1]?.signal).toBe(controller.signal)
  })

  it('preserves every advertised MCP tool name, including legacy aliases', () => {
    expect(extractAdvertisedMcpTools({
      result: {
        tools: [
          { name: ' tavily_search ' },
          { name: 'tavily-search' },
          { name: 'tavily_map' },
          { name: ' Acme_Lookup ' },
        ],
      },
    })).toEqual([
      { requestName: 'tavily_search', displayName: 'tavily-search', inputSchema: null },
      { requestName: 'tavily-search', displayName: 'tavily-search', inputSchema: null },
      { requestName: 'tavily_map', displayName: 'tavily-map', inputSchema: null },
      { requestName: 'Acme_Lookup', displayName: 'Acme_Lookup', inputSchema: null },
    ])
  })

  it('does not execute schema-backed non-Tavily tools during the sweep', async () => {
    const calls: Array<{ url: string, init?: RequestInit }> = []
    const context = createMcpProbeContext({ sessionId: 'session-123' })
    globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ url: requestUrl(input), init })
      return new Response(JSON.stringify({ jsonrpc: '2.0', id: 'ok', result: { ok: true } }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    }) as typeof fetch

    const steps = buildMcpToolCallProbeStepDefinitions(mcpProbeText, [
      ' tavily_search ',
      'tavily-search',
      {
        requestName: 'Acme_Lookup',
        displayName: 'Acme_Lookup',
        inputSchema: {
          type: 'object',
          required: ['query', 'target_url', 'include_images'],
          properties: {
            query: { type: 'string' },
            target_url: { type: 'string', format: 'uri' },
            include_images: { type: 'boolean' },
          },
        },
      },
    ])

    expect(steps.map((step) => ({ id: step.id, billable: step.billable }))).toEqual([
      { id: 'mcp-tool-call:tavily_search', billable: true },
      { id: 'mcp-tool-call:tavily-search', billable: true },
      { id: 'mcp-tool-call:Acme_Lookup', billable: false },
    ])
    expect(steps.map((step) => step.label)).toEqual([
      '调用 tavily_search 工具',
      '调用 tavily-search 工具',
      '调用 Acme_Lookup 工具',
    ])

    await expect(steps[0]?.run('th-zjvc-secret', context)).resolves.toBeNull()
    await expect(steps[1]?.run('th-zjvc-secret', context)).resolves.toBeNull()
    await expect(steps[2]?.run('th-zjvc-secret', context)).resolves.toEqual({
      detail: '当前本地没有 Acme_Lookup 的检测夹具，已跳过。',
      stepState: 'skipped',
    })

    expect(calls.map((call) => JSON.parse(String(call.init?.body ?? 'null')).params)).toEqual([
      {
        name: 'tavily_search',
        arguments: {
          query: 'health check',
          search_depth: 'basic',
        },
      },
      {
        name: 'tavily-search',
        arguments: {
          query: 'health check',
          search_depth: 'basic',
        },
      },
    ])
    expect(new Headers(calls[0]?.init?.headers ?? {}).get('Mcp-Session-Id')).toBe('session-123')
    expect(new Headers(calls[0]?.init?.headers ?? {}).get('Mcp-Protocol-Version')).toBe('2025-03-26')
  })

  it('keeps tools/list successful even when it advertises a tool without a probe fixture', async () => {
    globalThis.fetch = mock(async (_input: RequestInfo | URL, init?: RequestInit) => {
      const body = JSON.parse(String(init?.body ?? '{}')) as { id?: string }
      return new Response(JSON.stringify({
        jsonrpc: '2.0',
        id: body.id ?? 'unknown',
        result: {
          tools: [
            { name: 'tavily-search' },
            { name: 'Acme_Lookup' },
          ],
        },
      }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    }) as typeof fetch

    const steps = buildMcpProbeStepDefinitions(mcpProbeText)

    await expect(steps[3]?.run('th-zjvc-secret', createMcpProbeContext())).resolves.toEqual({
      discoveredTools: [
        { requestName: 'tavily-search', displayName: 'tavily-search', inputSchema: null },
        { requestName: 'Acme_Lookup', displayName: 'Acme_Lookup', inputSchema: null },
      ],
    })
  })

  it('surfaces JSON-RPC error envelopes from notifications/initialized instead of treating them as success', async () => {
    globalThis.fetch = mock(async () => {
      return new Response(JSON.stringify({
        jsonrpc: '2.0',
        error: {
          code: -32600,
          message: 'initialized rejected',
        },
      }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    }) as typeof fetch

    const steps = buildMcpProbeStepDefinitions(mcpProbeText)
    await expect(steps[1]?.run('th-zjvc-secret', createMcpProbeContext({ sessionId: 'session-123' }))).rejects.toThrow(
      'initialized rejected',
    )
  })

  it('skips advertised tools only when discovery provides no fixture and no input schema', async () => {
    const steps = buildMcpToolCallProbeStepDefinitions(mcpProbeText, [{
      requestName: 'Acme_Lookup',
      displayName: 'Acme_Lookup',
      inputSchema: null,
    }])

    await expect(steps[0]?.run('th-zjvc-secret', createMcpProbeContext())).resolves.toEqual({
      detail: '当前本地没有 Acme_Lookup 的检测夹具，已跳过。',
      stepState: 'skipped',
    })
  })

  it('treats tools/call JSON-RPC result envelopes with failing statuses as probe failures', async () => {
    globalThis.fetch = mock(async (_input: RequestInfo | URL, init?: RequestInit) => {
      const body = JSON.parse(String(init?.body ?? '{}')) as { id?: string }
      return new Response(JSON.stringify({
        jsonrpc: '2.0',
        id: body.id ?? 'unknown',
        result: {
          isError: true,
          structuredContent: {
            detail: { status: 500 },
          },
        },
      }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    }) as typeof fetch

    const steps = buildMcpToolCallProbeStepDefinitions(mcpProbeText, ['tavily-search'])

    await expect(steps[0]?.run('th-zjvc-secret', createMcpProbeContext())).rejects.toThrow('Request failed with status 500')
  })

  it('prefers specific upstream detail errors over generic MCP research failures', async () => {
    globalThis.fetch = mock(async (_input: RequestInfo | URL, init?: RequestInit) => {
      const body = JSON.parse(String(init?.body ?? '{}')) as { id?: string }
      return new Response(JSON.stringify({
        jsonrpc: '2.0',
        id: body.id ?? 'unknown',
        result: {
          structuredContent: {
            error: 'Research request failed',
            status: 432,
            detail: {
              error: 'This request exceeds your plan\'s set usage limit.',
            },
          },
        },
      }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    }) as typeof fetch

    const steps = buildMcpToolCallProbeStepDefinitions(mcpProbeText, ['tavily-research'])

    await expect(steps[0]?.run('th-zjvc-secret', createMcpProbeContext())).rejects.toThrow(
      "This request exceeds your plan's set usage limit.",
    )
  })

  it('executes live MCP probe calls with the expected JSON-RPC payloads', async () => {
    const calls: Array<{ url: string, init?: RequestInit }> = []
    globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ url: requestUrl(input), init })
      const body = JSON.parse(String(init?.body ?? '{}')) as { id?: string; method?: string }
      if (body.method === 'initialize') {
        return new Response(
          JSON.stringify({
            jsonrpc: '2.0',
            id: body.id ?? 'unknown',
            result: {
              protocolVersion: '2025-03-26',
              capabilities: {},
              serverInfo: { name: 'mock', version: '1.0.0' },
            },
          }),
          {
            status: 200,
            headers: {
              'Content-Type': 'application/json',
              'Mcp-Session-Id': 'session-123',
            },
          },
        )
      }

      if (body.method === 'notifications/initialized') {
        return new Response(null, {
          status: 202,
        })
      }

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
    const baseSteps = buildMcpProbeStepDefinitions(mcpProbeText)
    const context = createMcpProbeContext()
    await baseSteps[0]?.run(token, context)
    await baseSteps[1]?.run(token, context)
    await baseSteps[2]?.run(token, context)
    const toolsListResult = await baseSteps[3]?.run(token, context)
    const toolSteps = buildMcpToolCallProbeStepDefinitions(
      mcpProbeText,
      toolsListResult?.discoveredTools ?? [],
    )

    for (const step of toolSteps) {
      await step.run(token, context)
    }

    expect(calls).toHaveLength(9)
    expect(calls.every((call) => call.url === '/mcp')).toBe(true)

    const firstHeaders = new Headers(calls[0]?.init?.headers ?? {})
    const secondHeaders = new Headers(calls[1]?.init?.headers ?? {})
    expect(firstHeaders.get('Authorization')).toBe('Bearer th-zjvc-secret')
    expect(secondHeaders.get('Authorization')).toBe('Bearer th-zjvc-secret')
    expect(firstHeaders.get('Mcp-Protocol-Version')).toBe('2025-03-26')
    expect(secondHeaders.get('Mcp-Protocol-Version')).toBe('2025-03-26')

    const initializeBody = JSON.parse(String(calls[0]?.init?.body ?? 'null'))
    expect(initializeBody).toMatchObject({
      jsonrpc: '2.0',
      method: 'initialize',
      params: {
        protocolVersion: '2025-03-26',
        capabilities: {},
        clientInfo: {
          name: 'Tavily Hikari UserConsole Probe',
          version: '0.29.5-test',
        },
      },
    })
    expect(initializeBody.id).toMatch(/^req-initialize-ucp-/)

    expect(JSON.parse(String(calls[1]?.init?.body ?? 'null'))).toEqual({
      jsonrpc: '2.0',
      method: 'notifications/initialized',
    })
    expect(JSON.parse(String(calls[2]?.init?.body ?? 'null'))).toMatchObject({
      jsonrpc: '2.0',
      method: 'ping',
    })
    expect(JSON.parse(String(calls[3]?.init?.body ?? 'null'))).toMatchObject({
      jsonrpc: '2.0',
      method: 'tools/list',
    })

    const notificationHeaders = new Headers(calls[1]?.init?.headers ?? {})
    const pingHeaders = new Headers(calls[2]?.init?.headers ?? {})
    expect(notificationHeaders.get('Mcp-Session-Id')).toBe('session-123')
    expect(pingHeaders.get('Mcp-Session-Id')).toBe('session-123')

    const toolCallBodies = calls.slice(4).map((call) => JSON.parse(String(call.init?.body ?? 'null')))
    expect(toolCallBodies.map((body) => body.method)).toEqual([
      'tools/call',
      'tools/call',
      'tools/call',
      'tools/call',
      'tools/call',
    ])
    expect(new Set(toolCallBodies.map((body) => body.id)).size).toBe(5)
    expect(toolCallBodies.map((body) => body.params)).toEqual([
      {
        name: 'tavily_search',
        arguments: {
          query: 'health check',
          search_depth: 'basic',
        },
      },
      {
        name: 'tavily_extract',
        arguments: {
          urls: ['https://example.com'],
        },
      },
      {
        name: 'tavily-crawl',
        arguments: {
          url: 'https://example.com',
          max_depth: 1,
          limit: 1,
        },
      },
      {
        name: 'tavily_map',
        arguments: {
          url: 'https://example.com',
          max_depth: 1,
          limit: 1,
        },
      },
      {
        name: 'tavily_research',
        arguments: {
          input: 'health check',
          model: 'mini',
        },
      },
    ])
  })

  it('does not silently skip newly advertised Tavily tools when discovery exposes an input schema', async () => {
    const calls: Array<{ url: string, init?: RequestInit }> = []
    globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ url: requestUrl(input), init })
      return new Response(JSON.stringify({ jsonrpc: '2.0', id: 'ok', result: { ok: true } }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    }) as typeof fetch

    const steps = buildMcpToolCallProbeStepDefinitions(mcpProbeText, [{
      requestName: 'tavily_agents',
      displayName: 'tavily-agents',
      inputSchema: {
        type: 'object',
        required: ['query'],
        properties: {
          query: { type: 'string' },
        },
      },
    }])

    expect(steps).toHaveLength(1)
    expect(steps[0]?.billable).toBe(true)

    await expect(steps[0]?.run('th-zjvc-secret', createMcpProbeContext())).resolves.toBeNull()
    expect(JSON.parse(String(calls[0]?.init?.body ?? 'null'))).toMatchObject({
      jsonrpc: '2.0',
      method: 'tools/call',
      params: {
        name: 'tavily_agents',
        arguments: {
          query: 'health check',
        },
      },
    })
  })

  it('generates fresh identifier-like schema fields for each MCP tool-call item', async () => {
    const calls: Array<{ url: string, init?: RequestInit }> = []
    globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ url: requestUrl(input), init })
      return new Response(JSON.stringify({ jsonrpc: '2.0', id: 'ok', result: { ok: true } }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    }) as typeof fetch

    const steps = buildMcpToolCallProbeStepDefinitions(mcpProbeText, [
      {
        requestName: 'tavily_agents',
        displayName: 'tavily-agents',
        inputSchema: {
          type: 'object',
          required: ['query', 'requestId', 'session_id', 'traceUuid'],
          properties: {
            query: { type: 'string' },
            requestId: { type: 'string' },
            session_id: { type: 'string' },
            traceUuid: { type: 'string', format: 'uuid' },
          },
        },
      },
      {
        requestName: 'tavily_followup',
        displayName: 'tavily-followup',
        inputSchema: {
          type: 'object',
          required: ['query', 'requestId', 'session_id', 'traceUuid'],
          properties: {
            query: { type: 'string' },
            requestId: { type: 'string' },
            session_id: { type: 'string' },
            traceUuid: { type: 'string', format: 'uuid' },
          },
        },
      },
    ])

    const context = createMcpProbeContext({ sessionId: 'session-123' })
    await expect(steps[0]?.run('th-zjvc-secret', context)).resolves.toBeNull()
    await expect(steps[1]?.run('th-zjvc-secret', context)).resolves.toBeNull()

    const firstArgs = JSON.parse(String(calls[0]?.init?.body ?? 'null')).params.arguments
    const secondArgs = JSON.parse(String(calls[1]?.init?.body ?? 'null')).params.arguments

    expect(firstArgs.query).toBe('health check')
    expect(firstArgs.requestId).toMatch(/^req_ucp-/)
    expect(firstArgs.session_id).toMatch(/^sess_ucp-/)
    expect(firstArgs.traceUuid).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-a[0-9a-f]{3}-[0-9a-f]{12}$/)
    expect(secondArgs.requestId).not.toBe(firstArgs.requestId)
    expect(secondArgs.session_id).not.toBe(firstArgs.session_id)
    expect(secondArgs.traceUuid).not.toBe(firstArgs.traceUuid)
  })

  it('only treats actual identifier-like field names as dynamic identifiers', () => {
    expect(isIdentifierLikePropertyName('requestId')).toBe(true)
    expect(isIdentifierLikePropertyName('session_id')).toBe(true)
    expect(isIdentifierLikePropertyName('traceUuid')).toBe(true)
    expect(isIdentifierLikePropertyName('cursor')).toBe(true)

    expect(isIdentifierLikePropertyName('hybrid')).toBe(false)
    expect(isIdentifierLikePropertyName('grid')).toBe(false)
    expect(isIdentifierLikePropertyName('valid')).toBe(false)
  })

  it('skips Tavily tools when required schema fields cannot be synthesized safely', async () => {
    const calls: Array<{ url: string, init?: RequestInit }> = []
    globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ url: requestUrl(input), init })
      return new Response(JSON.stringify({ jsonrpc: '2.0', id: 'ok', result: { ok: true } }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    }) as typeof fetch

    const steps = buildMcpToolCallProbeStepDefinitions(mcpProbeText, [{
      requestName: 'tavily-new',
      displayName: 'tavily-new',
      inputSchema: {
        type: 'object',
        required: ['query'],
      },
    }])

    expect(steps).toHaveLength(1)
    expect(steps[0]?.billable).toBe(true)
    await expect(steps[0]?.run('th-zjvc-secret', createMcpProbeContext())).resolves.toEqual({
      detail: '当前本地没有 tavily-new 的检测夹具，已跳过。',
      stepState: 'skipped',
    })
    expect(calls).toHaveLength(0)
  })

  it('updates the running MCP progress total after discovering tool call steps', () => {
    const baseSteps = buildMcpProbeStepDefinitions(mcpProbeText)
    const toolSteps = buildMcpToolCallProbeStepDefinitions(mcpProbeText, [
      'tavily-search',
      'tavily-extract',
      'tavily-crawl',
      'tavily-map',
      'tavily-research',
    ])

    const stepDefinitions = [...baseSteps, ...toolSteps]
    const nextModel = nextRunningMcpProbeModel(
      { state: 'running', completed: 2, total: 2 },
      stepDefinitions,
      3,
    )

    expect(nextModel).toEqual({
      state: 'running',
      completed: 3,
      total: 9,
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

    const steps = buildApiProbeStepDefinitions(apiProbeText)
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

  it('threads abort signals through API probe requests', async () => {
    const calls: Array<{ url: string, init?: RequestInit }> = []
    globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ url: requestUrl(input), init })
      return new Response(JSON.stringify({ ok: true }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    }) as typeof fetch

    const controller = new AbortController()
    const steps = buildApiProbeStepDefinitions(apiProbeText)
    await expect(steps[0]?.run('th-zjvc-secret', { requestId: null, signal: controller.signal })).resolves.toBeNull()

    expect(calls).toHaveLength(1)
    expect(calls[0]?.url).toBe('/api/tavily/search')
    expect(calls[0]?.init?.signal).toBe(controller.signal)
  })
})
