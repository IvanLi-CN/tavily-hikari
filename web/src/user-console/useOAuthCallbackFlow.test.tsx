import '../../test/happydom'

import { afterEach, describe, expect, it, mock } from 'bun:test'
import { StrictMode } from 'react'
import { act } from 'react'
import { createRoot } from 'react-dom/client'

import { useOAuthCallbackFlow } from './useOAuthCallbackFlow'
import { EN } from './text'

const originalFetch = globalThis.fetch
const originalReplaceState = window.history.replaceState
async function flushEffects(ms = 0): Promise<void> {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
    await new Promise<void>((resolve) => window.setTimeout(resolve, 0))
    if (ms > 0) {
      await new Promise<void>((resolve) => window.setTimeout(resolve, ms))
    }
  })
}

afterEach(() => {
  globalThis.fetch = originalFetch
  window.history.replaceState = originalReplaceState
  window.history.replaceState({}, '', '/')
  document.body.innerHTML = ''
})

describe('useOAuthCallbackFlow', () => {
  it('keeps the original callback query through StrictMode rerenders and finalizes only once', async () => {
    const route = { name: 'oauthCallback', provider: 'linuxdo' } as const
    const abortActiveConsoleLoads = mock(() => undefined)
    const setLoading = mock((_value: boolean) => undefined)
    const setError = mock((_value: string | null) => undefined)
    const fetchMock = mock(async (_input: RequestInfo | URL, init?: RequestInit) => {
      expect(init?.method).toBe('POST')
      expect(init?.headers).toEqual({ 'Content-Type': 'application/json' })
      expect(init?.body).toBe(JSON.stringify({ code: 'oauth-code', state: 'oauth-state' }))
      return new Response(
        JSON.stringify({
          outcome: 'invalid_state',
          provider: 'linuxdo',
          redirectTo: null,
          detail: 'expired oauth state',
        }),
        {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        },
      )
    })
    globalThis.fetch = fetchMock as typeof fetch

    const replaceStateMock = mock((_state: unknown, _unused: string, url?: string | URL | null) => {
      if (typeof url === 'string') {
        window.history.pushState({}, '', url)
      }
    })
    window.history.replaceState = replaceStateMock as typeof window.history.replaceState
    window.history.replaceState({}, '', '/console/oauth/linuxdo/callback?code=oauth-code&state=oauth-state')

    let latestModelTitle = ''
    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)

    function Harness(): null {
      const flow = useOAuthCallbackFlow({
        route,
        providers: EN.header.providers,
        text: EN.oauthCallback,
        abortActiveConsoleLoads,
        setLoading,
        setError,
      })
      latestModelTitle = flow.model.title
      return null
    }

    await act(async () => {
      root.render(
        <StrictMode>
          <Harness />
        </StrictMode>,
      )
    })
    await flushEffects(900)

    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(replaceStateMock).toHaveBeenCalled()
    expect(window.location.search).toBe('')
    expect(latestModelTitle).toBe('This login attempt has expired')

    await act(async () => root.unmount())
  })

  it('clears the delayed success redirect when the callback unmounts', async () => {
    const route = { name: 'oauthCallback', provider: 'linuxdo' } as const
    const abortActiveConsoleLoads = mock(() => undefined)
    const setLoading = mock((_value: boolean) => undefined)
    const setError = mock((_value: string | null) => undefined)
    const fetchMock = mock(async () => {
      return new Response(
        JSON.stringify({
          outcome: 'success',
          provider: 'linuxdo',
          redirectTo: '/console',
          detail: null,
        }),
        {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        },
      )
    })
    globalThis.fetch = fetchMock as typeof fetch

    const replaceStateMock = mock((_state: unknown, _unused: string, url?: string | URL | null) => {
      if (typeof url === 'string') {
        window.history.pushState({}, '', url)
      }
    })
    window.history.replaceState = replaceStateMock as typeof window.history.replaceState
    window.history.replaceState({}, '', '/console/oauth/linuxdo/callback?code=oauth-code&state=oauth-state')

    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)

    function Harness(): null {
      useOAuthCallbackFlow({
        route,
        providers: EN.header.providers,
        text: EN.oauthCallback,
        abortActiveConsoleLoads,
        setLoading,
        setError,
      })
      return null
    }

    try {
      await act(async () => {
        root.render(<Harness />)
      })
      await flushEffects(50)
      expect(window.location.pathname).toBe('/console/oauth/linuxdo/callback')
      await act(async () => root.unmount())
      await flushEffects(900)

      expect(fetchMock).toHaveBeenCalledTimes(1)
      expect(window.location.pathname).toBe('/console/oauth/linuxdo/callback')
    } finally {
      // no-op
    }
  })
})
