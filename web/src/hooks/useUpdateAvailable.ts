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
import { getBundledFrontendVersion } from '../version'

const DISMISS_KEY = 'update-dismissed-version'

function shouldShowUpdateBanner(status: PwaUpdateStatus): boolean {
  return status === 'ready' || status === 'activating' || status === 'activation-failed'
}

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
  const bundledVersionRef = useRef<string | null>(getBundledFrontendVersion())
  const runningVersionRef = useRef<string | null>(bundledVersionRef.current)
  const [currentVersion, setCurrentVersion] = useState<string | null>(bundledVersionRef.current)
  const [availableVersion, setAvailableVersion] = useState<string | null>(null)
  const [visible, setVisible] = useState(false)
  const [updateSnapshot, setUpdateSnapshot] = useState(() => getPwaUpdateSnapshot())
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

  const syncVersionFromServer = useCallback(async () => {
    const next = await loadVersion()
    const nextFrontend = next?.frontend ?? null
    if (!nextFrontend) return null

    if (!runningVersionRef.current) {
      const runningVersion = bundledVersionRef.current ?? nextFrontend
      runningVersionRef.current = runningVersion
      setCurrentVersion((previous) => previous ?? runningVersion)
    }

    if (runningVersionRef.current && nextFrontend !== runningVersionRef.current) {
      setAvailableVersion(nextFrontend)
    }

    return nextFrontend
  }, [loadVersion])

  useEffect(() => {
    let cancelled = false
    ;(async () => {
      const nextFrontend = await loadVersion()
      if (cancelled) return

      const frontend = nextFrontend?.frontend ?? null
      if (!frontend) return

      if (!runningVersionRef.current) {
        runningVersionRef.current = frontend
        setCurrentVersion(frontend)
        return
      }

      if (frontend !== runningVersionRef.current) {
        setAvailableVersion(frontend)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [loadVersion])

  useEffect(() => subscribePwaUpdateState(setUpdateSnapshot), [])

  const checkVersion = useCallback(async () => {
    const nextFrontend = await syncVersionFromServer()
    if (!nextFrontend) return

    if (runningVersionRef.current && nextFrontend !== runningVersionRef.current) {
      void checkForPwaUpdate()
    }
  }, [syncVersionFromServer])

  // When SSE connects (or reconnects), re-check version and ask the SW to look for new assets.
  useEffect(() => {
    const unsubscribe = subscribeToSseOpen(() => {
      void checkVersion()
    })
    return unsubscribe
  }, [checkVersion])

  useEffect(() => {
    if (
      updateSnapshot.hasUpdate
      && (updateSnapshot.status === 'ready' || updateSnapshot.status === 'activation-failed')
    ) {
      void syncVersionFromServer()
    }
  }, [syncVersionFromServer, updateSnapshot.hasUpdate, updateSnapshot.status])

  useEffect(() => {
    if (!updateSnapshot.hasUpdate || !shouldShowUpdateBanner(updateSnapshot.status)) {
      setVisible(false)
      return
    }

    const dismissed = dismissedVersionRef.current
    const candidateVersion = availableVersion ?? 'service-worker-update'
    if (updateSnapshot.status !== 'activation-failed' && dismissed === candidateVersion) return

    setVisible(true)
  }, [availableVersion, updateSnapshot.hasUpdate, updateSnapshot.status])

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

  const loading = updateSnapshot.status === 'activating'

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
