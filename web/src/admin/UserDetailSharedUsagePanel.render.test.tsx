import '../../test/happydom'

import { afterEach, describe, expect, it } from 'bun:test'
import { act, type ComponentProps } from 'react'
import { createRoot, type Root } from 'react-dom/client'

import type { AdminUserUsageSeries, AdminUserUsageSeriesKey } from '../api'
import { ZH } from '../i18n/translations/zh'
import { ThemeProvider, useTheme } from '../theme'
import { UserDetailSharedUsagePanel } from './UserDetailSharedUsagePanel'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((innerResolve, innerReject) => {
    resolve = innerResolve
    reject = innerReject
  })
  return { promise, resolve, reject }
}

async function flushEffects(): Promise<void> {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
    await Promise.resolve()
  })
}

function abortError(): Error {
  try {
    return new DOMException('Aborted', 'AbortError') as Error
  } catch {
    const error = new Error('Aborted')
    error.name = 'AbortError'
    return error
  }
}

interface AbortableSeriesRequest {
  signal: AbortSignal
  deferred: ReturnType<typeof deferred<AdminUserUsageSeries>>
}

function createAbortableLoader() {
  const requests: Partial<Record<AdminUserUsageSeriesKey, AbortableSeriesRequest[]>> = {}

  const loadSeries: ComponentProps<typeof UserDetailSharedUsagePanel>['loadSeries'] = (series, signal) => {
    const request = { signal, deferred: deferred<AdminUserUsageSeries>() }
    requests[series] = [...(requests[series] ?? []), request]
    signal.addEventListener(
      'abort',
      () => {
        request.deferred.reject(abortError())
      },
      { once: true },
    )
    return request.deferred.promise
  }

  return {
    loadSeries,
    requests,
  }
}

function buildEmptySeries(limit: number): AdminUserUsageSeries {
  return {
    limit,
    points: [{ bucketStart: 1_776_200_400, value: null, limitValue: null }],
  }
}

interface MountedPanel {
  container: HTMLDivElement
  root: Root
}

async function mountPanel(
  props: Partial<ComponentProps<typeof UserDetailSharedUsagePanel>> = {},
): Promise<MountedPanel> {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)

  await act(async () => {
    root.render(
      <ThemeProvider>
        <UserDetailSharedUsagePanel
          usersStrings={ZH.admin.users}
          language="zh"
          loadSeries={async () => buildEmptySeries(100)}
          {...props}
        />
      </ThemeProvider>,
    )
  })
  await flushEffects()

  return { container, root }
}

function clickTab(container: HTMLElement, label: string) {
  const buttons = Array.from(container.querySelectorAll<HTMLButtonElement>('button[role="radio"]'))
  const target = buttons.find((button) => button.textContent?.trim() === label)
  if (!target) {
    throw new Error(`tab not found: ${label}`)
  }
  target.click()
}

afterEach(() => {
  document.body.innerHTML = ''
  document.documentElement.classList.remove('dark')
  document.documentElement.style.colorScheme = ''
  window.localStorage.clear()
})

describe('UserDetailSharedUsagePanel tab presentation', () => {
  it('orders windows from shortest to longest while keeping 1h as the default active series', async () => {
    const { container, root } = await mountPanel()

    const labels = Array.from(container.querySelectorAll<HTMLButtonElement>('button[role="radio"]'))
      .map((button) => button.textContent?.trim())

    expect(labels).toEqual([
      ZH.admin.users.detail.sharedUsageTabs.fiveMinute,
      ZH.admin.users.detail.sharedUsageTabs.oneHour,
      ZH.admin.users.detail.sharedUsageTabs.daily,
      ZH.admin.users.detail.sharedUsageTabs.monthly,
    ])
    expect(container.querySelector<HTMLElement>('.admin-user-shared-usage-panel')?.dataset.activeSeries).toBe('quota1h')

    await act(async () => {
      root.unmount()
    })
  })
})

