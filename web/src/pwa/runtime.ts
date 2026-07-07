import { isDemoMode } from '../api/demo'

export type PwaIdentity = 'public' | 'admin'
export type PwaUpdateStatus = 'idle' | 'checking' | 'installing' | 'ready' | 'activating'

const LANGUAGE_STORAGE_KEY = 'tavily-hikari-language'
const THEME_STORAGE_KEY = 'tavily-hikari-theme-mode'
const ACTIVATE_UPDATE_MESSAGE = 'TAVILY_HIKARI_ACTIVATE_UPDATE'

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
let activationRequested = false
let controllerReloadHandled = false

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

if (canUseDom()) {
  window.addEventListener('online', publishOfflineState)
  window.addEventListener('offline', publishOfflineState)
  if ('serviceWorker' in navigator) {
    navigator.serviceWorker.addEventListener('controllerchange', () => {
      if (!activationRequested || controllerReloadHandled) return
      controllerReloadHandled = true
      window.location.reload()
    })
  }
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

  const registration = await navigator.serviceWorker.register(swPath(identity), { scope: swScope(identity) })
  currentRegistration = registration
  observePwaRegistration(registration)
}

function observePwaRegistration(registration: ServiceWorkerRegistration): void {
  const maybeWaiting = registration.waiting
  if (maybeWaiting && navigator.serviceWorker.controller) {
    waitingWorker = maybeWaiting
    publishUpdateState({ status: 'ready', hasUpdate: true })
  }

  const installing = registration.installing
  if (installing) {
    observeInstallingWorker(installing)
  }

  registration.addEventListener('updatefound', () => {
    const nextWorker = registration.installing
    if (!nextWorker) return
    observeInstallingWorker(nextWorker)
  })
}

function observeInstallingWorker(worker: ServiceWorker): void {
  const isControlledPage = Boolean(navigator.serviceWorker.controller)
  if (isControlledPage) {
    publishUpdateState({ status: 'installing', hasUpdate: true })
  }

  worker.addEventListener('statechange', () => {
    if (worker.state === 'installing') {
      if (isControlledPage) publishUpdateState({ status: 'installing', hasUpdate: true })
      return
    }

    if (worker.state === 'installed') {
      if (!isControlledPage) {
        publishUpdateState({ status: 'idle', hasUpdate: false, activationRequested: false })
        return
      }
      waitingWorker = worker
      publishUpdateState({ status: 'ready', hasUpdate: true })
      if (activationRequested) {
        activateWaitingPwaUpdate()
      }
      return
    }

    if (worker.state === 'activated' && !activationRequested) {
      publishUpdateState({ status: 'idle', hasUpdate: false, activationRequested: false })
    }
  })
}

export async function checkForPwaUpdate(): Promise<void> {
  if (!currentRegistration) return
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
      publishUpdateState({ status: 'idle' })
    }
  }
}

export function activateWaitingPwaUpdate(): void {
  activationRequested = true
  publishUpdateState({ activationRequested: true })

  const worker = waitingWorker ?? currentRegistration?.waiting ?? null
  if (!worker) {
    if (currentUpdateState.status !== 'installing') {
      publishUpdateState({ status: 'checking' })
      void checkForPwaUpdate()
    }
    return
  }

  waitingWorker = worker
  publishUpdateState({ status: 'activating', hasUpdate: true, activationRequested: true })
  worker.postMessage({ type: ACTIVATE_UPDATE_MESSAGE })
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
