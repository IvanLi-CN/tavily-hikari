import { afterEach, describe, expect, it, mock } from 'bun:test'

import { finalizeLinuxDoAuth } from './api'

const originalFetch = globalThis.fetch

afterEach(() => {
  globalThis.fetch = originalFetch
})

describe('linuxdo auth api helpers', () => {
  it('posts finalize requests with credentials so the session cookie persists', async () => {
    const fetchMock = mock((_input: RequestInfo | URL, _init?: RequestInit) =>
      Promise.resolve(
        new Response(JSON.stringify({
          outcome: 'success',
          provider: 'linuxdo',
          redirectTo: '/console',
          detail: null,
        }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    await finalizeLinuxDoAuth('oauth-code', 'oauth-state')

    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/auth/linuxdo/finalize')
    expect(fetchMock.mock.calls[0]?.[1]).toMatchObject({
      method: 'POST',
      credentials: 'include',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ code: 'oauth-code', state: 'oauth-state' }),
    })
  })
})
