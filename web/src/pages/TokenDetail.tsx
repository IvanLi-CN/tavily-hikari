import { Fragment, type ReactNode, useEffect, useMemo, useRef, useState } from 'react'
import { Icon } from '@iconify/react'
import { useParams, Link } from '@tanstack/react-router'

type Period = 'day' | 'week' | 'month'

interface TokenDetailInfo {
  id: string
  enabled: boolean
  note: string | null
  total_requests: number
  created_at: number
  last_used_at: number | null
}

interface TokenSummary {
  total_requests: number
  success_count: number
  error_count: number
  quota_exhausted_count: number
  last_activity: number | null
}

interface TokenLog {
  id: number
  method: string
  path: string
  query: string | null
  http_status: number | null
  mcp_status: number | null
  result_status: string
  error_message: string | null
  created_at: number
}

const numberFormatter = new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 })
const dateTimeFormatter = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'medium' })
const weekdayFormatter = new Intl.DateTimeFormat('zh-CN', { weekday: 'short' })

function formatNumber(n: number) { return numberFormatter.format(n) }
function formatTime(ts: number | null) { return ts ? dateTimeFormatter.format(new Date(ts * 1000)) : '—' }
function formatLogTime(ts: number | null, period: Period) {
  if (!ts) return '—'
  const date = new Date(ts * 1000)
  const hh = date.getHours().toString().padStart(2, '0')
  const mm = date.getMinutes().toString().padStart(2, '0')
  const ss = date.getSeconds().toString().padStart(2, '0')
  const time = `${hh}:${mm}:${ss}`
  switch (period) {
    case 'day':
      return time
    case 'week':
      return `${weekdayFormatter.format(date)} ${time}`
    case 'month':
      return `${date.getDate().toString().padStart(2, '0')}日 ${time}`
    default:
      return dateTimeFormatter.format(date)
  }
}

function statusClass(status: string): string {
  const s = status.toLowerCase()
  if (s === 'active' || s === 'success') return 'status-badge status-active'
  if (s === 'exhausted' || s === 'quota_exhausted') return 'status-badge status-exhausted'
  if (s === 'error') return 'status-badge status-error'
  return 'status-badge status-unknown'
}

function statusLabel(status: string): string {
  switch (status.toLowerCase()) {
    case 'success': return 'Success'
    case 'error': return 'Error'
    case 'quota_exhausted': return 'Quota Exhausted'
    default: return status
  }
}

function formatDate(value: Date): string {
  const y = value.getFullYear()
  const m = (value.getMonth() + 1).toString().padStart(2, '0')
  const d = value.getDate().toString().padStart(2, '0')
  return `${y}-${m}-${d}`
}

function startOfDay(ts = Date.now()): Date {
  const d = new Date(ts)
  d.setHours(0, 0, 0, 0)
  return d
}
function startOfWeek(ts = Date.now()): Date {
  const d = new Date(ts)
  const day = (d.getDay() + 6) % 7
  d.setDate(d.getDate() - day)
  d.setHours(0, 0, 0, 0)
  return d
}
function startOfMonth(ts = Date.now()): Date {
  const d = new Date(ts)
  d.setDate(1)
  d.setHours(0, 0, 0, 0)
  return d
}

function computeStartDate(period: Period, input: string): Date {
  const now = new Date()
  const maxDate = startOfDay(now.getTime()).valueOf()
  if (!input) {
    return period === 'day' ? new Date(maxDate) : period === 'week' ? startOfWeek(maxDate) : startOfMonth(maxDate)
  }
  if (period === 'day') {
    const [y, m, d] = input.split('-').map(Number)
    if (!y || !m || !d) return startOfDay()
    const result = new Date(y, m - 1, d, 0, 0, 0, 0)
    return result.getTime() > maxDate ? new Date(maxDate) : result
  }
  if (period === 'week') {
    const [y, w] = input.split('-W')
    const year = Number(y)
    const week = Number(w)
    if (!year || !week) return startOfWeek()
    const jan4 = new Date(year, 0, 4)
    const day = (jan4.getDay() + 6) % 7
    const start = new Date(jan4)
    start.setDate(jan4.getDate() - day + (week - 1) * 7)
    start.setHours(0, 0, 0, 0)
    return start.getTime() > maxDate ? new Date(maxDate) : start
  }
  const [yy, mm] = input.split('-').map(Number)
  if (!yy || !mm) return startOfMonth()
  const start = new Date(yy, mm - 1, 1, 0, 0, 0, 0)
  return start.getTime() > maxDate ? startOfMonth(maxDate) : start
}

