import '../../test/happydom'

import { afterEach, beforeEach, describe, expect, it } from 'bun:test'
import { StrictMode } from 'react'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'

import { installDemoRuntime } from '../api/demo'
import { TooltipProvider } from '../components/ui/tooltip'
import { LanguageProvider } from '../i18n'
import { ThemeProvider } from '../theme'
import AdminDashboard from './AdminDashboardRuntime'
import {
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
  selector?: string
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

function withDemoMode(path: string): string {
  return path
}

function resetDemoRuntime(): void {
  window.fetch = originalFetch
  window.EventSource = originalEventSource
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
  if (expectation.selector) {
    expect(main?.querySelector(expectation.selector)).not.toBeNull()
  }
  if (expectation.text) {
    expect(main?.textContent).toContain(expectation.text)
  }
}

async function navigateTo(path: string): Promise<void> {
  await act(async () => {
    window.history.pushState(null, '', withDemoMode(path))
    window.dispatchEvent(new PopStateEvent('popstate'))
  })
  await flushUi(180)
}

async function mountAdminDashboard(initialPath: string): Promise<{ container: HTMLDivElement; root: Root }> {
  window.localStorage.setItem(DEMO_STORAGE_KEY, 'true')
  window.history.replaceState(null, '', withDemoMode(initialPath))
  installDemoRuntime()

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
})

afterEach(async () => {
  console.error = originalConsoleError
  window.fetch = originalFetch
  window.EventSource = originalEventSource
  window.matchMedia = originalMatchMedia
  window.requestAnimationFrame = originalRequestAnimationFrame
  window.cancelAnimationFrame = originalCancelAnimationFrame
  HTMLElement.prototype.scrollIntoView = originalScrollIntoView
  resetDemoRuntime()
  window.localStorage.removeItem(DEMO_STORAGE_KEY)
  document.body.innerHTML = ''
})

describe('AdminDashboard route switches', () => {
  for (const routeCase of routeSwitchCases) {
    it(`keeps admin runtime stable when switching ${routeCase.name}`, async () => {
      const { container, root } = await mountAdminDashboard(routeCase.startPath)

      try {
        assertRouteRendered(container, {})

        await navigateTo(routeCase.nextPath)
        assertRouteRendered(container, routeCase.nextExpectation)

        if (routeCase.returnPath && routeCase.returnExpectation) {
          await navigateTo(routeCase.returnPath)
          assertRouteRendered(container, routeCase.returnExpectation)
        }

        expect(hookOrderErrors()).toEqual([])
      } finally {
        await act(async () => {
          root.unmount()
        })
      }
    })
  }
})
