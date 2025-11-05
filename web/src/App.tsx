import { Icon } from '@iconify/react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  fetchApiKeys,
  fetchApiKeySecret,
  addApiKey,
  deleteApiKey,
  setKeyStatus,
  fetchProfile,
  fetchRequestLogs,
  fetchSummary,
  fetchVersion,
  type ApiKeyStats,
  type Profile,
  type RequestLog,
  type Summary,
  fetchTokens,
  type AuthToken,
  fetchTokenSecret,
  createToken,
  deleteToken,
  setTokenEnabled,
  updateTokenNote,
  fetchKeyMetrics,
  fetchKeyLogs,
  type KeySummary,
} from './api'

function parseHashForKeyId(): string | null {
  const hash = location.hash || ''
  const m = hash.match(/^#\/keys\/([^\/?#]+)/)
  return m ? decodeURIComponent(m[1]) : null
}

const REFRESH_INTERVAL_MS = 30_000

const numberFormatter = new Intl.NumberFormat('en-US', {
  maximumFractionDigits: 0,
})

const percentageFormatter = new Intl.NumberFormat('en-US', {
  style: 'percent',
  minimumFractionDigits: 0,
  maximumFractionDigits: 1,
})

const dateTimeFormatter = new Intl.DateTimeFormat(undefined, {
  dateStyle: 'medium',
  timeStyle: 'medium',
})

// Time-only formatter for compact "Updated HH:MM:SS"
const timeOnlyFormatter = new Intl.DateTimeFormat(undefined, {
  hour: '2-digit',
  minute: '2-digit',
  second: '2-digit',
  hour12: false,
})

function formatNumber(value: number): string {
  return numberFormatter.format(value)
}

function formatPercent(numerator: number, denominator: number): string {
  if (denominator === 0) return '—'
  return percentageFormatter.format(numerator / denominator)
}

function formatTimestamp(value: number | null): string {
  if (!value) {
    return '—'
  }
  return dateTimeFormatter.format(new Date(value * 1000))
}

function statusClass(status: string): string {
  const normalized = status.toLowerCase()
  if (normalized === 'active' || normalized === 'success') {
    return 'status-badge status-active'
  }
  if (normalized === 'exhausted' || normalized === 'quota_exhausted') {
    return 'status-badge status-exhausted'
  }
  // 'deleted' 由 deleted_at 字段控制，这里仅兜底
  if (normalized === 'deleted') return 'status-badge status-unknown'
  if (normalized === 'error') {
    return 'status-badge status-error'
  }
  return 'status-badge status-unknown'
}

function statusLabel(status: string): string {
  switch (status.toLowerCase()) {
    case 'active':
      return 'Active'
    case 'exhausted':
      return 'Exhausted'
    case 'success':
      return 'Success'
    case 'error':
      return 'Error'
    case 'quota_exhausted':
      return 'Quota Exhausted'
    case 'deleted':
      return 'Deleted'
    default:
      return status
  }
}

function formatErrorMessage(log: RequestLog): string {
  const message = log.error_message?.trim()
  if (message) {
    return message
  }

  const status = log.result_status.toLowerCase()
  if (status === 'quota_exhausted') {
    if (log.http_status != null) {
      return `Quota exhausted (HTTP ${log.http_status})`
    }
    return 'Quota exhausted'
  }

  if (status === 'error') {
    if (log.http_status != null && log.mcp_status != null) {
      return `Request failed (HTTP ${log.http_status}, MCP ${log.mcp_status})`
    }
    if (log.http_status != null) {
      return `Request failed (HTTP ${log.http_status})`
    }
    if (log.mcp_status != null) {
      return `Request failed (MCP ${log.mcp_status})`
    }
    return 'Request failed'
  }

  if (status === 'success') {
    return '—'
  }

  if (log.http_status != null) {
    return `HTTP ${log.http_status}`
  }

  return '—'
}

function App(): JSX.Element {
  const [route, setRoute] = useState<{ name: 'home' } | { name: 'key'; id: string }>(() => {
    const id = parseHashForKeyId()
    return id ? { name: 'key', id } : { name: 'home' }
  })
  const [summary, setSummary] = useState<Summary | null>(null)
  const [keys, setKeys] = useState<ApiKeyStats[]>([])
  const [tokens, setTokens] = useState<AuthToken[]>([])
  const [logs, setLogs] = useState<RequestLog[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const pollingTimerRef = useRef<number | null>(null)
  const [lastUpdated, setLastUpdated] = useState<Date | null>(null)
  const [version, setVersion] = useState<{ backend: string; frontend: string } | null>(null)
  const [profile, setProfile] = useState<Profile | null>(null)
  const secretCacheRef = useRef<Map<string, string>>(new Map())
  const [copyState, setCopyState] = useState<Map<string, 'loading' | 'copied'>>(() => new Map())
  const [expandedLogs, setExpandedLogs] = useState<Set<number>>(() => new Set())
  const [newKey, setNewKey] = useState('')
  const [newTokenNote, setNewTokenNote] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const [deletingId, setDeletingId] = useState<string | null>(null)
  const [togglingId, setTogglingId] = useState<string | null>(null)
  const deleteDialogRef = useRef<HTMLDialogElement | null>(null)
  const [pendingDeleteId, setPendingDeleteId] = useState<string | null>(null)
  const disableDialogRef = useRef<HTMLDialogElement | null>(null)
  const [pendingDisableId, setPendingDisableId] = useState<string | null>(null)
  const tokenDeleteDialogRef = useRef<HTMLDialogElement | null>(null)
  const [pendingTokenDeleteId, setPendingTokenDeleteId] = useState<string | null>(null)
  const tokenNoteDialogRef = useRef<HTMLDialogElement | null>(null)
  const [editingTokenId, setEditingTokenId] = useState<string | null>(null)
  const [editingTokenNote, setEditingTokenNote] = useState('')
  const [savingTokenNote, setSavingTokenNote] = useState(false)
  const [sseConnected, setSseConnected] = useState(false)

  const copyStateKey = useCallback((scope: 'keys' | 'logs' | 'tokens', identifier: string | number) => {
    return `${scope}:${identifier}`
  }, [])

  const updateCopyState = useCallback((key: string, next: 'loading' | 'copied' | null) => {
    setCopyState((previous) => {
      const clone = new Map(previous)
      if (next === null) {
        clone.delete(key)
      } else {
        clone.set(key, next)
      }
      return clone
    })
  }, [])

  const handleCopySecret = useCallback(
    async (id: string, stateKey: string) => {
      updateCopyState(stateKey, 'loading')
      try {
        let secret = secretCacheRef.current.get(id)
        if (!secret) {
          const result = await fetchApiKeySecret(id)
          secret = result.api_key
          secretCacheRef.current.set(id, secret)
        }

        const copyToClipboard = async (value: string) => {
          if (navigator.clipboard?.writeText) {
            await navigator.clipboard.writeText(value)
            return
          }

          const textarea = document.createElement('textarea')
          textarea.value = value
          textarea.style.position = 'fixed'
          textarea.style.opacity = '0'
          textarea.style.left = '-9999px'
          document.body.appendChild(textarea)
          textarea.focus()
          textarea.select()
          document.execCommand('copy')
          document.body.removeChild(textarea)
        }

        await copyToClipboard(secret)
        updateCopyState(stateKey, 'copied')
        window.setTimeout(() => updateCopyState(stateKey, null), 2000)
      } catch (err) {
        console.error(err)
        setError(err instanceof Error ? err.message : 'Failed to copy API key')
        updateCopyState(stateKey, null)
      }
    },
    [setError, updateCopyState],
  )

  const loadData = useCallback(
    async (signal?: AbortSignal) => {
      try {
        const [summaryData, keyData, logData, ver, profileData, tokenData] = await Promise.all([
          fetchSummary(signal),
          fetchApiKeys(signal),
          fetchRequestLogs(50, signal),
          fetchVersion(signal).catch(() => null),
          fetchProfile(signal).catch(() => null),
          fetchTokens(signal).catch(() => []),
        ])

        if (signal?.aborted) {
          return
        }

        setSummary(summaryData)
        setKeys(keyData)
        setLogs(logData)
        setTokens(tokenData)
        setExpandedLogs((previous) => {
          if (!isAdmin || previous.size === 0) {
            return new Set()
          }
          const validIds = new Set(logData.map((item) => item.id))
          const next = new Set<number>()
          for (const id of previous) {
            if (validIds.has(id)) {
              next.add(id)
            }
          }
          return next
        })
        if (ver) setVersion(ver)
        setProfile(profileData ?? null)
        setLastUpdated(new Date())
        setError(null)
      } catch (err) {
        if ((err as Error).name === 'AbortError') {
          return
        }
        setError(err instanceof Error ? err.message : 'Unexpected error occurred')
      } finally {
        if (!(signal?.aborted ?? false)) {
          setLoading(false)
        }
      }
    },
    [],
  )

  useEffect(() => {
    const controller = new AbortController()
    setLoading(true)
    void loadData(controller.signal)
    return () => controller.abort()
  }, [loadData])

  // Automatic fallback polling when SSE is not connected
  useEffect(() => {
    if (sseConnected) {
      if (pollingTimerRef.current != null) {
        window.clearInterval(pollingTimerRef.current)
        pollingTimerRef.current = null
      }
      return
    }
    if (pollingTimerRef.current == null) {
      pollingTimerRef.current = window.setInterval(() => {
        const controller = new AbortController()
        void loadData(controller.signal).finally(() => controller.abort())
      }, REFRESH_INTERVAL_MS) as unknown as number
    }
    return () => {
      if (pollingTimerRef.current != null) {
        window.clearInterval(pollingTimerRef.current)
        pollingTimerRef.current = null
      }
    }
  }, [sseConnected, loadData])

  // Establish SSE connection to receive live dashboard updates
  useEffect(() => {
    let es: EventSource | null = null
    let closed = false

    const connect = () => {
      if (es) {
        try { es.close() } catch {}
        es = null
      }
      es = new EventSource('/api/events')
      es.onopen = () => { setSseConnected(true) }
      es.onerror = () => {
        // Trigger fallback polling; attempt reconnect automatically
        setSseConnected(false)
      }
      es.addEventListener('snapshot', (ev: MessageEvent) => {
        try {
          const data = JSON.parse(ev.data) as { summary: Summary; keys: ApiKeyStats[]; logs: RequestLog[] }
          setSummary(data.summary)
          setKeys(data.keys)
          setLogs(data.logs)
          setExpandedLogs((previous) => {
            // keep expansion only for visible ids
            const valid = new Set(data.logs.map((l) => l.id))
            const next = new Set<number>()
            for (const id of previous) if (valid.has(id)) next.add(id)
            return next
          })
          setLastUpdated(new Date())
          setError(null)
          setLoading(false)
        } catch (e) {
          console.error('SSE parse error', e)
        }
      })
    }

    connect()
    return () => {
      closed = true
      if (es) {
        try { es.close() } catch {}
      }
    }
  }, [])

  useEffect(() => {
    const onHash = () => {
      const id = parseHashForKeyId()
      setRoute(id ? { name: 'key', id } : { name: 'home' })
    }
    window.addEventListener('hashchange', onHash)
    return () => window.removeEventListener('hashchange', onHash)
  }, [])

  const navigateHome = () => {
    history.pushState(null, '', '/')
    setRoute({ name: 'home' })
  }

  const navigateKey = (id: string) => {
    location.hash = `#/keys/${encodeURIComponent(id)}`
    setRoute({ name: 'key', id })
  }

  if (route.name === 'key') {
    return <KeyDetails id={route.id} onBack={navigateHome} isAdmin={profile?.isAdmin ?? false} />
  }

  const handleManualRefresh = () => {
    const controller = new AbortController()
    setLoading(true)
    void loadData(controller.signal).finally(() => controller.abort())
  }

  const metrics = useMemo(() => {
    if (!summary) {
      return []
    }

    const total = summary.total_requests
    return [
      {
        id: 'total',
        label: 'Total Requests',
        value: formatNumber(summary.total_requests),
        subtitle: '—',
      },
      {
        id: 'success',
        label: 'Successful',
        value: formatNumber(summary.success_count),
        subtitle: formatPercent(summary.success_count, total),
      },
      {
        id: 'errors',
        label: 'Errors',
        value: formatNumber(summary.error_count),
        subtitle: formatPercent(summary.error_count, total),
      },
      {
        id: 'quota',
        label: 'Quota Exhausted',
        value: formatNumber(summary.quota_exhausted_count),
        subtitle: formatPercent(summary.quota_exhausted_count, total),
      },
      {
        id: 'keys',
        label: 'Active Keys',
        value: `${formatNumber(summary.active_keys)} / ${formatNumber(summary.active_keys + summary.exhausted_keys)}`,
        subtitle: summary.exhausted_keys === 0 ? 'All keys available' : `${formatNumber(summary.exhausted_keys)} exhausted`,
      },
    ]
  }, [summary])

  const dedupedKeys = useMemo(() => {
    const map = new Map<string, ApiKeyStats>()
    for (const item of keys) {
      if (item.deleted_at) continue // hide soft-deleted keys
      map.set(item.id, item)
    }
    return Array.from(map.values())
  }, [keys])

  const sortedKeys = useMemo(() => {
    return [...dedupedKeys].sort((a, b) => {
      if (a.status !== b.status) {
        return a.status === 'active' ? -1 : 1
      }
      const left = a.last_used_at ?? 0
      const right = b.last_used_at ?? 0
      return right - left
    })
  }, [dedupedKeys])

  const displayName = profile?.displayName ?? null
  const isAdmin = profile?.isAdmin ?? false

  useEffect(() => {
    if (!isAdmin && expandedLogs.size > 0) {
      setExpandedLogs(new Set())
    }
  }, [isAdmin, expandedLogs])

  const toggleLogExpansion = useCallback(
    (id: number) => {
      if (!isAdmin) return
      setExpandedLogs((previous) => {
        const next = new Set(previous)
        if (next.has(id)) {
          next.delete(id)
        } else {
          next.add(id)
        }
        return next
      })
    },
    [isAdmin],
  )

  const handleAddKey = async () => {
    const value = newKey.trim()
    if (!value) return
    setSubmitting(true)
    try {
      await addApiKey(value)
      setNewKey('')
      const controller = new AbortController()
      setLoading(true)
      await loadData(controller.signal)
      controller.abort()
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : 'Failed to add API key')
    } finally {
      setSubmitting(false)
    }
  }

  const handleAddToken = async () => {
    const note = newTokenNote.trim()
    setSubmitting(true)
    try {
      const { token } = await createToken(note || undefined)
      setNewTokenNote('')
      try { await navigator.clipboard?.writeText(token) } catch {}
      const controller = new AbortController()
      setLoading(true)
      await loadData(controller.signal)
      controller.abort()
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : 'Failed to create token')
    } finally {
      setSubmitting(false)
    }
  }

  const handleCopyToken = async (id: string, stateKey: string) => {
    updateCopyState(stateKey, 'loading')
    try {
      const { token } = await fetchTokenSecret(id)
      await navigator.clipboard?.writeText(token)
      updateCopyState(stateKey, 'copied')
      window.setTimeout(() => updateCopyState(stateKey, null), 2000)
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : 'Failed to copy token')
      updateCopyState(stateKey, null)
    }
  }

  const toggleToken = async (id: string, enabled: boolean) => {
    setTogglingId(id)
    try {
      await setTokenEnabled(id, !enabled)
      const controller = new AbortController()
      setLoading(true)
      await loadData(controller.signal)
      controller.abort()
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : 'Failed to update token status')
    } finally {
      setTogglingId(null)
    }
  }

  const openTokenDeleteConfirm = (id: string) => {
    if (!id) return
    setPendingTokenDeleteId(id)
    window.requestAnimationFrame(() => tokenDeleteDialogRef.current?.showModal())
  }

  const confirmTokenDelete = async () => {
    if (!pendingTokenDeleteId) return
    const id = pendingTokenDeleteId
    setDeletingId(id)
    try {
      await deleteToken(id)
      tokenDeleteDialogRef.current?.close()
      setPendingTokenDeleteId(null)
      const controller = new AbortController()
      setLoading(true)
      await loadData(controller.signal)
      controller.abort()
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : 'Failed to delete token')
    } finally {
      setDeletingId(null)
    }
  }

  const cancelTokenDelete = () => {
    tokenDeleteDialogRef.current?.close()
    setPendingTokenDeleteId(null)
  }

  const openTokenNoteEdit = (id: string, current: string | null) => {
    setEditingTokenId(id)
    setEditingTokenNote(current ?? '')
    window.requestAnimationFrame(() => tokenNoteDialogRef.current?.showModal())
  }

  const saveTokenNote = async () => {
    if (!editingTokenId) return
    setSavingTokenNote(true)
    try {
      await updateTokenNote(editingTokenId, editingTokenNote)
      tokenNoteDialogRef.current?.close()
      setEditingTokenId(null)
      setEditingTokenNote('')
      const controller = new AbortController()
      setLoading(true)
      await loadData(controller.signal)
      controller.abort()
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : 'Failed to update token note')
    } finally {
      setSavingTokenNote(false)
    }
  }

  const cancelTokenNote = () => {
    tokenNoteDialogRef.current?.close()
    setEditingTokenId(null)
    setEditingTokenNote('')
  }

  const openDeleteConfirm = (id: string) => {
    if (!id) return
    setPendingDeleteId(id)
    window.requestAnimationFrame(() => deleteDialogRef.current?.showModal())
  }

  const confirmDelete = async () => {
    if (!pendingDeleteId) return
    const id = pendingDeleteId
    setDeletingId(id)
    try {
      await deleteApiKey(id)
      deleteDialogRef.current?.close()
      setPendingDeleteId(null)
      const controller = new AbortController()
      setLoading(true)
      await loadData(controller.signal)
      controller.abort()
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : 'Failed to delete API key')
    } finally {
      setDeletingId(null)
    }
  }

  const cancelDelete = () => {
    deleteDialogRef.current?.close()
    setPendingDeleteId(null)
  }

  const handleToggleDisable = async (id: string, toDisabled: boolean) => {
    if (!id) return
    setTogglingId(id)
    try {
      await setKeyStatus(id, toDisabled ? 'disabled' : 'active')
      const controller = new AbortController()
      setLoading(true)
      await loadData(controller.signal)
      controller.abort()
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : 'Failed to update key status')
    } finally {
      setTogglingId(null)
    }
  }

  // DaisyUI disable confirm flow
  const openDisableConfirm = (id: string) => {
    if (!id) return
    setPendingDisableId(id)
    window.requestAnimationFrame(() => disableDialogRef.current?.showModal())
  }

  const confirmDisable = async () => {
    if (!pendingDisableId) return
    const id = pendingDisableId
    await handleToggleDisable(id, true)
    disableDialogRef.current?.close()
    setPendingDisableId(null)
  }

  const cancelDisable = () => {
    disableDialogRef.current?.close()
    setPendingDisableId(null)
  }

  return (
    <>
    <main className="app-shell">
      <section className="surface app-header">
        <div className="title-group">
          <h1>Tavily Hikari Overview</h1>
          <p>Monitor API key allocation, quota health, and recent proxy activity.</p>
        </div>
        <div className="header-right">
          {displayName && (
            <div className={`user-badge${isAdmin ? ' user-badge-admin' : ''}`}>
              {isAdmin && <Icon icon="mdi:crown-outline" className="user-badge-icon" aria-hidden="true" />}
              <span>{displayName}</span>
            </div>
          )}
          <div className="controls">
            {lastUpdated && (
              <span className="panel-description updated-time" style={{ marginRight: 8 }}>
                Updated {timeOnlyFormatter.format(lastUpdated)}
              </span>
            )}
            <button
              type="button"
              className="button button-primary"
              onClick={handleManualRefresh}
              disabled={loading}
            >
              {loading ? 'Refreshing…' : 'Refresh Now'}
            </button>
          </div>
        </div>
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>Access Tokens</h2>
            <p className="panel-description">Auth for /mcp. Format th-xxxx-xxxxxxxxxxxx</p>
          </div>
          {isAdmin && (
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <input
                type="text"
                placeholder="Note (optional)"
                value={newTokenNote}
                onChange={(e) => setNewTokenNote(e.target.value)}
                style={{ padding: '8px 12px', borderRadius: 10, border: '1px solid rgba(15, 23, 42, 0.16)', minWidth: 240 }}
                aria-label="Token note"
              />
              <button type="button" className="button button-primary" onClick={() => void handleAddToken()} disabled={submitting}>
                {submitting ? 'Creating…' : 'New Token'}
              </button>
            </div>
          )}
        </div>
        <div className="table-wrapper">
          {tokens.length === 0 ? (
            <div className="empty-state">{loading ? 'Loading tokens…' : 'No tokens yet.'}</div>
          ) : (
            <table>
              <thead>
                <tr>
                  <th>ID</th>
                  <th>Note</th>
                  <th>Usage</th>
                  <th>Last Used</th>
                  {isAdmin && <th>Actions</th>}
                </tr>
              </thead>
              <tbody>
                {tokens.map((t) => {
                  const stateKey = copyStateKey('tokens', t.id)
                  const state = copyState.get(stateKey)
                  return (
                    <tr key={t.id}>
                      <td>
                        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                          <code>{t.id}</code>
                          <span
                            className="token-status-slot"
                            aria-hidden={t.enabled ? true : undefined}
                            title={t.enabled ? undefined : 'Disabled'}
                          >
                            {!t.enabled && (
                              <Icon
                                className="token-status-icon"
                                icon="mdi:pause-circle-outline"
                                width={14}
                                height={14}
                                aria-label="Disabled token"
                              />
                            )}
                          </span>
                        </div>
                      </td>
                      <td>{t.note || '—'}</td>
                      <td>{formatNumber(t.total_requests)}</td>
                      <td>{formatTimestamp(t.last_used_at)}</td>
                      {isAdmin && (
                        <td>
                          <div style={{ display: 'flex', gap: 8 }}>
                            <button
                              type="button"
                              className={`icon-button${state === 'copied' ? ' icon-button-success' : ''}${state === 'loading' ? ' icon-button-loading' : ''}`}
                              title="Copy full token"
                              aria-label="Copy full token"
                              onClick={() => void handleCopyToken(t.id, stateKey)}
                              disabled={state === 'loading'}
                            >
                              <Icon icon={state === 'copied' ? 'mdi:check' : 'mdi:content-copy'} width={18} height={18} />
                            </button>
                            <button
                              type="button"
                              className="icon-button"
                              title={t.enabled ? 'Disable token' : 'Enable token'}
                              aria-label={t.enabled ? 'Disable token' : 'Enable token'}
                              onClick={() => void toggleToken(t.id, t.enabled)}
                              disabled={togglingId === t.id}
                            >
                              <Icon icon={t.enabled ? 'mdi:pause-circle-outline' : 'mdi:play-circle-outline'} width={18} height={18} />
                            </button>
                            <button
                              type="button"
                              className="icon-button"
                              title="Edit note"
                              aria-label="Edit note"
                              onClick={() => openTokenNoteEdit(t.id, t.note)}
                            >
                              <Icon icon="mdi:pencil-outline" width={18} height={18} />
                            </button>
                            <button
                              type="button"
                              className="icon-button icon-button-danger"
                              title="Delete token"
                              aria-label="Delete token"
                              onClick={() => openTokenDeleteConfirm(t.id)}
                              disabled={deletingId === t.id}
                            >
                              <Icon icon={deletingId === t.id ? 'mdi:progress-helper' : 'mdi:trash-outline'} width={18} height={18} />
                            </button>
                          </div>
                        </td>
                      )}
                    </tr>
                  )
                })}
              </tbody>
            </table>
          )}
        </div>
      </section>
      {error && <div className="surface error-banner">{error}</div>}

      <section className="surface metrics-grid">
        {metrics.length === 0 && loading ? (
          <div className="empty-state" style={{ gridColumn: '1 / -1' }}>
            Loading latest metrics…
          </div>
        ) : (
          metrics.map((metric) => (
            <div key={metric.id} className="metric-card">
              <h3>{metric.label}</h3>
              <div className="metric-value">{metric.value}</div>
              <div className="metric-subtitle">{metric.subtitle}</div>
            </div>
          ))
        )}
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>API Keys</h2>
            <p className="panel-description">Status, usage, and recent success rates per Tavily API key.</p>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
            {isAdmin && (
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <input
                  type="text"
                  placeholder="New Tavily API Key"
                  value={newKey}
                  onChange={(e) => setNewKey(e.target.value)}
                  style={{
                    padding: '8px 12px',
                    borderRadius: 10,
                    border: '1px solid rgba(15, 23, 42, 0.16)',
                    minWidth: 240,
                  }}
                />
                <button
                  type="button"
                  className="button button-primary"
                  onClick={() => void handleAddKey()}
                  disabled={submitting || !newKey.trim()}
                >
                  {submitting ? 'Adding…' : 'Add Key'}
                </button>
              </div>
            )}
          </div>
        </div>
        <div className="table-wrapper">
          {sortedKeys.length === 0 ? (
            <div className="empty-state">{loading ? 'Loading key statistics…' : 'No key data recorded yet.'}</div>
          ) : (
            <table>
              <thead>
                <tr>
                  <th>Key ID</th>
                  <th>Status</th>
                  <th>Total</th>
                  <th>Success</th>
                  <th>Errors</th>
                  <th>Quota Exhausted</th>
                  <th>Success Rate</th>
                  <th>Last Used</th>
                  <th>Status Changed</th>
                  {isAdmin && <th>Actions</th>}
                </tr>
              </thead>
              <tbody>
                {sortedKeys.map((item) => {
                  const total = item.total_requests || 0
                  const stateKey = copyStateKey('keys', item.id)
                  const state = copyState.get(stateKey)
                  return (
                    <tr key={item.id}>
                      <td>
                        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                          <button type="button" className="link-button" onClick={() => navigateKey(item.id)} title="Open details">
                            <code>{item.id}</code>
                          </button>
                          {isAdmin && (
                            <button
                              type="button"
                              className={`icon-button${state === 'copied' ? ' icon-button-success' : ''}${state === 'loading' ? ' icon-button-loading' : ''}`}
                              title="复制原始 API key"
                              aria-label="复制原始 API key"
                              onClick={() => void handleCopySecret(item.id, stateKey)}
                              disabled={state === 'loading'}
                            >
                              <Icon icon={state === 'copied' ? 'mdi:check' : 'mdi:content-copy'} width={18} height={18} />
                            </button>
                          )}
                        </div>
                      </td>
                      <td>
                        <span className={statusClass(item.status)}>{statusLabel(item.status)}</span>
                      </td>
                      <td>{formatNumber(total)}</td>
                      <td>{formatNumber(item.success_count)}</td>
                      <td>{formatNumber(item.error_count)}</td>
                      <td>{formatNumber(item.quota_exhausted_count)}</td>
                      <td>{formatPercent(item.success_count, total)}</td>
                      <td>{formatTimestamp(item.last_used_at)}</td>
                      <td>{formatTimestamp(item.status_changed_at)}</td>
                      {isAdmin && (
                        <td>
                          <div style={{ display: 'flex', gap: 8 }}>
                            {item.status === 'disabled' ? (
                              <button
                                type="button"
                                className="icon-button"
                                title="Enable key"
                                aria-label="Enable key"
                                onClick={() => void handleToggleDisable(item.id, false)}
                                disabled={togglingId === item.id}
                              >
                                <Icon icon={togglingId === item.id ? 'mdi:progress-helper' : 'mdi:play-circle-outline'} width={18} height={18} />
                              </button>
                            ) : (
                              <button
                                type="button"
                                className="icon-button"
                                title="Disable key"
                                aria-label="Disable key"
                                onClick={() => openDisableConfirm(item.id)}
                                disabled={togglingId === item.id}
                              >
                                <Icon icon={togglingId === item.id ? 'mdi:progress-helper' : 'mdi:pause-circle-outline'} width={18} height={18} />
                              </button>
                            )}
                            <button
                              type="button"
                              className="icon-button icon-button-danger"
                              title="Remove key"
                              aria-label="Remove key"
                              onClick={() => openDeleteConfirm(item.id)}
                              disabled={deletingId === item.id}
                            >
                              <Icon icon={deletingId === item.id ? 'mdi:progress-helper' : 'mdi:trash-outline'} width={18} height={18} />
                            </button>
                            <button
                              type="button"
                              className="icon-button"
                              title="Details"
                              aria-label="Details"
                              onClick={() => navigateKey(item.id)}
                            >
                              <Icon icon="mdi:eye-outline" width={18} height={18} />
                            </button>
                          </div>
                        </td>
                      )}
                    </tr>
                  )
                })}
              </tbody>
            </table>
          )}
        </div>
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>Recent Requests</h2>
            <p className="panel-description">Up to the latest 50 invocations handled by the proxy.</p>
          </div>
        </div>
        <div className="table-wrapper">
          {logs.length === 0 ? (
            <div className="empty-state">{loading ? 'Collecting recent requests…' : 'No request logs captured yet.'}</div>
          ) : (
            <table>
              <thead>
                <tr>
                  <th>Time</th>
                  <th>Key</th>
                  <th>HTTP Status</th>
                  <th>MCP Status</th>
                  <th>Result</th>
                  <th>Error</th>
                </tr>
              </thead>
              <tbody>
                {logs.map((log) => {
                  const stateKey = copyStateKey('logs', log.id)
                  const state = copyState.get(stateKey)
                  return (
                    <LogRow
                      key={log.id}
                      log={log}
                      isAdmin={isAdmin}
                      onCopy={() => void handleCopySecret(log.key_id, stateKey)}
                      copyState={state}
                      expanded={expandedLogs.has(log.id)}
                      onToggle={toggleLogExpansion}
                    />
                  )
                })}
              </tbody>
            </table>
          )}
        </div>
      </section>

      <div className="app-footer">
        <span>Tavily Hikari Proxy Dashboard</span>
        <span style={{ marginLeft: 12, opacity: 0.85 }}>
          {/* GitHub repository link with Iconify icon */}
          <a
            href="https://github.com/IvanLi-CN/tavily-hikari"
            className="footer-link"
            target="_blank"
            rel="noreferrer"
            aria-label="Open GitHub repository"
          >
            <Icon icon="mdi:github" width={18} height={18} />
            <span style={{ marginLeft: 6 }}>GitHub</span>
          </a>
        </span>
        <span style={{ marginLeft: 12, opacity: 0.85 }}>
          {version ? (
            (() => {
              const raw = version.backend || ''
              const clean = raw.replace(/-.+$/, '')
              const tag = clean.startsWith('v') ? clean : `v${clean}`
              const href = `https://github.com/IvanLi-CN/tavily-hikari/releases/tag/${tag}`
              return (
                <>
                  {'· '}
                  <a href={href} className="footer-link" target="_blank" rel="noreferrer">
                    {`v${raw}`}
                  </a>
                </>
              )
            })()
          ) : (
            '· Loading version…'
          )}
        </span>
      </div>
    </main>
    {/* Disable Confirmation (daisyUI modal) */}
    <dialog id="confirm_disable_modal" ref={disableDialogRef} className="modal">
      <div className="modal-box">
        <h3 className="font-bold text-lg" style={{ marginTop: 0 }}>Disable API Key</h3>
        <p className="py-2">This will stop using the key until you enable it again. No data will be removed.</p>
        <div className="modal-action">
          <form method="dialog" onSubmit={(e) => e.preventDefault()} style={{ display: 'flex', gap: 8 }}>
            <button type="button" className="btn" onClick={cancelDisable}>Cancel</button>
            <button type="button" className="btn" onClick={() => void confirmDisable()} disabled={!!togglingId}>Disable</button>
          </form>
        </div>
      </div>
    </dialog>

    {/* Delete Confirmation (daisyUI modal) */}
    <dialog id="confirm_delete_modal" ref={deleteDialogRef} className="modal">
      <div className="modal-box">
        <h3 className="font-bold text-lg" style={{ marginTop: 0 }}>Remove API Key</h3>
        <p className="py-2">This will mark the key as Deleted. You can restore it later by re-adding the same secret.</p>
        <div className="modal-action">
          <form method="dialog" onSubmit={(e) => e.preventDefault()} style={{ display: 'flex', gap: 8 }}>
            <button type="button" className="btn" onClick={cancelDelete}>Cancel</button>
            <button type="button" className="btn btn-error" onClick={() => void confirmDelete()} disabled={!!deletingId}>Remove</button>
          </form>
        </div>
      </div>
    </dialog>
    {/* Token Delete Confirmation */}
    <dialog id="confirm_token_delete_modal" ref={tokenDeleteDialogRef} className="modal">
      <div className="modal-box">
        <h3 className="font-bold text-lg" style={{ marginTop: 0 }}>Delete Token</h3>
        <p className="py-2">This will permanently remove the access token. Clients using it will receive 401.</p>
        <div className="modal-action">
          <form method="dialog" onSubmit={(e) => e.preventDefault()} style={{ display: 'flex', gap: 8 }}>
            <button type="button" className="btn" onClick={cancelTokenDelete}>Cancel</button>
            <button type="button" className="btn btn-error" onClick={() => void confirmTokenDelete()} disabled={!!deletingId}>Delete</button>
          </form>
        </div>
      </div>
    </dialog>

    {/* Token Edit Note (DaisyUI modal) */}
    <dialog id="edit_token_note_modal" ref={tokenNoteDialogRef} className="modal">
      <div className="modal-box">
        <h3 className="font-bold text-lg" style={{ marginTop: 0 }}>Edit Token Note</h3>
        <div className="py-2" style={{ display: 'flex', gap: 8 }}>
          <input
            type="text"
            className="input"
            placeholder="Note"
            value={editingTokenNote}
            onChange={(e) => setEditingTokenNote(e.target.value)}
            style={{ flex: 1 }}
          />
        </div>
        <div className="modal-action">
          <form method="dialog" onSubmit={(e) => e.preventDefault()} style={{ display: 'flex', gap: 8 }}>
            <button type="button" className="btn" onClick={cancelTokenNote}>Cancel</button>
            <button type="button" className="btn btn-primary" onClick={() => void saveTokenNote()} disabled={savingTokenNote}>Save</button>
          </form>
        </div>
      </div>
    </dialog>
    </>
  )
}

