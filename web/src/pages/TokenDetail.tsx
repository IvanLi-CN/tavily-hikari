import { Fragment, type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Icon } from '@iconify/react'
import { Chart as ChartJS, BarElement, CategoryScale, Legend, LinearScale, Tooltip, type ChartOptions } from 'chart.js'
import { Bar } from 'react-chartjs-2'
import { fetchTokenUsageSeries, rotateTokenSecret, type TokenUsageBucket } from '../api'

ChartJS.register(CategoryScale, LinearScale, BarElement, Tooltip, Legend)

type Period = 'day' | 'week' | 'month'

interface TokenDetailInfo {
  id: string
  enabled: boolean
  note: string | null
  total_requests: number
  created_at: number
  last_used_at: number | null
  quota_state: 'normal' | 'hour' | 'day' | 'month'
  quota_hourly_used: number
  quota_hourly_limit: number
  quota_daily_used: number
  quota_daily_limit: number
  quota_monthly_used: number
  quota_monthly_limit: number
  quota_hourly_reset_at: number | null
  quota_daily_reset_at: number | null
  quota_monthly_reset_at: number | null
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

interface UsageBar {
  bucket: number
  success: number
  system: number
  external: number
}

const numberFormatter = new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 })
const dateTimeFormatter = new Intl.DateTimeFormat('en-US', { dateStyle: 'medium', timeStyle: 'medium' })
const weekdayFormatter = new Intl.DateTimeFormat('en-US', { weekday: 'short' })

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
      return `${date.toLocaleDateString('en-US', { month: 'short', day: '2-digit' })} ${time}`
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

interface QuotaStatCardProps {
  label: string
  used: number
  limit: number
  resetAt?: number | null
  description: string
}

