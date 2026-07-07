import '../../test/happydom'

import { describe, expect, it } from 'bun:test'

type RuntimeModule = typeof import('./runtime')

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
  updateCalls = 0

  async update(): Promise<void> {
    this.updateCalls += 1
  }
}

class MockServiceWorkerContainer extends EventTarget {
  controller: MockServiceWorker | null = new MockServiceWorker()

  constructor(private readonly registration: MockRegistration) {
    super()
  }

  get ready(): Promise<MockRegistration> {
    return Promise.resolve(this.registration)
  }

  async register(): Promise<MockRegistration> {
    return this.registration
  }
}

async function loadRuntimeWithMock(registration: MockRegistration): Promise<{
  runtime: RuntimeModule
  container: MockServiceWorkerContainer
}> {
  const container = new MockServiceWorkerContainer(registration)
  Object.defineProperty(navigator, 'serviceWorker', {
    configurable: true,
    value: container,
  })
  Object.defineProperty(navigator, 'onLine', {
    configurable: true,
    value: true,
  })
  window.history.replaceState(null, '', '/')
  const runtime = await import(`./runtime.ts?test=${Date.now()}-${Math.random()}`) as RuntimeModule
  return { runtime, container }
}

describe('PWA runtime update lifecycle', () => {
  it('publishes installing and ready only after the update worker finishes installation', async () => {
    const registration = new MockRegistration()
    const { runtime } = await loadRuntimeWithMock(registration)
    const snapshots = [] as ReturnType<RuntimeModule['getPwaUpdateSnapshot']>[]

    runtime.subscribePwaUpdateState((snapshot) => snapshots.push({ ...snapshot }))
    await runtime.registerPwaServiceWorker('public')

    const worker = new MockServiceWorker()
    registration.installing = worker
    registration.dispatchEvent(new Event('updatefound'))
    expect(snapshots.at(-1)).toMatchObject({ status: 'installing', hasUpdate: true })

    registration.waiting = worker
    worker.setState('installed')
    expect(snapshots.at(-1)).toMatchObject({ status: 'ready', hasUpdate: true })
    expect(worker.messages).toEqual([])
  })

  it('keeps apply-update loading while installation is not ready, then activates the waiting worker', async () => {
    const registration = new MockRegistration()
    const { runtime } = await loadRuntimeWithMock(registration)
    const snapshots = [] as ReturnType<RuntimeModule['getPwaUpdateSnapshot']>[]

    runtime.subscribePwaUpdateState((snapshot) => snapshots.push({ ...snapshot }))
    await runtime.registerPwaServiceWorker('public')

    const worker = new MockServiceWorker()
    registration.installing = worker
    registration.dispatchEvent(new Event('updatefound'))
    runtime.activateWaitingPwaUpdate()
    expect(snapshots.at(-1)).toMatchObject({
      status: 'installing',
      hasUpdate: true,
      activationRequested: true,
    })
    expect(worker.messages).toEqual([])

    registration.waiting = worker
    worker.setState('installed')
    expect(snapshots.at(-1)).toMatchObject({
      status: 'activating',
      hasUpdate: true,
      activationRequested: true,
    })
    expect(worker.messages).toEqual([{ type: 'TAVILY_HIKARI_ACTIVATE_UPDATE' }])
  })
})
