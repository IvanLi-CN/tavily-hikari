import { isDemoMode } from '../api/demo'

export type PwaIdentity = 'public' | 'admin'
export type PwaUpdateStatus = 'idle' | 'checking' | 'installing' | 'ready' | 'activating' | 'activation-failed'

const LANGUAGE_STORAGE_KEY = 'tavily-hikari-language'
const THEME_STORAGE_KEY = 'tavily-hikari-theme-mode'
const ACTIVATE_UPDATE_MESSAGE = 'TAVILY_HIKARI_ACTIVATE_UPDATE'
const ACTIVATION_TIMEOUT_MS = 10_000

export interface OfflineStateSnapshot {
  isOffline: boolean
}

export interface PwaUpdateSnapshot {
  status: PwaUpdateStatus
  hasUpdate: boolean
  activationRequested: boolean
}

let currentOfflineState: OfflineStateSnapshot = {
  isOffline: typeof navigator !== 'undefined' ? navigator.onLine === false : false,
}
let currentUpdateState: PwaUpdateSnapshot = {
  status: 'idle',
  hasUpdate: false,
  activationRequested: false,
}
let currentRegistration: ServiceWorkerRegistration | null = null
let waitingWorker: ServiceWorker | null = null
let waitingWorkerActivated = false
let activationRequested = false
let controllerReloadHandled = false
let activationTimeout: number | null = null
let controllerChangeObserved = false
const observedActivationWorkers = new WeakSet<ServiceWorker>()

const offlineListeners = new Set<(snapshot: OfflineStateSnapshot) => void>()
const updateListeners = new Set<(snapshot: PwaUpdateSnapshot) => void>()

function canUseDom(): boolean {
  return typeof window !== 'undefined' && typeof document !== 'undefined'
}

function readOfflineState(): OfflineStateSnapshot {
  return {
    isOffline: typeof navigator !== 'undefined' ? navigator.onLine === false : false,
  }
}

function publishOfflineState(): void {
  currentOfflineState = readOfflineState()
  for (const listener of offlineListeners) {
    listener(currentOfflineState)
  }
}

function publishUpdateState(next: Partial<PwaUpdateSnapshot>): void {
  currentUpdateState = {
    ...currentUpdateState,
    ...next,
  }
  for (const listener of updateListeners) {
    listener(currentUpdateState)
  }
}

function clearActivationTimeout(): void {
  if (activationTimeout === null) return
  window.clearTimeout(activationTimeout)
  activationTimeout = null
}

function ensureActivationTimeout(): void {
  if (activationTimeout !== null) return
  activationTimeout = window.setTimeout(() => {
    if (activationRequested && !controllerReloadHandled) {
      failActivation()
    }
  }, ACTIVATION_TIMEOUT_MS)
}

function reloadForActivatedUpdate(): void {
  if (controllerReloadHandled) return
  controllerReloadHandled = true
  clearActivationTimeout()
  window.location.reload()
}

function failActivation(): void {
  clearActivationTimeout()
  activationRequested = false
  publishUpdateState({ status: 'activation-failed', hasUpdate: true, activationRequested: false })
}

function observeActivationWorker(worker: ServiceWorker): void {
  if (observedActivationWorkers.has(worker)) return
  observedActivationWorkers.add(worker)
  worker.addEventListener('statechange', () => {
    if (worker.state === 'activated') {
      if (worker === waitingWorker) waitingWorkerActivated = true
      if (activationRequested) reloadForActivatedUpdate()
      return
    }
    if (!activationRequested) return
    if (worker.state === 'redundant') {
      failActivation()
    }
  })
}

function postActivationMessage(worker: ServiceWorker): boolean {
  try {
    worker.postMessage({ type: ACTIVATE_UPDATE_MESSAGE })
    return true
  } catch {
    return false
  }
}

