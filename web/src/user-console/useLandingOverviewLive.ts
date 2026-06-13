import { useCallback, useEffect, useRef, type Dispatch, type SetStateAction } from 'react'

import {
  buildUserDashboardEventsUrl,
  fetchUserDashboardOverview,
  parseUserDashboardOverviewEventSnapshot,
  type TodayWindowRange,
  type UserDashboard,
  type UserDashboardOverview,
} from '../api'
import type { UserConsoleRoute } from '../lib/userConsoleRoutes'

function errorStatus(err: unknown): number | undefined {
  return typeof err === 'object' && err !== null && 'status' in err
    && typeof (err as { status?: unknown }).status === 'number'
    ? (err as { status: number }).status
    : undefined
}

interface UseLandingOverviewLiveArgs {
  consoleAvailability: string
  route: UserConsoleRoute
  todayWindow: TodayWindowRange
  abortActiveConsoleLoads: () => void
  redirectAfterLogoutIfNeeded: (
    locationLike: Pick<Location, 'pathname' | 'search'>,
    options?: { showLoggedOutState?: boolean },
  ) => Promise<boolean>
  setDashboard: Dispatch<SetStateAction<UserDashboard | null>>
  setDashboardOverview: Dispatch<SetStateAction<UserDashboardOverview | null>>
  setError: Dispatch<SetStateAction<string | null>>
}

export function useLandingOverviewLive({
  consoleAvailability,
  route,
  todayWindow,
  abortActiveConsoleLoads,
  redirectAfterLogoutIfNeeded,
  setDashboard,
  setDashboardOverview,
  setError,
}: UseLandingOverviewLiveArgs): () => void {
  const landingEventsRef = useRef<EventSource | null>(null)
  const fallbackTimerRef = useRef<number | null>(null)

  const stopLandingOverviewLive = useCallback(() => {
    landingEventsRef.current?.close()
    landingEventsRef.current = null
    if (fallbackTimerRef.current != null) {
      window.clearInterval(fallbackTimerRef.current)
      fallbackTimerRef.current = null
    }
  }, [])

  useEffect(() => {
    let disposed = false

    const stopFallbackRefresh = () => {
      if (fallbackTimerRef.current != null) {
        window.clearInterval(fallbackTimerRef.current)
        fallbackTimerRef.current = null
      }
    }

    stopLandingOverviewLive()

    if (consoleAvailability !== 'enabled' || route.name !== 'landing') {
      return () => {
        disposed = true
        stopFallbackRefresh()
      }
    }

    const refreshOverview = async () => {
      try {
        const nextOverview = await fetchUserDashboardOverview(todayWindow)
        if (disposed) return
        setDashboard(nextOverview.summary)
        setDashboardOverview(nextOverview)
        setError(null)
      } catch (err) {
        if (disposed) return
        if (errorStatus(err) === 401) {
          abortActiveConsoleLoads()
          void redirectAfterLogoutIfNeeded(window.location)
          return
        }
        console.error('failed to refresh user dashboard overview', err)
      }
    }

    const startFallbackRefresh = () => {
      if (fallbackTimerRef.current != null) return
      fallbackTimerRef.current = window.setInterval(() => {
        void refreshOverview()
      }, 5_000)
    }

    if (typeof EventSource === 'undefined') {
      startFallbackRefresh()
      return () => {
        disposed = true
        stopFallbackRefresh()
      }
    }

    const source = new EventSource(buildUserDashboardEventsUrl(todayWindow))
    landingEventsRef.current = source

    const handleSnapshot = (event: MessageEvent<string>) => {
      try {
        const snapshot = parseUserDashboardOverviewEventSnapshot(event.data)
        if (disposed) return
        stopFallbackRefresh()
        setDashboard(snapshot.summary)
        setDashboardOverview(snapshot)
        setError(null)
      } catch (err) {
        console.error('failed to parse user dashboard SSE snapshot', err)
      }
    }

    const handleOpen = () => {
      stopFallbackRefresh()
    }

    const handleError = () => {
      startFallbackRefresh()
    }

    source.addEventListener('snapshot', handleSnapshot as EventListener)
    source.addEventListener('open', handleOpen as EventListener)
    source.onerror = handleError

    return () => {
      disposed = true
      stopFallbackRefresh()
      source.removeEventListener('snapshot', handleSnapshot as EventListener)
      source.removeEventListener('open', handleOpen as EventListener)
      source.close()
      source.onerror = null
      if (landingEventsRef.current === source) {
        landingEventsRef.current = null
      }
    }
  }, [
    abortActiveConsoleLoads,
    consoleAvailability,
    redirectAfterLogoutIfNeeded,
    route,
    setDashboard,
    setDashboardOverview,
    setError,
    stopLandingOverviewLive,
    todayWindow,
  ])

  return stopLandingOverviewLive
}
