import { useEffect, useMemo, useState } from 'react'
import LanguageSwitcher from '../components/LanguageSwitcher'
import { fetchProfile } from '../api'
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
    <div className="min-h-screen bg-base-200">
      <div className="max-w-3xl mx-auto px-6 py-10">
        <div className="flex items-center justify-between">
          <div className="space-y-1">
            <h1 className="text-2xl font-semibold tracking-tight">{ui.title}</h1>
            <p className="text-sm text-base-content/70">{ui.description}</p>
          </div>
          <LanguageSwitcher />
        </div>

        <div className="mt-8 grid gap-6">
          <div className="card bg-base-100 shadow-xl">
            <div className="card-body gap-5">
              {builtinEnabled === false && (
                <div className="alert alert-warning">
                  <span>{ui.hints.disabled}</span>
                </div>
              )}

              <form onSubmit={submit} className="grid gap-4">
                <label className="form-control w-full">
                  <div className="label">
                    <span className="label-text">{ui.password.label}</span>
                  </div>
                  <input
                    type="password"
                    className="input input-bordered w-full"
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                    placeholder={ui.password.placeholder}
                    autoComplete="current-password"
                    disabled={state !== 'ready'}
                  />
                </label>

                {error && (
                  <div className="alert alert-error">
                    <span>{error}</span>
                  </div>
                )}

                <div className="flex items-center justify-between gap-3">
                  <a href="/" className="link link-hover text-sm">
                    {ui.backHome}
                  </a>
                  <button type="submit" className="btn btn-primary" disabled={!canSubmit}>
                    {state === 'submitting' ? ui.submit.loading : ui.submit.label}
                  </button>
                </div>
              </form>
            </div>
          </div>

          {state === 'checking' && (
            <div className="text-center text-sm text-base-content/60">{ui.hints.checking}</div>
          )}
        </div>
      </div>
    </div>
  )
}

export default AdminLogin