function activateFirstInstallWorker(worker: ServiceWorker): void {
  if (!postActivationMessage(worker)) {
    publishUpdateState({ status: 'idle', hasUpdate: false, activationRequested: false })
  }
}

function activateFirstInstallWhenWaiting(
  registration: ServiceWorkerRegistration,
  installedWorker: ServiceWorker,
): void {
  queueMicrotask(() => {
    activateFirstInstallWorker(registration.waiting ?? installedWorker)
  })
}

function observeControllerChange(): void {
  if (controllerChangeObserved) return
  controllerChangeObserved = true
  navigator.serviceWorker.addEventListener('controllerchange', () => {
    if (!activationRequested || controllerReloadHandled) return
    reloadForActivatedUpdate()
  })
}

function silentlyPrepareWaitingWorkerForNextNavigation(): void {
  const registeredWaitingWorker = currentRegistration?.waiting ?? null
  const worker = registeredWaitingWorker ?? (waitingWorker?.state === 'installed' ? waitingWorker : null)
  if (!worker) return
  observeActivationWorker(worker)
  void postActivationMessage(worker)
}

if (canUseDom()) {
  window.addEventListener('online', publishOfflineState)
  window.addEventListener('offline', publishOfflineState)
  window.addEventListener('pagehide', () => {
    if (activationRequested) return
    if (!currentUpdateState.hasUpdate || currentUpdateState.status !== 'ready') return
    silentlyPrepareWaitingWorkerForNextNavigation()
  })
}

function shouldRegisterServiceWorker(): boolean {
  if (!canUseDom()) return false
  const env = (import.meta as ImportMeta & { env?: { DEV?: boolean } }).env
  if (env?.DEV) return false
  if (isDemoMode()) return false
  return 'serviceWorker' in navigator
}

function swPath(identity: PwaIdentity): string {
  return identity === 'admin' ? '/sw-admin.js' : '/sw-public.js'
}

function swScope(identity: PwaIdentity): string {
  return identity === 'admin' ? '/admin/' : '/'
}

export function normalizeAdminShellPath(): void {
  if (!canUseDom()) return
  if (window.location.pathname === '/admin') {
    window.history.replaceState(null, '', `/admin/${window.location.search}${window.location.hash}`)
  }
}

export async function registerPwaServiceWorker(identity: PwaIdentity): Promise<void> {
  if (!shouldRegisterServiceWorker()) return

  const pathname = window.location.pathname
  if (identity === 'admin') {
    if (!(pathname === '/admin/' || pathname.startsWith('/admin/'))) return
  } else if (pathname === '/admin' || pathname.startsWith('/admin/')) {
    return
  }

  observeControllerChange()
  const registration = await navigator.serviceWorker.register(swPath(identity), { scope: swScope(identity) })
  currentRegistration = registration
  observePwaRegistration(registration)
}

function observePwaRegistration(registration: ServiceWorkerRegistration): void {
  const maybeWaiting = registration.waiting
  if (maybeWaiting) {
    if (registration.active) {
      waitingWorker = maybeWaiting
      waitingWorkerActivated = maybeWaiting.state === 'activated'
      observeActivationWorker(maybeWaiting)
      publishUpdateState({ status: 'ready', hasUpdate: true })
    } else {
      activateFirstInstallWhenWaiting(registration, maybeWaiting)
    }
  }

  const installing = registration.installing
  if (installing) {
    observeInstallingWorker(registration, installing)
  }

  registration.addEventListener('updatefound', () => {
    const nextWorker = registration.installing
    if (!nextWorker) return
    observeInstallingWorker(registration, nextWorker)
  })
}

