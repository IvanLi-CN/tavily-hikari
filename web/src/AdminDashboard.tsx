import { Icon } from '@iconify/react'
import { StatusBadge, type StatusTone } from './components/StatusBadge'
import AdminTablePagination from './components/AdminTablePagination'
import AdminLoadingRegion from './components/AdminLoadingRegion'
import AdminTableShell from './components/AdminTableShell'
import { ApiKeysValidationDialog } from './components/ApiKeysValidationDialog'
import JobKeyLink from './components/JobKeyLink'
import ManualCopyBubble from './components/ManualCopyBubble'
import QuotaRangeField from './components/QuotaRangeField'
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import ThemeToggle from './components/ThemeToggle'
import AdminReturnToConsoleLink from './components/AdminReturnToConsoleLink'
import AdminPanelHeader from './components/AdminPanelHeader'
import { Button } from './components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from './components/ui/dialog'
import { Input } from './components/ui/input'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from './components/ui/dropdown-menu'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './components/ui/select'
import SegmentedTabs from './components/ui/SegmentedTabs'
import { Card } from './components/ui/card'
import { Badge } from './components/ui/badge'
import { Switch } from './components/ui/switch'
import { Table } from './components/ui/table'
import { Textarea } from './components/ui/textarea'
import TokenUsageHeader from './components/TokenUsageHeader'
import TokenDetail from './pages/TokenDetail'
import AdminShell, { type AdminNavItem } from './admin/AdminShell'
import DashboardOverview from './admin/DashboardOverview'
import ModulePlaceholder from './admin/ModulePlaceholder'
import {
  type QueryLoadState,
  getBlockingLoadState,
  getRefreshingLoadState,
  isBlockingLoadState,
  isRefreshingLoadState,
} from './admin/queryLoadState'
import {
  buildQuotaSliderTrack,
  clampQuotaSliderStageIndex,
  createQuotaSliderSeed,
  formatQuotaDraftInput,
  getQuotaSliderStagePosition,
  getQuotaSliderStageValue,
  normalizeQuotaDraftInput,
  parseQuotaDraftValue,
  type QuotaSliderField,
  type QuotaSliderSeed,
} from './admin/quotaSlider'
import {
  type AdminModuleId,
  type AdminPathRoute,
  buildAdminKeysPath,
  isSameAdminRoute,
  keyDetailPath,
  modulePath,
  parseAdminPath,
  buildAdminUsersPath,
  tokenDetailPath,
  tokenLeaderboardPath,
  userDetailPath,
  userTagCreatePath,
  userTagEditPath,
  userTagsPath,
} from './admin/routes'
import { useLanguage, useTranslate, type AdminTranslations } from './i18n'
import { extractTvlyDevApiKeysFromText } from './lib/api-key-extract'
import { ADMIN_USER_CONSOLE_HREF } from './lib/adminUserConsoleEntry'
import {
  copyText,
  isCopyIntentKey,
  selectAllReadonlyText,
  shouldPrewarmSecretCopy,
  type CopyTextOptions,
} from './lib/clipboard'
import {
  fetchApiKeys,
  fetchApiKeySecret,
  addApiKeysBatch,
  type AddApiKeysBatchResponse,
  validateApiKeys,
  type ValidateKeyResult,
  deleteApiKey,
  setKeyStatus,
  clearApiKeyQuarantine,
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
  createTokensBatch,
  fetchTokenUsageLeaderboard,
  type TokenUsageLeaderboardItem,
  type TokenLeaderboardPeriod,
  type TokenLeaderboardFocus,
  type Paginated,
  fetchKeyMetrics,
  fetchKeyLogs,
  type KeySummary,
  type JobLogView,
  fetchApiKeyDetail,
  syncApiKeyUsage,
  fetchJobs,
  fetchTokenGroups,
  type TokenGroup,
  fetchAdminUsers,
  fetchAdminUserDetail,
  fetchAdminRegistrationSettings,
  updateAdminUserQuota,
  updateAdminRegistrationSettings,
  fetchAdminUserTags,
  createAdminUserTag,
  updateAdminUserTag,
  deleteAdminUserTag,
  bindAdminUserTag,
  unbindAdminUserTag,
  type AdminUserSummary,
  type AdminUserDetail,
  type AdminUserTag,
  type AdminUserTagBinding,
} from './api'

const REFRESH_INTERVAL_MS = 30_000
const LOGS_PER_PAGE = 20
const LOGS_MAX_PAGES = 10
const DASHBOARD_RECENT_LOGS_PER_PAGE = 64
const DASHBOARD_RECENT_JOBS_PER_PAGE = 20
const DASHBOARD_OVERVIEW_SSE_REFRESH_INTERVAL_MS = 30_000
const DEFAULT_KEYS_PER_PAGE = 20
const USERS_PER_PAGE = 20
// Auto-collapse behavior for the API keys batch overlay (empty textarea only):
// The user wants "delay + close animation" to total 500ms.
const KEYS_BATCH_CLOSE_ANIMATION_MS = 200
const KEYS_BATCH_AUTO_COLLAPSE_TOTAL_MS = 500
const KEYS_BATCH_AUTO_COLLAPSE_DELAY_MS = Math.max(0, KEYS_BATCH_AUTO_COLLAPSE_TOTAL_MS - KEYS_BATCH_CLOSE_ANIMATION_MS)
const API_KEYS_IMPORT_CHUNK_SIZE = 1000
const DASHBOARD_EXHAUSTED_KEYS_PAGE_SIZE = 5
const DASHBOARD_TOKENS_PAGE_SIZE = 100
const DASHBOARD_TOKENS_MAX_PAGES = 10
const USER_TAG_DISPLAY_LIMIT = 3
const NEW_USER_TAG_CARD_ID = '__new__'

type UserQuotaSnapshot = Record<QuotaSliderField, QuotaSliderSeed>

type UserTagFormState = {
  tagId: string | null
  name: string
  displayName: string
  icon: string
  effectKind: string
  hourlyAnyDelta: string
  hourlyDelta: string
  dailyDelta: string
  monthlyDelta: string
}

type UserTagLike = Pick<AdminUserTagBinding, 'displayName' | 'icon' | 'systemKey' | 'effectKind'> & {
  source?: string | null
}

const EMPTY_USER_TAG_FORM: UserTagFormState = {
  tagId: null,
  name: '',
  displayName: '',
  icon: '',
  effectKind: 'quota_delta',
  hourlyAnyDelta: '0',
  hourlyDelta: '0',
  dailyDelta: '0',
  monthlyDelta: '0',
}

function buildUserQuotaSnapshot(detail: AdminUserDetail): UserQuotaSnapshot {
  return {
    hourlyAnyLimit: createQuotaSliderSeed('hourlyAnyLimit', detail.hourlyAnyUsed, detail.quotaBase.hourlyAnyLimit),
    hourlyLimit: createQuotaSliderSeed('hourlyLimit', detail.quotaHourlyUsed, detail.quotaBase.hourlyLimit),
    dailyLimit: createQuotaSliderSeed('dailyLimit', detail.quotaDailyUsed, detail.quotaBase.dailyLimit),
    monthlyLimit: createQuotaSliderSeed('monthlyLimit', detail.quotaMonthlyUsed, detail.quotaBase.monthlyLimit),
  }
}

function clampDisplayedQuota(value: number): number {
  return Math.max(0, value)
}

function formatQuotaLimitValue(value: number): string {
  return formatNumber(clampDisplayedQuota(value))
}

function formatQuotaUsagePair(used: number, limit: number): string {
  return `${formatNumber(Math.max(0, used))} / ${formatQuotaLimitValue(limit)}`
}

function formatKeyGroupName(group: string | null | undefined, ungroupedLabel: string): string {
  const normalized = group?.trim() ?? ''
  return normalized.length > 0 ? normalized : ungroupedLabel
}

function toggleSelection(values: string[], value: string): string[] {
  return values.includes(value) ? values.filter((item) => item !== value) : [...values, value]
}

function summarizeFilterSelection(
  label: string,
  selectedLabels: string[],
  allLabel: string,
  selectedSuffix: string,
): string {
  if (selectedLabels.length === 0) return `${label}: ${allLabel}`
  if (selectedLabels.length === 1) return `${label}: ${selectedLabels[0]}`
  return `${label}: ${selectedLabels.length} ${selectedSuffix}`
}

const adminTableStackStyle = {
  display: 'flex',
  flexDirection: 'column',
  gap: 4,
  minWidth: 0,
} as const

const adminTableFieldStyle = {
  whiteSpace: 'nowrap',
  lineHeight: 1.35,
} as const

const adminTableSecondaryFieldStyle = {
  ...adminTableFieldStyle,
  fontSize: '0.92em',
  opacity: 0.68,
} as const

const adminTableInlineFieldStyle = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 8,
  whiteSpace: 'nowrap',
  lineHeight: 1.35,
} as const

const adminTableHeaderStackStyle = {
  display: 'flex',
  flexDirection: 'column',
  gap: 2,
  minHeight: 40,
  justifyContent: 'center',
} as const

const keysUtilityRowStyle = {
  display: 'flex',
  alignItems: 'stretch',
  justifyContent: 'space-between',
  gap: 16,
  flexWrap: 'wrap',
  marginBottom: 16,
} as const

const keysFilterClusterStyle = {
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  flexWrap: 'wrap',
  flex: '1 1 360px',
  minWidth: 260,
} as const

const keysQuickAddCardStyle = {
  flex: '0 1 420px',
  minWidth: 300,
  width: 'min(420px, 100%)',
  padding: 0,
} as const

const keysQuickAddActionsStyle = {
  display: 'flex',
  alignItems: 'center',
  gap: 8,
  flexWrap: 'nowrap',
  width: '100%',
} as const

function formatSignedQuotaDelta(value: number): string {
  if (value > 0) {
    return `+${formatNumber(value)}`
  }
  return formatNumber(value)
}

function getAdminUsersQueryFromLocation(): string {
  return new URLSearchParams(window.location.search).get('q')?.trim() ?? ''
}

function getAdminUsersTagFilterFromLocation(): string | null {
  const tagId = new URLSearchParams(window.location.search).get('tagId')?.trim() ?? ''
  return tagId.length > 0 ? tagId : null
}

function getAdminUsersPageFromLocation(): number {
  const rawPage = new URLSearchParams(window.location.search).get('page')?.trim() ?? ''
  const parsedPage = Number.parseInt(rawPage, 10)
  return Number.isFinite(parsedPage) && parsedPage > 1 ? parsedPage : 1
}

function getAdminKeysPageFromLocation(): number {
  const rawPage = new URLSearchParams(window.location.search).get('page')?.trim() ?? ''
  const parsedPage = Number.parseInt(rawPage, 10)
  return Number.isFinite(parsedPage) && parsedPage > 1 ? parsedPage : 1
}

function getAdminKeysPerPageFromLocation(): number {
  const rawPerPage = new URLSearchParams(window.location.search).get('perPage')?.trim() ?? ''
  const parsedPerPage = Number.parseInt(rawPerPage, 10)
  if (!Number.isFinite(parsedPerPage)) return DEFAULT_KEYS_PER_PAGE
  return Math.min(100, Math.max(1, parsedPerPage))
}

function getAdminKeysValuesFromLocation(name: 'group' | 'status'): string[] {
  const values = new URLSearchParams(window.location.search).getAll(name)
  const normalized = new Set<string>()
  for (const value of values) {
    const trimmed = value.trim()
    if (!trimmed && name !== 'group') continue
    normalized.add(name === 'status' ? trimmed.toLowerCase() : trimmed)
  }
  return Array.from(normalized)
}

function getUserTagIconSrc(icon: string | null | undefined): string | null {
  if (icon === 'linuxdo') {
    return '/linuxdo-logo.svg'
  }
  return null
}

function isSystemUserTag(tag: { systemKey?: string | null; source?: string | null }): boolean {
  return Boolean(tag.systemKey) || tag.source === 'system_linuxdo'
}

function createUserTagFormState(tag?: AdminUserTag | null): UserTagFormState {
  if (!tag) {
    return { ...EMPTY_USER_TAG_FORM }
  }
  return {
    tagId: tag.id,
    name: tag.name,
    displayName: tag.displayName,
    icon: tag.icon ?? '',
    effectKind: tag.effectKind,
    hourlyAnyDelta: String(tag.hourlyAnyDelta),
    hourlyDelta: String(tag.hourlyDelta),
    dailyDelta: String(tag.dailyDelta),
    monthlyDelta: String(tag.monthlyDelta),
  }
}

function UserTagBadge({
  tag,
  usersStrings,
}: {
  tag: UserTagLike
  usersStrings: AdminTranslations['users']
}): JSX.Element {
  const iconSrc = getUserTagIconSrc(tag.icon)
  const isSystem = isSystemUserTag(tag)
  const isBlockAll = tag.effectKind === 'block_all'
  const classes = [
    'user-tag-pill',
    isSystem ? 'user-tag-pill-system' : '',
    isBlockAll ? 'user-tag-pill-block' : '',
  ]
    .filter(Boolean)
    .join(' ')

  return (
    <Badge variant="outline" className={classes} title={tag.displayName}>
      {iconSrc && <img src={iconSrc} alt="" className="user-tag-pill-icon" aria-hidden="true" />}
      <span>{tag.displayName}</span>
      {isSystem && <span className="user-tag-pill-meta">{usersStrings.catalog.scopeSystemShort}</span>}
      {isBlockAll && <span className="user-tag-pill-meta">{usersStrings.catalog.blockShort}</span>}
    </Badge>
  )
}

function UserTagBadgeList({
  tags,
  usersStrings,
  emptyLabel,
  limit = USER_TAG_DISPLAY_LIMIT,
}: {
  tags: AdminUserTagBinding[]
  usersStrings: AdminTranslations['users']
  emptyLabel: string
  limit?: number
}): JSX.Element {
  if (tags.length === 0) {
    return <span className="panel-description">{emptyLabel}</span>
  }

  const visibleTags = tags.slice(0, limit)
  const overflow = Math.max(0, tags.length - visibleTags.length)

  return (
    <div className="user-tag-pill-list">
      {visibleTags.map((tag) => (
        <UserTagBadge key={`${tag.tagId}:${tag.source}`} tag={tag} usersStrings={usersStrings} />
      ))}
      {overflow > 0 && <Badge variant="outline" className="user-tag-pill-overflow">+{overflow}</Badge>}
    </div>
  )
}

function leaderboardPrimaryValue(
  item: TokenUsageLeaderboardItem,
  period: 'day' | 'month' | 'all',
  focus: 'usage' | 'errors' | 'other',
): number {
  const metrics =
    period === 'day'
      ? { usage: item.today_total ?? 0, errors: item.today_errors ?? 0, other: item.today_other ?? 0 }
      : period === 'month'
        ? { usage: item.month_total ?? 0, errors: item.month_errors ?? 0, other: item.month_other ?? 0 }
        : { usage: item.all_total ?? 0, errors: item.all_errors ?? 0, other: item.all_other ?? 0 }
  return metrics[focus] ?? 0
}

function sortLeaderboard(
  items: TokenUsageLeaderboardItem[],
  period: 'day' | 'month' | 'all',
  focus: 'usage' | 'errors' | 'other',
): TokenUsageLeaderboardItem[] {
  return [...items].sort(
    (a, b) => leaderboardPrimaryValue(b, period, focus) - leaderboardPrimaryValue(a, period, focus) || b.total_requests - a.total_requests,
  )
}

type MetricKey = 'usage' | 'errors' | 'other'

function pickPrimaryForPeriod(
  item: TokenUsageLeaderboardItem,
  period: 'day' | 'month' | 'all',
  focus: MetricKey,
): { primaryKey: MetricKey; values: Record<MetricKey, number> } {
  const values: Record<MetricKey, number> =
    period === 'day'
      ? {
          usage: item.today_total ?? 0,
          errors: item.today_errors ?? 0,
          other: item.today_other ?? 0,
        }
      : period === 'month'
        ? {
            usage: item.month_total ?? 0,
            errors: item.month_errors ?? 0,
            other: item.month_other ?? 0,
          }
        : {
            usage: item.all_total ?? 0,
            errors: item.all_errors ?? 0,
            other: item.all_other ?? 0,
          }

  return { primaryKey: focus, values }
}

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

// Date/time without year for compact "Last Used" rendering
const dateTimeNoYearFormatter = new Intl.DateTimeFormat(undefined, {
  month: 'short',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit',
  second: '2-digit',
  hour12: false,
})

const dateOnlyFormatter = new Intl.DateTimeFormat(undefined, {
  year: 'numeric',
  month: 'short',
  day: '2-digit',
})

// Time-only formatter for compact "Updated HH:MM:SS"
const timeOnlyFormatter = new Intl.DateTimeFormat(undefined, {
  hour: '2-digit',
  minute: '2-digit',
  second: '2-digit',
  hour12: false,
})

const tooltipTimeFormatter = new Intl.DateTimeFormat(undefined, {
  year: 'numeric',
  month: 'short',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit',
  second: '2-digit',
  hour12: false,
  fractionalSecondDigits: 3,
})

const relativeTimeFormatter = new Intl.RelativeTimeFormat(undefined, {
  numeric: 'auto',
})

function formatClockTime(value: number | null): string {
  if (!value) return '—'
  return timeOnlyFormatter.format(new Date(value * 1000))
}

function formatTimestampWithMs(value: number | null): string {
  if (!value) return '—'
  return tooltipTimeFormatter.format(new Date(value * 1000))
}

function formatRelativeTime(value: number | null): string {
  if (!value) return '—'
  const nowSeconds = Date.now() / 1000
  const diffSeconds = value - nowSeconds
  const divisions: Array<{ amount: number; unit: Intl.RelativeTimeFormatUnit }> = [
    { amount: 60, unit: 'second' },
    { amount: 60, unit: 'minute' },
    { amount: 24, unit: 'hour' },
    { amount: 7, unit: 'day' },
    { amount: 4.34524, unit: 'week' },
    { amount: 12, unit: 'month' },
    { amount: Number.POSITIVE_INFINITY, unit: 'year' },
  ]

  let duration = diffSeconds
  for (const division of divisions) {
    if (Math.abs(duration) < division.amount) {
      return relativeTimeFormatter.format(Math.round(duration), division.unit)
    }
    duration /= division.amount
  }
  return relativeTimeFormatter.format(Math.round(duration), 'year')
}

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

function formatTimestampNoYear(value: number | null): string {
  if (!value) return '—'
  return dateTimeNoYearFormatter.format(new Date(value * 1000))
}

function formatDateOnly(value: number | null): string {
  if (!value) return '—'
  const d = new Date(value * 1000)
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}

function tokenOwnerPrimary(owner: AuthToken['owner']): string {
  if (!owner) return ''
  return owner.displayName || owner.userId
}

function tokenOwnerSecondary(owner: AuthToken['owner']): string | null {
  if (!owner?.username) return null
  return `@${owner.username}`
}

function TokenOwnerValue({
  owner,
  emptyLabel,
  onOpenUser,
  compact = false,
}: {
  owner: AuthToken['owner']
  emptyLabel: string
  onOpenUser: (userId: string) => void
  compact?: boolean
}): JSX.Element {
  if (!owner) {
    return <span className="token-owner-empty">{emptyLabel}</span>
  }

  const secondary = tokenOwnerSecondary(owner)
  return (
    <div className={`token-owner-block${compact ? ' token-owner-block-compact' : ''}`}>
      <button
        type="button"
        className={`link-button token-owner-trigger${compact ? ' token-owner-trigger-compact' : ''}`}
        onClick={() => onOpenUser(owner.userId)}
      >
        <span className="token-owner-link">{tokenOwnerPrimary(owner)}</span>
        {secondary ? <span className="token-owner-secondary">{secondary}</span> : null}
      </button>
    </div>
  )
}

function statusTone(status: string): StatusTone {
  const normalized = status.toLowerCase()
  if (normalized === 'active' || normalized === 'success' || normalized === 'completed') return 'success'
  if (normalized === 'quarantined') return 'warning'
  if (normalized === 'exhausted' || normalized === 'quota_exhausted' || normalized === 'retry_exhausted') return 'warning'
  if (normalized === 'running' || normalized === 'in_progress' || normalized === 'queued' || normalized === 'pending') {
    return 'info'
  }
  if (normalized === 'error' || normalized === 'failed' || normalized === 'timeout' || normalized === 'cancelled' || normalized === 'canceled') {
    return 'error'
  }
  if (normalized === 'deleted') return 'neutral'
  return 'neutral'
}

function quotaTone(quotaState: string): StatusTone {
  const normalized = quotaState.toLowerCase()
  if (normalized === 'hour') return 'warning'
  if (normalized === 'day') return 'error'
  if (normalized === 'month') return 'info'
  return 'success'
}

function statusLabel(status: string, strings: AdminTranslations): string {
  const normalized = status.toLowerCase()
  return strings.statuses[normalized] ?? status
}

function keyBadgeStatus(item: Pick<ApiKeyStats, 'status' | 'quarantine'>): string {
  return item.quarantine ? 'quarantined' : item.status
}

