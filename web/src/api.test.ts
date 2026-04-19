import { afterEach, describe, expect, it, mock } from 'bun:test'

import {
  buildPublicEventsUrl,
  bindAdminUserTag,
  createBrowserTodayWindow,
  fetchAdminRegistrationSettings,
  fetchAdminUnboundTokenUsage,
  fetchAdminUsers,
  fetchAdminUserTags,
  fetchAlertCatalog,
  fetchAlertEvents,
  fetchAlertGroups,
  fetchApiKeys,
  fetchDashboardOverview,
  fetchJobs,
  fetchKeyLogsCatalog,
  fetchKeyLogDetails,
  fetchKeyLogsList,
  fetchPublicMetrics,
  fetchRequestLogs,
  fetchRequestLogsCatalog,
  fetchRequestLogDetails,
  fetchRequestLogsList,
  fetchSystemSettings,
  fetchTokenLogsCatalog,
  fetchTokenMetrics,
  fetchTokenLogDetails,
  fetchTokenLogsList,
  fetchUserDashboard,
  postUserLogout,
  fetchUserTokenDetail,
  fetchUserTokens,
  millisecondsUntilNextBrowserDayBoundary,
  updateForwardProxySettingsWithProgress,
  updateAdminRegistrationSettings,
  updateAdminUserQuota,
  updateSystemSettings,
  validateForwardProxyCandidateWithProgress,
} from './api'

const originalFetch = globalThis.fetch

afterEach(() => {
  globalThis.fetch = originalFetch
})

function createSseResponse(chunks: string[]): Response {
  const encoder = new TextEncoder()
  return new Response(
    new ReadableStream({
      start(controller) {
        for (const chunk of chunks) {
          controller.enqueue(encoder.encode(chunk))
        }
        controller.close()
      },
    }),
    {
      status: 200,
      headers: { 'Content-Type': 'text/event-stream' },
    },
  )
}