function computeEndDate(period: Period, start: Date): Date {
  const end = new Date(start)
  if (period === 'day') {
    end.setDate(end.getDate() + 1)
  } else if (period === 'week') {
    end.setDate(end.getDate() + 7)
  } else {
    end.setMonth(end.getMonth() + 1)
  }
  return end
}

function toIso(date: Date): string {
  const pad = (value: number, length = 2) => value.toString().padStart(length, '0')
  const year = date.getFullYear()
  const month = pad(date.getMonth() + 1)
  const day = pad(date.getDate())
  const hours = pad(date.getHours())
  const minutes = pad(date.getMinutes())
  const seconds = pad(date.getSeconds())
  const offsetMinutes = -date.getTimezoneOffset()
  const sign = offsetMinutes >= 0 ? '+' : '-'
  const offsetHour = pad(Math.floor(Math.abs(offsetMinutes) / 60))
  const offsetMinute = pad(Math.abs(offsetMinutes) % 60)
  return `${year}-${month}-${day}T${hours}:${minutes}:${seconds}${sign}${offsetHour}:${offsetMinute}`
}

function formatWeekInput(date: Date): string {
  const tmp = new Date(date)
  tmp.setHours(0, 0, 0, 0)
  // Move to Thursday to ensure correct year
  tmp.setDate(tmp.getDate() + 3 - ((tmp.getDay() + 6) % 7))
  const week1 = new Date(tmp.getFullYear(), 0, 4)
  const weekNumber = 1 + Math.round(((tmp.getTime() - week1.getTime()) / 86400000 - 3 + ((week1.getDay() + 6) % 7)) / 7)
  return `${tmp.getFullYear()}-W${weekNumber.toString().padStart(2, '0')}`
}

function formatPeriodInput(period: Period, date: Date): string {
  if (period === 'day') return formatDate(date)
  if (period === 'week') return formatWeekInput(date)
  return `${date.getFullYear()}-${(date.getMonth() + 1).toString().padStart(2, '0')}`
}

function defaultInputValue(period: Period): string {
  const now = Date.now()
  const base = period === 'day' ? startOfDay(now) : period === 'week' ? startOfWeek(now) : startOfMonth(now)
  return formatPeriodInput(period, base)
}

function sanitizeInput(period: Period, raw: string): string {
  const start = computeStartDate(period, raw)
  return formatPeriodInput(period, start)
}

