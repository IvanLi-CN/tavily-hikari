import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { fetchVersion } from '../api'
import { subscribeToSseOpen } from '../sse'

const DISMISS_KEY = 'update-dismissed-version'

export interface UpdateAvailableState {
  currentVersion: string | null
  availableVersion: string | null
  visible: boolean
  dismiss: () => void
  reload: () => void
}

export default function useUpdateAvailable(): UpdateAvailableState {
  const [currentVersion, setCurrentVersion] = useState<string | null>(null)
  const [availableVersion, setAvailableVersion] = useState<string | null>(null)
  const [visible, setVisible] = useState(false)
  const initialVersionRef = useRef<string | null>(null)

  const dismissed = useMemo(() => {
    try {
      return localStorage.getItem(DISMISS_KEY)
    } catch {
      return null
    }
  }, [])

  const loadVersion = useCallback(async () => {
    try {
      const v = await fetchVersion()
      return v.backend
    } catch {
      return null
    }
  }, [])

  // Fetch initial version
  useEffect(() => {
    let cancelled = false
    ;(async () => {
      const v = await loadVersion()
      if (cancelled) return
      setCurrentVersion(v)
      initialVersionRef.current = v
    })()
    return () => {
      cancelled = true
    }
  }, [loadVersion])

  // When SSE connects (or reconnects), re-check version
  useEffect(() => {
    const unsubscribe = subscribeToSseOpen(async () => {
      const next = await loadVersion()
      if (!next) return
      const initial = initialVersionRef.current
      if (!initial) {
        initialVersionRef.current = next
        setCurrentVersion(next)
        return
      }
      if (next !== initial && next !== dismissed) {
        setAvailableVersion(next)
        setVisible(true)
      }
    })
    return unsubscribe
  }, [dismissed, loadVersion])

  const dismiss = useCallback(() => {
    if (availableVersion) {
      try {
        localStorage.setItem(DISMISS_KEY, availableVersion)
      } catch {
        /* noop */
      }
    }
    setVisible(false)
  }, [availableVersion])

  const reload = useCallback(() => {
    window.location.reload()
  }, [])

  // Provide a manual trigger for validation/testing
  useEffect(() => {
    ;(window as unknown as { __FORCE_UPDATE_BANNER__?: () => void }).__FORCE_UPDATE_BANNER__ = () => {
      setAvailableVersion((v) => v ?? (currentVersion ? `${currentVersion}-next` : 'next'))
      setVisible(true)
    }
  }, [currentVersion])

  return { currentVersion, availableVersion, visible, dismiss, reload }
}
