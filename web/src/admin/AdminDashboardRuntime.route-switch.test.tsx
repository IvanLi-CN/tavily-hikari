import '../../test/happydom'

import { afterEach, beforeEach, describe, expect, it } from 'bun:test'
import { StrictMode } from 'react'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'

import { installDemoRuntime } from '../api/demo'
import { fetchDashboardOverview, type DashboardSnapshotEvent } from '../api/runtime'
import { TooltipProvider } from '../components/ui/tooltip'
import { LanguageProvider } from '../i18n'
import { ThemeProvider } from '../theme'
import AdminDashboard from './AdminDashboardRuntime'
import {
  analysisPath,
  buildAdminKeysPath,
  buildAdminTokensPath,
  buildAdminUsersOverviewPath,
  keyDetailPath,
  rankingsPath,
  tokenDetailPath,
  unboundTokenUsagePath,
  userDetailPath,
  userTagEditPath,
  userTagsPath,
  userUsagePath,
} from './routes'

interface MockMediaQueryList {
  matches: boolean
  media: string
  onchange: ((event: MediaQueryListEvent) => void) | null
  addListener: (listener: (event: MediaQueryListEvent) => void) => void
  removeListener: (listener: (event: MediaQueryListEvent) => void) => void
  addEventListener: (type: string, listener: (event: MediaQueryListEvent) => void) => void
  removeEventListener: (type: string, listener: (event: MediaQueryListEvent) => void) => void
  dispatchEvent: (event: Event) => boolean
}

interface RouteExpectation {
  pathname?: string
  search?: string
  selector?: string
  selectorText?: string
  text?: string
}

interface RouteSwitchCase {
  name: string
  startPath: string
  nextPath: string
  nextExpectation: RouteExpectation
  returnPath?: string
  returnExpectation?: RouteExpectation
}

const DEMO_STORAGE_KEY = 'tavily-hikari-demo-mode'
const originalConsoleError = console.error
const originalFetch = window.fetch
const originalEventSource = window.EventSource
const originalMatchMedia = window.matchMedia
const originalRequestAnimationFrame = window.requestAnimationFrame
const originalCancelAnimationFrame = window.cancelAnimationFrame
const originalScrollIntoView = HTMLElement.prototype.scrollIntoView

let consoleErrors: string[] = []

class ControlledEventSource {
  static instances: ControlledEventSource[] = []

  readonly url: string
  readonly withCredentials = false
  onopen: ((this: EventSource, event: Event) => unknown) | null = null
  onerror: ((this: EventSource, event: Event) => unknown) | null = null
  private readonly listeners = new Map<string, Set<EventListenerOrEventListenerObject>>()

  constructor(url: string | URL) {
    this.url = String(url)
    ControlledEventSource.instances.push(this)
    queueMicrotask(() => this.onopen?.call(this as unknown as EventSource, new Event('open')))
  }

  addEventListener(type: string, listener: EventListenerOrEventListenerObject | null): void {
    if (!listener) return
    const listeners = this.listeners.get(type) ?? new Set<EventListenerOrEventListenerObject>()
    listeners.add(listener)
    this.listeners.set(type, listeners)
  }

  removeEventListener(type: string, listener: EventListenerOrEventListenerObject | null): void {
    if (listener) this.listeners.get(type)?.delete(listener)
  }

  close(): void {
    this.listeners.clear()
  }

  emitSnapshot(payload: DashboardSnapshotEvent): void {
    const event = new MessageEvent('snapshot', { data: JSON.stringify(payload) })
    for (const listener of this.listeners.get('snapshot') ?? []) {
      if (typeof listener === 'function') {
        listener.call(this as unknown as EventSource, event)
      } else {
        listener.handleEvent(event)
      }
    }
  }
}

