import '../../test/happydom'

import { afterEach, beforeEach, describe, expect, it } from 'bun:test'
import { StrictMode, act } from 'react'
import { createRoot, type Root } from 'react-dom/client'

import { installDemoRuntime } from '../api/demo'
import { LanguageProvider } from '../i18n'
import { ThemeProvider } from '../theme'
import AdminDashboard from './AdminDashboardRuntime'

/**
 * Plan todo 23b caller wiring proof for the shipped-but-unwired
 * AdminRecentRequestsPanel provider contract (commit 404941f6):
 * - providerOptions: an empty request page plus a facet-only provider proves the
 *   panel receives `/api/logs/catalog` facets (row-derived options stay empty);
 * - selectedProvider: a deep-linked `?provider=` value reaches the panel and the
 *   T21 `RequestLogsPageQuery.provider` on `/api/logs/list`.
 *
 * The Radix dropdown itself is intentionally not opened here: bun's shared
 * module cache evaluates `@radix-ui/react-use-layout-effect` before happy-dom in
 * some file orders, so portal mounting is not reliable across the full suite
 * (documented in .omo/evidence/task-23-serpapi-exa-brave-providers.txt). The
 * panel's `onProviderFilterChange` -> `?provider=` contract is covered by
 * AdminRecentRequestsPanel.providerFilter.test.tsx.
 */

const DEMO_STORAGE_KEY = 'tavily-hikari-demo-mode'
const originalFetch = window.fetch
const originalEventSource = window.EventSource
const originalMatchMedia = window.matchMedia
const originalRequestAnimationFrame = window.requestAnimationFrame
const originalCancelAnimationFrame = window.cancelAnimationFrame

let listUrls: string[] = []
let root: Root | null = null

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

function installViewportMocks(): void {
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

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  })
}

async function waitFor(predicate: () => boolean, timeoutMs = 2500): Promise<void> {
  const deadline = Date.now() + timeoutMs
  while (Date.now() <= deadline) {
    if (predicate()) return
    await flushUi(40)
  }
  throw new Error('Timed out waiting for the admin requests provider wiring')
}

function providerTrigger(): HTMLButtonElement | null {
  return document.querySelector<HTMLButtonElement>('button[aria-label^="Provider:"]')
}

async function mountRequestsRoute(search: string): Promise<HTMLElement> {
  window.localStorage.setItem(DEMO_STORAGE_KEY, 'true')
  window.history.replaceState(null, '', `/admin/requests${search}`)
  installDemoRuntime()

  const demoFetch = window.fetch.bind(window)
  window.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
    const url =
      typeof input === 'string' ? input : input instanceof URL ? input.toString() : input.url
    const response = await demoFetch(input, init)
    if (url.includes('/api/logs/list')) {
      listUrls.push(url)
      // Empty page: provider filter options can then only come from facets.
      const data = (await response.clone().json()) as { items?: Array<Record<string, unknown>> }
      return jsonResponse({ ...data, items: [] })
    }
    if (url.includes('/api/logs/catalog')) {
      const data = (await response.clone().json()) as { facets?: Record<string, unknown> }
      return jsonResponse({
        ...data,
        facets: {
          ...(data.facets ?? {}),
          providers: [
            { value: 'serpapi', count: 9 },
            { value: 'exa', count: 5 },
          ],
        },
      })
    }
    return response
  }

  const container = document.createElement('div')
  container.style.width = '1440px'
  container.style.minHeight = '1200px'
  document.body.appendChild(container)
  root = createRoot(container)

  await act(async () => {
    root!.render(
      <StrictMode>
        <LanguageProvider initialLanguage="en">
          <ThemeProvider>
            <AdminDashboard />
          </ThemeProvider>
        </LanguageProvider>
      </StrictMode>,
    )
  })
  await flushUi(200)
  return container
}

beforeEach(() => {
  listUrls = []
  installViewportMocks()
  HTMLElement.prototype.scrollIntoView = () => undefined
})

afterEach(async () => {
  if (root) {
    await act(async () => {
      root!.unmount()
    })
    root = null
  }
  window.fetch = originalFetch
  Object.defineProperty(window, 'EventSource', {
    configurable: true,
    writable: true,
    value: originalEventSource,
  })
  window.matchMedia = originalMatchMedia
  window.requestAnimationFrame = originalRequestAnimationFrame
  window.cancelAnimationFrame = originalCancelAnimationFrame
  window.localStorage.removeItem(DEMO_STORAGE_KEY)
  delete document.documentElement.dataset.demoMode
  delete window.__tavilyHikariDemoFetch
  delete window.__tavilyHikariDemoEventSource
  delete window.__tavilyHikariDemoInstalled
  document.body.innerHTML = ''
})

describe('AdminDashboardRuntime provider caller wiring (plan todo 23b)', () => {
  it('passes facet providers into the panel when the request page has no provider rows', async () => {
    await mountRequestsRoute('')

    // With an empty request page, row-derived options are empty; the provider
    // facet field can only render because providerOptions came from the
    // `/api/logs/catalog` facet payload.
    await waitFor(() => providerTrigger() !== null)
    expect(providerTrigger()?.getAttribute('aria-label')).toContain('Provider: All providers')
    expect(listUrls.some((url) => url.includes('provider='))).toBe(false)
  }, 20_000)

  it('forwards the provider selection into the T21 ?provider= list query', async () => {
    await mountRequestsRoute('?provider=exa')

    await waitFor(() => providerTrigger() !== null)
    await waitFor(() => listUrls.some((url) => url.includes('provider=exa')))

    // `selectedProvider` reaches the panel control ...
    expect(providerTrigger()?.getAttribute('aria-label')).toContain('Provider: Exa')
    // ... and the T21 query value is emitted on the request-log list request.
    const filteredUrl = listUrls.find((url) => url.includes('provider=exa'))
    expect(filteredUrl).toBeDefined()
    expect(new URLSearchParams(filteredUrl!.split('?')[1] ?? '').get('provider')).toBe('exa')
  }, 20_000)
})