export default App

type CopyStateValue = 'loading' | 'copied' | undefined

interface LogRowProps {
  log: RequestLog
  isAdmin: boolean
  copyState: CopyStateValue
  onCopy: () => void
  expanded: boolean
  onToggle: (id: number) => void
}

function LogRow({ log, isAdmin, copyState, onCopy, expanded, onToggle }: LogRowProps): JSX.Element {
  const copyButtonClass = `icon-button${copyState === 'copied' ? ' icon-button-success' : ''}${copyState === 'loading' ? ' icon-button-loading' : ''}`
  const requestButtonLabel = expanded ? 'Hide request details' : 'Show request details'

  return (
    <>
      <tr>
        <td>{formatTimestamp(log.created_at)}</td>
        <td>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <code>{log.key_id}</code>
            {isAdmin && (
              <button
                type="button"
                className={copyButtonClass}
                title="复制原始 API key"
                aria-label="复制原始 API key"
                onClick={onCopy}
                disabled={copyState === 'loading'}
              >
                <Icon icon={copyState === 'copied' ? 'mdi:check' : 'mdi:content-copy'} width={18} height={18} />
              </button>
            )}
          </div>
        </td>
        <td>{log.http_status ?? '—'}</td>
        <td>{log.mcp_status ?? '—'}</td>
        <td>
          {isAdmin ? (
            <button
              type="button"
              className={`log-result-button${expanded ? ' log-result-button-active' : ''}`}
              onClick={() => onToggle(log.id)}
              aria-expanded={expanded}
              aria-controls={`log-details-${log.id}`}
              aria-label={requestButtonLabel}
              title={requestButtonLabel}
            >
              <span className={statusClass(log.result_status)}>{statusLabel(log.result_status)}</span>
              <Icon icon={expanded ? 'mdi:chevron-up' : 'mdi:chevron-down'} width={18} height={18} className="log-result-icon" />
            </button>
          ) : (
            <span className={statusClass(log.result_status)}>{statusLabel(log.result_status)}</span>
          )}
        </td>
        <td>{formatErrorMessage(log)}</td>
      </tr>
      {isAdmin && expanded && (
        <tr className="log-details-row">
          <td colSpan={6} id={`log-details-${log.id}`}>
            <LogDetails log={log} />
          </td>
        </tr>
      )}
    </>
  )
}

