import '../../test/happydom'

import { afterEach, describe, expect, it } from 'bun:test'
import { act, type ComponentProps } from 'react'
import { createRoot, type Root } from 'react-dom/client'

import type { AlertCatalog, AlertEvent, AlertGroup, AlertsPage } from '../api'
import AlertsCenter from './AlertsCenter'
import { alertsPath } from './routes'

const storyCatalog: AlertCatalog = {
  retentionDays: 30,
  types: [
    { value: 'upstream_usage_limit_432', count: 2 },
    { value: 'upstream_rate_limited_429', count: 1 },
  ],
  requestKindOptions: [
    { key: 'tavily_search', label: 'Tavily Search', protocol_group: 'api', billing_group: 'billable', count: 2 },
    { key: 'mcp_search', label: 'MCP Search', protocol_group: 'mcp', billing_group: 'billable', count: 1 },
  ],
  users: [{ value: 'usr_alice', label: 'Alice Wang', count: 2 }],
  tokens: [{ value: 'tok_ops_01', label: 'tok_ops_01', count: 2 }],
  keys: [{ value: 'key_001', label: 'key_001', count: 1 }],
}

const storyEvents: AlertsPage<AlertEvent> = {
  page: 1,
  perPage: 20,
  total: 1,
  items: [
    {
      id: 'alert_evt_001',
      type: 'upstream_usage_limit_432',
      title: '上游用量限制 432',
      summary: 'Alice Wang 的 Tavily Search 请求命中了上游 Tavily 用量限制。',
      occurredAt: 1_776_220_680,
      subjectKind: 'user',
      subjectId: 'usr_alice',
      subjectLabel: 'Alice Wang',
      user: { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
      token: { id: 'tok_ops_01', label: 'tok_ops_01' },
      key: { id: 'key_001', label: 'key_001' },
      request: { id: 501, method: 'POST', path: '/api/tavily/search', query: null },
      requestKind: { key: 'tavily_search', label: 'Tavily Search', detail: 'POST /api/tavily/search' },
      failureKind: null,
      resultStatus: 'quota_exhausted',
      errorMessage: 'This request exceeds your plan\'s set usage limit.',
      reasonCode: null,
      reasonSummary: null,
      reasonDetail: null,
      source: { kind: 'auth_token_log', id: 'log_501' },
    },
  ],
}

const compatibilityGroups: AlertsPage<AlertGroup> = {
  page: 1,
  perPage: 20,
  total: 1,
  items: [
    {
      id: 'group:upstream_usage_limit_432:key:key_001:tavily_search',
      type: 'upstream_usage_limit_432',
      subjectKind: 'key',
      subjectId: 'key_001',
      subjectLabel: 'key_001',
      user: null,
      token: { id: 'tok_ops_01', label: 'tok_ops_01' },
      key: { id: 'key_001', label: 'key_001' },
      requestKind: { key: 'mcp_search', label: 'MCP | search', detail: 'search' },
      count: 2,
      firstSeen: 1_776_220_680,
      lastSeen: 1_776_221_040,
      latestEvent: {
        ...storyEvents.items[0],
        id: 'alert_evt_compat_001',
        type: 'upstream_usage_limit_432',
        title: '上游用量限制 432',
        summary: 'A mock upstream key reached its monthly limit.',
      },
      groupingKind: 'compat',
      semanticWindowKind: null,
      semanticWindowMinutes: null,
      semanticWindowStart: null,
      semanticWindowEnd: null,
      semanticWindowKey: null,
      childCount: 0,
      eventCount: 2,
      children: [],
      childEvents: [],
    },
  ],
}

const semanticGroups: AlertsPage<AlertGroup> = {
  page: 1,
  perPage: 20,
  total: 1,
  items: [
    {
      id: 'group:user_request_rate_limited:user:usr_alice:request_rate:mother:0',
      type: 'user_request_rate_limited',
      subjectKind: 'user',
      subjectId: 'usr_alice',
      subjectLabel: 'Alice Wang',
      user: { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
      token: { id: 'tok_ops_01', label: 'tok_ops_01' },
      key: null,
      requestKind: null,
      count: 4,
      firstSeen: 1_776_220_200,
      lastSeen: 1_776_220_680,
      latestEvent: {
        ...storyEvents.items[0],
        id: 'alert_evt_010',
        type: 'user_request_rate_limited',
        title: '用户请求限流',
        summary: 'Alice Wang hit the local rolling request-rate window for MCP resources/list.',
        occurredAt: 1_776_220_680,
        requestKind: { key: 'mcp_resources_list', label: 'MCP resources/list', detail: 'resources/list' },
        errorMessage: 'user request rate limit exceeded on rolling 5m window (limit 25, used 25)',
      },
      groupingKind: 'mother',
      semanticWindowKind: 'request_rate',
      semanticWindowMinutes: 5,
      semanticWindowStart: 1_776_219_900,
      semanticWindowEnd: 1_776_220_680,
      childCount: 2,
      eventCount: 4,
      children: [
        {
          id: 'group:user_request_rate_limited:user:usr_alice:request_rate:child:0',
          type: 'user_request_rate_limited',
          subjectKind: 'user',
          subjectId: 'usr_alice',
          subjectLabel: 'Alice Wang',
          user: { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
          token: { id: 'tok_ops_01', label: 'tok_ops_01' },
          key: null,
          requestKind: null,
          count: 2,
          firstSeen: 1_776_220_200,
          lastSeen: 1_776_220_260,
          latestEvent: {
            ...storyEvents.items[0],
            id: 'alert_evt_011',
            type: 'user_request_rate_limited',
            title: '用户请求限流',
            summary: 'Alice Wang hit the local rolling request-rate window for MCP tools/list.',
            occurredAt: 1_776_220_260,
            requestKind: { key: 'mcp_tools_list', label: 'MCP tools/list', detail: 'tools/list' },
            request: { id: 502, method: 'POST', path: '/mcp', query: null },
            errorMessage: 'user request rate limit exceeded on rolling 5m window (limit 25, used 25)',
          },
          groupingKind: 'child',
          semanticWindowKind: 'request_rate',
          semanticWindowMinutes: 5,
          semanticWindowStart: 1_776_219_900,
          semanticWindowEnd: 1_776_220_260,
          semanticWindowKey: 'request_rate:test:0',
          childCount: 0,
          eventCount: 2,
          children: [],
          childEvents: [
            {
              ...storyEvents.items[0],
              id: 'alert_evt_011',
              type: 'user_request_rate_limited',
              title: '用户请求限流',
              summary: 'Alice Wang hit the local rolling request-rate window for MCP tools/list.',
              occurredAt: 1_776_220_260,
              requestKind: { key: 'mcp_tools_list', label: 'MCP tools/list', detail: 'tools/list' },
              request: { id: 502, method: 'POST', path: '/mcp', query: null },
              errorMessage: 'user request rate limit exceeded on rolling 5m window (limit 25, used 25)',
            },
            {
              ...storyEvents.items[0],
              id: 'alert_evt_012',
              type: 'user_request_rate_limited',
              title: '用户请求限流',
              summary: 'Alice Wang hit the local rolling request-rate window for MCP initialize.',
              occurredAt: 1_776_220_200,
              requestKind: { key: 'mcp_initialize', label: 'MCP initialize', detail: 'initialize' },
              request: { id: 503, method: 'POST', path: '/mcp', query: null },
              errorMessage: 'user request rate limit exceeded on rolling 5m window (limit 25, used 25)',
            },
          ],
        },
        {
          id: 'group:user_request_rate_limited:user:usr_alice:request_rate:child:1',
          type: 'user_request_rate_limited',
          subjectKind: 'user',
          subjectId: 'usr_alice',
          subjectLabel: 'Alice Wang',
          user: { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
          token: { id: 'tok_ops_01', label: 'tok_ops_01' },
          key: null,
          requestKind: null,
          count: 2,
          firstSeen: 1_776_220_620,
          lastSeen: 1_776_220_680,
          latestEvent: {
            ...storyEvents.items[0],
            id: 'alert_evt_010',
            type: 'user_request_rate_limited',
            title: '用户请求限流',
            summary: 'Alice Wang hit the local rolling request-rate window for MCP resources/list.',
            occurredAt: 1_776_220_680,
            requestKind: { key: 'mcp_resources_list', label: 'MCP resources/list', detail: 'resources/list' },
            request: { id: 504, method: 'POST', path: '/mcp', query: null },
            errorMessage: 'user request rate limit exceeded on rolling 5m window (limit 25, used 25)',
          },
          groupingKind: 'child',
          semanticWindowKind: 'request_rate',
          semanticWindowMinutes: 5,
          semanticWindowStart: 1_776_220_320,
          semanticWindowEnd: 1_776_220_680,
          semanticWindowKey: 'request_rate:test:1',
          childCount: 0,
          eventCount: 2,
          children: [],
          childEvents: [
            {
              ...storyEvents.items[0],
              id: 'alert_evt_010',
              type: 'user_request_rate_limited',
              title: '用户请求限流',
              summary: 'Alice Wang hit the local rolling request-rate window for MCP resources/list.',
              occurredAt: 1_776_220_680,
              requestKind: { key: 'mcp_resources_list', label: 'MCP resources/list', detail: 'resources/list' },
              request: { id: 504, method: 'POST', path: '/mcp', query: null },
              errorMessage: 'user request rate limit exceeded on rolling 5m window (limit 25, used 25)',
            },
            {
              ...storyEvents.items[0],
              id: 'alert_evt_013',
              type: 'user_request_rate_limited',
              title: '用户请求限流',
              summary: 'Alice Wang hit the local rolling request-rate window for MCP notifications/initialized.',
              occurredAt: 1_776_220_620,
              requestKind: { key: 'mcp_notifications_initialized', label: 'MCP notifications/initialized', detail: 'notifications/initialized' },
              request: { id: 505, method: 'POST', path: '/mcp', query: null },
              errorMessage: 'user request rate limit exceeded on rolling 5m window (limit 25, used 25)',
            },
          ],
        },
      ],
      childEvents: [],
    },
  ],
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((innerResolve) => {
    resolve = innerResolve
  })
  return { promise, resolve }
}

async function flushEffects(): Promise<void> {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
    await Promise.resolve()
  })
}

interface MountedAlertsCenter {
  container: HTMLDivElement
  root: Root
  rerender: (patch?: Partial<AlertsCenterProps>) => Promise<void>
}

type AlertsCenterProps = ComponentProps<typeof AlertsCenter>

const childRequestListStub = {
  items: [
    {
      id: 502,
      key_id: null,
      auth_token_id: 'tok_ops_01',
      method: 'POST',
      path: '/mcp',
      query: null,
      http_status: null,
      mcp_status: null,
      business_credits: null,
      request_kind_key: 'mcp_tools_list',
      request_kind_label: 'MCP tools/list',
      request_kind_detail: 'tools/list',
      result_status: 'quota_exhausted',
      created_at: 1_776_220_260,
      error_message: 'Alice Wang hit the local rolling request-rate window for MCP tools/list.',
      failure_kind: null,
      key_effect_code: 'none',
      key_effect_summary: null,
      binding_effect_code: 'none',
      binding_effect_summary: null,
      selection_effect_code: 'none',
      selection_effect_summary: null,
      gateway_mode: null,
      experiment_variant: null,
      proxy_session_id: null,
      routing_subject_hash: null,
      upstream_operation: null,
      fallback_reason: null,
      request_body: null,
      response_body: null,
      request_body_bytes: null,
      response_body_bytes: null,
      request_body_sha256: null,
      response_body_sha256: null,
      body_cleaned_reason: null,
      body_cleaned_at: null,
      forwarded_headers: [],
      dropped_headers: [],
      remote_addr: null,
      client_ip: null,
      client_ip_source: null,
      client_ip_trusted: false,
      ip_headers: [],
      operationalClass: 'quota_exhausted' as const,
      requestKindProtocolGroup: 'mcp' as const,
      requestKindBillingGroup: 'non_billable' as const,
    },
    {
      id: 503,
      key_id: null,
      auth_token_id: 'tok_ops_01',
      method: 'POST',
      path: '/mcp',
      query: null,
      http_status: null,
      mcp_status: null,
      business_credits: null,
      request_kind_key: 'mcp_initialize',
      request_kind_label: 'MCP initialize',
      request_kind_detail: 'initialize',
      result_status: 'quota_exhausted',
      created_at: 1_776_220_200,
      error_message: 'Alice Wang hit the local rolling request-rate window for MCP initialize.',
      failure_kind: null,
      key_effect_code: 'none',
      key_effect_summary: null,
      binding_effect_code: 'none',
      binding_effect_summary: null,
      selection_effect_code: 'none',
      selection_effect_summary: null,
      gateway_mode: null,
      experiment_variant: null,
      proxy_session_id: null,
      routing_subject_hash: null,
      upstream_operation: null,
      fallback_reason: null,
      request_body: null,
      response_body: null,
      request_body_bytes: null,
      response_body_bytes: null,
      request_body_sha256: null,
      response_body_sha256: null,
      body_cleaned_reason: null,
      body_cleaned_at: null,
      forwarded_headers: [],
      dropped_headers: [],
      remote_addr: null,
      client_ip: null,
      client_ip_source: null,
      client_ip_trusted: false,
      ip_headers: [],
      operationalClass: 'quota_exhausted' as const,
      requestKindProtocolGroup: 'mcp' as const,
      requestKindBillingGroup: 'non_billable' as const,
    },
  ],
  pageSize: 50,
  nextCursor: null,
  prevCursor: null,
  hasOlder: false,
  hasNewer: false,
}

function installMatchMediaMock(matches: boolean): void {
  Object.defineProperty(window, 'matchMedia', {
    configurable: true,
    writable: true,
    value: (query: string) => ({
      matches: query.includes('max-width') ? matches : false,
      media: query,
      onchange: null,
      addListener: () => undefined,
      removeListener: () => undefined,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
      dispatchEvent: () => true,
    }),
  })
}

function formatExpectedRangeLine(timestamp: number): string {
  const parsed = new Date(timestamp * 1000)
  const month = String(parsed.getMonth() + 1).padStart(2, '0')
  const day = String(parsed.getDate()).padStart(2, '0')
  const hours = String(parsed.getHours()).padStart(2, '0')
  const minutes = String(parsed.getMinutes()).padStart(2, '0')
  const seconds = String(parsed.getSeconds()).padStart(2, '0')
  return `${month}月${day}日 ${hours}:${minutes}:${seconds}`
}

function formatExpectedEnglishRangeLine(timestamp: number): string {
  const parsed = new Date(timestamp * 1000)
  const month = String(parsed.getMonth() + 1).padStart(2, '0')
  const day = String(parsed.getDate()).padStart(2, '0')
  const hours = String(parsed.getHours()).padStart(2, '0')
  const minutes = String(parsed.getMinutes()).padStart(2, '0')
  const seconds = String(parsed.getSeconds()).padStart(2, '0')
  return `${month}/${day} ${hours}:${minutes}:${seconds}`
}

async function mountAlertsCenter(partialProps: Partial<AlertsCenterProps> = {}): Promise<MountedAlertsCenter> {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)

  let props: AlertsCenterProps = {
    language: 'zh',
    search: alertsPath({ view: 'events', requestKinds: ['tavily_search'] }).replace('/admin/alerts', ''),
    refreshToken: 0,
    onNavigate: () => {},
    onOpenUser: () => {},
    onOpenToken: () => {},
    onOpenKey: () => {},
    formatTime: () => '04/19 09:00',
    formatTimeDetail: () => '04/19 09:00',
    catalogLoader: async () => storyCatalog,
    eventsLoader: async () => storyEvents,
    groupsLoader: async () => ({ page: 1, perPage: 20, total: 0, items: [] }),
    requestLoader: async () => ({ request_body: null, response_body: null }),
    childRequestLoader: async () => childRequestListStub,
    ...partialProps,
  }

  await act(async () => {
    root.render(<AlertsCenter {...props} />)
  })
  await flushEffects()

  return {
    container,
    root,
    rerender: async (patch = {}) => {
      props = { ...props, ...patch }
      await act(async () => {
        root.render(<AlertsCenter {...props} />)
      })
      await flushEffects()
    },
  }
}

afterEach(() => {
  document.body.innerHTML = ''
  installMatchMediaMock(false)
})

describe('AlertsCenter loading behavior', () => {
  it('does not keep refetching when the search string is stable', async () => {
    let catalogCalls = 0
    let eventsCalls = 0
    const { root, rerender } = await mountAlertsCenter({
      catalogLoader: async () => {
        catalogCalls += 1
        return storyCatalog
      },
      eventsLoader: async () => {
        eventsCalls += 1
        return storyEvents
      },
    })

    await flushEffects()
    expect(catalogCalls).toBe(1)
    expect(eventsCalls).toBe(1)

    await rerender()
    expect(catalogCalls).toBe(1)
    expect(eventsCalls).toBe(1)

    await act(async () => {
      root.unmount()
    })
  })

  it('keeps current rows visible during a same-query background refresh', async () => {
    let eventsCalls = 0
    const secondResponse = deferred<AlertsPage<AlertEvent>>()
    const { container, root, rerender } = await mountAlertsCenter({
      initialCatalog: storyCatalog,
      initialEventsPage: storyEvents,
      eventsLoader: async () => {
        eventsCalls += 1
        if (eventsCalls === 1) {
          return storyEvents
        }
        return secondResponse.promise
      },
    })

    expect(eventsCalls).toBe(1)

    await rerender({ refreshToken: 1 })
    expect(eventsCalls).toBe(2)
    expect(container.textContent).toContain('上游用量限制 432')
    expect(container.querySelector('.alerts-center-table-shell .admin-loading-region-placeholder')).toBeNull()

    await flushEffects()
    expect(eventsCalls).toBe(2)

    secondResponse.resolve(storyEvents)
    await flushEffects()

    await act(async () => {
      root.unmount()
    })
  })

  it('uses a blocking load exactly once when the alert query changes', async () => {
    let eventsCalls = 0
    const switchedResponse = deferred<AlertsPage<AlertEvent>>()
    const { container, root, rerender } = await mountAlertsCenter({
      initialCatalog: storyCatalog,
      initialEventsPage: storyEvents,
      eventsLoader: async () => {
        eventsCalls += 1
        if (eventsCalls === 1) {
          return storyEvents
        }
        return switchedResponse.promise
      },
    })

    expect(eventsCalls).toBe(1)

    await rerender({
      search: alertsPath({ view: 'events', requestKinds: ['mcp_search'] }).replace('/admin/alerts', ''),
    })
    expect(eventsCalls).toBe(2)
    expect(container.querySelector('.alerts-center-table-shell .admin-loading-region-placeholder')).not.toBeNull()

    await flushEffects()
    expect(eventsCalls).toBe(2)

    switchedResponse.resolve(storyEvents)
    await flushEffects()

    await act(async () => {
      root.unmount()
    })
  })

  it('loads grouped alerts by default and clears filters back to grouped history', async () => {
    let eventsCalls = 0
    let groupsCalls = 0
    const navigations: string[] = []
    const { container, root } = await mountAlertsCenter({
      search: alertsPath().replace('/admin/alerts', ''),
      initialCatalog: storyCatalog,
      initialGroupsPage: semanticGroups,
      onNavigate: (next) => navigations.push(next),
      eventsLoader: async () => {
        eventsCalls += 1
        return storyEvents
      },
      groupsLoader: async () => {
        groupsCalls += 1
        return semanticGroups
      },
    })

    expect(groupsCalls).toBe(1)
    expect(eventsCalls).toBe(0)
    expect(container.textContent).toContain('聚合告警')

    const clearButton = Array.from(container.querySelectorAll('button'))
      .find((button) => button.textContent?.includes('清空筛选'))
    expect(clearButton).toBeDefined()
    await act(async () => {
      clearButton?.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })
    expect(navigations.at(-1)).toBe(alertsPath({ view: 'groups' }))

    await act(async () => {
      root.unmount()
    })
  })

  it('keeps alert view tabs as segmented buttons even when the viewport is compact', async () => {
    installMatchMediaMock(true)

    const { container, root } = await mountAlertsCenter({
      search: alertsPath({ view: 'groups' }).replace('/admin/alerts', ''),
      initialCatalog: storyCatalog,
      initialGroupsPage: semanticGroups,
      groupsLoader: async () => semanticGroups,
    })

    expect(container.querySelector('.segmented-tab')).not.toBeNull()
    expect(container.querySelector('.segmented-tabs-select-trigger')).toBeNull()
    expect(container.textContent).toContain('事件记录')
    expect(container.textContent).toContain('聚合告警')

    await act(async () => {
      root.unmount()
    })
  })

  it('renders grouped mother ranges as two full datetime lines', async () => {
    const { container, root } = await mountAlertsCenter({
      search: alertsPath({ view: 'groups' }).replace('/admin/alerts', ''),
      initialCatalog: storyCatalog,
      initialGroupsPage: semanticGroups,
      groupsLoader: async () => semanticGroups,
    })

    const firstRangeCell = container.querySelector('.alerts-center-time-cell--range')
    expect(firstRangeCell?.textContent).toContain(formatExpectedRangeLine(semanticGroups.items[0]!.firstSeen))
    expect(firstRangeCell?.textContent).toContain(formatExpectedRangeLine(semanticGroups.items[0]!.lastSeen))
    expect(firstRangeCell?.textContent).not.toContain('开始')
    expect(firstRangeCell?.textContent).not.toContain('结束')

    await act(async () => {
      root.unmount()
    })
  })

  it('does not show compatibility placeholder copy in grouped compatibility rows', async () => {
    const { container, root } = await mountAlertsCenter({
      search: alertsPath({ view: 'groups' }).replace('/admin/alerts', ''),
      initialCatalog: storyCatalog,
      initialGroupsPage: compatibilityGroups,
      groupsLoader: async () => compatibilityGroups,
    })

    const windowCell = container.querySelector('.alerts-center-col--request-kind .alerts-center-summary-cell')
    expect(windowCell?.textContent).toContain('—')
    expect(windowCell?.querySelector('.request-kind-badge')).toBeNull()
    expect(windowCell?.textContent).toContain('x2')
    expect(windowCell?.textContent).not.toContain('兼容分组')
    expect(container.textContent).not.toContain('MCP | search')
    expect(container.textContent).not.toContain('兼容分组')

    await act(async () => {
      root.unmount()
    })
  })

  it('shows only the actual grouped subject and hides related token values in the subject column', async () => {
    const { container, root } = await mountAlertsCenter({
      search: alertsPath({ view: 'groups' }).replace('/admin/alerts', ''),
      initialCatalog: storyCatalog,
      initialGroupsPage: compatibilityGroups,
      groupsLoader: async () => compatibilityGroups,
    })

    const subjectCell = container.querySelector('.alerts-center-col--subject .alerts-center-subject-cell')
    expect(subjectCell?.textContent).toContain('key_001')
    expect(subjectCell?.textContent).not.toContain('tok_ops_01')

    await act(async () => {
      root.unmount()
    })
  })

  it('shows only the event subject label without the raw subject kind text', async () => {
    const { container, root } = await mountAlertsCenter({
      initialCatalog: storyCatalog,
      initialEventsPage: storyEvents,
      eventsLoader: async () => storyEvents,
    })

    const subjectCell = container.querySelector('.alerts-center-col--subject .alerts-center-subject-cell')
    expect(subjectCell?.textContent).toContain('Alice Wang')
    expect(subjectCell?.textContent).not.toContain('user')

    await act(async () => {
      root.unmount()
    })
  })

  it('opens child raw events in a drawer instead of rendering a third table layer', async () => {
    const { container, root } = await mountAlertsCenter({
      search: alertsPath({ view: 'groups' }).replace('/admin/alerts', ''),
      initialCatalog: storyCatalog,
      initialGroupsPage: semanticGroups,
      groupsLoader: async () => semanticGroups,
    })

    const groupToggle = container.querySelector('.alerts-center-row-expander')
    expect(groupToggle).not.toBeNull()
    await act(async () => {
      groupToggle?.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })
    await flushEffects()
    expect(container.textContent).toContain('展开 2 条调用记录')
    expect(container.textContent).not.toContain('收起 2 条原始告警')
    expect(container.textContent).toContain(formatExpectedRangeLine(semanticGroups.items[0]!.children![0]!.semanticWindowStart!))
    expect(container.textContent).toContain(formatExpectedRangeLine(semanticGroups.items[0]!.children![0]!.semanticWindowEnd!))

    const toggles = container.querySelectorAll('.alerts-center-inline-toggle')
    expect(toggles.length).toBeGreaterThan(1)
    await act(async () => {
      toggles[1]?.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })
    await flushEffects()
    expect(container.textContent).toContain('MCP tools/list')
    expect(container.textContent).toContain('Alice Wang hit the local rolling request-rate window for MCP tools/list.')

    await act(async () => {
      root.unmount()
    })
  })

  it('localizes the child request drawer in English', async () => {
    const loaderStarted = deferred<void>()
    let loaderCallCount = 0
    const { container, root } = await mountAlertsCenter({
      language: 'en',
      search: alertsPath({ view: 'groups' }).replace('/admin/alerts', ''),
      initialCatalog: storyCatalog,
      initialGroupsPage: semanticGroups,
      groupsLoader: async () => semanticGroups,
      childRequestLoader: async () => {
        loaderCallCount += 1
        loaderStarted.resolve()
        return childRequestListStub
      },
      formatTime: () => '09:00:00',
      formatTimeDetail: () => '04/19',
    })

    const groupToggle = container.querySelector('.alerts-center-row-expander')
    expect(groupToggle).not.toBeNull()
    await act(async () => {
      groupToggle?.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })
    await flushEffects()

    const toggles = container.querySelectorAll('.alerts-center-inline-toggle')
    await act(async () => {
      toggles[1]?.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })
    await loaderStarted.promise
    await flushEffects()

    const pageText = container.textContent ?? ''
    expect(pageText).toContain('Expand 2 call records')
    expect(pageText).toContain(formatExpectedEnglishRangeLine(semanticGroups.items[0]!.children![0]!.semanticWindowStart!))
    expect(pageText).toContain(formatExpectedEnglishRangeLine(semanticGroups.items[0]!.children![0]!.semanticWindowEnd!))
    expect(pageText).not.toContain('调用记录')
    expect(pageText).not.toContain('全部调用类型')
    expect(loaderCallCount).toBe(1)

    await act(async () => {
      root.unmount()
    })
  })
})
