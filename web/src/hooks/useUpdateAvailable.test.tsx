import '../../test/happydom'

import { afterEach, describe, expect, it, mock } from 'bun:test'
import { act } from 'react'
import { createRoot } from 'react-dom/client'

import useUpdateAvailable from './useUpdateAvailable'
import { registerPwaServiceWorker } from '../pwa/runtime'

class MockServiceWorker extends EventTarget {
  state: ServiceWorkerState = 'installing'
  messages: unknown[] = []

  postMessage(message: unknown): void {
    this.messages.push(message)
  }

  setState(state: ServiceWorkerState): void {
    this.state = state
    this.dispatchEvent(new Event('statechange'))
  }
}

class MockRegistration extends EventTarget {
  active: MockServiceWorker | null = new MockServiceWorker()
  installing: MockServiceWorker | null = null
  waiting: MockServiceWorker | null = null

  async update(): Promise<void> {
    return undefined
  }
}

class MockServiceWorkerContainer extends EventTarget {
  controller: MockServiceWorker | null = new MockServiceWorker()

  constructor(private readonly registration: MockRegistration) {
    super()
  }

  async register(): Promise<MockRegistration> {
    return this.registration
  }
}

class MockEventSource extends EventTarget {
  close(): void {
    return undefined
  }
}

async function flushEffects(): Promise<void> {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
    await new Promise<void>((resolve) => setTimeout(resolve, 0))
  })
}

describe('useUpdateAvailable', () => {
  const originalFetch = globalThis.fetch

  afterEach(() => {
    globalThis.fetch = originalFetch
    document.body.innerHTML = ''
    window.localStorage.clear()
    delete (globalThis as typeof globalThis & {
      __TAVILY_HIKARI_APP_VERSION_OVERRIDE__?: string
    }).__TAVILY_HIKARI_APP_VERSION_OVERRIDE__
  })

  it('resolves a concrete available version when the waiting worker is ready', async () => {
    ;(globalThis as typeof globalThis & {
      __TAVILY_HIKARI_APP_VERSION_OVERRIDE__?: string
    }).__TAVILY_HIKARI_APP_VERSION_OVERRIDE__ = '0.79.0'

    const versions = ['0.79.1', '0.79.2']
    globalThis.fetch = mock(async (input: RequestInfo | URL) => {
      if (String(input) !== '/api/version') {
        return new Response('not found', { status: 404 })
      }

      const frontend = versions.shift() ?? '0.79.2'
      return Response.json({
        backend: '0.79.1',
        frontend,
      })
    }) as typeof fetch

    const registration = new MockRegistration()
    Object.defineProperty(navigator, 'serviceWorker', {
      configurable: true,
      value: new MockServiceWorkerContainer(registration),
    })
    Object.defineProperty(navigator, 'onLine', {
      configurable: true,
      value: true,
    })
    Object.defineProperty(globalThis, 'EventSource', {
      configurable: true,
      value: MockEventSource,
    })
    window.history.replaceState(null, '', '/')

    let latest: ReturnType<typeof useUpdateAvailable> | null = null
    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)

    function Harness(): null {
      latest = useUpdateAvailable()
      return null
    }

    await act(async () => {
      root.render(<Harness />)
    })
    await flushEffects()

    expect(latest?.currentBackendVersion).toBe('0.79.1')
    expect(latest?.currentVersion).toBe('0.79.0')
    expect(latest?.availableVersion).toBe('0.79.1')
    expect(latest?.visible).toBe(false)

    await act(async () => {
      await registerPwaServiceWorker('public')
    })
    const waitingWorker = new MockServiceWorker()
    await act(async () => {
      registration.installing = waitingWorker
      registration.dispatchEvent(new Event('updatefound'))
      registration.waiting = waitingWorker
      waitingWorker.setState('installed')
    })
    await flushEffects()

    expect(latest?.status).toBe('ready')
    expect(latest?.visible).toBe(true)
    expect(latest?.availableVersion).toBe('0.79.2')

    await act(async () => {
      root.unmount()
    })
  })
})
