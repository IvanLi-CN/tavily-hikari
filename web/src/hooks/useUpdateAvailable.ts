import { useCallback, useEffect, useRef, useState } from 'react'
import { fetchVersion, type VersionInfo } from '../api'
import {
  activateWaitingPwaUpdate,
  checkForPwaUpdate,
  getPwaUpdateSnapshot,
  subscribePwaUpdateState,
  type PwaUpdateStatus,
} from '../pwa/runtime'
import { subscribeToSseOpen } from '../sse'

const DISMISS_KEY = 'update-dismissed-version'

export interface UpdateAvailableState {
  currentVersion: string | null
  availableVersion: string | null
  status: PwaUpdateStatus
  visible: boolean
  loading: boolean
  dismiss: () => void
  applyUpdate: () => void
}

export default function useUpdateAvailable(): UpdateAvailableState {
  const [currentVersion, setCurrentVersion] = useState<string | null>(null)
  const [availableVersion, setAvailableVersion] = useState<string | null>(null)
  const [visible, setVisible] = useState(false)
  const [updateSnapshot, setUpdateSnapshot] = useState(() => getPwaUpdateSnapshot())
  const initialVersionRef = useRef<string | null>(null)
  const dismissedVersionRef = useRef<string | null>(null)

  const loadVersion = useCallback(async (): Promise<VersionInfo | null> => {
    try {
      return await fetchVersion()
    } catch {
      return null
    }
  }, [])

  useEffect(() => {
    try {
      dismissedVersionRef.current = localStorage.getItem(DISMISS_KEY)
    } catch {
      dismissedVersionRef.current = null
    }
  }, [])

  // Fetch initial version
  useEffect(() => {
    let cancelled = false
    ;(async () => {
      const v = await loadVersion()
      if (cancelled) return
      const frontend = v?.frontend ?? null
      setCurrentVersion(frontend)
      initialVersionRef.current = frontend
    })()
    return () => {
      cancelled = true
    }
  }, [loadVersion])

  useEffect(() => subscribePwaUpdateState(setUpdateSnapshot), [])

  const checkVersion = useCallback(async () => {
    const next = await loadVersion()
    const nextFrontend = next?.frontend ?? null
    if (!nextFrontend) return

    const initial = initialVersionRef.current
    if (!initial) {
      initialVersionRef.current = nextFrontend
      setCurrentVersion(nextFrontend)
      return
    }

    if (nextFrontend !== initial) {
      setAvailableVersion(nextFrontend)
      void checkForPwaUpdate()
    }
  }, [loadVersion])

  // When SSE connects (or reconnects), re-check version and ask the SW to look for new assets.
  useEffect(() => {
    const unsubscribe = subscribeToSseOpen(() => {
      void checkVersion()
    })
    return unsubscribe
  }, [checkVersion])

  useEffect(() => {
    if (!updateSnapshot.hasUpdate) {
      setVisible(false)
      return
    }

    const dismissed = dismissedVersionRef.current
    const candidateVersion = availableVersion ?? 'service-worker-update'
    if (dismissed === candidateVersion) return

    setVisible(true)
  }, [availableVersion, updateSnapshot.hasUpdate])

  const dismiss = useCallback(() => {
    const candidateVersion = availableVersion ?? 'service-worker-update'
    if (candidateVersion) {
      try {
        localStorage.setItem(DISMISS_KEY, candidateVersion)
        dismissedVersionRef.current = candidateVersion
      } catch {
        /* noop */
      }
    }
    setVisible(false)
  }, [availableVersion])

  const applyUpdate = useCallback(() => {
    setVisible(true)
    activateWaitingPwaUpdate()
  }, [])

  // Provide a manual trigger for validation/testing
  useEffect(() => {
    ;(window as unknown as { __FORCE_UPDATE_BANNER__?: () => void }).__FORCE_UPDATE_BANNER__ = () => {
      setAvailableVersion((v) => v ?? (currentVersion ? `${currentVersion}-next` : 'next'))
      setVisible(true)
    }
  }, [currentVersion])

  const loading = updateSnapshot.status === 'checking'
    || updateSnapshot.status === 'installing'
    || updateSnapshot.status === 'activating'

  return {
    currentVersion,
    availableVersion,
    status: updateSnapshot.status,
    visible,
    loading,
    dismiss,
    applyUpdate,
  }
}