const routeSwitchCases: RouteSwitchCase[] = [
  {
    name: 'users overview -> user usage -> users overview',
    startPath: buildAdminUsersOverviewPath(),
    nextPath: userUsagePath(),
    nextExpectation: {
      selector: 'input[name="user-usage-search"]',
      text: 'User Usage',
    },
    returnPath: buildAdminUsersOverviewPath(),
    returnExpectation: {
      selector: 'input[name="users-search"]',
      text: 'User Management',
    },
  },
  {
    name: 'tokens overview -> unbound token usage -> tokens overview',
    startPath: buildAdminTokensPath(),
    nextPath: unboundTokenUsagePath(),
    nextExpectation: {
      selector: 'input[name="unbound-token-usage-search"]',
      text: 'Unbound Token Usage',
    },
    returnPath: buildAdminTokensPath(),
    returnExpectation: {
      selector: 'input[name="token-search"]',
      text: 'Access Tokens',
    },
  },
  {
    name: 'keys overview -> key detail',
    startPath: buildAdminKeysPath(),
    nextPath: keyDetailPath('Hk01'),
    nextExpectation: {
      text: 'Key Details',
    },
  },
  {
    name: 'tokens overview -> token detail',
    startPath: buildAdminTokensPath(),
    nextPath: tokenDetailPath('dm01'),
    nextExpectation: {
      text: 'Regenerate Secret',
    },
  },
  {
    name: 'users overview -> user detail',
    startPath: buildAdminUsersOverviewPath(),
    nextPath: userDetailPath('user-demo-admin'),
    nextExpectation: {
      selector: '#user-detail-identity',
      text: 'User Detail',
    },
  },
  {
    name: 'rankings -> user detail -> rankings preserves tab',
    startPath: rankingsPath('uniqueIp'),
    nextPath: userDetailPath('user-demo-admin'),
    nextExpectation: {
      selector: '#user-detail-identity',
      text: 'User Detail',
    },
    returnPath: rankingsPath('uniqueIp'),
    returnExpectation: {
      selector: '.admin-rankings-tab.is-active',
      text: 'IP',
    },
  },
  {
    name: 'rankings -> rankings syncs the selected tab from location and preserves demo params',
    startPath: `${analysisPath('rankings')}?demo=true&tab=last7d`,
    nextPath: `${analysisPath('rankings')}?demo=true&tab=businessCredits`,
    nextExpectation: {
      pathname: analysisPath('rankings'),
      search: '?demo=true&tab=businessCredits',
      selector: '.admin-rankings-tab.is-active',
      selectorText: 'Credits',
    },
    returnPath: `${analysisPath('rankings')}?demo=true&tab=uniqueIp`,
    returnExpectation: {
      pathname: analysisPath('rankings'),
      search: '?demo=true&tab=uniqueIp',
      selector: '.admin-rankings-tab.is-active',
      selectorText: 'IP',
    },
  },
  {
    name: 'rankings analysis alias preserves alias path and demo params',
    startPath: '/admin/analysis?demo=true&tab=last7d',
    nextPath: '/admin/analysis?demo=true&tab=businessCredits',
    nextExpectation: {
      pathname: '/admin/analysis',
      search: '?demo=true&tab=businessCredits',
      selector: '.admin-rankings-tab.is-active',
      selectorText: 'Credits',
    },
    returnPath: '/admin/analysis?demo=true&tab=uniqueIp',
    returnExpectation: {
      pathname: '/admin/analysis',
      search: '?demo=true&tab=uniqueIp',
      selector: '.admin-rankings-tab.is-active',
      selectorText: 'IP',
    },
  },
  {
    name: 'users overview -> user tags',
    startPath: buildAdminUsersOverviewPath(),
    nextPath: userTagsPath(),
    nextExpectation: {
      selector: '.user-tag-catalog-grid',
      text: 'Tag Catalog',
    },
  },
  {
    name: 'users overview -> user tag editor',
    startPath: buildAdminUsersOverviewPath(),
    nextPath: userTagEditPath('tag-demo'),
    nextExpectation: {
      selector: '.user-tag-catalog-card-active button[aria-label="Save tag"]',
    },
  },
]