describe('admin user tag api helpers', () => {
  it('formats browser today windows with explicit ISO8601 offsets', () => {
    const localNoon = new Date()
    localNoon.setFullYear(2026, 2, 8)
    localNoon.setHours(12, 34, 56, 0)
    const windowRange = createBrowserTodayWindow(localNoon)

    expect(windowRange.todayStart).toMatch(/^2026-03-08T00:00:00[+-]\d{2}:\d{2}$/)
    expect(windowRange.todayEnd).toMatch(/^2026-03-09T00:00:00[+-]\d{2}:\d{2}$/)
  })

  it('computes the next browser-day refresh delay from the local clock', () => {
    const nearMidnight = new Date()
    nearMidnight.setHours(23, 59, 30, 0)
    const delay = millisecondsUntilNextBrowserDayBoundary(nearMidnight)

    expect(delay).toBe(30_000)
  })

  it('appends explicit today windows to user-facing metric endpoints', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(new Response(JSON.stringify({ monthlySuccess: 1, dailySuccess: 2 }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })),
    )
    globalThis.fetch = fetchMock as typeof fetch

    const todayWindow = {
      todayStart: '2026-04-03T00:00:00+08:00',
      todayEnd: '2026-04-04T00:00:00+08:00',
    }

    await fetchPublicMetrics(todayWindow)
    await fetchTokenMetrics('th-a1b2-secretsecret', todayWindow)
    await fetchUserDashboard(todayWindow)
    await fetchUserTokens(todayWindow)
    await fetchUserTokenDetail('a1b2', todayWindow)

    expect((fetchMock.mock.calls[0] as [string])[0]).toBe(
      '/api/public/metrics?today_start=2026-04-03T00%3A00%3A00%2B08%3A00&today_end=2026-04-04T00%3A00%3A00%2B08%3A00',
    )
    expect((fetchMock.mock.calls[1] as [string])[0]).toBe(
      '/api/token/metrics?token=th-a1b2-secretsecret&today_start=2026-04-03T00%3A00%3A00%2B08%3A00&today_end=2026-04-04T00%3A00%3A00%2B08%3A00',
    )
    expect((fetchMock.mock.calls[2] as [string])[0]).toBe(
      '/api/user/dashboard?today_start=2026-04-03T00%3A00%3A00%2B08%3A00&today_end=2026-04-04T00%3A00%3A00%2B08%3A00',
    )
    expect((fetchMock.mock.calls[3] as [string])[0]).toBe(
      '/api/user/tokens?today_start=2026-04-03T00%3A00%3A00%2B08%3A00&today_end=2026-04-04T00%3A00%3A00%2B08%3A00',
    )
    expect((fetchMock.mock.calls[4] as [string])[0]).toBe(
      '/api/user/tokens/a1b2?today_start=2026-04-03T00%3A00%3A00%2B08%3A00&today_end=2026-04-04T00%3A00%3A00%2B08%3A00',
    )
  })

  it('treats user logout 204 and 401 as successful sign-out responses', async () => {
    let status = 204
    const fetchMock = mock((_input: RequestInfo | URL) => {
      const response = new Response(null, { status })
      status = 401
      return Promise.resolve(response)
    })
    globalThis.fetch = fetchMock as typeof fetch

    await expect(postUserLogout()).resolves.toBeUndefined()
    await expect(postUserLogout()).resolves.toBeUndefined()

    expect(fetchMock).toHaveBeenCalledTimes(2)
    expect((fetchMock.mock.calls[0] as [string])[0]).toBe('/api/user/logout')
    expect((fetchMock.mock.calls[1] as [string])[0]).toBe('/api/user/logout')
  })

  it('surfaces logout request failures for the caller to render', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(new Response('upstream unavailable', { status: 503 })),
    )
    globalThis.fetch = fetchMock as typeof fetch

    await expect(postUserLogout()).rejects.toMatchObject({
      message: 'upstream unavailable',
      status: 503,
    })
  })

  it('loads the dashboard overview from the dedicated aggregate endpoint', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            summary: {
              total_requests: 1,
              success_count: 1,
              error_count: 0,
              quota_exhausted_count: 0,
              active_keys: 1,
              exhausted_keys: 0,
              quarantined_keys: 0,
              last_activity: null,
              total_quota_limit: 10,
              total_quota_remaining: 9,
            },
            summaryWindows: {
              today: { total_requests: 1, success_count: 1, error_count: 0, quota_exhausted_count: 0, valuable_success_count: 0, valuable_failure_count: 0, other_success_count: 0, other_failure_count: 0, unknown_count: 0, upstream_exhausted_key_count: 0, new_keys: 0, new_quarantines: 0 },
              yesterday: { total_requests: 0, success_count: 0, error_count: 0, quota_exhausted_count: 0, valuable_success_count: 0, valuable_failure_count: 0, other_success_count: 0, other_failure_count: 0, unknown_count: 0, upstream_exhausted_key_count: 0, new_keys: 0, new_quarantines: 0 },
              month: { total_requests: 1, success_count: 1, error_count: 0, quota_exhausted_count: 0, valuable_success_count: 0, valuable_failure_count: 0, other_success_count: 0, other_failure_count: 0, unknown_count: 0, upstream_exhausted_key_count: 0, new_keys: 0, new_quarantines: 0 },
            },
            siteStatus: {
              remainingQuota: 9,
              totalQuotaLimit: 10,
              activeKeys: 1,
              quarantinedKeys: 0,
              exhaustedKeys: 0,
              availableProxyNodes: 1,
              totalProxyNodes: 1,
            },
            forwardProxy: { availableNodes: 1, totalNodes: 1 },
            hourlyRequestWindow: {
              bucketSeconds: 3600,
              visibleBuckets: 25,
              retainedBuckets: 49,
              buckets: Array.from({ length: 49 }, (_, index) => ({
                bucketStart: 1_775_534_400 + index * 3600,
                secondarySuccess: 0,
                primarySuccess: index === 48 ? 1 : 0,
                secondaryFailure: 0,
                primaryFailure429: 0,
                primaryFailureOther: 0,
                unknown: 0,
                mcpNonBillable: 0,
                mcpBillable: 0,
                apiNonBillable: 0,
                apiBillable: index === 48 ? 1 : 0,
              })),
            },
            trend: { request: [1, 0, 0, 0, 0, 0, 0, 0], error: [0, 0, 0, 0, 0, 0, 0, 0] },
            exhaustedKeys: [],
            recentLogs: [],
            recentJobs: [],
            disabledTokens: [],
            tokenCoverage: 'ok',
            recentAlerts: {
              windowHours: 24,
              totalEvents: 3,
              groupedCount: 2,
              countsByType: [
                { type: 'upstream_rate_limited_429', count: 1 },
                { type: 'user_quota_exhausted', count: 2 },
              ],
              topGroups: [
                {
                  id: 'group-1',
                  type: 'user_quota_exhausted',
                  subjectKind: 'user',
                  subjectId: 'usr_001',
                  subjectLabel: 'Alice Wang',
                  user: { userId: 'usr_001', displayName: 'Alice Wang', username: 'alice' },
                  token: { id: 'tok_001', label: 'tok_001' },
                  key: null,
                  requestKind: { key: 'tavily_search', label: 'Tavily Search', detail: null },
                  count: 2,
                  firstSeen: 1_775_534_400,
                  lastSeen: 1_775_535_400,
                  latestEvent: {
                    id: 'alert_evt_001',
                    type: 'user_quota_exhausted',
                    title: 'User quota exhausted',
                    summary: 'Alice Wang reached the quota ceiling.',
                    occurredAt: 1_775_535_400,
                    subjectKind: 'user',
                    subjectId: 'usr_001',
                    subjectLabel: 'Alice Wang',
                    user: { userId: 'usr_001', displayName: 'Alice Wang', username: 'alice' },
                    token: { id: 'tok_001', label: 'tok_001' },
                    key: null,
                    request: { id: 91, method: 'POST', path: '/api/tavily/search', query: null },
                    requestKind: { key: 'tavily_search', label: 'Tavily Search', detail: null },
                    failureKind: null,
                    resultStatus: 'quota_exhausted',
                    errorMessage: 'quota exhausted',
                    reasonCode: null,
                    reasonSummary: null,
                    reasonDetail: null,
                    source: { kind: 'auth_token_log', id: 'log_91' },
                  },
                },
              ],
            },
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    const overview = await fetchDashboardOverview()

    expect((fetchMock.mock.calls[0] as [string])[0]).toBe('/api/dashboard/overview')
    expect(overview.siteStatus.activeKeys).toBe(1)
    expect(overview.trend.request).toHaveLength(8)
    expect(overview.hourlyRequestWindow.buckets).toHaveLength(49)
    expect(overview.tokenCoverage).toBe('ok')
    expect(overview.recentAlerts.totalEvents).toBe(3)
    expect(overview.recentAlerts.topGroups[0]?.latestEvent.request?.id).toBe(91)
  })

  it('fetches the alert catalog from the dedicated endpoint', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            retentionDays: 30,
            types: [{ value: 'upstream_rate_limited_429', count: 2 }],
            requestKindOptions: [
              {
                key: 'tavily_search',
                label: 'Tavily Search',
                protocol_group: 'api',
                billing_group: 'billable',
                count: 2,
              },
            ],
            users: [{ value: 'usr_001', label: 'Alice Wang', count: 2 }],
            tokens: [{ value: 'tok_001', label: 'tok_001', count: 2 }],
            keys: [{ value: 'key_001', label: 'key_001', count: 1 }],
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    const catalog = await fetchAlertCatalog()

    expect((fetchMock.mock.calls[0] as [string])[0]).toBe('/api/alerts/catalog')
    expect(catalog.types[0]).toEqual({ value: 'upstream_rate_limited_429', count: 2 })
    expect(catalog.requestKindOptions[0]?.key).toBe('tavily_search')
    expect(catalog.users[0]?.label).toBe('Alice Wang')
  })

  it('builds repeated alert query params and normalizes event payloads', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            items: [
              {
                id: 'alert_evt_001',
                type: 'upstream_rate_limited_429',
                title: 'Upstream 429',
                summary: 'The upstream returned 429.',
                occurredAt: 1_775_535_400,
                subjectKind: 'user',
                subjectId: 'usr_001',
                subjectLabel: 'Alice Wang',
                user: { userId: 'usr_001', displayName: 'Alice Wang', username: 'alice' },
                token: { id: 'tok_001', label: 'tok_001' },
                key: { id: 'key_001', label: 'key_001' },
                request: { id: 91, method: 'POST', path: '/api/tavily/search', query: null },
                requestKind: { key: 'tavily_search', label: 'Tavily Search', detail: 'POST /api/tavily/search' },
                failureKind: 'upstream_rate_limited_429',
                resultStatus: 'error',
                errorMessage: 'HTTP 429',
                reasonCode: null,
                reasonSummary: null,
                reasonDetail: null,
                source: { kind: 'auth_token_log', id: 'log_91' },
              },
            ],
            total: 1,
            page: 2,
            per_page: 50,
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    const page = await fetchAlertEvents({
      page: 2,
      perPage: 50,
      type: 'upstream_rate_limited_429',
      since: '2026-04-18T00:00:00+08:00',
      until: '2026-04-18T12:00:00+08:00',
      userId: 'usr_001',
      tokenId: 'tok_001',
      keyId: 'key_001',
      requestKinds: ['tavily_search', 'mcp_search'],
    })

    expect((fetchMock.mock.calls[0] as [string])[0]).toBe(
      '/api/alerts/events?page=2&per_page=50&type=upstream_rate_limited_429&since=2026-04-18T00%3A00%3A00%2B08%3A00&until=2026-04-18T12%3A00%3A00%2B08%3A00&user_id=usr_001&token_id=tok_001&key_id=key_001&request_kind=tavily_search&request_kind=mcp_search',
    )
    expect(page.perPage).toBe(50)
    expect(page.items[0]?.requestKind?.label).toBe('Tavily Search')
    expect(page.items[0]?.source.kind).toBe('auth_token_log')
  })

  it('normalizes grouped alert payloads from the groups endpoint', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            items: [
              {
                id: 'group-1',
                type: 'upstream_key_blocked',
                subjectKind: 'key',
                subjectId: 'key_001',
                subjectLabel: 'key_001',
                user: null,
                token: null,
                key: { id: 'key_001', label: 'key_001' },
                requestKind: { key: 'mcp_search', label: 'MCP Search', detail: 'POST /mcp' },
                count: 2,
                firstSeen: 1_775_534_400,
                lastSeen: 1_775_535_400,
                latestEvent: {
                  id: 'alert_evt_003',
                  type: 'upstream_key_blocked',
                  title: 'Upstream key blocked',
                  summary: 'The upstream disabled key_001.',
                  occurredAt: 1_775_535_400,
                  subjectKind: 'key',
                  subjectId: 'key_001',
                  subjectLabel: 'key_001',
                  user: null,
                  token: null,
                  key: { id: 'key_001', label: 'key_001' },
                  request: null,
                  requestKind: { key: 'mcp_search', label: 'MCP Search', detail: 'POST /mcp' },
                  failureKind: null,
                  resultStatus: null,
                  errorMessage: null,
                  reasonCode: 'account_deactivated',
                  reasonSummary: 'Upstream account deactivated',
                  reasonDetail: 'quarantined locally',
                  source: { kind: 'api_key_maintenance_record', id: 'maint_3' },
                },
              },
            ],
            total: 1,
            page: 1,
            perPage: 20,
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    const page = await fetchAlertGroups({
      page: 1,
      perPage: 20,
      requestKinds: ['mcp_search'],
    })

    expect((fetchMock.mock.calls[0] as [string])[0]).toBe(
      '/api/alerts/groups?page=1&per_page=20&request_kind=mcp_search',
    )
    expect(page.items[0]?.count).toBe(2)
    expect(page.items[0]?.latestEvent.reasonCode).toBe('account_deactivated')
    expect(page.items[0]?.requestKind?.key).toBe('mcp_search')
  })

  it('builds the public SSE url with token and explicit today windows', () => {
    expect(buildPublicEventsUrl('th-a1b2-secretsecret', {
      todayStart: '2026-04-03T00:00:00+08:00',
      todayEnd: '2026-04-04T00:00:00+08:00',
    })).toBe(
      '/api/public/events?token=th-a1b2-secretsecret&today_start=2026-04-03T00%3A00%3A00%2B08%3A00&today_end=2026-04-04T00%3A00%3A00%2B08%3A00',
    )
  })

  it('streams forward proxy validation progress events before returning the final payload', async () => {
    const events: string[] = []
    const fetchMock = mock(() =>
      Promise.resolve(
        createSseResponse([
          'data: {"type":"phase","operation":"validate","phaseKey":"parse_input","label":"Parse input"}\n\n',
          'data: {"type":"nodes","operation":"validate","nodes":[{"nodeKey":"edge-a","displayName":"edge-a","protocol":"ss","status":"pending"}]}\n\n',
          'data: {"type":"node","operation":"validate","node":{"nodeKey":"edge-a","displayName":"edge-a","protocol":"ss","status":"probing"}}\n\n',
          'data: {"type":"phase","operation":"validate","phaseKey":"probe_nodes","label":"Probe nodes","current":1,"total":3,"detail":"edge-a"}\n\n',
          'data: {"type":"node","operation":"validate","node":{"nodeKey":"edge-a","displayName":"edge-a","protocol":"ss","status":"ok","ok":true,"latencyMs":42,"ip":"203.0.113.8","location":"JP / NRT"}}\n\n',
          'data: {"type":"complete","operation":"validate","payload":{"ok":true,"message":"proxy validation succeeded","normalizedValue":"http://127.0.0.1:8080","discoveredNodes":1,"latencyMs":42,"nodes":[{"displayName":"edge-a","protocol":"ss","ok":true,"ip":"203.0.113.8","location":"JP / NRT","latencyMs":42}]}}\n\n',
        ]),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    const payload = await validateForwardProxyCandidateWithProgress(
      { kind: 'proxyUrl', value: 'http://127.0.0.1:8080' },
      (event) => events.push(`${event.type}:${event.operation}:${'phaseKey' in event ? event.phaseKey ?? 'none' : 'complete'}`),
    )

    expect(payload.ok).toBe(true)
    expect(payload.nodes?.[0]).toMatchObject({
      displayName: 'edge-a',
      protocol: 'ss',
      ip: '203.0.113.8',
      location: 'JP / NRT',
    })
    expect(events).toEqual([
      'phase:validate:parse_input',
      'nodes:validate:complete',
      'node:validate:complete',
      'phase:validate:probe_nodes',
      'node:validate:complete',
      'complete:validate:complete',
    ])
  })

  it('falls back to JSON forward proxy save responses without breaking callers', async () => {
    const seen: string[] = []
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            proxyUrls: ['http://127.0.0.1:8080'],
            subscriptionUrls: [],
            subscriptionUpdateIntervalSecs: 3600,
            insertDirect: true,
            egressSocks5Enabled: false,
            egressSocks5Url: '',
            nodes: [],
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    const payload = await updateForwardProxySettingsWithProgress(
      {
        proxyUrls: ['http://127.0.0.1:8080'],
        subscriptionUrls: [],
        subscriptionUpdateIntervalSecs: 3600,
        insertDirect: true,
        egressSocks5Enabled: false,
        egressSocks5Url: '',
      },
      (event) => seen.push(event.type),
    )

    expect(payload.proxyUrls).toEqual(['http://127.0.0.1:8080'])
    expect(payload.egressSocks5Enabled).toBe(false)
    expect(seen).toEqual(['complete'])
  })

  it('parses new global SOCKS5 save phases from SSE responses', async () => {
    const phases: string[] = []
    const fetchMock = mock(() =>
      Promise.resolve(
        createSseResponse([
          'data: {"type":"phase","operation":"save","phaseKey":"validate_egress_socks5","label":"Validate global SOCKS5 relay"}\n\n',
          'data: {"type":"phase","operation":"save","phaseKey":"apply_egress_socks5","label":"Apply global SOCKS5 relay"}\n\n',
          'data: {"type":"complete","operation":"save","payload":{"proxyUrls":[],"subscriptionUrls":[],"subscriptionUpdateIntervalSecs":3600,"insertDirect":true,"egressSocks5Enabled":true,"egressSocks5Url":"socks5h://127.0.0.1:1080","nodes":[]}}\n\n',
        ]),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    const payload = await updateForwardProxySettingsWithProgress(
      {
        proxyUrls: [],
        subscriptionUrls: [],
        subscriptionUpdateIntervalSecs: 3600,
        insertDirect: true,
        egressSocks5Enabled: true,
        egressSocks5Url: 'socks5h://127.0.0.1:1080',
      },
      (event) => {
        if (event.type === 'phase') phases.push(event.phaseKey)
      },
    )

    expect(phases).toEqual(['validate_egress_socks5', 'apply_egress_socks5'])
    expect(payload.egressSocks5Enabled).toBe(true)
    expect(payload.egressSocks5Url).toBe('socks5h://127.0.0.1:1080')
  })

  it('supports aborting forward proxy validation progress requests', async () => {
    const fetchMock = mock((_input: RequestInfo | URL, init?: RequestInit) =>
      new Promise<Response>((_resolve, reject) => {
        init?.signal?.addEventListener(
          'abort',
          () => reject(new DOMException('The operation was aborted.', 'AbortError')),
          { once: true },
        )
      }))
    globalThis.fetch = fetchMock as typeof fetch

    const controller = new AbortController()
    const promise = validateForwardProxyCandidateWithProgress(
      { kind: 'proxyUrl', value: 'http://127.0.0.1:8080' },
      undefined,
      controller.signal,
    )
    controller.abort()

    await expect(promise).rejects.toMatchObject({ name: 'AbortError' })
  })

  it('unwraps tag catalog list responses', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            items: [
              {
                id: 'linuxdo_l2',
                name: 'linuxdo_l2',
                displayName: 'L2',
                icon: 'linuxdo',
                systemKey: 'linuxdo_l2',
                effectKind: 'quota_delta',
                hourlyAnyDelta: 0,
                hourlyDelta: 0,
                dailyDelta: 0,
                monthlyDelta: 0,
                userCount: 4,
              },
            ],
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    const tags = await fetchAdminUserTags()

    expect(tags).toHaveLength(1)
    expect(tags[0]).toMatchObject({
      id: 'linuxdo_l2',
      displayName: 'L2',
      systemKey: 'linuxdo_l2',
      effectKind: 'quota_delta',
    })
  })

  it('sends user tag binding requests to the user-scoped endpoint', async () => {
    const fetchMock = mock(() => Promise.resolve(new Response(null, { status: 204 })))
    globalThis.fetch = fetchMock as typeof fetch

    await bindAdminUserTag('usr_alice', 'team_lead')

    expect(fetchMock).toHaveBeenCalledTimes(1)
    const [input, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(input).toBe('/api/users/usr_alice/tags')
    expect(init.method).toBe('POST')
    expect(init.body).toBe(JSON.stringify({ tagId: 'team_lead' }))
  })

  it('sends exact tag filters and sort params when listing admin users', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            items: [],
            total: 0,
            page: 1,
            per_page: 20,
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    await fetchAdminUsers(1, 20, 'L2', 'linuxdo_l2', 'monthlySuccessRate', 'asc')

    expect(fetchMock).toHaveBeenCalledTimes(1)
    const [input] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(input).toBe('/api/users?page=1&per_page=20&q=L2&tagId=linuxdo_l2&sort=monthlySuccessRate&order=asc')
  })

  it('sends exact search and sort params when listing unbound token usage', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            items: [],
            total: 0,
            page: 2,
            perPage: 20,
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    const result = await fetchAdminUnboundTokenUsage(2, 20, 'ops', 'monthlyBrokenCount', 'asc')

    expect(fetchMock).toHaveBeenCalledTimes(1)
    const [input] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(input).toBe('/api/tokens/unbound-usage?page=2&per_page=20&q=ops&sort=monthlyBrokenCount&order=asc')
    expect(result.page).toBe(2)
    expect(result.perPage).toBe(20)
  })

  it('sends repeated key group and status filters when listing paginated api keys', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            items: [],
            total: 0,
            page: 2,
            perPage: 50,
            facets: {
              groups: [{ value: 'ops', count: 3 }],
              statuses: [{ value: 'quarantined', count: 2 }],
              regions: [{ value: 'US', count: 1 }],
            },
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    const result = await fetchApiKeys(2, 50, {
      groups: ['ops', ''],
      statuses: ['Quarantined', 'disabled'],
      registrationIp: '8.8.8.8',
      regions: ['US', 'US Westfield (MA)'],
    })

    expect(fetchMock).toHaveBeenCalledTimes(1)
    const [input] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(input).toBe(
      '/api/keys?page=2&per_page=50&group=ops&group=&status=quarantined&status=disabled&registration_ip=8.8.8.8&region=US&region=US+Westfield+%28MA%29',
    )
    expect(result.page).toBe(2)
    expect(result.perPage).toBe(50)
    expect(result.facets.groups[0]).toEqual({ value: 'ops', count: 3 })
    expect(result.facets.regions[0]).toEqual({ value: 'US', count: 1 })
  })

  it('patches base quota through the existing user quota endpoint', async () => {
    const fetchMock = mock(() => Promise.resolve(new Response(null, { status: 204 })))
    globalThis.fetch = fetchMock as typeof fetch

    await updateAdminUserQuota('usr_alice', {
      hourlyAnyLimit: 1200,
      hourlyLimit: 1000,
      dailyLimit: 24000,
      monthlyLimit: 600000,
    })

    expect(fetchMock).toHaveBeenCalledTimes(1)
    const [input, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(input).toBe('/api/users/usr_alice/quota')
    expect(init.method).toBe('PATCH')
    expect(init.body).toBe(
      JSON.stringify({
        hourlyAnyLimit: 1200,
        hourlyLimit: 1000,
        dailyLimit: 24000,
        monthlyLimit: 600000,
      }),
    )
  })

  it('reads admin registration settings from the dedicated endpoint', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(JSON.stringify({ allowRegistration: false }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    const settings = await fetchAdminRegistrationSettings()

    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/admin/registration')
    expect(settings).toEqual({ allowRegistration: false })
  })

  it('patches admin registration settings through the dedicated endpoint', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(JSON.stringify({ allowRegistration: true }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    const settings = await updateAdminRegistrationSettings(true)

    expect(fetchMock).toHaveBeenCalledTimes(1)
    const [input, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(input).toBe('/api/admin/registration')
    expect(init.method).toBe('PATCH')
    expect(init.body).toBe(JSON.stringify({ allowRegistration: true }))
    expect(settings).toEqual({ allowRegistration: true })
  })

  it('normalizes jobs responses to the snake_case shape used by the admin UI', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            items: [
              {
                id: 37696,
                jobType: 'quota_sync',
                keyId: '7QZ5',
                keyGroup: 'ops',
                status: 'error',
                attempt: 1,
                message: 'usage_http 401',
                startedAt: 1_773_344_460,
                finishedAt: 1_773_344_470,
              },
            ],
            total: 1,
            page: 1,
            perPage: 10,
            groupCounts: {
              all: 4,
              quota: 1,
              usage: 1,
              logs: 1,
              geo: 0,
              linuxdo: 1,
            },
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    const jobs = await fetchJobs()

    expect(jobs.page).toBe(1)
    expect(jobs.perPage).toBe(10)
    expect(jobs.groupCounts).toEqual({
      all: 4,
      quota: 1,
      usage: 1,
      logs: 1,
      geo: 0,
      linuxdo: 1,
    })
    expect(jobs.items[0]).toEqual({
      id: 37696,
      job_type: 'quota_sync',
      key_id: '7QZ5',
      key_group: 'ops',
      status: 'error',
      attempt: 1,
      message: 'usage_http 401',
      started_at: 1_773_344_460,
      finished_at: 1_773_344_470,
    })
  })

  it('passes the geo job filter through to the jobs API', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            items: [],
            total: 0,
            page: 1,
            perPage: 10,
            groupCounts: {
              all: 0,
              quota: 0,
              usage: 0,
              logs: 0,
              geo: 0,
              linuxdo: 0,
            },
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    await fetchJobs(1, 10, 'geo')

    const [input] = fetchMock.mock.calls[0] as [string]
    expect(input).toBe('/api/jobs?page=1&per_page=10&group=geo')
  })

  it('passes the operational class filter through to the admin logs API', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            items: [],
            total: 0,
            page: 1,
            perPage: 20,
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    await fetchRequestLogs(1, 20, 'error', undefined, 'neutral')

    const [input] = fetchMock.mock.calls[0] as [string]
    expect(input).toBe(
      '/api/logs?page=1&per_page=20&result=error&operational_class=neutral&include_bodies=true',
    )
  })

  it('fetches global log bodies from the dedicated detail endpoint', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(JSON.stringify({ request_body: '{"query":"health"}', response_body: '{"ok":true}' }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    const detail = await fetchRequestLogDetails(481)

    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/logs/481/details')
    expect(detail).toEqual({ request_body: '{"query":"health"}', response_body: '{"ok":true}' })
  })

  it('fetches key-scoped log bodies from the dedicated detail endpoint', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(JSON.stringify({ request_body: null, response_body: null }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    const detail = await fetchKeyLogDetails('CBoX', 9512)

    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/keys/CBoX/logs/9512/details')
    expect(detail).toEqual({ request_body: null, response_body: null })
  })

  it('fetches token-scoped log bodies from the dedicated detail endpoint', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(JSON.stringify({ request_body: '{"tool":"search"}', response_body: null }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    const detail = await fetchTokenLogDetails('ZjvC', 73)

    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/tokens/ZjvC/logs/73/details')
    expect(detail).toEqual({ request_body: '{"tool":"search"}', response_body: null })
  })

  it('builds cursor-based admin request log list URLs', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            items: [],
            pageSize: 20,
            nextCursor: '300:3',
            prevCursor: null,
            hasOlder: true,
            hasNewer: false,
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    await fetchRequestLogsList({
      limit: 20,
      cursor: '400:4',
      direction: 'older',
      requestKinds: ['api:search', 'mcp:search'],
      result: 'error',
      keyId: 'K001',
    })

    expect(fetchMock.mock.calls[0]?.[0]).toBe(
      '/api/logs/list?limit=20&cursor=400%3A4&direction=older&request_kind=api%3Asearch&request_kind=mcp%3Asearch&result=error&key_id=K001',
    )
  })

  it('builds admin request log catalog URLs across scopes', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            retentionDays: 32,
            requestKindOptions: [],
            facets: {
              results: [],
              keyEffects: [],
              bindingEffects: [{ value: 'http_project_affinity_bound', count: 1 }],
              selectionEffects: [{ value: 'http_project_affinity_pressure_avoided', count: 2 }],
              tokens: [],
              keys: [],
            },
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    await fetchRequestLogsCatalog({
      requestKinds: ['api:search'],
      result: 'error',
      keyId: 'K001',
    })
    await fetchKeyLogsCatalog('K001', {
      since: 0,
      requestKinds: ['mcp:search'],
      bindingEffect: 'http_project_affinity_reused',
      tokenId: 'T001',
    })
    await fetchTokenLogsCatalog('T001', {
      sinceIso: '2026-04-01T00:00:00+08:00',
      untilIso: '2026-04-02T00:00:00+08:00',
      requestKinds: ['api:extract'],
      result: 'quota_exhausted',
      keyId: 'K001',
    })

    expect(fetchMock.mock.calls[0]?.[0]).toBe(
      '/api/logs/catalog?request_kind=api%3Asearch&result=error&key_id=K001',
    )
    expect(fetchMock.mock.calls[1]?.[0]).toBe(
      '/api/keys/K001/logs/catalog?request_kind=mcp%3Asearch&binding_effect=http_project_affinity_reused&auth_token_id=T001&since=0',
    )
    expect(fetchMock.mock.calls[2]?.[0]).toBe(
      '/api/tokens/T001/logs/catalog?request_kind=api%3Aextract&result=quota_exhausted&key_id=K001&since=2026-04-01T00%3A00%3A00%2B08%3A00&until=2026-04-02T00%3A00%3A00%2B08%3A00',
    )
    const catalog = await fetchRequestLogsCatalog()
    expect(catalog.facets.bindingEffects).toEqual([{ value: 'http_project_affinity_bound', count: 1 }])
    expect(catalog.facets.selectionEffects).toEqual([
      { value: 'http_project_affinity_pressure_avoided', count: 2 },
    ])
  })

  it('builds cursor-based scoped request log list URLs', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            items: [],
            pageSize: 10,
            nextCursor: null,
            prevCursor: '200:2',
            hasOlder: false,
            hasNewer: true,
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    await fetchKeyLogsList('K001', {
      limit: 10,
      direction: 'newer',
      cursor: '150:1',
      since: 100,
      requestKinds: ['api:extract'],
    })
    await fetchTokenLogsList('T001', {
      limit: 10,
      direction: 'older',
      sinceIso: '2026-04-01T00:00:00+08:00',
      untilIso: '2026-04-02T00:00:00+08:00',
      keyId: 'K001',
      selectionEffect: 'mcp_session_init_pressure_avoided',
      operationalClass: 'neutral',
    })

    expect(fetchMock.mock.calls[0]?.[0]).toBe(
      '/api/keys/K001/logs/list?limit=10&cursor=150%3A1&direction=newer&request_kind=api%3Aextract&since=100',
    )
    expect(fetchMock.mock.calls[1]?.[0]).toBe(
      '/api/tokens/T001/logs/list?limit=10&direction=older&selection_effect=mcp_session_init_pressure_avoided&operational_class=neutral&key_id=K001&since=2026-04-01T00%3A00%3A00%2B08%3A00&until=2026-04-02T00%3A00%3A00%2B08%3A00',
    )
  })

  it('loads system settings including the request-rate threshold', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            systemSettings: {
              requestRateLimit: 72,
              mcpSessionAffinityKeyCount: 3,
              rebalanceMcpEnabled: true,
              rebalanceMcpSessionPercent: 35,
            },
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    await expect(fetchSystemSettings()).resolves.toEqual({
      requestRateLimit: 72,
      mcpSessionAffinityKeyCount: 3,
      rebalanceMcpEnabled: true,
      rebalanceMcpSessionPercent: 35,
    })
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/settings')
  })

  it('updates system settings with requestRateLimit in the payload body', async () => {
    const fetchMock = mock((_input: RequestInfo | URL, init?: RequestInit) =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            requestRateLimit: 75,
            mcpSessionAffinityKeyCount: 4,
            rebalanceMcpEnabled: false,
            rebalanceMcpSessionPercent: 100,
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    await expect(
      updateSystemSettings({
        requestRateLimit: 75,
        mcpSessionAffinityKeyCount: 4,
        rebalanceMcpEnabled: false,
        rebalanceMcpSessionPercent: 100,
      }),
    ).resolves.toEqual({
      requestRateLimit: 75,
      mcpSessionAffinityKeyCount: 4,
      rebalanceMcpEnabled: false,
      rebalanceMcpSessionPercent: 100,
    })

    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/settings/system')
    expect(fetchMock.mock.calls[0]?.[1]).toMatchObject({
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        requestRateLimit: 75,
        mcpSessionAffinityKeyCount: 4,
        rebalanceMcpEnabled: false,
        rebalanceMcpSessionPercent: 100,
      }),
    })
  })
})
