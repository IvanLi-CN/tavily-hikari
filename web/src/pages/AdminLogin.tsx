import { useEffect, useMemo, useState } from 'react'

import { fetchProfile } from '../api'
import LanguageSwitcher from '../components/LanguageSwitcher'
import ThemeToggle from '../components/ThemeToggle'
import { Button } from '../components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../components/ui/card'
import { Input } from '../components/ui/input'
import { useTranslate } from '../i18n'

type LoginState = 'checking' | 'ready' | 'submitting'

function AdminLogin(): JSX.Element {
  const strings = useTranslate()
  const ui = strings.public.adminLogin

  const [password, setPassword] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [state, setState] = useState<LoginState>('checking')
  const [builtinEnabled, setBuiltinEnabled] = useState<boolean | null>(null)

  useEffect(() => {
    let alive = true
    fetchProfile()
      .then((profile) => {
        if (!alive) return
        setBuiltinEnabled(profile.builtinAuthEnabled ?? false)
        if (profile.isAdmin) {
          window.location.href = '/admin'
          return
        }
      })
      .catch(() => {
        if (!alive) return
        setBuiltinEnabled(null)
      })
      .finally(() => {
        if (!alive) return
        setState('ready')
      })
    return () => {
      alive = false
    }
  }, [])

  const canSubmit = useMemo(() => state !== 'submitting' && password.trim().length > 0, [password, state])

  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    if (!canSubmit) return

    setError(null)
    setState('submitting')
    try {
      const res = await fetch('/api/admin/login', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ password: password.trim() }),
      })

      if (res.status === 404) {
        setError(ui.errors.disabled)
        return
      }
      if (res.status === 401) {
        setError(ui.errors.invalid)
        return
      }
      if (!res.ok) {
        const msg = await res.text().catch(() => '')
        setError(msg || ui.errors.generic)
        return
      }
      window.location.href = '/admin'
    } catch {
      setError(ui.errors.generic)
    } finally {
      setState('ready')
    }
  }

  return (
    <div className="auth-shell min-h-screen bg-background text-foreground">
      <div className="mx-auto flex w-full max-w-4xl flex-col gap-6 px-6 py-10">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="space-y-1">
            <h1 className="auth-title text-3xl font-semibold tracking-tight">{ui.title}</h1>
            <p className="auth-subtitle text-sm text-muted-foreground">{ui.description}</p>
          </div>
          <div className="flex items-center gap-2">
            <ThemeToggle />
            <LanguageSwitcher />
          </div>
        </div>

        <Card className="auth-card border-border/80 bg-card/90 backdrop-blur">
          <CardHeader>
            <CardTitle>{ui.title}</CardTitle>
            <CardDescription>{ui.description}</CardDescription>
          </CardHeader>
          <CardContent className="space-y-5">
            {builtinEnabled === false ? (
              <div className="rounded-lg border border-warning/35 bg-warning/10 p-3 text-sm text-warning">
                {ui.hints.disabled}
              </div>
            ) : null}

            <form onSubmit={submit} className="grid gap-4">
              <label className="grid w-full gap-2 text-sm font-medium" htmlFor="admin-password-input">
                <span>{ui.password.label}</span>
                <Input
                  id="admin-password-input"
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  placeholder={ui.password.placeholder}
                  autoComplete="current-password"
                  disabled={state !== 'ready'}
                />
              </label>

              {error ? (
                <div className="rounded-lg border border-destructive/35 bg-destructive/10 p-3 text-sm text-destructive">
                  {error}
                </div>
              ) : null}

              <div className="flex items-center justify-between gap-3">
                <a href="/" className="text-sm text-primary underline-offset-4 hover:underline">
                  {ui.backHome}
                </a>
                <Button type="submit" disabled={!canSubmit}>
                  {state === 'submitting' ? ui.submit.loading : ui.submit.label}
                </Button>
              </div>
            </form>
          </CardContent>
        </Card>

        {state === 'checking' ? <div className="text-center text-sm text-muted-foreground">{ui.hints.checking}</div> : null}
      </div>
    </div>
  )
}

export default AdminLogin
