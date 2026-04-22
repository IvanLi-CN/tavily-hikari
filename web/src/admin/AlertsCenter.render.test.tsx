import '../../test/happydom'

import { afterEach, describe, expect, it } from 'bun:test'
import { act, type ComponentProps } from 'react'
import { createRoot, type Root } from 'react-dom/client'

import type { AlertCatalog, AlertEvent, AlertsPage } from '../api'
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
})