function jobTypeLabel(jobType: string, strings: AdminTranslations['jobs']): string {
  const normalized = jobType.trim()
  if (!normalized) return '—'

  const direct = strings.types?.[normalized]
  if (direct) return direct

  const aliases: Record<string, string> = {
    usage_aggregation: 'token_usage_rollup',
    log_cleanup: 'auth_token_logs_gc',
  }
  const aliasTarget = aliases[normalized]
  if (aliasTarget && strings.types?.[aliasTarget]) {
    return strings.types[aliasTarget]
  }

  return normalized
    .replace(/[\/_]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
}

function jobStatusLabel(status: string): string {
  const normalized = status.trim().toLowerCase()
  if (!normalized) return '—'

  const aliases: Record<string, string> = {
    success: 'Success',
    running: 'Running',
    in_progress: 'In progress',
    pending: 'Pending',
    queued: 'Queued',
    completed: 'Completed',
    error: 'Error',
    failed: 'Failed',
    quota_exhausted: 'Quota exhausted',
    retry_exhausted: 'Retry exhausted',
    timeout: 'Timed out',
    cancelled: 'Canceled',
    canceled: 'Canceled',
  }
  const alias = aliases[normalized]
  if (alias) return alias

  return normalized
    .replace(/[\/_]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
    .replace(/\b[a-z]/g, (ch) => ch.toUpperCase())
}

function formatErrorMessage(log: RequestLog, errorsStrings: AdminTranslations['logs']['errors']): string {
  const message = log.error_message?.trim()
  if (message) {
    return message
  }

  const status = log.result_status.toLowerCase()
  if (status === 'quota_exhausted') {
    if (log.http_status != null) {
      return errorsStrings.quotaExhaustedHttp.replace('{http}', String(log.http_status))
    }
    return errorsStrings.quotaExhausted
  }

  if (status === 'error') {
    if (log.http_status != null && log.mcp_status != null) {
      return errorsStrings.requestFailedHttpMcp
        .replace('{http}', String(log.http_status))
        .replace('{mcp}', String(log.mcp_status))
    }
    if (log.http_status != null) {
      return errorsStrings.requestFailedHttp.replace('{http}', String(log.http_status))
    }
    if (log.mcp_status != null) {
      return errorsStrings.requestFailedMcp.replace('{mcp}', String(log.mcp_status))
    }
    return errorsStrings.requestFailedGeneric
  }

  if (status === 'success') {
    return errorsStrings.none
  }

  if (log.http_status != null) {
    return errorsStrings.httpStatus.replace('{http}', String(log.http_status))
  }

  return errorsStrings.none
}

interface ManualCopyBubbleState {
  anchorEl: HTMLElement | null
  title: string
  description: string
  fieldLabel: string
  value: string
  multiline?: boolean
}

type ManualCopyDialogState = Omit<ManualCopyBubbleState, 'anchorEl'>

function AdminDashboard(): JSX.Element {
  const [route, setRoute] = useState<AdminPathRoute>(() => parseAdminPath(window.location.pathname))
  const { language } = useLanguage()
  const translations = useTranslate()
  const adminStrings = translations.admin
  const headerStrings = adminStrings.header
  const loadingStateStrings = adminStrings.loadingStates
  const userConsoleHref = ADMIN_USER_CONSOLE_HREF
  const tokenStrings = adminStrings.tokens
  const tokenLeaderboardStrings = adminStrings.tokenLeaderboard
  const quotaLabels = tokenStrings.quotaStates ?? {
    normal: 'Normal',
    hour: '1 hour limit',
    day: '24 hour limit',
    month: 'Monthly limit',
  }
  const metricsStrings = adminStrings.metrics
  const keyStrings = adminStrings.keys
  const logStrings = adminStrings.logs
  const jobsStrings = adminStrings.jobs
  const footerStrings = adminStrings.footer
  const errorStrings = adminStrings.errors
  const [summary, setSummary] = useState<Summary | null>(null)
  const [keys, setKeys] = useState<ApiKeyStats[]>([])
  const [dashboardKeys, setDashboardKeys] = useState<ApiKeyStats[]>([])
  const [keysTotal, setKeysTotal] = useState(0)
  const [keysPage, setKeysPage] = useState(getAdminKeysPageFromLocation)
  const [keysPerPage, setKeysPerPage] = useState(getAdminKeysPerPageFromLocation)
  const [keysLoadState, setKeysLoadState] = useState<QueryLoadState>('initial_loading')
  const [keysError, setKeysError] = useState<string | null>(null)
  const [keyGroupFacets, setKeyGroupFacets] = useState<Array<{ value: string; count: number }>>([])
  const [keyStatusFacets, setKeyStatusFacets] = useState<Array<{ value: string; count: number }>>([])
  const [tokens, setTokens] = useState<AuthToken[]>([])
  const [dashboardTokens, setDashboardTokens] = useState<AuthToken[]>([])
  const [dashboardTokenCoverage, setDashboardTokenCoverage] = useState<'ok' | 'truncated' | 'error'>('ok')
  const [dashboardOverviewLoaded, setDashboardOverviewLoaded] = useState(false)
  const [tokensPage, setTokensPage] = useState(1)
  const tokensPerPage = 10
  const [tokensTotal, setTokensTotal] = useState(0)
  const [tokensLoadState, setTokensLoadState] = useState<QueryLoadState>('initial_loading')
  const [tokenGroups, setTokenGroups] = useState<TokenGroup[]>([])
  const [selectedTokenGroupName, setSelectedTokenGroupName] = useState<string | null>(null)
  const [selectedTokenUngrouped, setSelectedTokenUngrouped] = useState(false)
  const [tokenGroupsExpanded, setTokenGroupsExpanded] = useState(false)
  const [tokenGroupsCollapsedOverflowing, setTokenGroupsCollapsedOverflowing] = useState(false)
  const [tokenLeaderboard, setTokenLeaderboard] = useState<TokenUsageLeaderboardItem[]>([])
  const [tokenLeaderboardLoadState, setTokenLeaderboardLoadState] = useState<QueryLoadState>('initial_loading')
  const [tokenLeaderboardError, setTokenLeaderboardError] = useState<string | null>(null)
  const [tokenLeaderboardPeriod, setTokenLeaderboardPeriod] = useState<TokenLeaderboardPeriod>('day')
  const [tokenLeaderboardFocus, setTokenLeaderboardFocus] = useState<TokenLeaderboardFocus>('usage')
  const [tokenLeaderboardNonce, setTokenLeaderboardNonce] = useState(0)
  const [logs, setLogs] = useState<RequestLog[]>([])
  const [dashboardLogs, setDashboardLogs] = useState<RequestLog[]>([])
  const [logsTotal, setLogsTotal] = useState(0)
  const [logsPage, setLogsPage] = useState(1)
  const [logResultFilter, setLogResultFilter] = useState<'all' | 'success' | 'error' | 'quota_exhausted'>('all')
  const [requestsLoadState, setRequestsLoadState] = useState<QueryLoadState>('initial_loading')
  const [requestsError, setRequestsError] = useState<string | null>(null)
  const [jobs, setJobs] = useState<JobLogView[]>([])
  const [dashboardJobs, setDashboardJobs] = useState<JobLogView[]>([])
  const [jobFilter, setJobFilter] = useState<'all' | 'quota' | 'usage' | 'logs'>('all')
  const [jobsPage, setJobsPage] = useState(1)
  const jobsPerPage = 10
  const [jobsTotal, setJobsTotal] = useState(0)
  const [jobsLoadState, setJobsLoadState] = useState<QueryLoadState>('initial_loading')
  const [jobsError, setJobsError] = useState<string | null>(null)
  const [users, setUsers] = useState<AdminUserSummary[]>([])
  const [usersTotal, setUsersTotal] = useState(0)
  const [usersPage, setUsersPage] = useState(1)
  const [usersQueryInput, setUsersQueryInput] = useState('')
  const [usersQuery, setUsersQuery] = useState('')
  const [usersTagFilterId, setUsersTagFilterId] = useState<string | null>(null)
  const [usersLoadState, setUsersLoadState] = useState<QueryLoadState>('initial_loading')
  const [usersError, setUsersError] = useState<string | null>(null)
  const [allowRegistration, setAllowRegistration] = useState<boolean | null>(null)
  const [registrationSettingsLoaded, setRegistrationSettingsLoaded] = useState(false)
  const [registrationSettingsLoading, setRegistrationSettingsLoading] = useState(false)
  const [registrationSettingsSaving, setRegistrationSettingsSaving] = useState(false)
  const [registrationSettingsError, setRegistrationSettingsError] = useState<string | null>(null)
  const [selectedUserDetail, setSelectedUserDetail] = useState<AdminUserDetail | null>(null)
  const [userDetailLoading, setUserDetailLoading] = useState(false)
  const [userQuotaSnapshot, setUserQuotaSnapshot] = useState<UserQuotaSnapshot | null>(null)
  const [userQuotaDraft, setUserQuotaDraft] = useState<Record<QuotaSliderField, string> | null>(null)
  const [savingUserQuota, setSavingUserQuota] = useState(false)
  const [userQuotaError, setUserQuotaError] = useState<string | null>(null)
  const [userQuotaSavedAt, setUserQuotaSavedAt] = useState<number | null>(null)
  const [tagCatalog, setTagCatalog] = useState<AdminUserTag[]>([])
  const [tagCatalogLoading, setTagCatalogLoading] = useState(false)
  const [tagCatalogLoadedOnce, setTagCatalogLoadedOnce] = useState(false)
  const [tagCatalogError, setTagCatalogError] = useState<string | null>(null)
  const [activeUserTagEditorId, setActiveUserTagEditorId] = useState<string | null>(null)
  const [userTagCatalogDraft, setUserTagCatalogDraft] = useState<UserTagFormState>({ ...EMPTY_USER_TAG_FORM })
  const [savingUserTagCatalog, setSavingUserTagCatalog] = useState(false)
  const [deletingUserTagId, setDeletingUserTagId] = useState<string | null>(null)
  const [pendingUserTagDelete, setPendingUserTagDelete] = useState<AdminUserTag | null>(null)
  const [selectedBindableTagId, setSelectedBindableTagId] = useState('')
  const [savingUserTagBinding, setSavingUserTagBinding] = useState(false)
  const [userTagError, setUserTagError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const pollingTimerRef = useRef<number | null>(null)
  const routeRef = useRef<AdminPathRoute>(route)
  const loadDashboardOverviewRef = useRef<((signal?: AbortSignal) => Promise<void>) | null>(null)
  const dashboardOverviewInFlightRef = useRef(false)
  const dashboardOverviewLastSseRefreshAtRef = useRef(0)
  const baseDataLoadedRef = useRef(false)
  const tokenLeaderboardQueryKeyRef = useRef<string | null>(null)
  const tokenLeaderboardNonceRef = useRef(0)
  const requestsLoadedRef = useRef(false)
  const jobsLoadedRef = useRef(false)
  const keysLoadedRef = useRef(false)
  const usersLoadedRef = useRef(false)
  const keysQueryKeyRef = useRef<string | null>(null)
  const usersQueryKeyRef = useRef<string | null>(null)
  const baseDataAbortRef = useRef<AbortController | null>(null)
  const requestsAbortRef = useRef<AbortController | null>(null)
  const jobsAbortRef = useRef<AbortController | null>(null)
  const keysAbortRef = useRef<AbortController | null>(null)
  const usersAbortRef = useRef<AbortController | null>(null)
  const [lastUpdated, setLastUpdated] = useState<Date | null>(null)
  const [version, setVersion] = useState<{ backend: string; frontend: string } | null>(null)
  const [profile, setProfile] = useState<Profile | null>(null)
  const secretCacheRef = useRef<Map<string, string>>(new Map())
  const secretRequestCacheRef = useRef<Map<string, Promise<string>>>(new Map())
  const tokenSecretCacheRef = useRef<Map<string, string>>(new Map())
  const tokenSecretRequestCacheRef = useRef<Map<string, Promise<string>>>(new Map())
  const tokenSecretVersionRef = useRef<Map<string, number>>(new Map())
  const secretWarmTimerRef = useRef<Map<string, number>>(new Map())
  const secretWarmAbortRef = useRef<Map<string, AbortController>>(new Map())
  const tokenGroupsListRef = useRef<HTMLDivElement | null>(null)
  const [copyState, setCopyState] = useState<Map<string, 'loading' | 'copied'>>(() => new Map())
  const [manualCopyBubble, setManualCopyBubble] = useState<ManualCopyBubbleState | null>(null)
  const [manualCopyDialog, setManualCopyDialog] = useState<ManualCopyDialogState | null>(null)
  const manualCopyDialogFieldRef = useRef<HTMLInputElement | null>(null)
  const [expandedLogs, setExpandedLogs] = useState<Set<number>>(() => new Set())
  type AddKeysBatchReportState =
    | { kind: 'success'; response: AddApiKeysBatchResponse }
    | { kind: 'error'; message: string; input_lines: number; valid_lines: number }

  const [newKeysText, setNewKeysText] = useState('')
  const [newKeysGroup, setNewKeysGroup] = useState('')
  const [selectedKeyGroups, setSelectedKeyGroups] = useState<string[]>(() => getAdminKeysValuesFromLocation('group'))
  const [selectedKeyStatuses, setSelectedKeyStatuses] = useState<string[]>(() => getAdminKeysValuesFromLocation('status'))
  const [keysBatchExpanded, setKeysBatchExpanded] = useState(false)
  const [keysBatchClosing, setKeysBatchClosing] = useState(false)
  const keysBatchOpenReasonRef = useRef<'hover' | 'focus' | null>(null)
  const keysBatchSuppressNextHoverRef = useRef(false)
  const keysBatchLastPointerRef = useRef<{ x: number; y: number } | null>(null)
  const keysBatchAutoCollapseTimerRef = useRef<number | null>(null)
  const keysBatchCloseTimerRef = useRef<number | null>(null)
  const keysBatchAnchorRef = useRef<HTMLDivElement | null>(null)
  const keysBatchCollapsedInputRef = useRef<HTMLInputElement | null>(null)
  const keysBatchTextareaRef = useRef<HTMLTextAreaElement | null>(null)
  const keysBatchFooterRef = useRef<HTMLDivElement | null>(null)
  const keysBatchOverlayRef = useRef<HTMLDivElement | null>(null)
  const [keysBatchReport, setKeysBatchReport] = useState<AddKeysBatchReportState | null>(null)

  type KeyValidationStatus =
    | 'pending'
    | 'duplicate_in_input'
    | 'ok'
    | 'ok_exhausted'
    | 'unauthorized'
    | 'forbidden'
    | 'invalid'
    | 'error'

  type KeyValidationRow = {
    api_key: string
    status: KeyValidationStatus
    quota_limit?: number
    quota_remaining?: number
    detail?: string
    attempts: number
  }

  type KeysValidationState = {
    group: string
    input_lines: number
    valid_lines: number
    unique_in_input: number
    duplicate_in_input: number
    checking: boolean
    importing: boolean
    rows: KeyValidationRow[]
    imported_api_keys: string[]
    importReport?: AddApiKeysBatchResponse
    importWarning?: string
    importError?: string
  }

  const keysValidateAbortRef = useRef<AbortController | null>(null)
  const keysValidateRunIdRef = useRef(0)
  const [keysValidation, setKeysValidation] = useState<KeysValidationState | null>(null)
  const [newTokenNote, setNewTokenNote] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const [deletingId, setDeletingId] = useState<string | null>(null)
  const [togglingId, setTogglingId] = useState<string | null>(null)
  const [clearingQuarantineId, setClearingQuarantineId] = useState<string | null>(null)
  const [pendingDeleteId, setPendingDeleteId] = useState<string | null>(null)
  const [pendingDisableId, setPendingDisableId] = useState<string | null>(null)
  const [pendingTokenDeleteId, setPendingTokenDeleteId] = useState<string | null>(null)
  const [editingTokenId, setEditingTokenId] = useState<string | null>(null)
  const [editingTokenNote, setEditingTokenNote] = useState('')
  const [savingTokenNote, setSavingTokenNote] = useState(false)
  const [sseConnected, setSseConnected] = useState(false)
  const [expandedJobs, setExpandedJobs] = useState<Set<number>>(() => new Set())
  // Batch dialog state
  const [batchDialogOpen, setBatchDialogOpen] = useState(false)
  const [batchGroup, setBatchGroup] = useState('')
  const [batchCount, setBatchCount] = useState(10)
  const [batchCreating, setBatchCreating] = useState(false)
  const [batchShareText, setBatchShareText] = useState<string | null>(null)
  const isAdmin = profile?.isAdmin ?? false
  const keysBatchVisible = keysBatchExpanded || keysBatchClosing
  const manualCopyText = useMemo(
    () => (
      language === 'zh'
        ? {
            title: '请手动复制',
            description: '当前浏览器拦截了自动复制，下面已选中原文，可直接手动复制。',
            close: '关闭',
            fields: {
              apiKey: '完整 API Key',
              token: '完整 Token',
              shareLink: '分享链接',
            },
            createToken: {
              title: '令牌已创建，请手动复制',
              description: '自动复制失败，下面保留了完整令牌，请先手动复制后再继续操作。',
            },
          }
        : {
            title: 'Manual copy required',
            description: 'This browser blocked automatic copy. The original value is selected below for manual copy.',
            close: 'Close',
            fields: {
              apiKey: 'Full API Key',
              token: 'Full Token',
              shareLink: 'Share Link',
            },
            createToken: {
              title: 'Token created — copy manually',
              description: 'Automatic copy failed. The full token is selected below so you can copy it before continuing.',
            },
          }
    ),
    [language],
  )

  useEffect(() => {
    if (!manualCopyDialog) return
    const frame = window.requestAnimationFrame(() => {
      selectAllReadonlyText(manualCopyDialogFieldRef.current)
    })
    return () => window.cancelAnimationFrame(frame)
  }, [manualCopyDialog])

  useEffect(() => {
    return () => {
      for (const timer of secretWarmTimerRef.current.values()) {
        window.clearTimeout(timer)
      }
      for (const controller of secretWarmAbortRef.current.values()) {
        controller.abort()
      }
    }
  }, [])

  const clearKeysBatchAutoCollapseTimer = useCallback(() => {
    if (keysBatchAutoCollapseTimerRef.current != null) {
      window.clearTimeout(keysBatchAutoCollapseTimerRef.current)
      keysBatchAutoCollapseTimerRef.current = null
    }
  }, [])

  const clearKeysBatchCloseTimer = useCallback(() => {
    if (keysBatchCloseTimerRef.current != null) {
      window.clearTimeout(keysBatchCloseTimerRef.current)
      keysBatchCloseTimerRef.current = null
    }
  }, [])

  useEffect(() => () => {
    clearKeysBatchAutoCollapseTimer()
    clearKeysBatchCloseTimer()
  }, [clearKeysBatchAutoCollapseTimer, clearKeysBatchCloseTimer])

  useEffect(() => {
    setManualCopyBubble(null)
    setManualCopyDialog(null)
  }, [route])

  useEffect(() => {
    if (!keysBatchExpanded) return
    if (keysBatchOpenReasonRef.current === 'focus') {
      window.requestAnimationFrame(() => keysBatchTextareaRef.current?.focus())
    }
  }, [keysBatchExpanded])

  useEffect(() => {
    const recordPointer = (event: PointerEvent) => {
      keysBatchLastPointerRef.current = { x: event.clientX, y: event.clientY }
    }
    window.addEventListener('pointermove', recordPointer, { passive: true })
    window.addEventListener('pointerdown', recordPointer)
    return () => {
      window.removeEventListener('pointermove', recordPointer)
      window.removeEventListener('pointerdown', recordPointer)
    }
  }, [])

  const maybeSuppressHoverReopen = useCallback(() => {
    const anchor = keysBatchAnchorRef.current
    const pointer = keysBatchLastPointerRef.current
    if (!anchor || !pointer) return
    const rect = anchor.getBoundingClientRect()
    if (
      pointer.x >= rect.left &&
      pointer.x <= rect.right &&
      pointer.y >= rect.top &&
      pointer.y <= rect.bottom
    ) {
      keysBatchSuppressNextHoverRef.current = true
    }
  }, [])

  const beginKeysBatchClose = useCallback(() => {
    if (!keysBatchVisible) return

    clearKeysBatchAutoCollapseTimer()
    clearKeysBatchCloseTimer()

    maybeSuppressHoverReopen()
    keysBatchOpenReasonRef.current = null
    setKeysBatchExpanded(false)

    const prefersReducedMotion = window.matchMedia?.('(prefers-reduced-motion: reduce)')?.matches ?? false
    if (prefersReducedMotion) {
      setKeysBatchClosing(false)
      return
    }

    setKeysBatchClosing(true)
    keysBatchCloseTimerRef.current = window.setTimeout(() => {
      keysBatchCloseTimerRef.current = null
      setKeysBatchClosing(false)
    }, KEYS_BATCH_CLOSE_ANIMATION_MS)
  }, [
    clearKeysBatchAutoCollapseTimer,
    clearKeysBatchCloseTimer,
    keysBatchVisible,
    maybeSuppressHoverReopen,
  ])

  const scheduleKeysBatchAutoCollapse = useCallback(
    (mode: 'blur' | 'hover') => {
      if (!keysBatchExpanded) return

      const textarea = keysBatchTextareaRef.current
      if (!textarea) return
      if (textarea.value.trim().length !== 0) return

      clearKeysBatchAutoCollapseTimer()
      keysBatchAutoCollapseTimerRef.current = window.setTimeout(() => {
        keysBatchAutoCollapseTimerRef.current = null

        const currentOverlay = keysBatchOverlayRef.current
        const currentTextarea = keysBatchTextareaRef.current
        if (!currentOverlay || !currentTextarea) return
        if (currentTextarea.value.trim().length !== 0) return

        // If the user re-focused the overlay before the timeout, keep it open.
        const active = document.activeElement
        if (active instanceof Node && currentOverlay.contains(active)) return

        if (mode === 'hover') {
          const pointer = keysBatchLastPointerRef.current
          const anchor = keysBatchAnchorRef.current
          if (pointer && anchor) {
            const anchorRect = anchor.getBoundingClientRect()
            const overlayRect = currentOverlay.getBoundingClientRect()
            const containsPointer = (rect: DOMRect) =>
              pointer.x >= rect.left && pointer.x <= rect.right && pointer.y >= rect.top && pointer.y <= rect.bottom
            if (containsPointer(anchorRect) || containsPointer(overlayRect)) return
          }
        }

        beginKeysBatchClose()
      }, KEYS_BATCH_AUTO_COLLAPSE_DELAY_MS)
    },
    [beginKeysBatchClose, clearKeysBatchAutoCollapseTimer, keysBatchExpanded],
  )

  useEffect(() => {
    if (!keysBatchExpanded) return

    const handlePointerDown = (event: PointerEvent) => {
      const root = keysBatchAnchorRef.current
      const overlay = keysBatchOverlayRef.current
      if (!root && !overlay) return
      const target = event.target
      if (
        target instanceof Node &&
        (root == null || !root.contains(target)) &&
        (overlay == null || !overlay.contains(target))
      ) {
        beginKeysBatchClose()
      }
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        beginKeysBatchClose()
      }
    }

    document.addEventListener('pointerdown', handlePointerDown)
    document.addEventListener('keydown', handleKeyDown)
    return () => {
      document.removeEventListener('pointerdown', handlePointerDown)
      document.removeEventListener('keydown', handleKeyDown)
    }
  }, [beginKeysBatchClose, keysBatchExpanded])

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

  const copyToClipboard = useCallback(async (value: string, options?: CopyTextOptions) => {
    return await copyText(value, options)
  }, [])

  const openManualCopyBubble = useCallback((state: ManualCopyBubbleState) => {
    setManualCopyBubble(state)
  }, [])

  const resolveApiKeySecret = useCallback(async (id: string, signal?: AbortSignal) => {
    const cached = secretCacheRef.current.get(id)
    if (cached) {
      return cached
    }

    const pending = secretRequestCacheRef.current.get(id)
    if (pending) {
      return await pending
    }

    const request = fetchApiKeySecret(id, signal)
      .then((result) => {
        if (!signal?.aborted) {
          secretCacheRef.current.set(id, result.api_key)
        }
        return result.api_key
      })
      .finally(() => {
        secretRequestCacheRef.current.delete(id)
      })

    secretRequestCacheRef.current.set(id, request)
    return await request
  }, [])

  const resolveTokenSecret = useCallback(async (id: string, signal?: AbortSignal) => {
    const cached = tokenSecretCacheRef.current.get(id)
    if (cached) {
      return cached
    }

    const pending = tokenSecretRequestCacheRef.current.get(id)
    if (pending) {
      return await pending
    }

    const requestVersion = tokenSecretVersionRef.current.get(id) ?? 0
    let request: Promise<string>
    request = fetchTokenSecret(id, signal)
      .then((result) => {
        if (!signal?.aborted && (tokenSecretVersionRef.current.get(id) ?? 0) === requestVersion) {
          tokenSecretCacheRef.current.set(id, result.token)
        }
        return result.token
      })
      .finally(() => {
        if (tokenSecretRequestCacheRef.current.get(id) === request) {
          tokenSecretRequestCacheRef.current.delete(id)
        }
      })

    tokenSecretRequestCacheRef.current.set(id, request)
    return await request
  }, [])

  const shouldPrewarmAdminSecretCopy = useMemo(() => shouldPrewarmSecretCopy(), [])

  const clearSecretWarmTimer = useCallback((key: string) => {
    const timer = secretWarmTimerRef.current.get(key)
    if (timer != null) {
      window.clearTimeout(timer)
      secretWarmTimerRef.current.delete(key)
    }
  }, [])

  const dropSecretWarmRequest = useCallback((key: string) => {
    const separatorIndex = key.indexOf(':')
    if (separatorIndex < 0) return
    const scope = key.slice(0, separatorIndex)
    const id = key.slice(separatorIndex + 1)
    if (!id) return
    if (scope === 'token') {
      tokenSecretRequestCacheRef.current.delete(id)
      return
    }
    if (scope === 'key') {
      secretRequestCacheRef.current.delete(id)
    }
  }, [])

  const cancelSecretWarm = useCallback((key: string) => {
    clearSecretWarmTimer(key)
    const controller = secretWarmAbortRef.current.get(key)
    if (controller) {
      controller.abort()
      secretWarmAbortRef.current.delete(key)
      dropSecretWarmRequest(key)
    }
  }, [clearSecretWarmTimer, dropSecretWarmRequest])

  const commitSecretWarm = useCallback((key: string) => {
    clearSecretWarmTimer(key)
    secretWarmAbortRef.current.delete(key)
  }, [clearSecretWarmTimer])

  const warmTokenSecret = useCallback((id: string) => {
    if (!shouldPrewarmAdminSecretCopy) return
    const key = `token:${id}`
    clearSecretWarmTimer(key)
    if (tokenSecretCacheRef.current.has(id) || tokenSecretRequestCacheRef.current.has(id)) return
    const controller = new AbortController()
    const requestVersion = tokenSecretVersionRef.current.get(id) ?? 0
    secretWarmAbortRef.current.set(key, controller)
    void resolveTokenSecret(id, controller.signal)
      .then((token) => {
        if (secretWarmAbortRef.current.get(key) !== controller) return
        if ((tokenSecretVersionRef.current.get(id) ?? 0) === requestVersion) {
          tokenSecretCacheRef.current.set(id, token)
        }
      })
      .catch(() => undefined)
      .finally(() => {
        if (secretWarmAbortRef.current.get(key) === controller) {
          secretWarmAbortRef.current.delete(key)
        }
      })
  }, [clearSecretWarmTimer, resolveTokenSecret, shouldPrewarmAdminSecretCopy])

  const warmApiKeySecret = useCallback((id: string) => {
    if (!shouldPrewarmAdminSecretCopy) return
    const key = `key:${id}`
    clearSecretWarmTimer(key)
    if (secretCacheRef.current.has(id) || secretRequestCacheRef.current.has(id)) return
    const controller = new AbortController()
    secretWarmAbortRef.current.set(key, controller)
    void resolveApiKeySecret(id, controller.signal)
      .then((secret) => {
        if (secretWarmAbortRef.current.get(key) !== controller) return
        secretCacheRef.current.set(id, secret)
      })
      .catch(() => undefined)
      .finally(() => {
        if (secretWarmAbortRef.current.get(key) === controller) {
          secretWarmAbortRef.current.delete(key)
        }
      })
  }, [clearSecretWarmTimer, resolveApiKeySecret, shouldPrewarmAdminSecretCopy])

  const scheduleSecretWarm = useCallback((key: string, warmup: () => void) => {
    if (!shouldPrewarmAdminSecretCopy) return
    clearSecretWarmTimer(key)
    const timer = window.setTimeout(() => {
      secretWarmTimerRef.current.delete(key)
      warmup()
    }, 120)
    secretWarmTimerRef.current.set(key, timer)
  }, [clearSecretWarmTimer, shouldPrewarmAdminSecretCopy])

  const handleTokenSecretRotated = useCallback((id: string, token: string) => {
    tokenSecretVersionRef.current.set(id, (tokenSecretVersionRef.current.get(id) ?? 0) + 1)
    tokenSecretRequestCacheRef.current.delete(id)
    tokenSecretCacheRef.current.set(id, token)
  }, [])

  const beginManagedRequest = useCallback(
    (ref: { current: AbortController | null }, upstreamSignal?: AbortSignal) => {
      ref.current?.abort()
      const controller = new AbortController()
      ref.current = controller
      const forwardAbort = () => controller.abort()
      if (upstreamSignal) {
        if (upstreamSignal.aborted) {
          controller.abort()
        } else {
          upstreamSignal.addEventListener('abort', forwardAbort, { once: true })
        }
      }
      return {
        signal: controller.signal,
        abort: () => controller.abort(),
        cleanup: () => {
          if (upstreamSignal) {
            upstreamSignal.removeEventListener('abort', forwardAbort)
          }
          if (ref.current === controller) {
            ref.current = null
          }
        },
      }
    },
    [],
  )

  const loadAllTokensForDashboard = useCallback(async (
    signal?: AbortSignal,
  ): Promise<{ items: AuthToken[]; truncated: boolean }> => {
    const perPage = DASHBOARD_TOKENS_PAGE_SIZE
    const maxPages = DASHBOARD_TOKENS_MAX_PAGES
    let page = 1
    let total = Number.POSITIVE_INFINITY
    const items: AuthToken[] = []

    while (page <= maxPages && items.length < total) {
      const result = await fetchTokens(page, perPage, undefined, signal)
      if (signal?.aborted) break
      items.push(...result.items)
      total = result.total
      if (result.items.length < perPage) break
      page += 1
    }

    const truncated = items.length < total
    return { items, truncated }
  }, [])

  const loadExhaustedKeysForDashboard = useCallback(
    async (signal?: AbortSignal): Promise<ApiKeyStats[]> => {
      const result = await fetchApiKeys(
        1,
        DASHBOARD_EXHAUSTED_KEYS_PAGE_SIZE,
        { statuses: ['exhausted'] },
        signal,
      )
      return result.items
    },
    [],
  )

  const handleCopySecret = useCallback(
    async (id: string, stateKey: string, anchorEl?: HTMLElement | null) => {
      setManualCopyBubble(null)
      commitSecretWarm(`key:${id}`)
      updateCopyState(stateKey, 'loading')
      try {
        const hasCachedSecret = secretCacheRef.current.has(id)
        const secret = await resolveApiKeySecret(id)
        const copyResult = await copyToClipboard(
          secret,
          hasCachedSecret ? { preferExecCommand: true } : undefined,
        )
        if (!copyResult.ok) {
          updateCopyState(stateKey, null)
          if (anchorEl) {
            openManualCopyBubble({
              anchorEl,
              title: manualCopyText.title,
              description: manualCopyText.description,
              fieldLabel: manualCopyText.fields.apiKey,
              value: secret,
            })
          }
          return
        }
        setManualCopyBubble(null)
        updateCopyState(stateKey, 'copied')
        window.setTimeout(() => updateCopyState(stateKey, null), 2000)
      } catch (err) {
        console.error(err)
        setError(err instanceof Error ? err.message : errorStrings.copyKey)
        updateCopyState(stateKey, null)
      }
    },
    [
      copyToClipboard,
      errorStrings.copyKey,
      manualCopyText,
      openManualCopyBubble,
      resolveApiKeySecret,
      setError,
      commitSecretWarm,
      updateCopyState,
    ],
  )

  const loadData = useCallback(
    async ({
      signal,
      reason = 'refresh',
      showGlobalLoading = false,
    }: {
      signal?: AbortSignal
      reason?: 'initial' | 'switch' | 'refresh'
      showGlobalLoading?: boolean
    } = {}) => {
      const request = beginManagedRequest(baseDataAbortRef, signal)
      setTokensLoadState(
        reason === 'refresh'
          ? getRefreshingLoadState(baseDataLoadedRef.current)
          : getBlockingLoadState(baseDataLoadedRef.current),
      )
      if (reason !== 'refresh') {
        setTokens([])
        setTokensTotal(0)
      }
      try {
        const [summaryData, ver, profileData, tokenData, tokenGroupsData] = await Promise.all([
          fetchSummary(request.signal),
          fetchVersion(request.signal).catch(() => null),
          fetchProfile(request.signal).catch(() => null),
          fetchTokens(
            tokensPage,
            tokensPerPage,
            { group: selectedTokenGroupName, ungrouped: selectedTokenUngrouped },
            request.signal,
          ).catch(
            () =>
              ({
                items: [],
                total: 0,
                page: tokensPage,
                perPage: tokensPerPage,
              }) as Paginated<AuthToken>,
          ),
          fetchTokenGroups(request.signal).catch(() => [] as TokenGroup[]),
        ])

        if (request.signal.aborted) {
          return
        }

        setProfile(profileData ?? null)
        setSummary(summaryData)
        setTokens(tokenData.items)
        setTokensTotal(tokenData.total)
        setTokenGroups(tokenGroupsData)
        setVersion(ver ?? null)
        setLastUpdated(new Date())
        setError(null)
        setTokensLoadState('ready')
        baseDataLoadedRef.current = true
      } catch (err) {
        if ((err as Error).name === 'AbortError') {
          return
        }
        setError(err instanceof Error ? err.message : 'Unexpected error occurred')
        setTokensLoadState('error')
      } finally {
        if (showGlobalLoading && !request.signal.aborted) {
          setLoading(false)
        }
        request.cleanup()
      }
    },
    [beginManagedRequest, tokensPage, selectedTokenGroupName, selectedTokenUngrouped],
  )

  const loadDashboardOverview = useCallback(
    async (signal?: AbortSignal) => {
      try {
        const [dashboardTokenSnapshot, dashboardKeysData, dashboardLogsData, dashboardJobsData] = await Promise.all([
          loadAllTokensForDashboard(signal)
            .then((value) => ({ kind: 'ok' as const, ...value }))
            .catch(() => ({ kind: 'error' as const })),
          loadExhaustedKeysForDashboard(signal).catch(() => [] as ApiKeyStats[]),
          fetchRequestLogs(1, DASHBOARD_RECENT_LOGS_PER_PAGE, undefined, signal).catch(
            () =>
              ({
                items: [],
                total: 0,
                page: 1,
                perPage: DASHBOARD_RECENT_LOGS_PER_PAGE,
              }) as Paginated<RequestLog>,
          ),
          fetchJobs(1, DASHBOARD_RECENT_JOBS_PER_PAGE, 'all', signal).catch(
            () =>
              ({
                items: [],
                total: 0,
                page: 1,
                perPage: DASHBOARD_RECENT_JOBS_PER_PAGE,
              }) as Paginated<JobLogView>,
          ),
        ])

        if (signal?.aborted) {
          return
        }

        if (dashboardTokenSnapshot.kind === 'ok') {
          setDashboardTokens(dashboardTokenSnapshot.items)
          setDashboardTokenCoverage(dashboardTokenSnapshot.truncated ? 'truncated' : 'ok')
        } else {
          setDashboardTokens([])
          setDashboardTokenCoverage('error')
        }
        setDashboardKeys(dashboardKeysData)
        setDashboardLogs(dashboardLogsData.items)
        setDashboardJobs(dashboardJobsData.items)
      } catch (err) {
        if ((err as Error).name === 'AbortError') {
          return
        }
        setDashboardTokens([])
        setDashboardTokenCoverage('error')
        setDashboardKeys([])
        setDashboardLogs([])
        setDashboardJobs([])
      } finally {
        if (!(signal?.aborted ?? false)) {
          setDashboardOverviewLoaded(true)
        }
      }
    },
    [loadAllTokensForDashboard, loadExhaustedKeysForDashboard],
  )

  useEffect(() => {
    routeRef.current = route
  }, [route])

  useEffect(() => {
    loadDashboardOverviewRef.current = loadDashboardOverview
  }, [loadDashboardOverview])

  const loadTokenLeaderboard = useCallback(
    async ({
      signal,
      reason = 'refresh',
    }: {
      signal?: AbortSignal
      reason?: 'initial' | 'switch' | 'refresh'
    } = {}) => {
      try {
        setTokenLeaderboardLoadState(
          reason === 'refresh'
            ? getRefreshingLoadState(tokenLeaderboardQueryKeyRef.current != null)
            : getBlockingLoadState(tokenLeaderboardQueryKeyRef.current != null),
        )
        setTokenLeaderboardError(null)
        if (reason !== 'refresh') {
          setTokenLeaderboard([])
        }
        const items = await fetchTokenUsageLeaderboard(
          tokenLeaderboardPeriod,
          tokenLeaderboardFocus,
          signal,
        )
        if (signal?.aborted) return
        const sorted = sortLeaderboard(items, tokenLeaderboardPeriod, tokenLeaderboardFocus).slice(0, 50)
        setTokenLeaderboard(sorted)
        setTokenLeaderboardLoadState('ready')
        tokenLeaderboardQueryKeyRef.current = `${tokenLeaderboardPeriod}:${tokenLeaderboardFocus}`
        tokenLeaderboardNonceRef.current = tokenLeaderboardNonce
      } catch (err) {
        if (signal?.aborted) return
        console.error(err)
        setTokenLeaderboard([])
        setTokenLeaderboardError(err instanceof Error ? err.message : tokenLeaderboardStrings.error)
        setTokenLeaderboardLoadState('error')
      }
  },
    [tokenLeaderboardFocus, tokenLeaderboardNonce, tokenLeaderboardPeriod, tokenLeaderboardStrings.error],
  )

  useEffect(() => {
    const controller = new AbortController()
    if (!baseDataLoadedRef.current) {
      setLoading(true)
    }
    void loadData({
      signal: controller.signal,
      reason: baseDataLoadedRef.current ? 'switch' : 'initial',
      showGlobalLoading: !baseDataLoadedRef.current,
    })
    return () => controller.abort()
  }, [loadData])

  useEffect(() => {
    if (!(route.name === 'module' && route.module === 'dashboard')) {
      return
    }
    const controller = new AbortController()
    dashboardOverviewLastSseRefreshAtRef.current = Date.now()
    void loadDashboardOverview(controller.signal)
    return () => controller.abort()
  }, [route, loadDashboardOverview])

  useEffect(() => {
    const controller = new AbortController()
    const queryKey = `${tokenLeaderboardPeriod}:${tokenLeaderboardFocus}`
    const isRefreshOnly =
      tokenLeaderboardQueryKeyRef.current === queryKey && tokenLeaderboardNonceRef.current !== tokenLeaderboardNonce
    void loadTokenLeaderboard({
      signal: controller.signal,
      reason: isRefreshOnly ? 'refresh' : tokenLeaderboardQueryKeyRef.current ? 'switch' : 'initial',
    })
    return () => controller.abort()
  }, [loadTokenLeaderboard, tokenLeaderboardFocus, tokenLeaderboardNonce, tokenLeaderboardPeriod])

  // Logs list: backend pagination & result filter
  useEffect(() => {
    const request = beginManagedRequest(requestsAbortRef)
    const resultParam =
      logResultFilter === 'all' ? undefined : (logResultFilter as 'success' | 'error' | 'quota_exhausted')
    setRequestsLoadState(getBlockingLoadState(requestsLoadedRef.current))
    setRequestsError(null)
    setLogs([])
    setLogsTotal(0)
    setExpandedLogs(new Set())

    fetchRequestLogs(logsPage, LOGS_PER_PAGE, resultParam, request.signal)
      .then((result) => {
        if (request.signal.aborted) return
        setLogs(result.items)
        setLogsTotal(result.total)
        setRequestsLoadState('ready')
        requestsLoadedRef.current = true
      })
      .catch((err) => {
        if (request.signal.aborted) return
        console.error(err)
        setLogs([])
        setLogsTotal(0)
        setRequestsError(err instanceof Error ? err.message : loadingStateStrings.error)
        setRequestsLoadState('error')
      })
      .finally(() => {
        request.cleanup()
      })

    return () => {
      request.abort()
      request.cleanup()
    }
  }, [beginManagedRequest, logsPage, logResultFilter])

  // Jobs list: refetch when filter or page changes
  useEffect(() => {
    const request = beginManagedRequest(jobsAbortRef)
    setJobsLoadState(getBlockingLoadState(jobsLoadedRef.current))
    setJobsError(null)
    setJobs([])
    setJobsTotal(0)
    setExpandedJobs(new Set())
    fetchJobs(jobsPage, jobsPerPage, jobFilter, request.signal)
      .then((result) => {
        if (!request.signal.aborted) {
          setJobs(result.items)
          setJobsTotal(result.total)
          setJobsLoadState('ready')
          jobsLoadedRef.current = true
        }
      })
      .catch(() => {
        if (!request.signal.aborted) {
          setJobs([])
          setJobsTotal(0)
          setJobsError(loadingStateStrings.error)
          setJobsLoadState('error')
        }
      })
      .finally(() => {
        request.cleanup()
      })
    return () => {
      request.abort()
      request.cleanup()
    }
  }, [beginManagedRequest, jobFilter, jobsPage])

  useEffect(() => {
    if (!(route.name === 'module' && route.module === 'keys')) return

    const request = beginManagedRequest(keysAbortRef)
    const nextQueryKey = `${keysPage}:${keysPerPage}:${selectedKeyGroups.join('\u0000')}:${selectedKeyStatuses.join('\u0000')}`
    const sameQueryRefresh = keysLoadedRef.current && keysQueryKeyRef.current === nextQueryKey
    setKeysLoadState(
      sameQueryRefresh ? getRefreshingLoadState(true) : getBlockingLoadState(keysLoadedRef.current),
    )
    setKeysError(null)
    if (!sameQueryRefresh) {
      setKeys([])
      setKeysTotal(0)
    }

    fetchApiKeys(
      keysPage,
      keysPerPage,
      {
        groups: selectedKeyGroups,
        statuses: selectedKeyStatuses,
      },
      request.signal,
    )
      .then((result) => {
        if (request.signal.aborted) return
        setKeys(result.items)
        setKeysTotal(result.total)
        setKeysPage(result.page)
        setKeysPerPage(result.perPage)
        setKeyGroupFacets(result.facets.groups)
        setKeyStatusFacets(result.facets.statuses)
        setKeysLoadState('ready')
        keysLoadedRef.current = true
        keysQueryKeyRef.current = nextQueryKey
        const normalizedLocation = buildAdminKeysPath({
          page: result.page,
          perPage: result.perPage,
          groups: selectedKeyGroups,
          statuses: selectedKeyStatuses,
        })
        const currentLocation = `${window.location.pathname}${window.location.search}`
        if (currentLocation !== normalizedLocation) {
          window.history.replaceState(null, '', normalizedLocation)
        }
      })
      .catch((err) => {
        if (request.signal.aborted) return
        console.error(err)
        setKeys([])
        setKeysTotal(0)
        setKeyGroupFacets([])
        setKeyStatusFacets([])
        setKeysError(err instanceof Error ? err.message : loadingStateStrings.error)
        setKeysLoadState('error')
      })
      .finally(() => {
        request.cleanup()
      })

    return () => {
      request.abort()
      request.cleanup()
    }
  }, [beginManagedRequest, keysPage, keysPerPage, loadingStateStrings.error, route, selectedKeyGroups, selectedKeyStatuses])

  useEffect(() => {
    const timer = window.setTimeout(() => {
      const normalized = usersQueryInput.trim()
      setUsersQuery((previous) => {
        if (previous === normalized) return previous
        setUsersPage(1)
        return normalized
      })
    }, 250)
    return () => window.clearTimeout(timer)
  }, [usersQueryInput])

  useEffect(() => {
    const usersRouteActive =
      (route.name === 'module' && route.module === 'users') || route.name === 'user'
    if (!usersRouteActive) return

    const request = beginManagedRequest(usersAbortRef)
    const nextQueryKey = `${usersPage}:${usersQuery}:${usersTagFilterId ?? ''}`
    const sameQueryRefresh = usersLoadedRef.current && usersQueryKeyRef.current === nextQueryKey
    setUsersLoadState(
      sameQueryRefresh ? getRefreshingLoadState(true) : getBlockingLoadState(usersLoadedRef.current),
    )
    setUsersError(null)
    if (!sameQueryRefresh) {
      setUsers([])
      setUsersTotal(0)
    }
    fetchAdminUsers(usersPage, USERS_PER_PAGE, usersQuery, usersTagFilterId, request.signal)
      .then((result) => {
        if (request.signal.aborted) return
        setUsers(result.items)
        setUsersTotal(result.total)
        setUsersLoadState('ready')
        usersLoadedRef.current = true
        usersQueryKeyRef.current = nextQueryKey
      })
      .catch((err) => {
        if (request.signal.aborted) return
        console.error(err)
        setUsers([])
        setUsersTotal(0)
        setUsersError(err instanceof Error ? err.message : loadingStateStrings.error)
        setUsersLoadState('error')
      })
      .finally(() => {
        request.cleanup()
      })

    return () => {
      request.abort()
      request.cleanup()
    }
  }, [beginManagedRequest, route, usersPage, usersQuery, usersTagFilterId])

  useEffect(() => {
    const userTagRouteActive =
      (route.name === 'module' && route.module === 'users')
      || route.name === 'user'
      || route.name === 'user-tags'
      || route.name === 'user-tag-editor'
    if (!userTagRouteActive) return

    const controller = new AbortController()
    setTagCatalogLoading(true)
    setTagCatalogError(null)
    fetchAdminUserTags(controller.signal)
      .then((tags) => {
        if (controller.signal.aborted) return
        setTagCatalog(tags)
      })
      .catch((err) => {
        if (controller.signal.aborted) return
        console.error(err)
        setTagCatalog([])
        setTagCatalogError(err instanceof Error ? err.message : adminStrings.users.catalog.loadFailed)
      })
      .finally(() => {
        if (!controller.signal.aborted) {
          setTagCatalogLoading(false)
          setTagCatalogLoadedOnce(true)
        }
      })

    return () => controller.abort()
  }, [route, adminStrings.users.catalog.loadFailed])

  useEffect(() => {
    const usersRouteActive =
      (route.name === 'module' && route.module === 'users')
      || route.name === 'user'
      || route.name === 'user-tags'
      || route.name === 'user-tag-editor'
    if (!usersRouteActive) return

    const controller = new AbortController()
    setRegistrationSettingsLoading(true)
    setRegistrationSettingsError(null)
    fetchAdminRegistrationSettings(controller.signal)
      .then((settings) => {
        if (controller.signal.aborted) return
        setAllowRegistration(settings.allowRegistration)
        setRegistrationSettingsLoaded(true)
      })
      .catch((err) => {
        if (controller.signal.aborted) return
        console.error(err)
        setAllowRegistration(null)
        setRegistrationSettingsError(
          err instanceof Error ? err.message : adminStrings.users.registration.loadFailed,
        )
      })
      .finally(() => {
        if (!controller.signal.aborted) {
          setRegistrationSettingsLoading(false)
        }
      })

    return () => controller.abort()
  }, [route, adminStrings.users.registration.loadFailed])

  useEffect(() => {
    if (!(route.name === 'module' && route.module === 'users')) return
    const locationQuery = getAdminUsersQueryFromLocation()
    const locationTagFilterId = getAdminUsersTagFilterFromLocation()
    const locationPage = getAdminUsersPageFromLocation()
    setUsersPage((previous) => (previous === locationPage ? previous : locationPage))
    setUsersQueryInput((previous) => (previous === locationQuery ? previous : locationQuery))
    setUsersQuery((previous) => (previous === locationQuery ? previous : locationQuery))
    setUsersTagFilterId((previous) => (previous === locationTagFilterId ? previous : locationTagFilterId))
  }, [route])

  useEffect(() => {
    if (!(route.name === 'module' && route.module === 'keys')) return
    const locationPage = getAdminKeysPageFromLocation()
    const locationPerPage = getAdminKeysPerPageFromLocation()
    const locationGroups = getAdminKeysValuesFromLocation('group')
    const locationStatuses = getAdminKeysValuesFromLocation('status')
    setKeysPage((previous) => (previous === locationPage ? previous : locationPage))
    setKeysPerPage((previous) => (previous === locationPerPage ? previous : locationPerPage))
    setSelectedKeyGroups((previous) =>
      previous.length === locationGroups.length && previous.every((value, index) => value === locationGroups[index])
        ? previous
        : locationGroups,
    )
    setSelectedKeyStatuses((previous) =>
      previous.length === locationStatuses.length && previous.every((value, index) => value === locationStatuses[index])
        ? previous
        : locationStatuses,
    )
  }, [route])

  useEffect(() => {
    if (route.name !== 'user-tag-editor') return

    if (route.mode === 'create') {
      setActiveUserTagEditorId(NEW_USER_TAG_CARD_ID)
      setUserTagCatalogDraft({ ...EMPTY_USER_TAG_FORM })
      setTagCatalogError(null)
      return
    }

    const editingTag = tagCatalog.find((tag) => tag.id === route.id)
    if (editingTag) {
      setActiveUserTagEditorId(editingTag.id)
      setUserTagCatalogDraft(createUserTagFormState(editingTag))
      setTagCatalogError(null)
    } else if (tagCatalogLoadedOnce && !tagCatalogLoading) {
      setActiveUserTagEditorId(null)
      setTagCatalogError(adminStrings.users.catalog.tagNotFound)
    }
  }, [route, tagCatalog, tagCatalogLoadedOnce, tagCatalogLoading, adminStrings.users.catalog.tagNotFound])

  useEffect(() => {
    if (route.name !== 'user') return
    const controller = new AbortController()
    setUserDetailLoading(true)
    setUserQuotaError(null)
    fetchAdminUserDetail(route.id, controller.signal)
      .then((detail) => {
        if (controller.signal.aborted) return
        setSelectedUserDetail(detail)
        setUserQuotaSnapshot(buildUserQuotaSnapshot(detail))
        setUserQuotaDraft({
          hourlyAnyLimit: String(detail.quotaBase.hourlyAnyLimit),
          hourlyLimit: String(detail.quotaBase.hourlyLimit),
          dailyLimit: String(detail.quotaBase.dailyLimit),
          monthlyLimit: String(detail.quotaBase.monthlyLimit),
        })
        setSelectedBindableTagId('')
        setUserTagError(null)
      })
      .catch((err) => {
        if (controller.signal.aborted) return
        console.error(err)
        setSelectedUserDetail(null)
        setUserQuotaSnapshot(null)
        setUserQuotaDraft(null)
      })
      .finally(() => {
        if (!controller.signal.aborted) {
          setUserDetailLoading(false)
        }
      })
    return () => controller.abort()
  }, [route])

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
        const tasks: Array<Promise<unknown>> = [loadData({ signal: controller.signal, reason: 'refresh' })]
        if (route.name === 'module' && route.module === 'dashboard') {
          tasks.push(loadDashboardOverview(controller.signal))
        }
        void Promise.all(tasks).finally(() => controller.abort())
      }, REFRESH_INTERVAL_MS) as unknown as number
    }

    return () => {
      if (pollingTimerRef.current != null) {
        window.clearInterval(pollingTimerRef.current)
        pollingTimerRef.current = null
      }
    }
  }, [sseConnected, loadData, loadDashboardOverview, route])

  // Detect whether the collapsed token groups row overflows horizontally.
  // If everything fits in a single line, we hide the "more" toggle button.
  useEffect(() => {
    if (!Array.isArray(tokenGroups) || tokenGroups.length === 0 || tokenGroupsExpanded) {
      setTokenGroupsCollapsedOverflowing(false)
      return
    }
    const el = tokenGroupsListRef.current
    if (!el) return

    const measure = () => {
      const overflowing = el.scrollWidth > el.clientWidth
      setTokenGroupsCollapsedOverflowing(overflowing)
    }

    measure()
    window.addEventListener('resize', measure)
    return () => window.removeEventListener('resize', measure)
  }, [tokenGroups, tokenGroupsExpanded, selectedTokenGroupName, selectedTokenUngrouped])

  // Establish SSE connection to receive live dashboard updates
  useEffect(() => {
    let es: EventSource | null = null

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
          setDashboardKeys(data.keys)
          setDashboardLogs(data.logs)
          setLastUpdated(new Date())
          setError(null)
          setLoading(false)
          const canRefreshOverview =
            Date.now() - dashboardOverviewLastSseRefreshAtRef.current >=
            DASHBOARD_OVERVIEW_SSE_REFRESH_INTERVAL_MS
          if (
            routeRef.current.name === 'module' &&
            routeRef.current.module === 'dashboard' &&
            !dashboardOverviewInFlightRef.current &&
            canRefreshOverview
          ) {
            const refreshOverview = loadDashboardOverviewRef.current
            if (refreshOverview) {
              dashboardOverviewLastSseRefreshAtRef.current = Date.now()
              dashboardOverviewInFlightRef.current = true
              const controller = new AbortController()
              void refreshOverview(controller.signal).finally(() => {
                controller.abort()
                dashboardOverviewInFlightRef.current = false
              })
            }
          }
        } catch (e) {
          console.error('SSE parse error', e)
        }
      })
    }

    connect()
    return () => {
      if (es) {
        try { es.close() } catch {}
      }
      setSseConnected(false)
    }
  }, [])

  useEffect(() => {
    const onPopState = () => {
      setRoute(parseAdminPath(window.location.pathname))
    }
    window.addEventListener('popstate', onPopState)
    return () => window.removeEventListener('popstate', onPopState)
  }, [])

  const navigateToPath = useCallback((path: string) => {
    const nextUrl = new URL(path, window.location.origin)
    const nextRoute = parseAdminPath(nextUrl.pathname)
    const nextLocation = `${nextUrl.pathname}${nextUrl.search}${nextUrl.hash}`
    const currentLocation = `${window.location.pathname}${window.location.search}${window.location.hash}`
    if (currentLocation !== nextLocation) {
      window.history.pushState(null, '', nextLocation)
    }
    setRoute((previous) => (isSameAdminRoute(previous, nextRoute) ? previous : nextRoute))
  }, [])

  const navigateModule = useCallback(
    (module: AdminModuleId) => {
      navigateToPath(modulePath(module))
    },
    [navigateToPath],
  )

  const navigateKey = useCallback(
    (id: string, options?: { preserveKeysContext?: boolean }) => {
      if (options?.preserveKeysContext) {
        navigateToPath(
          keyDetailPath(id, {
            page: keysPage,
            perPage: keysPerPage,
            groups: selectedKeyGroups,
            statuses: selectedKeyStatuses,
          }),
        )
        return
      }
      navigateToPath(keyDetailPath(id))
    },
    [keysPage, keysPerPage, navigateToPath, selectedKeyGroups, selectedKeyStatuses],
  )

  const navigateToken = useCallback(
    (id: string) => {
      navigateToPath(tokenDetailPath(id))
    },
    [navigateToPath],
  )

  const navigateUser = useCallback(
    (id: string, options?: { preserveUsersContext?: boolean }) => {
      if (options?.preserveUsersContext) {
        navigateToPath(userDetailPath(id, usersQuery, usersTagFilterId, usersPage))
        return
      }
      navigateToPath(userDetailPath(id))
    },
    [navigateToPath, usersPage, usersQuery, usersTagFilterId],
  )

  const navigateUsersSearch = useCallback(
    (query: string, options?: { tagId?: string | null; page?: number | null }) => {
      const normalized = query.trim()
      const normalizedTagId = options?.tagId?.trim() ?? null
      const normalizedPage = options?.page != null ? Math.max(1, options.page) : 1
      setUsersPage(normalizedPage)
      setUsersQueryInput(normalized)
      setUsersQuery(normalized)
      setUsersTagFilterId(normalizedTagId)
      navigateToPath(buildAdminUsersPath(normalized, normalizedTagId, normalizedPage))
    },
    [navigateToPath],
  )

  const navigateKeysList = useCallback(
    (options?: {
      page?: number | null
      perPage?: number | null
      groups?: string[] | null
      statuses?: string[] | null
    }) => {
      const page = options?.page != null ? Math.max(1, Math.trunc(options.page)) : 1
      const perPage = options?.perPage != null ? Math.max(1, Math.trunc(options.perPage)) : DEFAULT_KEYS_PER_PAGE
      const groups = Array.from(new Set((options?.groups ?? []).map((value) => value.trim())))
      const statuses = Array.from(
        new Set((options?.statuses ?? []).map((value) => value.trim()).filter((value) => value.length > 0)),
      )

      setKeysPage(page)
      setKeysPerPage(perPage)
      setSelectedKeyGroups(groups)
      setSelectedKeyStatuses(statuses)
      navigateToPath(
        buildAdminKeysPath({
          page,
          perPage,
          groups,
          statuses,
        }),
      )
    },
    [navigateToPath],
  )

  const navigateUserTags = useCallback(() => {
    const query = route.name === 'module' && route.module === 'users' ? usersQuery : getAdminUsersQueryFromLocation()
    const tagId = route.name === 'module' && route.module === 'users'
      ? usersTagFilterId
      : getAdminUsersTagFilterFromLocation()
    const page = route.name === 'module' && route.module === 'users' ? usersPage : getAdminUsersPageFromLocation()
    navigateToPath(userTagsPath(query, tagId, page))
  }, [navigateToPath, route, usersPage, usersQuery, usersTagFilterId])

  const navigateUserTagCreate = useCallback(() => {
    setActiveUserTagEditorId(NEW_USER_TAG_CARD_ID)
    setUserTagCatalogDraft({ ...EMPTY_USER_TAG_FORM })
    setTagCatalogError(null)
    navigateToPath(
      userTagCreatePath(
        getAdminUsersQueryFromLocation(),
        getAdminUsersTagFilterFromLocation(),
        getAdminUsersPageFromLocation(),
      ),
    )
  }, [navigateToPath])

  const navigateUserTagEdit = useCallback(
    (id: string) => {
      const editingTag = tagCatalog.find((tag) => tag.id === id)
      if (editingTag) {
        setActiveUserTagEditorId(editingTag.id)
        setUserTagCatalogDraft(createUserTagFormState(editingTag))
        setTagCatalogError(null)
      }
      navigateToPath(
        userTagEditPath(
          id,
          getAdminUsersQueryFromLocation(),
          getAdminUsersTagFilterFromLocation(),
          getAdminUsersPageFromLocation(),
        ),
      )
    },
    [navigateToPath, tagCatalog],
  )

  const navigateTokenLeaderboard = useCallback(() => {
    navigateToPath(tokenLeaderboardPath())
  }, [navigateToPath])

  const handleManualRefresh = () => {
    const controller = new AbortController()
    setLoading(true)
    setTokenLeaderboardNonce((value) => value + 1)
    const tasks: Array<Promise<unknown>> = [loadData({ signal: controller.signal, reason: 'refresh', showGlobalLoading: true })]
    if (route.name === 'module' && route.module === 'requests') {
      const request = beginManagedRequest(requestsAbortRef, controller.signal)
      setRequestsLoadState(getRefreshingLoadState(requestsLoadedRef.current))
      setRequestsError(null)
      tasks.push(
        fetchRequestLogs(
          logsPage,
          LOGS_PER_PAGE,
          logResultFilter === 'all' ? undefined : (logResultFilter as 'success' | 'error' | 'quota_exhausted'),
          request.signal,
        )
          .then((result) => {
            if (request.signal.aborted) return
            setLogs(result.items)
            setLogsTotal(result.total)
            setRequestsLoadState('ready')
          })
          .catch((err) => {
            if (request.signal.aborted) return
            console.error(err)
            setRequestsError(err instanceof Error ? err.message : loadingStateStrings.error)
            setRequestsLoadState('error')
          })
          .finally(() => {
            request.cleanup()
          }),
      )
    }
    if (route.name === 'module' && route.module === 'jobs') {
      const request = beginManagedRequest(jobsAbortRef, controller.signal)
      setJobsLoadState(getRefreshingLoadState(jobsLoadedRef.current))
      setJobsError(null)
      tasks.push(
        fetchJobs(jobsPage, jobsPerPage, jobFilter, request.signal).then((result) => {
          if (request.signal.aborted) return
          setJobs(result.items)
          setJobsTotal(result.total)
          setJobsLoadState('ready')
        }).catch((err) => {
          if (request.signal.aborted) return
          console.error(err)
          setJobsError(err instanceof Error ? err.message : loadingStateStrings.error)
          setJobsLoadState('error')
        }).finally(() => {
          request.cleanup()
        }),
      )
    }
    if ((route.name === 'module' && route.module === 'users') || route.name === 'user') {
      const request = beginManagedRequest(usersAbortRef, controller.signal)
      setUsersLoadState(getRefreshingLoadState(usersLoadedRef.current))
      setUsersError(null)
      tasks.push(
        fetchAdminUsers(usersPage, USERS_PER_PAGE, usersQuery, usersTagFilterId, request.signal).then((result) => {
          if (request.signal.aborted) return
          setUsers(result.items)
          setUsersTotal(result.total)
          setUsersLoadState('ready')
        }).catch((err) => {
          if (request.signal.aborted) return
          console.error(err)
          setUsersError(err instanceof Error ? err.message : loadingStateStrings.error)
          setUsersLoadState('error')
        }).finally(() => {
          request.cleanup()
        }),
      )
    }
    if (route.name === 'module' && route.module === 'keys') {
      const request = beginManagedRequest(keysAbortRef, controller.signal)
      const nextQueryKey = `${keysPage}:${keysPerPage}:${selectedKeyGroups.join('\u0000')}:${selectedKeyStatuses.join('\u0000')}`
      setKeysLoadState(getRefreshingLoadState(keysLoadedRef.current))
      setKeysError(null)
      tasks.push(
        fetchApiKeys(
          keysPage,
          keysPerPage,
          {
            groups: selectedKeyGroups,
            statuses: selectedKeyStatuses,
          },
          request.signal,
        )
          .then((result) => {
            if (request.signal.aborted) return
            setKeys(result.items)
            setKeysTotal(result.total)
            setKeysPage(result.page)
            setKeysPerPage(result.perPage)
            setKeyGroupFacets(result.facets.groups)
            setKeyStatusFacets(result.facets.statuses)
            setKeysLoadState('ready')
            keysLoadedRef.current = true
            keysQueryKeyRef.current = nextQueryKey
            const normalizedLocation = buildAdminKeysPath({
              page: result.page,
              perPage: result.perPage,
              groups: selectedKeyGroups,
              statuses: selectedKeyStatuses,
            })
            const currentLocation = `${window.location.pathname}${window.location.search}`
            if (currentLocation !== normalizedLocation) {
              window.history.replaceState(null, '', normalizedLocation)
            }
          })
          .catch((err) => {
            if (request.signal.aborted) return
            console.error(err)
            setKeysError(err instanceof Error ? err.message : loadingStateStrings.error)
            setKeysLoadState('error')
          })
          .finally(() => {
            request.cleanup()
          }),
      )
    }
    if (route.name === 'module' && route.module === 'dashboard') {
      tasks.push(loadDashboardOverview(controller.signal))
    }
    void Promise.all(tasks).finally(() => controller.abort())
  }

  const metrics = useMemo(() => {
    if (!summary) {
      return []
    }

    const total = summary.total_requests
    return [
      {
        id: 'total',
        label: metricsStrings.labels.total,
        value: formatNumber(summary.total_requests),
        subtitle: '—',
      },
      {
        id: 'success',
        label: metricsStrings.labels.success,
        value: formatNumber(summary.success_count),
        subtitle: formatPercent(summary.success_count, total),
      },
      {
        id: 'errors',
        label: metricsStrings.labels.errors,
        value: formatNumber(summary.error_count),
        subtitle: formatPercent(summary.error_count, total),
      },
      {
        id: 'quota',
        label: metricsStrings.labels.quota,
        value: formatNumber(summary.quota_exhausted_count),
        subtitle: formatPercent(summary.quota_exhausted_count, total),
      },
      {
        id: 'remaining',
        label: metricsStrings.labels.remaining,
        value: `${formatNumber(summary.total_quota_remaining)} / ${formatNumber(summary.total_quota_limit)}`,
        subtitle:
          summary.total_quota_limit > 0
            ? formatPercent(summary.total_quota_remaining, summary.total_quota_limit)
            : '—',
      },
      {
        id: 'keys',
        label: metricsStrings.labels.keys,
        value: formatNumber(summary.active_keys),
        subtitle: metricsStrings.subtitles.keysAvailability
          .replace('{active}', formatNumber(summary.active_keys))
          .replace('{quarantined}', formatNumber(summary.quarantined_keys))
          .replace('{exhausted}', formatNumber(summary.exhausted_keys)),
      },
      {
        id: 'quarantined',
        label: metricsStrings.labels.quarantined,
        value: formatNumber(summary.quarantined_keys),
        subtitle:
          summary.quarantined_keys === 0
            ? metricsStrings.subtitles.keysAll
            : keyStrings.quarantine.badge,
      },
    ]
  }, [keyStrings.quarantine.badge, metricsStrings, summary])

  const namedKeyGroups = keyGroupFacets
    .filter((group) => group.value.trim().length > 0)
    .map((group) => ({ name: group.value, keyCount: group.count }))
  const hasKeyGroups = keyGroupFacets.length > 0

  const keyGroupFilterOptions = useMemo(
    () =>
      keyGroupFacets.map((group) => ({
        value: group.value,
        label: group.value.trim().length > 0 ? group.value : keyStrings.groups.ungrouped,
        count: group.count,
      })),
    [keyGroupFacets, keyStrings.groups.ungrouped],
  )

  const keyStatusFilterOptions = useMemo(
    () =>
      keyStatusFacets
        .map((status) => ({
          value: status.value,
          label: statusLabel(status.value, adminStrings),
          count: status.count,
        }))
        .sort((left, right) => left.label.localeCompare(right.label)),
    [adminStrings, keyStatusFacets],
  )

  const ungroupedTokenGroup = tokenGroups.find((group) => group.name.trim().length === 0)
  const namedTokenGroups = tokenGroups.filter((group) => group.name.trim().length > 0)
  const hasTokenGroups = tokenGroups.length > 0

  const tokenList = useMemo(() => {
    if (selectedTokenUngrouped) {
      return tokens.filter((item) => (item.group ?? '').trim().length === 0)
    }
    if (selectedTokenGroupName != null) {
      return tokens.filter((item) => (item.group ?? '').trim() === selectedTokenGroupName)
    }
    return tokens
  }, [selectedTokenGroupName, selectedTokenUngrouped, tokens])

  const visibleKeys = keys

  const selectedKeyGroupLabels = useMemo(
    () =>
      keyGroupFilterOptions
        .filter((option) => selectedKeyGroups.includes(option.value))
        .map((option) => option.label),
    [keyGroupFilterOptions, selectedKeyGroups],
  )

  const selectedKeyStatusLabels = useMemo(
    () =>
      keyStatusFilterOptions
        .filter((option) => selectedKeyStatuses.includes(option.value))
        .map((option) => option.label),
    [keyStatusFilterOptions, selectedKeyStatuses],
  )

  const keyGroupFilterSummary = useMemo(
    () =>
      summarizeFilterSelection(
        keyStrings.groups.label,
        selectedKeyGroupLabels,
        keyStrings.groups.all,
        keyStrings.filters.selectedSuffix,
      ),
    [keyStrings.filters.selectedSuffix, keyStrings.groups.all, keyStrings.groups.label, selectedKeyGroupLabels],
  )

  const keyStatusFilterSummary = useMemo(
    () =>
      summarizeFilterSelection(
        keyStrings.filters.status,
        selectedKeyStatusLabels,
        keyStrings.groups.all,
        keyStrings.filters.selectedSuffix,
      ),
    [keyStrings.filters.selectedSuffix, keyStrings.filters.status, keyStrings.groups.all, selectedKeyStatusLabels],
  )

  const keysBatchParsed = useMemo(() => {
    return extractTvlyDevApiKeysFromText(newKeysText)
  }, [newKeysText])

  const keysBatchFirstLine = useMemo(() => {
    return newKeysText.split(/\r?\n/)[0] ?? ''
  }, [newKeysText])

  const keysBatchFailures = useMemo(() => {
    if (!keysBatchReport || keysBatchReport.kind !== 'success') return []
    return keysBatchReport.response.results.filter((item) => item.status === 'failed')
  }, [keysBatchReport])

  const keysValidationImportedSet = useMemo(
    () => new Set(keysValidation?.imported_api_keys ?? []),
    [keysValidation?.imported_api_keys],
  )

  const keysValidationVisibleRows = useMemo(() => {
    const rows = keysValidation?.rows ?? []
    if (keysValidationImportedSet.size === 0) return rows
    return rows.filter((row) => !keysValidationImportedSet.has(row.api_key))
  }, [keysValidation?.rows, keysValidationImportedSet])

  const keysValidationVisibleState = useMemo(() => {
    if (!keysValidation) return null
    if (keysValidationImportedSet.size === 0) return keysValidation
    const uniqueVisible = new Set<string>()
    let duplicateVisible = 0
    for (const row of keysValidationVisibleRows) {
      if (row.status === 'duplicate_in_input') {
        duplicateVisible += 1
      } else {
        uniqueVisible.add(row.api_key)
      }
    }
    return {
      ...keysValidation,
      rows: keysValidationVisibleRows,
      unique_in_input: uniqueVisible.size,
      duplicate_in_input: duplicateVisible,
    }
  }, [keysValidation, keysValidationImportedSet, keysValidationVisibleRows])

  const keysValidationCounts = useMemo(() => {
    const rows = keysValidationVisibleRows
    let pending = 0
    let duplicate = 0
    let ok = 0
    let exhausted = 0
    let invalid = 0
    let errorCount = 0
    for (const row of rows) {
      switch (row.status) {
        case 'pending':
          pending += 1
          break
        case 'duplicate_in_input':
          duplicate += 1
          break
        case 'ok':
          ok += 1
          break
        case 'ok_exhausted':
          exhausted += 1
          break
        case 'unauthorized':
        case 'forbidden':
        case 'invalid':
          invalid += 1
          break
        case 'error':
          errorCount += 1
          break
      }
    }
    const checked = ok + exhausted + invalid + errorCount
    const totalToCheck = new Set(
      rows
        .filter((row) => row.status !== 'duplicate_in_input')
        .map((row) => row.api_key),
    ).size
    return { pending, duplicate, ok, exhausted, invalid, error: errorCount, checked, totalToCheck }
  }, [keysValidationVisibleRows])

  const keysValidationValidKeys = useMemo(() => {
    const set = new Set<string>()
    for (const row of keysValidationVisibleRows) {
      if (row.status === 'ok' || row.status === 'ok_exhausted') {
        set.add(row.api_key)
      }
    }
    return Array.from(set)
  }, [keysValidationVisibleRows])

  const keysValidationExhaustedKeys = useMemo(() => {
    const set = new Set<string>()
    for (const row of keysValidationVisibleRows) {
      if (row.status === 'ok_exhausted') {
        set.add(row.api_key)
      }
    }
    return Array.from(set)
  }, [keysValidationVisibleRows])

  const updateKeysBatchOverlayLayout = useCallback(() => {
    if (!keysBatchExpanded) return
    const anchor = keysBatchAnchorRef.current
    const anchorInput = keysBatchCollapsedInputRef.current
    const overlay = keysBatchOverlayRef.current
    const textarea = keysBatchTextareaRef.current
    if (!anchor || !overlay || !textarea) return

    const anchorRect = anchor.getBoundingClientRect()
    const layoutAnchorRect = (anchorInput ?? anchor).getBoundingClientRect()
    const visualViewport = window.visualViewport
    const visibleBottom = visualViewport ? visualViewport.offsetTop + visualViewport.height : window.innerHeight

    const viewportWidth = window.innerWidth
    // The overlay is the "expanded" version of the collapsed controls:
    // keep its position anchored to the original control, but expand the card width for a proper
    // multi-line paste experience (matching the pre-existing wide overlay feel).
    const overlayWidth = Math.max(0, Math.min(720, viewportWidth - 32))
    const leftMin = 16
    const leftMax = Math.max(leftMin, viewportWidth - leftMin - overlayWidth)
    // Right-align to the collapsed control group so expansion grows leftwards instead of jumping
    // off-screen (the controls live on the right side of the header).
    const preferredLeft = anchorRect.right - overlayWidth
    const left = Math.min(leftMax, Math.max(leftMin, preferredLeft))
    const topPreferred = layoutAnchorRect.top

    overlay.style.left = `${Math.round(left)}px`
    overlay.style.width = `${Math.round(overlayWidth)}px`

    const fitTextarea = () => {
      // Reset first so scrollHeight reflects full content.
      textarea.style.height = 'auto'

      const textareaRect = textarea.getBoundingClientRect()
      const footer = keysBatchFooterRef.current
      const footerHeight = footer ? footer.getBoundingClientRect().height : 0

      const viewportBottom = visibleBottom - 16
      const documentBottom = document.documentElement.scrollHeight - window.scrollY - 16
      const availableBottom = Math.min(viewportBottom, documentBottom)

      const maxTextareaHeight = Math.max(120, availableBottom - textareaRect.top - footerHeight - 24)
      const desiredHeight = Math.min(textarea.scrollHeight, maxTextareaHeight)

      textarea.style.height = `${Math.max(120, desiredHeight)}px`
      textarea.style.overflowY = textarea.scrollHeight > maxTextareaHeight ? 'auto' : 'hidden'
    }

    // Expand in-place from the collapsed control position.
    overlay.style.top = `${Math.round(topPreferred)}px`
    fitTextarea()

    const overlayRect = overlay.getBoundingClientRect()
    const overflowBottom = overlayRect.bottom - (visibleBottom - 16)
    if (overflowBottom > 0) {
      // Keep the overlay anchored; just re-fit the textarea height so the card stays within view.
      // This preserves the "expands from the input" mental model.
      fitTextarea()
    }
  }, [keysBatchExpanded])

  useLayoutEffect(() => {
    if (!keysBatchExpanded) return
    updateKeysBatchOverlayLayout()
  }, [keysBatchExpanded, newKeysText, updateKeysBatchOverlayLayout])

  useEffect(() => {
    if (!keysBatchExpanded) return
    const onResize = () => updateKeysBatchOverlayLayout()
    const onScroll = () => updateKeysBatchOverlayLayout()
    window.addEventListener('resize', onResize)
    window.addEventListener('scroll', onScroll, { passive: true })
    window.visualViewport?.addEventListener('resize', onResize)
    window.visualViewport?.addEventListener('scroll', onScroll, { passive: true })
    return () => {
      window.removeEventListener('resize', onResize)
      window.removeEventListener('scroll', onScroll)
      window.visualViewport?.removeEventListener('resize', onResize)
      window.visualViewport?.removeEventListener('scroll', onScroll)
    }
  }, [keysBatchExpanded, updateKeysBatchOverlayLayout])

  useEffect(() => {
    if (!keysBatchExpanded) return
    const raf = window.requestAnimationFrame(() => updateKeysBatchOverlayLayout())
    return () => window.cancelAnimationFrame(raf)
  }, [keysBatchExpanded, updateKeysBatchOverlayLayout])

  const logsTotalPagesRaw = useMemo(
    () => Math.max(1, Math.ceil(logsTotal / LOGS_PER_PAGE)),
    [logsTotal],
  )

  const logsTotalPages = Math.min(logsTotalPagesRaw, LOGS_MAX_PAGES)

  const safeLogsPage = Math.min(logsPage, logsTotalPages)
  const tokensBlocking = isBlockingLoadState(tokensLoadState)
  const tokensRefreshing = isRefreshingLoadState(tokensLoadState)
  const requestsBlocking = isBlockingLoadState(requestsLoadState)
  const requestsRefreshing = isRefreshingLoadState(requestsLoadState)
  const jobsBlocking = isBlockingLoadState(jobsLoadState)
  const jobsRefreshing = isRefreshingLoadState(jobsLoadState)
  const usersBlocking = isBlockingLoadState(usersLoadState)
  const usersRefreshing = isRefreshingLoadState(usersLoadState)
  const tokenLeaderboardBlocking = isBlockingLoadState(tokenLeaderboardLoadState)
  const tokenLeaderboardRefreshing = isRefreshingLoadState(tokenLeaderboardLoadState)
  const activeModuleBlocking =
    (route.name === 'module' && route.module === 'tokens' && tokensBlocking)
    || (route.name === 'module' && route.module === 'requests' && requestsBlocking)
    || (route.name === 'module' && route.module === 'jobs' && jobsBlocking)
    || ((route.name === 'module' && route.module === 'users') || route.name === 'user') && usersBlocking
    || (route.name === 'token-usage' && tokenLeaderboardBlocking)

  const displayName = profile?.displayName ?? null

  const toggleLogExpansion = useCallback((id: number) => {
    setExpandedLogs((previous) => {
      const next = new Set(previous)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }, [])

  const toggleJobExpansion = useCallback((id: number) => {
    setExpandedJobs((previous) => {
      const next = new Set(previous)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }, [])

  const closeKeysValidationDialog = useCallback(() => {
    keysValidateAbortRef.current?.abort()
    keysValidateAbortRef.current = null
    setKeysValidation(null)
  }, [])

  useEffect(() => () => {
    keysValidateAbortRef.current?.abort()
    keysValidateAbortRef.current = null
  }, [])

  const coerceValidationStatus = (raw: string): KeyValidationStatus => {
    switch ((raw || '').toLowerCase()) {
      case 'pending':
        return 'pending'
      case 'duplicate_in_input':
        return 'duplicate_in_input'
      case 'ok':
        return 'ok'
      case 'ok_exhausted':
        return 'ok_exhausted'
      case 'unauthorized':
        return 'unauthorized'
      case 'forbidden':
        return 'forbidden'
      case 'invalid':
        return 'invalid'
      case 'error':
        return 'error'
      default:
        return 'error'
    }
  }

  const applyValidationResults = useCallback((results: ValidateKeyResult[], runId: number) => {
    setKeysValidation((prev) => {
      if (!prev) return prev
      if (runId !== keysValidateRunIdRef.current) return prev
      const byKey = new Map(results.map((r) => [r.api_key, r]))
      const nextRows = prev.rows.map((row): KeyValidationRow => {
        if (row.status === 'duplicate_in_input') return row
        const res = byKey.get(row.api_key)
        if (!res) return row
        const status = coerceValidationStatus(res.status)
        return {
          ...row,
          status,
          quota_limit: res.quota_limit,
          quota_remaining: res.quota_remaining,
          detail: res.detail,
        }
      })
      return { ...prev, rows: nextRows }
    })
  }, [])

  const markKeysPendingForRetry = useCallback((apiKeys: string[], runId: number) => {
    setKeysValidation((prev) => {
      if (!prev) return prev
      if (runId !== keysValidateRunIdRef.current) return prev
      const set = new Set(apiKeys)
      const rows = prev.rows.map((row): KeyValidationRow => {
        if (!set.has(row.api_key)) return row
        if (row.status === 'duplicate_in_input') return row
        return {
          ...row,
          status: 'pending' as const,
          detail: undefined,
          quota_limit: undefined,
          quota_remaining: undefined,
          attempts: row.attempts + 1,
        }
      })
      return { ...prev, rows }
    })
  }, [])

  const runValidateKeys = useCallback(async (apiKeys: string[], runId: number) => {
    const controller = new AbortController()
    keysValidateAbortRef.current?.abort()
    keysValidateAbortRef.current = controller

    const CHUNK_SIZE = 25
    for (let i = 0; i < apiKeys.length; i += CHUNK_SIZE) {
      const chunk = apiKeys.slice(i, i + CHUNK_SIZE)
      if (chunk.length === 0) continue
      try {
        const resp = await validateApiKeys(chunk, controller.signal)
        applyValidationResults(resp.results, runId)
      } catch (err) {
        const message = err instanceof Error ? err.message : 'Failed to validate keys'
        applyValidationResults(
          chunk.map((api_key) => ({ api_key, status: 'error', detail: message })),
          runId,
        )
      }
    }

    // Only mark "checking" as done if this is still the active run.
    setKeysValidation((prev) => {
      if (!prev) return prev
      if (runId !== keysValidateRunIdRef.current) return prev
      return { ...prev, checking: false }
    })
    // Avoid clobbering a newer validation run's abort controller.
    if (keysValidateAbortRef.current === controller) {
      keysValidateAbortRef.current = null
    }
  }, [applyValidationResults])

  const handleAddKey = async () => {
    const rawLines = newKeysText.split(/\r?\n/)
    const apiKeys = extractTvlyDevApiKeysFromText(newKeysText)
    if (apiKeys.length === 0) return

    const group = newKeysGroup.trim()

    const seen = new Set<string>()
    const rows: KeyValidationRow[] = []
    const uniqueKeys: string[] = []
    let duplicateCount = 0
    for (const api_key of apiKeys) {
      if (seen.has(api_key)) {
        duplicateCount += 1
        rows.push({ api_key, status: 'duplicate_in_input', attempts: 0 })
        continue
      }
      seen.add(api_key)
      uniqueKeys.push(api_key)
      rows.push({ api_key, status: 'pending', attempts: 0 })
    }

    keysValidateRunIdRef.current = (keysValidateRunIdRef.current ?? 0) + 1
    const runId = keysValidateRunIdRef.current
    setKeysValidation({
      group,
      input_lines: rawLines.length,
      valid_lines: apiKeys.length,
      unique_in_input: uniqueKeys.length,
      duplicate_in_input: duplicateCount,
      checking: true,
      importing: false,
      rows,
      imported_api_keys: [],
    })

    // Collapse the in-place overlay once we hand off to the dialog.
    setNewKeysText('')
    setNewKeysGroup('')
    beginKeysBatchClose()

    await runValidateKeys(uniqueKeys, runId)
  }

  const handleRetryFailedValidation = async () => {
    if (!keysValidation) return
    if (keysValidation.checking || keysValidation.importing) return

    const failed = new Set<string>()
    for (const row of keysValidation.rows) {
      if (row.status === 'unauthorized' || row.status === 'forbidden' || row.status === 'invalid' || row.status === 'error') {
        failed.add(row.api_key)
      }
    }
    const failedKeys = Array.from(failed)
    if (failedKeys.length === 0) return

    keysValidateRunIdRef.current = (keysValidateRunIdRef.current ?? 0) + 1
    const runId = keysValidateRunIdRef.current
    setKeysValidation((prev) => prev ? ({
      ...prev,
      checking: true,
      importError: undefined,
      importWarning: undefined,
    }) : prev)
    markKeysPendingForRetry(failedKeys, runId)
    await runValidateKeys(failedKeys, runId)
  }

  const handleRetryOneValidation = async (api_key: string) => {
    if (!keysValidation) return
    if (keysValidation.checking || keysValidation.importing) return
    keysValidateRunIdRef.current = (keysValidateRunIdRef.current ?? 0) + 1
    const runId = keysValidateRunIdRef.current
    setKeysValidation((prev) => prev ? ({
      ...prev,
      checking: true,
      importError: undefined,
      importWarning: undefined,
    }) : prev)
    markKeysPendingForRetry([api_key], runId)
    await runValidateKeys([api_key], runId)
  }

  const handleImportValidatedKeys = async () => {
    if (!keysValidation) return
    if (keysValidation.checking || keysValidation.importing) return
    if (keysValidationValidKeys.length === 0) return

    const importRunId = keysValidateRunIdRef.current
    const group = keysValidation.group.trim()
    const normalizedGroup = group.length > 0 ? group : undefined
    const exhaustedSet = new Set(keysValidationExhaustedKeys)
    setKeysValidation((prev) => {
      if (!prev) return prev
      if (importRunId !== keysValidateRunIdRef.current) return prev
      return {
        ...prev,
        importing: true,
        importError: undefined,
        importWarning: undefined,
        importReport: undefined,
      }
    })
    try {
      const response: AddApiKeysBatchResponse = {
        summary: {
          input_lines: 0,
          valid_lines: 0,
          unique_in_input: 0,
          created: 0,
          undeleted: 0,
          existed: 0,
          duplicate_in_input: 0,
          failed: 0,
        },
        results: [],
      }
      let markExhaustedFailedCount = 0

      for (let i = 0; i < keysValidationValidKeys.length; i += API_KEYS_IMPORT_CHUNK_SIZE) {
        const chunk = keysValidationValidKeys.slice(i, i + API_KEYS_IMPORT_CHUNK_SIZE)
        const exhaustedInChunk = chunk.filter((apiKey) => exhaustedSet.has(apiKey))
        const chunkResponse = await addApiKeysBatch(chunk, normalizedGroup, exhaustedInChunk)
        response.summary.input_lines += chunkResponse.summary.input_lines
        response.summary.valid_lines += chunkResponse.summary.valid_lines
        response.summary.unique_in_input += chunkResponse.summary.unique_in_input
        response.summary.created += chunkResponse.summary.created
        response.summary.undeleted += chunkResponse.summary.undeleted
        response.summary.existed += chunkResponse.summary.existed
        response.summary.duplicate_in_input += chunkResponse.summary.duplicate_in_input
        response.summary.failed += chunkResponse.summary.failed
        for (const result of chunkResponse.results) {
          if (!exhaustedSet.has(result.api_key)) continue
          if (result.status === 'failed') continue
          if (result.marked_exhausted === true) continue
          markExhaustedFailedCount += 1
        }
        response.results.push(...chunkResponse.results)
      }

      const importedByResponse = new Set<string>()
      for (const result of response.results) {
        if (result.status === 'created' || result.status === 'undeleted' || result.status === 'existed') {
          importedByResponse.add(result.api_key)
        }
      }

      const imported = new Set(keysValidation.imported_api_keys)
      for (const apiKey of importedByResponse) imported.add(apiKey)
      const shouldAutoClose = keysValidation.rows.every((row) => imported.has(row.api_key))
      setKeysValidation((prev) => {
        if (!prev) return prev
        if (importRunId !== keysValidateRunIdRef.current) return prev
        const warning = markExhaustedFailedCount > 0
          ? keyStrings.validation.import.exhaustedMarkFailed.replace('{count}', String(markExhaustedFailedCount))
          : undefined
        return {
          ...prev,
          importing: false,
          imported_api_keys: Array.from(imported),
          importReport: response,
          importWarning: warning,
        }
      })
      if (shouldAutoClose && importRunId === keysValidateRunIdRef.current) {
        window.requestAnimationFrame(() => closeKeysValidationDialog())
      }
      await Promise.all([refreshBaseData(), refreshKeysList()])
    } catch (err) {
      console.error(err)
      const message = err instanceof Error ? err.message : errorStrings.addKeysBatch
      setKeysValidation((prev) => {
        if (!prev) return prev
        if (importRunId !== keysValidateRunIdRef.current) return prev
        return { ...prev, importing: false, importWarning: undefined, importError: message }
      })
    }
  }

  const handleAddToken = async (anchorEl?: HTMLElement | null) => {
    const note = newTokenNote.trim()
    void anchorEl
    setManualCopyBubble(null)
    setManualCopyDialog(null)
    setSubmitting(true)
    try {
      const { token } = await createToken(note || undefined)
      setNewTokenNote('')
      const copyResult = await copyToClipboard(token)
      if (!copyResult.ok) {
        setManualCopyDialog({
          title: manualCopyText.createToken.title,
          description: manualCopyText.createToken.description,
          fieldLabel: manualCopyText.fields.token,
          value: token,
        })
      }
      const controller = new AbortController()
      setLoading(true)
      await loadData({ signal: controller.signal, reason: 'refresh', showGlobalLoading: true })
      controller.abort()
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : errorStrings.createToken)
    } finally {
      setSubmitting(false)
    }
  }

  const totalPages = useMemo(() => Math.max(1, Math.ceil(tokensTotal / tokensPerPage)), [tokensTotal])
  const keysTotalPages = useMemo(() => Math.max(1, Math.ceil(keysTotal / keysPerPage)), [keysPerPage, keysTotal])
  const keysHasFilters = selectedKeyGroups.length > 0 || selectedKeyStatuses.length > 0
  const keysBlocking = isBlockingLoadState(keysLoadState)
  const keysRefreshing = isRefreshingLoadState(keysLoadState)

  const goPrevPage = () => {
    setTokensPage((p) => Math.max(1, p - 1))
  }
  const goNextPage = () => {
    setTokensPage((p) => Math.min(totalPages, p + 1))
  }

  const hasLogsPagination = logsTotal > LOGS_PER_PAGE
  const usersTotalPages = useMemo(() => Math.max(1, Math.ceil(usersTotal / USERS_PER_PAGE)), [usersTotal])

  const goPrevLogsPage = () => {
    setLogsPage((p) => Math.max(1, p - 1))
  }

  const goNextLogsPage = () => {
    setLogsPage((p) => Math.min(logsTotalPages, p + 1))
  }

  const goPrevUsersPage = () => {
    navigateUsersSearch(usersQuery, { tagId: usersTagFilterId, page: usersPage - 1 })
  }

  const goNextUsersPage = () => {
    navigateUsersSearch(usersQuery, { tagId: usersTagFilterId, page: usersPage + 1 })
  }

  const applyUserSearch = () => {
    navigateUsersSearch(usersQueryInput, { tagId: usersTagFilterId, page: 1 })
  }

  const resetUserSearch = () => {
    navigateUsersSearch('', { tagId: null, page: 1 })
  }

  const goPrevKeysPage = () => {
    navigateKeysList({
      page: keysPage - 1,
      perPage: keysPerPage,
      groups: selectedKeyGroups,
      statuses: selectedKeyStatuses,
    })
  }

  const goNextKeysPage = () => {
    navigateKeysList({
      page: keysPage + 1,
      perPage: keysPerPage,
      groups: selectedKeyGroups,
      statuses: selectedKeyStatuses,
    })
  }

  const changeKeysPerPage = (nextPerPage: number) => {
    navigateKeysList({
      page: 1,
      perPage: nextPerPage,
      groups: selectedKeyGroups,
      statuses: selectedKeyStatuses,
    })
  }

  const refreshBaseData = async (options?: { includeKeys?: boolean }) => {
    const controller = new AbortController()
    setLoading(true)
    try {
      await loadData({ signal: controller.signal, reason: 'refresh', showGlobalLoading: true })
      if (options?.includeKeys && route.name === 'module' && route.module === 'keys') {
        await refreshKeysList()
      }
    } finally {
      controller.abort()
    }
  }

  const refreshKeysList = async () => {
    const pagedKeys = await fetchApiKeys(
      keysPage,
      keysPerPage,
      {
        groups: selectedKeyGroups,
        statuses: selectedKeyStatuses,
      },
    )
    setKeys(pagedKeys.items)
    setKeysTotal(pagedKeys.total)
    setKeysPage(pagedKeys.page)
    setKeysPerPage(pagedKeys.perPage)
    setKeyGroupFacets(pagedKeys.facets.groups)
    setKeyStatusFacets(pagedKeys.facets.statuses)
    return pagedKeys
  }
  const toggleAllowRegistration = async () => {
    if (registrationSettingsSaving || registrationSettingsLoading || allowRegistration === null) return
    const previous = allowRegistration
    const next = !previous
    setAllowRegistration(next)
    setRegistrationSettingsSaving(true)
    setRegistrationSettingsError(null)
    try {
      const result = await updateAdminRegistrationSettings(next)
      setAllowRegistration(result.allowRegistration)
    } catch (err) {
      console.error(err)
      setAllowRegistration(previous)
      setRegistrationSettingsError(
        err instanceof Error ? err.message : adminStrings.users.registration.saveFailed,
      )
    } finally {
      setRegistrationSettingsSaving(false)
    }
  }
  const refreshUsersList = async () => {
    const pagedUsers = await fetchAdminUsers(usersPage, USERS_PER_PAGE, usersQuery, usersTagFilterId)
    setUsers(pagedUsers.items)
    setUsersTotal(pagedUsers.total)
    return pagedUsers
  }

  const refreshUserDetail = async (userId: string) => {
    const detail = await fetchAdminUserDetail(userId)
    setSelectedUserDetail(detail)
    setUserQuotaSnapshot(buildUserQuotaSnapshot(detail))
    setUserQuotaDraft({
      hourlyAnyLimit: String(detail.quotaBase.hourlyAnyLimit),
      hourlyLimit: String(detail.quotaBase.hourlyLimit),
      dailyLimit: String(detail.quotaBase.dailyLimit),
      monthlyLimit: String(detail.quotaBase.monthlyLimit),
    })
    setSelectedBindableTagId('')
    return detail
  }

  const refreshTagCatalog = async () => {
    const tags = await fetchAdminUserTags()
    setTagCatalog(tags)
    return tags
  }

  const updateQuotaDraftField = (field: QuotaSliderField, value: string) => {
    const normalizedValue = normalizeQuotaDraftInput(value)
    if (normalizedValue == null) return
    setUserQuotaDraft((previous) => {
      if (!previous) return previous
      return { ...previous, [field]: normalizedValue }
    })
    setUserQuotaSavedAt(null)
    setUserQuotaError(null)
  }

  const updateUserTagCatalogField = (field: keyof UserTagFormState, value: string) => {
    setUserTagCatalogDraft((previous) => ({ ...previous, [field]: value }))
    setTagCatalogError(null)
  }

  const cancelUserTagCatalogEdit = () => {
    setActiveUserTagEditorId(null)
    setUserTagCatalogDraft({ ...EMPTY_USER_TAG_FORM })
    setTagCatalogError(null)
    if (route.name === 'user-tag-editor') {
      navigateUserTags()
    }
  }

  const beginCreateUserTag = () => {
    navigateUserTagCreate()
  }

  const beginEditUserTag = (tag: AdminUserTag) => {
    navigateUserTagEdit(tag.id)
  }

  const saveUserQuota = async () => {
    if (route.name !== 'user' || !userQuotaDraft) return
    const payload = {
      hourlyAnyLimit: Number.parseInt(userQuotaDraft.hourlyAnyLimit, 10),
      hourlyLimit: Number.parseInt(userQuotaDraft.hourlyLimit, 10),
      dailyLimit: Number.parseInt(userQuotaDraft.dailyLimit, 10),
      monthlyLimit: Number.parseInt(userQuotaDraft.monthlyLimit, 10),
    }
    if (
      !Number.isFinite(payload.hourlyAnyLimit) || payload.hourlyAnyLimit < 0
      || !Number.isFinite(payload.hourlyLimit) || payload.hourlyLimit < 0
      || !Number.isFinite(payload.dailyLimit) || payload.dailyLimit < 0
      || !Number.isFinite(payload.monthlyLimit) || payload.monthlyLimit < 0
    ) {
      setUserQuotaError(adminStrings.users.quota.invalid)
      return
    }
    setSavingUserQuota(true)
    setUserQuotaError(null)
    try {
      await updateAdminUserQuota(route.id, payload)
      await Promise.all([
        refreshUserDetail(route.id),
        refreshUsersList(),
      ])
      setUserQuotaSavedAt(Date.now())
    } catch (err) {
      console.error(err)
      setUserQuotaError(err instanceof Error ? err.message : adminStrings.users.quota.saveFailed)
    } finally {
      setSavingUserQuota(false)
    }
  }

  const saveUserTagCatalog = async () => {
    const editingTag = userTagCatalogDraft.tagId
      ? tagCatalog.find((tag) => tag.id === userTagCatalogDraft.tagId) ?? null
      : null
    const isSystemEditing = editingTag?.systemKey != null
    const parsedDeltas = {
      hourlyAnyDelta: Number.parseInt(userTagCatalogDraft.hourlyAnyDelta, 10),
      hourlyDelta: Number.parseInt(userTagCatalogDraft.hourlyDelta, 10),
      dailyDelta: Number.parseInt(userTagCatalogDraft.dailyDelta, 10),
      monthlyDelta: Number.parseInt(userTagCatalogDraft.monthlyDelta, 10),
    }
    const effectKind = userTagCatalogDraft.effectKind === 'block_all' ? 'block_all' : 'quota_delta'
    const deltasAreValid = Object.values(parsedDeltas).every((value) => Number.isFinite(value))
    if (!isSystemEditing && (userTagCatalogDraft.name.trim().length === 0 || userTagCatalogDraft.displayName.trim().length === 0)) {
      setTagCatalogError(adminStrings.users.catalog.invalid)
      return
    }
    if (!deltasAreValid) {
      setTagCatalogError(adminStrings.users.catalog.invalid)
      return
    }

    const payload = {
      name: isSystemEditing ? editingTag?.name ?? userTagCatalogDraft.name.trim() : userTagCatalogDraft.name.trim(),
      displayName: isSystemEditing
        ? editingTag?.displayName ?? userTagCatalogDraft.displayName.trim()
        : userTagCatalogDraft.displayName.trim(),
      icon: isSystemEditing ? editingTag?.icon ?? null : (userTagCatalogDraft.icon.trim() || null),
      effectKind,
      hourlyAnyDelta: effectKind === 'block_all' ? 0 : parsedDeltas.hourlyAnyDelta,
      hourlyDelta: effectKind === 'block_all' ? 0 : parsedDeltas.hourlyDelta,
      dailyDelta: effectKind === 'block_all' ? 0 : parsedDeltas.dailyDelta,
      monthlyDelta: effectKind === 'block_all' ? 0 : parsedDeltas.monthlyDelta,
    }

    setSavingUserTagCatalog(true)
    setTagCatalogError(null)
    try {
      if (editingTag) {
        await updateAdminUserTag(editingTag.id, payload)
      } else {
        await createAdminUserTag(payload)
      }
      await Promise.all([
        refreshTagCatalog(),
        refreshUsersList(),
        route.name === 'user' ? refreshUserDetail(route.id) : Promise.resolve(null),
      ])
      cancelUserTagCatalogEdit()
    } catch (err) {
      console.error(err)
      setTagCatalogError(err instanceof Error ? err.message : adminStrings.users.catalog.saveFailed)
    } finally {
      setSavingUserTagCatalog(false)
    }
  }

  const requestUserTagCatalogDelete = (tag: AdminUserTag) => {
    if (tag.systemKey) return
    setTagCatalogError(null)
    setPendingUserTagDelete(tag)
  }

  const closeUserTagDeleteDialog = () => {
    if (deletingUserTagId) return
    setPendingUserTagDelete(null)
  }

  const confirmUserTagCatalogDelete = async () => {
    const tag = pendingUserTagDelete
    if (!tag || tag.systemKey) return
    setDeletingUserTagId(tag.id)
    setTagCatalogError(null)
    try {
      await deleteAdminUserTag(tag.id)
      await Promise.all([
        refreshTagCatalog(),
        refreshUsersList(),
        route.name === 'user' ? refreshUserDetail(route.id) : Promise.resolve(null),
      ])
      if (userTagCatalogDraft.tagId === tag.id) {
        cancelUserTagCatalogEdit()
      }
      setPendingUserTagDelete((current) => (current?.id === tag.id ? null : current))
    } catch (err) {
      console.error(err)
      setTagCatalogError(err instanceof Error ? err.message : adminStrings.users.catalog.deleteFailed)
    } finally {
      setDeletingUserTagId(null)
    }
  }

  const bindSelectedUserTag = async () => {
    if (route.name !== 'user' || !selectedBindableTagId) return
    setSavingUserTagBinding(true)
    setUserTagError(null)
    try {
      await bindAdminUserTag(route.id, selectedBindableTagId)
      await Promise.all([refreshUserDetail(route.id), refreshUsersList(), refreshTagCatalog()])
      setSelectedBindableTagId('')
    } catch (err) {
      console.error(err)
      setUserTagError(err instanceof Error ? err.message : adminStrings.users.userTags.bindFailed)
    } finally {
      setSavingUserTagBinding(false)
    }
  }

  const unbindSelectedUserTag = async (tag: AdminUserTagBinding) => {
    if (route.name !== 'user' || isSystemUserTag(tag) || tag.source !== 'manual') return
    setSavingUserTagBinding(true)
    setUserTagError(null)
    try {
      await unbindAdminUserTag(route.id, tag.tagId)
      await Promise.all([refreshUserDetail(route.id), refreshUsersList(), refreshTagCatalog()])
    } catch (err) {
      console.error(err)
      setUserTagError(err instanceof Error ? err.message : adminStrings.users.userTags.unbindFailed)
    } finally {
      setSavingUserTagBinding(false)
    }
  }

  const handleSelectTokenGroupAll = () => {
    setSelectedTokenGroupName(null)
    setSelectedTokenUngrouped(false)
    setTokensPage(1)
  }

  const handleSelectTokenGroupUngrouped = () => {
    setSelectedTokenGroupName(null)
    setSelectedTokenUngrouped(true)
    setTokensPage(1)
  }

  const handleSelectTokenGroupNamed = (group: string) => {
    setSelectedTokenGroupName(group)
    setSelectedTokenUngrouped(false)
    setTokensPage(1)
  }

  const toggleTokenGroupsExpanded = () => {
    setTokenGroupsExpanded((previous) => !previous)
  }

  const handleToggleKeyGroupFilter = (group: string) => {
    navigateKeysList({
      page: 1,
      perPage: keysPerPage,
      groups: toggleSelection(selectedKeyGroups, group),
      statuses: selectedKeyStatuses,
    })
  }

  const handleToggleKeyStatusFilter = (status: string) => {
    navigateKeysList({
      page: 1,
      perPage: keysPerPage,
      groups: selectedKeyGroups,
      statuses: toggleSelection(selectedKeyStatuses, status),
    })
  }

  const handleClearKeyGroupFilters = () => {
    navigateKeysList({
      page: 1,
      perPage: keysPerPage,
      groups: [],
      statuses: selectedKeyStatuses,
    })
  }

  const handleClearKeyStatusFilters = () => {
    navigateKeysList({
      page: 1,
      perPage: keysPerPage,
      groups: selectedKeyGroups,
      statuses: [],
    })
  }

  const openBatchDialog = () => {
    setBatchGroup('')
    setBatchCount(10)
    setBatchShareText(null)
    setBatchDialogOpen(true)
  }
  const submitBatchCreate = async () => {
    const group = batchGroup.trim()
    if (!group) return
    setBatchCreating(true)
    try {
      const res = await createTokensBatch(group, Math.max(1, Math.min(1000, batchCount)), newTokenNote.trim() || undefined)
      const links = res.tokens.map((t) => `${window.location.origin}/#${encodeURIComponent(t)}`).join('\n')
      setBatchShareText(links)
      // refresh list to first page
      setTokensPage(1)
      const controller = new AbortController()
      setLoading(true)
      await loadData({ signal: controller.signal, reason: 'refresh', showGlobalLoading: true })
      controller.abort()
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : errorStrings.createToken)
    } finally {
      setBatchCreating(false)
    }
  }
  const closeBatchDialog = () => {
    setBatchDialogOpen(false)
  }

  const closeKeysBatchReportDialog = () => {
    setKeysBatchReport(null)
  }

  const handleCopyToken = async (id: string, stateKey: string, anchorEl?: HTMLElement | null) => {
    setManualCopyBubble(null)
    commitSecretWarm(`token:${id}`)
    updateCopyState(stateKey, 'loading')
    try {
      const hasCachedToken = tokenSecretCacheRef.current.has(id)
      const token = await resolveTokenSecret(id)
      const copyResult = await copyToClipboard(
        token,
        hasCachedToken ? { preferExecCommand: true } : undefined,
      )
      if (!copyResult.ok) {
        updateCopyState(stateKey, null)
        if (anchorEl) {
          openManualCopyBubble({
            anchorEl,
            title: manualCopyText.title,
            description: manualCopyText.description,
            fieldLabel: manualCopyText.fields.token,
            value: token,
          })
        }
        return
      }
      setManualCopyBubble(null)
      updateCopyState(stateKey, 'copied')
      window.setTimeout(() => updateCopyState(stateKey, null), 2000)
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : errorStrings.copyToken)
      updateCopyState(stateKey, null)
    }
  }

  const handleShareToken = async (id: string, stateKey: string, anchorEl?: HTMLElement | null) => {
    setManualCopyBubble(null)
    commitSecretWarm(`token:${id}`)
    updateCopyState(stateKey, 'loading')
    try {
      const hasCachedToken = tokenSecretCacheRef.current.has(id)
      const token = await resolveTokenSecret(id)
      const shareUrl = `${window.location.origin}/#${encodeURIComponent(token)}`
      const copyResult = await copyToClipboard(
        shareUrl,
        hasCachedToken ? { preferExecCommand: true } : undefined,
      )
      if (!copyResult.ok) {
        updateCopyState(stateKey, null)
        if (anchorEl) {
          openManualCopyBubble({
            anchorEl,
            title: manualCopyText.title,
            description: manualCopyText.description,
            fieldLabel: manualCopyText.fields.shareLink,
            value: shareUrl,
          })
        }
        return
      }
      setManualCopyBubble(null)
      updateCopyState(stateKey, 'copied')
      window.setTimeout(() => updateCopyState(stateKey, null), 2000)
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : errorStrings.copyToken)
      updateCopyState(stateKey, null)
    }
  }

  const toggleToken = async (id: string, enabled: boolean) => {
    setTogglingId(id)
    try {
      await setTokenEnabled(id, !enabled)
      const controller = new AbortController()
      setLoading(true)
      await loadData({ signal: controller.signal, reason: 'refresh', showGlobalLoading: true })
      controller.abort()
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : errorStrings.toggleToken)
    } finally {
      setTogglingId(null)
    }
  }

  const openTokenDeleteConfirm = (id: string) => {
    if (!id) return
    setPendingTokenDeleteId(id)
  }

  const confirmTokenDelete = async () => {
    if (!pendingTokenDeleteId) return
    const id = pendingTokenDeleteId
    setDeletingId(id)
    try {
      await deleteToken(id)
      setPendingTokenDeleteId(null)
      const controller = new AbortController()
      setLoading(true)
      await loadData({ signal: controller.signal, reason: 'refresh', showGlobalLoading: true })
      controller.abort()
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : errorStrings.deleteToken)
    } finally {
      setDeletingId(null)
    }
  }

  const cancelTokenDelete = () => {
    setPendingTokenDeleteId(null)
  }

  const openTokenNoteEdit = (id: string, current: string | null) => {
    setEditingTokenId(id)
    setEditingTokenNote(current ?? '')
  }

  const saveTokenNote = async () => {
    if (!editingTokenId) return
    setSavingTokenNote(true)
    try {
      await updateTokenNote(editingTokenId, editingTokenNote)
      setEditingTokenId(null)
      setEditingTokenNote('')
      const controller = new AbortController()
      setLoading(true)
      await loadData({ signal: controller.signal, reason: 'refresh', showGlobalLoading: true })
      controller.abort()
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : errorStrings.updateTokenNote)
    } finally {
      setSavingTokenNote(false)
    }
  }

  const cancelTokenNote = () => {
    setEditingTokenId(null)
    setEditingTokenNote('')
  }

  const openDeleteConfirm = (id: string) => {
    if (!id) return
    setPendingDeleteId(id)
  }

  const confirmDelete = async () => {
    if (!pendingDeleteId) return
    const id = pendingDeleteId
    setDeletingId(id)
    try {
      await deleteApiKey(id)
      setPendingDeleteId(null)
      await refreshBaseData({ includeKeys: true })
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : errorStrings.deleteKey)
    } finally {
      setDeletingId(null)
    }
  }

  const cancelDelete = () => {
    setPendingDeleteId(null)
  }

  const handleToggleDisable = async (id: string, toDisabled: boolean) => {
    if (!id) return
    setTogglingId(id)
    try {
      await setKeyStatus(id, toDisabled ? 'disabled' : 'active')
      await refreshBaseData({ includeKeys: true })
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : errorStrings.toggleKey)
    } finally {
      setTogglingId(null)
    }
  }

  const handleClearQuarantine = async (id: string) => {
    if (!id) return
    setClearingQuarantineId(id)
    try {
      await clearApiKeyQuarantine(id)
      await refreshBaseData({ includeKeys: true })
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : errorStrings.clearQuarantine)
    } finally {
      setClearingQuarantineId(null)
    }
  }

  // Disable confirm flow
  const openDisableConfirm = (id: string) => {
    if (!id) return
    setPendingDisableId(id)
  }

  const confirmDisable = async () => {
    if (!pendingDisableId) return
    const id = pendingDisableId
    await handleToggleDisable(id, true)
    setPendingDisableId(null)
  }

  const cancelDisable = () => {
    setPendingDisableId(null)
  }

  const tokenLeaderboardView = useMemo(() => {
    if (!tokenLeaderboard || tokenLeaderboard.length === 0) return []
    return sortLeaderboard(tokenLeaderboard, tokenLeaderboardPeriod, tokenLeaderboardFocus).slice(0, 50)
  }, [tokenLeaderboard, tokenLeaderboardPeriod, tokenLeaderboardFocus])
  const navItems: AdminNavItem[] = [
    { module: 'dashboard', label: adminStrings.nav.dashboard, icon: 'mdi:view-dashboard-outline' },
    { module: 'tokens', label: adminStrings.nav.tokens, icon: 'mdi:key-chain-variant' },
    { module: 'keys', label: adminStrings.nav.keys, icon: 'mdi:key-outline' },
    { module: 'requests', label: adminStrings.nav.requests, icon: 'mdi:file-document-outline' },
    { module: 'jobs', label: adminStrings.nav.jobs, icon: 'mdi:calendar-clock-outline' },
    { module: 'users', label: adminStrings.nav.users, icon: 'mdi:account-group-outline' },
    { module: 'alerts', label: adminStrings.nav.alerts, icon: 'mdi:bell-ring-outline' },
    { module: 'proxy-settings', label: adminStrings.nav.proxySettings, icon: 'mdi:tune-variant' },
  ]
  const activeModule: AdminModuleId =
    route.name === 'module'
      ? route.module
      : route.name === 'key'
        ? 'keys'
        : route.name === 'user' || route.name === 'user-tags' || route.name === 'user-tag-editor'
          ? 'users'
          : 'tokens'
  const usersStrings = adminStrings.users
  const registrationStatusText = registrationSettingsLoading && !registrationSettingsLoaded
    ? usersStrings.registration.description
    : registrationSettingsSaving
      ? usersStrings.registration.saving
      : allowRegistration === null
        ? usersStrings.registration.unavailable
      : allowRegistration
        ? usersStrings.registration.enabled
        : usersStrings.registration.disabled

  const registrationInlineStatus = registrationSettingsError ?? (registrationSettingsSaving ? registrationStatusText : null)
  const sortedTagCatalog = useMemo(
    () => [...tagCatalog].sort((left, right) => right.userCount - left.userCount || left.displayName.localeCompare(right.displayName)),
    [tagCatalog],
  )
  const editingCatalogTag = activeUserTagEditorId && activeUserTagEditorId !== NEW_USER_TAG_CARD_ID
    ? tagCatalog.find((tag) => tag.id === activeUserTagEditorId) ?? null
    : null
  const editingSystemTag = editingCatalogTag?.systemKey != null
  const tagCatalogEffectIsBlockAll = userTagCatalogDraft.effectKind === 'block_all'
  const bindableCustomTags = route.name === 'user' && selectedUserDetail
    ? tagCatalog.filter((tag) => !tag.systemKey && !selectedUserDetail.tags.some((boundTag) => boundTag.tagId === tag.id))
    : []
  const visibleTagCards: Array<AdminUserTag | null> = activeUserTagEditorId === NEW_USER_TAG_CARD_ID
    ? [null, ...sortedTagCatalog]
    : sortedTagCatalog

  const renderUserTagSummaryPanel = (): JSX.Element => (
    <section className="surface panel">
      <div className="panel-header" style={{ gap: 12, flexWrap: 'wrap' }}>
        <div>
          <h2>{usersStrings.catalog.summaryTitle}</h2>
          <p className="panel-description">{usersStrings.catalog.summaryDescription}</p>
        </div>
        <button type="button" className="btn btn-outline" onClick={navigateUserTags}>
          {usersStrings.userTags.manageCatalog}
        </button>
      </div>

      {tagCatalogError && (
        <div className="alert alert-error" role="alert" style={{ marginBottom: 12 }}>
          {tagCatalogError}
        </div>
      )}

      {tagCatalogLoading ? (
        <div className="empty-state alert">{usersStrings.catalog.loading}</div>
      ) : sortedTagCatalog.length === 0 ? (
        <div className="empty-state alert">{usersStrings.catalog.summaryEmpty}</div>
      ) : (
        <div className="user-tag-summary-grid">
          {sortedTagCatalog.map((tag) => {
            const isSystem = tag.systemKey != null
            const isBlockAll = tag.effectKind === 'block_all'
            const cardClasses = ['user-tag-summary-card', isBlockAll ? 'user-tag-summary-card-block' : '']
              .filter(Boolean)
              .join(' ')

            return (
              <article className={cardClasses} key={tag.id}>
                <div className="user-tag-summary-card-head">
                  <UserTagBadge
                    tag={{
                      displayName: tag.displayName,
                      icon: tag.icon,
                      systemKey: tag.systemKey,
                      effectKind: tag.effectKind,
                    }}
                    usersStrings={usersStrings}
                  />
                  <StatusBadge tone={isSystem ? 'info' : isBlockAll ? 'error' : 'neutral'}>
                    {isSystem ? usersStrings.catalog.scopeSystem : usersStrings.catalog.scopeCustom}
                  </StatusBadge>
                </div>
                <div className="user-tag-summary-count">
                  <strong>{formatNumber(tag.userCount)}</strong>
                  <span className="panel-description">{usersStrings.catalog.summaryAccounts}</span>
                </div>
              </article>
            )
          })}
        </div>
      )}
    </section>
  )

  const renderUserTagEffectToggle = (): JSX.Element => (
    <div className="user-tag-effect-toggle" role="group" aria-label={usersStrings.catalog.fields.effect}>
      {([
        ['quota_delta', usersStrings.catalog.effectKinds.quotaDelta],
        ['block_all', usersStrings.catalog.effectKinds.blockAll],
      ] as const).map(([effectKind, label]) => {
        const isActive = userTagCatalogDraft.effectKind === effectKind
        return (
          <Button
            key={effectKind}
            type="button"
            variant={isActive ? 'secondary' : 'outline'}
            size="xs"
            className={`user-tag-effect-chip${isActive ? ' is-active' : ''}`}
            onClick={() => updateUserTagCatalogField('effectKind', effectKind)}
            disabled={savingUserTagCatalog}
          >
            {label}
          </Button>
        )
      })}
    </div>
  )

  const renderUserTagCatalogCard = (tag: AdminUserTag | null): JSX.Element => {
    const isNewCard = tag == null
    const isEditing = isNewCard
      ? activeUserTagEditorId === NEW_USER_TAG_CARD_ID
      : activeUserTagEditorId === tag.id
    const isSystem = tag?.systemKey != null
    const viewTag = isEditing
      ? {
          displayName: userTagCatalogDraft.displayName || userTagCatalogDraft.name || usersStrings.catalog.formCreateTitle,
          icon: userTagCatalogDraft.icon,
          systemKey: tag?.systemKey ?? null,
          effectKind: userTagCatalogDraft.effectKind,
        }
      : {
          displayName: tag?.displayName ?? usersStrings.catalog.formCreateTitle,
          icon: tag?.icon ?? null,
          systemKey: tag?.systemKey ?? null,
          effectKind: tag?.effectKind ?? 'quota_delta',
        }
    const isBlockAll = viewTag.effectKind === 'block_all'
    const iconSrc = getUserTagIconSrc(viewTag.icon)
    const cardClasses = [
      'user-tag-catalog-card',
      isEditing ? 'user-tag-catalog-card-active' : '',
      isNewCard ? 'user-tag-catalog-card-draft' : '',
    ]
      .filter(Boolean)
      .join(' ')

    return (
      <Card className={cardClasses} key={tag?.id ?? NEW_USER_TAG_CARD_ID}>
        <div className="user-tag-catalog-card-head">
          <div className="user-tag-catalog-name">
            {isEditing ? (
              <div className="user-tag-inline-fields">
                <Input
                  type="text"
                  className="user-tag-inline-input user-tag-inline-input-display"
                  value={userTagCatalogDraft.displayName}
                  onChange={(event) => updateUserTagCatalogField('displayName', event.target.value)}
                  disabled={editingSystemTag || savingUserTagCatalog}
                  placeholder={usersStrings.catalog.fields.displayName}
                />
                <div className="user-tag-inline-fields-row">
                  <Input
                    type="text"
                    className="user-tag-inline-input"
                    value={userTagCatalogDraft.name}
                    onChange={(event) => updateUserTagCatalogField('name', event.target.value)}
                    disabled={editingSystemTag || savingUserTagCatalog}
                    placeholder={usersStrings.catalog.fields.name}
                  />
                  <Input
                    type="text"
                    className="user-tag-inline-input"
                    value={userTagCatalogDraft.icon}
                    onChange={(event) => updateUserTagCatalogField('icon', event.target.value)}
                    disabled={editingSystemTag || savingUserTagCatalog}
                    placeholder={usersStrings.catalog.iconPlaceholder}
                  />
                </div>
              </div>
            ) : (
              <>
                <div className="user-tag-pill-list">
                  <UserTagBadge tag={viewTag} usersStrings={usersStrings} />
                </div>
                <div className="panel-description user-tag-catalog-subtitle">
                  <code>{tag?.name}</code>
                  {iconSrc ? ` · ${viewTag.icon}` : ''}
                </div>
              </>
            )}
          </div>
          <div className="user-tag-catalog-actions">
            {isEditing ? (
              <>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="user-tag-catalog-icon-button"
                  title={usersStrings.catalog.actions.save}
                  aria-label={usersStrings.catalog.actions.save}
                  onClick={() => void saveUserTagCatalog()}
                  disabled={savingUserTagCatalog}
                >
                  <Icon icon="mdi:check" width={16} height={16} />
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="user-tag-catalog-icon-button"
                  title={usersStrings.catalog.actions.cancelEdit}
                  aria-label={usersStrings.catalog.actions.cancelEdit}
                  onClick={cancelUserTagCatalogEdit}
                  disabled={savingUserTagCatalog}
                >
                  <Icon icon="mdi:close" width={16} height={16} />
                </Button>
                {!isNewCard && !isSystem && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    className="user-tag-catalog-icon-button"
                    title={usersStrings.catalog.actions.delete}
                    aria-label={usersStrings.catalog.actions.delete}
                    onClick={() => requestUserTagCatalogDelete(tag)}
                    disabled={savingUserTagCatalog || deletingUserTagId === tag.id}
                  >
                    <Icon icon="mdi:trash-can-outline" width={16} height={16} />
                  </Button>
                )}
              </>
            ) : (
              <>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="user-tag-catalog-icon-button"
                  title={usersStrings.catalog.actions.edit}
                  aria-label={usersStrings.catalog.actions.edit}
                  onClick={() => tag && beginEditUserTag(tag)}
                >
                  <Icon icon="mdi:pencil-outline" width={16} height={16} />
                </Button>
                {!isSystem && tag && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    className="user-tag-catalog-icon-button"
                    title={usersStrings.catalog.actions.delete}
                    aria-label={usersStrings.catalog.actions.delete}
                    onClick={() => requestUserTagCatalogDelete(tag)}
                    disabled={deletingUserTagId === tag.id}
                  >
                    <Icon icon="mdi:trash-can-outline" width={16} height={16} />
                  </Button>
                )}
              </>
            )}
          </div>
        </div>

        <div className="user-tag-catalog-card-meta">
          <Badge variant={isSystem ? 'info' : 'neutral'} className="user-tag-meta-badge">
            {isSystem ? usersStrings.catalog.scopeSystem : usersStrings.catalog.scopeCustom}
          </Badge>
          {isEditing ? (
            renderUserTagEffectToggle()
          ) : (
            <Badge variant={isBlockAll ? 'destructive' : 'success'} className="user-tag-meta-badge">
              {isBlockAll
                ? usersStrings.catalog.effectKinds.blockAll
                : usersStrings.catalog.effectKinds.quotaDelta}
            </Badge>
          )}
          <Button
            type="button"
            variant="secondary"
            size="xs"
            className="user-tag-catalog-users user-tag-catalog-users-button"
            onClick={() => tag && navigateUsersSearch(tag.displayName, { tagId: tag.id })}
            disabled={isNewCard}
          >
            <span className="user-tag-catalog-users-label">{usersStrings.catalog.columns.users}</span>
            <strong>{formatNumber(tag?.userCount ?? 0)}</strong>
          </Button>
        </div>

        <div className="user-tag-catalog-body">
          {isBlockAll ? (
            <div className="alert alert-warning user-tag-catalog-block-note" role="note">
              {usersStrings.catalog.blockDescription}
            </div>
          ) : (
            <dl className="user-tag-catalog-delta-grid">
              {([
                ['hourlyAnyDelta', tag?.hourlyAnyDelta ?? 0, usersStrings.quota.hourlyAny],
                ['hourlyDelta', tag?.hourlyDelta ?? 0, usersStrings.quota.hourly],
                ['dailyDelta', tag?.dailyDelta ?? 0, usersStrings.quota.daily],
                ['monthlyDelta', tag?.monthlyDelta ?? 0, usersStrings.quota.monthly],
              ] as const).map(([field, value, label]) => (
                <div className="user-tag-catalog-delta-item" key={field}>
                  <dt>{label}</dt>
                  <dd>
                    {isEditing ? (
                      <Input
                        type="number"
                        className="user-tag-delta-input"
                        value={userTagCatalogDraft[field]}
                        onChange={(event) => updateUserTagCatalogField(field, event.target.value)}
                        disabled={savingUserTagCatalog || tagCatalogEffectIsBlockAll}
                      />
                    ) : (
                      formatSignedQuotaDelta(value)
                    )}
                  </dd>
                </div>
              ))}
            </dl>
          )}
        </div>
      </Card>
    )
  }

  const renderUserTagCatalogIndexPage = (): JSX.Element => (
    <AdminShell
      activeModule={activeModule}
      navItems={navItems}
      skipToContentLabel={adminStrings.accessibility.skipToContent}
      onSelectModule={navigateModule}
    >
      <section className="surface panel">
        <div className="panel-header" style={{ gap: 12, flexWrap: 'wrap' }}>
          <div>
            <h2>{usersStrings.catalog.title}</h2>
            <p className="panel-description">{usersStrings.catalog.description}</p>
          </div>
          <div className="user-tag-page-actions">
            <button
              type="button"
              className="btn btn-outline"
              onClick={() =>
                navigateToPath(
                  buildAdminUsersPath(
                    getAdminUsersQueryFromLocation(),
                    getAdminUsersTagFilterFromLocation(),
                    getAdminUsersPageFromLocation(),
                  ),
                )
              }
            >
              {usersStrings.catalog.backToUsers}
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={beginCreateUserTag}
              disabled={activeUserTagEditorId === NEW_USER_TAG_CARD_ID}
            >
              {usersStrings.catalog.actions.create}
            </button>
          </div>
        </div>
      </section>

      {tagCatalogError && (
        <div className="surface error-banner" role="alert">
          {tagCatalogError}
        </div>
      )}

      <section className="surface panel">
        {tagCatalogLoading ? (
          <div className="empty-state alert">{usersStrings.catalog.loading}</div>
        ) : visibleTagCards.length === 0 ? (
          <div className="empty-state alert">{usersStrings.catalog.empty}</div>
        ) : (
          <div className="user-tag-catalog-grid">
            {visibleTagCards.map((tag) => renderUserTagCatalogCard(tag))}
          </div>
        )}
      </section>

      <Dialog open={pendingUserTagDelete != null} onOpenChange={(open) => { if (!open) closeUserTagDeleteDialog() }}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{usersStrings.catalog.deleteDialogTitle}</DialogTitle>
            <DialogDescription>
              {pendingUserTagDelete
                ? usersStrings.catalog.deleteConfirm.replace('{name}', pendingUserTagDelete.displayName)
                : usersStrings.catalog.deleteConfirm.replace('{name}', '')}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="gap-2 sm:justify-end">
            <Button
              type="button"
              variant="outline"
              onClick={closeUserTagDeleteDialog}
              disabled={deletingUserTagId != null}
            >
              {usersStrings.catalog.deleteDialogCancel}
            </Button>
            <Button
              type="button"
              variant="destructive"
              onClick={() => void confirmUserTagCatalogDelete()}
              disabled={deletingUserTagId != null}
            >
              {usersStrings.catalog.deleteDialogConfirm}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </AdminShell>
  )

  if (route.name === 'key') {
    return (
      <AdminShell
        activeModule={activeModule}
        navItems={navItems}
        skipToContentLabel={adminStrings.accessibility.skipToContent}
        onSelectModule={navigateModule}
      >
        <KeyDetails
          key={route.id}
          id={route.id}
          onBack={() =>
            navigateToPath(
              buildAdminKeysPath({
                page: getAdminKeysPageFromLocation(),
                perPage: getAdminKeysPerPageFromLocation(),
                groups: getAdminKeysValuesFromLocation('group'),
                statuses: getAdminKeysValuesFromLocation('status'),
              }),
            )
          }
        />
      </AdminShell>
    )
  }
  if (route.name === 'token') {
    return (
      <AdminShell
        activeModule={activeModule}
        navItems={navItems}
        skipToContentLabel={adminStrings.accessibility.skipToContent}
        onSelectModule={navigateModule}
      >
        <TokenDetail
          key={route.id}
          id={route.id}
          onBack={() => navigateModule('tokens')}
          onOpenUser={navigateUser}
          onSecretRotated={handleTokenSecretRotated}
        />
      </AdminShell>
    )
  }
  if (route.name === 'user-tags' || route.name === 'user-tag-editor') {
    return renderUserTagCatalogIndexPage()
  }
  if (route.name === 'user') {
    const detail = selectedUserDetail
    const tokenItems = detail?.tokens ?? []
    const boundTags = detail?.tags ?? []
    const hasBlockAllTag = boundTags.some((tag) => tag.effectKind === 'block_all')

    return (
      <AdminShell
        activeModule={activeModule}
        navItems={navItems}
        skipToContentLabel={adminStrings.accessibility.skipToContent}
        onSelectModule={navigateModule}
      >
        <section className="surface panel">
          <div className="panel-header">
            <div>
              <h2>{usersStrings.detail.title}</h2>
              <p className="panel-description">
                {usersStrings.detail.subtitle.replace('{id}', route.id)}
              </p>
            </div>
            <div className="admin-inline-actions">
              <AdminReturnToConsoleLink
                label={headerStrings.returnToConsole}
                href={userConsoleHref}
                className="admin-return-link--detail"
              />
              <Button
                type="button"
                variant="outline"
                onClick={() =>
                  navigateToPath(
                    buildAdminUsersPath(
                      getAdminUsersQueryFromLocation(),
                      getAdminUsersTagFilterFromLocation(),
                      getAdminUsersPageFromLocation(),
                    ),
                  )
                }
              >
                {usersStrings.detail.back}
              </Button>
            </div>
          </div>
        </section>

        {userTagError && (
          <div className="surface error-banner" role="alert">
            {userTagError}
          </div>
        )}

        {userQuotaError && (
          <div className="surface error-banner" role="alert">
            {userQuotaError}
          </div>
        )}

        {userDetailLoading ? (
          <section className="surface panel">
            <div className="empty-state alert">{usersStrings.empty.loading}</div>
          </section>
        ) : !detail ? (
          <section className="surface panel">
            <div className="empty-state alert">{usersStrings.empty.notFound}</div>
          </section>
        ) : (
          <>
            <section className="surface panel">
              <div className="panel-header">
                <div>
                  <h2>{usersStrings.detail.identityTitle}</h2>
                  <p className="panel-description">{usersStrings.detail.identityDescription}</p>
                </div>
              </div>
              <div className="token-info-grid">
                <div className="token-info-card">
                  <span className="token-info-label">{usersStrings.detail.userId}</span>
                  <span className="token-info-value">
                    <code>{detail.userId}</code>
                  </span>
                </div>
                <div className="token-info-card">
                  <span className="token-info-label">{usersStrings.table.displayName}</span>
                  <span className="token-info-value">{detail.displayName || '—'}</span>
                </div>
                <div className="token-info-card">
                  <span className="token-info-label">{usersStrings.table.username}</span>
                  <span className="token-info-value">{detail.username || '—'}</span>
                </div>
                <div className="token-info-card">
                  <span className="token-info-label">{usersStrings.table.status}</span>
                  <span className="token-info-value">
                    <StatusBadge tone={detail.active ? 'success' : 'neutral'}>
                      {detail.active ? usersStrings.status.active : usersStrings.status.inactive}
                    </StatusBadge>
                  </span>
                </div>
                <div className="token-info-card">
                  <span className="token-info-label">{usersStrings.table.lastLogin}</span>
                  <span className="token-info-value">{formatTimestamp(detail.lastLoginAt)}</span>
                </div>
                <div className="token-info-card">
                  <span className="token-info-label">{usersStrings.table.tokenCount}</span>
                  <span className="token-info-value">{formatNumber(detail.tokenCount)}</span>
                </div>
              </div>
            </section>

            <section className="surface panel">
              <div className="panel-header" style={{ gap: 12, flexWrap: 'wrap' }}>
                <div>
                  <h2>{usersStrings.userTags.title}</h2>
                  <p className="panel-description">{usersStrings.userTags.description}</p>
                </div>
                <button type="button" className="btn btn-outline" onClick={navigateUserTags}>
                  {usersStrings.userTags.manageCatalog}
                </button>
              </div>
              <div className="user-tag-binding-toolbar">
                <UserTagBadgeList
                  tags={boundTags}
                  usersStrings={usersStrings}
                  emptyLabel={usersStrings.userTags.empty}
                  limit={Math.max(USER_TAG_DISPLAY_LIMIT, boundTags.length)}
                />
                <div className="user-tag-bind-controls">
                  <select
                    className="select select-bordered"
                    value={selectedBindableTagId}
                    onChange={(event) => setSelectedBindableTagId(event.target.value)}
                    disabled={savingUserTagBinding || bindableCustomTags.length === 0}
                  >
                    <option value="">{usersStrings.userTags.bindPlaceholder}</option>
                    {bindableCustomTags.map((tag) => (
                      <option key={tag.id} value={tag.id}>
                        {tag.displayName}
                      </option>
                    ))}
                  </select>
                  <button
                    type="button"
                    className="btn btn-primary"
                    onClick={() => void bindSelectedUserTag()}
                    disabled={savingUserTagBinding || !selectedBindableTagId}
                  >
                    {savingUserTagBinding ? usersStrings.userTags.binding : usersStrings.userTags.bindAction}
                  </button>
                </div>
              </div>

              {boundTags.length === 0 ? (
                <div className="empty-state alert" style={{ marginTop: 12 }}>{usersStrings.userTags.empty}</div>
              ) : (
                <div className="user-tag-binding-list">
                  {boundTags.map((tag) => {
                    const isSystem = isSystemUserTag(tag)
                    return (
                      <article className="user-tag-binding-card" key={`${tag.tagId}:${tag.source}`}>
                        <div className="user-tag-binding-card-head">
                          <div className="user-tag-pill-list">
                            <UserTagBadge tag={tag} usersStrings={usersStrings} />
                            <StatusBadge tone={isSystem ? 'info' : 'neutral'}>
                              {tag.source === 'system_linuxdo'
                                ? usersStrings.userTags.sourceSystem
                                : usersStrings.userTags.sourceManual}
                            </StatusBadge>
                            <StatusBadge tone={tag.effectKind === 'block_all' ? 'error' : 'success'}>
                              {tag.effectKind === 'block_all'
                                ? usersStrings.catalog.effectKinds.blockAll
                                : usersStrings.catalog.effectKinds.quotaDelta}
                            </StatusBadge>
                          </div>
                          <button
                            type="button"
                            className="btn btn-ghost btn-sm"
                            onClick={() => void unbindSelectedUserTag(tag)}
                            disabled={savingUserTagBinding || isSystem || tag.source !== 'manual'}
                          >
                            {isSystem || tag.source !== 'manual'
                              ? usersStrings.userTags.readOnly
                              : usersStrings.userTags.unbindAction}
                          </button>
                        </div>
                        <div className="token-compact-pair">
                          <div className="token-compact-field">
                            <span className="token-compact-label">{usersStrings.quota.hourlyAny}</span>
                            <span className="token-compact-value">{formatSignedQuotaDelta(tag.hourlyAnyDelta)}</span>
                          </div>
                          <div className="token-compact-field">
                            <span className="token-compact-label">{usersStrings.quota.hourly}</span>
                            <span className="token-compact-value">{formatSignedQuotaDelta(tag.hourlyDelta)}</span>
                          </div>
                          <div className="token-compact-field">
                            <span className="token-compact-label">{usersStrings.quota.daily}</span>
                            <span className="token-compact-value">{formatSignedQuotaDelta(tag.dailyDelta)}</span>
                          </div>
                          <div className="token-compact-field">
                            <span className="token-compact-label">{usersStrings.quota.monthly}</span>
                            <span className="token-compact-value">{formatSignedQuotaDelta(tag.monthlyDelta)}</span>
                          </div>
                        </div>
                      </article>
                    )
                  })}
                </div>
              )}
            </section>

            <section className="surface panel">
              <div className="panel-header" style={{ gap: 12, flexWrap: 'wrap' }}>
                <div>
                  <h2>{usersStrings.quota.title}</h2>
                  <p className="panel-description">{usersStrings.quota.description}</p>
                </div>
                <StatusBadge tone={detail.quotaBase.inheritsDefaults ? 'info' : 'neutral'}>
                  {detail.quotaBase.inheritsDefaults
                    ? usersStrings.quota.inheritsDefaults
                    : usersStrings.quota.customized}
                </StatusBadge>
              </div>
              <div className="quota-grid">
                {([
                  {
                    field: 'hourlyAnyLimit',
                    label: usersStrings.quota.hourlyAny,
                    used: detail.hourlyAnyUsed,
                    currentLimit: detail.quotaBase.hourlyAnyLimit,
                  },
                  {
                    field: 'hourlyLimit',
                    label: usersStrings.quota.hourly,
                    used: detail.quotaHourlyUsed,
                    currentLimit: detail.quotaBase.hourlyLimit,
                  },
                  {
                    field: 'dailyLimit',
                    label: usersStrings.quota.daily,
                    used: detail.quotaDailyUsed,
                    currentLimit: detail.quotaBase.dailyLimit,
                  },
                  {
                    field: 'monthlyLimit',
                    label: usersStrings.quota.monthly,
                    used: detail.quotaMonthlyUsed,
                    currentLimit: detail.quotaBase.monthlyLimit,
                  },
                ] as const).map((item) => {
                  const sliderSeed = userQuotaSnapshot?.[item.field] ?? createQuotaSliderSeed(item.field, item.used, item.currentLimit)
                  const draftValue = userQuotaDraft?.[item.field] ?? String(sliderSeed.initialLimit)
                  const parsedDraft = parseQuotaDraftValue(draftValue, sliderSeed.initialLimit)
                  const sliderPosition = getQuotaSliderStagePosition(sliderSeed.stages, parsedDraft)
                  return (
                    <QuotaRangeField
                      key={item.field}
                      label={item.label}
                      sliderName={`${item.field}-slider`}
                      sliderMin={0}
                      sliderMax={Math.max(0, sliderSeed.stages.length - 1)}
                      sliderValue={sliderPosition}
                      sliderAriaLabel={item.label}
                      sliderStyle={{ background: buildQuotaSliderTrack(sliderSeed.stages, sliderSeed.used, parsedDraft) }}
                      onSliderChange={(nextValue) => {
                        const nextIndex = clampQuotaSliderStageIndex(sliderSeed.stages, nextValue)
                        updateQuotaDraftField(item.field, String(getQuotaSliderStageValue(sliderSeed.stages, nextIndex)))
                      }}
                      helperText={
                        <>
                          {formatNumber(sliderSeed.used)} / {formatNumber(parsedDraft)}
                        </>
                      }
                      inputName={item.field}
                      inputValue={formatQuotaDraftInput(draftValue)}
                      inputAriaLabel={`${item.label} input`}
                      onInputChange={(nextValue) => updateQuotaDraftField(item.field, nextValue)}
                    />
                  )
                })}
              </div>
              <div
                style={{
                  marginTop: 16,
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'space-between',
                  gap: 12,
                  flexWrap: 'wrap',
                }}
              >
                <span className="panel-description">
                  {userQuotaSavedAt
                    ? usersStrings.quota.savedAt.replace(
                        '{time}',
                        timeOnlyFormatter.format(new Date(userQuotaSavedAt)),
                      )
                    : usersStrings.quota.hint}
                </span>
                <Button type="button" onClick={() => void saveUserQuota()} disabled={savingUserQuota}>
                  {savingUserQuota ? usersStrings.quota.saving : usersStrings.quota.save}
                </Button>
              </div>
            </section>

            <section className="surface panel">
              <div className="panel-header">
                <div>
                  <h2>{usersStrings.effectiveQuota.title}</h2>
                  <p className="panel-description">{usersStrings.effectiveQuota.description}</p>
                </div>
              </div>
              {hasBlockAllTag && (
                <div className="alert alert-warning" role="status" style={{ marginBottom: 12 }}>
                  {usersStrings.effectiveQuota.blockAllNotice}
                </div>
              )}
              <div className="token-info-grid">
                {([
                  ['hourlyAny', usersStrings.quota.hourlyAny, detail.effectiveQuota.hourlyAnyLimit],
                  ['hourly', usersStrings.quota.hourly, detail.effectiveQuota.hourlyLimit],
                  ['daily', usersStrings.quota.daily, detail.effectiveQuota.dailyLimit],
                  ['monthly', usersStrings.quota.monthly, detail.effectiveQuota.monthlyLimit],
                ] as const).map(([key, label, value]) => (
                  <div className="token-info-card" key={key}>
                    <span className="token-info-label">{label}</span>
                    <span className="token-info-value">{formatQuotaLimitValue(value)}</span>
                  </div>
                ))}
              </div>
              <div className="table-wrapper jobs-table-wrapper" style={{ marginTop: 12 }}>
                <table className="jobs-table admin-users-table user-tag-breakdown-table">
                  <thead>
                    <tr>
                      <th>{usersStrings.effectiveQuota.columns.item}</th>
                      <th>{usersStrings.effectiveQuota.columns.source}</th>
                      <th>{usersStrings.effectiveQuota.columns.effect}</th>
                      <th>{usersStrings.quota.hourlyAny}</th>
                      <th>{usersStrings.quota.hourly}</th>
                      <th>{usersStrings.quota.daily}</th>
                      <th>{usersStrings.quota.monthly}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {detail.quotaBreakdown.map((entry, index) => {
                      const isAbsoluteRow = entry.kind === 'base' || entry.kind === 'effective'
                      const breakdownLabel =
                        entry.kind === 'base'
                          ? usersStrings.effectiveQuota.baseLabel
                          : entry.kind === 'effective'
                            ? usersStrings.effectiveQuota.effectiveLabel
                            : entry.label
                      const formatBreakdownValue = (value: number) => (
                        isAbsoluteRow ? formatQuotaLimitValue(value) : formatSignedQuotaDelta(value)
                      )
                      return (
                        <tr key={`${entry.kind}:${entry.tagId ?? 'row'}:${index}`}>
                          <td>
                            <div className="token-compact-pair">
                              <div className="token-compact-field">
                                <span className="token-compact-value">{breakdownLabel}</span>
                              </div>
                              {entry.tagName && (
                                <div className="token-compact-field">
                                  <code className="token-compact-value">{entry.tagName}</code>
                                </div>
                              )}
                            </div>
                          </td>
                          <td>{entry.source ? (entry.source === 'system_linuxdo' ? usersStrings.userTags.sourceSystem : usersStrings.userTags.sourceManual) : '—'}</td>
                          <td>
                            <StatusBadge tone={entry.effectKind === 'block_all' ? 'error' : 'neutral'}>
                              {entry.effectKind === 'block_all'
                                ? usersStrings.catalog.effectKinds.blockAll
                                : entry.effectKind === 'base'
                                  ? usersStrings.effectiveQuota.baseLabel
                                  : entry.kind === 'effective' || entry.effectKind === 'effective'
                                    ? usersStrings.effectiveQuota.effectiveLabel
                                    : usersStrings.catalog.effectKinds.quotaDelta}
                            </StatusBadge>
                          </td>
                          <td>{formatBreakdownValue(entry.hourlyAnyDelta)}</td>
                          <td>{formatBreakdownValue(entry.hourlyDelta)}</td>
                          <td>{formatBreakdownValue(entry.dailyDelta)}</td>
                          <td>{formatBreakdownValue(entry.monthlyDelta)}</td>
                        </tr>
                      )
                    })}
                  </tbody>
                </table>
              </div>
            </section>

            <section className="surface panel">
              <div className="panel-header">
                <div>
                  <h2>{usersStrings.detail.tokensTitle}</h2>
                  <p className="panel-description">{usersStrings.detail.tokensDescription}</p>
                </div>
              </div>
              <div className="table-wrapper jobs-table-wrapper">
                {tokenItems.length === 0 ? (
                  <div className="empty-state alert">{usersStrings.empty.noTokens}</div>
                ) : (
                  <Table className="jobs-table admin-users-table admin-user-tokens-table">
                    <thead>
                      <tr>
                        <th>{`${usersStrings.tokens.table.id} · ${usersStrings.tokens.table.note}`}</th>
                        <th>{`${usersStrings.tokens.table.status} · ${usersStrings.tokens.table.lastUsed}`}</th>
                        <th>{`${usersStrings.tokens.table.hourlyAny} · ${usersStrings.tokens.table.hourly}`}</th>
                        <th>{`${usersStrings.tokens.table.daily} · ${usersStrings.tokens.table.monthly}`}</th>
                        <th>{`${usersStrings.tokens.table.successDaily} · ${usersStrings.tokens.table.successMonthly}`}</th>
                        <th>{usersStrings.tokens.table.actions}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {tokenItems.map((token) => {
                        const hourlyAnyText = formatQuotaUsagePair(token.hourlyAnyUsed, token.hourlyAnyLimit)
                        const hourlyText = formatQuotaUsagePair(token.quotaHourlyUsed, token.quotaHourlyLimit)
                        const dailyText = formatQuotaUsagePair(token.quotaDailyUsed, token.quotaDailyLimit)
                        const monthlyText = formatQuotaUsagePair(token.quotaMonthlyUsed, token.quotaMonthlyLimit)
                        const successDailyText = `${formatNumber(token.dailySuccess)} / ${formatNumber(token.dailyFailure)}`
                        const successMonthlyText = formatNumber(token.monthlySuccess)
                        return (
                          <tr key={token.tokenId}>
                            <td>
                              <div className="token-compact-pair">
                                <div className="token-compact-field">
                                  <code className="token-compact-value">{token.tokenId}</code>
                                </div>
                                <div className="token-compact-field">
                                  <span className="token-compact-value">{token.note || '—'}</span>
                                </div>
                              </div>
                            </td>
                            <td>
                              <div className="token-compact-pair">
                                <div className="token-compact-field">
                                  <StatusBadge tone={token.enabled ? 'success' : 'neutral'}>
                                    {token.enabled ? usersStrings.status.enabled : usersStrings.status.disabled}
                                  </StatusBadge>
                                </div>
                                <div className="token-compact-field">
                                  <span className="token-compact-value">{formatTimestamp(token.lastUsedAt)}</span>
                                </div>
                              </div>
                            </td>
                            <td>
                              <div className="token-compact-pair">
                                <div className="token-compact-field">
                                  <span className="token-compact-label">{usersStrings.tokens.table.hourlyAny}</span>
                                  <span className="token-compact-value">{hourlyAnyText}</span>
                                </div>
                                <div className="token-compact-field">
                                  <span className="token-compact-label">{usersStrings.tokens.table.hourly}</span>
                                  <span className="token-compact-value">{hourlyText}</span>
                                </div>
                              </div>
                            </td>
                            <td>
                              <div className="token-compact-pair">
                                <div className="token-compact-field">
                                  <span className="token-compact-label">{usersStrings.tokens.table.daily}</span>
                                  <span className="token-compact-value">{dailyText}</span>
                                </div>
                                <div className="token-compact-field">
                                  <span className="token-compact-label">{usersStrings.tokens.table.monthly}</span>
                                  <span className="token-compact-value">{monthlyText}</span>
                                </div>
                              </div>
                            </td>
                            <td>
                              <div className="token-compact-pair">
                                <div className="token-compact-field">
                                  <span className="token-compact-label">{usersStrings.tokens.table.successDaily}</span>
                                  <span className="token-compact-value">{successDailyText}</span>
                                </div>
                                <div className="token-compact-field">
                                  <span className="token-compact-label">{usersStrings.tokens.table.successMonthly}</span>
                                  <span className="token-compact-value">{successMonthlyText}</span>
                                </div>
                              </div>
                            </td>
                            <td>
<Button
  type="button"
  variant="ghost"
  size="icon"
  className="h-8 w-8 rounded-full p-0 shadow-none"
  title={usersStrings.tokens.actions.view}
  aria-label={usersStrings.tokens.actions.view}
  onClick={() => navigateToken(token.tokenId)}
>
  <Icon icon="mdi:eye-outline" width={16} height={16} />
</Button>
                            </td>
                          </tr>
                        )
                      })}
                    </tbody>
                  </Table>
                )}
              </div>
            </section>
          </>
        )}
      </AdminShell>
    )
  }

  if (route.name === 'token-usage') {
    const primaryMetric: MetricKey = tokenLeaderboardFocus

    const renderPeriodCell = (
      item: TokenUsageLeaderboardItem,
      period: 'day' | 'month' | 'all',
      primary: MetricKey,
    ) => {
      const { values } = pickPrimaryForPeriod(item, period, primary)
      const secondaryKeys: MetricKey[] = ['usage', 'errors', 'other'].filter((k) => k !== primary) as MetricKey[]
      const label = (key: MetricKey) =>
        key === 'usage'
          ? tokenLeaderboardStrings.focus.usage
          : key === 'errors'
            ? tokenLeaderboardStrings.table.errors
            : tokenLeaderboardStrings.table.other

      return (
        <td>
          <div className="token-leaderboard-usage">{formatNumber(values[primary])}</div>
          <div className="token-leaderboard-sub">
            {secondaryKeys.map((key) => (
              <span key={key}>
                {label(key)}: {formatNumber(values[key])}
              </span>
            ))}
          </div>
        </td>
      )
    }

    return (
      <AdminShell
        activeModule={activeModule}
        navItems={navItems}
        skipToContentLabel={adminStrings.accessibility.skipToContent}
        onSelectModule={navigateModule}
      >
        <TokenUsageHeader
          title={tokenLeaderboardStrings.title}
          subtitle={tokenLeaderboardStrings.description}
          visualPreset="accent"
          backLabel={tokenLeaderboardStrings.back}
          refreshLabel={headerStrings.refreshNow}
          refreshingLabel={headerStrings.refreshing}
          userConsoleLabel={headerStrings.returnToConsole}
          userConsoleHref={userConsoleHref}
          isRefreshing={tokenLeaderboardRefreshing}
          period={tokenLeaderboardPeriod}
          focus={tokenLeaderboardFocus}
          periodOptions={[
            { value: 'day', label: tokenLeaderboardStrings.period.day },
            { value: 'month', label: tokenLeaderboardStrings.period.month },
            { value: 'all', label: tokenLeaderboardStrings.period.all },
          ]}
          focusOptions={[
            { value: 'usage', label: tokenLeaderboardStrings.focus.usage },
            { value: 'errors', label: tokenLeaderboardStrings.focus.errors },
            { value: 'other', label: tokenLeaderboardStrings.focus.other },
          ]}
          controlsDisabled={tokenLeaderboardBlocking}
          onBack={() => navigateModule('tokens')}
          onRefresh={() => setTokenLeaderboardNonce((x) => x + 1)}
          onPeriodChange={setTokenLeaderboardPeriod}
          onFocusChange={setTokenLeaderboardFocus}
        />
        <section className="surface panel token-leaderboard-panel">
          <AdminLoadingRegion
            className="table-wrapper jobs-table-wrapper token-leaderboard-wrapper admin-responsive-up"
            loadState={tokenLeaderboardLoadState}
            loadingLabel={tokenLeaderboardRefreshing ? loadingStateStrings.refreshing : tokenLeaderboardStrings.empty.loading}
            minHeight={340}
          >
          {tokenLeaderboardView.length === 0 ? (
            <div className="empty-state alert">
              {tokenLeaderboardStrings.empty.none}
            </div>
          ) : (
            <Table className="jobs-table token-leaderboard-table">
              <thead>
                <tr>
                  <th>{tokenLeaderboardStrings.table.token}</th>
                  <th>{tokenLeaderboardStrings.table.group}</th>
                  <th>{tokenLeaderboardStrings.table.hourly}</th>
                  <th>{tokenLeaderboardStrings.table.hourlyAny}</th>
                  <th>{tokenLeaderboardStrings.table.daily}</th>
                    <th>{tokenLeaderboardStrings.table.today}</th>
                    <th>{tokenLeaderboardStrings.table.month}</th>
                    <th>{tokenLeaderboardStrings.table.all}</th>
                    <th>{tokenLeaderboardStrings.table.lastUsed}</th>
                  </tr>
                </thead>
                <tbody>
                  {tokenLeaderboardView.map((item) => (
                    <tr key={item.id}>
                      <td>
                        <div className="token-id-cell">
                          <button type="button" className="link-button token-id-link" onClick={() => navigateToken(item.id)}>
                            <code className="token-id-code">{item.id}</code>
                          </button>
                          <span
                            className="token-status-slot"
                            aria-hidden={item.enabled ? true : undefined}
                            title={item.enabled ? undefined : tokenStrings.statusBadges.disabled}
                          >
                            {!item.enabled && (
                              <Icon
                                className="token-status-icon"
                                icon="mdi:pause-circle-outline"
                                width={14}
                                height={14}
                                aria-label={tokenStrings.statusBadges.disabled}
                              />
                            )}
                          </span>
                        </div>
                      </td>
                      <td>{item.group && item.group.trim().length > 0 ? item.group : '—'}</td>
                  <td>
                    <div className="token-leaderboard-usage">{formatNumber(item.quota_hourly_used)}</div>
                    <div className="token-leaderboard-sub">/ {formatNumber(item.quota_hourly_limit)}</div>
                  </td>
                  <td>
                    <div className="token-leaderboard-usage">{formatNumber(item.hourly_any_used)}</div>
                    <div className="token-leaderboard-sub">/ {formatNumber(item.hourly_any_limit)}</div>
                  </td>
                  <td>
                    <div className="token-leaderboard-usage">{formatNumber(item.quota_daily_used)}</div>
                    <div className="token-leaderboard-sub">/ {formatNumber(item.quota_daily_limit)}</div>
                  </td>
                      {renderPeriodCell(item, 'day', primaryMetric)}
                      {renderPeriodCell(item, 'month', primaryMetric)}
                      {renderPeriodCell(item, 'all', primaryMetric)}
                      <td>
                        <div className="token-last-used">
                          <span className="token-last-date">{formatDateOnly(item.last_used_at)}</span>
                          <span className="token-last-time">{formatClockTime(item.last_used_at)}</span>
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </Table>
            )}
          </AdminLoadingRegion>
          <AdminLoadingRegion
            className="admin-mobile-list admin-responsive-down"
            loadState={tokenLeaderboardLoadState}
            loadingLabel={tokenLeaderboardRefreshing ? loadingStateStrings.refreshing : tokenLeaderboardStrings.empty.loading}
            minHeight={260}
          >
            {tokenLeaderboardView.length === 0 ? (
              <div className="empty-state alert">
                {tokenLeaderboardStrings.empty.none}
              </div>
            ) : (
              tokenLeaderboardView.map((item) => (
                <article key={item.id} className="admin-mobile-card">
                  <div className="admin-mobile-kv">
                    <span>{tokenLeaderboardStrings.table.token}</span>
                    <strong>
                      <code>{item.id}</code>
                    </strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{tokenLeaderboardStrings.table.group}</span>
                    <strong>{item.group && item.group.trim().length > 0 ? item.group : '—'}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{tokenLeaderboardStrings.table.hourly}</span>
                    <strong>{`${formatNumber(item.quota_hourly_used)} / ${formatNumber(item.quota_hourly_limit)}`}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{tokenLeaderboardStrings.table.hourlyAny}</span>
                    <strong>{`${formatNumber(item.hourly_any_used)} / ${formatNumber(item.hourly_any_limit)}`}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{tokenLeaderboardStrings.table.daily}</span>
                    <strong>{`${formatNumber(item.quota_daily_used)} / ${formatNumber(item.quota_daily_limit)}`}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{tokenLeaderboardStrings.table.today}</span>
                    <strong>{formatNumber(leaderboardPrimaryValue(item, 'day', primaryMetric))}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{tokenLeaderboardStrings.table.month}</span>
                    <strong>{formatNumber(leaderboardPrimaryValue(item, 'month', primaryMetric))}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{tokenLeaderboardStrings.table.all}</span>
                    <strong>{formatNumber(leaderboardPrimaryValue(item, 'all', primaryMetric))}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{tokenLeaderboardStrings.table.lastUsed}</span>
                    <strong>{`${formatDateOnly(item.last_used_at)} ${formatClockTime(item.last_used_at)}`}</strong>
                  </div>
                  <div className="admin-mobile-actions">
<Button type="button" variant="outline" size="sm" onClick={() => navigateToken(item.id)}>
  {keyStrings.actions.details}
</Button>
                  </div>
                </article>
              ))
            )}
          </AdminLoadingRegion>
          {tokenLeaderboardError && tokenLeaderboardView.length === 0 && (
            <div className="surface error-banner" style={{ marginTop: 12 }}>
              {tokenLeaderboardError}
            </div>
          )}
        </section>
      </AdminShell>
    )
  }
  const showDashboard = activeModule === 'dashboard'
  const showTokens = activeModule === 'tokens'
  const showKeys = activeModule === 'keys'
  const showRequests = activeModule === 'requests'
  const showJobs = activeModule === 'jobs'
  const showUsers = activeModule === 'users'
  const showAlerts = activeModule === 'alerts'
  const showProxySettings = activeModule === 'proxy-settings'
  const trendBuckets = (() => {
    const windowSize = 8
    const sorted = [...dashboardLogs]
      .filter((log) => typeof log.created_at === 'number' && Number.isFinite(log.created_at))
      .sort((a, b) => a.created_at - b.created_at)
      .slice(-64)
    if (sorted.length === 0) {
      return { request: new Array(windowSize).fill(0), error: new Array(windowSize).fill(0) }
    }
    const minTime = sorted[0].created_at
    const maxTime = sorted[sorted.length - 1].created_at
    const span = Math.max(1, maxTime - minTime + 1)
    const request = new Array<number>(windowSize).fill(0)
    const error = new Array<number>(windowSize).fill(0)
    for (const item of sorted) {
      const ratio = (item.created_at - minTime) / span
      const index = Math.min(windowSize - 1, Math.max(0, Math.floor(ratio * windowSize)))
      request[index] += 1
      if (item.result_status === 'error' || item.result_status === 'quota_exhausted') {
        error[index] += 1
      }
    }
    return { request, error }
  })()
  return (
    <>
      {showKeys &&
        keysBatchVisible &&
        typeof document !== 'undefined' &&
        createPortal(
          <div
            ref={keysBatchOverlayRef}
            className={`card bg-base-100 shadow-xl border border-base-300 keys-batch-overlay${keysBatchClosing ? ' is-closing' : ''}`}
            onMouseEnter={() => {
              clearKeysBatchAutoCollapseTimer()
              if (keysBatchClosing) {
                clearKeysBatchCloseTimer()
                setKeysBatchClosing(false)
                keysBatchOpenReasonRef.current = 'hover'
                setKeysBatchExpanded(true)
              }
            }}
            onMouseLeave={() => scheduleKeysBatchAutoCollapse('hover')}
            onPointerDown={(event) => {
              clearKeysBatchAutoCollapseTimer()
              if (keysBatchClosing) {
                clearKeysBatchCloseTimer()
                setKeysBatchClosing(false)
                setKeysBatchExpanded(true)
              }
              keysBatchOpenReasonRef.current = 'focus'

              // The overlay visually "replaces" the collapsed input. If the user clicks the card padding
              // (common when the overlay opens on hover), ensure the textarea receives focus so blur-based
              // auto-collapse works as expected.
	              if (document.activeElement === keysBatchTextareaRef.current) return
	              if (event.target instanceof Element) {
	                if (event.target.closest('textarea')) return
	                if (event.target.closest('button')) return
	                if (event.target.closest('input')) return
	                if (event.target.closest('select')) return
	                if (event.target.closest('a')) return
	              }

              window.requestAnimationFrame(() => keysBatchTextareaRef.current?.focus())
            }}
            style={{
              position: 'fixed',
              top: 0,
              left: 16,
              zIndex: 1000,
              width: 'min(720px, calc(100vw - 32px))',
            }}
          >
            <div className="card-body" style={{ padding: 16 }}>
              <Textarea
                ref={keysBatchTextareaRef}
                className="min-h-[112px] w-full text-sm"
                rows={4}
                placeholder={keyStrings.batch.placeholder}
                aria-label={keyStrings.batch.placeholder}
                value={newKeysText}
                onChange={(e) => setNewKeysText(e.target.value)}
                onFocus={() => clearKeysBatchAutoCollapseTimer()}
                onBlur={(event) => {
                  if (event.currentTarget.value.trim().length !== 0) return
                  const overlay = keysBatchOverlayRef.current
                  const next = event.relatedTarget
                  if (overlay && next instanceof Node && overlay.contains(next)) return
                  scheduleKeysBatchAutoCollapse('blur')
                }}
                style={{
                  fontFamily:
                    'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
                  lineHeight: 1.4,
                  borderRadius: 14,
                  whiteSpace: 'pre',
                  overflowY: 'hidden',
                }}
              />
	              <div ref={keysBatchFooterRef} className="mt-3 flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
	                <div className="text-xs opacity-70 flex-1 min-w-0">
	                  <div>{keyStrings.batch.hint}</div>
	                  <div>{keyStrings.batch.count.replace('{count}', formatNumber(keysBatchParsed.length))}</div>
	                </div>
	                <div className="flex gap-2 items-center justify-end flex-wrap sm:flex-nowrap sm:flex-shrink-0">
<Input
  type="text"
  name="new-keys-group"
  placeholder={keyStrings.batch.groupPlaceholder}
  aria-label={keyStrings.batch.groupPlaceholder}
  value={newKeysGroup}
  onChange={(e) => setNewKeysGroup(e.target.value)}
  list="api-key-group-datalist"
  style={{ flex: '1 1 220px', minWidth: 160, maxWidth: '100%' }}
/>
<Button
  type="button"
  onClick={() => void handleAddKey()}
  disabled={submitting || keysBatchParsed.length === 0}
>
  {submitting ? keyStrings.adding : keyStrings.addButton}
</Button>
	                </div>
	              </div>
	            </div>
	          </div>,
          document.body,
        )}
      <AdminShell
        activeModule={activeModule}
        navItems={navItems}
        skipToContentLabel={adminStrings.accessibility.skipToContent}
        onSelectModule={navigateModule}
      >
      <AdminPanelHeader
        title={headerStrings.title}
        subtitle={headerStrings.subtitle}
        displayName={displayName}
        isAdmin={isAdmin}
        updatedPrefix={headerStrings.updatedPrefix}
        updatedTime={lastUpdated ? timeOnlyFormatter.format(lastUpdated) : null}
        isRefreshing={loading}
        refreshDisabled={activeModuleBlocking}
        refreshLabel={headerStrings.refreshNow}
        refreshingLabel={headerStrings.refreshing}
        userConsoleLabel={headerStrings.returnToConsole}
        userConsoleHref={userConsoleHref}
        onRefresh={handleManualRefresh}
      />

      {showDashboard && (
        <DashboardOverview
          strings={adminStrings.dashboard}
          overviewReady={dashboardOverviewLoaded}
          metrics={metrics}
          trend={trendBuckets}
          tokenCoverage={dashboardTokenCoverage}
          tokens={dashboardTokens}
          keys={dashboardKeys}
          logs={dashboardLogs}
          jobs={dashboardJobs}
          onOpenModule={navigateModule}
          onOpenToken={navigateToken}
          onOpenKey={navigateKey}
        />
      )}

      {showTokens && (
      <section className="surface panel">
        <div className="panel-header" style={{ flexWrap: 'wrap', gap: 12, alignItems: 'flex-start' }}>
          <div style={{ flex: '1 1 320px', minWidth: 240 }}>
            <div style={{ display: 'inline-flex', alignItems: 'center', gap: 8 }}>
              <h2 style={{ margin: 0 }}>{tokenStrings.title}</h2>
              <div className="tooltip" data-tip={tokenStrings.actions.viewLeaderboard}>
<Button
  type="button"
  variant="ghost"
  size="icon"
  className="h-8 w-8 rounded-full p-0 shadow-none"
  aria-label={tokenStrings.actions.viewLeaderboard}
  onClick={navigateTokenLeaderboard}
>
  <Icon icon="mdi:chart-timeline-variant" width={20} height={20} />
</Button>
              </div>
            </div>
            <p className="panel-description">{tokenStrings.description}</p>
          </div>
          {isAdmin && (
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 8,
                flexWrap: 'wrap',
                justifyContent: 'flex-end',
                flex: '0 1 auto',
                minWidth: 0,
                maxWidth: '100%',
                marginLeft: 'auto',
              }}
            >
<Input
  type="text"
  name="new-token-note"
  placeholder={tokenStrings.notePlaceholder}
  value={newTokenNote}
  onChange={(e) => setNewTokenNote(e.target.value)}
  style={{ minWidth: 0, flex: '1 1 240px' }}
  aria-label={tokenStrings.notePlaceholder}
/>
<Button
  type="button"
  onClick={(event) => void handleAddToken(event.currentTarget)}
  disabled={submitting}
>
  {submitting ? tokenStrings.creating : tokenStrings.newToken}
</Button>
<Button
  type="button"
  variant="outline"
  onClick={openBatchDialog}
  disabled={submitting}
>
  {tokenStrings.batchCreate}
</Button>
            </div>
          )}
        </div>
        {hasTokenGroups && (
          <div className="token-groups-container">
            <div className="token-groups-label">
              <span>{tokenStrings.groups.label}</span>
            </div>
            <div className="token-groups-row">
              <div
                ref={tokenGroupsListRef}
                className={`token-groups-list${tokenGroupsExpanded ? ' token-groups-list-expanded' : ''}`}
              >
                <button
                  type="button"
                  className={`token-group-chip${
                    !selectedTokenUngrouped && selectedTokenGroupName == null ? ' token-group-chip-active' : ''
                  }`}
                  onClick={handleSelectTokenGroupAll}
                  disabled={tokensBlocking}
                >
                  <span className="token-group-name">{tokenStrings.groups.all}</span>
                </button>
                {ungroupedTokenGroup && (
                  <button
                    type="button"
                    className={`token-group-chip${selectedTokenUngrouped ? ' token-group-chip-active' : ''}`}
                    onClick={handleSelectTokenGroupUngrouped}
                    disabled={tokensBlocking}
                  >
                    <span className="token-group-name">{tokenStrings.groups.ungrouped}</span>
                    {tokenGroupsExpanded && (
                      <span className="token-group-count">
                        {ungroupedTokenGroup.tokenCount}
                      </span>
                    )}
                  </button>
                )}
                {namedTokenGroups.map((group) => (
                  <button
                    key={group.name}
                    type="button"
                    className={`token-group-chip${
                      !selectedTokenUngrouped && selectedTokenGroupName === group.name ? ' token-group-chip-active' : ''
                    }`}
                    onClick={() => handleSelectTokenGroupNamed(group.name)}
                    disabled={tokensBlocking}
                  >
                    <span className="token-group-name">{group.name}</span>
                    {tokenGroupsExpanded && (
                      <span className="token-group-count">
                        {group.tokenCount}
                      </span>
                    )}
                  </button>
                ))}
              </div>
              {(tokenGroupsCollapsedOverflowing || tokenGroupsExpanded) && (
                <button
                  type="button"
                  className={`token-group-chip token-group-toggle${tokenGroupsExpanded ? ' token-group-toggle-active' : ''}`}
                  onClick={toggleTokenGroupsExpanded}
                  aria-label={tokenGroupsExpanded ? tokenStrings.groups.moreHide : tokenStrings.groups.moreShow}
                  disabled={tokensBlocking}
                >
                  <Icon icon={tokenGroupsExpanded ? 'mdi:chevron-up' : 'mdi:chevron-down'} width={18} height={18} />
                </button>
              )}
            </div>
          </div>
        )}
        <AdminTableShell
          className="jobs-table-wrapper admin-responsive-up"
          tableClassName="jobs-table tokens-table"
          loadState={tokensLoadState}
          loadingLabel={tokensRefreshing ? loadingStateStrings.refreshing : tokenStrings.empty.loading}
          minHeight={320}
        >
          {tokenList.length === 0 ? (
            <tbody>
              <tr>
                <td colSpan={isAdmin ? 7 : 6}>
                  <div className="empty-state alert">{tokenStrings.empty.none}</div>
                </td>
              </tr>
            </tbody>
          ) : (
            <>
              <thead>
                <tr>
                  <th>{tokenStrings.table.id}</th>
                  <th>{tokenStrings.table.owner}</th>
                  <th>{tokenStrings.table.note}</th>
                  <th>{tokenStrings.table.usage}</th>
                  <th>{tokenStrings.table.quota}</th>
                  <th>{tokenStrings.table.lastUsed}</th>
                  {isAdmin && <th>{tokenStrings.table.actions}</th>}
                </tr>
              </thead>
              <tbody>
                {tokenList.map((t) => {
                  const stateKey = copyStateKey('tokens', t.id)
                  const state = copyState.get(stateKey)
                  const shareStateKey = copyStateKey('tokens', `${t.id}:share`)
                  const shareState = copyState.get(shareStateKey)
                  const quotaStateKey = t.quota_state ?? 'normal'
                  const quotaLabel = quotaLabels[quotaStateKey] ?? quotaLabels.normal
                  const quotaTitle = `${t.quota_hourly_used}/${t.quota_hourly_limit} · ${t.quota_daily_used}/${t.quota_daily_limit} · ${t.quota_monthly_used}/${t.quota_monthly_limit}`
                  return (
                    <tr key={t.id}>
                      <td>
                        <div className="token-id-cell">
                          <button
                            type="button"
                            title={tokenStrings.table.id}
                            className="link-button token-id-link"
                            onClick={() => navigateToken(t.id)}
                          >
                            <code className="token-id-code">{t.id}</code>
                          </button>
                          <span
                            className="token-status-slot"
                            aria-hidden={t.enabled ? true : undefined}
                            title={t.enabled ? undefined : tokenStrings.statusBadges.disabled}
                          >
                            {!t.enabled && (
                              <Icon
                                className="token-status-icon"
                                icon="mdi:pause-circle-outline"
                                width={14}
                                height={14}
                                aria-label={tokenStrings.statusBadges.disabled}
                              />
                            )}
                          </span>
                        </div>
                      </td>
                      <td>
                        <TokenOwnerValue owner={t.owner} emptyLabel={tokenStrings.owner.unbound} onOpenUser={navigateUser} />
                      </td>
                      <td>{t.note || '—'}</td>
                      <td>{formatNumber(t.total_requests)}</td>
                      <td>
                        <StatusBadge
                          tone={quotaTone(quotaStateKey)}
                          className={`token-quota-pill token-quota-pill-${quotaStateKey}`}
                        >
                          {quotaLabel}
                        </StatusBadge>
                      </td>
                      <td>{formatTimestamp(t.last_used_at)}</td>
                      {isAdmin && (
                        <td className="jobs-message-cell">
                          <div className="table-actions">
<Button
  type="button"
  variant={state === 'copied' ? 'success' : 'ghost'}
  size="icon"
  className="token-action-button shadow-none"
  title={tokenStrings.actions.copy}
  aria-label={tokenStrings.actions.copy}
  onPointerEnter={() => scheduleSecretWarm(`token:${t.id}`, () => warmTokenSecret(t.id))}
  onPointerLeave={() => cancelSecretWarm(`token:${t.id}`)}
  onBlur={() => cancelSecretWarm(`token:${t.id}`)}
  onPointerDown={() => warmTokenSecret(t.id)}
  onKeyDown={(event) => { if (!isCopyIntentKey(event.key)) return; warmTokenSecret(t.id) }}
  onClick={(event) => void handleCopyToken(t.id, stateKey, event.currentTarget)}
  disabled={state === 'loading'}
>
  <Icon icon={state === 'copied' ? 'mdi:check' : 'mdi:content-copy'} width={16} height={16} />
</Button>
<Button
  type="button"
  variant={shareState === 'copied' ? 'success' : 'ghost'}
  size="icon"
  className="token-action-button shadow-none"
  title={tokenStrings.actions.share}
  aria-label={tokenStrings.actions.share}
  onPointerEnter={() => scheduleSecretWarm(`token:${t.id}`, () => warmTokenSecret(t.id))}
  onPointerLeave={() => cancelSecretWarm(`token:${t.id}`)}
  onBlur={() => cancelSecretWarm(`token:${t.id}`)}
  onPointerDown={() => warmTokenSecret(t.id)}
  onKeyDown={(event) => { if (!isCopyIntentKey(event.key)) return; warmTokenSecret(t.id) }}
  onClick={(event) => void handleShareToken(t.id, shareStateKey, event.currentTarget)}
  disabled={shareState === 'loading'}
>
  <Icon icon={shareState === 'copied' ? 'mdi:check' : 'mdi:share-variant'} width={16} height={16} />
</Button>
<Button
  type="button"
  variant="ghost"
  size="icon"
  className="token-action-button shadow-none"
  title={keyStrings.actions.details}
  aria-label={keyStrings.actions.details}
  onClick={() => navigateToken(t.id)}
>
  <Icon icon="mdi:eye-outline" width={16} height={16} />
</Button>
<Button
  type="button"
  variant="ghost"
  size="icon"
  className="token-action-button shadow-none"
  title={t.enabled ? tokenStrings.actions.disable : tokenStrings.actions.enable}
  aria-label={t.enabled ? tokenStrings.actions.disable : tokenStrings.actions.enable}
  onClick={() => void toggleToken(t.id, t.enabled)}
  disabled={togglingId === t.id}
>
  <Icon icon={t.enabled ? 'mdi:pause-circle-outline' : 'mdi:play-circle-outline'} width={16} height={16} />
</Button>
<Button
  type="button"
  variant="ghost"
  size="icon"
  className="token-action-button shadow-none"
  title={tokenStrings.actions.edit}
  aria-label={tokenStrings.actions.edit}
  onClick={() => openTokenNoteEdit(t.id, t.note)}
>
  <Icon icon="mdi:pencil-outline" width={16} height={16} />
</Button>
<Button
  type="button"
  variant="ghost"
  size="icon"
  className="token-action-button shadow-none"
  title={tokenStrings.actions.delete}
  aria-label={tokenStrings.actions.delete}
  onClick={() => openTokenDeleteConfirm(t.id)}
  disabled={deletingId === t.id}
>
  <Icon
    icon={deletingId === t.id ? 'mdi:progress-helper' : 'mdi:trash-outline'}
    width={16}
    height={16}
    color="#ef4444"
  />
</Button>
                          </div>
                        </td>
                      )}
                    </tr>
                  )
                })}
              </tbody>
            </>
          )}
        </AdminTableShell>
        <AdminLoadingRegion
          className="admin-mobile-list admin-responsive-down"
          loadState={tokensLoadState}
          loadingLabel={tokensRefreshing ? loadingStateStrings.refreshing : tokenStrings.empty.loading}
          minHeight={260}
        >
          {tokenList.length === 0 ? (
            <div className="empty-state alert">{tokenStrings.empty.none}</div>
          ) : (
            tokenList.map((t) => {
              const stateKey = copyStateKey('tokens', t.id)
              const state = copyState.get(stateKey)
              const shareStateKey = copyStateKey('tokens', `${t.id}:share`)
              const shareState = copyState.get(shareStateKey)
              const quotaStateKey = t.quota_state ?? 'normal'
              const quotaLabel = quotaLabels[quotaStateKey] ?? quotaLabels.normal
              return (
                <article key={t.id} className="admin-mobile-card">
                  <div className="admin-mobile-kv">
                    <span>{tokenStrings.table.id}</span>
                    <strong>
                      <code>{t.id}</code>
                    </strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{tokenStrings.table.owner}</span>
                    <TokenOwnerValue owner={t.owner} emptyLabel={tokenStrings.owner.unbound} onOpenUser={navigateUser} compact />
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{tokenStrings.table.note}</span>
                    <strong>{t.note || '—'}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{tokenStrings.table.usage}</span>
                    <strong>{formatNumber(t.total_requests)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{tokenStrings.table.quota}</span>
                    <StatusBadge tone={quotaTone(quotaStateKey)} className={`token-quota-pill token-quota-pill-${quotaStateKey}`}>
                      {quotaLabel}
                    </StatusBadge>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{tokenStrings.table.lastUsed}</span>
                    <strong>{formatTimestamp(t.last_used_at)}</strong>
                  </div>
                  {isAdmin && (
                    <div className="admin-mobile-actions">
<Button
  type="button"
  variant={state === 'copied' ? 'success' : 'outline'}
  size="sm"
  onPointerEnter={() => scheduleSecretWarm(`token:${t.id}`, () => warmTokenSecret(t.id))}
  onPointerLeave={() => cancelSecretWarm(`token:${t.id}`)}
  onBlur={() => cancelSecretWarm(`token:${t.id}`)}
  onPointerDown={() => warmTokenSecret(t.id)}
  onKeyDown={(event) => { if (!isCopyIntentKey(event.key)) return; warmTokenSecret(t.id) }}
  onClick={(event) => void handleCopyToken(t.id, stateKey, event.currentTarget)}
  disabled={state === 'loading'}
>
  {tokenStrings.actions.copy}
</Button>
<Button
  type="button"
  variant={shareState === 'copied' ? 'success' : 'outline'}
  size="sm"
  onPointerEnter={() => scheduleSecretWarm(`token:${t.id}`, () => warmTokenSecret(t.id))}
  onPointerLeave={() => cancelSecretWarm(`token:${t.id}`)}
  onBlur={() => cancelSecretWarm(`token:${t.id}`)}
  onPointerDown={() => warmTokenSecret(t.id)}
  onKeyDown={(event) => { if (!isCopyIntentKey(event.key)) return; warmTokenSecret(t.id) }}
  onClick={(event) => void handleShareToken(t.id, shareStateKey, event.currentTarget)}
  disabled={shareState === 'loading'}
>
  {tokenStrings.actions.share}
</Button>
<Button type="button" variant="outline" size="sm" onClick={() => navigateToken(t.id)}>
  {keyStrings.actions.details}
</Button>
<Button
  type="button"
  variant="outline"
  size="sm"
  onClick={() => void toggleToken(t.id, t.enabled)}
  disabled={togglingId === t.id}
>
  {t.enabled ? tokenStrings.actions.disable : tokenStrings.actions.enable}
</Button>
<Button type="button" variant="outline" size="sm" onClick={() => openTokenNoteEdit(t.id, t.note)}>
  {tokenStrings.actions.edit}
</Button>
<Button
  type="button"
  variant="warning"
  size="sm"
  onClick={() => openTokenDeleteConfirm(t.id)}
  disabled={deletingId === t.id}
>
  {tokenStrings.actions.delete}
</Button>
                    </div>
                  )}
                </article>
              )
            })
          )}
        </AdminLoadingRegion>
        {tokensTotal > tokensPerPage && (
          <AdminTablePagination
            page={tokensPage}
            totalPages={totalPages}
            pageSummary={
              <span className="panel-description">
                {tokenStrings.pagination.page
                  .replace('{page}', String(tokensPage))
                  .replace('{total}', String(totalPages))}
              </span>
            }
            previousLabel={tokenStrings.pagination.prev}
            nextLabel={tokenStrings.pagination.next}
            previousDisabled={tokensPage <= 1}
            nextDisabled={tokensPage >= totalPages}
            disabled={tokensBlocking}
            onPrevious={goPrevPage}
            onNext={goNextPage}
          />
        )}
      </section>
      )}
      {error && <div className="surface error-banner">{error}</div>}

      {showKeys && (
      <section className="surface panel" style={keysBatchVisible ? { position: 'relative', zIndex: 40 } : undefined}>
	        <div className="panel-header" style={{ flexWrap: 'wrap', gap: 12, alignItems: 'flex-start' }}>
	          <div style={{ flex: '1 1 320px', minWidth: 240 }}>
	            <h2>{keyStrings.title}</h2>
	            <p className="panel-description">{keyStrings.description}</p>
	          </div>
	        </div>
          <div style={keysUtilityRowStyle}>
            <div style={keysFilterClusterStyle}>
              {hasKeyGroups && (
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button type="button" variant="outline" size="sm" aria-label={keyGroupFilterSummary}>
                      <Icon icon="mdi:filter-variant" width={16} height={16} aria-hidden="true" />
                      <span style={{ whiteSpace: 'nowrap' }}>{keyGroupFilterSummary}</span>
                      {selectedKeyGroups.length > 0 ? (
                        <Badge variant="neutral" className="ml-1 px-1.5 py-0 text-[10px]">
                          {selectedKeyGroups.length}
                        </Badge>
                      ) : null}
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="start" className="w-64">
                    <DropdownMenuLabel>{keyStrings.groups.label}</DropdownMenuLabel>
                    <DropdownMenuItem
                      className="cursor-pointer"
                      disabled={selectedKeyGroups.length === 0}
                      onSelect={(event) => {
                        event.preventDefault()
                        handleClearKeyGroupFilters()
                      }}
                    >
                      {keyStrings.filters.clearGroups}
                    </DropdownMenuItem>
                    <DropdownMenuSeparator />
                    {keyGroupFilterOptions.map((option) => (
                      <DropdownMenuCheckboxItem
                        key={option.value || '__ungrouped__'}
                        className="cursor-pointer"
                        checked={selectedKeyGroups.includes(option.value)}
                        onSelect={(event) => event.preventDefault()}
                        onCheckedChange={() => handleToggleKeyGroupFilter(option.value)}
                      >
                        <span>{option.label}</span>
                        <span className="ml-auto text-xs opacity-60">{formatNumber(option.count)}</span>
                      </DropdownMenuCheckboxItem>
                    ))}
                  </DropdownMenuContent>
                </DropdownMenu>
              )}
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button type="button" variant="outline" size="sm" aria-label={keyStatusFilterSummary}>
                    <Icon icon="mdi:filter-outline" width={16} height={16} aria-hidden="true" />
                    <span style={{ whiteSpace: 'nowrap' }}>{keyStatusFilterSummary}</span>
                    {selectedKeyStatuses.length > 0 ? (
                      <Badge variant="neutral" className="ml-1 px-1.5 py-0 text-[10px]">
                        {selectedKeyStatuses.length}
                      </Badge>
                    ) : null}
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="start" className="w-64">
                  <DropdownMenuLabel>{keyStrings.filters.status}</DropdownMenuLabel>
                  <DropdownMenuItem
                    className="cursor-pointer"
                    disabled={selectedKeyStatuses.length === 0}
                    onSelect={(event) => {
                      event.preventDefault()
                      handleClearKeyStatusFilters()
                    }}
                  >
                    {keyStrings.filters.clearStatuses}
                  </DropdownMenuItem>
                  <DropdownMenuSeparator />
                  {keyStatusFilterOptions.map((option) => (
                    <DropdownMenuCheckboxItem
                      key={option.value}
                      className="cursor-pointer"
                      checked={selectedKeyStatuses.includes(option.value)}
                      onSelect={(event) => event.preventDefault()}
                      onCheckedChange={() => handleToggleKeyStatusFilter(option.value)}
                    >
                      <span>{option.label}</span>
                      <span className="ml-auto text-xs opacity-60">{formatNumber(option.count)}</span>
                    </DropdownMenuCheckboxItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
            {isAdmin && (
              <div
                ref={keysBatchAnchorRef}
                onMouseEnter={() => {
                  clearKeysBatchAutoCollapseTimer()
                  clearKeysBatchCloseTimer()
                  setKeysBatchClosing(false)
                  if (keysBatchSuppressNextHoverRef.current) {
                    keysBatchSuppressNextHoverRef.current = false
                    return
                  }
                  keysBatchOpenReasonRef.current = 'hover'
                  setKeysBatchExpanded(true)
                }}
                onMouseLeave={() => {
                  keysBatchSuppressNextHoverRef.current = false
                  scheduleKeysBatchAutoCollapse('hover')
                }}
                onFocusCapture={() => {
                  clearKeysBatchAutoCollapseTimer()
                  clearKeysBatchCloseTimer()
                  setKeysBatchClosing(false)
                  keysBatchOpenReasonRef.current = 'focus'
                  setKeysBatchExpanded(true)
                }}
                style={{ ...keysQuickAddCardStyle, position: 'relative' }}
              >
                <div
                  className={`keys-batch-collapsed${keysBatchVisible ? ' is-hidden' : ''}`}
                  aria-hidden={keysBatchVisible}
                  style={keysQuickAddActionsStyle}
                >
                  <Input
                    ref={keysBatchCollapsedInputRef}
                    type="text"
                    name="collapsed-key-input"
                    placeholder={keyStrings.placeholder}
                    aria-label={keyStrings.placeholder}
                    value={keysBatchFirstLine}
                    onChange={(e) => setNewKeysText(e.target.value)}
                    disabled={keysBatchVisible}
                    style={{ flex: '1 1 260px', minWidth: 260, maxWidth: '100%' }}
                  />
                  <Button
                    type="button"
                    size="sm"
                    onClick={() => void handleAddKey()}
                    disabled={keysBatchVisible || submitting || keysBatchParsed.length === 0}
                    style={{ flexShrink: 0, whiteSpace: 'nowrap' }}
                  >
                    {submitting ? keyStrings.adding : keyStrings.addButton}
                  </Button>
                </div>
                <datalist id="api-key-group-datalist">
                  {namedKeyGroups.map((group) => (
                    <option key={group.name} value={group.name} />
                  ))}
                </datalist>
              </div>
            )}
          </div>
        <AdminTableShell
          className="jobs-table-wrapper admin-responsive-up"
          loadState={keysLoadState}
          loadingLabel={keysRefreshing ? loadingStateStrings.refreshing : keyStrings.empty.loading}
          errorLabel={keysError ?? loadingStateStrings.error}
          minHeight={320}
        >
          {visibleKeys.length === 0 ? (
            <tbody>
              <tr>
                <td colSpan={isAdmin ? 6 : 5}>
                  <div className="empty-state alert">
                    {keysHasFilters ? keyStrings.empty.filtered : keyStrings.empty.none}
                  </div>
                </td>
              </tr>
            </tbody>
          ) : (
            <>
              <thead>
                <tr>
                  <th>
                    <div style={adminTableHeaderStackStyle}>
                      <span style={adminTableFieldStyle}>{keyStrings.table.keyId}</span>
                      <span style={adminTableSecondaryFieldStyle}>{keyStrings.groups.label}</span>
                    </div>
                  </th>
                  <th>
                    <div style={adminTableHeaderStackStyle}>
                      <span style={adminTableFieldStyle}>{keyStrings.table.status}</span>
                      <span style={adminTableSecondaryFieldStyle} aria-hidden="true">&nbsp;</span>
                    </div>
                  </th>
                  <th>
                    <div style={adminTableHeaderStackStyle}>
                      <span style={adminTableFieldStyle}>{keyStrings.table.success}</span>
                      <span style={adminTableSecondaryFieldStyle}>{keyStrings.table.errors}</span>
                    </div>
                  </th>
                  <th>
                    <div style={adminTableHeaderStackStyle}>
                      <span style={adminTableFieldStyle}>{keyStrings.table.quotaLeft}</span>
                      <span style={adminTableSecondaryFieldStyle} aria-hidden="true">&nbsp;</span>
                    </div>
                  </th>
                  <th>
                    <div style={adminTableHeaderStackStyle}>
                      <span style={adminTableFieldStyle}>{keyStrings.table.lastUsed}</span>
                      <span style={adminTableSecondaryFieldStyle}>{keyStrings.table.statusChanged}</span>
                    </div>
                  </th>
                  {isAdmin && (
                    <th>
                      <div style={adminTableHeaderStackStyle}>
                        <span style={adminTableFieldStyle}>{keyStrings.table.actions}</span>
                        <span style={adminTableSecondaryFieldStyle} aria-hidden="true">&nbsp;</span>
                      </div>
                    </th>
                  )}
                </tr>
              </thead>
              <tbody>
                {visibleKeys.map((item) => {
                  const stateKey = copyStateKey('keys', item.id)
                  const state = copyState.get(stateKey)
                  const keyGroupName = formatKeyGroupName(item.group, keyStrings.groups.ungrouped)
                  return (
                    <tr key={item.id}>
                      <td>
                        <div style={adminTableStackStyle}>
                          <div style={adminTableInlineFieldStyle}>
                            <button
                              type="button"
                              className="link-button"
                              onClick={() => navigateKey(item.id, { preserveKeysContext: true })}
                              title={keyStrings.actions.details}
                              aria-label={keyStrings.actions.details}
                              style={{ whiteSpace: 'nowrap' }}
                            >
                              <code>{item.id}</code>
                            </button>
                            {isAdmin && (
<Button
  type="button"
  variant={state === 'copied' ? 'success' : 'ghost'}
  size="icon"
  className="h-8 w-8 rounded-full p-0 shadow-none"
  title={keyStrings.actions.copy}
  aria-label={keyStrings.actions.copy}
  onPointerEnter={() => scheduleSecretWarm(`key:${item.id}`, () => warmApiKeySecret(item.id))}
  onPointerLeave={() => cancelSecretWarm(`key:${item.id}`)}
  onBlur={() => cancelSecretWarm(`key:${item.id}`)}
  onPointerDown={() => warmApiKeySecret(item.id)}
  onKeyDown={(event) => { if (!isCopyIntentKey(event.key)) return; warmApiKeySecret(item.id) }}
  onClick={(event) => void handleCopySecret(item.id, stateKey, event.currentTarget)}
  disabled={state === 'loading'}
>
  <Icon icon={state === 'copied' ? 'mdi:check' : 'mdi:content-copy'} width={18} height={18} />
</Button>
                            )}
                          </div>
                          <span style={adminTableSecondaryFieldStyle}>{keyGroupName}</span>
                        </div>
                      </td>
                      <td>
                        <div style={adminTableStackStyle}>
                          <span style={adminTableFieldStyle}>
                            <StatusBadge tone={statusTone(keyBadgeStatus(item))}>
                              {statusLabel(keyBadgeStatus(item), adminStrings)}
                            </StatusBadge>
                          </span>
                        </div>
                      </td>
                      <td>
                        <div style={adminTableStackStyle}>
                          <span style={adminTableFieldStyle}>{formatNumber(item.success_count)}</span>
                          <span style={adminTableSecondaryFieldStyle}>{formatNumber(item.error_count)}</span>
                        </div>
                      </td>
                      <td>
                        <span style={adminTableFieldStyle}>
                          {item.quota_remaining != null && item.quota_limit != null
                            ? `${formatNumber(item.quota_remaining)} / ${formatNumber(item.quota_limit)}`
                            : '—'}
                        </span>
                      </td>
                      <td>
                        <div style={adminTableStackStyle}>
                          <span style={adminTableFieldStyle}>{formatTimestampNoYear(item.last_used_at)}</span>
                          <span style={adminTableSecondaryFieldStyle}>{formatTimestamp(item.status_changed_at)}</span>
                        </div>
                      </td>
                      {isAdmin && (
                        <td>
                          <div className="table-actions" style={{ flexWrap: 'nowrap' }}>
                            {item.quarantine ? (
  <Button
    type="button"
    variant="ghost"
    size="icon"
    className="h-8 w-8 rounded-full p-0 shadow-none"
    title={keyStrings.actions.clearQuarantine}
    aria-label={keyStrings.actions.clearQuarantine}
    onClick={() => void handleClearQuarantine(item.id)}
    disabled={clearingQuarantineId === item.id}
  >
    <Icon
      icon={clearingQuarantineId === item.id ? 'mdi:progress-helper' : 'mdi:shield-check-outline'}
      width={18}
      height={18}
    />
  </Button>
) : item.status === 'disabled' ? (
  <Button
    type="button"
    variant="ghost"
    size="icon"
    className="h-8 w-8 rounded-full p-0 shadow-none"
    title={keyStrings.actions.enable}
    aria-label={keyStrings.actions.enable}
    onClick={() => void handleToggleDisable(item.id, false)}
    disabled={togglingId === item.id}
  >
    <Icon icon={togglingId === item.id ? 'mdi:progress-helper' : 'mdi:play-circle-outline'} width={18} height={18} />
  </Button>
) : (
  <Button
    type="button"
    variant="ghost"
    size="icon"
    className="h-8 w-8 rounded-full p-0 shadow-none"
    title={keyStrings.actions.disable}
    aria-label={keyStrings.actions.disable}
    onClick={() => openDisableConfirm(item.id)}
    disabled={togglingId === item.id}
  >
    <Icon icon={togglingId === item.id ? 'mdi:progress-helper' : 'mdi:pause-circle-outline'} width={18} height={18} />
  </Button>
)}
<Button
  type="button"
  variant="ghost"
  size="icon"
  className="h-8 w-8 rounded-full p-0 shadow-none"
  title={keyStrings.actions.delete}
  aria-label={keyStrings.actions.delete}
  onClick={() => openDeleteConfirm(item.id)}
  disabled={deletingId === item.id || clearingQuarantineId === item.id}
>
  <Icon
    icon={deletingId === item.id ? 'mdi:progress-helper' : 'mdi:trash-outline'}
    width={18}
    height={18}
    color="#ef4444"
  />
</Button>
<Button
  type="button"
  variant="ghost"
  size="icon"
  className="h-8 w-8 rounded-full p-0 shadow-none"
  title={keyStrings.actions.details}
  aria-label={keyStrings.actions.details}
  onClick={() => navigateKey(item.id, { preserveKeysContext: true })}
  >
    <Icon icon="mdi:eye-outline" width={18} height={18} />
</Button>
                          </div>
                        </td>
                      )}
                    </tr>
                  )
                })}
              </tbody>
            </>
          )}
        </AdminTableShell>
        <AdminLoadingRegion
          className="admin-mobile-list admin-responsive-down"
          loadState={keysLoadState}
          loadingLabel={keysRefreshing ? loadingStateStrings.refreshing : keyStrings.empty.loading}
          errorLabel={keysError ?? loadingStateStrings.error}
          minHeight={240}
        >
          {visibleKeys.length === 0 ? (
            <div className="empty-state alert">
              {keysHasFilters ? keyStrings.empty.filtered : keyStrings.empty.none}
            </div>
          ) : (
            visibleKeys.map((item) => {
              const total = item.total_requests || 0
              const stateKey = copyStateKey('keys', item.id)
              const state = copyState.get(stateKey)
              return (
                <article key={item.id} className="admin-mobile-card">
                  <div className="admin-mobile-kv">
                    <span>{keyStrings.table.keyId}</span>
                    <strong>
                      <code>{item.id}</code>
                    </strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{keyStrings.table.status}</span>
                    <div>
                      <StatusBadge tone={statusTone(keyBadgeStatus(item))}>
                        {statusLabel(keyBadgeStatus(item), adminStrings)}
                      </StatusBadge>
                      {item.quarantine && (
                        <div className="panel-description" style={{ marginTop: 4 }}>
                          {keyStrings.quarantine.badge}: {item.quarantine.reasonSummary || keyStrings.quarantine.noReason}
                        </div>
                      )}
                    </div>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{keyStrings.table.total}</span>
                    <strong>{formatNumber(total)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{keyStrings.table.success}</span>
                    <strong>{formatNumber(item.success_count)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{keyStrings.table.errors}</span>
                    <strong>{formatNumber(item.error_count)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{keyStrings.table.quotaLeft}</span>
                    <strong>
                      {item.quota_remaining != null && item.quota_limit != null
                        ? `${formatNumber(item.quota_remaining)} / ${formatNumber(item.quota_limit)}`
                        : '—'}
                    </strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{keyStrings.table.lastUsed}</span>
                    <strong>{formatTimestampNoYear(item.last_used_at)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{keyStrings.table.statusChanged}</span>
                    <strong>{formatTimestamp(item.status_changed_at)}</strong>
                  </div>
                  {isAdmin && (
                    <div className="admin-mobile-actions">
<Button
  type="button"
  variant={state === 'copied' ? 'success' : 'outline'}
  size="sm"
  onPointerEnter={() => scheduleSecretWarm(`key:${item.id}`, () => warmApiKeySecret(item.id))}
  onPointerLeave={() => cancelSecretWarm(`key:${item.id}`)}
  onBlur={() => cancelSecretWarm(`key:${item.id}`)}
  onPointerDown={() => warmApiKeySecret(item.id)}
  onKeyDown={(event) => { if (!isCopyIntentKey(event.key)) return; warmApiKeySecret(item.id) }}
  onClick={(event) => void handleCopySecret(item.id, stateKey, event.currentTarget)}
  disabled={state === 'loading'}
>
  {keyStrings.actions.copy}
</Button>
{item.quarantine ? (
  <Button
    type="button"
    variant="outline"
    size="sm"
    onClick={() => void handleClearQuarantine(item.id)}
    disabled={clearingQuarantineId === item.id}
  >
    {keyStrings.actions.clearQuarantine}
  </Button>
) : item.status === 'disabled' ? (
  <Button
    type="button"
    variant="outline"
    size="sm"
    onClick={() => void handleToggleDisable(item.id, false)}
    disabled={togglingId === item.id || clearingQuarantineId === item.id}
  >
    {keyStrings.actions.enable}
  </Button>
) : (
  <Button
    type="button"
    variant="outline"
    size="sm"
    onClick={() => openDisableConfirm(item.id)}
    disabled={togglingId === item.id || clearingQuarantineId === item.id}
  >
    {keyStrings.actions.disable}
  </Button>
)}
<Button
  type="button"
  variant="warning"
  size="sm"
  onClick={() => openDeleteConfirm(item.id)}
  disabled={deletingId === item.id}
>
  {keyStrings.actions.delete}
</Button>
<Button type="button" variant="outline" size="sm" onClick={() => navigateKey(item.id, { preserveKeysContext: true })}>
  {keyStrings.actions.details}
</Button>
                    </div>
                  )}
                </article>
              )
            })
          )}
        </AdminLoadingRegion>
        {keysTotal > keysPerPage && (
          <AdminTablePagination
            page={keysPage}
            totalPages={keysTotalPages}
            pageSummary={
              <span className="panel-description">
                {keyStrings.pagination.page
                  .replace('{page}', String(keysPage))
                  .replace('{total}', String(keysTotalPages))}
              </span>
            }
            perPage={keysPerPage}
            perPageLabel={keyStrings.pagination.perPage}
            perPageAriaLabel={keyStrings.pagination.perPage}
            previousLabel={tokenStrings.pagination.prev}
            nextLabel={tokenStrings.pagination.next}
            previousDisabled={keysPage <= 1}
            nextDisabled={keysPage >= keysTotalPages}
            disabled={keysBlocking}
            onPrevious={goPrevKeysPage}
            onNext={goNextKeysPage}
            onPerPageChange={changeKeysPerPage}
          />
        )}
      </section>
      )}

      {showRequests && (
      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{logStrings.title}</h2>
            <p className="panel-description">{logStrings.description}</p>
          </div>
          <div className="panel-actions">
            <SegmentedTabs<'all' | 'success' | 'error' | 'quota_exhausted'>
              value={logResultFilter}
              onChange={(next) => {
                setLogResultFilter(next)
                setLogsPage(1)
              }}
              options={[
                { value: 'all', label: logStrings.filters.all },
                { value: 'success', label: logStrings.filters.success },
                { value: 'error', label: logStrings.filters.error },
                { value: 'quota_exhausted', label: logStrings.filters.quota },
              ]}
              ariaLabel={logStrings.title}
              disabled={requestsBlocking}
            />
          </div>
        </div>
        <AdminTableShell
          className="jobs-table-wrapper admin-responsive-up"
          tableClassName="admin-logs-table"
          loadState={requestsLoadState}
          loadingLabel={requestsRefreshing ? loadingStateStrings.refreshing : logStrings.empty.loading}
          errorLabel={requestsError ?? loadingStateStrings.error}
          minHeight={320}
        >
          {logs.length === 0 ? (
            <tbody>
              <tr>
                <td colSpan={7}>
                  <div className="empty-state alert">{logStrings.empty.none}</div>
                </td>
              </tr>
            </tbody>
          ) : (
            <>
              <thead>
                <tr>
                  <th>{logStrings.table.time}</th>
                  <th>{logStrings.table.key}</th>
                  <th>{logStrings.table.token}</th>
                  <th>{logStrings.table.httpStatus}</th>
                  <th>{logStrings.table.mcpStatus}</th>
                  <th>{logStrings.table.result}</th>
                  <th>{logStrings.table.error}</th>
                </tr>
              </thead>
              <tbody>
                {logs.map((log) => (
                  <LogRow
                    key={log.id}
                    log={log}
                    expanded={expandedLogs.has(log.id)}
                    onToggle={toggleLogExpansion}
                    strings={adminStrings}
                    onOpenKey={navigateKey}
                    onOpenToken={navigateToken}
                  />
                ))}
              </tbody>
            </>
          )}
        </AdminTableShell>
        <AdminLoadingRegion
          className="admin-mobile-list admin-responsive-down"
          loadState={requestsLoadState}
          loadingLabel={requestsRefreshing ? loadingStateStrings.refreshing : logStrings.empty.loading}
          errorLabel={requestsError ?? loadingStateStrings.error}
          minHeight={240}
        >
          {logs.length === 0 ? (
            <div className="empty-state alert">{logStrings.empty.none}</div>
          ) : (
            logs.map((log) => (
              <article key={log.id} className="admin-mobile-card">
                <div className="admin-mobile-kv">
                  <span>{logStrings.table.time}</span>
                  <strong>{formatTimestamp(log.created_at)}</strong>
                </div>
                <div className="admin-mobile-kv">
                  <span>{logStrings.table.key}</span>
                  <strong>
                    <code>{log.key_id ?? '—'}</code>
                  </strong>
                </div>
                <div className="admin-mobile-kv">
                  <span>{logStrings.table.token}</span>
                  <strong>
                    <code>{log.auth_token_id ?? '—'}</code>
                  </strong>
                </div>
                <div className="admin-mobile-kv">
                  <span>{logStrings.table.httpStatus}</span>
                  <strong>{log.http_status ?? '—'}</strong>
                </div>
                <div className="admin-mobile-kv">
                  <span>{logStrings.table.mcpStatus}</span>
                  <strong>{log.mcp_status ?? '—'}</strong>
                </div>
                <div className="admin-mobile-kv">
                  <span>{logStrings.table.result}</span>
                  <StatusBadge tone={statusTone(log.result_status)}>
                    {statusLabel(log.result_status, adminStrings)}
                  </StatusBadge>
                </div>
                <div className="admin-mobile-kv">
                  <span>{logStrings.table.error}</span>
                  <strong>{formatErrorMessage(log, adminStrings.logs.errors)}</strong>
                </div>
              </article>
            ))
          )}
        </AdminLoadingRegion>
        {hasLogsPagination && (
          <AdminTablePagination
            page={safeLogsPage}
            totalPages={logsTotalPages}
            pageSummary={<span className="panel-description">{logStrings.description} ({safeLogsPage} / {logsTotalPages})</span>}
            previousLabel={tokenStrings.pagination.prev}
            nextLabel={tokenStrings.pagination.next}
            previousDisabled={safeLogsPage <= 1}
            nextDisabled={safeLogsPage >= logsTotalPages}
            disabled={requestsBlocking}
            onPrevious={goPrevLogsPage}
            onNext={goNextLogsPage}
          />
        )}
      </section>
      )}

      {showJobs && (
      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{jobsStrings.title}</h2>
            <p className="panel-description">{jobsStrings.description}</p>
          </div>
          <div className="panel-actions">
            <SegmentedTabs<'all' | 'quota' | 'usage' | 'logs'>
              value={jobFilter}
              onChange={setJobFilter}
              options={[
                { value: 'all', label: jobsStrings.filters.all },
                { value: 'quota', label: jobsStrings.filters.quota },
                { value: 'usage', label: jobsStrings.filters.usage },
                { value: 'logs', label: jobsStrings.filters.logs },
              ]}
              ariaLabel={jobsStrings.title}
              disabled={jobsBlocking}
            />
          </div>
        </div>
        <AdminTableShell
          className="jobs-table-wrapper admin-responsive-up"
          tableClassName="jobs-table jobs-module-table"
          loadState={jobsLoadState}
          loadingLabel={jobsRefreshing ? loadingStateStrings.refreshing : jobsStrings.empty.loading}
          errorLabel={jobsError ?? loadingStateStrings.error}
          minHeight={320}
        >
          {jobs.length === 0 ? (
            <tbody>
              <tr>
                <td colSpan={7}>
                  <div className="empty-state alert">{jobsStrings.empty.none}</div>
                </td>
              </tr>
            </tbody>
          ) : (
            <>
              <thead>
                <tr>
                  <th>{jobsStrings.table.id}</th>
                  <th>{jobsStrings.table.type}</th>
                  <th>{jobsStrings.table.key}</th>
                  <th>{jobsStrings.table.status}</th>
                  <th>{jobsStrings.table.attempt}</th>
                  <th>{jobsStrings.table.started}</th>
                  <th>{jobsStrings.table.message}</th>
                </tr>
              </thead>
              <tbody>
                {jobs.map((j) => {
                  const jt = j.job_type
                  const jobTypeLabelText = jobTypeLabel(jt, jobsStrings)
                  const jobStatusText = jobStatusLabel(String(j.status ?? ''))
                  const keyId = j.key_id
                  const keyGroup = j.key_group
                  const started: number | null = j.started_at ?? null
                  const finished: number | null = j.finished_at ?? null
                  const startedTimeLabel = formatTimestamp(started)
                  const startedDetail =
                    started != null
                      ? `${formatTimestampWithMs(started)} · ${formatRelativeTime(started)}`
                      : jobsStrings.empty.none
                  const isExpanded = expandedJobs.has(j.id)
                  const jobMessage: string | null = j.message ?? null
                  const messageLabel = isExpanded
                    ? jobsStrings.toggles?.hide ?? jobsStrings.table.message
                    : jobsStrings.toggles?.show ?? jobsStrings.table.message
                  const duration =
                    started != null && finished != null
                      ? (() => {
                          const seconds = Math.max(0, finished - started)
                          if (seconds < 60) return `${seconds}s`
                          const minutes = Math.round(seconds / 60)
                          return `${minutes}m`
                        })()
                      : null
                  const startedSummary =
                    started != null ? `${formatTimestampWithMs(started)} · ${formatRelativeTime(started)}` : null
                  const finishedSummary =
                    finished != null ? `${formatTimestampWithMs(finished)} · ${formatRelativeTime(finished)}` : null
                  const rows: JSX.Element[] = []

                  rows.push(
                    <tr key={j.id}>
                      <td>{j.id}</td>
                      <td>{jobTypeLabelText}</td>
                      <td>
                        <JobKeyLink
                          keyId={keyId}
                          keyGroup={keyGroup}
                          ungroupedLabel={keyStrings.groups.ungrouped}
                          detailLabel={keyStrings.actions.details}
                          onOpenKey={navigateKey}
                        />
                      </td>
                      <td>
                        <StatusBadge tone={statusTone(j.status)} title={String(j.status ?? '')}>
                          {jobStatusText}
                        </StatusBadge>
                      </td>
                      <td>{j.attempt}</td>
                      <td>{started ? startedTimeLabel : '—'}</td>
                      <td className="jobs-message-cell">
                        {jobMessage ? (
                          <button
                            type="button"
                            className={`jobs-message-button${isExpanded ? ' jobs-message-button-active' : ''}`}
                            onClick={() => toggleJobExpansion(j.id)}
                            aria-expanded={isExpanded}
                            aria-controls={`job-details-${j.id}`}
                            aria-label={messageLabel}
                            title={jobMessage}
                          >
                            <span className="jobs-message-text">{jobMessage}</span>
                            <Icon
                              icon={isExpanded ? 'mdi:chevron-up' : 'mdi:chevron-down'}
                              width={16}
                              height={16}
                              className="jobs-message-icon"
                              aria-hidden="true"
                            />
                          </button>
                        ) : (
                          '—'
                        )}
                      </td>
                    </tr>,
                  )

                  if (isExpanded) {
                    rows.push(
                      <tr key={`${j.id}-details`} className="log-details-row">
                        <td colSpan={7} id={`job-details-${j.id}`}>
                          <div className="log-details-panel">
                            <div className="log-details-summary">
                              <div>
                                <div className="log-details-label">{jobsStrings.table.id}</div>
                                <div className="log-details-value">{j.id}</div>
                              </div>
                              <div>
                                <div className="log-details-label">{jobsStrings.table.type}</div>
                                <div className="log-details-value">
                                  {jt ? (
                                    <span className="job-type-pill">
                                      <button
                                        type="button"
                                        className="job-type-trigger"
                                        aria-label={jt}
                                      >
                                        <span className="job-type-main">{jobTypeLabelText}</span>
                                      </button>
                                      <div className="job-type-bubble">{jt}</div>
                                    </span>
                                  ) : (
                                    '—'
                                  )}
                                </div>
                              </div>
                              <div>
                                <div className="log-details-label">{jobsStrings.table.key}</div>
                                <div className="log-details-value">
                                  <JobKeyLink
                                    keyId={keyId}
                                    keyGroup={keyGroup}
                                    ungroupedLabel={keyStrings.groups.ungrouped}
                                    detailLabel={keyStrings.actions.details}
                                    onOpenKey={navigateKey}
                                  />
                                </div>
                              </div>
                              <div>
                                <div className="log-details-label">{jobsStrings.table.status}</div>
                                <div className="log-details-value">{jobStatusText}</div>
                              </div>
                              <div>
                                <div className="log-details-label">{jobsStrings.table.attempt}</div>
                                <div className="log-details-value">{j.attempt}</div>
                              </div>
                              <div>
                                <div className="log-details-label">{jobsStrings.table.started}</div>
                                <div className="log-details-value">
                                  {startedSummary ?? jobsStrings.empty.none}
                                </div>
                              </div>
                              {finishedSummary && (
                                <div>
                                  <div className="log-details-label">Finished</div>
                                  <div className="log-details-value">
                                    {finishedSummary}
                                  </div>
                                </div>
                              )}
                              {duration && (
                                <div>
                                  <div className="log-details-label">DURATION</div>
                                  <div className="log-details-value">{duration}</div>
                                </div>
                              )}
                            </div>
                            {jobMessage && (
                              <div className="log-details-body">
                                <section className="log-details-section">
                                  <header>{jobsStrings.table.message}</header>
                                  <pre>{jobMessage}</pre>
                                </section>
                              </div>
                            )}
                          </div>
                        </td>
                      </tr>,
                    )
                  }

                  return rows
                })}
              </tbody>
            </>
          )}
        </AdminTableShell>
        <AdminLoadingRegion
          className="admin-mobile-list admin-responsive-down"
          loadState={jobsLoadState}
          loadingLabel={jobsRefreshing ? loadingStateStrings.refreshing : jobsStrings.empty.loading}
          errorLabel={jobsError ?? loadingStateStrings.error}
          minHeight={240}
        >
          {jobs.length === 0 ? (
            <div className="empty-state alert">{jobsStrings.empty.none}</div>
          ) : (
            jobs.map((j) => {
              const jt = j.job_type
              const started: number | null = j.started_at ?? null
              return (
                <article key={j.id} className="admin-mobile-card">
                  <div className="admin-mobile-kv">
                    <span>{jobsStrings.table.id}</span>
                    <strong>{j.id}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{jobsStrings.table.type}</span>
                    <strong>{jobTypeLabel(jt, jobsStrings)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{jobsStrings.table.key}</span>
                    <strong>
                      <JobKeyLink
                        keyId={j.key_id}
                        keyGroup={j.key_group}
                        ungroupedLabel={keyStrings.groups.ungrouped}
                        detailLabel={keyStrings.actions.details}
                        onOpenKey={navigateKey}
                        showBubble={false}
                      />
                    </strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{jobsStrings.table.status}</span>
                    <StatusBadge tone={statusTone(j.status)} title={String(j.status ?? '')}>
                      {jobStatusLabel(String(j.status ?? ''))}
                    </StatusBadge>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{jobsStrings.table.attempt}</span>
                    <strong>{j.attempt}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{jobsStrings.table.started}</span>
                    <strong>{started ? formatTimestamp(started) : '—'}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{jobsStrings.table.message}</span>
                    <strong>{j.message ?? '—'}</strong>
                  </div>
                </article>
              )
            })
          )}
        </AdminLoadingRegion>
        {jobsTotal > jobsPerPage && (
          <AdminTablePagination
            page={jobsPage}
            totalPages={Math.max(1, Math.ceil(jobsTotal / jobsPerPage))}
            pageSummary={
              <span className="panel-description">
                {jobsStrings.description} ({jobsPage} / {Math.max(1, Math.ceil(jobsTotal / jobsPerPage))})
              </span>
            }
            previousLabel={tokenStrings.pagination.prev}
            nextLabel={tokenStrings.pagination.next}
            previousDisabled={jobsPage <= 1}
            nextDisabled={jobsPage >= Math.ceil(jobsTotal / jobsPerPage)}
            disabled={jobsBlocking}
            onPrevious={() => setJobsPage((page) => Math.max(1, page - 1))}
            onNext={() => setJobsPage((page) => page + 1)}
          />
        )}
      </section>
      )}

      {showUsers && (
        <>
          {renderUserTagSummaryPanel()}

          <section className="surface panel">
            <div className="panel-header" style={{ gap: 12, flexWrap: 'wrap' }}>
              <div style={{ flex: '1 1 340px', minWidth: 260 }}>
                <h2>{usersStrings.title}</h2>
                <p className="panel-description">{usersStrings.description}</p>
              </div>
              <div style={{ display: 'flex', flex: '1 1 520px', flexWrap: 'wrap', gap: 12, justifyContent: 'flex-end' }}>
                <div
                  className="rounded-xl border border-border/60 bg-background/55 px-4 py-3 shadow-sm backdrop-blur"
                  style={{
                    display: 'flex',
                    minWidth: 260,
                    maxWidth: 380,
                    flex: '1 1 300px',
                    alignItems: 'flex-start',
                    justifyContent: 'space-between',
                    gap: 12,
                  }}
                >
                  <div style={{ minWidth: 0, flex: '1 1 auto' }}>
                    <div
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        gap: 8,
                        flexWrap: 'wrap',
                      }}
                    >
                      <div className="text-sm font-semibold">{usersStrings.registration.title}</div>
                      <Badge
                        variant={
                          registrationSettingsError
                            ? 'destructive'
                            : allowRegistration === null
                              ? 'secondary'
                              : allowRegistration
                                ? 'success'
                                : 'warning'
                        }
                      >
                        {allowRegistration === null
                          ? usersStrings.status.unknown
                          : allowRegistration
                            ? usersStrings.status.enabled
                            : usersStrings.status.disabled}
                      </Badge>
                    </div>
                    {registrationInlineStatus && (
                      <p
                        className="text-xs font-medium"
                        role="status"
                        aria-live="polite"
                        style={{ margin: '6px 0 0', color: registrationSettingsError ? 'hsl(var(--destructive))' : undefined }}
                      >
                        {registrationInlineStatus}
                      </p>
                    )}
                  </div>
                  <Switch
                    disabled={registrationSettingsLoading || registrationSettingsSaving || allowRegistration === null}
                    checked={allowRegistration ?? false}
                    aria-label={usersStrings.registration.title}
                    onCheckedChange={() => void toggleAllowRegistration()}
                    style={{ flex: '0 0 auto' }}
                  />
                </div>
                <div className="users-search-controls">
                  <Input
                    type="text"
                    name="users-search"
                    className="users-search-input"
                    placeholder={usersStrings.searchPlaceholder}
                    value={usersQueryInput}
                    disabled={usersBlocking}
                    onChange={(event) => setUsersQueryInput(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter') {
                        event.preventDefault()
                        applyUserSearch()
                      }
                    }}
                  />
                  <Button type="button" variant="outline" onClick={applyUserSearch} disabled={usersBlocking}>
                    {usersStrings.search}
                  </Button>
                  {(usersQueryInput.length > 0 || usersQuery.length > 0 || usersTagFilterId != null) && (
                    <Button type="button" variant="ghost" onClick={resetUserSearch} disabled={usersBlocking}>
                      {usersStrings.clear}
                    </Button>
                  )}
                </div>
              </div>
            </div>
            {registrationSettingsError && (
              <div className="surface error-banner" style={{ marginBottom: 12 }}>
                {registrationSettingsError}
              </div>
            )}

            <AdminTableShell
              className="jobs-table-wrapper"
              tableClassName="jobs-table admin-users-table admin-users-list-table"
              loadState={usersLoadState}
              loadingLabel={usersRefreshing ? loadingStateStrings.refreshing : usersStrings.empty.loading}
              errorLabel={usersError ?? loadingStateStrings.error}
              minHeight={360}
            >
              {users.length === 0 ? (
                <tbody>
                  <tr>
                    <td colSpan={13}>
                      <div className="empty-state alert">{usersStrings.empty.none}</div>
                    </td>
                  </tr>
                </tbody>
              ) : (
                <>
                  <thead>
                    <tr>
                      <th>{usersStrings.table.user}</th>
                      <th>{usersStrings.table.status}</th>
                      <th>{usersStrings.table.tokenCount}</th>
                      <th>{usersStrings.table.tags}</th>
                      <th>{usersStrings.table.hourlyAny}</th>
                      <th>{usersStrings.table.hourly}</th>
                      <th>{usersStrings.table.daily}</th>
                      <th>{usersStrings.table.monthly}</th>
                      <th>{usersStrings.table.successDaily}</th>
                      <th>{usersStrings.table.successMonthly}</th>
                      <th>{usersStrings.table.lastActivity}</th>
                      <th>{usersStrings.table.lastLogin}</th>
                      <th>{usersStrings.table.actions}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {users.map((item) => (
                      <tr key={item.userId}>
                        <td>
                          <button
                            type="button"
                            className="link-button"
                            onClick={() => navigateUser(item.userId, { preserveUsersContext: true })}
                          >
                            <strong>{item.displayName || item.username || item.userId}</strong>
                          </button>
                          <div className="panel-description" style={{ marginTop: 4 }}>
                            <code>{item.userId}</code>
                            {item.username ? ` · @${item.username}` : ''}
                          </div>
                        </td>
                        <td>
                          <StatusBadge tone={item.active ? 'success' : 'neutral'}>
                            {item.active ? usersStrings.status.active : usersStrings.status.inactive}
                          </StatusBadge>
                        </td>
                        <td>{formatNumber(item.tokenCount)}</td>
                        <td>
                          <UserTagBadgeList
                            tags={item.tags}
                            usersStrings={usersStrings}
                            emptyLabel={usersStrings.userTags.empty}
                          />
                        </td>
                        <td>{formatQuotaUsagePair(item.hourlyAnyUsed, item.hourlyAnyLimit)}</td>
                        <td>{formatQuotaUsagePair(item.quotaHourlyUsed, item.quotaHourlyLimit)}</td>
                        <td>{formatQuotaUsagePair(item.quotaDailyUsed, item.quotaDailyLimit)}</td>
                        <td>{formatQuotaUsagePair(item.quotaMonthlyUsed, item.quotaMonthlyLimit)}</td>
                        <td>{formatNumber(item.dailySuccess)} / {formatNumber(item.dailyFailure)}</td>
                        <td>{formatNumber(item.monthlySuccess)}</td>
                        <td>{formatTimestamp(item.lastActivity)}</td>
                        <td>{formatTimestamp(item.lastLoginAt)}</td>
                        <td>
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8 rounded-full p-0 shadow-none"
                            title={usersStrings.actions.view}
                            aria-label={usersStrings.actions.view}
                            onClick={() => navigateUser(item.userId, { preserveUsersContext: true })}
                          >
                            <Icon icon="mdi:eye-outline" width={16} height={16} />
                          </Button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </>
              )}
            </AdminTableShell>
            <AdminLoadingRegion
              className="admin-mobile-list admin-responsive-down"
              loadState={usersLoadState}
              loadingLabel={usersRefreshing ? loadingStateStrings.refreshing : usersStrings.empty.loading}
              errorLabel={usersError ?? loadingStateStrings.error}
              minHeight={260}
            >
              {users.length === 0 ? (
                <div className="empty-state alert">{usersStrings.empty.none}</div>
              ) : (
                users.map((item) => (
                  <article key={item.userId} className="admin-mobile-card">
                    <div className="admin-mobile-kv">
                      <span>{usersStrings.table.user}</span>
                      <strong>{item.displayName || item.username || item.userId}</strong>
                    </div>
                    <div className="admin-mobile-kv">
                      <span>{usersStrings.table.status}</span>
                      <StatusBadge tone={item.active ? 'success' : 'neutral'}>
                        {item.active ? usersStrings.status.active : usersStrings.status.inactive}
                      </StatusBadge>
                    </div>
                    <div className="admin-mobile-kv">
                      <span>{usersStrings.table.tokenCount}</span>
                      <strong>{formatNumber(item.tokenCount)}</strong>
                    </div>
                    <div className="admin-mobile-kv">
                      <span>{usersStrings.table.tags}</span>
                      <UserTagBadgeList
                        tags={item.tags}
                        usersStrings={usersStrings}
                        emptyLabel={usersStrings.userTags.empty}
                      />
                    </div>
                    <div className="admin-mobile-kv">
                      <span>{usersStrings.table.hourlyAny}</span>
                      <strong>{formatQuotaUsagePair(item.hourlyAnyUsed, item.hourlyAnyLimit)}</strong>
                    </div>
                    <div className="admin-mobile-kv">
                      <span>{usersStrings.table.hourly}</span>
                      <strong>{formatQuotaUsagePair(item.quotaHourlyUsed, item.quotaHourlyLimit)}</strong>
                    </div>
                    <div className="admin-mobile-kv">
                      <span>{usersStrings.table.daily}</span>
                      <strong>{formatQuotaUsagePair(item.quotaDailyUsed, item.quotaDailyLimit)}</strong>
                    </div>
                    <div className="admin-mobile-kv">
                      <span>{usersStrings.table.monthly}</span>
                      <strong>{formatQuotaUsagePair(item.quotaMonthlyUsed, item.quotaMonthlyLimit)}</strong>
                    </div>
                    <div className="admin-mobile-kv">
                      <span>{usersStrings.table.successDaily}</span>
                      <strong>{`${formatNumber(item.dailySuccess)} / ${formatNumber(item.dailyFailure)}`}</strong>
                    </div>
                    <div className="admin-mobile-kv">
                      <span>{usersStrings.table.successMonthly}</span>
                      <strong>{formatNumber(item.monthlySuccess)}</strong>
                    </div>
                    <div className="admin-mobile-kv">
                      <span>{usersStrings.table.lastActivity}</span>
                      <strong>{formatTimestamp(item.lastActivity)}</strong>
                    </div>
                    <div className="admin-mobile-kv">
                      <span>{usersStrings.table.lastLogin}</span>
                      <strong>{formatTimestamp(item.lastLoginAt)}</strong>
                    </div>
                    <div className="admin-mobile-actions">
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        onClick={() => navigateUser(item.userId, { preserveUsersContext: true })}
                      >
                        {usersStrings.actions.view}
                      </Button>
                    </div>
                  </article>
                ))
              )}
            </AdminLoadingRegion>

            {usersTotal > USERS_PER_PAGE && (
              <AdminTablePagination
                page={usersPage}
                totalPages={usersTotalPages}
                pageSummary={
                  <span className="panel-description">
                    {usersStrings.pagination
                      .replace('{page}', String(usersPage))
                      .replace('{total}', String(usersTotalPages))}
                  </span>
                }
                previousLabel={tokenStrings.pagination.prev}
                nextLabel={tokenStrings.pagination.next}
                previousDisabled={usersPage <= 1}
                nextDisabled={usersPage >= usersTotalPages}
                disabled={usersBlocking}
                onPrevious={goPrevUsersPage}
                onNext={goNextUsersPage}
              />
            )}
          </section>
        </>
      )}
      {showAlerts && (

        <ModulePlaceholder
          title={adminStrings.modules.alerts.title}
          description={adminStrings.modules.alerts.description}
          comingSoonLabel={adminStrings.modules.comingSoon}
          sections={[
            adminStrings.modules.alerts.sections.rules,
            adminStrings.modules.alerts.sections.thresholds,
            adminStrings.modules.alerts.sections.channels,
          ]}
        />
      )}

      {showProxySettings && (
        <ModulePlaceholder
          title={adminStrings.modules.proxySettings.title}
          description={adminStrings.modules.proxySettings.description}
          comingSoonLabel={adminStrings.modules.comingSoon}
          sections={[
            adminStrings.modules.proxySettings.sections.upstream,
            adminStrings.modules.proxySettings.sections.routing,
            adminStrings.modules.proxySettings.sections.rateLimit,
          ]}
        />
      )}

      <div className="app-footer">
        <span>{footerStrings.title}</span>
        <span className="footer-meta">
          {/* GitHub repository link with Iconify icon */}
          <a
            href="https://github.com/IvanLi-CN/tavily-hikari"
            className="footer-link"
            target="_blank"
            rel="noreferrer"
            aria-label={footerStrings.githubAria}
          >
            <Icon icon="mdi:github" width={18} height={18} className="footer-link-icon" />
            <span>{footerStrings.githubLabel}</span>
          </a>
        </span>
        <span className="footer-meta">
          {version ? (
            (() => {
              const raw = version.backend || ''
              const clean = raw.replace(/-.+$/, '')
              const tag = clean.startsWith('v') ? clean : `v${clean}`
              const href = `https://github.com/IvanLi-CN/tavily-hikari/releases/tag/${tag}`
              return (
                <>
                  {footerStrings.tagPrefix}
                  <a href={href} className="footer-link" target="_blank" rel="noreferrer">
                    {`v${raw}`}
                  </a>
                </>
              )
            })()
          ) : (
            footerStrings.loadingVersion
          )}
        </span>
      </div>
    </AdminShell>
    {/* Batch Create Tokens modal */}
    <Dialog open={batchDialogOpen} onOpenChange={(open) => { if (!open) closeBatchDialog() }}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{tokenStrings.batchDialog.title}</DialogTitle>
        </DialogHeader>
        {batchShareText == null ? (
          <>
            <div className="flex flex-col gap-3 py-2 sm:flex-row">
              <Input
                type="text"
                name="batch-token-group"
                placeholder={tokenStrings.batchDialog.groupPlaceholder}
                value={batchGroup}
                onChange={(e) => setBatchGroup(e.target.value)}
                style={{ flex: 1 }}
              />
              <Input
                type="number"
                name="batch-token-count"
                min={1}
                max={1000}
                value={batchCount}
                onChange={(e) => setBatchCount(Number(e.target.value) || 1)}
                className="w-full sm:w-[120px]"
              />
            </div>
            <DialogFooter className="modal-action">
              <Button type="button" variant="outline" onClick={closeBatchDialog}>
                {tokenStrings.batchDialog.cancel}
              </Button>
              <Button type="button" onClick={() => void submitBatchCreate()} disabled={batchCreating}>
                {batchCreating ? tokenStrings.batchDialog.creating : tokenStrings.batchDialog.confirm}
              </Button>
            </DialogFooter>
          </>
        ) : (
          <>
            <div className="batch-dialog-body">
              <p className="py-2">
                {tokenStrings.batchDialog.createdN.replace(
                  '{n}',
                  String((batchShareText ?? '').split('\n').filter((line) => line.length > 0).length),
                )}
              </p>
              <Textarea
                readOnly
                wrap="off"
                rows={6}
                className="min-h-[144px] resize-none"
                style={{
                  width: '100%',
                  fontFamily:
                    'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
                  whiteSpace: 'pre',
                  overflowX: 'auto',
                  overflowY: 'auto',
                }}
                value={batchShareText ?? ''}
                onClick={(event) => selectAllReadonlyText(event.currentTarget)}
                onFocus={(event) => selectAllReadonlyText(event.currentTarget)}
              />
            </div>
            <DialogFooter className="modal-action">
              <Button
                type="button"
                variant="outline"
                onClick={async () => {
                  if (!batchShareText) return
                  const copyResult = await copyToClipboard(batchShareText, { preferExecCommand: true })
                  if (!copyResult.ok) {
                    setError(errorStrings.copyToken)
                  }
                }}
              >
                {tokenStrings.batchDialog.copyAll}
              </Button>
              <Button type="button" onClick={closeBatchDialog}>
                {tokenStrings.batchDialog.done}
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>

{/* API Keys Validation modal */}
<ApiKeysValidationDialog
  open={keysValidationVisibleState != null}
  state={keysValidationVisibleState}
  counts={keysValidationCounts}
  validKeys={keysValidationValidKeys}
  exhaustedKeys={keysValidationExhaustedKeys}
  onClose={closeKeysValidationDialog}
  onRetryFailed={() => void handleRetryFailedValidation()}
  onRetryOne={(apiKey) => void handleRetryOneValidation(apiKey)}
  onImportValid={() => void handleImportValidatedKeys()}
/>

{/* Batch Add API Keys Report modal */}
<Dialog open={keysBatchReport != null} onOpenChange={(open) => { if (!open) closeKeysBatchReportDialog() }}>
  <DialogContent className="max-w-4xl sm:max-h-[min(calc(100dvh-6rem),calc(100vh-6rem))]">
    <DialogHeader>
      <DialogTitle>{keyStrings.batch.report.title}</DialogTitle>
    </DialogHeader>
    <div style={{ overflowY: 'auto', minHeight: 0, paddingTop: 12 }}>
      {keysBatchReport?.kind === 'error' ? (
        <>
          <div className="alert alert-error">
            {keysBatchReport.message}
          </div>
          <div className="py-2" style={{ display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: 8 }}>
            <div>
              <span className="opacity-70">{keyStrings.batch.report.summary.inputLines}</span> {formatNumber(keysBatchReport.input_lines)}
            </div>
            <div>
              <span className="opacity-70">{keyStrings.batch.report.summary.validLines}</span> {formatNumber(keysBatchReport.valid_lines)}
            </div>
          </div>
        </>
      ) : keysBatchReport?.kind === 'success' ? (
        <div className="grid gap-4 lg:grid-cols-2">
          <div>
            <div className="py-2" style={{ display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: 8 }}>
              <div>
                <span className="opacity-70">{keyStrings.batch.report.summary.inputLines}</span>{' '}
                {formatNumber(keysBatchReport.response.summary.input_lines)}
              </div>
              <div>
                <span className="opacity-70">{keyStrings.batch.report.summary.validLines}</span>{' '}
                {formatNumber(keysBatchReport.response.summary.valid_lines)}
              </div>
              <div>
                <span className="opacity-70">{keyStrings.batch.report.summary.uniqueInInput}</span>{' '}
                {formatNumber(keysBatchReport.response.summary.unique_in_input)}
              </div>
              <div>
                <span className="opacity-70">{keyStrings.batch.report.summary.duplicateInInput}</span>{' '}
                {formatNumber(keysBatchReport.response.summary.duplicate_in_input)}
              </div>
              <div>
                <span className="opacity-70">{keyStrings.batch.report.summary.created}</span>{' '}
                {formatNumber(keysBatchReport.response.summary.created)}
              </div>
              <div>
                <span className="opacity-70">{keyStrings.batch.report.summary.undeleted}</span>{' '}
                {formatNumber(keysBatchReport.response.summary.undeleted)}
              </div>
              <div>
                <span className="opacity-70">{keyStrings.batch.report.summary.existed}</span>{' '}
                {formatNumber(keysBatchReport.response.summary.existed)}
              </div>
              <div>
                <span className="opacity-70">{keyStrings.batch.report.summary.failed}</span>{' '}
                {formatNumber(keysBatchReport.response.summary.failed)}
              </div>
            </div>
          </div>

          <div>
            <h4 className="font-bold">{keyStrings.batch.report.failures.title}</h4>
            {keysBatchFailures.length === 0 ? (
              <div className="py-2">{keyStrings.batch.report.failures.none}</div>
            ) : (
              <div
                className="overflow-x-auto"
                style={{
                  marginTop: 8,
                  maxHeight: 'min(calc(100dvh - 18rem), calc(100vh - 18rem))',
                  overflowY: 'auto',
                }}
              >
                <Table className="table-zebra">
                  <thead>
                    <tr>
                      <th>{keyStrings.batch.report.failures.table.apiKey}</th>
                      <th>{keyStrings.batch.report.failures.table.error}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {keysBatchFailures.map((item, index) => (
                      <tr key={`${item.api_key}-${index}`}>
                        <td style={{ wordBreak: 'break-all' }}>
                          <code>{item.api_key}</code>
                        </td>
                        <td style={{ wordBreak: 'break-word' }}>{item.error || '—'}</td>
                      </tr>
                    ))}
                  </tbody>
                </Table>
              </div>
            )}
          </div>
        </div>
      ) : (
        <div className="py-2">{keyStrings.batch.hint}</div>
      )}
    </div>
    <DialogFooter className="modal-action" style={{ marginTop: 12 }}>
      <Button type="button" variant="outline" onClick={closeKeysBatchReportDialog}>
        {keyStrings.batch.report.close}
      </Button>
    </DialogFooter>
  </DialogContent>
</Dialog>

{/* Disable Confirmation modal */}
<Dialog open={pendingDisableId != null} onOpenChange={(open) => { if (!open) cancelDisable() }}>
  <DialogContent className="max-w-md">
    <DialogHeader>
      <DialogTitle>{keyStrings.dialogs.disable.title}</DialogTitle>
      <DialogDescription>{keyStrings.dialogs.disable.description}</DialogDescription>
    </DialogHeader>
    <DialogFooter className="modal-action">
      <Button type="button" variant="outline" onClick={cancelDisable}>
        {keyStrings.dialogs.disable.cancel}
      </Button>
      <Button type="button" onClick={() => void confirmDisable()} disabled={!!togglingId}>
        {keyStrings.dialogs.disable.confirm}
      </Button>
    </DialogFooter>
  </DialogContent>
</Dialog>

{/* Delete Confirmation modal */}
<Dialog open={pendingDeleteId != null} onOpenChange={(open) => { if (!open) cancelDelete() }}>
  <DialogContent className="max-w-md">
    <DialogHeader>
      <DialogTitle>{keyStrings.dialogs.delete.title}</DialogTitle>
      <DialogDescription>{keyStrings.dialogs.delete.description}</DialogDescription>
    </DialogHeader>
    <DialogFooter className="modal-action">
      <Button type="button" variant="outline" onClick={cancelDelete}>
        {keyStrings.dialogs.delete.cancel}
      </Button>
      <Button type="button" variant="destructive" onClick={() => void confirmDelete()} disabled={!!deletingId}>
        {keyStrings.dialogs.delete.confirm}
      </Button>
    </DialogFooter>
  </DialogContent>
</Dialog>

{/* Token Delete Confirmation */}
<Dialog open={pendingTokenDeleteId != null} onOpenChange={(open) => { if (!open) cancelTokenDelete() }}>
  <DialogContent className="max-w-md">
    <DialogHeader>
      <DialogTitle>{tokenStrings.dialogs.delete.title}</DialogTitle>
      <DialogDescription>{tokenStrings.dialogs.delete.description}</DialogDescription>
    </DialogHeader>
    <DialogFooter className="modal-action">
      <Button type="button" variant="outline" onClick={cancelTokenDelete}>
        {tokenStrings.dialogs.delete.cancel}
      </Button>
      <Button type="button" variant="destructive" onClick={() => void confirmTokenDelete()} disabled={!!deletingId}>
        {tokenStrings.dialogs.delete.confirm}
      </Button>
    </DialogFooter>
  </DialogContent>
</Dialog>

{/* Token Edit Note modal */}
<Dialog open={editingTokenId != null} onOpenChange={(open) => { if (!open) cancelTokenNote() }}>
  <DialogContent className="max-w-lg">
    <DialogHeader>
      <DialogTitle>{tokenStrings.dialogs.note.title}</DialogTitle>
    </DialogHeader>
    <Input
      type="text"
      name="editing-token-note"
      placeholder={tokenStrings.dialogs.note.placeholder}
      value={editingTokenNote}
      onChange={(e) => setEditingTokenNote(e.target.value)}
    />
    <DialogFooter className="modal-action">
      <Button type="button" variant="outline" onClick={cancelTokenNote}>
        {tokenStrings.dialogs.note.cancel}
      </Button>
      <Button type="button" onClick={() => void saveTokenNote()} disabled={savingTokenNote}>
        {savingTokenNote ? tokenStrings.dialogs.note.saving : tokenStrings.dialogs.note.confirm}
      </Button>
    </DialogFooter>
  </DialogContent>
</Dialog>
<Dialog open={manualCopyDialog != null} onOpenChange={(open) => { if (!open) setManualCopyDialog(null) }}>
  <DialogContent
    className="max-w-lg"
    onEscapeKeyDown={(event) => event.preventDefault()}
    onInteractOutside={(event) => event.preventDefault()}
  >
    <DialogHeader>
      <DialogTitle>{manualCopyDialog?.title ?? manualCopyText.createToken.title}</DialogTitle>
      <DialogDescription>{manualCopyDialog?.description ?? manualCopyText.createToken.description}</DialogDescription>
    </DialogHeader>
    <div className="grid gap-2">
      <label className="manual-copy-bubble-label" htmlFor="admin-manual-copy-dialog-field">
        {manualCopyDialog?.fieldLabel ?? manualCopyText.fields.token}
      </label>
      <Input
        id="admin-manual-copy-dialog-field"
        ref={manualCopyDialogFieldRef}
        className="manual-copy-bubble-field"
        readOnly
        value={manualCopyDialog?.value ?? ''}
        onFocus={(event) => selectAllReadonlyText(event.currentTarget)}
        onClick={(event) => selectAllReadonlyText(event.currentTarget)}
      />
    </div>
    <DialogFooter className="modal-action">
      <Button type="button" onClick={() => setManualCopyDialog(null)}>
        {manualCopyText.close}
      </Button>
    </DialogFooter>
  </DialogContent>
</Dialog>
      <ManualCopyBubble
        open={manualCopyBubble != null}
        anchorEl={manualCopyBubble?.anchorEl ?? null}
        title={manualCopyBubble?.title ?? manualCopyText.title}
        description={manualCopyBubble?.description ?? manualCopyText.description}
        fieldLabel={manualCopyBubble?.fieldLabel ?? manualCopyText.fields.token}
        value={manualCopyBubble?.value ?? ''}
        multiline={manualCopyBubble?.multiline ?? false}
        closeLabel={manualCopyText.close}
        onClose={() => setManualCopyBubble(null)}
      />
    </>
  )
}

interface LogRowProps {
  log: RequestLog
  expanded: boolean
  onToggle: (id: number) => void
  strings: AdminTranslations
  onOpenKey?: (id: string) => void
  onOpenToken?: (id: string) => void
}

function LogRow({ log, expanded, onToggle, strings, onOpenKey, onOpenToken }: LogRowProps): JSX.Element {
  const requestButtonLabel = expanded ? strings.logs.toggles.hide : strings.logs.toggles.show
  const tokenId = log.auth_token_id ?? null
  const timeLabel = formatClockTime(log.created_at)
  const timeDetail =
    log.created_at != null
      ? `${formatTimestampWithMs(log.created_at)} · ${formatRelativeTime(log.created_at)}`
      : strings.logs.errors.none

  return (
    <>
      <tr>
        <td>
          <div className="log-time-cell">
            <button
              type="button"
              className="log-time-trigger"
              aria-label={timeDetail}
            >
              <span className="log-time-main">{timeLabel}</span>
            </button>
            <div className="log-time-bubble">{timeDetail}</div>
          </div>
        </td>
        <td>
          <a
            href={keyDetailPath(log.key_id)}
            className="log-key-pill"
            title={strings.keys.actions.details}
            aria-label={strings.keys.actions.details}
            onClick={(event) => {
              if (!onOpenKey) return
              if (event.defaultPrevented) return
              if (event.button !== 0 || event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) return
              event.preventDefault()
              onOpenKey(log.key_id)
            }}
          >
            <code>{log.key_id}</code>
          </a>
        </td>
        <td>
          {tokenId ? (
            <a
              href={tokenDetailPath(tokenId)}
              className="link-button log-token-link"
              title={strings.tokens.table.id}
              aria-label={strings.tokens.table.id}
              onClick={(event) => {
                if (!onOpenToken) return
                if (event.defaultPrevented) return
                if (event.button !== 0 || event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) return
                event.preventDefault()
                onOpenToken(tokenId)
              }}
            >
              <code>{tokenId}</code>
            </a>
          ) : (
            '—'
          )}
        </td>
        <td>{log.http_status ?? '—'}</td>
        <td>{log.mcp_status ?? '—'}</td>
        <td>
          <button
            type="button"
            className={`log-result-button${expanded ? ' log-result-button-active' : ''}`}
            onClick={() => onToggle(log.id)}
            aria-expanded={expanded}
            aria-controls={`log-details-${log.id}`}
            aria-label={requestButtonLabel}
            title={requestButtonLabel}
          >
            <StatusBadge tone={statusTone(log.result_status)}>
              {statusLabel(log.result_status, strings)}
            </StatusBadge>
            <Icon icon={expanded ? 'mdi:chevron-up' : 'mdi:chevron-down'} width={18} height={18} className="log-result-icon" />
          </button>
        </td>
        <td>{formatErrorMessage(log, strings.logs.errors)}</td>
      </tr>
      {expanded && (
        <tr className="log-details-row">
          <td colSpan={7} id={`log-details-${log.id}`}>
            <LogDetails log={log} strings={strings} />
          </td>
        </tr>
      )}
    </>
  )
}

function LogDetails({ log, strings }: { log: RequestLog; strings: AdminTranslations }): JSX.Element {
  const query = log.query ? `?${log.query}` : ''
  const requestLine = `${log.method} ${log.path}${query}`
  const forwarded = (log.forwarded_headers ?? []).filter((value) => value.trim().length > 0)
  const dropped = (log.dropped_headers ?? []).filter((value) => value.trim().length > 0)
  const httpLabel = `${strings.logs.table.httpStatus}: ${log.http_status ?? strings.logs.errors.none}`
  const mcpLabel = `${strings.logs.table.mcpStatus}: ${log.mcp_status ?? strings.logs.errors.none}`
  const requestBody = log.request_body ?? strings.logDetails.noBody
  const responseBody = log.response_body ?? strings.logDetails.noBody

  return (
    <div className="log-details-panel">
      <div className="log-details-summary">
        <div>
          <span className="log-details-label">{strings.logDetails.request}</span>
          <span className="log-details-value">{requestLine}</span>
        </div>
        <div>
          <span className="log-details-label">{strings.logDetails.response}</span>
          <span className="log-details-value">
            {httpLabel}
            {` · ${mcpLabel}`}
          </span>
        </div>
        <div>
          <span className="log-details-label">{strings.logDetails.outcome}</span>
          <span className="log-details-value">{statusLabel(log.result_status, strings)}</span>
        </div>
      </div>
      <div className="log-details-body">
        <div className="log-details-section">
          <header>{strings.logDetails.requestBody}</header>
          <pre>{requestBody}</pre>
        </div>
        <div className="log-details-section">
          <header>{strings.logDetails.responseBody}</header>
          <pre>{responseBody}</pre>
        </div>
      </div>
      {(forwarded.length > 0 || dropped.length > 0) && (
        <div className="log-details-headers">
          {forwarded.length > 0 && (
            <div className="log-details-section">
              <header>{strings.logDetails.forwardedHeaders}</header>
              <ul>
                {forwarded.map((header, index) => (
                  <li key={`forwarded-${index}-${header}`}>{header}</li>
                ))}
              </ul>
            </div>
          )}
          {dropped.length > 0 && (
            <div className="log-details-section">
              <header>{strings.logDetails.droppedHeaders}</header>
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

function KeyDetails({ id, onBack }: { id: string; onBack: () => void }): JSX.Element {
  const translations = useTranslate()
  const adminStrings = translations.admin
  const keyStrings = adminStrings.keys
  const keyDetailsStrings = adminStrings.keyDetails
  const logsTableStrings = adminStrings.logs.table
  const loadingStateStrings = adminStrings.loadingStates
  const [detail, setDetail] = useState<ApiKeyStats | null>(null)
  const [period, setPeriod] = useState<'day' | 'week' | 'month'>('month')
  const [startDate, setStartDate] = useState<string>(() => new Date().toISOString().slice(0, 10))
  const [summary, setSummary] = useState<KeySummary | null>(null)
  const [logs, setLogs] = useState<RequestLog[]>([])
  const [detailLoadState, setDetailLoadState] = useState<QueryLoadState>('initial_loading')
  const [error, setError] = useState<string | null>(null)
  const [syncState, setSyncState] = useState<'idle' | 'syncing' | 'success'>('idle')
  const [quarantineState, setQuarantineState] = useState<'idle' | 'clearing'>('idle')
  const [quarantineDetailExpanded, setQuarantineDetailExpanded] = useState(false)
  const syncInFlightRef = useRef(false)
  const syncFeedbackTimerRef = useRef<number | null>(null)
  const loadAbortRef = useRef<AbortController | null>(null)
  const queryKeyRef = useRef<string | null>(null)
  const queryKey = `${id}:${period}:${startDate}`
  const quarantineDetailId = `key-quarantine-detail-${id}`

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

  const load = useCallback(async (reason: 'initial' | 'switch' | 'refresh' = 'refresh') => {
    loadAbortRef.current?.abort()
    const controller = new AbortController()
    loadAbortRef.current = controller
    try {
      setDetailLoadState(
        reason === 'refresh'
          ? getRefreshingLoadState(queryKeyRef.current != null)
          : getBlockingLoadState(queryKeyRef.current != null),
      )
      setError(null)
      if (reason !== 'refresh') {
        setDetail(null)
        setSummary(null)
        setLogs([])
      }
      const since = computeSince()
      const [s, ls, d] = await Promise.all([
        fetchKeyMetrics(id, period, since, controller.signal),
        fetchKeyLogs(id, 50, since, controller.signal),
        fetchApiKeyDetail(id, controller.signal).catch(() => null),
      ])
      if (controller.signal.aborted) return
      setSummary(s)
      setLogs(ls)
      setDetail(d)
      setDetailLoadState('ready')
      queryKeyRef.current = queryKey
    } catch (err) {
      if ((err as Error).name === 'AbortError') return
      console.error(err)
      setError(err instanceof Error ? err.message : adminStrings.errors.loadKeyDetails)
      setDetailLoadState('error')
    }
  }, [adminStrings.errors.loadKeyDetails, computeSince, id, period, queryKey])

  useEffect(() => {
    const reason = queryKeyRef.current == null ? 'initial' : queryKeyRef.current === queryKey ? 'refresh' : 'switch'
    void load(reason)
    return () => {
      loadAbortRef.current?.abort()
    }
  }, [load, queryKey])

  useEffect(() => () => {
    if (syncFeedbackTimerRef.current != null) {
      window.clearTimeout(syncFeedbackTimerRef.current)
    }
  }, [])

  useEffect(() => {
    setQuarantineDetailExpanded(false)
  }, [id])

  const syncUsage = useCallback(async () => {
    if (syncInFlightRef.current) return
    syncInFlightRef.current = true
    try {
      setSyncState('syncing')
      setError(null)
      await syncApiKeyUsage(id)
      await load('refresh')
      setSyncState('success')
      if (syncFeedbackTimerRef.current != null) {
        window.clearTimeout(syncFeedbackTimerRef.current)
      }
      syncFeedbackTimerRef.current = window.setTimeout(() => {
        setSyncState('idle')
        syncFeedbackTimerRef.current = null
      }, 2500)
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : adminStrings.errors.syncUsage)
      setSyncState('idle')
    } finally {
      syncInFlightRef.current = false
    }
  }, [adminStrings.errors.syncUsage, id, load])

  const clearQuarantine = useCallback(async () => {
    if (quarantineState === 'clearing') return
    try {
      setQuarantineState('clearing')
      setError(null)
      await clearApiKeyQuarantine(id)
      await load('refresh')
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : adminStrings.errors.clearQuarantine)
    } finally {
      setQuarantineState('idle')
    }
  }, [adminStrings.errors.clearQuarantine, id, load, quarantineState])

  const metricCards = useMemo(() => {
    if (!summary) return []
    const total = summary.total_requests
    const lastActivitySubtitle = summary.last_activity
      ? `${keyDetailsStrings.metrics.lastActivityPrefix} ${formatTimestamp(summary.last_activity)}`
      : keyDetailsStrings.metrics.noActivity
    return [
      { id: 'total', label: keyDetailsStrings.metrics.total, value: formatNumber(summary.total_requests), subtitle: lastActivitySubtitle },
      { id: 'success', label: keyDetailsStrings.metrics.success, value: formatNumber(summary.success_count), subtitle: formatPercent(summary.success_count, total) },
      { id: 'errors', label: keyDetailsStrings.metrics.errors, value: formatNumber(summary.error_count), subtitle: formatPercent(summary.error_count, total) },
      { id: 'quota', label: keyDetailsStrings.metrics.quota, value: formatNumber(summary.quota_exhausted_count), subtitle: formatPercent(summary.quota_exhausted_count, total) },
    ]
  }, [summary, keyDetailsStrings])
  const detailBlocking = isBlockingLoadState(detailLoadState)
  const detailRefreshing = isRefreshingLoadState(detailLoadState)
  const detailLoadingLabel = detailRefreshing ? loadingStateStrings.refreshing : loadingStateStrings.switching
  const quarantineRawDetail = detail?.quarantine?.reasonDetail?.trim() ?? ''
  const hasQuarantineRawDetail = quarantineRawDetail.length > 0

  return (
    <div className="admin-detail-stack">
      <section className="surface app-header">
        <div className="title-group">
          <h1>{keyDetailsStrings.title}</h1>
          <p>
            {keyDetailsStrings.descriptionPrefix}{' '}
            <code>{id}</code>
          </p>
        </div>
        <div className="controls">
          <ThemeToggle />
          <AdminReturnToConsoleLink
            label={adminStrings.header.returnToConsole}
            href={ADMIN_USER_CONSOLE_HREF}
            className="admin-return-link--detail"
          />
          <Button
            type="button"
            variant={syncState === 'success' ? 'success' : 'default'}
            onClick={() => void syncUsage()}
            disabled={syncState === 'syncing'}
            aria-busy={syncState === 'syncing'}
          >
            <Icon
              icon={syncState === 'syncing' ? 'mdi:loading' : syncState === 'success' ? 'mdi:check-bold' : 'mdi:refresh'}
              width={18}
              height={18}
              className={syncState === 'syncing' ? 'icon-spin' : undefined}
            />
            {syncState === 'syncing'
              ? keyDetailsStrings.syncing
              : syncState === 'success'
                ? keyDetailsStrings.syncSuccess
                : keyDetailsStrings.syncAction}
          </Button>
          <Button type="button" variant="ghost" onClick={onBack}>
            <Icon icon="mdi:arrow-left" width={18} height={18} />
            {keyDetailsStrings.back}
          </Button>
        </div>
      </section>

      {error && <div className="surface error-banner" style={{ marginTop: 8, marginBottom: 0 }}>{error}</div>}

      {detail?.quarantine && (
        <section className="surface panel">
          <div className="panel-header">
            <div>
              <h2>{keyDetailsStrings.quarantine.title}</h2>
              <p className="panel-description">{keyDetailsStrings.quarantine.description}</p>
            </div>
            <Button
              type="button"
              variant="warning"
              onClick={() => void clearQuarantine()}
              disabled={quarantineState === 'clearing'}
              aria-busy={quarantineState === 'clearing'}
            >
              <Icon
                icon={quarantineState === 'clearing' ? 'mdi:loading' : 'mdi:shield-check-outline'}
                width={18}
                height={18}
                className={quarantineState === 'clearing' ? 'icon-spin' : undefined}
              />
              {quarantineState === 'clearing'
                ? keyDetailsStrings.quarantine.clearing
                : keyDetailsStrings.quarantine.clearAction}
            </Button>
          </div>
          <div className="admin-mobile-kv">
            <span>{keyDetailsStrings.quarantine.source}</span>
            <strong>{detail.quarantine.source}</strong>
          </div>
          <div className="admin-mobile-kv">
            <span>{keyDetailsStrings.quarantine.reason}</span>
            <strong>{detail.quarantine.reasonSummary || keyStrings.quarantine.noReason}</strong>
          </div>
          <div className="admin-mobile-kv">
            <span>{keyDetailsStrings.quarantine.createdAt}</span>
            <strong>{formatTimestamp(detail.quarantine.createdAt)}</strong>
          </div>
          {hasQuarantineRawDetail && (
            <div className="quarantine-detail-block">
              <div className="quarantine-detail-header">
                <div className="panel-description">{keyDetailsStrings.quarantine.detail}</div>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="quarantine-detail-toggle"
                  aria-expanded={quarantineDetailExpanded}
                  aria-controls={quarantineDetailId}
                  onClick={() => setQuarantineDetailExpanded((current) => !current)}
                >
                  <Icon
                    icon={quarantineDetailExpanded ? 'mdi:chevron-up' : 'mdi:chevron-down'}
                    width={18}
                    height={18}
                    aria-hidden="true"
                  />
                  {quarantineDetailExpanded
                    ? keyDetailsStrings.quarantine.hideDetail
                    : keyDetailsStrings.quarantine.showDetail}
                </Button>
              </div>
              <pre
                id={quarantineDetailId}
                className="log-details-pre"
                hidden={!quarantineDetailExpanded}
                aria-hidden={!quarantineDetailExpanded}
              >
                {quarantineRawDetail}
              </pre>
            </div>
          )}
        </section>
      )}

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>Quota</h2>
            <p className="panel-description">Tavily Usage for this key</p>
          </div>
        </div>
        <AdminLoadingRegion
          loadState={detailLoadState}
          loadingLabel={detailLoadingLabel}
          minHeight={180}
        >
          <section className="metrics-grid">
            {!detail ? (
              <div className="empty-state alert" style={{ gridColumn: '1 / -1' }}>{keyDetailsStrings.loading}</div>
            ) : (
              (() => {
                const limit = detail?.quota_limit ?? null
                const remaining = detail?.quota_remaining ?? null
                const used = (limit != null && remaining != null) ? Math.max(limit - remaining, 0) : null
                const percent = (limit && remaining != null && limit > 0) ? formatPercent(remaining, limit) : '—'
                return [
                  { id: 'used', label: 'Used', value: used != null ? formatNumber(used) : '—', subtitle: limit != null ? `of ${formatNumber(limit)}` : '—' },
                  { id: 'remaining', label: 'Remaining', value: remaining != null ? formatNumber(remaining) : '—', subtitle: percent },
                  { id: 'synced', label: 'Synced', value: detail?.quota_synced_at ? formatTimestamp(detail.quota_synced_at) : '—', subtitle: '' },
                ].map((m) => (
                  <div key={m.id} className="metric-card">
                    <h3>{m.label}</h3>
                    <div className="metric-value">{m.value}</div>
                    <div className="metric-subtitle">{m.subtitle}</div>
                  </div>
                ))
              })()
            )}
          </section>
        </AdminLoadingRegion>
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{keyDetailsStrings.usageTitle}</h2>
            <p className="panel-description">{keyDetailsStrings.usageDescription}</p>
          </div>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
            <Select value={period} onValueChange={(value) => setPeriod(value as 'day' | 'week' | 'month')} disabled={detailBlocking}>
              <SelectTrigger className="w-[132px]" aria-label={keyDetailsStrings.usageTitle} disabled={detailBlocking}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent align="end">
                <SelectItem value="day">{keyDetailsStrings.periodOptions.day}</SelectItem>
                <SelectItem value="week">{keyDetailsStrings.periodOptions.week}</SelectItem>
                <SelectItem value="month">{keyDetailsStrings.periodOptions.month}</SelectItem>
              </SelectContent>
            </Select>
            <Input
              type="date"
              name="key-usage-start-date"
              value={startDate}
              onChange={(e) => setStartDate(e.target.value)}
              className="w-[176px]"
              disabled={detailBlocking}
            />
            <Button type="button" onClick={() => void load('refresh')} disabled={detailBlocking}>
              {keyDetailsStrings.apply}
            </Button>
          </div>
        </div>
        <AdminLoadingRegion
          loadState={detailLoadState}
          loadingLabel={detailLoadingLabel}
          minHeight={180}
        >
          <section className="metrics-grid">
            {!summary ? (
              <div className="empty-state alert" style={{ gridColumn: '1 / -1' }}>{keyDetailsStrings.loading}</div>
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
        </AdminLoadingRegion>
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{keyDetailsStrings.logsTitle}</h2>
            <p className="panel-description">{keyDetailsStrings.logsDescription}</p>
          </div>
        </div>
        <AdminLoadingRegion
          className="table-wrapper admin-responsive-up"
          loadState={detailLoadState}
          loadingLabel={detailLoadingLabel}
          minHeight={260}
        >
          {logs.length === 0 ? (
            <div className="empty-state alert">{keyDetailsStrings.logsEmpty}</div>
          ) : (
            <Table className="admin-logs-table">
              <thead>
                <tr>
                  <th>{logsTableStrings.time}</th>
                  <th>{logsTableStrings.httpStatus}</th>
                  <th>{logsTableStrings.mcpStatus}</th>
                  <th>{logsTableStrings.result}</th>
                  <th>{logsTableStrings.error}</th>
                </tr>
              </thead>
              <tbody>
                {logs.map((log) => (
                  <tr key={log.id}>
                    <td>{formatTimestamp(log.created_at)}</td>
                    <td>{log.http_status ?? '—'}</td>
                    <td>{log.mcp_status ?? '—'}</td>
                    <td>
                      <StatusBadge tone={statusTone(log.result_status)}>
                        {statusLabel(log.result_status, adminStrings)}
                      </StatusBadge>
                    </td>
                    <td>{formatErrorMessage(log, adminStrings.logs.errors)}</td>
                  </tr>
                ))}
              </tbody>
            </Table>
          )}
        </AdminLoadingRegion>
        <AdminLoadingRegion
          className="admin-mobile-list admin-responsive-down"
          loadState={detailLoadState}
          loadingLabel={detailLoadingLabel}
          minHeight={220}
        >
          {logs.length === 0 ? (
            <div className="empty-state alert">{keyDetailsStrings.logsEmpty}</div>
          ) : (
            logs.map((log) => (
              <article key={log.id} className="admin-mobile-card">
                <div className="admin-mobile-kv">
                  <span>{logsTableStrings.time}</span>
                  <strong>{formatTimestamp(log.created_at)}</strong>
                </div>
                <div className="admin-mobile-kv">
                  <span>{logsTableStrings.httpStatus}</span>
                  <strong>{log.http_status ?? '—'}</strong>
                </div>
                <div className="admin-mobile-kv">
                  <span>{logsTableStrings.mcpStatus}</span>
                  <strong>{log.mcp_status ?? '—'}</strong>
                </div>
                <div className="admin-mobile-kv">
                  <span>{logsTableStrings.result}</span>
                  <StatusBadge tone={statusTone(log.result_status)}>
                    {statusLabel(log.result_status, adminStrings)}
                  </StatusBadge>
                </div>
                <div className="admin-mobile-kv">
                  <span>{logsTableStrings.error}</span>
                  <strong>{formatErrorMessage(log, adminStrings.logs.errors)}</strong>
                </div>
              </article>
            ))
          )}
        </AdminLoadingRegion>
      </section>
    </div>
  )
}

export default AdminDashboard
