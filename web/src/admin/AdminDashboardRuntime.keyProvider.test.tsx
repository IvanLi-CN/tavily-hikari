import '../../test/happydom'

import { afterEach, beforeEach, describe, expect, it } from 'bun:test'
import { StrictMode, act } from 'react'
import { createRoot, type Root } from 'react-dom/client'

import { installDemoRuntime } from '../api/demo'
import { LanguageProvider } from '../i18n'
import { ThemeProvider } from '../theme'
import AdminDashboard from './AdminDashboardRuntime'

/**
 * Plan todo 22 caller wiring proof: the admin key table renders a provider
 * column with text badges (legacy NULL provider -> Tavily), and the provider
 * filter narrows the rendered rows through the engine-side `?provider=` query.
 */

const DEMO_STORAGE_KEY = 'tavily-hikari-demo-mode'
const originalFetch = window.fetch
const originalEventSource = window.EventSource
const originalMatchMedia = window.matchMedia
const originalRequestAnimationFrame = window.requestAnimationFrame
const originalCancelAnimationFrame = window.cancelAnimationFrame

const KEY_ROWS = [
  {
    id: 'legacy1',
    provider: undefined,
    status: 'active',
    group: 'default',
    registration_ip: null,
    registration_region: null,
    status_changed_at: null,
    last_used_at: null,
    deleted_at: null,
    quota_limit: 100,
    quota_remaining: 50,
    quota_synced_at: null,
    total_requests: 1,
    success_count: 1,
    error_count: 0,
    quota_exhausted_count: 0,
    quarantine: null,
    transient_backoff: null,
  },
  { id: 'serp1', provider: 'serpapi' },
  { id: 'exa1', provider: 'exa' },
  { id: 'brave1', provider: 'brave' },
]

let keysUrls: string[] = []
let root: Root | null = null

function installViewportMocks(): void {
  Object.defineProperty(window, 'matchMedia', {
    configurable: true,
    writable: true,
    value: (query: string) => ({
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

async function waitFor(predicate: () => boolean, timeoutMs = 3000): Promise<void> {
  const deadline = Date.now() + timeoutMs
  while (Date.now() <= deadline) {
    if (predicate()) return
    await flushUi(40)
  }
  throw new Error('Timed out waiting for the admin key provider wiring')
}

function badges(): HTMLElement[] {
  return Array.from(document.querySelectorAll<HTMLElement>('[data-provider].key-provider-badge'))
}

async function mountKeysRoute(): Promise<void> {
  window.localStorage.setItem(DEMO_STORAGE_KEY, 'true')
  window.history.replaceState(null, '', '/admin/keys')
  installDemoRuntime()

  const demoFetch = window.fetch.bind(window)
  window.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = typeof input === 'string' ? input : input instanceof URL ? input.toString() : input.url
    const parsed = new URL(url, 'http://localhost')
    if (parsed.pathname === '/api/keys' && (init?.method ?? 'GET') === 'GET') {
      keysUrls.push(url)
      const provider = parsed.searchParams.get('provider')
      const items = KEY_ROWS.filter((row) => !provider || row.provider === provider).map((row) => {
        const base = KEY_ROWS[0]!
        return {
          ...base,
          ...row,
          provider: row.provider ?? 'tavily',
        }
      })
      return jsonResponse({
        items,
        total: items.length,
        page: 1,
        per_page: 20,
        facets: { groups: [], statuses: [], regions: [] },
      })
    }
    return demoFetch(input, init)
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
  await flushUi(250)
}

beforeEach(() => {
  keysUrls = []
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

describe('AdminDashboard key provider table + filter (plan todo 22)', () => {
  it('renders a provider column with text badges including legacy NULL -> Tavily', async () => {
    await mountKeysRoute()
    await waitFor(() => badges().length >= 4)

    const found = badges().map((badge) => badge.dataset.provider)
    expect(found).toEqual(expect.arrayContaining(['tavily', 'serpapi', 'exa', 'brave']))
    const labels = badges().map((badge) => badge.textContent)
    expect(labels).toEqual(expect.arrayContaining(['Tavily', 'SerpApi', 'Exa', 'Brave']))
    expect(document.body.textContent).toContain('Provider')
  }, 20_000)

  it("narrows the table to Brave rows when the provider filter selects 'Brave'", async () => {
    await mountKeysRoute()
    await waitFor(() => badges().length >= 4)

    const select = document.querySelector<HTMLSelectElement>('select[aria-label="Provider"]')
    expect(select).not.toBeNull()

    await act(async () => {
      select!.value = 'brave'
      select!.dispatchEvent(new Event('change', { bubbles: true }))
    })

    await waitFor(() => keysUrls.some((url) => url.includes('provider=brave')))
    await waitFor(() => badges().length > 0 && badges().every((badge) => badge.dataset.provider === 'brave'))

    expect(new Set(badges().map((badge) => badge.dataset.provider))).toEqual(new Set(['brave']))
    expect(badges().every((badge) => badge.textContent === 'Brave')).toBe(true)
  }, 20_000)
})