export default function TokenDetail(): JSX.Element {
  const { id } = useParams({ from: '/tokens/$id' })
  const [info, setInfo] = useState<TokenDetailInfo | null>(null)
  const [summary, setSummary] = useState<TokenSummary | null>(null)
  const [period, setPeriod] = useState<Period>('month')
  const [sinceInput, setSinceInput] = useState<string>('')
  const [debouncedSinceInput, setDebouncedSinceInput] = useState<string>('')
  const [logs, setLogs] = useState<TokenLog[]>([])
  const [page, setPage] = useState(1)
  const [perPage, setPerPage] = useState(20)
  const [total, setTotal] = useState(0)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [warning, setWarning] = useState<string | null>(null)
  const [loadingMore, setLoadingMore] = useState(false)
  const sseRef = useRef<EventSource | null>(null)
  const [expandedLogs, setExpandedLogs] = useState<Set<number>>(() => new Set())
  const warningTimerRef = useRef<number | null>(null)
  const sinceDebounceRef = useRef<number | null>(null)

  const { sinceIso, untilIso } = useMemo(() => {
    const start = computeStartDate(period, debouncedSinceInput)
    const end = computeEndDate(period, start)
    return { sinceIso: toIso(start), untilIso: toIso(end) }
  }, [period, debouncedSinceInput])

  const periodSelectId = `token-period-select-${id}`
  const sinceInputId = `token-since-input-${id}`

  const applyStartInput = (raw: string, nextPeriod: Period = period, opts?: { suppressWarning?: boolean }) => {
    const sanitized = sanitizeInput(nextPeriod, raw || defaultInputValue(nextPeriod))
    const shouldWarn = !opts?.suppressWarning && raw.trim() !== '' && sanitized !== raw
    setWarning(shouldWarn ? 'Start 已自动校正到当前可用范围内' : null)
    setSinceInput((prev) => (prev === sanitized ? prev : sanitized))
  }

  const handleStartChange = (nextPeriod: Period, value: string) => {
    applyStartInput(value, nextPeriod)
  }

  useEffect(() => {
    applyStartInput(sinceInput, period, { suppressWarning: sinceInput.trim() === '' })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [period])

  useEffect(() => {
    if (sinceDebounceRef.current != null) {
      window.clearTimeout(sinceDebounceRef.current)
      sinceDebounceRef.current = null
    }
    sinceDebounceRef.current = window.setTimeout(() => {
      setDebouncedSinceInput(sinceInput)
      sinceDebounceRef.current = null
    }, 500)
    return () => {
      if (sinceDebounceRef.current != null) {
        window.clearTimeout(sinceDebounceRef.current)
        sinceDebounceRef.current = null
      }
    }
  }, [sinceInput])

  useEffect(() => {
    if (!warning) {
      if (warningTimerRef.current != null) {
        window.clearTimeout(warningTimerRef.current)
        warningTimerRef.current = null
      }
      return
    }
    if (warningTimerRef.current != null) {
      window.clearTimeout(warningTimerRef.current)
    }
    warningTimerRef.current = window.setTimeout(() => {
      setWarning(null)
      warningTimerRef.current = null
    }, 4000)
    return () => {
      if (warningTimerRef.current != null) {
        window.clearTimeout(warningTimerRef.current)
        warningTimerRef.current = null
      }
    }
  }, [warning])

  async function getJson<T = any>(url: string): Promise<T> {
    const res = await fetch(url)
    const contentType = res.headers.get('content-type') ?? ''
    const body = await res.text()
    if (!res.ok) {
      throw new Error(body || `${res.status} ${res.statusText}`)
    }
    if (!contentType.toLowerCase().includes('application/json')) {
      throw new Error(body || 'Response was not valid JSON')
    }
    try {
      return JSON.parse(body) as T
    } catch {
      throw new Error(body || 'Failed to parse response JSON')
    }
  }

  // initial load (details + metrics + first page logs)
  useEffect(() => {
    let cancelled = false
    const run = async () => {
      setLoading(true)
      try {
        const [detailRes, summaryRes, logsRes] = await Promise.all([
          getJson(`/api/tokens/${encodeURIComponent(id)}`),
          getJson(`/api/tokens/${encodeURIComponent(id)}/metrics?period=${period}&since=${encodeURIComponent(sinceIso)}&until=${encodeURIComponent(untilIso)}`),
          getJson(`/api/tokens/${encodeURIComponent(id)}/logs/page?page=1&per_page=${perPage}&since=${encodeURIComponent(sinceIso)}&until=${encodeURIComponent(untilIso)}`),
        ])
        if (cancelled) return
        setInfo(detailRes)
        setSummary(summaryRes)
        setLogs(logsRes.items)
        setPage(1)
        setPerPage(logsRes.per_page ?? logsRes.perPage ?? perPage)
        setTotal(logsRes.total)
        setExpandedLogs(new Set())
        setError(null)
      } catch (e) {
        if (cancelled) return
        setError(e instanceof Error ? e.message : 'Failed to load token details')
      } finally {
        if (!cancelled) setLoading(false)
      }
    }
    void run()
    return () => { cancelled = true }
  }, [id, period, sinceIso, untilIso, perPage])

  // SSE for live updates (refresh first page upon snapshot)
  useEffect(() => {
    try { sseRef.current?.close() } catch {}
    const es = new EventSource(`/api/tokens/${encodeURIComponent(id)}/events`)
    sseRef.current = es
    es.addEventListener('snapshot', (ev: MessageEvent) => {
      try {
        const data = JSON.parse(ev.data) as { summary: TokenSummary, logs: TokenLog[] }
        if (period === 'month' && !sinceInput) {
          setSummary(data.summary)
        }
        if (page === 1) {
          getJson(`/api/tokens/${encodeURIComponent(id)}/logs/page?page=1&per_page=${perPage}&since=${encodeURIComponent(sinceIso)}&until=${encodeURIComponent(untilIso)}`)
            .then((p) => {
              setLogs(p.items)
              setTotal(p.total)
              setPerPage(p.per_page ?? p.perPage ?? perPage)
              setPage(1)
              setExpandedLogs(new Set())
            })
            .catch(() => {})
        }
      } catch {}
    })
    es.onerror = () => { /* ignore, fallback to polling via initial load */ }
    return () => { try { es.close() } catch {} }
  }, [id, page, perPage, period, sinceInput, sinceIso, untilIso])

  useEffect(() => {
    ;(window as typeof window & { __TOKEN_PERIOD__?: Period }).__TOKEN_PERIOD__ = period
  }, [period])

  const goToPage = async (next: number) => {
    const p = Math.max(1, Math.min(next, Math.max(1, Math.ceil(total / perPage) || 1)))
    setLoadingMore(true)
    try {
      const data = await getJson(`/api/tokens/${encodeURIComponent(id)}/logs/page?page=${p}&per_page=${perPage}&since=${encodeURIComponent(sinceIso)}&until=${encodeURIComponent(untilIso)}`)
      setLogs(data.items)
      setPage(data.page)
      setPerPage(data.per_page ?? data.perPage ?? perPage)
      setTotal(data.total)
      setExpandedLogs(new Set())
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load page')
    } finally {
      setLoadingMore(false)
    }
  }

  const totalPages = Math.max(1, Math.ceil(total / perPage) || 1)
  const toggleLog = (logId: number) => {
    setExpandedLogs((prev) => {
      const next = new Set(prev)
      if (next.has(logId)) {
        next.delete(logId)
      } else {
        next.add(logId)
      }
      return next
    })
  }

  return (
    <main className="app-shell">
      <section className="surface app-header">
        <div className="title-group">
          <h1>Access Token Detail</h1>
          <div className="subtitle">Token <code>{id}</code></div>
        </div>
        <Link to="/" className="button">Back</Link>
      </section>

      {error && <div className="surface error-banner" role="alert">{error}</div>}

      <section className="surface panel token-info-section">
        <div className="token-info-grid" aria-label="Token metadata">
          <InfoCard
            label="Token ID"
            value={<code className="code-chip" title={info?.id ?? id}>{info?.id ?? id}</code>}
          />
          <InfoCard
            label="Status"
            value={
              <span className={info?.enabled ? 'status-badge status-active' : 'status-badge status-error'}>
                {info?.enabled ? 'Enabled' : 'Disabled'}
              </span>
            }
          />
          <InfoCard label="Total Requests" value={formatNumber(info?.total_requests ?? 0)} />
          <InfoCard label="Created" value={formatTime(info?.created_at ?? null)} />
          <InfoCard label="Last Used" value={formatTime(info?.last_used_at ?? null)} />
          <InfoCard
            label="Note"
            value={info?.note ? <span className="token-info-note" title={info.note}>{info.note}</span> : '—'}
          />
        </div>
      </section>

      <section className="surface panel">
        <div className="panel-header token-panel-header">
          <div>
            <h2>Usage Snapshot</h2>
            <p className="panel-description">Aggregated metrics for the selected window.</p>
          </div>
          <div className="token-period-controls" role="group" aria-label="Period filter">
            <div className="token-period-control">
              <label htmlFor={periodSelectId}>Period</label>
              <select
                id={periodSelectId}
                className="input"
                value={period}
                onChange={(e) => { const next = e.target.value as Period; setPeriod(next); applyStartInput('', next) }}
              >
                <option value="day">Day</option>
                <option value="week">Week</option>
                <option value="month">Month</option>
              </select>
            </div>
            <div className="token-period-control">
              <label htmlFor={sinceInputId}>Start</label>
              {period === 'day' && (
                <input
                  id={sinceInputId}
                  type="date"
                  className="input"
                  max={defaultInputValue('day')}
                  value={sinceInput}
                  onChange={(e) => handleStartChange(period, e.target.value)}
                />
              )}
              {period === 'week' && (
                <input
                  id={sinceInputId}
                  type="week"
                  className="input"
                  max={defaultInputValue('week')}
                  value={sinceInput}
                  onChange={(e) => handleStartChange(period, e.target.value)}
                />
              )}
              {period === 'month' && (
                <input
                  id={sinceInputId}
                  type="month"
                  className="input"
                  max={defaultInputValue('month')}
                  value={sinceInput}
                  onChange={(e) => handleStartChange(period, e.target.value)}
                />
              )}
            </div>
          </div>
        </div>
        {warning && (
          <div className="token-period-warning alert alert-warning" role="status">
            <Icon icon="mdi:alert-circle-outline" width={18} height={18} aria-hidden="true" className="token-warning-icon" />
            <span>{warning}</span>
          </div>
        )}
        <div className="stats stats-vertical lg:stats-horizontal shadow token-stats">
          <MetricCard label="Requests" value={formatNumber(summary?.total_requests ?? 0)} />
          <MetricCard label="Success" value={formatNumber(summary?.success_count ?? 0)} />
          <MetricCard label="Errors" value={formatNumber(summary?.error_count ?? 0)} />
          <MetricCard label="Quota Exhausted" value={formatNumber(summary?.quota_exhausted_count ?? 0)} />
        </div>
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>Request Records</h2>
            <p className="panel-description">Newest entries first. Live refresh applies to the first page.</p>
          </div>
        </div>
        <div className="table-wrapper">
          <table className="token-detail-table">
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
              {logs.map((l) => (
                <Fragment key={l.id}>
                  <tr>
                    <td>{formatLogTime(l.created_at, period)}</td>
                    <td>{l.http_status ?? '—'}</td>
                    <td>{l.mcp_status ?? '—'}</td>
                    <td>
                      <button
                        type="button"
                        className={`log-result-button${expandedLogs.has(l.id) ? ' log-result-button-active' : ''}`}
                        onClick={() => toggleLog(l.id)}
                        aria-expanded={expandedLogs.has(l.id)}
                        aria-controls={`token-log-details-${l.id}`}
                      >
                        <span className={statusClass(l.result_status)}>{statusLabel(l.result_status)}</span>
                        <Icon
                          icon={expandedLogs.has(l.id) ? 'mdi:chevron-up' : 'mdi:chevron-down'}
                          width={18}
                          height={18}
                          className="log-result-icon"
                          aria-hidden="true"
                        />
                      </button>
                    </td>
                    <td>{l.error_message ?? '—'}</td>
                  </tr>
                  {expandedLogs.has(l.id) && (
                    <tr className="log-details-row">
                      <td colSpan={5} id={`token-log-details-${l.id}`}>
                        <TokenLogDetails log={l} period={period} />
                      </td>
                    </tr>
                  )}
                </Fragment>
              ))}
              {logs.length === 0 && (
                <tr><td colSpan={5} style={{ padding: 12 }}>{loading ? 'Loading…' : 'No logs yet.'}</td></tr>
              )}
            </tbody>
          </table>
        </div>
        <div className="table-pagination">
          <span>每页</span>
          <select className="input" value={perPage} onChange={(e) => { setPerPage(Number(e.target.value)); void goToPage(1) }}>
            {[10, 20, 50, 100].map((v) => <option key={v} value={v}>{v}</option>)}
          </select>
          <button type="button" className="button" onClick={() => void goToPage(page - 1)} disabled={page <= 1 || loadingMore}>上一页</button>
          <span>第 {page} / {totalPages} 页</span>
          <button type="button" className="button" onClick={() => void goToPage(page + 1)} disabled={page >= totalPages || loadingMore}>下一页</button>
        </div>
      </section>
    </main>
  )
}

function MetricCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="stat token-stat">
      <div className="stat-title">{label}</div>
      <div className="stat-value">{value}</div>
    </div>
  )
}