describe('UserDetailSharedUsagePanel loading behavior', () => {
  it('keeps an in-flight tab request usable after switching away and back', async () => {
    const loader = createAbortableLoader()
    const { container, root } = await mountPanel({ loadSeries: loader.loadSeries })

    expect(loader.requests.quota1h?.length).toBe(1)
    expect(container.textContent).toContain(ZH.admin.users.detail.sharedUsageLoading)

    await act(async () => {
      clickTab(container, ZH.admin.users.detail.sharedUsageTabs.fiveMinute)
    })
    await flushEffects()

    expect(loader.requests.rate5m?.length).toBe(1)
    expect(loader.requests.quota1h?.[0]?.signal.aborted).toBe(false)

    await act(async () => {
      clickTab(container, ZH.admin.users.detail.sharedUsageTabs.oneHour)
    })
    await flushEffects()

    expect(loader.requests.quota1h?.length).toBe(1)
    loader.requests.quota1h?.[0]?.deferred.resolve(buildEmptySeries(120))
    await flushEffects()

    expect(container.textContent).not.toContain(ZH.admin.users.detail.sharedUsageLoading)
    expect(container.textContent).toContain(ZH.admin.users.detail.sharedUsageEmpty)

    await act(async () => {
      root.unmount()
    })
  })

  it('treats missing historical limit snapshots as partial history', async () => {
    const { container, root } = await mountPanel({
      loadSeries: async () => ({
        limit: 120,
        points: [{ bucketStart: 1_776_200_400, value: 36, limitValue: null }],
      }),
    })

    expect(container.textContent).toContain(ZH.admin.users.detail.sharedUsagePartialHint)
    expect(container.textContent).not.toContain('· 120')
    expect(container.textContent).not.toContain(ZH.admin.users.detail.sharedUsageEmpty)

    await act(async () => {
      root.unmount()
    })
  })

  it('refetches the active series after the backing user detail refreshes', async () => {
    const firstLoader = createAbortableLoader()
    const secondLoader = createAbortableLoader()
    const { container, root } = await mountPanel({ loadSeries: firstLoader.loadSeries })

    expect(firstLoader.requests.quota1h?.length).toBe(1)
    firstLoader.requests.quota1h?.[0]?.deferred.resolve(buildEmptySeries(120))
    await flushEffects()
    expect(container.textContent).toContain(ZH.admin.users.detail.sharedUsageEmpty)

    await act(async () => {
      root.render(
        <ThemeProvider>
          <UserDetailSharedUsagePanel
            key="after-refresh"
            usersStrings={ZH.admin.users}
            language="zh"
            loadSeries={secondLoader.loadSeries}
          />
        </ThemeProvider>,
      )
    })
    await flushEffects()

    expect(secondLoader.requests.quota1h?.length).toBe(1)
    expect(container.textContent).toContain(ZH.admin.users.detail.sharedUsageLoading)

    secondLoader.requests.quota1h?.[0]?.deferred.resolve(buildEmptySeries(200))
    await flushEffects()
    expect(container.textContent).toContain(ZH.admin.users.detail.sharedUsageEmpty)

    await act(async () => {
      root.unmount()
    })
  })

  it('settles into an error state without retrying forever and lets the operator retry manually', async () => {
    let attempts = 0
    const { container, root } = await mountPanel({
      loadSeries: async () => {
        attempts += 1
        if (attempts === 1) {
          throw new Error('boom')
        }
        return buildEmptySeries(100)
      },
    })

    expect(container.textContent).toContain(ZH.admin.users.detail.sharedUsageLoadFailed)
    expect(container.textContent).toContain(ZH.admin.users.detail.sharedUsageRetryAction)
    expect(attempts).toBe(1)

    await flushEffects()
    await flushEffects()

    expect(container.textContent).toContain(ZH.admin.users.detail.sharedUsageLoadFailed)
    expect(attempts).toBe(1)

    const retryButton = Array.from(container.querySelectorAll<HTMLButtonElement>('button')).find(
      (button) => button.textContent?.trim() === ZH.admin.users.detail.sharedUsageRetryAction,
    )
    expect(retryButton).toBeDefined()

    await act(async () => {
      retryButton?.click()
    })
    await flushEffects()

    expect(attempts).toBe(2)
    expect(container.textContent).toContain(ZH.admin.users.detail.sharedUsageEmpty)

    await act(async () => {
      root.unmount()
    })
  })
})

describe('UserDetailSharedUsagePanel theme behavior', () => {
  it('refreshes its theme-bound chart state when the admin theme changes', async () => {
    function ThemeHarness(): JSX.Element {
      const { setMode } = useTheme()
      return (
        <>
          <button type="button" onClick={() => setMode('dark')}>
            toggle-dark
          </button>
          <UserDetailSharedUsagePanel
            usersStrings={ZH.admin.users}
            language="zh"
            loadSeries={async () => buildEmptySeries(100)}
          />
        </>
      )
    }

    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)

    await act(async () => {
      root.render(
        <ThemeProvider>
          <ThemeHarness />
        </ThemeProvider>,
      )
    })
    await flushEffects()

    const panel = container.querySelector<HTMLElement>('.admin-user-shared-usage-panel')
    const toggle = Array.from(container.querySelectorAll<HTMLButtonElement>('button')).find(
      (button) => button.textContent === 'toggle-dark',
    )
    expect(panel?.dataset.resolvedTheme).toBe('light')
    expect(toggle).toBeDefined()

    await act(async () => {
      toggle?.click()
    })
    await flushEffects()

    expect(panel?.dataset.resolvedTheme).toBe('dark')
    expect(document.documentElement.classList.contains('dark')).toBe(true)

    await act(async () => {
      root.unmount()
    })
  })
})
