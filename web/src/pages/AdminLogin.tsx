import type { FormEvent } from 'react'
import { useEffect, useMemo, useState } from 'react'
import { KeyRound } from 'lucide-react'

import {
  fetchProfile,
  loginWithAdminPasskey,
  registerAdminPasskeyWithResetToken,
  requestJson,
} from '../api'
import { isDemoMode } from '../api/demo'
import BrandLockup from '../components/BrandLockup'
import LanguageSwitcher from '../components/LanguageSwitcher'
import OfflineStatusBanner from '../components/OfflineStatusBanner'
import ThemeToggle from '../components/ThemeToggle'
import { Button } from '../components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '../components/ui/card'
import { Input } from '../components/ui/input'
import { useTranslate } from '../i18n'
import { useOfflineState } from '../pwa/useOfflineState'

type LoginState = 'checking' | 'ready' | 'submitting'
type SubmitAction = 'password' | 'passkey' | 'reset'

function AdminLogin(): JSX.Element {
  const strings = useTranslate()
  const ui = strings.public.adminLogin
  const offline = useOfflineState()

  const [password, setPassword] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [state, setState] = useState<LoginState>('checking')
  const [submittingAction, setSubmittingAction] = useState<SubmitAction | null>(null)
  const [builtinEnabled, setBuiltinEnabled] = useState<boolean | null>(null)
  const [passkeyEnabled, setPasskeyEnabled] = useState(false)
  const [totpRequired, setTotpRequired] = useState(false)
  const [totpCode, setTotpCode] = useState('')
  const [profileUnavailable, setProfileUnavailable] = useState(false)
  const resetToken = useMemo(() => {
    if (typeof window === 'undefined') return ''
    return new URLSearchParams(window.location.search).get('adminPasskeyResetToken')?.trim() ?? ''
  }, [])
  const resetRegistered = useMemo(() => {
    if (typeof window === 'undefined') return false
    return new URLSearchParams(window.location.search).get('adminPasskeyRegistered') === '1'
  }, [])
  const resetMode = resetToken.length > 0

  useEffect(() => {
    let alive = true
    fetchProfile()
      .then((profile) => {
        if (!alive) return
        setBuiltinEnabled(profile.builtinAuthEnabled ?? false)
        setPasskeyEnabled(profile.passkeyAuthEnabled ?? false)
        setTotpRequired(profile.adminLoginTotpRequired ?? false)
        setProfileUnavailable(false)
        if (profile.isAdmin && !isDemoMode()) {
          window.location.href = '/admin'
          return
        }
      })
      .catch(() => {
        if (!alive) return
        setBuiltinEnabled(null)
        setPasskeyEnabled(false)
        setTotpRequired(false)
        setProfileUnavailable(true)
      })
      .finally(() => {
        if (!alive) return
        setState('ready')
      })
    return () => {
      alive = false
    }
  }, [])

  const showPasswordForm = !resetMode && builtinEnabled !== false
  const showPasskeyLogin = !resetMode && passkeyEnabled
  const showTotpInput = totpRequired && (showPasswordForm || showPasskeyLogin)
  const noLoginMethods = !resetMode && builtinEnabled === false && !passkeyEnabled

  const canSubmit = useMemo(
    () => showPasswordForm && state === 'ready' && password.trim().length > 0 && (!totpRequired || totpCode.length === 6),
    [password, showPasswordForm, state, totpCode.length, totpRequired],
  )
  const canUsePasskey = showPasskeyLogin && state === 'ready' && !offline.isOffline && (!totpRequired || totpCode.length === 6)
  const canRegisterResetPasskey = resetMode && state === 'ready' && !offline.isOffline

  const finishWithErrorHandling = async (
    submitAction: SubmitAction,
    action: () => Promise<unknown>,
    onSuccess: () => void = () => {
      window.location.href = '/admin'
    },
  ) => {
    setError(null)
    setState('submitting')
    setSubmittingAction(submitAction)
    try {
      await action()
      onSuccess()
    } catch (err) {
      const status = typeof err === 'object' && err && 'status' in err ? (err as { status?: unknown }).status : undefined
      if (status === 404) {
        setError(ui.errors.disabled)
      } else if (status === 401) {
        setError(ui.errors.invalid)
      } else if (status === 403) {
        setError(ui.errors.totpInvalid)
      } else if (err instanceof Error && err.message) {
        setError(err.message)
      } else {
        setError(ui.errors.generic)
      }
    } finally {
      setState('ready')
      setSubmittingAction(null)
    }
  }

  const submit = async (event: FormEvent) => {
    event.preventDefault()
    if (!canSubmit) return
    await finishWithErrorHandling('password', () => requestJson('/api/admin/login', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        password: password.trim(),
        ...(totpRequired ? { totpCode } : {}),
      }),
    }))
  }

  const submitPasskey = async () => {
    if (!canUsePasskey) return
    await finishWithErrorHandling('passkey', () => (
      isDemoMode()
        ? Promise.resolve({ ok: true })
        : loginWithAdminPasskey(totpRequired ? totpCode : undefined)
    ))
  }

  const submitResetRegistration = async () => {
    if (!canRegisterResetPasskey) return
    await finishWithErrorHandling('reset', () => (
      isDemoMode()
        ? Promise.resolve({ ok: true })
        : registerAdminPasskeyWithResetToken(resetToken, 'Admin passkey')
    ), () => {
      window.location.href = '/login?adminPasskeyRegistered=1'
    })
  }

  return (
    <div className="auth-shell min-h-screen bg-background text-foreground">
      <div className="auth-page-frame mx-auto flex w-full max-w-6xl flex-col gap-8 px-6 py-10 lg:px-10 lg:py-14">
        <div className="auth-page-header flex flex-wrap items-center justify-between gap-4">
          <div className="space-y-1">
            <BrandLockup
              title="Tavily Hikari"
              className="auth-brand-lockup"
              markClassName="auth-brand-mark"
            />
            <h1 className="auth-title text-4xl font-semibold tracking-tight">{ui.title}</h1>
            <p className="auth-subtitle text-base text-muted-foreground">{ui.description}</p>
          </div>
          <div className="flex items-center gap-2">
            <ThemeToggle />
            <LanguageSwitcher />
          </div>
        </div>

        {offline.isOffline ? (
          <OfflineStatusBanner
            title="Offline shell loaded"
            description="Admin sign-in needs the network. Reconnect before submitting your password."
          />
        ) : null}

        <Card className="auth-card border-border/80 bg-card/90 backdrop-blur">
          <CardHeader>
            <CardTitle>{ui.credentialsTitle}</CardTitle>
          </CardHeader>
          <CardContent className="auth-card-content space-y-6">
            {profileUnavailable ? (
              <div className="rounded-lg border border-warning/35 bg-warning/10 p-3 text-sm text-warning">
                {ui.hints.profileUnavailable}
              </div>
            ) : null}

            {noLoginMethods ? (
              <div className="rounded-lg border border-warning/35 bg-warning/10 p-3 text-sm text-warning">
                {ui.hints.disabled}
              </div>
            ) : null}

            {resetMode ? (
              <div className="rounded-lg border border-primary/25 bg-primary/10 p-3 text-sm text-primary">
                {ui.hints.resetEnrollment}
              </div>
            ) : null}

            {resetRegistered && !resetMode ? (
              <div className="rounded-lg border border-primary/25 bg-primary/10 p-3 text-sm text-primary">
                {ui.hints.resetRegistered}
              </div>
            ) : null}

            <form onSubmit={submit} className="auth-password-form grid gap-5">
              {resetMode ? (
                <>
                  <Button
                    type="button"
                    className="auth-passkey-button w-full gap-2"
                    disabled={!canRegisterResetPasskey}
                    onClick={submitResetRegistration}
                  >
                    <KeyRound className="h-4 w-4" aria-hidden="true" />
                    {submittingAction === 'reset' ? ui.passkey.registering : ui.passkey.register}
                  </Button>
                  <a href="/" className="auth-back-link text-sm text-primary underline-offset-4 hover:underline">
                    {ui.backHome}
                  </a>
                </>
              ) : null}

              {showTotpInput ? (
                <label className="auth-password-label grid w-full gap-2 text-base font-medium" htmlFor="admin-totp-code-input">
                  <span>{ui.totp.label}</span>
                  <Input
                    id="admin-totp-code-input"
                    className="auth-password-input"
                    type="text"
                    value={totpCode}
                    onChange={(e) => setTotpCode(e.target.value.replace(/\D/g, '').slice(0, 6))}
                    placeholder={ui.totp.placeholder}
                    aria-label={ui.totp.label}
                    autoComplete="one-time-code"
                    inputMode="numeric"
                    pattern="[0-9]*"
                    maxLength={6}
                    disabled={state !== 'ready'}
                  />
                  <span className="text-sm font-medium text-muted-foreground">{ui.totp.hint}</span>
                </label>
              ) : null}

              {showPasskeyLogin ? (
                <Button type="button" className="auth-passkey-button w-full gap-2" disabled={!canUsePasskey} onClick={submitPasskey}>
                  <KeyRound className="h-4 w-4" aria-hidden="true" />
                  {submittingAction === 'passkey' ? ui.passkey.signingIn : ui.passkey.signIn}
                </Button>
              ) : null}

              {showPasskeyLogin && showPasswordForm ? (
                <div className="auth-method-divider" role="separator">
                  <span>{ui.passkey.orPassword}</span>
                </div>
              ) : null}

              {showPasswordForm ? (
                <>
                  <label className="auth-password-label grid w-full gap-2 text-base font-medium" htmlFor="admin-password-input">
                    <span>{ui.password.label}</span>
                    <Input
                      id="admin-password-input"
                      className="auth-password-input"
                      type="password"
                      value={password}
                      onChange={(e) => setPassword(e.target.value)}
                      placeholder={ui.password.placeholder}
                      aria-label={ui.password.label}
                      autoComplete="current-password"
                      disabled={state !== 'ready'}
                    />
                  </label>

                  <div className="auth-form-actions flex items-center justify-between gap-4">
                    <a href="/" className="auth-back-link text-sm text-primary underline-offset-4 hover:underline">
                      {ui.backHome}
                    </a>
                    <Button type="submit" className="auth-submit-button" disabled={!canSubmit}>
                      {submittingAction === 'password' ? ui.submit.loading : ui.submit.label}
                    </Button>
                  </div>
                </>
              ) : !resetMode ? (
                <a href="/" className="auth-back-link text-sm text-primary underline-offset-4 hover:underline">
                  {ui.backHome}
                </a>
              ) : null}
            </form>

            {error ? (
              <div role="alert" className="rounded-lg border border-destructive/35 bg-destructive/10 p-3 text-sm text-destructive">
                {error}
              </div>
            ) : null}
          </CardContent>
        </Card>

        {state === 'checking' ? <div className="text-center text-sm text-muted-foreground">{ui.hints.checking}</div> : null}
      </div>
    </div>
  )
}

export default AdminLogin