function InfoCard({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="token-info-card">
      <span className="token-info-label">{label}</span>
      <div className="token-info-value">{value}</div>
    </div>
  )
}

function TokenLogDetails({ log, period }: { log: TokenLog; period: Period }) {
  const query = log.query ? `?${log.query}` : ''
  const requestLine = `${log.method} ${log.path}${query}`
  const errorText = (log.error_message ?? '').trim() || 'No error reported.'
  const httpStatus = log.http_status != null ? `HTTP ${log.http_status}` : 'HTTP —'
  const mcpStatus = log.mcp_status != null ? `MCP ${log.mcp_status}` : 'MCP —'

  return (
    <div className="log-details-panel">
      <div className="log-details-summary">
        <div>
          <span className="log-details-label">Time</span>
          <span className="log-details-value">{formatLogTime(log.created_at, period)}</span>
        </div>
        <div>
          <span className="log-details-label">Status</span>
          <span className="log-details-value">{`${httpStatus} · ${mcpStatus}`}</span>
        </div>
        <div>
          <span className="log-details-label">Outcome</span>
          <span className="log-details-value">{statusLabel(log.result_status)}</span>
        </div>
      </div>
      <div className="log-details-body">
        <div className="log-details-section">
          <header>Request</header>
          <pre>{requestLine}</pre>
        </div>
        <div className="log-details-section">
          <header>Error Message</header>
          <pre>{errorText}</pre>
        </div>
      </div>
    </div>
  )
}