function observeInstallingWorker(registration: ServiceWorkerRegistration, worker: ServiceWorker): void {
  const isUpdate = Boolean(registration.active)
  if (isUpdate) {
    publishUpdateState({ status: 'installing', hasUpdate: true })
  }

  worker.addEventListener('statechange', () => {
    if (worker.state === 'installing') {
      if (isUpdate) publishUpdateState({ status: 'installing', hasUpdate: true })
      return
    }

    if (worker.state === 'installed') {
      if (!isUpdate) {
        activateFirstInstallWhenWaiting(registration, worker)
        publishUpdateState({ status: 'idle', hasUpdate: false, activationRequested: false })
        return
      }
      waitingWorker = worker
      waitingWorkerActivated = false
      observeActivationWorker(worker)
      publishUpdateState({ status: 'ready', hasUpdate: true })
      if (activationRequested) {
        activateWaitingPwaUpdate()
      }
      return
    }

    if (worker.state === 'redundant' && activationRequested) {
      failActivation()
      return
    }

    if (worker.state === 'activated' && !isUpdate && !activationRequested) {
      publishUpdateState({ status: 'idle', hasUpdate: false, activationRequested: false })
    }
  })
}

export async function checkForPwaUpdate(): Promise<void> {
  if (!currentRegistration) {
    if (activationRequested) failActivation()
    return
  }
  if (currentUpdateState.status === 'installing' || currentUpdateState.status === 'ready' || currentUpdateState.status === 'activating') {
    return
  }

  publishUpdateState({ status: 'checking' })
  try {
    await currentRegistration.update()
  } catch {
    // Network failures while checking for an app shell update should not break the page.
  } finally {
    if (currentUpdateState.status === 'checking') {
      if (activationRequested) {
        failActivation()
      } else {
        publishUpdateState({ status: 'idle' })
      }
    }
  }
}

export function activateWaitingPwaUpdate(): void {
  if (!activationRequested) {
    clearActivationTimeout()
    activationRequested = true
    controllerReloadHandled = false
    publishUpdateState({ activationRequested: true })
    ensureActivationTimeout()
  }

  const registeredWaitingWorker = currentRegistration?.waiting ?? null
  if (waitingWorkerActivated) {
    reloadForActivatedUpdate()
    return
  }
  const worker = registeredWaitingWorker ?? (waitingWorker?.state === 'installed' ? waitingWorker : null)
  if (!worker) {
    if (currentUpdateState.status !== 'installing') {
      publishUpdateState({ status: 'checking' })
      void checkForPwaUpdate()
    }
    return
  }

  waitingWorker = worker
  waitingWorkerActivated = worker.state === 'activated'
  observeActivationWorker(worker)
  publishUpdateState({ status: 'activating', hasUpdate: true, activationRequested: true })
  if (!postActivationMessage(worker)) {
    failActivation()
  }
}

export function subscribeOfflineState(listener: (snapshot: OfflineStateSnapshot) => void): () => void {
  offlineListeners.add(listener)
  listener(currentOfflineState)
  return () => {
    offlineListeners.delete(listener)
  }
}

export function getOfflineStateSnapshot(): OfflineStateSnapshot {
  return currentOfflineState
}

export function subscribePwaUpdateState(listener: (snapshot: PwaUpdateSnapshot) => void): () => void {
  updateListeners.add(listener)
  listener(currentUpdateState)
  return () => {
    updateListeners.delete(listener)
  }
}

export function getPwaUpdateSnapshot(): PwaUpdateSnapshot {
  return currentUpdateState
}

export function bootstrapOfflineShellDocument(): void {
  if (!canUseDom()) return
  try {
    const language = window.localStorage.getItem(LANGUAGE_STORAGE_KEY)
    if (language === 'en' || language === 'zh') {
      document.documentElement.lang = language
    }
  } catch {
    // ignore storage failures
  }
  try {
    const theme = window.localStorage.getItem(THEME_STORAGE_KEY)
    if (theme === 'dark') {
      document.documentElement.classList.add('dark')
      document.documentElement.style.colorScheme = 'dark'
      return
    }
    if (theme === 'light') {
      document.documentElement.classList.remove('dark')
      document.documentElement.style.colorScheme = 'light'
      return
    }
  } catch {
    // ignore storage failures
  }
}