function QuotaStatCard({ label, used, limit, resetAt, description }: QuotaStatCardProps): JSX.Element {
  const shouldShowReset = used > 0 && typeof resetAt === 'number' && resetAt * 1000 > Date.now()
  let resetLabel = 'Not used yet'
  if (shouldShowReset) {
    try {
      resetLabel = dateTimeFormatter.format(new Date(resetAt! * 1000))
    } catch {
      resetLabel = '—'
    }
  }
  return (
    <div className="quota-stat-card">
      <div className="quota-stat-label">{label}</div>
      <div className="quota-stat-value">
        {formatNumber(used)}
        <span>/ {formatNumber(limit)}</span>
      </div>
      <div className="quota-stat-description">{description}</div>
      <div className="quota-stat-reset">
        {shouldShowReset ? `Next reset: ${resetLabel}` : resetLabel}
      </div>
    </div>
  )
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

function alignToBucket(timestampSec: number, bucketSeconds: number): number {
  return timestampSec - (timestampSec % bucketSeconds)
}

function buildUsageBars(
  buckets: TokenUsageBucket[],
  startSec: number,
  bucketSeconds: number,
  bucketCount: number,
): UsageBar[] {
  const map = new Map<number, TokenUsageBucket>()
  for (const bucket of buckets) {
    map.set(bucket.bucket_start, bucket)
  }
  const bars: UsageBar[] = []
  for (let i = 0; i < bucketCount; i += 1) {
    const bucketStart = startSec + i * bucketSeconds
    const found = map.get(bucketStart)
    bars.push({
      bucket: bucketStart,
      success: found?.success_count ?? 0,
      system: found?.system_failure_count ?? 0,
      external: found?.external_failure_count ?? 0,
    })
  }
  return bars
}

function hourLabel(bucket: number): string {
  const date = new Date(bucket * 1000)
  return `${date.getHours().toString().padStart(2, '0')}:00`
}

function dayLabel(bucket: number): string {
  const date = new Date(bucket * 1000)
  return date.toLocaleDateString('en-US', { month: 'short', day: '2-digit' })
}

export default function TokenDetail({ id, onBack }: { id: string; onBack?: () => void }): JSX.Element {
  const [info, setInfo] = useState<TokenDetailInfo | null>(null)
  const [summary, setSummary] = useState<TokenSummary | null>(null)
  const [quickStats, setQuickStats] = useState<{
    day: TokenSummary | null
    month: TokenSummary | null
    total: TokenSummary | null
  }>({ day: null, month: null, total: null })
  const [period, setPeriod] = useState<Period>('month')
  const [sinceInput, setSinceInput] = useState<string>('')
  const [debouncedSinceInput, setDebouncedSinceInput] = useState<string>('')
  const [logs, setLogs] = useState<TokenLog[]>([])
  const [page, setPage] = useState(1)
  const [perPage, setPerPage] = useState(20)
  const [total, setTotal] = useState(0)
  const [loading, setLoading] = useState(true)
  const [quickUsage, setQuickUsage] = useState<UsageBar[]>([])
  const [quickUsageLoading, setQuickUsageLoading] = useState(true)
  const [snapshotUsage, setSnapshotUsage] = useState<UsageBar[]>([])
  const [snapshotUsageLoading, setSnapshotUsageLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [warning, setWarning] = useState<string | null>(null)
  const [loadingMore, setLoadingMore] = useState(false)
  const sseRef = useRef<EventSource | null>(null)
  const [expandedLogs, setExpandedLogs] = useState<Set<number>>(() => new Set())
  const warningTimerRef = useRef<number | null>(null)
  const sinceDebounceRef = useRef<number | null>(null)
  const rotateDialogRef = useRef<HTMLDialogElement | null>(null)
  const rotatedDialogRef = useRef<HTMLDialogElement | null>(null)
  const [rotating, setRotating] = useState(false)
  const [rotatedToken, setRotatedToken] = useState<string | null>(null)
  const [sseConnected, setSseConnected] = useState(false)
  const quickUsageAbortRef = useRef<AbortController | null>(null)
  const snapshotUsageAbortRef = useRef<AbortController | null>(null)

  useEffect(() => {
    setInfo(null)
    setSummary(null)
    setQuickStats({ day: null, month: null, total: null })
    setLogs([])
    setPage(1)
    setTotal(0)
    setWarning(null)
    setQuickUsage([])
    setQuickUsageLoading(true)
    setSnapshotUsage([])
    setSnapshotUsageLoading(true)
  }, [id])

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
    setWarning(shouldWarn ? 'Start value was adjusted to the valid range' : null)
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

  async function loadQuickStats() {
    const now = new Date()
    const dayStart = startOfDay(now.getTime())
    const monthStart = startOfMonth(now.getTime())
    const sinceDay = toIso(dayStart)
    const sinceMonth = toIso(monthStart)
    const sinceEpoch = '1970-01-01T00:00:00+00:00'
    const untilNow = toIso(now)
    try {
      const [d, m, t] = await Promise.all([
        getJson<TokenSummary>(`/api/tokens/${encodeURIComponent(id)}/metrics?since=${encodeURIComponent(sinceDay)}&until=${encodeURIComponent(untilNow)}`),
        getJson<TokenSummary>(`/api/tokens/${encodeURIComponent(id)}/metrics?since=${encodeURIComponent(sinceMonth)}&until=${encodeURIComponent(untilNow)}`),
        getJson<TokenSummary>(`/api/tokens/${encodeURIComponent(id)}/metrics?since=${encodeURIComponent(sinceEpoch)}&until=${encodeURIComponent(untilNow)}`),
      ])
      setQuickStats({ day: d, month: m, total: t })
    } catch {
      // ignore quick stats errors to avoid blocking page
    }
  }

  const refreshQuickUsage = useCallback(() => {
    quickUsageAbortRef.current?.abort()
    const controller = new AbortController()
    quickUsageAbortRef.current = controller
    setQuickUsageLoading(true)
    const bucketSeconds = 3600
    const nowSec = Math.floor(Date.now() / 1000)
    const currentBucket = alignToBucket(nowSec, bucketSeconds)
    const bucketCount = 25
    const startSec = currentBucket - (bucketCount - 1) * bucketSeconds
    const untilSec = currentBucket + bucketSeconds
    fetchTokenUsageSeries(
      id,
      { since: toIso(new Date(startSec * 1000)), until: toIso(new Date(untilSec * 1000)), bucketSecs: bucketSeconds },
      controller.signal,
    )
      .then((rows) => {
        if (!controller.signal.aborted) {
          setQuickUsage(buildUsageBars(rows, startSec, bucketSeconds, bucketCount))
        }
      })
      .catch(() => {
        if (!controller.signal.aborted) setQuickUsage([])
      })
      .finally(() => {
        if (!controller.signal.aborted) setQuickUsageLoading(false)
      })
  }, [id])

  const refreshSnapshotUsage = useCallback(() => {
    snapshotUsageAbortRef.current?.abort()
    const controller = new AbortController()
    snapshotUsageAbortRef.current = controller
    setSnapshotUsageLoading(true)
    const bucketSeconds = period === 'day' ? 3600 : 86400
    const startMs = Date.parse(sinceIso)
    const endMs = Date.parse(untilIso)
    const safeStart = Number.isNaN(startMs) ? Date.now() : startMs
    const safeEnd = Number.isNaN(endMs) ? safeStart + bucketSeconds * 1000 : endMs
    const startSec = alignToBucket(Math.floor(safeStart / 1000), bucketSeconds)
    const endSec = Math.max(startSec + bucketSeconds, Math.floor(safeEnd / 1000))
    const bucketCount = Math.max(1, Math.ceil((endSec - startSec) / bucketSeconds))
    const untilAligned = startSec + bucketCount * bucketSeconds
    fetchTokenUsageSeries(
      id,
      { since: toIso(new Date(startSec * 1000)), until: toIso(new Date(untilAligned * 1000)), bucketSecs: bucketSeconds },
      controller.signal,
    )
      .then((rows) => {
        if (!controller.signal.aborted) {
          setSnapshotUsage(buildUsageBars(rows, startSec, bucketSeconds, bucketCount))
        }
      })
      .catch(() => {
        if (!controller.signal.aborted) setSnapshotUsage([])
      })
      .finally(() => {
        if (!controller.signal.aborted) setSnapshotUsageLoading(false)
      })
  }, [id, period, sinceIso, untilIso])

  useEffect(() => {
    refreshQuickUsage()
    return () => { quickUsageAbortRef.current?.abort() }
  }, [refreshQuickUsage])

  useEffect(() => {
    refreshSnapshotUsage()
    return () => { snapshotUsageAbortRef.current?.abort() }
  }, [refreshSnapshotUsage])

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
        void loadQuickStats()
        refreshQuickUsage()
        refreshSnapshotUsage()
      } catch (e) {
        if (cancelled) return
        setError(e instanceof Error ? e.message : 'Failed to load token details')
      } finally {
        if (!cancelled) setLoading(false)
      }
    }
    void run()
    return () => { cancelled = true }
  }, [id, period, sinceIso, untilIso, perPage, refreshQuickUsage, refreshSnapshotUsage])

  // SSE for live updates (refresh first page upon snapshot)
  useEffect(() => {
    const refreshDetail = async () => {
      try {
        const detail = await getJson(`/api/tokens/${encodeURIComponent(id)}`)
        setInfo(detail)
      } catch {
        // ignore
      }
    }
    const refreshLogs = async () => {
      if (page !== 1) return
      try {
        const data = await getJson(`/api/tokens/${encodeURIComponent(id)}/logs/page?page=1&per_page=${perPage}&since=${encodeURIComponent(sinceIso)}&until=${encodeURIComponent(untilIso)}`)
        setLogs(data.items)
        setTotal(data.total)
        setPerPage(data.per_page ?? data.perPage ?? perPage)
        setPage(1)
        setExpandedLogs(new Set())
      } catch {
        // ignore
      }
    }
    try { sseRef.current?.close() } catch {}
    const es = new EventSource(`/api/tokens/${encodeURIComponent(id)}/events`)
    sseRef.current = es
    es.addEventListener('snapshot', (ev: MessageEvent) => {
      try {
        const data = JSON.parse(ev.data) as { summary: TokenSummary, logs: TokenLog[] }
        const defaultMonthInput = defaultInputValue('month')
        const isMonthView = period === 'month' && (debouncedSinceInput === '' || debouncedSinceInput === defaultMonthInput)
        if (isMonthView) {
          setSummary(data.summary)
        }
        void refreshDetail()
        void refreshLogs()
        void loadQuickStats()
        refreshQuickUsage()
        refreshSnapshotUsage()
        setSseConnected(true)
      } catch {
        // ignore bad payloads
      }
    })
    es.onopen = () => setSseConnected(true)
    es.onerror = () => { setSseConnected(false) }
    return () => { try { es.close() } catch {} setSseConnected(false) }
  }, [id, page, perPage, period, sinceIso, untilIso, debouncedSinceInput, refreshQuickUsage, refreshSnapshotUsage])

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
        <div style={{ display: 'flex', gap: 8 }}>
          <span className={`sse-chip ${sseConnected ? 'sse-chip-ok' : 'sse-chip-warn'}`} title="Live updates via SSE">
            <span className="sse-dot" aria-hidden="true" /> {sseConnected ? 'Live' : 'Offline'}
          </span>
          <button type="button" className="btn" onClick={() => (onBack ? onBack() : window.history.back())}>
            <Icon icon="mdi:arrow-left" width={18} height={18} />
            &nbsp;Back
          </button>
          <button
            type="button"
            className="btn btn-warning"
            onClick={() => rotateDialogRef.current?.showModal()}
            aria-label="Regenerate secret"
          >
            <Icon icon="mdi:key-change" width={18} height={18} />
            &nbsp;Regenerate Secret
          </button>
        </div>
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
        <div className="panel-header">
          <div>
            <h2>Quick Stats</h2>
            <p className="panel-description">Rolling usage windows (1 hour / 24 hours / calendar month).</p>
          </div>
        </div>
        <section className="quick-stats-grid">
          {info ? (
            <>
              <QuotaStatCard
                label="1 Hour"
                used={info.quota_hourly_used}
                limit={info.quota_hourly_limit}
                resetAt={info.quota_hourly_reset_at}
                description="Rolling 1-hour window"
              />
              <QuotaStatCard
                label="24 Hours"
                used={info.quota_daily_used}
                limit={info.quota_daily_limit}
                resetAt={info.quota_daily_reset_at}
                description="Rolling 24-hour window"
              />
              <QuotaStatCard
                label="This Month"
                used={info.quota_monthly_used}
                limit={info.quota_monthly_limit}
                resetAt={info.quota_monthly_reset_at}
                description="Calendar month"
              />
            </>
          ) : (
            <div className="empty-state" style={{ gridColumn: '1 / -1' }}>Loading…</div>
          )}
        </section>
        <div style={{ marginTop: 16 }}>
          <UsageChart data={quickUsage} loading={quickUsageLoading} labelFormatter={hourLabel} height={200} />
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
        <div className="token-stats">
          <MetricCard label="Requests" value={formatNumber(summary?.total_requests ?? 0)} />
          <MetricCard label="Success" value={formatNumber(summary?.success_count ?? 0)} />
          <MetricCard label="Errors" value={formatNumber(summary?.error_count ?? 0)} />
          <MetricCard label="Quota Exhausted" value={formatNumber(summary?.quota_exhausted_count ?? 0)} />
        </div>
        <div style={{ marginTop: 16 }}>
          <UsageChart
            data={snapshotUsage}
            loading={snapshotUsageLoading}
            labelFormatter={period === 'day' ? hourLabel : dayLabel}
            height={220}
          />
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
          <span>Per page</span>
          <select className="input" value={perPage} onChange={(e) => { setPerPage(Number(e.target.value)); void goToPage(1) }}>
            {[10, 20, 50, 100].map((v) => <option key={v} value={v}>{v}</option>)}
          </select>
          <button type="button" className="button" onClick={() => void goToPage(page - 1)} disabled={page <= 1 || loadingMore}>Previous</button>
          <span>Page {page} / {totalPages}</span>
          <button type="button" className="button" onClick={() => void goToPage(page + 1)} disabled={page >= totalPages || loadingMore}>Next</button>
        </div>
      </section>
    
    <dialog id="confirm_rotate_token_modal" ref={rotateDialogRef} className="modal">
      <div className="modal-box">
        <h3 className="font-bold text-lg" style={{ marginTop: 0 }}>Regenerate Token Secret</h3>
        <p className="py-2">
          This will invalidate the current token secret and generate a new one. The 4-char token ID will remain the same.
          Clients must be updated to use the new token.
        </p>
        <div className="modal-action">
          <form method="dialog" onSubmit={(e) => e.preventDefault()} style={{ display: 'flex', gap: 8 }}>
            <button type="button" className="btn" onClick={() => rotateDialogRef.current?.close()}>Cancel</button>
            <button
              type="button"
              className={`btn ${rotating ? 'btn-disabled' : ''}`}
              onClick={async () => {
                try {
                  setRotating(true)
                  const res = await rotateTokenSecret(id)
                  setRotatedToken(res.token)
                  try { await navigator.clipboard?.writeText(res.token) } catch {}
                  rotateDialogRef.current?.close()
                  window.requestAnimationFrame(() => rotatedDialogRef.current?.showModal())
                } catch (e) {
                  // Fallback: close dialog and surface error inline
                  rotateDialogRef.current?.close()
                  alert((e as Error)?.message || 'Failed to regenerate token secret')
                } finally {
                  setRotating(false)
                }
              }}
              disabled={rotating}
            >
              {rotating ? 'Regenerating…' : 'Regenerate'}
            </button>
          </form>
        </div>
      </div>
    </dialog>

    <dialog id="rotated_token_modal" ref={rotatedDialogRef} className="modal">
      <div className="modal-box">
        <h3 className="font-bold text-lg" style={{ marginTop: 0 }}>New Token Generated</h3>
        <div className="py-2">
          <p className="panel-description" style={{ marginBottom: 8 }}>Full token (copied to clipboard):</p>
          <pre style={{ whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>{rotatedToken ?? '—'}</pre>
        </div>
        <div className="modal-action">
          <form method="dialog" onSubmit={(e) => e.preventDefault()} style={{ display: 'flex', gap: 8 }}>
            <button type="button" className="btn" onClick={() => rotatedDialogRef.current?.close()}>Close</button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={async () => { if (rotatedToken) { try { await navigator.clipboard?.writeText(rotatedToken) } catch {} } }}
            >
              Copy
            </button>
          </form>
        </div>
      </div>
    </dialog>
    </main>
  )
}

function MetricCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="token-stat">
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

function UsageChart({
  data,
  loading,
  labelFormatter,
  height = 180,
}: {
  data: UsageBar[]
  loading: boolean
  labelFormatter: (bucket: number) => string
  height?: number
}) {
  const labels = data.map((d) => labelFormatter(d.bucket))
  const totals = data.reduce(
    (acc, cur) => {
      acc.success += cur.success
      acc.system += cur.system
      acc.external += cur.external
      return acc
    },
    { success: 0, system: 0, external: 0 },
  )
  const chartData = {
    labels,
    datasets: [
      { label: 'Success', data: data.map((d) => d.success), backgroundColor: '#16a34a', stack: 'requests' },
      { label: 'System limited', data: data.map((d) => d.system), backgroundColor: '#f97316', stack: 'requests' },
      { label: 'Other failures', data: data.map((d) => d.external), backgroundColor: '#ef4444', stack: 'requests' },
    ],
  }
  const options: ChartOptions<'bar'> = {
    responsive: true,
    maintainAspectRatio: false,
    plugins: { legend: { position: 'bottom' }, tooltip: { mode: 'index', intersect: false } },
    scales: {
      x: { stacked: true },
      y: { stacked: true, beginAtZero: true, title: { display: true, text: 'Requests' } },
    },
  } as ChartOptions<'bar'>
  return (
    <div className="hourly-chart" style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      {loading ? (
        <div className="empty-state">Loading…</div>
      ) : (
        <div style={{ height }}>
          <Bar options={options} data={chartData} />
        </div>
      )}
    </div>
  )
}