function installMatchMediaMock(): void {
  Object.defineProperty(window, 'matchMedia', {
    configurable: true,
    writable: true,
    value: (query: string): MockMediaQueryList => ({
      matches: false,
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

function installAnimationFrameMock(): void {
  Object.defineProperty(window, 'requestAnimationFrame', {
    configurable: true,
    writable: true,
    value: (callback: FrameRequestCallback) => window.setTimeout(() => callback(Date.now()), 0),
  })
  Object.defineProperty(window, 'cancelAnimationFrame', {
    configurable: true,
    writable: true,
    value: (handle: number) => window.clearTimeout(handle),
  })
}

async function flushUi(ms = 0): Promise<void> {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
    await new Promise<void>((resolve) => window.setTimeout(resolve, 0))
    if (ms > 0) {
      await new Promise<void>((resolve) => window.setTimeout(resolve, ms))
    }
  })
}

function restoreWindowEventSource(eventSource: typeof EventSource | undefined): void {
  if (typeof eventSource === 'undefined') {
    delete (window as Window & { EventSource?: typeof EventSource }).EventSource
    return
  }
  Object.defineProperty(window, 'EventSource', {
    configurable: true,
    writable: true,
    value: eventSource,
  })
}

function withDemoMode(path: string): string {
  return path
}

function resetDemoRuntime(): void {
  window.fetch = originalFetch
  restoreWindowEventSource(originalEventSource)
  delete document.documentElement.dataset.demoMode
  delete window.__tavilyHikariDemoFetch
  delete window.__tavilyHikariDemoEventSource
  delete window.__tavilyHikariDemoInstalled
}

function formatConsoleArgs(args: unknown[]): string {
  return args
    .map((value) => {
      if (value instanceof Error) return value.stack ?? value.message
      if (typeof value === 'string') return value
      try {
        return JSON.stringify(value)
      } catch {
        return String(value)
      }
    })
    .join(' ')
}

function hookOrderErrors(): string[] {
  return consoleErrors.filter((entry) =>
    /rendered fewer hooks than expected|rendered more hooks than during the previous render|change in the order of hooks|react error #300/i.test(
      entry,
    ),
  )
}

function assertRouteRendered(container: HTMLElement, expectation: RouteExpectation): void {
  const main = container.querySelector<HTMLElement>('[role="main"]')
  expect(main).not.toBeNull()
  expect(main?.textContent?.replace(/\s+/g, ' ').trim().length ?? 0).toBeGreaterThan(0)
  if (expectation.pathname) {
    expect(window.location.pathname).toBe(expectation.pathname)
  }
  if (expectation.search) {
    expect(window.location.search).toBe(expectation.search)
  }
  if (expectation.selector) {
    const selected = main?.querySelector<HTMLElement>(expectation.selector) ?? null
    expect(selected).not.toBeNull()
    if (expectation.selectorText) {
      expect(selected?.textContent).toContain(expectation.selectorText)
    }
  }
  if (expectation.text) {
    expect(main?.textContent).toContain(expectation.text)
  }
}

async function waitForRouteRendered(container: HTMLElement, expectation: RouteExpectation, timeoutMs = 1600): Promise<void> {
  const deadline = Date.now() + timeoutMs
  let lastError: unknown = null
  while (Date.now() <= deadline) {
    try {
      assertRouteRendered(container, expectation)
      return
    } catch (error) {
      lastError = error
      await flushUi(40)
    }
  }
  throw lastError ?? new Error(`Timed out waiting for route ${window.location.pathname}${window.location.search}`)
}

async function navigateTo(path: string): Promise<void> {
  await act(async () => {
    window.history.pushState(null, '', withDemoMode(path))
    window.dispatchEvent(new PopStateEvent('popstate'))
  })
  await flushUi(180)
}

async function mountAdminDashboard(
  initialPath: string,
  eventSource?: typeof EventSource,
): Promise<{ container: HTMLDivElement; root: Root }> {
  window.localStorage.setItem(DEMO_STORAGE_KEY, 'true')
  window.history.replaceState(null, '', withDemoMode(initialPath))
  installDemoRuntime()
  if (eventSource) {
    Object.defineProperty(window, 'EventSource', {
      configurable: true,
      writable: true,
      value: eventSource,
    })
  }

  const container = document.createElement('div')
  container.style.width = '1440px'
  container.style.minHeight = '1200px'
  document.body.appendChild(container)
  const root = createRoot(container)

  await act(async () => {
    root.render(
      <StrictMode>
        <LanguageProvider initialLanguage="en">
          <ThemeProvider>
            <TooltipProvider delayDuration={0} skipDelayDuration={0}>
              <AdminDashboard />
            </TooltipProvider>
          </ThemeProvider>
        </LanguageProvider>
      </StrictMode>,
    )
  })
  await flushUi(180)

  return { container, root }
}

beforeEach(() => {
  consoleErrors = []
  console.error = (...args: unknown[]) => {
    consoleErrors.push(formatConsoleArgs(args))
  }
  installMatchMediaMock()
  installAnimationFrameMock()
  HTMLElement.prototype.scrollIntoView = () => undefined
  ControlledEventSource.instances = []
})

afterEach(async () => {
  console.error = originalConsoleError
  resetDemoRuntime()
  window.matchMedia = originalMatchMedia
  window.requestAnimationFrame = originalRequestAnimationFrame
  window.cancelAnimationFrame = originalCancelAnimationFrame
  HTMLElement.prototype.scrollIntoView = originalScrollIntoView
  window.localStorage.removeItem(DEMO_STORAGE_KEY)
  document.body.innerHTML = ''
})

describe('AdminDashboard route switches', () => {
  for (const routeCase of routeSwitchCases) {
    it(`keeps admin runtime stable when switching ${routeCase.name}`, async () => {
      const { container, root } = await mountAdminDashboard(routeCase.startPath)

      try {
        await waitForRouteRendered(container, {})

        await navigateTo(routeCase.nextPath)
        await waitForRouteRendered(container, routeCase.nextExpectation)

        if (routeCase.returnPath && routeCase.returnExpectation) {
          await navigateTo(routeCase.returnPath)
          await waitForRouteRendered(container, routeCase.returnExpectation)
        }

        expect(hookOrderErrors()).toEqual([])
      } finally {
        await act(async () => {
          root.unmount()
        })
      }
    })
  }

  it('updates dashboard recent alerts from the shared SSE snapshot', async () => {
    const { container, root } = await mountAdminDashboard(
      '/admin/dashboard',
      ControlledEventSource as unknown as typeof EventSource,
    )

    try {
      await waitForRouteRendered(container, { text: 'Dashboard' })
      await flushUi(300)
      const eventSource = ControlledEventSource.instances
        .filter((source) => source.url === '/api/events')
        .at(-1)
      expect(eventSource).toBeDefined()
      const overview = await fetchDashboardOverview()
      const firstGroup = overview.recentAlerts.topGroups[0]
      expect(firstGroup).toBeDefined()
      const marker = 'SSE alert projection marker'
      await act(async () => {
        eventSource?.emitSnapshot({
          ...overview,
          keys: overview.exhaustedKeys,
          logs: overview.recentLogs,
          recentAlerts: {
            ...overview.recentAlerts,
            totalEvents: Math.max(1, overview.recentAlerts.totalEvents),
            groupedCount: 1,
            groupedCountWindows: overview.recentAlerts.groupedCountWindows.map((item) => ({
              ...item,
              groupedCount: 1,
            })),
            topGroups: [
              {
                ...firstGroup!,
                subjectLabel: marker,
                latestEvent: { ...firstGroup!.latestEvent, summary: marker },
              },
            ],
          },
        })
        await Promise.resolve()
      })
      await flushUi(80)
      expect(container.textContent).toContain(marker)
    } finally {
      await act(async () => {
        root.unmount()
      })
    }
  }, 20_000)
})

/**
 * Plan todo 25 demo parity: every provider REST route and every provider MCP
 * endpoint must answer locally from browser fixtures (ground truth:
 * src/bin/mock_serpapi.rs, src/bin/mock_exa.rs, src/bin/mock_brave.rs; catalogs:
 * src/server/provider_mcp_{serpapi,exa,brave}.rs). The whitelist is extended
 * for the three explicit provider MCP paths (M22) while unknown `/api/*` paths
 * keep the pre-existing demo fall-through shape.
 */
function installProviderDemoFetch(): typeof fetch {
  window.localStorage.setItem(DEMO_STORAGE_KEY, 'true')
  installDemoRuntime()
  return window.fetch
}

async function demoJson(response: Response): Promise<Record<string, unknown>> {
  return (await response.json()) as Record<string, unknown>
}

function demoPost(path: string, payload: unknown): [string, RequestInit] {
  return [path, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(payload) }]
}

const SERPAPI_SEARCH_FIXTURE = {
  search_metadata: { id: 'mock', status: 'Success' },
  search_parameters: {},
  organic_results: [
    { position: 1, title: 'Mock Result', link: 'https://example.com/1', snippet: 'mock snippet' },
  ],
}

interface DemoMcpTool {
  name: string
  description?: string
  inputSchema?: { type?: string; required?: string[]; properties?: Record<string, unknown> }
}

async function demoToolsList(fetchImpl: typeof fetch, path: string): Promise<DemoMcpTool[]> {
  const response = await fetchImpl(...demoPost(path, { jsonrpc: '2.0', id: 1, method: 'tools/list' }))
  expect(response.status).toBe(200)
  const body = await demoJson(response)
  expect(body.jsonrpc).toBe('2.0')
  const result = body.result as { tools?: DemoMcpTool[] } | undefined
  expect(Array.isArray(result?.tools)).toBe(true)
  return result?.tools ?? []
}

async function demoToolCall(
  fetchImpl: typeof fetch,
  path: string,
  name: string,
  args: Record<string, unknown>,
): Promise<Record<string, unknown>> {
  const response = await fetchImpl(...demoPost(path, { jsonrpc: '2.0', id: 2, method: 'tools/call', params: { name, arguments: args } }))
  expect(response.status).toBe(200)
  const body = await demoJson(response)
  expect(body.jsonrpc).toBe('2.0')
  return (body.result ?? {}) as Record<string, unknown>
}

describe('AdminDashboard demo provider parity', () => {
  it('serves every provider REST route with the committed mock shapes', async () => {
    const demoFetch = installProviderDemoFetch()

    const serpapi = await demoFetch('/api/serpapi/search?q=coffee')
    expect(serpapi.status).toBe(200)
    expect(await demoJson(serpapi)).toEqual(SERPAPI_SEARCH_FIXTURE)

    const serpapiPost = await demoFetch(...demoPost('/api/serpapi/search', { q: 'coffee' }))
    expect(serpapiPost.status).toBe(200)
    expect(await demoJson(serpapiPost)).toEqual(SERPAPI_SEARCH_FIXTURE)

    const exaSearch = await demoFetch(...demoPost('/api/exa/search', { query: 'demo' }))
    expect(exaSearch.status).toBe(200)
    expect(await demoJson(exaSearch)).toEqual({
      requestId: 'mock',
      results: [
        { title: 'Mock', url: 'https://example.com/1', text: 'mock text', publishedDate: '2026-01-01T00:00:00.000Z' },
      ],
      costDollars: { total: 0.007 },
      searchTime: 123.4,
    })

    const exaContents = await demoFetch(...demoPost('/api/exa/contents', { urls: ['https://example.com/1'] }))
    expect(exaContents.status).toBe(200)
    expect(await demoJson(exaContents)).toEqual({
      results: [{ title: 'Mock', url: 'https://example.com/1', text: 'mock text' }],
      statuses: [{ id: 'https://example.com/1', status: 'success', source: 'cached' }],
    })

    const exaAnswer = await demoFetch(...demoPost('/api/exa/answer', { query: 'demo' }))
    expect(exaAnswer.status).toBe(200)
    expect(await demoJson(exaAnswer)).toEqual({
      requestId: 'mock',
      answer: 'mock answer',
      citations: [{ url: 'https://example.com/1' }],
      costDollars: { total: 0.005 },
    })

    const exaStream = await demoFetch(...demoPost('/api/exa/search', { query: 'demo', stream: true }))
    expect(exaStream.status).toBe(400)
    expect(await demoJson(exaStream)).toEqual({
      error: 'invalid_request',
      message: 'streaming not supported; omit stream or set false',
    })

    const braveWeb = await demoFetch('/api/brave/res/v1/web/search?q=x')
    expect(braveWeb.status).toBe(200)
    expect(braveWeb.headers.get('x-ratelimit-limit')).toBe('50, 1000')
    expect(braveWeb.headers.get('x-ratelimit-policy')).toBe('1;w=1, 1000;w=2592000')
    expect(braveWeb.headers.get('x-ratelimit-remaining')).toBe('1, 999')
    expect(braveWeb.headers.get('x-ratelimit-reset')).toBe('1, 2592000')
    expect(await demoJson(braveWeb)).toEqual({
      type: 'search',
      query: { original: 'mock', more_results_available: false },
      web: { results: [{ title: 'Mock', url: 'https://example.com/1', description: 'mock desc' }] },
    })

    const braveNews = await demoFetch('/api/brave/res/v1/news/search?q=x')
    expect(braveNews.status).toBe(200)
    expect(await demoJson(braveNews)).toEqual({
      type: 'news',
      results: [{ title: 'Mock News', url: 'https://example.com/n1', description: 'mock' }],
    })

    const braveContext = await demoFetch('/api/brave/res/v1/llm/context?q=x')
    expect(braveContext.status).toBe(200)
    expect(await demoJson(braveContext)).toEqual({
      grounding: { generic: [{ url: 'https://example.com/1', title: 'Mock', snippets: ['mock snippet'] }] },
      sources: {},
    })
  })

  it('serves the per-provider MCP catalogs and tool-call fixtures', async () => {
    const demoFetch = installProviderDemoFetch()

    const serpapiTools = await demoToolsList(demoFetch, '/serpapi/mcp')
    expect(serpapiTools.map((tool) => tool.name)).toEqual(['search', 'search_table', 'search_dashboard'])
    for (const tool of serpapiTools) {
      expect(tool.inputSchema?.type).toBe('object')
      expect(tool.inputSchema?.required).toEqual(['q'])
    }
    const serpapiCall = await demoToolCall(demoFetch, '/serpapi/mcp', 'search', { q: 'hello' })
    const serpapiStructured = serpapiCall.structuredContent as { result?: string } | undefined
    expect(JSON.parse(serpapiStructured?.result ?? 'null')).toEqual(SERPAPI_SEARCH_FIXTURE)

    const exaTools = await demoToolsList(demoFetch, '/exa/mcp')
    expect(exaTools.map((tool) => tool.name)).toEqual(['web_search_exa', 'web_fetch_exa', 'exa_answer'])
    const exaFetchSchema = exaTools.find((tool) => tool.name === 'web_fetch_exa')?.inputSchema
    expect(exaFetchSchema?.properties?.urls).toBeDefined()
    expect(exaFetchSchema?.properties?.ids).toBeDefined()
    const exaCall = await demoToolCall(demoFetch, '/exa/mcp', 'web_search_exa', { query: 'demo' })
    expect(exaCall.structuredContent).toEqual({
      requestId: 'mock',
      results: [
        { title: 'Mock', url: 'https://example.com/1', text: 'mock text', publishedDate: '2026-01-01T00:00:00.000Z' },
      ],
      costDollars: { total: 0.007 },
      searchTime: 123.4,
      status: 200,
    })
    const exaContent = (exaCall.content as Array<{ type: string; text: string }> | undefined)?.[0]
    expect(exaContent?.type).toBe('text')
    expect(JSON.parse(exaContent?.text ?? 'null')).toEqual({
      requestId: 'mock',
      results: [
        { title: 'Mock', url: 'https://example.com/1', text: 'mock text', publishedDate: '2026-01-01T00:00:00.000Z' },
      ],
      costDollars: { total: 0.007 },
      searchTime: 123.4,
    })
    const exaAnswerCall = await demoToolCall(demoFetch, '/exa/mcp', 'exa_answer', { query: 'demo' })
    expect((exaAnswerCall.structuredContent as { answer?: string } | undefined)?.answer).toBe('mock answer')

    const braveTools = await demoToolsList(demoFetch, '/brave/mcp')
    expect(braveTools.map((tool) => tool.name)).toEqual(['brave_web_search', 'brave_news_search', 'brave_llm_context'])
    for (const tool of braveTools) {
      expect(tool.inputSchema?.type).toBe('object')
      expect(tool.inputSchema?.required).toEqual(['query'])
    }
    const braveCall = await demoToolCall(demoFetch, '/brave/mcp', 'brave_web_search', { query: 'demo' })
    const braveStructured = braveCall.structuredContent as { status?: number; web?: { results: Array<{ title: string }> } } | undefined
    expect(braveStructured?.status).toBe(200)
    expect(braveStructured?.web?.results[0]?.title).toBe('Mock')
    const braveNewsCall = await demoToolCall(demoFetch, '/brave/mcp', 'brave_news_search', { query: 'demo' })
    expect((braveNewsCall.structuredContent as { type?: string } | undefined)?.type).toBe('news')
  })

  it('keeps unknown /api paths on the existing demo fall-through shape', async () => {
    const demoFetch = installProviderDemoFetch()
    const unknown = await demoFetch('/api/openai/v1/search?q=x')
    expect(unknown.status).toBe(200)
    expect(await demoJson(unknown)).toEqual({ demo: true, path: '/api/openai/v1/search' })
  })
})
