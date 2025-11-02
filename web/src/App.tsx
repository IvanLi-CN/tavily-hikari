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
} from './api'

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

function App(): JSX.Element {
  const [summary, setSummary] = useState<Summary | null>(null)
  const [keys, setKeys] = useState<ApiKeyStats[]>([])
  const [logs, setLogs] = useState<RequestLog[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [autoRefresh, setAutoRefresh] = useState(true)
  const [lastUpdated, setLastUpdated] = useState<Date | null>(null)
  const [version, setVersion] = useState<{ backend: string; frontend: string } | null>(null)
  const [profile, setProfile] = useState<Profile | null>(null)
  const secretCacheRef = useRef<Map<string, string>>(new Map())
  const [copyState, setCopyState] = useState<Map<string, 'loading' | 'copied'>>(() => new Map())
  const [newKey, setNewKey] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const [deletingId, setDeletingId] = useState<string | null>(null)
  const [togglingId, setTogglingId] = useState<string | null>(null)
  const deleteDialogRef = useRef<HTMLDialogElement | null>(null)
  const [pendingDeleteId, setPendingDeleteId] = useState<string | null>(null)
  const disableDialogRef = useRef<HTMLDialogElement | null>(null)
  const [pendingDisableId, setPendingDisableId] = useState<string | null>(null)

  const copyStateKey = useCallback((scope: 'keys' | 'logs', identifier: string | number) => {
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
        const [summaryData, keyData, logData, ver, profileData] = await Promise.all([
          fetchSummary(signal),
          fetchApiKeys(signal),
          fetchRequestLogs(50, signal),
          fetchVersion(signal).catch(() => null),
          fetchProfile(signal).catch(() => null),
        ])

        if (signal?.aborted) {
          return
        }

        setSummary(summaryData)
        setKeys(keyData)
        setLogs(logData)
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

  useEffect(() => {
    if (!autoRefresh) {
      return
    }
    const timer = window.setInterval(() => {
      const controller = new AbortController()
      void loadData(controller.signal).finally(() => controller.abort())
    }, REFRESH_INTERVAL_MS)
    return () => window.clearInterval(timer)
  }, [autoRefresh, loadData])

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
        subtitle: summary.last_activity ? `Last activity ${formatTimestamp(summary.last_activity)}` : 'No activity yet',
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
            <button
              type="button"
              className={`refresh-toggle ${autoRefresh ? 'active' : ''}`}
              onClick={() => setAutoRefresh((value) => !value)}
            >
              {autoRefresh ? 'Auto Refresh On' : 'Auto Refresh Off'}
            </button>
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
            {!autoRefresh && lastUpdated && (
              <span className="panel-description">Updated {dateTimeFormatter.format(lastUpdated)}</span>
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
                          <code>{item.id}</code>
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
                    <tr key={log.id}>
                      <td>{formatTimestamp(log.created_at)}</td>
                      <td>
                        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                          <code>{log.key_id}</code>
                          {isAdmin && (
                            <button
                              type="button"
                              className={`icon-button${state === 'copied' ? ' icon-button-success' : ''}${state === 'loading' ? ' icon-button-loading' : ''}`}
                              title="复制原始 API key"
                              aria-label="复制原始 API key"
                              onClick={() => void handleCopySecret(log.key_id, stateKey)}
                              disabled={state === 'loading'}
                            >
                              <Icon icon={state === 'copied' ? 'mdi:check' : 'mdi:content-copy'} width={18} height={18} />
                            </button>
                          )}
                        </div>
                      </td>
                      <td>{log.http_status ?? '—'}</td>
                    <td>{log.mcp_status ?? '—'}</td>
                    <td>
                      <span className={statusClass(log.result_status)}>{statusLabel(log.result_status)}</span>
                    </td>
                    <td>{log.error_message ?? '—'}</td>
                  </tr>
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
    </>
  )
}

export default App
