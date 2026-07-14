import '../../test/happydom'

import { describe, expect, it } from 'bun:test'

type RuntimeModule = typeof import('./runtime')

class MockServiceWorker extends EventTarget {
  state: ServiceWorkerState = 'installing'
  messages: unknown[] = []
  postMessageError: Error | null = null

  postMessage(message: unknown): void {
    if (this.postMessageError) throw this.postMessageError
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

function installImmediateTimeout(): { run: () => void; restore: () => void } {
  const originalSetTimeout = window.setTimeout
  let callback: (() => void) | null = null
  window.setTimeout = ((handler: TimerHandler) => {
    callback = typeof handler === 'function' ? handler : () => undefined
    return 1
  }) as typeof window.setTimeout

  return {
    run: () => {
      expect(callback).not.toBeNull()
      callback?.()
    },
    restore: () => {
      window.setTimeout = originalSetTimeout
    },
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

async function loadRuntimeWithMock(registration: MockRegistration, pathname = '/'): Promise<{
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
  window.history.replaceState(null, '', pathname)
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

  it('leaves activating and exposes a retryable failure when controller takeover times out', async () => {
    const timeout = installImmediateTimeout()
    try {
      const registration = new MockRegistration()
      const waitingWorker = new MockServiceWorker()
      waitingWorker.state = 'installed'
      registration.waiting = waitingWorker
      const { runtime } = await loadRuntimeWithMock(registration)

      await runtime.registerPwaServiceWorker('public')
      runtime.activateWaitingPwaUpdate()
      expect(runtime.getPwaUpdateSnapshot()).toMatchObject({ status: 'activating', hasUpdate: true })

      timeout.run()
      expect(runtime.getPwaUpdateSnapshot()).toMatchObject({
        status: 'activation-failed',
        hasUpdate: true,
        activationRequested: false,
      })

      runtime.activateWaitingPwaUpdate()
      expect(runtime.getPwaUpdateSnapshot()).toMatchObject({ status: 'activating', activationRequested: true })
      expect(waitingWorker.messages).toHaveLength(2)
    } finally {
      timeout.restore()
    }
  })

  it('silently activates a first-install registration controlled by another scope', async () => {
    const registration = new MockRegistration()
    const waitingWorker = new MockServiceWorker()
    waitingWorker.state = 'installed'
    registration.active = null
    registration.waiting = waitingWorker
    const { runtime } = await loadRuntimeWithMock(registration, '/admin/')
    const snapshots = [] as ReturnType<RuntimeModule['getPwaUpdateSnapshot']>[]

    runtime.subscribePwaUpdateState((snapshot) => snapshots.push({ ...snapshot }))
    await runtime.registerPwaServiceWorker('admin')
    await Promise.resolve()

    expect(snapshots.at(-1)).toMatchObject({ status: 'idle', hasUpdate: false })
    expect(waitingWorker.messages).toEqual([{ type: 'TAVILY_HIKARI_ACTIVATE_UPDATE' }])
  })

  it('silently prepares the waiting worker during pagehide so the next refresh loads the new version', async () => {
    const registration = new MockRegistration()
    const waitingWorker = new MockServiceWorker()
    waitingWorker.state = 'installed'
    registration.waiting = waitingWorker
    const { runtime } = await loadRuntimeWithMock(registration)

    await runtime.registerPwaServiceWorker('public')
    expect(runtime.getPwaUpdateSnapshot()).toMatchObject({ status: 'ready', hasUpdate: true })

    window.dispatchEvent(new Event('pagehide'))

    expect(waitingWorker.messages).toEqual([{ type: 'TAVILY_HIKARI_ACTIVATE_UPDATE' }])
    expect(runtime.getPwaUpdateSnapshot()).toMatchObject({ status: 'ready', hasUpdate: true })
  })

  it('fails activation when the waiting worker becomes redundant', async () => {
    const registration = new MockRegistration()
    const waitingWorker = new MockServiceWorker()
    waitingWorker.state = 'installed'
    registration.waiting = waitingWorker
    const { runtime } = await loadRuntimeWithMock(registration)

    await runtime.registerPwaServiceWorker('public')
    runtime.activateWaitingPwaUpdate()
    waitingWorker.setState('redundant')

    expect(runtime.getPwaUpdateSnapshot()).toMatchObject({
      status: 'activation-failed',
      hasUpdate: true,
      activationRequested: false,
    })
  })

  it('fails activation when an in-progress installation becomes redundant', async () => {
    const registration = new MockRegistration()
    const { runtime } = await loadRuntimeWithMock(registration)

    await runtime.registerPwaServiceWorker('public')
    const installingWorker = new MockServiceWorker()
    registration.installing = installingWorker
    registration.dispatchEvent(new Event('updatefound'))
    runtime.activateWaitingPwaUpdate()
    installingWorker.setState('redundant')

    expect(runtime.getPwaUpdateSnapshot()).toMatchObject({
      status: 'activation-failed',
      hasUpdate: true,
      activationRequested: false,
    })
  })

  it('fails activation when the waiting worker rejects the activation message', async () => {
    const registration = new MockRegistration()
    const waitingWorker = new MockServiceWorker()
    waitingWorker.state = 'installed'
    waitingWorker.postMessageError = new Error('message channel closed')
    registration.waiting = waitingWorker
    const { runtime } = await loadRuntimeWithMock(registration)

    await runtime.registerPwaServiceWorker('public')
    runtime.activateWaitingPwaUpdate()

    expect(runtime.getPwaUpdateSnapshot()).toMatchObject({
      status: 'activation-failed',
      hasUpdate: true,
      activationRequested: false,
    })
  })

  it('reloads when the observed waiting worker activated before the click handler runs', async () => {
    const registration = new MockRegistration()
    const waitingWorker = new MockServiceWorker()
    waitingWorker.state = 'installed'
    registration.waiting = waitingWorker
    const { runtime } = await loadRuntimeWithMock(registration)
    const locationPrototype = Object.getPrototypeOf(window.location) as Location
    const originalReload = locationPrototype.reload
    let reloadCalls = 0
    locationPrototype.reload = () => {
      reloadCalls += 1
    }

    try {
      await runtime.registerPwaServiceWorker('public')
      waitingWorker.setState('activated')
      registration.waiting = null
      runtime.activateWaitingPwaUpdate()
      expect(reloadCalls).toBe(1)
    } finally {
      locationPrototype.reload = originalReload
    }
  })

  it('waits for the target worker to activate when controllerchange arrives before the click handler', async () => {
    const registration = new MockRegistration()
    const waitingWorker = new MockServiceWorker()
    waitingWorker.state = 'installed'
    registration.waiting = waitingWorker
    const { runtime, container } = await loadRuntimeWithMock(registration)
    const locationPrototype = Object.getPrototypeOf(window.location) as Location
    const originalReload = locationPrototype.reload
    let reloadCalls = 0
    locationPrototype.reload = () => {
      reloadCalls += 1
    }

    try {
      await runtime.registerPwaServiceWorker('public')
      container.dispatchEvent(new Event('controllerchange'))
      runtime.activateWaitingPwaUpdate()
      expect(reloadCalls).toBe(0)
      expect(waitingWorker.messages).toEqual([{ type: 'TAVILY_HIKARI_ACTIVATE_UPDATE' }])
      waitingWorker.setState('activated')
      expect(reloadCalls).toBe(1)
    } finally {
      locationPrototype.reload = originalReload
    }
  })

  it('reloads only once when activation is confirmed by worker state and controller change', async () => {
    const registration = new MockRegistration()
    const waitingWorker = new MockServiceWorker()
    waitingWorker.state = 'installed'
    registration.waiting = waitingWorker
    const { runtime, container } = await loadRuntimeWithMock(registration)
    const locationPrototype = Object.getPrototypeOf(window.location) as Location
    const originalReload = locationPrototype.reload
    let reloadCalls = 0
    locationPrototype.reload = () => {
      reloadCalls += 1
    }

    try {
      await runtime.registerPwaServiceWorker('public')
      runtime.activateWaitingPwaUpdate()
      waitingWorker.setState('activated')
      container.dispatchEvent(new Event('controllerchange'))

      expect(reloadCalls).toBe(1)
    } finally {
      locationPrototype.reload = originalReload
    }
  })
})
