import { useCallback, useEffect, useMemo, useRef, useState, type Dispatch, type SetStateAction } from 'react'

import { finalizeLinuxDoAuth, type LinuxDoFinalizeResult } from '../api'
import { userConsoleRouteToPath, type UserConsoleRoute } from '../lib/userConsoleRoutes'
import {
  OAUTH_CALLBACK_FINALIZE_TIMEOUT_MS,
  USER_CONSOLE_LOGIN_START_PATH,
  parseOAuthCallbackQuery,
  resolveOAuthCallbackPanelModel,
  resolveOAuthCallbackProviderLabel,
  type OAuthCallbackScreenState,
} from './oauthCallback'
import type { EN } from './text'

interface UseOAuthCallbackFlowArgs {
  route: UserConsoleRoute
  providers: typeof EN.header.providers
  text: typeof EN.oauthCallback
  abortActiveConsoleLoads: () => void
  setLoading: Dispatch<SetStateAction<boolean>>
  setError: Dispatch<SetStateAction<string | null>>
}

export function useOAuthCallbackFlow({
  route,
  providers,
  text,
  abortActiveConsoleLoads,
  setLoading,
  setError,
}: UseOAuthCallbackFlowArgs) {
  const isOAuthCallbackRoute = route.name === 'oauthCallback'
  const oauthCallbackProvider = route.name === 'oauthCallback' ? route.provider : null
  const [oauthCallbackState, setOauthCallbackState] = useState<OAuthCallbackScreenState>('connecting')
  const [oauthCallbackDetail, setOauthCallbackDetail] = useState<string | null>(null)
  const oauthCallbackQueryRef = useRef<ReturnType<typeof parseOAuthCallbackQuery> | null>(null)
  const oauthCallbackQueryRouteRef = useRef<string | null>(null)
  const oauthCallbackAttemptRef = useRef<{
    controller: AbortController
    key: string
    promise: Promise<LinuxDoFinalizeResult>
  } | null>(null)
  const oauthCallbackRedirectTimerRef = useRef<number | null>(null)

  useEffect(() => {
    if (!oauthCallbackProvider) {
      oauthCallbackQueryRef.current = null
      oauthCallbackQueryRouteRef.current = null
      oauthCallbackAttemptRef.current?.controller.abort()
      oauthCallbackAttemptRef.current = null
      if (oauthCallbackRedirectTimerRef.current != null) {
        window.clearTimeout(oauthCallbackRedirectTimerRef.current)
        oauthCallbackRedirectTimerRef.current = null
      }
      return
    }

    const routeKey = `oauth:${oauthCallbackProvider}`
    if (oauthCallbackQueryRouteRef.current && oauthCallbackQueryRouteRef.current !== routeKey) {
      oauthCallbackAttemptRef.current?.controller.abort()
      oauthCallbackAttemptRef.current = null
      if (oauthCallbackRedirectTimerRef.current != null) {
        window.clearTimeout(oauthCallbackRedirectTimerRef.current)
        oauthCallbackRedirectTimerRef.current = null
      }
    }
    if (oauthCallbackQueryRouteRef.current !== routeKey || oauthCallbackQueryRef.current == null) {
      oauthCallbackQueryRef.current = parseOAuthCallbackQuery(window.location.search)
      oauthCallbackQueryRouteRef.current = routeKey
    }
    if (window.location.search || window.location.hash) {
      window.history.replaceState(null, '', userConsoleRouteToPath(route))
    }
  }, [oauthCallbackProvider, route])

  const ensureFinalizeAttempt = useCallback((key: string, code: string, state: string) => {
    if (oauthCallbackAttemptRef.current?.key === key) {
      return oauthCallbackAttemptRef.current
    }

    const controller = new AbortController()
    const promise = finalizeLinuxDoAuth(code, state, controller.signal).finally(() => {
      if (oauthCallbackAttemptRef.current?.key === key) {
        oauthCallbackAttemptRef.current = null
      }
    })
    const attempt = { controller, key, promise }
    oauthCallbackAttemptRef.current = attempt
    return attempt
  }, [])

  useEffect(() => {
    if (!oauthCallbackProvider) return

    abortActiveConsoleLoads()
    setLoading(false)
    setError(null)

    if (oauthCallbackProvider !== 'linuxdo') {
      setOauthCallbackState('unsupportedProvider')
      setOauthCallbackDetail(null)
      return
    }

    const query = oauthCallbackQueryRef.current ?? parseOAuthCallbackQuery(window.location.search)
    if (query.error) {
      setOauthCallbackState('providerDenied')
      setOauthCallbackDetail(query.errorDescription ?? query.error)
      return
    }
    if (!query.code || !query.state) {
      setOauthCallbackState('invalidRequest')
      setOauthCallbackDetail(null)
      return
    }

    const attempt = ensureFinalizeAttempt(
      `${oauthCallbackProvider}:${query.code}:${query.state}`,
      query.code,
      query.state,
    )
    let active = true
    let timedOut = false
    const timeout = window.setTimeout(() => {
      timedOut = true
      attempt.controller.abort()
      if (!active) return
      setOauthCallbackState('timeout')
      setOauthCallbackDetail(null)
    }, OAUTH_CALLBACK_FINALIZE_TIMEOUT_MS)

    setOauthCallbackState('connecting')
    setOauthCallbackDetail(null)

    void attempt.promise
      .then((result) => {
        if (!active || timedOut) return
        if (result.outcome === 'success') {
          setOauthCallbackState('success')
          setOauthCallbackDetail(null)
          oauthCallbackRedirectTimerRef.current = window.setTimeout(() => {
            window.location.href = result.redirectTo || '/console'
          }, 720)
          return
        }
        if (result.outcome === 'registration_paused') {
          window.location.href = result.redirectTo || '/registration-paused'
          return
        }
        if (result.outcome === 'invalid_state') {
          setOauthCallbackState('invalidState')
          setOauthCallbackDetail(result.detail)
          return
        }
        if (result.outcome === 'inactive_user') {
          setOauthCallbackState('inactiveUser')
          setOauthCallbackDetail(result.detail)
          return
        }
        if (result.outcome === 'upstream_failure') {
          setOauthCallbackState('upstreamFailure')
          setOauthCallbackDetail(result.detail)
          return
        }
        setOauthCallbackState('serverError')
        setOauthCallbackDetail(result.detail)
      })
      .catch((err) => {
        if (!active) return
        if (timedOut) {
          setOauthCallbackState('timeout')
          setOauthCallbackDetail(null)
          return
        }
        if (attempt.controller.signal.aborted) {
          setOauthCallbackState('timeout')
          setOauthCallbackDetail(null)
          return
        }
        setOauthCallbackState('serverError')
        setOauthCallbackDetail(err instanceof Error ? err.message : null)
      })
      .finally(() => {
        window.clearTimeout(timeout)
      })

    return () => {
      active = false
      window.clearTimeout(timeout)
      if (oauthCallbackRedirectTimerRef.current != null) {
        window.clearTimeout(oauthCallbackRedirectTimerRef.current)
        oauthCallbackRedirectTimerRef.current = null
      }
    }
  }, [abortActiveConsoleLoads, ensureFinalizeAttempt, oauthCallbackProvider, setError, setLoading])

  const providerLabel = route.name === 'oauthCallback'
    ? resolveOAuthCallbackProviderLabel(route.provider, providers)
    : providers.linuxdo
  const model = useMemo(
    () => resolveOAuthCallbackPanelModel({
      state: oauthCallbackState,
      providerLabel,
      text,
      detail: oauthCallbackDetail,
    }),
    [oauthCallbackDetail, oauthCallbackState, providerLabel, text],
  )
  const restartAuth = useCallback(() => {
    window.location.href = USER_CONSOLE_LOGIN_START_PATH
  }, [])

  return {
    isOAuthCallbackRoute,
    model,
    restartAuth,
  }
}
