import { Fragment, type ReactNode, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { Icon } from '../lib/icons'
import { Chart as ChartJS, BarElement, CategoryScale, Legend, LinearScale, Tooltip, type ChartOptions } from 'chart.js'
import { Bar } from 'react-chartjs-2'
import {
  fetchTokenUsageSeries,
  rotateTokenSecret,
  type LogOperationalClass,
  type TokenOwnerSummary,
  type TokenUsageBucket,
} from '../api'
import { type QueryLoadState, getBlockingLoadState, getRefreshingLoadState, isBlockingLoadState, isRefreshingLoadState } from '../admin/queryLoadState'
import AdminLoadingRegion from '../components/AdminLoadingRegion'
import AdminReturnToConsoleLink from '../components/AdminReturnToConsoleLink'
import AdminTablePagination from '../components/AdminTablePagination'
import AdminTableShell from '../components/AdminTableShell'
import RequestKindBadge from '../components/RequestKindBadge'
import ThemeToggle from '../components/ThemeToggle'
import { StatusBadge, type StatusTone } from '../components/StatusBadge'
import { Button } from '../components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '../components/ui/dialog'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '../components/ui/dropdown-menu'
import { Input } from '../components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../components/ui/select'
import SegmentedTabs from '../components/ui/SegmentedTabs'
import { TableBody, TableCell, TableHead, TableHeader, TableRow } from '../components/ui/table'
import { Textarea } from '../components/ui/textarea'
import { AnchoredInfoDisclosure } from '../components/ui/anchored-info-disclosure'
import { Tooltip as OverlayTooltip, TooltipContent, TooltipTrigger } from '../components/ui/tooltip'
import { useLanguage, useTranslate } from '../i18n'
import { ADMIN_USER_CONSOLE_HREF } from '../lib/adminUserConsoleEntry'
import { copyText, selectAllReadonlyText } from '../lib/clipboard'
import {
  operationalClassGuidance,
  operationalClassLabel,
  operationalClassTone,
} from '../lib/logOperationalClass'
import { useResponsiveModes } from '../lib/responsive'
import {
  buildRequestKindQuickFilterSelection,
  buildVisibleRequestKindOptions,
  buildTokenLogsPagePath,
  defaultTokenLogRequestKindQuickFilters,
  hasActiveRequestKindQuickFilters,
  mergeRequestKindOptionsByKey,
  resolveEffectiveRequestKindSelection,
  resolveRequestKindOptionsRefresh,
  resolveManualRequestKindQuickFilters,
  requestKindSelectionsMatch,
  summarizeRequestKindQuickFilters,
  summarizeSelectedRequestKinds,
  toggleRequestKindSelection,
  type TokenLogRequestKindQuickBilling,
  type TokenLogRequestKindQuickProtocol,
  type TokenLogRequestKindOption,
  type TokenLogOperationalClassFilter,
  uniqueSelectedRequestKinds,
} from '../tokenLogRequestKinds'

ChartJS.register(CategoryScale, LinearScale, BarElement, Tooltip, Legend)

type Period = 'day' | 'week' | 'month'

interface TokenDetailInfo {
  id: string
  enabled: boolean
  note: string | null
  owner?: TokenOwnerSummary | null
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
  business_credits: number | null
  request_kind_key: string
  request_kind_label: string
  request_kind_detail: string | null
  result_status: string
  error_message: string | null
  failure_kind?: string | null
  key_effect_code?: string
  key_effect_summary?: string | null
  created_at: number
  operationalClass:
    | 'success'
    | 'neutral'
    | 'client_error'
    | 'upstream_error'
    | 'system_error'
    | 'quota_exhausted'
  requestKindProtocolGroup: 'api' | 'mcp'
  requestKindBillingGroup: 'billable' | 'non_billable'
}

interface TokenLogsPageResponse {
  items: TokenLog[]
  page: number
  per_page?: number
  perPage?: number
  total: number
  request_kind_options: TokenLogRequestKindOption[]
}

interface UsageBar {
  bucket: number
  success: number
  system: number
  external: number
}

const requestKindBillingQuickFilterOptions = [
  { value: 'all', label: 'Any' },
  { value: 'billable', label: 'Paid' },
  { value: 'non_billable', label: 'Free' },
] as const

const requestKindProtocolQuickFilterOptions = [
  { value: 'all', label: 'Any' },
  { value: 'mcp', label: 'MCP' },
  { value: 'api', label: 'API' },
] as const
const operationalClassFilterOptions = [
  { value: 'all', label: 'All outcomes' },
  { value: 'success', label: 'Success' },
  { value: 'neutral', label: 'Neutral' },
  { value: 'client_error', label: 'Client Error' },
  { value: 'upstream_error', label: 'Upstream Error' },
  { value: 'system_error', label: 'System Error' },
  { value: 'quota_exhausted', label: 'Quota Exhausted' },
] as const

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

function statusTone(status: string): StatusTone {
  return operationalClassTone(status)
}

function statusLabel(status: string, language: 'en' | 'zh'): string {
  return operationalClassLabel(status, language)
}

function formatTokenKeyEffectSummary(log: TokenLog, language: 'en' | 'zh'): string {
  const summary = log.key_effect_summary?.trim()
  switch ((log.key_effect_code ?? '').trim()) {
    case 'quarantined':
      return language === 'zh' ? '系统已自动隔离该 Key' : 'The system automatically quarantined this key'
    case 'marked_exhausted':
      return language === 'zh' ? '系统已自动将该 Key 标记为耗尽' : 'The system automatically marked this key as exhausted'
    case 'restored_active':
      return language === 'zh'
        ? '系统已自动将 exhausted Key 恢复为 active'
        : 'The system automatically restored this exhausted key to active'
    case 'cleared_quarantine':
      return language === 'zh' ? '管理员已解除该 Key 的隔离' : 'An admin cleared the quarantine on this key'
    case 'none':
      return language === 'zh' ? '无自动状态变更' : 'No automatic key state change'
    default:
      return summary && summary.length > 0 ? summary : language === 'zh' ? '无自动状态变更' : 'No automatic key state change'
  }
}

function tokenKeyEffectTone(code: string | null | undefined): StatusTone {
  switch ((code ?? '').trim()) {
    case 'quarantined':
      return 'error'
    case 'marked_exhausted':
      return 'warning'
    case 'restored_active':
    case 'cleared_quarantine':
      return 'success'
    default:
      return 'neutral'
  }
}

function tokenKeyEffectBadgeLabel(log: TokenLog, language: 'en' | 'zh'): string {
  switch ((log.key_effect_code ?? '').trim()) {
    case 'quarantined':
      return language === 'zh' ? '已隔离' : 'Quarantined'
    case 'marked_exhausted':
      return language === 'zh' ? '已耗尽' : 'Exhausted'
    case 'restored_active':
      return language === 'zh' ? '已恢复' : 'Restored'
    case 'cleared_quarantine':
      return language === 'zh' ? '已清除' : 'Cleared'
    case 'none':
    case '':
      return language === 'zh' ? '无变更' : 'No Change'
    default:
      return language === 'zh' ? '已更新' : 'Updated'
  }
}

function tokenOwnerPrimary(owner: TokenOwnerSummary | null): string {
  if (!owner) return ''
  return owner.displayName || owner.userId
}

function tokenOwnerSecondary(owner: TokenOwnerSummary | null): string | null {
  if (!owner?.username) return null
  return `@${owner.username}`
}

function TokenOwnerValue({
  owner,
  emptyLabel,
  onOpenUser,
}: {
  owner: TokenOwnerSummary | null
  emptyLabel: string
  onOpenUser?: (userId: string) => void
}): JSX.Element {
  if (!owner) {
    return <span className="token-owner-empty">{emptyLabel}</span>
  }

  const secondary = tokenOwnerSecondary(owner)
  return (
    <div className="token-owner-block">
      {onOpenUser ? (
        <button type="button" className="link-button token-owner-trigger" onClick={() => onOpenUser(owner.userId)}>
          <span className="token-owner-link">{tokenOwnerPrimary(owner)}</span>
          {secondary ? <span className="token-owner-secondary">{secondary}</span> : null}
        </button>
      ) : (
        <>
          <span className="token-owner-link">{tokenOwnerPrimary(owner)}</span>
          {secondary ? <span className="token-owner-secondary">{secondary}</span> : null}
        </>
      )}
    </div>
  )
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

export default function TokenDetail({
  id,
  onBack,
  onOpenUser,
  onSecretRotated,
}: {
  id: string
  onBack?: () => void
  onOpenUser?: (userId: string) => void
  onSecretRotated?: (id: string, token: string) => void
}): JSX.Element {
  const translations = useTranslate()
  const { language } = useLanguage()
  const tokenStrings = translations.admin.tokens
  const loadingStateStrings = translations.admin.loadingStates
  const pageRef = useRef<HTMLDivElement>(null)
  const { viewportMode, contentMode, isCompactLayout } = useResponsiveModes(pageRef)
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
  const [requestKindOptions, setRequestKindOptions] = useState<TokenLogRequestKindOption[]>([])
  const [requestKindOptionsByKey, setRequestKindOptionsByKey] = useState<Record<string, TokenLogRequestKindOption>>({})
  const [selectedRequestKinds, setSelectedRequestKinds] = useState<string[]>([])
  const [requestKindQuickBilling, setRequestKindQuickBilling] = useState<TokenLogRequestKindQuickBilling>('all')
  const [requestKindQuickProtocol, setRequestKindQuickProtocol] = useState<TokenLogRequestKindQuickProtocol>('all')
  const [operationalClassFilter, setOperationalClassFilter] =
    useState<TokenLogOperationalClassFilter>('all')
  const [summaryLoadState, setSummaryLoadState] = useState<QueryLoadState>('initial_loading')
  const [logsLoadState, setLogsLoadState] = useState<QueryLoadState>('initial_loading')
  const [quickUsage, setQuickUsage] = useState<UsageBar[]>([])
  const [quickUsageLoading, setQuickUsageLoading] = useState(true)
  const [snapshotUsage, setSnapshotUsage] = useState<UsageBar[]>([])
  const [snapshotUsageLoading, setSnapshotUsageLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [warning, setWarning] = useState<string | null>(null)
  const sseRef = useRef<EventSource | null>(null)
  const [expandedLogs, setExpandedLogs] = useState<Set<number>>(() => new Set())
  const warningTimerRef = useRef<number | null>(null)
  const sinceDebounceRef = useRef<number | null>(null)
  const [isRotateDialogOpen, setIsRotateDialogOpen] = useState(false)
  const [isRotatedDialogOpen, setIsRotatedDialogOpen] = useState(false)
  const [rotating, setRotating] = useState(false)
  const [rotatedToken, setRotatedToken] = useState<string | null>(null)
  const [rotatedCopyState, setRotatedCopyState] = useState<'idle' | 'copied' | 'error'>('idle')
  const [sseConnected, setSseConnected] = useState(false)
  const [logsContentMinHeight, setLogsContentMinHeight] = useState(320)
  const perPageRef = useRef(20)
  const logsContentRef = useRef<HTMLDivElement | null>(null)
  const quickUsageAbortRef = useRef<AbortController | null>(null)
  const snapshotUsageAbortRef = useRef<AbortController | null>(null)
  const detailAbortRef = useRef<AbortController | null>(null)
  const logsAbortRef = useRef<AbortController | null>(null)
  const requestKindOptionsAbortRef = useRef<AbortController | null>(null)
  const summaryQueryKeyRef = useRef<string | null>(null)
  const logsQueryKeyRef = useRef<string | null>(null)
  const rotatedTokenFieldRef = useRef<HTMLTextAreaElement | null>(null)
  const logsQueryBaseKeyRef = useRef<string>('')
  const logsRequestContextRef = useRef<{
    tokenId: string
    sinceIso: string
    untilIso: string
    requestKinds: string[]
    forceEmptyMatch: boolean
    operationalClass: TokenLogOperationalClassFilter
  }>({
    tokenId: id,
    sinceIso: '',
    untilIso: '',
    requestKinds: [],
    forceEmptyMatch: false,
    operationalClass: 'all',
  })
  const requestKindRefreshContextRef = useRef<{
    selectedRequestKindsNormalized: string[]
    requestKindQuickFilters: {
      billing: TokenLogRequestKindQuickBilling
      protocol: TokenLogRequestKindQuickProtocol
    }
    effectiveSelectedRequestKinds: string[]
    hasQuickRequestKindEmptyMatch: boolean
  }>({
    selectedRequestKindsNormalized: [],
    requestKindQuickFilters: {
      billing: 'all',
      protocol: 'all',
    },
    effectiveSelectedRequestKinds: [],
    hasQuickRequestKindEmptyMatch: false,
  })

  useEffect(() => {
    setInfo(null)
    setSummary(null)
    setQuickStats({ day: null, month: null, total: null })
    setLogs([])
    setPage(1)
    setTotal(0)
    setRequestKindOptions([])
    setRequestKindOptionsByKey({})
    setSelectedRequestKinds([])
    setRequestKindQuickBilling('all')
    setRequestKindQuickProtocol('all')
    setOperationalClassFilter('all')
    setWarning(null)
    setQuickUsage([])
    setQuickUsageLoading(true)
    setSnapshotUsage([])
    setSnapshotUsageLoading(true)
    setLogsContentMinHeight(320)
    setSummaryLoadState('initial_loading')
    setLogsLoadState('initial_loading')
    summaryQueryKeyRef.current = null
    logsQueryKeyRef.current = null
  }, [id])

  useEffect(() => {
    perPageRef.current = perPage
  }, [perPage])

  useEffect(() => {
    if (!isRotatedDialogOpen || !rotatedToken) return
    const frame = window.requestAnimationFrame(() => {
      selectAllReadonlyText(rotatedTokenFieldRef.current)
    })
    return () => window.cancelAnimationFrame(frame)
  }, [isRotatedDialogOpen, rotatedToken])

  const { sinceIso, untilIso } = useMemo(() => {
    const start = computeStartDate(period, debouncedSinceInput)
    const end = computeEndDate(period, start)
    return { sinceIso: toIso(start), untilIso: toIso(end) }
  }, [period, debouncedSinceInput])
  const summaryBlocking = isBlockingLoadState(summaryLoadState)
  const summaryRefreshing = isRefreshingLoadState(summaryLoadState)
  const logsBlocking = isBlockingLoadState(logsLoadState)
  const logsRefreshing = isRefreshingLoadState(logsLoadState)
  const filterControlsDisabled = summaryBlocking || logsBlocking
  const infoRegionLoadState: QueryLoadState = info
    ? (summaryRefreshing ? 'refreshing' : 'ready')
    : summaryLoadState

  const periodSelectId = `token-period-select-${id}`
  const sinceInputId = `token-since-input-${id}`
  const selectedRequestKindsNormalized = useMemo(
    () => uniqueSelectedRequestKinds(selectedRequestKinds),
    [selectedRequestKinds],
  )
  const requestKindQuickFilters = useMemo(
    () => ({
      billing: requestKindQuickBilling,
      protocol: requestKindQuickProtocol,
    }),
    [requestKindQuickBilling, requestKindQuickProtocol],
  )
  const hasActiveQuickRequestKindFilters = useMemo(
    () => hasActiveRequestKindQuickFilters(requestKindQuickFilters),
    [requestKindQuickFilters],
  )
  const requestKindQuickSelection = useMemo(
    () => buildRequestKindQuickFilterSelection(requestKindOptions, requestKindQuickFilters),
    [requestKindOptions, requestKindQuickFilters],
  )
  const effectiveSelectedRequestKinds = useMemo(
    () =>
      resolveEffectiveRequestKindSelection(
        selectedRequestKindsNormalized,
        requestKindQuickFilters,
        requestKindQuickSelection,
      ),
    [requestKindQuickFilters, requestKindQuickSelection, selectedRequestKindsNormalized],
  )
  const hasQuickRequestKindEmptyMatch = useMemo(
    () => hasActiveQuickRequestKindFilters && requestKindQuickSelection.length === 0,
    [hasActiveQuickRequestKindFilters, requestKindQuickSelection.length],
  )
  const visibleRequestKindOptions = useMemo(
    () =>
      buildVisibleRequestKindOptions(
        effectiveSelectedRequestKinds,
        requestKindOptions,
        requestKindOptionsByKey,
      ),
    [effectiveSelectedRequestKinds, requestKindOptionsByKey, requestKindOptions],
  )
  const requestKindSummary = useMemo(
    () => summarizeSelectedRequestKinds(effectiveSelectedRequestKinds, visibleRequestKindOptions),
    [effectiveSelectedRequestKinds, visibleRequestKindOptions],
  )
  const requestKindQuickSummary = useMemo(
    () => summarizeRequestKindQuickFilters(requestKindQuickFilters),
    [requestKindQuickFilters],
  )
  const requestKindTriggerSummary = useMemo(() => {
    if (hasActiveQuickRequestKindFilters) return requestKindQuickSummary
    if (effectiveSelectedRequestKinds.length === 0) return 'All request types'
    if (effectiveSelectedRequestKinds.length <= 2) return requestKindSummary
    return 'Request types'
  }, [
    effectiveSelectedRequestKinds.length,
    hasActiveQuickRequestKindFilters,
    requestKindQuickSummary,
    requestKindSummary,
  ])
  const summaryQueryBaseKey = useMemo(
    () => `${id}:${period}:${sinceIso}:${untilIso}`,
    [id, period, sinceIso, untilIso],
  )
  const logsQueryBaseKey = useMemo(
    () =>
      `${summaryQueryBaseKey}:op=${operationalClassFilter}:quick=${requestKindQuickBilling}:${requestKindQuickProtocol}:requestKinds=${selectedRequestKindsNormalized.join(',')}`,
    [
      operationalClassFilter,
      requestKindQuickBilling,
      requestKindQuickProtocol,
      selectedRequestKindsNormalized,
      summaryQueryBaseKey,
    ],
  )
  logsRequestContextRef.current = {
    tokenId: id,
    sinceIso,
    untilIso,
    requestKinds: effectiveSelectedRequestKinds,
    forceEmptyMatch: hasQuickRequestKindEmptyMatch,
    operationalClass: operationalClassFilter,
  }
  requestKindRefreshContextRef.current = {
    selectedRequestKindsNormalized,
    requestKindQuickFilters,
    effectiveSelectedRequestKinds,
    hasQuickRequestKindEmptyMatch,
  }

  useEffect(() => {
    const preservedHeight = Math.max(320, logsContentRef.current?.offsetHeight ?? 0)
    setLogsContentMinHeight(preservedHeight)
  }, [logsQueryBaseKey, page, perPage])

  useEffect(() => {
    logsQueryBaseKeyRef.current = logsQueryBaseKey
  }, [logsQueryBaseKey])

  useEffect(() => {
    // Page > 1 responses re-sync active quick presets inside the paginated fetch path
    // so we do not bounce the user back to page 1 here.
    if (page !== 1 || !hasActiveQuickRequestKindFilters) return
    if (requestKindSelectionsMatch(selectedRequestKindsNormalized, requestKindQuickSelection)) return
    setSelectedRequestKinds(requestKindQuickSelection)
    setExpandedLogs(new Set())
  }, [
    hasActiveQuickRequestKindFilters,
    page,
    requestKindQuickSelection,
    selectedRequestKindsNormalized,
  ])

  useLayoutEffect(() => {
    if (logsBlocking) return
    const nextHeight = Math.max(320, logsContentRef.current?.offsetHeight ?? 0)
    setLogsContentMinHeight(nextHeight)
  }, [logsBlocking, logs, expandedLogs, viewportMode, contentMode, isCompactLayout])

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

  async function getJson<T = any>(url: string, signal?: AbortSignal): Promise<T> {
    const res = await fetch(url, { signal })
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

  const buildLogsPageUrl = useCallback(
    (nextPage: number, nextPerPage = perPageRef.current) => {
      const { tokenId, sinceIso, untilIso, requestKinds, forceEmptyMatch, operationalClass } =
        logsRequestContextRef.current
      return buildTokenLogsPagePath({
        tokenId,
        page: nextPage,
        perPage: nextPerPage,
        sinceIso,
        untilIso,
        forceEmptyMatch,
        requestKinds,
        operationalClass,
      })
    },
    [],
  )
  const buildLogsPageUrlForSelection = useCallback(
    (
      nextPage: number,
      requestKinds: string[],
      forceEmptyMatch: boolean,
      nextPerPage = perPageRef.current,
    ) => {
      const { tokenId, sinceIso, untilIso, operationalClass } = logsRequestContextRef.current
      return buildTokenLogsPagePath({
        tokenId,
        page: nextPage,
        perPage: nextPerPage,
        sinceIso,
        untilIso,
        forceEmptyMatch,
        requestKinds,
        operationalClass,
      })
    },
    [],
  )

  const syncRequestKindState = useCallback(
    (nextOptions: TokenLogRequestKindOption[]) => {
      setRequestKindOptions(nextOptions)
      setRequestKindOptionsByKey((prev) => mergeRequestKindOptionsByKey(prev, nextOptions))
    },
    [],
  )

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

  useEffect(() => () => {
    detailAbortRef.current?.abort()
    logsAbortRef.current?.abort()
    requestKindOptionsAbortRef.current?.abort()
  }, [])

  // load detail + summary when the time window changes
  useEffect(() => {
    detailAbortRef.current?.abort()
    const detailController = new AbortController()
    detailAbortRef.current = detailController
    const nextQueryKey = summaryQueryBaseKey
    setSummaryLoadState(getBlockingLoadState(summaryQueryKeyRef.current != null))
    setSummary(null)
    setError(null)
    const run = async () => {
      try {
        const [detailRes, summaryRes] = await Promise.all([
          getJson(`/api/tokens/${encodeURIComponent(id)}`, detailController.signal),
          getJson(`/api/tokens/${encodeURIComponent(id)}/metrics?period=${period}&since=${encodeURIComponent(sinceIso)}&until=${encodeURIComponent(untilIso)}`, detailController.signal),
        ])
        if (detailController.signal.aborted) return
        setInfo(detailRes)
        setSummary(summaryRes)
        setError(null)
        setSummaryLoadState('ready')
        summaryQueryKeyRef.current = nextQueryKey
        void loadQuickStats()
      } catch (e) {
        if ((e as Error).name === 'AbortError') return
        setError(e instanceof Error ? e.message : 'Failed to load token details')
        setSummaryLoadState('error')
      }
    }
    void run()
    return () => {
      detailController.abort()
    }
  }, [id, period, sinceIso, summaryQueryBaseKey, untilIso])

  // load first-page logs when the time window or request-type filter changes
  useEffect(() => {
    logsAbortRef.current?.abort()
    requestKindOptionsAbortRef.current?.abort()
    const logsController = new AbortController()
    logsAbortRef.current = logsController
    setLogsLoadState(getBlockingLoadState(logsQueryKeyRef.current != null))
    setLogs([])
    setPage(1)
    setTotal(0)
    setExpandedLogs(new Set())
    setError(null)
    const run = async () => {
      try {
        const logsRes = await getJson<TokenLogsPageResponse>(buildLogsPageUrl(1), logsController.signal)
        if (logsController.signal.aborted) return
        const resolvedPerPage = logsRes.per_page ?? logsRes.perPage ?? perPageRef.current
        setLogs(logsRes.items)
        setPage(1)
        setPerPage(resolvedPerPage)
        setTotal(logsRes.total)
        syncRequestKindState(logsRes.request_kind_options ?? [])
        setExpandedLogs(new Set())
        setError(null)
        setLogsLoadState('ready')
        logsQueryKeyRef.current = `${logsQueryBaseKey}:page=1:perPage=${resolvedPerPage}`
      } catch (e) {
        if ((e as Error).name === 'AbortError') return
        setError(e instanceof Error ? e.message : 'Failed to load request records')
        setLogsLoadState('error')
      }
    }
    void run()
    return () => {
      logsController.abort()
    }
  }, [buildLogsPageUrl, logsQueryBaseKey, syncRequestKindState])

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
      logsAbortRef.current?.abort()
      requestKindOptionsAbortRef.current?.abort()
      const controller = new AbortController()
      logsAbortRef.current = controller
      setLogsLoadState(getRefreshingLoadState(logsQueryKeyRef.current != null))
      try {
        const data = await getJson<TokenLogsPageResponse>(buildLogsPageUrl(1), controller.signal)
        if (controller.signal.aborted) return
        const resolvedPerPage = data.per_page ?? data.perPage ?? perPageRef.current
        setLogs(data.items)
        setTotal(data.total)
        setPerPage(resolvedPerPage)
        syncRequestKindState(data.request_kind_options ?? [])
        setPage(1)
        setExpandedLogs(new Set())
        setLogsLoadState('ready')
        logsQueryKeyRef.current = `${logsQueryBaseKey}:page=1:perPage=${resolvedPerPage}`
      } catch {
        if (!controller.signal.aborted) {
          setLogsLoadState('error')
        }
        // ignore
      }
    }
    const refreshRequestKindOptions = async () => {
      requestKindOptionsAbortRef.current?.abort()
      const controller = new AbortController()
      requestKindOptionsAbortRef.current = controller
      const requestQueryBaseKey = logsQueryBaseKeyRef.current
      try {
        const data = await getJson<TokenLogsPageResponse>(buildLogsPageUrl(1), controller.signal)
        if (controller.signal.aborted || logsQueryBaseKeyRef.current !== requestQueryBaseKey) return
        const nextOptions = data.request_kind_options ?? []
        const refreshContext = requestKindRefreshContextRef.current
        const refreshedSelection = resolveRequestKindOptionsRefresh(
          nextOptions,
          refreshContext.selectedRequestKindsNormalized,
          refreshContext.requestKindQuickFilters,
          refreshContext.effectiveSelectedRequestKinds,
          refreshContext.hasQuickRequestKindEmptyMatch,
        )
        if (refreshedSelection.selectionChanged) {
          logsAbortRef.current?.abort()
          const logsController = new AbortController()
          logsAbortRef.current = logsController
          setLogsLoadState(getRefreshingLoadState(logsQueryKeyRef.current != null))

          const loadRefreshedPage = async (nextPage: number, nextPerPage = perPageRef.current) =>
            getJson<TokenLogsPageResponse>(
              buildLogsPageUrlForSelection(
                nextPage,
                refreshedSelection.effectiveSelection,
                refreshedSelection.hasEmptyMatch,
                nextPerPage,
              ),
              logsController.signal,
            )

          const resolvedPage = Math.max(1, page)
          let refreshedPage = await loadRefreshedPage(resolvedPage)
          if (logsController.signal.aborted || logsQueryBaseKeyRef.current !== requestQueryBaseKey) return

          const resolvedPerPage = refreshedPage.per_page ?? refreshedPage.perPage ?? perPageRef.current
          const pageCount = Math.max(1, Math.ceil(refreshedPage.total / resolvedPerPage) || 1)
          const clampedPage = Math.min(resolvedPage, pageCount)

          if (clampedPage !== resolvedPage) {
            refreshedPage = await loadRefreshedPage(clampedPage, resolvedPerPage)
            if (logsController.signal.aborted || logsQueryBaseKeyRef.current !== requestQueryBaseKey) return
          }

          const finalPerPage = refreshedPage.per_page ?? refreshedPage.perPage ?? resolvedPerPage
          const finalPageCount = Math.max(1, Math.ceil(refreshedPage.total / finalPerPage) || 1)
          const finalPage = Math.min(clampedPage, finalPageCount)
          setLogs(refreshedPage.items)
          setPage(finalPage)
          setPerPage(finalPerPage)
          setTotal(refreshedPage.total)
          syncRequestKindState(refreshedPage.request_kind_options ?? nextOptions)
          setExpandedLogs(new Set())
          setLogsLoadState('ready')
          logsQueryKeyRef.current = `${requestQueryBaseKey}:page=${finalPage}:perPage=${finalPerPage}`
          return
        }

        setTotal(data.total)
        setPerPage(data.per_page ?? data.perPage ?? perPageRef.current)
        syncRequestKindState(nextOptions)
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
          setSummaryLoadState(getRefreshingLoadState(summaryQueryKeyRef.current != null))
          setSummary(data.summary)
          setSummaryLoadState('ready')
        }
        void refreshDetail()
        if (page === 1) {
          void refreshLogs()
        } else {
          void refreshRequestKindOptions()
        }
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
  }, [
    buildLogsPageUrl,
    buildLogsPageUrlForSelection,
    debouncedSinceInput,
    id,
    logsQueryBaseKey,
    page,
    period,
    refreshQuickUsage,
    refreshSnapshotUsage,
    sinceIso,
    syncRequestKindState,
    untilIso,
  ])

  useEffect(() => {
    ;(window as typeof window & { __TOKEN_PERIOD__?: Period }).__TOKEN_PERIOD__ = period
  }, [period])

  const goToPage = async (next: number, nextPerPage = perPage) => {
    const pageCount = Math.max(1, Math.ceil(total / nextPerPage) || 1)
    const p = Math.max(1, Math.min(next, pageCount))
    logsAbortRef.current?.abort()
    requestKindOptionsAbortRef.current?.abort()
    const controller = new AbortController()
    logsAbortRef.current = controller
    setLogsLoadState(getBlockingLoadState(logsQueryKeyRef.current != null))
    setLogs([])
    setPage(p)
    setExpandedLogs(new Set())
    setError(null)
    const requestQueryBaseKey = logsQueryBaseKeyRef.current
    try {
      const data = await getJson<TokenLogsPageResponse>(buildLogsPageUrl(p, nextPerPage), controller.signal)
      if (controller.signal.aborted || logsQueryBaseKeyRef.current !== requestQueryBaseKey) return
      const nextOptions = data.request_kind_options ?? []
      const refreshContext = requestKindRefreshContextRef.current
      const refreshedSelection = resolveRequestKindOptionsRefresh(
        nextOptions,
        refreshContext.selectedRequestKindsNormalized,
        refreshContext.requestKindQuickFilters,
        refreshContext.effectiveSelectedRequestKinds,
        refreshContext.hasQuickRequestKindEmptyMatch,
      )
      if (refreshedSelection.selectionChanged) {
        const loadRefreshedPage = async (nextPage: number, pagePerPage = nextPerPage) =>
          getJson<TokenLogsPageResponse>(
            buildLogsPageUrlForSelection(
              nextPage,
              refreshedSelection.effectiveSelection,
              refreshedSelection.hasEmptyMatch,
              pagePerPage,
            ),
            controller.signal,
          )

        const requestedPage = Math.max(1, p)
        let refreshedPage = await loadRefreshedPage(requestedPage)
        if (controller.signal.aborted || logsQueryBaseKeyRef.current !== requestQueryBaseKey) return

        const resolvedPerPage = refreshedPage.per_page ?? refreshedPage.perPage ?? nextPerPage
        const refreshedPageCount = Math.max(1, Math.ceil(refreshedPage.total / resolvedPerPage) || 1)
        const clampedPage = Math.min(requestedPage, refreshedPageCount)

        if (clampedPage !== requestedPage) {
          refreshedPage = await loadRefreshedPage(clampedPage, resolvedPerPage)
          if (controller.signal.aborted || logsQueryBaseKeyRef.current !== requestQueryBaseKey) return
        }

        const finalPerPage = refreshedPage.per_page ?? refreshedPage.perPage ?? resolvedPerPage
        const finalPageCount = Math.max(1, Math.ceil(refreshedPage.total / finalPerPage) || 1)
        const finalPage = Math.min(clampedPage, finalPageCount)
        setLogs(refreshedPage.items)
        setPage(finalPage)
        setPerPage(finalPerPage)
        setTotal(refreshedPage.total)
        syncRequestKindState(refreshedPage.request_kind_options ?? nextOptions)
        setExpandedLogs(new Set())
        setLogsLoadState('ready')
        logsQueryKeyRef.current = `${requestQueryBaseKey}:page=${finalPage}:perPage=${finalPerPage}`
        return
      }

      const resolvedPerPage = data.per_page ?? data.perPage ?? nextPerPage
      setLogs(data.items)
      setPage(data.page)
      setPerPage(resolvedPerPage)
      setTotal(data.total)
      syncRequestKindState(nextOptions)
      setExpandedLogs(new Set())
      setLogsLoadState('ready')
      logsQueryKeyRef.current = `${requestQueryBaseKey}:page=${data.page}:perPage=${resolvedPerPage}`
    } catch (e) {
      if ((e as Error).name === 'AbortError') return
      setError(e instanceof Error ? e.message : 'Failed to load page')
      setLogsLoadState('error')
    }
  }

  const changePerPage = async (nextPerPage: number) => {
    logsAbortRef.current?.abort()
    requestKindOptionsAbortRef.current?.abort()
    const controller = new AbortController()
    logsAbortRef.current = controller
    setLogsLoadState(getBlockingLoadState(logsQueryKeyRef.current != null))
    setLogs([])
    setPerPage(nextPerPage)
    setPage(1)
    setExpandedLogs(new Set())
    setError(null)
    try {
      const data = await getJson<TokenLogsPageResponse>(buildLogsPageUrl(1, nextPerPage), controller.signal)
      if (controller.signal.aborted) return
      const resolvedPerPage = data.per_page ?? data.perPage ?? nextPerPage
      setLogs(data.items)
      setPage(1)
      setPerPage(resolvedPerPage)
      setTotal(data.total)
      syncRequestKindState(data.request_kind_options ?? [])
      setExpandedLogs(new Set())
      setLogsLoadState('ready')
      logsQueryKeyRef.current = `${logsQueryBaseKey}:page=1:perPage=${resolvedPerPage}`
    } catch (e) {
      if ((e as Error).name === 'AbortError') return
      setError(e instanceof Error ? e.message : 'Failed to load page size')
      setLogsLoadState('error')
    }
  }

  const applyRequestKindQuickFilters = useCallback(
    (nextBilling: TokenLogRequestKindQuickBilling, nextProtocol: TokenLogRequestKindQuickProtocol) => {
      const nextFilters = {
        billing: nextBilling,
        protocol: nextProtocol,
      }
      setRequestKindQuickBilling(nextBilling)
      setRequestKindQuickProtocol(nextProtocol)
      setSelectedRequestKinds(buildRequestKindQuickFilterSelection(requestKindOptions, nextFilters))
      setPage(1)
      setExpandedLogs(new Set())
    },
    [requestKindOptions],
  )

  const handleToggleRequestKind = useCallback(
    (key: string) => {
      const nextSelected = toggleRequestKindSelection(effectiveSelectedRequestKinds, key)
      const nextQuickFilters = resolveManualRequestKindQuickFilters(
        nextSelected,
        requestKindQuickFilters,
        requestKindQuickSelection,
        requestKindOptions,
      )
      setSelectedRequestKinds(nextSelected)
      setRequestKindQuickBilling(nextQuickFilters.billing)
      setRequestKindQuickProtocol(nextQuickFilters.protocol)
      setPage(1)
      setExpandedLogs(new Set())
    },
    [
      effectiveSelectedRequestKinds,
      requestKindOptions,
      requestKindQuickFilters,
      requestKindQuickSelection,
    ],
  )

  const handleClearRequestKinds = useCallback(() => {
    setSelectedRequestKinds([])
    setRequestKindQuickBilling(defaultTokenLogRequestKindQuickFilters.billing)
    setRequestKindQuickProtocol(defaultTokenLogRequestKindQuickFilters.protocol)
    setPage(1)
    setExpandedLogs(new Set())
  }, [])

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

  const handleRotateToken = useCallback(async () => {
    try {
      setRotating(true)
      const res = await rotateTokenSecret(id)
      setRotatedToken(res.token)
      onSecretRotated?.(id, res.token)
      const copyResult = await copyText(res.token)
      setRotatedCopyState(copyResult.ok ? 'copied' : 'error')
      setIsRotateDialogOpen(false)
      setIsRotatedDialogOpen(true)
    } catch (e) {
      setIsRotateDialogOpen(false)
      alert((e as Error)?.message || 'Failed to regenerate token secret')
    } finally {
      setRotating(false)
    }
  }, [id, onSecretRotated])

  const handleCopyRotatedToken = useCallback(async () => {
    if (!rotatedToken) return
    const copyResult = await copyText(rotatedToken, { preferExecCommand: true })
    setRotatedCopyState(copyResult.ok ? 'copied' : 'error')
    if (!copyResult.ok) {
      window.requestAnimationFrame(() => {
        selectAllReadonlyText(rotatedTokenFieldRef.current)
      })
    }
  }, [rotatedToken])

  return (
    <div
      ref={pageRef}
      className={`admin-detail-stack viewport-${viewportMode} content-${contentMode}${
        isCompactLayout ? ' is-compact-layout' : ''
      }`}
    >
      <section className="surface app-header">
        <div className="title-group">
          <h1>Access Token Detail</h1>
          <div className="subtitle">Token <code>{id}</code></div>
        </div>
        <div className="controls token-detail-controls">
          <ThemeToggle />
          <AdminReturnToConsoleLink
            label={translations.admin.header.returnToConsole}
            href={ADMIN_USER_CONSOLE_HREF}
            className="admin-return-link--detail"
          />
          <span className={`sse-chip ${sseConnected ? 'sse-chip-ok' : 'sse-chip-warn'}`} title="Live updates via SSE">
            <span className="sse-dot" aria-hidden="true" /> {sseConnected ? 'Live' : 'Offline'}
          </span>
          <Button type="button" variant="outline" onClick={() => (onBack ? onBack() : window.history.back())}>
            <Icon icon="mdi:arrow-left" width={18} height={18} />
            Back
          </Button>
          <Button
            type="button"
            variant="warning"
            onClick={() => setIsRotateDialogOpen(true)}
            aria-label="Regenerate secret"
          >
            <Icon icon="mdi:key-change" width={18} height={18} />
            Regenerate Secret
          </Button>
        </div>
      </section>

      {error && <div className="surface error-banner" role="alert">{error}</div>}

      <section className="surface panel token-info-section">
        <AdminLoadingRegion
          loadState={infoRegionLoadState}
          loadingLabel={summaryRefreshing ? loadingStateStrings.refreshing : loadingStateStrings.switching}
          minHeight={184}
        >
          {info ? (
            <div className="token-info-grid" aria-label="Token metadata">
              <InfoCard
                label="Token ID"
                value={<code className="code-chip" title={info.id}>{info.id}</code>}
              />
              <InfoCard
                label="Status"
                value={
                  <StatusBadge tone={info.enabled ? 'success' : 'error'}>
                    {info.enabled ? 'Enabled' : 'Disabled'}
                  </StatusBadge>
                }
              />
              <InfoCard label="Total Requests" value={formatNumber(info.total_requests)} />
              <InfoCard label="Created" value={formatTime(info.created_at)} />
              <InfoCard label="Last Used" value={formatTime(info.last_used_at)} />
              <InfoCard
                label={tokenStrings.owner.label}
                value={<TokenOwnerValue owner={info.owner ?? null} emptyLabel={tokenStrings.owner.unbound} onOpenUser={onOpenUser} />}
              />
              <InfoCard
                label="Note"
                value={info.note ? <span className="token-info-note" title={info.note}>{info.note}</span> : '—'}
              />
            </div>
          ) : (
            <div className="empty-state alert">Token details are unavailable right now.</div>
          )}
        </AdminLoadingRegion>
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>Quick Stats</h2>
            <p className="panel-description">Rolling usage windows (1 hour / 24 hours / calendar month).</p>
          </div>
        </div>
        <AdminLoadingRegion
          loadState={infoRegionLoadState}
          loadingLabel={summaryRefreshing ? loadingStateStrings.refreshing : loadingStateStrings.switching}
          minHeight={176}
        >
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
              <div className="empty-state alert" style={{ gridColumn: '1 / -1' }}>
                Token quota details are unavailable right now.
              </div>
            )}
          </section>
        </AdminLoadingRegion>
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
              <Select value={period} onValueChange={(value) => { const next = value as Period; setPeriod(next); applyStartInput('', next) }} disabled={filterControlsDisabled}>
                <SelectTrigger id={periodSelectId} disabled={filterControlsDisabled}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent align="start">
                  <SelectItem value="day">Day</SelectItem>
                  <SelectItem value="week">Week</SelectItem>
                  <SelectItem value="month">Month</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="token-period-control">
              <label htmlFor={sinceInputId}>Start</label>
              {period === 'day' && (
                <Input
                  id={sinceInputId}
                  type="date"
                  max={defaultInputValue('day')}
                  value={sinceInput}
                  disabled={filterControlsDisabled}
                  onChange={(e) => handleStartChange(period, e.target.value)}
                />
              )}
              {period === 'week' && (
                <Input
                  id={sinceInputId}
                  type="week"
                  max={defaultInputValue('week')}
                  value={sinceInput}
                  disabled={filterControlsDisabled}
                  onChange={(e) => handleStartChange(period, e.target.value)}
                />
              )}
              {period === 'month' && (
                <Input
                  id={sinceInputId}
                  type="month"
                  max={defaultInputValue('month')}
                  value={sinceInput}
                  disabled={filterControlsDisabled}
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
        <AdminLoadingRegion
          loadState={summaryLoadState}
          loadingLabel={summaryRefreshing ? loadingStateStrings.refreshing : loadingStateStrings.switching}
          minHeight={160}
        >
          <div className="token-stats">
            <MetricCard label="Requests" value={formatNumber(summary?.total_requests ?? 0)} />
            <MetricCard label="Success" value={formatNumber(summary?.success_count ?? 0)} />
            <MetricCard label="Errors" value={formatNumber(summary?.error_count ?? 0)} />
            <MetricCard label="Quota Exhausted" value={formatNumber(summary?.quota_exhausted_count ?? 0)} />
          </div>
        </AdminLoadingRegion>
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
        <div className="panel-header token-request-records-header">
          <div>
            <h2>Request Records</h2>
            <p className="panel-description">Newest entries first. Live refresh applies to the first page.</p>
          </div>
          <div className="token-request-records-actions">
            <Select
              value={operationalClassFilter}
              onValueChange={(value) => {
                setOperationalClassFilter(value as TokenLogOperationalClassFilter)
                setPage(1)
                setExpandedLogs(new Set())
              }}
              disabled={filterControlsDisabled}
            >
              <SelectTrigger className="w-[196px]" aria-label="Operational outcome filter">
                <SelectValue />
              </SelectTrigger>
              <SelectContent align="end">
                {operationalClassFilterOptions.map((option) => (
                  <SelectItem key={option.value} value={option.value}>
                    {option.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  type="button"
                  variant="outline"
                  className="token-request-kind-trigger"
                  aria-label={`Filter request types: ${requestKindTriggerSummary}`}
                >
                  <Icon icon="mdi:filter-variant" width={16} height={16} aria-hidden="true" />
                  <span className="token-request-kind-trigger-content">
                    <span className="token-request-kind-trigger-summary">{requestKindTriggerSummary}</span>
                  </span>
                  {effectiveSelectedRequestKinds.length > 0 ? (
                    <span className="token-request-kind-count">{effectiveSelectedRequestKinds.length}</span>
                  ) : null}
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="token-request-kind-menu w-80">
                <DropdownMenuLabel>Filter request types</DropdownMenuLabel>
                <div className="token-request-quick-filters">
                  <div className="token-request-quick-filter-row">
                    <span className="token-request-quick-filter-label">Billing</span>
                    <SegmentedTabs
                      value={requestKindQuickBilling}
                      onChange={(next) =>
                        applyRequestKindQuickFilters(next as TokenLogRequestKindQuickBilling, requestKindQuickProtocol)
                      }
                      options={requestKindBillingQuickFilterOptions}
                      ariaLabel="Billing request type filter"
                      className="token-request-quick-segmented"
                      disabled={filterControlsDisabled}
                    />
                  </div>
                  <div className="token-request-quick-filter-row">
                    <span className="token-request-quick-filter-label">Type</span>
                    <SegmentedTabs
                      value={requestKindQuickProtocol}
                      onChange={(next) =>
                        applyRequestKindQuickFilters(requestKindQuickBilling, next as TokenLogRequestKindQuickProtocol)
                      }
                      options={requestKindProtocolQuickFilterOptions}
                      ariaLabel="Protocol request type filter"
                      className="token-request-quick-segmented"
                      disabled={filterControlsDisabled}
                    />
                  </div>
                </div>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  className="cursor-pointer"
                  disabled={effectiveSelectedRequestKinds.length === 0 && !hasActiveQuickRequestKindFilters}
                  onSelect={(event) => {
                    event.preventDefault()
                    handleClearRequestKinds()
                  }}
                >
                  Show all request types
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                {visibleRequestKindOptions.length === 0 ? (
                  <DropdownMenuItem disabled>No request types in this window</DropdownMenuItem>
                ) : (
                  visibleRequestKindOptions.map((option) => (
                    <DropdownMenuCheckboxItem
                      key={option.key}
                      className="cursor-pointer"
                      checked={effectiveSelectedRequestKinds.includes(option.key)}
                      onSelect={(event) => event.preventDefault()}
                      onCheckedChange={() => handleToggleRequestKind(option.key)}
                    >
                      <RequestKindBadge requestKindKey={option.key} requestKindLabel={option.label} size="sm" />
                    </DropdownMenuCheckboxItem>
                  ))
                )}
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </div>
        <div ref={logsContentRef} style={{ minHeight: logsContentMinHeight }}>
          <AdminTableShell
            className="token-detail-md-up"
            tableClassName="token-detail-table token-detail-request-records-table"
            loadState={logsLoadState}
            loadingLabel={logsRefreshing ? loadingStateStrings.refreshing : loadingStateStrings.switching}
            minHeight={logsContentMinHeight}
          >
            <TableHeader>
              <TableRow>
                <TableHead>Time</TableHead>
                <TableHead>Request Type</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Charged Credits</TableHead>
                <TableHead>Result</TableHead>
                <TableHead>Key Effect</TableHead>
                <TableHead>Error</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {logs.map((l) => (
                <Fragment key={l.id}>
                  <TableRow>
                    <TableCell>{formatLogTime(l.created_at, period)}</TableCell>
                    <TableCell>
                      <RequestKindBadge
                        requestKindKey={l.request_kind_key}
                        requestKindLabel={l.request_kind_label}
                        size="sm"
                      />
                    </TableCell>
                    <TableCell>
                      <OverlayTooltip>
                        <TooltipTrigger asChild>
                          <button
                            type="button"
                            className="status-pair-trigger"
                            aria-label={formatTokenStatusTip(l.http_status, l.mcp_status)}
                          >
                            {formatTokenStatusPair(l.http_status, l.mcp_status)}
                          </button>
                        </TooltipTrigger>
                        <TooltipContent className="max-w-[min(18rem,calc(100vw-2rem))]" side="top">
                          {formatTokenStatusTip(l.http_status, l.mcp_status)}
                        </TooltipContent>
                      </OverlayTooltip>
                    </TableCell>
                    <TableCell>{formatChargedCredits(l.business_credits)}</TableCell>
                    <TableCell>
                      <Button
                        type="button"
                        variant="ghost"
                        className={`log-result-button${expandedLogs.has(l.id) ? ' log-result-button-active' : ''}`}
                        onClick={() => toggleLog(l.id)}
                        aria-expanded={expandedLogs.has(l.id)}
                        aria-controls={`token-log-details-${l.id}`}
                      >
                        <StatusBadge tone={statusTone(l.operationalClass)}>
                          {statusLabel(l.operationalClass, language)}
                        </StatusBadge>
                        <Icon
                          icon={expandedLogs.has(l.id) ? 'mdi:chevron-up' : 'mdi:chevron-down'}
                          width={18}
                          height={18}
                          className="log-result-icon"
                          aria-hidden="true"
                        />
                      </Button>
                    </TableCell>
                    <TableCell>
                      <StatusBadge
                        tone={tokenKeyEffectTone(l.key_effect_code)}
                        title={formatTokenKeyEffectSummary(l, language)}
                      >
                        {tokenKeyEffectBadgeLabel(l, language)}
                      </StatusBadge>
                    </TableCell>
                    <TableCell>{l.error_message ?? '—'}</TableCell>
                  </TableRow>
                  {expandedLogs.has(l.id) && (
                    <TableRow className="log-details-row">
                      <TableCell colSpan={7} id={`token-log-details-${l.id}`}>
                        <TokenLogDetails log={l} period={period} language={language} />
                      </TableCell>
                    </TableRow>
                  )}
                </Fragment>
              ))}
              {logs.length === 0 && (
                <TableRow>
                  <TableCell colSpan={7} style={{ padding: 12 }}>
                    <div className="empty-state alert" style={{ padding: 12 }}>No logs yet.</div>
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </AdminTableShell>
          <AdminLoadingRegion
            className="token-detail-mobile-list token-detail-md-down"
            loadState={logsLoadState}
            loadingLabel={logsRefreshing ? loadingStateStrings.refreshing : loadingStateStrings.switching}
            minHeight={Math.max(240, logsContentMinHeight)}
          >
            {logs.length === 0 ? (
              <div className="empty-state alert" style={{ padding: 12 }}>No logs yet.</div>
            ) : (
              logs.map((log) => (
                <article key={log.id} className="user-console-mobile-card">
                  <div className="user-console-mobile-kv">
                    <span>Time</span>
                    <strong>{formatLogTime(log.created_at, period)}</strong>
                  </div>
                  <div className="user-console-mobile-kv">
                    <span>Request</span>
                    <strong>{`${log.method} ${log.path}${log.query ? `?${log.query}` : ''}`}</strong>
                  </div>
                  <div className="user-console-mobile-kv">
                    <span>Request Type</span>
                    <RequestKindBadge
                      requestKindKey={log.request_kind_key}
                      requestKindLabel={log.request_kind_label}
                      size="sm"
                      className="user-console-mobile-request-kind"
                    />
                  </div>
                  {log.request_kind_detail ? (
                    <div className="user-console-mobile-kv user-console-mobile-kv--stacked">
                      <span>Request Type Detail</span>
                      <strong className="user-console-mobile-detail">{log.request_kind_detail}</strong>
                    </div>
                  ) : null}
                  <div className="user-console-mobile-kv">
                    <span>HTTP / Tavily</span>
                    <AnchoredInfoDisclosure
                      className="status-pair-trigger"
                      aria-label={formatTokenStatusTip(log.http_status, log.mcp_status)}
                      bubbleContent={formatTokenStatusTip(log.http_status, log.mcp_status)}
                    >
                      <strong>{formatTokenStatusPair(log.http_status, log.mcp_status)}</strong>
                    </AnchoredInfoDisclosure>
                  </div>
                  <div className="user-console-mobile-kv">
                    <span>Charged Credits</span>
                    <strong>{formatChargedCredits(log.business_credits)}</strong>
                  </div>
                  <div className="user-console-mobile-kv">
                    <span>Result</span>
                    <StatusBadge className="user-console-mobile-status" tone={statusTone(log.operationalClass)}>
                      {statusLabel(log.operationalClass, language)}
                    </StatusBadge>
                  </div>
                  <div className="user-console-mobile-kv user-console-mobile-kv--stacked">
                    <span>Key Effect</span>
                    <StatusBadge
                      className="user-console-mobile-status"
                      tone={tokenKeyEffectTone(log.key_effect_code)}
                      title={formatTokenKeyEffectSummary(log, language)}
                    >
                      {tokenKeyEffectBadgeLabel(log, language)}
                    </StatusBadge>
                  </div>
                  <div className="user-console-mobile-kv">
                    <span>Error</span>
                    <strong>{log.error_message ?? '—'}</strong>
                  </div>
                </article>
              ))
            )}
          </AdminLoadingRegion>
        </div>
        <AdminTablePagination
          page={page}
          totalPages={totalPages}
          perPage={perPage}
          onPerPageChange={(value) => {
            void changePerPage(value)
          }}
          disabled={logsBlocking}
          previousDisabled={page <= 1}
          nextDisabled={page >= totalPages}
          onPrevious={() => void goToPage(page - 1)}
          onNext={() => void goToPage(page + 1)}
        />
      </section>
    
    <Dialog open={isRotateDialogOpen} onOpenChange={setIsRotateDialogOpen}>
      <DialogContent className="sm:max-w-[480px]">
        <DialogHeader>
          <DialogTitle>Regenerate Token Secret</DialogTitle>
          <DialogDescription>
            This will invalidate the current token secret and generate a new one. The 4-char token ID will remain the same.
            Clients must be updated to use the new token.
          </DialogDescription>
        </DialogHeader>
        <div className="flex justify-end gap-2">
          <Button type="button" variant="outline" onClick={() => setIsRotateDialogOpen(false)}>
            Cancel
          </Button>
          <Button type="button" variant="warning" onClick={() => void handleRotateToken()} disabled={rotating}>
            {rotating ? 'Regenerating…' : 'Regenerate'}
          </Button>
        </div>
      </DialogContent>
    </Dialog>

    <Dialog open={isRotatedDialogOpen} onOpenChange={setIsRotatedDialogOpen}>
      <DialogContent className="sm:max-w-[520px]">
        <DialogHeader>
          <DialogTitle>New Token Generated</DialogTitle>
          <DialogDescription>
            {rotatedCopyState === 'error'
              ? 'Automatic copy was blocked. The full token is selected below for manual copy.'
              : 'Full token copied to clipboard:'}
          </DialogDescription>
        </DialogHeader>
        <Textarea
          ref={rotatedTokenFieldRef}
          readOnly
          rows={3}
          className="manual-copy-bubble-field min-h-[96px] resize-none font-mono text-xs"
          value={rotatedToken ?? '—'}
          onClick={(event) => selectAllReadonlyText(event.currentTarget)}
          onFocus={(event) => selectAllReadonlyText(event.currentTarget)}
        />
        <div className="flex justify-end gap-2">
          <Button type="button" variant="outline" onClick={() => setIsRotatedDialogOpen(false)}>
            Close
          </Button>
          <Button type="button" onClick={() => void handleCopyRotatedToken()}>
            {rotatedCopyState === 'copied' ? 'Copied' : rotatedCopyState === 'error' ? 'Copy Failed' : 'Copy'}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
    </div>
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

function formatChargedCredits(value: number | null): string {
  return value != null ? String(value) : '—'
}

function formatTokenStatusPair(httpStatus: number | null, mcpStatus: number | null): string {
  return `${httpStatus ?? '—'} / ${mcpStatus ?? '—'}`
}

function formatTokenStatusTip(httpStatus: number | null, mcpStatus: number | null): string {
  return `HTTP: ${httpStatus ?? '—'} · Tavily: ${mcpStatus ?? '—'}`
}

function formatRequestLine(log: TokenLog): string {
  const query = log.query ? `?${log.query}` : ''
  return `${log.method} ${log.path}${query}`
}

function TokenLogDetails({
  log,
  period,
  language,
}: {
  log: TokenLog
  period: Period
  language: 'en' | 'zh'
}) {
  const requestLine = formatRequestLine(log)
  const errorText = (log.error_message ?? '').trim() || 'No error reported.'
  const httpStatus = log.http_status != null ? `HTTP ${log.http_status}` : 'HTTP —'
  const tavilyStatus = log.mcp_status != null ? `Tavily ${log.mcp_status}` : 'Tavily —'
  const guidance = operationalClassGuidance(log.operationalClass, log.failure_kind, language)

  return (
    <div className="log-details-panel">
      <div className="log-details-summary">
        <div>
          <span className="log-details-label">Time</span>
          <span className="log-details-value">{formatLogTime(log.created_at, period)}</span>
        </div>
        <div>
          <span className="log-details-label">Status</span>
          <span className="log-details-value">{`${httpStatus} · ${tavilyStatus}`}</span>
        </div>
        <div>
          <span className="log-details-label">Charged Credits</span>
          <span className="log-details-value">{formatChargedCredits(log.business_credits)}</span>
        </div>
        <div>
          <span className="log-details-label">Request Type</span>
          <span className="log-details-value">
            <RequestKindBadge
              requestKindKey={log.request_kind_key}
              requestKindLabel={log.request_kind_label}
              size="sm"
            />
          </span>
        </div>
        <div>
          <span className="log-details-label">Outcome</span>
          <span className="log-details-value">{statusLabel(log.operationalClass, language)}</span>
        </div>
        <div>
          <span className="log-details-label">Key Effect</span>
          <span className="log-details-value">{formatTokenKeyEffectSummary(log, language)}</span>
        </div>
      </div>
      <div className="log-details-body">
        <div className="log-details-section">
          <header>Request</header>
          <pre>{requestLine}</pre>
        </div>
        {log.request_kind_detail ? (
          <div className="log-details-section">
            <header>Request Type Detail</header>
            <pre>{log.request_kind_detail}</pre>
          </div>
        ) : null}
        <div className="log-details-section">
          <header>Error Message</header>
          <pre>{errorText}</pre>
        </div>
        {guidance ? (
          <div className="log-details-section">
            <header>Suggested Handling</header>
            <pre>{guidance}</pre>
          </div>
        ) : null}
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