function LogDetails({ log }: { log: RequestLog }): JSX.Element {
  const query = log.query ? `?${log.query}` : ''
  const requestLine = `${log.method} ${log.path}${query}`
  const forwarded = log.forwarded_headers.filter((value) => value.trim().length > 0)
  const dropped = log.dropped_headers.filter((value) => value.trim().length > 0)

  return (
    <div className="log-details-panel">
      <div className="log-details-summary">
        <div>
          <span className="log-details-label">Request</span>
          <span className="log-details-value">{requestLine}</span>
        </div>
        <div>
          <span className="log-details-label">Response</span>
          <span className="log-details-value">
            {log.http_status != null ? `HTTP ${log.http_status}` : 'HTTP —'}
            {log.mcp_status != null ? ` · MCP ${log.mcp_status}` : ''}
          </span>
        </div>
        <div>
          <span className="log-details-label">Outcome</span>
          <span className="log-details-value">{statusLabel(log.result_status)}</span>
        </div>
      </div>
      <div className="log-details-body">
        <div className="log-details-section">
          <header>Request Body</header>
          <pre>{log.request_body ?? 'No body captured.'}</pre>
        </div>
        <div className="log-details-section">
          <header>Response Body</header>
          <pre>{log.response_body ?? 'No body captured.'}</pre>
        </div>
      </div>
      {(forwarded.length > 0 || dropped.length > 0) && (
        <div className="log-details-headers">
          {forwarded.length > 0 && (
            <div className="log-details-section">
              <header>Forwarded Headers</header>
              <ul>
                {forwarded.map((header, index) => (
                  <li key={`forwarded-${index}-${header}`}>{header}</li>
                ))}
              </ul>
            </div>
          )}
          {dropped.length > 0 && (
            <div className="log-details-section">
              <header>Dropped Headers</header>
              <ul>
                {dropped.map((header, index) => (
                  <li key={`dropped-${index}-${header}`}>{header}</li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
    </div>
  )
}

function KeyDetails({ id, onBack, isAdmin }: { id: string; onBack: () => void; isAdmin: boolean }): JSX.Element {
  const [period, setPeriod] = useState<'day' | 'week' | 'month'>('month')
  const [startDate, setStartDate] = useState<string>(() => new Date().toISOString().slice(0, 10))
  const [summary, setSummary] = useState<KeySummary | null>(null)
  const [logs, setLogs] = useState<RequestLog[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const computeSince = useCallback((): number => {
    const base = new Date(startDate + 'T00:00:00Z')
    if (Number.isNaN(base.getTime())) return Math.floor(Date.now() / 1000)
    const d = new Date(base)
    if (period === 'day') return Math.floor(d.getTime() / 1000)
    if (period === 'week') {
      const day = d.getUTCDay() // 0..6 (Sun..Sat)
      const diff = (day + 6) % 7 // days since Monday
      d.setUTCDate(d.getUTCDate() - diff)
      return Math.floor(d.getTime() / 1000)
    }
    // month
    d.setUTCDate(1)
    return Math.floor(d.getTime() / 1000)
  }, [period, startDate])

  const load = useCallback(async () => {
    try {
      setLoading(true)
      setError(null)
      const since = computeSince()
      const [s, ls] = await Promise.all([
        fetchKeyMetrics(id, period, since),
        fetchKeyLogs(id, 50, since),
      ])
      setSummary(s)
      setLogs(ls)
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : 'Failed to load details')
    } finally {
      setLoading(false)
    }
  }, [id, period, computeSince])

  useEffect(() => {
    void load()
  }, [load])

  const metricCards = useMemo(() => {
    if (!summary) return []
    const total = summary.total_requests
    return [
      { id: 'total', label: 'Total', value: formatNumber(summary.total_requests), subtitle: summary.last_activity ? `Last activity ${formatTimestamp(summary.last_activity)}` : 'No activity' },
      { id: 'success', label: 'Successful', value: formatNumber(summary.success_count), subtitle: formatPercent(summary.success_count, total) },
      { id: 'errors', label: 'Errors', value: formatNumber(summary.error_count), subtitle: formatPercent(summary.error_count, total) },
      { id: 'quota', label: 'Quota Exhausted', value: formatNumber(summary.quota_exhausted_count), subtitle: formatPercent(summary.quota_exhausted_count, total) },
    ]
  }, [summary])

  return (
    <main className="app-shell">
      <section className="surface app-header">
        <div className="title-group">
          <h1>Key Details</h1>
          <p>Inspect usage and recent requests for key: <code>{id}</code></p>
        </div>
        <div className="controls">
          <button type="button" className="button" onClick={onBack}><Icon icon="mdi:arrow-left" width={18} height={18} />&nbsp;Back</button>
        </div>
      </section>

      {error && <div className="surface error-banner">{error}</div>}

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>Usage</h2>
            <p className="panel-description">Aggregated counts for selected period.</p>
          </div>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <select value={period} onChange={(e) => setPeriod(e.target.value as any)} className="input" aria-label="Period">
              <option value="day">Day</option>
              <option value="week">Week</option>
              <option value="month">Month</option>
            </select>
            <input type="date" value={startDate} onChange={(e) => setStartDate(e.target.value)} className="input" />
            <button type="button" className="button button-primary" onClick={() => void load()} disabled={loading}>Apply</button>
          </div>
        </div>
        <section className="metrics-grid">
          {(!summary || loading) ? (
            <div className="empty-state" style={{ gridColumn: '1 / -1' }}>Loading…</div>
          ) : (
            metricCards.map((m) => (
              <div key={m.id} className="metric-card">
                <h3>{m.label}</h3>
                <div className="metric-value">{m.value}</div>
                <div className="metric-subtitle">{m.subtitle}</div>
              </div>
            ))
          )}
        </section>
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>Recent Requests</h2>
            <p className="panel-description">Up to the latest 50 for this key.</p>
          </div>
        </div>
        <div className="table-wrapper">
          {logs.length === 0 ? (
            <div className="empty-state">{loading ? 'Loading…' : 'No request logs for this period.'}</div>
          ) : (
            <table>
              <thead>
                <tr>
                  <th>Time</th>
                  <th>HTTP Status</th>
                  <th>MCP Status</th>
                  <th>Result</th>
                  <th>Error</th>
                </tr>
              </thead>
              <tbody>
                {logs.map((log) => (
                  <tr key={log.id}>
                    <td>{formatTimestamp(log.created_at)}</td>
                    <td>{log.http_status ?? '—'}</td>
                    <td>{log.mcp_status ?? '—'}</td>
                    <td><span className={statusClass(log.result_status)}>{statusLabel(log.result_status)}</span></td>
                    <td>{formatErrorMessage(log)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </section>
    </main>
  )
}
