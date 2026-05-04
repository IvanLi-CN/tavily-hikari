import { Icon } from '../lib/icons'
import { StatusBadge, type StatusTone } from '../components/StatusBadge'
import type { RecentRequestsOutcomeFilter } from '../components/AdminRecentRequestsPanel'
import AdminTablePagination from '../components/AdminTablePagination'
import AdminLoadingRegion from '../components/AdminLoadingRegion'
import AdminTableShell from '../components/AdminTableShell'
import JobKeyLink from '../components/JobKeyLink'
import ManualCopyBubble from '../components/ManualCopyBubble'
import QuotaRangeField from '../components/QuotaRangeField'
import {
  Suspense,
  lazy,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react'
import { createPortal } from 'react-dom'
import ThemeToggle from '../components/ThemeToggle'
import AdminReturnToConsoleLink from '../components/AdminReturnToConsoleLink'
import AdminPanelHeader from '../components/AdminPanelHeader'
import AdminCompactIntro from '../components/AdminCompactIntro'
import LanguageSwitcher from '../components/LanguageSwitcher'
import { AdminSidebarUtilityCard, AdminSidebarUtilityStack } from '../components/AdminSidebarUtility'
import { Button } from '../components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../components/ui/dialog'
import {
  Drawer,
  DrawerContent,
} from '../components/ui/drawer'
import { Input } from '../components/ui/input'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '../components/ui/dropdown-menu'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../components/ui/select'
import { Card } from '../components/ui/card'
import { Badge } from '../components/ui/badge'
import { Switch } from '../components/ui/switch'
import { Table } from '../components/ui/table'
import { Textarea } from '../components/ui/textarea'
import { AnchoredInfoDisclosure } from '../components/ui/anchored-info-disclosure'
import { Tooltip, TooltipContent, TooltipTrigger } from '../components/ui/tooltip'
import { ArrowDown, ArrowUp, ArrowUpDown, ChartColumnIncreasing } from 'lucide-react'
import AdminShell, { AdminShellSidebarUtility, type AdminNavItem, type AdminNavTarget } from './AdminShell'
import AdminOverlayHost from './AdminOverlayHost'
import DashboardOverview, { type DashboardQuotaChargeCardData } from './DashboardOverview'
import { AnchoredApiKeyBulkSyncProgressBubble } from './ApiKeyBulkSyncProgressBubble'
import {
  createDashboardMonthMetrics,
  createDashboardTodayMetrics,
} from './dashboardTodayMetrics'
import {
  buildAdminJobFilterOptions,
  emptyAdminJobGroupCounts,
  summarizeAdminJobFilter,
} from './jobFilters'
import { fetchAllMonthlyBrokenKeyItems } from './fetchAllMonthlyBrokenKeyItems'
import type {
  ForwardProxyDraft,
  ForwardProxyValidationEntry,
} from './ForwardProxySettingsModule'
import {
  type QueryLoadState,
  getBlockingLoadState,
  getRefreshingLoadState,
  isBlockingLoadState,
  isRefreshingLoadState,
} from './queryLoadState'
import {
  createApiKeyBulkSyncProgressState,
  markApiKeyBulkSyncRefreshDone,
  markApiKeyBulkSyncRefreshError,
  markApiKeyBulkSyncRefreshRunning,
  type ApiKeyBulkSyncProgressState,
  updateApiKeyBulkSyncProgressState,
} from './apiKeyBulkSyncProgress'
import { retainVisibleApiKeySelection } from './apiKeySelection'
import { UserDetailQuotaBreakdown } from './UserDetailQuotaBreakdown'
import {
  createApiKeyBulkSyncBubblePinnedPosition,
  type ApiKeyBulkSyncBubblePinnedPosition,
} from './apiKeyBulkSyncBubblePosition'
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
} from './quotaSlider'
import { formatRequestRateScope, formatRequestRateSummary, resolveRequestRate } from '../requestRate'
import {
  type AdminUsersCollectionView,
  type AdminModuleId,
  type AdminPathRoute,
  alertsPath,
  buildAdminKeysPath,
  isSameAdminRoute,
  keyDetailPath,
  modulePath,
  parseAdminPath,
  buildAdminUsersPath,
  tokenDetailPath,
  unboundTokenUsagePath,
  userDetailPath,
  userTagCreatePath,
  userTagEditPath,
  userTagsPath,
  userUsagePath,
} from './routes'
import {
  buildRequestLogsCatalogPlan,
  buildRequestLogsListPlan,
  formatRequestLogsDescription,
  formatRequestLogsPaginationSummary,
} from './requestLogsUi'
import { useLanguage, useTranslate, type AdminTranslations } from '../i18n'
import { extractApiKeyImportEntriesFromText } from '../lib/api-key-extract'
import { ADMIN_USER_CONSOLE_HREF } from '../lib/adminUserConsoleEntry'
import {
  copyText,
  isCopyIntentKey,
  selectAllReadonlyText,
  shouldPrewarmSecretCopy,
  type CopyTextOptions,
} from '../lib/clipboard'
import {
  operationalClassGuidance,
  operationalClassLabel,
  operationalClassTone,
} from '../lib/logOperationalClass'
import {
  fetchApiKeys,
  applyApiKeyBulkAction,
  syncApiKeyBulkUsageWithProgress,
  type ApiKeyBulkAction,
  type RecentAlertsSummary,
  type PaginatedApiKeys,
  fetchApiKeySecret,
  addApiKeysBatch,
  type AddApiKeysBatchItem,
  type AddApiKeysBatchResponse,
  validateApiKeys,
  type ValidateKeyResult,
  deleteApiKey,
  setKeyStatus,
  clearApiKeyQuarantine,
  fetchProfile,
  fetchRequestLogs,
  fetchRequestLogsCatalog,
  fetchRequestLogsList,
  fetchRequestLogDetails,
  fetchDashboardOverview,
  fetchSummary,
  fetchVersion,
  type ApiKeyStats,
  type DashboardSnapshotEvent,
  type DashboardHourlyRequestWindow,
  type DashboardSiteStatusSnapshot,
  type DashboardTokenCoverage,
  type DashboardTrendBuckets,
  type Profile,
  type RequestLog,
  type RequestLogsCatalog,
  type RequestLogFacets,
  type RequestLogsListPage,
  type Summary,
  type SummaryWindowsResponse,
  fetchTokens,
  type AuthToken,
  fetchTokenSecret,
  createToken,
  deleteToken,
  setTokenEnabled,
  updateTokenNote,
  createTokensBatch,
  type Paginated,
  fetchKeyMetrics,
  fetchKeyLogsCatalog,
  fetchKeyLogsList,
  fetchKeyLogDetails,
  fetchKeyStickyNodes,
  fetchKeyStickyUsers,
  type KeySummary,
  type JobGroup,
  type JobGroupCounts,
  type JobLogView,
  fetchApiKeyDetail,
  syncApiKeyUsage,
  fetchJobs,
  fetchTokenGroups,
  type AdminUsersSortField,
  type TokenGroup,
  fetchAdminUsers,
  fetchAdminUserBrokenKeys,
  fetchAdminUnboundTokenUsage,
  fetchAdminUserDetail,
  fetchAdminUserUsageSeries,
  fetchAdminRegistrationSettings,
  fetchTokenBrokenKeys,
  updateAdminUserQuota,
  updateAdminRegistrationSettings,
  fetchAdminUserTags,
  createAdminUserTag,
  updateAdminUserTag,
  deleteAdminUserTag,
  bindAdminUserTag,
  unbindAdminUserTag,
  type AdminUserSummary,
  type AdminUnboundTokenUsageSortField,
  type AdminUnboundTokenUsageSummary,
  type AdminUserDetail,
  type AdminUserTag,
  type AdminUserTagBinding,
  type SortDirection,
  type MonthlyBrokenKeyDetail,
  type ForwardProxySettings,
  type SystemSettings,
  type ForwardProxyStatsResponse,
  type ForwardProxyValidationKind,
  type SummaryWindowMetrics,
  type StickyNode,
  type StickyUserRow,
  type ForwardProxyProgressEvent,
  fetchForwardProxySettings,
  fetchSystemSettings,
  fetchForwardProxyStats,
  revalidateForwardProxyWithProgress,
  updateSystemSettings,
  updateForwardProxySettingsWithProgress,
  validateForwardProxyCandidateWithProgress,
} from '../api'
import {
  createDialogProgressState,
  type ForwardProxyDialogProgressState,
  updateDialogProgressState,
} from './forwardProxyDialogProgress'
import {
  buildRequestKindQuickFilterSelection,
  defaultTokenLogRequestKindQuickFilters,
  hasActiveRequestKindQuickFilters,
  resolveEffectiveRequestKindSelection,
  resolveManualRequestKindQuickFilters,
  toggleRequestKindSelection,
  type TokenLogRequestKindOption,
  type TokenLogRequestKindQuickBilling,
  type TokenLogRequestKindQuickProtocol,
} from '../tokenLogRequestKinds'
import { finalizeForwardProxyRevalidate } from './forwardProxyRevalidate'

const LazyAdminRecentRequestsPanel = lazy(() => import('../components/AdminRecentRequestsPanel'))
const LazyApiKeysValidationDialog = lazy(async () =>
  import('../components/ApiKeysValidationDialog').then((module) => ({
    default: module.ApiKeysValidationDialog,
  })),
)
const LazyTokenDetail = lazy(() => import('../pages/TokenDetail'))
const LazyForwardProxySettingsModule = lazy(() => import('./ForwardProxySettingsModule'))
const LazyKeyStickyPanels = lazy(() => import('./KeyStickyPanels'))
const LazyAlertsCenter = lazy(() => import('./AlertsCenter'))
const LazySystemSettingsModule = lazy(() => import('./SystemSettingsModule'))
const LazyUserDetailSharedUsagePanel = lazy(async () =>
  import('./UserDetailSharedUsagePanel').then((module) => ({
    default: module.UserDetailSharedUsagePanel,
  })),
)
const LazyUserDetailTokenTable = lazy(async () =>
  import('./UserDetailTokenTable').then((module) => ({
    default: module.UserDetailTokenTable,
  })),
)

function AdminLazyBoundary({
  children,
  loadingLabel,
  minHeight = 220,
}: {
  children: ReactNode
  loadingLabel: ReactNode
  minHeight?: number | string
}): JSX.Element {
  return (
    <Suspense
      fallback={
        <AdminLoadingRegion
          loadState="initial_loading"
          loadingLabel={loadingLabel}
          minHeight={minHeight}
        />
      }
    >
      {children}
    </Suspense>
  )
}

const REFRESH_INTERVAL_MS = 30_000
const LOGS_PER_PAGE = 20
const DASHBOARD_RECENT_LOGS_PER_PAGE = 64
const DASHBOARD_RECENT_JOBS_PER_PAGE = 20
const DASHBOARD_HOURLY_CHART_PERSISTENCE_KEY = 'admin.dashboard.hourly-request-charts.v1'
const DEFAULT_KEYS_PER_PAGE = 20
const USERS_PER_PAGE = 20
// Auto-collapse behavior for the API keys batch overlay (empty textarea only):
// The user wants "delay + close animation" to total 500ms.
const KEYS_BATCH_CLOSE_ANIMATION_MS = 200
const KEYS_BATCH_AUTO_COLLAPSE_TOTAL_MS = 500
const KEYS_BATCH_AUTO_COLLAPSE_DELAY_MS = Math.max(0, KEYS_BATCH_AUTO_COLLAPSE_TOTAL_MS - KEYS_BATCH_CLOSE_ANIMATION_MS)
const API_KEYS_IMPORT_CHUNK_SIZE = 1000
const USER_TAG_DISPLAY_LIMIT = 3
const MONTHLY_BROKEN_DRAWER_PAGE_SIZE = 100
const NEW_USER_TAG_CARD_ID = '__new__'
const DASHBOARD_TREND_WINDOW_SIZE = 8
const ADMIN_USERS_DEFAULT_SORT_FIELD: AdminUsersSortField = 'lastLoginAt'
const ADMIN_USERS_DEFAULT_SORT_ORDER: SortDirection = 'desc'
const ADMIN_USERS_SORT_FIELDS: readonly AdminUsersSortField[] = [
  'hourlyAnyUsed',
  'quotaHourlyUsed',
  'quotaDailyUsed',
  'quotaMonthlyUsed',
  'dailySuccessRate',
  'monthlySuccessRate',
  'monthlyBrokenCount',
  'lastActivity',
  'lastLoginAt',
]
const ADMIN_USERS_OVERVIEW_SORT_FIELDS = new Set<AdminUsersSortField>([
  'quotaDailyUsed',
  'quotaMonthlyUsed',
  'lastActivity',
  'lastLoginAt',
])

function createEmptyDashboardTrend(): DashboardTrendBuckets {
  return {
    request: new Array(DASHBOARD_TREND_WINDOW_SIZE).fill(0),
    error: new Array(DASHBOARD_TREND_WINDOW_SIZE).fill(0),
  }
}

function createEmptyDashboardHourlyRequestWindow(): DashboardHourlyRequestWindow {
  return {
    bucketSeconds: 3600,
    visibleBuckets: 25,
    retainedBuckets: 49,
    buckets: [],
  }
}
const ADMIN_UNBOUND_TOKEN_USAGE_DEFAULT_SORT_FIELD: AdminUnboundTokenUsageSortField = 'lastUsedAt'
const ADMIN_UNBOUND_TOKEN_USAGE_DEFAULT_SORT_ORDER: SortDirection = 'desc'
const ADMIN_UNBOUND_TOKEN_USAGE_SORT_FIELDS: readonly AdminUnboundTokenUsageSortField[] = [
  'hourlyAnyUsed',
  'quotaHourlyUsed',
  'quotaDailyUsed',
  'quotaMonthlyUsed',
  'monthlyBrokenCount',
  'dailySuccessRate',
  'monthlySuccessRate',
  'lastUsedAt',
]

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

type DashboardSiteStatusState = DashboardSiteStatusSnapshot

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

function splitMultilineEntries(value: string): string[] {
  const seen = new Set<string>()
  const entries: string[] = []
  for (const rawLine of value.split(/\r?\n/)) {
    const normalized = rawLine.trim()
    if (!normalized || seen.has(normalized)) continue
    seen.add(normalized)
    entries.push(normalized)
  }
  return entries
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

type AdminTableStackedValue = {
  primary: string
  secondary?: string | null
  primaryClassName?: string | null
}

function quotaUsagePrimaryClassName(used: number, limit: number): string | null {
  const normalizedUsed = Math.max(0, used)
  const normalizedLimit = Math.max(0, limit)

  if (normalizedLimit <= 0) {
    return normalizedUsed > 0 ? 'admin-table-value-primary-danger' : null
  }

  const usageRatio = normalizedUsed / normalizedLimit
  if (usageRatio >= 1) return 'admin-table-value-primary-danger'
  if (usageRatio > 0.9) return 'admin-table-value-primary-warning'
  return null
}

function formatQuotaStackValue(used: number, limit: number): AdminTableStackedValue {
  return {
    primary: formatNumber(Math.max(0, used)),
    secondary: formatQuotaLimitValue(limit),
    primaryClassName: quotaUsagePrimaryClassName(used, limit),
  }
}

function formatSuccessRateStackValue(
  success: number,
  failure: number,
  language: 'en' | 'zh',
): AdminTableStackedValue {
  const total = success + failure
  return {
    primary: formatPercent(success, total),
    secondary: language === 'zh' ? `失败 ${formatNumber(failure)}` : `Fail ${formatNumber(failure)}`,
  }
}

function formatCompactSuccessRateValue(success: number, failure: number, language: 'en' | 'zh'): string {
  const total = success + failure
  const rate = formatPercent(success, total)
  const failureLabel = language === 'zh' ? '失败' : 'Fail'
  return `${rate} · ${failureLabel} ${formatNumber(failure)}`
}

function formatStackedTimestamp(
  value: number | null,
  language: 'en' | 'zh',
): AdminTableStackedValue {
  if (!value) {
    return { primary: '—' }
  }
  return {
    primary: formatDateOnly(value, language),
    secondary: formatClockTime(value),
  }
}

function monthlyBrokenPrimaryClassName(count: number, limit: number): string | null {
  if (limit <= 0) {
    return count > 0 ? 'admin-table-value-primary-danger' : null
  }
  if (count >= limit) return 'admin-table-value-primary-danger'
  if (count > 0) return 'admin-table-value-primary-warning'
  return null
}

function formatMonthlyBrokenStackValue(count: number, limit: number): AdminTableStackedValue {
  return {
    primary: formatNumber(Math.max(0, count)),
    secondary: formatQuotaLimitValue(limit),
    primaryClassName: monthlyBrokenPrimaryClassName(count, limit),
  }
}

function MonthlyBrokenCountTrigger({
  count,
  onOpen,
  ariaLabel,
  className,
}: {
  count: number
  onOpen?: (() => void) | null
  ariaLabel: string
  className?: string | null
}): JSX.Element {
  const primary = formatNumber(Math.max(0, count))
  if (count <= 0 || !onOpen) {
    return <span className={`admin-table-value-primary${className ? ` ${className}` : ''}`}>{primary}</span>
  }
  return (
    <button
      type="button"
      className={`link-button admin-table-value-link${className ? ` ${className}` : ''}`}
      onClick={onOpen}
      aria-label={ariaLabel}
    >
      {primary}
    </button>
  )
}

function formatMonthlyBrokenRelatedUsers(
  users: MonthlyBrokenKeyDetail['relatedUsers'],
  emptyLabel: string,
): string {
  if (users.length === 0) return emptyLabel
  return users.map((user) => user.displayName || user.username || user.userId).join(', ')
}

function formatMonthlyBrokenBreaker(
  item: MonthlyBrokenKeyDetail,
  strings: {
    breakerSystem: string
    breakerUnknown: string
  },
): string {
  if (item.breakerUserDisplayName) return item.breakerUserDisplayName
  if (item.breakerUserId) return item.breakerUserId
  if (item.breakerTokenId) return item.breakerTokenId
  if (item.source === 'manual') return strings.breakerSystem
  return strings.breakerUnknown
}

function MonthlyBrokenKeyValue({
  keyId,
  ungroupedLabel,
  detailLabel,
  copyLabel,
  copiedLabel,
  copyState,
  onOpenKey,
  onCopy,
}: {
  keyId: string
  ungroupedLabel: string
  detailLabel: string
  copyLabel: string
  copiedLabel: string
  copyState: 'loading' | 'copied' | null | undefined
  onOpenKey?: (id: string) => void
  onCopy: (anchorEl: HTMLButtonElement) => void | Promise<void>
}): JSX.Element {
  const copyText = copyState === 'copied' ? copiedLabel : copyLabel
  return (
    <div className="monthly-broken-key-value">
      <JobKeyLink
        keyId={keyId}
        keyGroup={null}
        ungroupedLabel={ungroupedLabel}
        detailLabel={detailLabel}
        onOpenKey={onOpenKey}
        showBubble={false}
      />
      <Button
        type="button"
        variant={copyState === 'copied' ? 'success' : 'ghost'}
        size="icon"
        className="monthly-broken-key-copy-button shadow-none"
        title={copyText}
        aria-label={copyText}
        onClick={(event) => void onCopy(event.currentTarget)}
        disabled={copyState === 'loading'}
      >
        <Icon
          icon={copyState === 'copied' ? 'mdi:check' : 'mdi:content-copy'}
          width={16}
          height={16}
          aria-hidden="true"
        />
      </Button>
    </div>
  )
}

function formatUnboundTokenIdentityMeta(
  note: string | null,
  group: string | null,
  groupLabel: string,
): string {
  const parts: string[] = []
  const normalizedNote = note?.trim() ?? ''
  const normalizedGroup = group?.trim() ?? ''
  if (normalizedNote) parts.push(normalizedNote)
  if (normalizedGroup) parts.push(`${groupLabel} ${normalizedGroup}`)
  return parts.join(' · ') || '—'
}

function isAdminUsersSortField(value: string | null): value is AdminUsersSortField {
  return value != null && ADMIN_USERS_SORT_FIELDS.includes(value as AdminUsersSortField)
}

function isAdminUnboundTokenUsageSortField(value: string | null): value is AdminUnboundTokenUsageSortField {
  return value != null && ADMIN_UNBOUND_TOKEN_USAGE_SORT_FIELDS.includes(value as AdminUnboundTokenUsageSortField)
}

function formatKeyGroupName(group: string | null | undefined, ungroupedLabel: string): string {
  const normalized = group?.trim() ?? ''
  return normalized.length > 0 ? normalized : ungroupedLabel
}

function formatRegistrationValue(value: string | null | undefined): string {
  const normalized = value?.trim() ?? ''
  return normalized.length > 0 ? normalized : '—'
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

const adminTableEllipsisFieldStyle = {
  ...adminTableFieldStyle,
  display: 'block',
  minWidth: 0,
  overflow: 'hidden',
  textOverflow: 'ellipsis',
} as const

const adminTableSecondaryFieldStyle = {
  ...adminTableFieldStyle,
  fontSize: '0.92em',
  opacity: 0.68,
} as const

const adminTableEllipsisSecondaryFieldStyle = {
  ...adminTableSecondaryFieldStyle,
  display: 'block',
  minWidth: 0,
  overflow: 'hidden',
  textOverflow: 'ellipsis',
} as const

const adminTableInlineFieldStyle = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 8,
  whiteSpace: 'nowrap',
  lineHeight: 1.35,
  position: 'relative',
  paddingRight: 40,
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

const keysBulkToolbarStyle = {
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  gap: 12,
  flexWrap: 'wrap',
  marginBottom: 16,
  padding: 12,
  borderRadius: 16,
  border: '1px solid hsl(var(--border))',
  background: 'hsl(var(--muted) / 0.35)',
} as const

const keysBulkSelectionStyle = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 10,
  flexWrap: 'wrap',
} as const

const keysBulkActionsStyle = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 8,
  flexWrap: 'wrap',
} as const

const keySelectionCheckboxLabelStyle = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 8,
  cursor: 'pointer',
  whiteSpace: 'nowrap',
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

function getAdminUsersSortFromLocation(): AdminUsersSortField | null {
  const rawSort = new URLSearchParams(window.location.search).get('sort')?.trim() ?? ''
  return isAdminUsersSortField(rawSort) ? rawSort : null
}

function getAdminUsersSortDirectionFromLocation(): SortDirection | null {
  if (getAdminUsersSortFromLocation() == null) return null
  const rawOrder = new URLSearchParams(window.location.search).get('order')?.trim()
  return rawOrder === 'asc' ? 'asc' : 'desc'
}

function getAdminUsersCollectionFromLocation(): AdminUsersCollectionView {
  return new URLSearchParams(window.location.search).get('view') === 'usage' ? 'usage' : 'users'
}

function getAdminUnboundTokenUsageQueryFromLocation(): string {
  return new URLSearchParams(window.location.search).get('q')?.trim() ?? ''
}

function getAdminUnboundTokenUsagePageFromLocation(): number {
  const rawPage = new URLSearchParams(window.location.search).get('page')?.trim() ?? ''
  const parsedPage = Number.parseInt(rawPage, 10)
  return Number.isFinite(parsedPage) && parsedPage > 1 ? parsedPage : 1
}

function getAdminUnboundTokenUsageSortFromLocation(): AdminUnboundTokenUsageSortField | null {
  const rawSort = new URLSearchParams(window.location.search).get('sort')?.trim() ?? ''
  return isAdminUnboundTokenUsageSortField(rawSort) ? rawSort : null
}

function getAdminUnboundTokenUsageSortDirectionFromLocation(): SortDirection | null {
  if (getAdminUnboundTokenUsageSortFromLocation() == null) return null
  const rawOrder = new URLSearchParams(window.location.search).get('order')?.trim()
  return rawOrder === 'asc' ? 'asc' : 'desc'
}

function getAdminTokensCollectionFromLocation(): 'tokens' | 'unbound-usage' {
  return new URLSearchParams(window.location.search).get('view') === 'unbound-usage'
    ? 'unbound-usage'
    : 'tokens'
}

function isAdminUsersOverviewSortField(value: AdminUsersSortField | null): boolean {
  return value != null && ADMIN_USERS_OVERVIEW_SORT_FIELDS.has(value)
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

function getAdminKeysRegistrationIpFromLocation(): string {
  return new URLSearchParams(window.location.search).get('registrationIp')?.trim() ?? ''
}

function getAdminKeysRegionsFromLocation(): string[] {
  const values = new URLSearchParams(window.location.search).getAll('region')
  const normalized = new Set<string>()
  for (const value of values) {
    const trimmed = value.trim()
    if (!trimmed) continue
    normalized.add(trimmed)
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

const englishDateOnlyFormatter = new Intl.DateTimeFormat('en-US', {
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

function buildDashboardQuotaChargeCard(
  window: SummaryWindowMetrics,
  strings: {
    title: string
    localEstimate: string
    upstreamActual: string
    delta: string
    sampledKeys: string
    staleKeys: string
    latestSync: string
    noSync: string
  },
): DashboardQuotaChargeCardData {
  const quotaCharge = window.quota_charge ?? {
    local_estimated_credits: 0,
    upstream_actual_credits: 0,
    sampled_key_count: 0,
    stale_key_count: 0,
    latest_sync_at: null,
  }
  const deltaValue =
    quotaCharge.local_estimated_credits - quotaCharge.upstream_actual_credits
  const latestSync =
    quotaCharge.latest_sync_at != null
      ? `${strings.latestSync} · ${formatRelativeTime(quotaCharge.latest_sync_at)} · ${formatTimestampNoYear(quotaCharge.latest_sync_at)}`
      : `${strings.latestSync} · ${strings.noSync}`

  return {
    title: strings.title,
    localLabel: strings.localEstimate,
    localValue: formatNumber(quotaCharge.local_estimated_credits),
    upstreamLabel: strings.upstreamActual,
    upstreamValue: formatNumber(quotaCharge.upstream_actual_credits),
    deltaLabel: strings.delta,
    deltaValue: formatSignedQuotaDelta(deltaValue),
    deltaTone: deltaValue === 0 ? 'neutral' : 'negative',
    coverage: `${strings.sampledKeys} ${formatNumber(quotaCharge.sampled_key_count)} · ${strings.staleKeys} ${formatNumber(quotaCharge.stale_key_count)}`,
    freshness: latestSync,
  }
}

function formatDateOnly(value: number | null, language: 'en' | 'zh'): string {
  if (!value) return '—'
  const date = new Date(value * 1000)
  if (language === 'zh') {
    const year = date.getFullYear()
    const month = String(date.getMonth() + 1).padStart(2, '0')
    const day = String(date.getDate()).padStart(2, '0')
    return `${year}-${month}-${day}`
  }
  return englishDateOnlyFormatter.format(date)
}

function tokenOwnerPrimary(owner: AuthToken['owner']): string {
  if (!owner) return ''
  return owner.displayName || owner.userId
}

function tokenOwnerSecondary(owner: AuthToken['owner']): string | null {
  if (!owner?.username) return null
  return `@${owner.username}`
}

function isRebalanceGatewayLog(log: RequestLog): boolean {
  return (log.gateway_mode ?? '').trim() === 'rebalance_http'
}

function RebalanceGatewayMarker(): JSX.Element {
  return (
    <span className="log-key-pill__marker" aria-hidden="true">
      <svg viewBox="0 0 16 16" focusable="false">
        <path
          d="M5 2.75a1.75 1.75 0 1 1-1 3.18V11a1 1 0 0 0 1 1h4.18a1.75 1.75 0 1 1 0 1.5H5A2.5 2.5 0 0 1 2.5 11V5.93A1.75 1.75 0 0 1 5 2.75m6 0a1.75 1.75 0 1 1-1 3.18V8a1 1 0 0 1-1 1H6.5v-1.5H8.5V5.93A1.75 1.75 0 0 1 11 2.75"
          fill="currentColor"
        />
      </svg>
    </span>
  )
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

function AdminTableValueStack({
  primary,
  secondary,
  primaryClassName,
  className,
}: {
  primary: string
  secondary?: string | null
  primaryClassName?: string | null
  className?: string
}): JSX.Element {
  return (
    <div className={`admin-table-value-stack${className ? ` ${className}` : ''}`}>
      <span className={`admin-table-value-primary${primaryClassName ? ` ${primaryClassName}` : ''}`}>{primary}</span>
      {secondary ? <span className="admin-table-value-secondary">{secondary}</span> : null}
    </div>
  )
}

function AdminUsersSortableHeader<Field extends string>({
  label,
  displayLabel,
  tooltipLabel,
  field,
  activeField,
  activeOrder,
  onToggle,
}: {
  label: string
  displayLabel?: string
  tooltipLabel?: string
  field: Field
  activeField: Field | null
  activeOrder: SortDirection | null
  onToggle: (field: Field) => void
}): JSX.Element {
  const isActive = activeField === field
  const ariaSort = !isActive ? 'none' : activeOrder === 'asc' ? 'ascending' : 'descending'
  const SortIndicatorIcon = !isActive ? ArrowUpDown : activeOrder === 'asc' ? ArrowUp : ArrowDown
  const visibleLabel = displayLabel ?? label
  const bubbleLabel = tooltipLabel ?? label
  const hasTooltip = bubbleLabel.trim() !== visibleLabel.trim()
  const trigger = (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      className={`admin-table-sort-button${isActive ? ' is-active' : ''}`}
      onClick={() => onToggle(field)}
      aria-label={hasTooltip ? bubbleLabel : undefined}
    >
      <span className="admin-table-sort-label">{visibleLabel}</span>
      <SortIndicatorIcon className="admin-table-sort-indicator" aria-hidden="true" />
    </Button>
  )
  return (
    <th aria-sort={ariaSort}>
      {hasTooltip ? (
        <Tooltip>
          <TooltipTrigger asChild>{trigger}</TooltipTrigger>
          <TooltipContent className="max-w-[min(18rem,calc(100vw-2rem))]" side="top">
            {bubbleLabel}
          </TooltipContent>
        </Tooltip>
      ) : (
        trigger
      )}
    </th>
  )
}

function statusTone(status: string): StatusTone {
  const normalized = status.toLowerCase()
  if (normalized === 'active' || normalized === 'success' || normalized === 'completed') return 'success'
  if (normalized === 'quarantined' || normalized === 'temporary_isolated') return 'warning'
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

function requestLogTone(log: Pick<RequestLog, 'operationalClass' | 'result_status'>): StatusTone {
  return operationalClassTone(log.operationalClass ?? log.result_status)
}

function requestLogLabel(
  log: Pick<RequestLog, 'operationalClass' | 'result_status'>,
  language: 'en' | 'zh',
): string {
  return operationalClassLabel(log.operationalClass ?? log.result_status, language)
}

function keyBadgeStatus(item: Pick<ApiKeyStats, 'status' | 'quarantine' | 'transient_backoff'>): string {
  if (item.quarantine) return 'quarantined'
  if (item.transient_backoff) return 'temporary_isolated'
  return item.status
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

function formatErrorMessage(
  log: RequestLog,
  errorsStrings: AdminTranslations['logs']['errors'],
  language: 'en' | 'zh',
): string {
  const message = log.error_message?.trim()
  if (message) {
    return message
  }

  if (log.operationalClass === 'neutral') {
    return language === 'zh'
      ? 'MCP 控制面 / 非计费请求'
      : 'MCP control-plane / non-billable request'
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

function formatKeyEffectSummary(
  log: RequestLog,
  strings: AdminTranslations,
  language: 'en' | 'zh',
): string {
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
    case 'transient_backoff_set':
      return language === 'zh'
        ? '系统已临时降低该 Key 的使用概率'
        : 'The system temporarily reduced this key selection priority'
    case 'transient_backoff_cleared':
      return language === 'zh'
        ? '后续成功请求已解除该 Key 的临时降级'
        : 'A later successful request cleared this key temporary cooldown'
    case 'cleared_quarantine':
      return language === 'zh' ? '管理员已解除该 Key 的隔离' : 'An admin cleared the quarantine on this key'
    case 'none':
      return strings.logDetails.noKeyEffect
    default:
      return summary && summary.length > 0 ? summary : strings.logDetails.noKeyEffect
  }
}

function formatRequestStatusPair(httpStatus: number | null, mcpStatus: number | null): string {
  return `${httpStatus ?? '—'} / ${mcpStatus ?? '—'}`
}

function formatRequestStatusTooltip(log: RequestLog, strings: AdminTranslations): string {
  return `${strings.logs.table.httpStatus}: ${log.http_status ?? '—'} · ${strings.logs.table.mcpStatus}: ${log.mcp_status ?? '—'}`
}

function keyEffectTone(code: string | null | undefined): StatusTone {
  switch ((code ?? '').trim()) {
    case 'quarantined':
      return 'error'
    case 'marked_exhausted':
      return 'warning'
    case 'restored_active':
    case 'transient_backoff_cleared':
    case 'cleared_quarantine':
      return 'success'
    case 'transient_backoff_set':
      return 'warning'
    default:
      return 'neutral'
  }
}

function keyEffectBadgeLabel(log: RequestLog, strings: AdminTranslations): string {
  switch ((log.key_effect_code ?? '').trim()) {
    case 'quarantined':
      return strings.logs.keyEffects.quarantined
    case 'marked_exhausted':
      return strings.logs.keyEffects.markedExhausted
    case 'restored_active':
      return strings.logs.keyEffects.restoredActive
    case 'transient_backoff_set':
      return strings.logs.keyEffects.transientBackoffSet
    case 'transient_backoff_cleared':
      return strings.logs.keyEffects.transientBackoffCleared
    case 'cleared_quarantine':
      return strings.logs.keyEffects.clearedQuarantine
    case 'none':
    case '':
      return strings.logs.keyEffects.none
    default:
      return strings.logs.keyEffects.unknown
  }
}

const emptyRequestLogFacets: RequestLogFacets = {
  results: [],
  keyEffects: [],
  bindingEffects: [],
  selectionEffects: [],
  tokens: [],
  keys: [],
}

const emptyRequestLogsListPage: RequestLogsListPage = {
  items: [],
  pageSize: LOGS_PER_PAGE,
  nextCursor: null,
  prevCursor: null,
  hasOlder: false,
  hasNewer: false,
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
  const unboundTokenUsageStrings = adminStrings.unboundTokenUsage
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
  const proxySettingsStrings = adminStrings.proxySettings
  const systemSettingsStrings = adminStrings.systemSettings
  const footerStrings = adminStrings.footer
  const errorStrings = adminStrings.errors
  const [summary, setSummary] = useState<Summary | null>(null)
  const [dashboardSummaryWindows, setDashboardSummaryWindows] = useState<SummaryWindowsResponse | null>(null)
  const [dashboardSiteStatusSnapshot, setDashboardSiteStatusSnapshot] = useState<DashboardSiteStatusState | null>(null)
  const [keys, setKeys] = useState<ApiKeyStats[]>([])
  const [dashboardKeys, setDashboardKeys] = useState<ApiKeyStats[]>([])
  const [keysTotal, setKeysTotal] = useState(0)
  const [keysPage, setKeysPage] = useState(getAdminKeysPageFromLocation)
  const [keysPerPage, setKeysPerPage] = useState(getAdminKeysPerPageFromLocation)
  const [keysLoadState, setKeysLoadState] = useState<QueryLoadState>('initial_loading')
  const [keysError, setKeysError] = useState<string | null>(null)
  const [keyGroupFacets, setKeyGroupFacets] = useState<Array<{ value: string; count: number }>>([])
  const [keyStatusFacets, setKeyStatusFacets] = useState<Array<{ value: string; count: number }>>([])
  const [keyRegionFacets, setKeyRegionFacets] = useState<Array<{ value: string; count: number }>>([])
  const [tokens, setTokens] = useState<AuthToken[]>([])
  const [dashboardTokens, setDashboardTokens] = useState<AuthToken[]>([])
  const [dashboardTokenCoverage, setDashboardTokenCoverage] = useState<DashboardTokenCoverage>('ok')
  const [dashboardTrend, setDashboardTrend] = useState<DashboardTrendBuckets>(() => createEmptyDashboardTrend())
  const [dashboardHourlyRequestWindow, setDashboardHourlyRequestWindow] = useState<DashboardHourlyRequestWindow>(
    () => createEmptyDashboardHourlyRequestWindow(),
  )
  const [dashboardRecentAlerts, setDashboardRecentAlerts] = useState<RecentAlertsSummary>({
    windowHours: 24,
    totalEvents: 0,
    groupedCount: 0,
    countsByType: [],
    topGroups: [],
  })
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
  const [unboundTokenUsage, setUnboundTokenUsage] = useState<AdminUnboundTokenUsageSummary[]>([])
  const [unboundTokenUsageTotal, setUnboundTokenUsageTotal] = useState(0)
  const [unboundTokenUsagePage, setUnboundTokenUsagePage] = useState(() => getAdminUnboundTokenUsagePageFromLocation())
  const [unboundTokenUsageQueryInput, setUnboundTokenUsageQueryInput] =
    useState(() => getAdminUnboundTokenUsageQueryFromLocation())
  const [unboundTokenUsageQuery, setUnboundTokenUsageQuery] =
    useState(() => getAdminUnboundTokenUsageQueryFromLocation())
  const [unboundTokenUsageSort, setUnboundTokenUsageSort] =
    useState<AdminUnboundTokenUsageSortField | null>(() => getAdminUnboundTokenUsageSortFromLocation())
  const [unboundTokenUsageSortOrder, setUnboundTokenUsageSortOrder] =
    useState<SortDirection | null>(() => getAdminUnboundTokenUsageSortDirectionFromLocation())
  const [unboundTokenUsageLoadState, setUnboundTokenUsageLoadState] = useState<QueryLoadState>('initial_loading')
  const [unboundTokenUsageError, setUnboundTokenUsageError] = useState<string | null>(null)
  const [logs, setLogs] = useState<RequestLog[]>([])
  const [dashboardLogs, setDashboardLogs] = useState<RequestLog[]>([])
  const [logsPerPage, setLogsPerPage] = useState(LOGS_PER_PAGE)
  const [logsCursor, setLogsCursor] = useState<string | null>(null)
  const [logsDirection, setLogsDirection] = useState<'older' | 'newer'>('older')
  const [logsPageInfo, setLogsPageInfo] = useState<RequestLogsListPage>(emptyRequestLogsListPage)
  const [requestLogRequestKindOptions, setRequestLogRequestKindOptions] = useState<TokenLogRequestKindOption[]>([])
  const [requestLogSelectedKinds, setRequestLogSelectedKinds] = useState<string[]>([])
  const [requestLogQuickBilling, setRequestLogQuickBilling] = useState<TokenLogRequestKindQuickBilling>('all')
  const [requestLogQuickProtocol, setRequestLogQuickProtocol] = useState<TokenLogRequestKindQuickProtocol>('all')
  const [requestLogFacets, setRequestLogFacets] = useState<RequestLogFacets>(emptyRequestLogFacets)
  const [requestLogsCatalog, setRequestLogsCatalog] = useState<RequestLogsCatalog | null>(null)
  const [requestLogOutcomeFilter, setRequestLogOutcomeFilter] = useState<RecentRequestsOutcomeFilter | null>(null)
  const [requestLogKeyFilter, setRequestLogKeyFilter] = useState<string | null>(null)
  const [requestsLoadState, setRequestsLoadState] = useState<QueryLoadState>('initial_loading')
  const [requestsError, setRequestsError] = useState<string | null>(null)
  const [requestEntityDrawer, setRequestEntityDrawer] = useState<{ kind: 'key' | 'token'; id: string } | null>(null)
  const [monthlyBrokenDrawer, setMonthlyBrokenDrawer] = useState<{
    subjectKind: 'user' | 'token'
    id: string
    label: string
  } | null>(null)
  const [monthlyBrokenDrawerItems, setMonthlyBrokenDrawerItems] = useState<MonthlyBrokenKeyDetail[]>([])
  const [monthlyBrokenDrawerLoadState, setMonthlyBrokenDrawerLoadState] =
    useState<QueryLoadState>('initial_loading')
  const [monthlyBrokenDrawerError, setMonthlyBrokenDrawerError] = useState<string | null>(null)
  const [jobs, setJobs] = useState<JobLogView[]>([])
  const [dashboardJobs, setDashboardJobs] = useState<JobLogView[]>([])
  const [jobFilter, setJobFilter] = useState<JobGroup>('all')
  const [jobsPage, setJobsPage] = useState(1)
  const jobsPerPage = 10
  const [jobsTotal, setJobsTotal] = useState(0)
  const [jobsGroupCounts, setJobsGroupCounts] = useState<JobGroupCounts>(() => emptyAdminJobGroupCounts())
  const [jobsLoadState, setJobsLoadState] = useState<QueryLoadState>('initial_loading')
  const [jobsError, setJobsError] = useState<string | null>(null)
  const [users, setUsers] = useState<AdminUserSummary[]>([])
  const [usersTotal, setUsersTotal] = useState(0)
  const [usersPage, setUsersPage] = useState(() => getAdminUsersPageFromLocation())
  const [usersQueryInput, setUsersQueryInput] = useState(() => getAdminUsersQueryFromLocation())
  const [usersQuery, setUsersQuery] = useState(() => getAdminUsersQueryFromLocation())
  const [usersTagFilterId, setUsersTagFilterId] = useState<string | null>(() => getAdminUsersTagFilterFromLocation())
  const [usersSort, setUsersSort] = useState<AdminUsersSortField | null>(() => getAdminUsersSortFromLocation())
  const [usersSortOrder, setUsersSortOrder] = useState<SortDirection | null>(() => getAdminUsersSortDirectionFromLocation())
  const [usersLoadState, setUsersLoadState] = useState<QueryLoadState>('initial_loading')
  const isUsersModuleRoute = route.name === 'module' && route.module === 'users'
  const isUserUsageRoute = route.name === 'user-usage'
  const isUsersCollectionRoute = isUsersModuleRoute || isUserUsageRoute
  const [usersError, setUsersError] = useState<string | null>(null)
  const [allowRegistration, setAllowRegistration] = useState<boolean | null>(null)
  const [registrationSettingsLoaded, setRegistrationSettingsLoaded] = useState(false)
  const [registrationSettingsLoading, setRegistrationSettingsLoading] = useState(false)
  const [registrationSettingsSaving, setRegistrationSettingsSaving] = useState(false)
  const [registrationSettingsError, setRegistrationSettingsError] = useState<string | null>(null)
  const [selectedUserDetail, setSelectedUserDetail] = useState<AdminUserDetail | null>(null)
  const [userDetailRevision, setUserDetailRevision] = useState(0)
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
  const [forwardProxySettings, setForwardProxySettings] = useState<ForwardProxySettings | null>(null)
  const [forwardProxySettingsLoadState, setForwardProxySettingsLoadState] =
    useState<QueryLoadState>('initial_loading')
  const [forwardProxySettingsError, setForwardProxySettingsError] = useState<string | null>(null)
  const [systemSettings, setSystemSettings] = useState<SystemSettings | null>(null)
  const [systemSettingsLoadState, setSystemSettingsLoadState] =
    useState<QueryLoadState>('initial_loading')
  const [systemSettingsError, setSystemSettingsError] = useState<string | null>(null)
  const [systemSettingsSaving, setSystemSettingsSaving] = useState(false)
  const [forwardProxyStats, setForwardProxyStats] = useState<ForwardProxyStatsResponse | null>(null)
  const [forwardProxyStatsLoadState, setForwardProxyStatsLoadState] =
    useState<QueryLoadState>('initial_loading')
  const [forwardProxyStatsError, setForwardProxyStatsError] = useState<string | null>(null)
  const [forwardProxySaving, setForwardProxySaving] = useState(false)
  const [forwardProxySaveError, setForwardProxySaveError] = useState<string | null>(null)
  const [forwardProxyRevalidating, setForwardProxyRevalidating] = useState(false)
  const [forwardProxyRevalidateError, setForwardProxyRevalidateError] = useState<string | null>(null)
  const [forwardProxyRevalidateProgress, setForwardProxyRevalidateProgress] =
    useState<ForwardProxyDialogProgressState | null>(null)
  const [forwardProxySavedAt, setForwardProxySavedAt] = useState<number | null>(null)
  const [alertsRefreshToken, setAlertsRefreshToken] = useState(0)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const pollingTimerRef = useRef<number | null>(null)
  const routeRef = useRef<AdminPathRoute>(route)
  const baseDataLoadedRef = useRef(false)
  const dashboardOverviewVersionRef = useRef(0)
  const unboundTokenUsageQueryKeyRef = useRef<string | null>(null)
  const requestsLoadedRef = useRef(false)
  const jobsLoadedRef = useRef(false)
  const keysLoadedRef = useRef(false)
  const usersLoadedRef = useRef(false)
  const unboundTokenUsageLoadedRef = useRef(false)
  const keysQueryKeyRef = useRef<string | null>(null)
  const usersQueryKeyRef = useRef<string | null>(null)
  const baseDataAbortRef = useRef<AbortController | null>(null)
  const requestsAbortRef = useRef<AbortController | null>(null)
  const jobsAbortRef = useRef<AbortController | null>(null)
  const keysAbortRef = useRef<AbortController | null>(null)
  const usersAbortRef = useRef<AbortController | null>(null)
  const unboundTokenUsageAbortRef = useRef<AbortController | null>(null)
  const forwardProxySettingsAbortRef = useRef<AbortController | null>(null)
  const systemSettingsAbortRef = useRef<AbortController | null>(null)
  const forwardProxyStatsAbortRef = useRef<AbortController | null>(null)
  const forwardProxySettingsLoadedRef = useRef(false)
  const systemSettingsLoadedRef = useRef(false)
  const forwardProxyStatsLoadedRef = useRef(false)
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
  const [selectedKeyRegistrationIp, setSelectedKeyRegistrationIp] = useState(getAdminKeysRegistrationIpFromLocation)
  const [keyRegistrationIpInput, setKeyRegistrationIpInput] = useState(getAdminKeysRegistrationIpFromLocation)
  const [selectedKeyRegions, setSelectedKeyRegions] = useState<string[]>(() => getAdminKeysRegionsFromLocation())
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
    | 'already_exists'
    | 'ok'
    | 'ok_exhausted'
    | 'unauthorized'
    | 'forbidden'
    | 'invalid'
    | 'error'

  type KeyValidationRow = {
    api_key: string
    status: KeyValidationStatus
    registration_ip?: string | null
    registration_region?: string | null
    assigned_proxy_key?: string | null
    assigned_proxy_label?: string | null
    assigned_proxy_match_kind?: ValidateKeyResult['assigned_proxy_match_kind']
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
    registration_ip_by_key: Record<string, string>
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
  const [pendingBulkDelete, setPendingBulkDelete] = useState(false)
  const [pendingDisableId, setPendingDisableId] = useState<string | null>(null)
  const [selectedKeyIds, setSelectedKeyIds] = useState<Set<string>>(() => new Set())
  const [bulkKeyActionInFlight, setBulkKeyActionInFlight] = useState<ApiKeyBulkAction | null>(null)
  const [keysBulkFeedback, setKeysBulkFeedback] = useState<{ kind: 'success' | 'error'; message: string } | null>(null)
  const [bulkSyncProgress, setBulkSyncProgress] = useState<ApiKeyBulkSyncProgressState | null>(null)
  const [bulkSyncBubbleVisible, setBulkSyncBubbleVisible] = useState(false)
  const [bulkSyncBubblePinnedPosition, setBulkSyncBubblePinnedPosition] =
    useState<ApiKeyBulkSyncBubblePinnedPosition | null>(null)
  const keysSelectAllRef = useRef<HTMLInputElement | null>(null)
  const bulkSyncButtonRef = useRef<HTMLButtonElement | null>(null)
  const [pendingTokenDeleteId, setPendingTokenDeleteId] = useState<string | null>(null)
  const [editingTokenId, setEditingTokenId] = useState<string | null>(null)
  const [editingTokenNote, setEditingTokenNote] = useState('')
  const [savingTokenNote, setSavingTokenNote] = useState(false)
  const [sseConnected, setSseConnected] = useState(false)
  const [sseFallbackActive, setSseFallbackActive] = useState(false)
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
              keyId: '上游 Key ID',
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
              keyId: 'Upstream Key ID',
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

  const copyStateKey = useCallback((scope: 'brokenKeys' | 'keys' | 'logs' | 'tokens', identifier: string | number) => {
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

  const handleCopyMonthlyBrokenKeyId = useCallback(
    async (keyId: string, stateKey: string, anchorEl?: HTMLElement | null) => {
      setManualCopyBubble(null)
      updateCopyState(stateKey, 'loading')
      try {
        const copyResult = await copyToClipboard(keyId, { preferExecCommand: true })
        if (!copyResult.ok) {
          updateCopyState(stateKey, null)
          if (anchorEl) {
            openManualCopyBubble({
              anchorEl,
              title: manualCopyText.title,
              description: manualCopyText.description,
              fieldLabel: manualCopyText.fields.keyId,
              value: keyId,
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
    [copyToClipboard, errorStrings.copyKey, manualCopyText, openManualCopyBubble, setError, updateCopyState],
  )

  const loadShellData = useCallback(
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
      try {
        const [ver, profileData] = await Promise.all([
          fetchVersion(request.signal).catch(() => null),
          fetchProfile(request.signal).catch(() => null),
        ])

        if (request.signal.aborted) {
          return
        }

        setProfile(profileData ?? null)
        setVersion(ver ?? null)
        setLastUpdated(new Date())
        baseDataLoadedRef.current = true
      } catch (err) {
        if ((err as Error).name === 'AbortError') {
          return
        }
        setError(err instanceof Error ? err.message : 'Unexpected error occurred')
      } finally {
        if (showGlobalLoading && !request.signal.aborted) {
          setLoading(false)
        }
        request.cleanup()
      }
    },
    [beginManagedRequest],
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
        const shouldLoadTokens = route.name === 'module' && route.module === 'tokens'
        const [ver, profileData, tokenData, tokenGroupsData] = await Promise.all([
          fetchVersion(request.signal).catch(() => null),
          fetchProfile(request.signal).catch(() => null),
          shouldLoadTokens
            ? fetchTokens(
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
              )
            : Promise.resolve(null),
          shouldLoadTokens
            ? fetchTokenGroups(request.signal).catch(() => [] as TokenGroup[])
            : Promise.resolve(null),
        ])

        if (request.signal.aborted) {
          return
        }

        setProfile(profileData ?? null)
        if (tokenData) {
          setTokens(tokenData.items)
          setTokensTotal(tokenData.total)
        }
        if (tokenGroupsData) {
          setTokenGroups(tokenGroupsData)
        }
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
    [beginManagedRequest, route, tokensPage, selectedTokenGroupName, selectedTokenUngrouped],
  )

  const loadDashboardOverview = useCallback(
    async (signal?: AbortSignal) => {
      const requestVersion = ++dashboardOverviewVersionRef.current
      try {
        const overview = await fetchDashboardOverview(signal)

        if (signal?.aborted) {
          return
        }
        const overviewStale = requestVersion !== dashboardOverviewVersionRef.current
        if (!overviewStale) {
          setSummary(overview.summary)
          setDashboardSummaryWindows(overview.summaryWindows)
          setDashboardSiteStatusSnapshot(overview.siteStatus)
          setDashboardTokens(overview.disabledTokens)
          setDashboardTokenCoverage(overview.tokenCoverage)
          setDashboardTrend(overview.trend)
          setDashboardHourlyRequestWindow(overview.hourlyRequestWindow)
          setDashboardKeys(overview.exhaustedKeys)
          setDashboardLogs(overview.recentLogs)
          setDashboardJobs(overview.recentJobs)
          setDashboardRecentAlerts(overview.recentAlerts)
          setDashboardOverviewLoaded(true)
          setLastUpdated(new Date())
          setError(null)
        }
      } catch (err) {
        if ((err as Error).name === 'AbortError') {
          return
        }
        const overviewStale = requestVersion !== dashboardOverviewVersionRef.current
        if (overviewStale) {
          return
        }
        setSummary(null)
        setDashboardSummaryWindows(null)
        setDashboardSiteStatusSnapshot(null)
        setDashboardTokens([])
        setDashboardTokenCoverage('error')
        setDashboardTrend(createEmptyDashboardTrend())
        setDashboardHourlyRequestWindow(createEmptyDashboardHourlyRequestWindow())
        setDashboardKeys([])
        setDashboardLogs([])
        setDashboardJobs([])
        setDashboardRecentAlerts({
          windowHours: 24,
          totalEvents: 0,
          groupedCount: 0,
          countsByType: [],
          topGroups: [],
        })
        setDashboardOverviewLoaded(true)
      }
    },
    [],
  )

  useEffect(() => {
    routeRef.current = route
  }, [route])

  const loadUnboundTokenUsage = useCallback(
    async ({
      signal,
      reason = 'refresh',
    }: {
      signal?: AbortSignal
      reason?: 'initial' | 'switch' | 'refresh'
    } = {}) => {
      const request = beginManagedRequest(unboundTokenUsageAbortRef, signal)
      try {
        setUnboundTokenUsageLoadState(
          reason === 'refresh'
            ? getRefreshingLoadState(unboundTokenUsageQueryKeyRef.current != null)
            : getBlockingLoadState(unboundTokenUsageQueryKeyRef.current != null),
        )
        setUnboundTokenUsageError(null)
        if (reason !== 'refresh') {
          setUnboundTokenUsage([])
          setUnboundTokenUsageTotal(0)
        }
        const paged = await fetchAdminUnboundTokenUsage(
          unboundTokenUsagePage,
          USERS_PER_PAGE,
          unboundTokenUsageQuery,
          unboundTokenUsageSort,
          unboundTokenUsageSortOrder,
          request.signal,
        )
        if (request.signal.aborted) return
        setUnboundTokenUsage(paged.items)
        setUnboundTokenUsageTotal(paged.total)
        setUnboundTokenUsageLoadState('ready')
        unboundTokenUsageLoadedRef.current = true
        unboundTokenUsageQueryKeyRef.current = [
          unboundTokenUsagePage,
          unboundTokenUsageQuery,
          unboundTokenUsageSort ?? '',
          unboundTokenUsageSortOrder ?? '',
        ].join(':')
      } catch (err) {
        if (request.signal.aborted) return
        console.error(err)
        setUnboundTokenUsage([])
        setUnboundTokenUsageTotal(0)
        setUnboundTokenUsageError(err instanceof Error ? err.message : unboundTokenUsageStrings.error)
        setUnboundTokenUsageLoadState('error')
      } finally {
        request.cleanup()
      }
    },
    [
      beginManagedRequest,
      unboundTokenUsagePage,
      unboundTokenUsageQuery,
      unboundTokenUsageSort,
      unboundTokenUsageSortOrder,
      unboundTokenUsageStrings.error,
    ],
  )

  const loadForwardProxySettingsData = useCallback(
    async ({
      signal,
      reason = 'refresh',
    }: {
      signal?: AbortSignal
      reason?: 'initial' | 'switch' | 'refresh'
    } = {}) => {
      const request = beginManagedRequest(forwardProxySettingsAbortRef, signal)
      setForwardProxySettingsLoadState(
        reason === 'refresh'
          ? getRefreshingLoadState(forwardProxySettingsLoadedRef.current)
          : getBlockingLoadState(forwardProxySettingsLoadedRef.current),
      )
      setForwardProxySettingsError(null)

      try {
        const nextSettings = await fetchForwardProxySettings(request.signal)
        if (request.signal.aborted) return
        setForwardProxySettings(nextSettings)
        setForwardProxySettingsLoadState('ready')
        setLastUpdated(new Date())
        forwardProxySettingsLoadedRef.current = true
      } catch (err) {
        if (request.signal.aborted) return
        console.error(err)
        setForwardProxySettingsError(err instanceof Error ? err.message : loadingStateStrings.error)
        setForwardProxySettingsLoadState('error')
      } finally {
        request.cleanup()
      }
    },
    [beginManagedRequest, loadingStateStrings.error],
  )

  const loadSystemSettingsData = useCallback(
    async ({
      signal,
      reason = 'refresh',
    }: {
      signal?: AbortSignal
      reason?: 'initial' | 'switch' | 'refresh'
    } = {}) => {
      const request = beginManagedRequest(systemSettingsAbortRef, signal)
      setSystemSettingsLoadState(
        reason === 'refresh'
          ? getRefreshingLoadState(systemSettingsLoadedRef.current)
          : getBlockingLoadState(systemSettingsLoadedRef.current),
      )
      setSystemSettingsError(null)

      try {
        const nextSettings = await fetchSystemSettings(request.signal)
        if (request.signal.aborted) return
        setSystemSettings(nextSettings)
        setSystemSettingsLoadState('ready')
        setLastUpdated(new Date())
        systemSettingsLoadedRef.current = true
      } catch (err) {
        if (request.signal.aborted) return
        console.error(err)
        setSystemSettingsError(err instanceof Error ? err.message : loadingStateStrings.error)
        setSystemSettingsLoadState('error')
      } finally {
        request.cleanup()
      }
    },
    [beginManagedRequest, loadingStateStrings.error],
  )

  const loadForwardProxyStatsData = useCallback(
    async ({
      signal,
      reason = 'refresh',
    }: {
      signal?: AbortSignal
      reason?: 'initial' | 'switch' | 'refresh'
    } = {}) => {
      const request = beginManagedRequest(forwardProxyStatsAbortRef, signal)
      setForwardProxyStatsLoadState(
        reason === 'refresh'
          ? getRefreshingLoadState(forwardProxyStatsLoadedRef.current)
          : getBlockingLoadState(forwardProxyStatsLoadedRef.current),
      )
      setForwardProxyStatsError(null)

      try {
        const nextStats = await fetchForwardProxyStats(request.signal)
        if (request.signal.aborted) return
        setForwardProxyStats(nextStats)
        setForwardProxyStatsLoadState('ready')
        setLastUpdated(new Date())
        forwardProxyStatsLoadedRef.current = true
      } catch (err) {
        if (request.signal.aborted) return
        console.error(err)
        setForwardProxyStatsError(err instanceof Error ? err.message : loadingStateStrings.error)
        setForwardProxyStatsLoadState('error')
      } finally {
        request.cleanup()
      }
    },
    [beginManagedRequest, loadingStateStrings.error],
  )

  const validateForwardProxyCandidates = useCallback(
    async (
      kind: ForwardProxyValidationKind,
      rawValues: string[],
      onProgress?: (event: ForwardProxyProgressEvent) => void,
      signal?: AbortSignal,
    ): Promise<ForwardProxyValidationEntry[]> => {
      const results = new Array<ForwardProxyValidationEntry>(rawValues.length)
      let hadFatalError = false
      const fallbackPhaseKey = kind === 'subscriptionUrl' ? 'normalize_input' : 'parse_input'
      const fallbackLabel =
        kind === 'subscriptionUrl'
          ? proxySettingsStrings.progress.steps.normalize_input
          : proxySettingsStrings.progress.steps.parse_input
      const emitValidationError = (
        message: string,
        lastPhase?: Extract<ForwardProxyProgressEvent, { type: 'phase' }> | null,
        detail?: string,
      ) => {
        onProgress?.({
          type: 'error',
          operation: 'validate',
          phaseKey: lastPhase?.phaseKey ?? fallbackPhaseKey,
          label: lastPhase?.label ?? fallbackLabel,
          current: lastPhase?.current,
          total: lastPhase?.total,
          message,
          detail: lastPhase?.detail ?? detail,
        })
      }

      if (rawValues.length === 1) {
        const value = rawValues[0]
        let lastPhase: Extract<ForwardProxyProgressEvent, { type: 'phase' }> | null = null
        try {
          const result = await validateForwardProxyCandidateWithProgress(
            { kind, value },
            (event) => {
              if (event.type === 'complete') return
              if (event.type === 'phase') {
                lastPhase = event
              }
              onProgress?.(event)
            },
            signal,
          )
          results[0] = {
            id: `${kind}:0:${value}`,
            kind,
            value,
            result,
          } satisfies ForwardProxyValidationEntry
          if (!result.ok) {
            emitValidationError(result.message, lastPhase, value)
          }
        } catch (err) {
          if (signal?.aborted || (err as Error).name === 'AbortError') {
            throw err
          }
          hadFatalError = true
          const message =
            err instanceof Error ? err.message : proxySettingsStrings.validation.requestFailed
          emitValidationError(message, lastPhase, value)
          results[0] = {
            id: `${kind}:0:${value}`,
            kind,
            value,
            result: {
              ok: false,
              message,
              normalizedValue: value,
              discoveredNodes: 0,
              latencyMs: null,
            },
          } satisfies ForwardProxyValidationEntry
        }
      } else {
        let introEmitted = false
        const probeStarted = new Set<number>()
        const lastPhaseByIndex = new Array<Extract<ForwardProxyProgressEvent, { type: 'phase' }> | null>(
          rawValues.length,
        ).fill(null)

        await Promise.all(rawValues.map(async (value, index) => {
          try {
            const result = await validateForwardProxyCandidateWithProgress(
              { kind, value },
              (event) => {
                if (!onProgress || event.type === 'complete') return

                if (event.type === 'phase') {
                  lastPhaseByIndex[index] = event
                  if (
                    (event.phaseKey === 'parse_input' ||
                      event.phaseKey === 'normalize_input' ||
                      event.phaseKey === 'fetch_subscription') &&
                    !introEmitted
                  ) {
                    introEmitted = true
                    onProgress(event)
                    return
                  }

                  if (event.phaseKey === 'probe_nodes') {
                    probeStarted.add(index)
                    onProgress({
                      ...event,
                      current: probeStarted.size,
                      total: rawValues.length,
                    })
                    return
                  }

                  if (event.phaseKey === 'generate_result') {
                    return
                  }
                }

                onProgress(event)
              },
              signal,
            )
            results[index] = {
              id: `${kind}:${index}:${value}`,
              kind,
              value,
              result,
            } satisfies ForwardProxyValidationEntry
          } catch (err) {
            if (signal?.aborted || (err as Error).name === 'AbortError') {
              throw err
            }
            hadFatalError = true
            const message =
              err instanceof Error ? err.message : proxySettingsStrings.validation.requestFailed
            emitValidationError(message, lastPhaseByIndex[index], value)
            results[index] = {
              id: `${kind}:${index}:${value}`,
              kind,
              value,
              result: {
                ok: false,
                message,
                normalizedValue: value,
                discoveredNodes: 0,
                latencyMs: null,
              },
            } satisfies ForwardProxyValidationEntry
          }
        }))

        const hasAnySuccess = results.some((entry) => entry?.result.ok)
        if (!hadFatalError && hasAnySuccess) {
          onProgress?.({
            type: 'phase',
            operation: 'validate',
            phaseKey: 'generate_result',
            label: proxySettingsStrings.progress.steps.generate_result,
            current: rawValues.length,
            total: rawValues.length,
          })
        } else if (!hadFatalError && !hasAnySuccess) {
          emitValidationError(
            proxySettingsStrings.validation.requestFailed,
            lastPhaseByIndex.find((event) => event != null) ?? null,
          )
        }
      }

      const hasAnySuccess = results.some((entry) => entry?.result.ok)
      if (!hadFatalError && hasAnySuccess) {
        onProgress?.({
          type: 'complete',
          operation: 'validate',
          payload: null,
        })
      }

      return results.filter((entry): entry is ForwardProxyValidationEntry => entry != null)
    },
    [
      proxySettingsStrings.progress.steps.generate_result,
      proxySettingsStrings.progress.steps.normalize_input,
      proxySettingsStrings.progress.steps.parse_input,
      proxySettingsStrings.validation.requestFailed,
    ],
  )

  const saveForwardProxySettings = useCallback(async (
    nextDraft: ForwardProxyDraft,
    onProgress?: (event: ForwardProxyProgressEvent) => void,
    options?: { skipBootstrapProbe?: boolean },
  ) => {
    const parsedInterval = Number.parseInt(nextDraft.subscriptionUpdateIntervalSecs.trim(), 10)
    if (!Number.isFinite(parsedInterval) || parsedInterval <= 0) {
      setForwardProxySaveError(proxySettingsStrings.config.invalidInterval)
      throw new Error(proxySettingsStrings.config.invalidInterval)
    }

    setForwardProxySaveError(null)
    setForwardProxySaving(true)

    try {
      const nextSettings = await updateForwardProxySettingsWithProgress(
        {
          proxyUrls: splitMultilineEntries(nextDraft.proxyUrlsText),
          subscriptionUrls: splitMultilineEntries(nextDraft.subscriptionUrlsText),
          subscriptionUpdateIntervalSecs: parsedInterval,
          insertDirect: nextDraft.insertDirect,
          egressSocks5Enabled: nextDraft.egressSocks5Enabled,
          egressSocks5Url: nextDraft.egressSocks5Url.trim(),
          skipBootstrapProbe: options?.skipBootstrapProbe ?? false,
        },
        onProgress,
      )
      setForwardProxySettings(nextSettings)
      setForwardProxySettingsLoadState('ready')
      setForwardProxySettingsError(null)
      setForwardProxySavedAt(Date.now())
      setLastUpdated(new Date())
      forwardProxySettingsLoadedRef.current = true
      onProgress?.({
        type: 'phase',
        operation: 'save',
        phaseKey: 'refresh_ui',
        label: 'Refreshing settings and stats',
      })
      try {
        await loadForwardProxyStatsData({ reason: 'refresh' })
      } catch (refreshErr) {
        console.error(refreshErr)
      } finally {
        onProgress?.({
          type: 'complete',
          operation: 'save',
          payload: null,
        })
      }
    } catch (err) {
      console.error(err)
      const message = err instanceof Error ? err.message : proxySettingsStrings.config.saveFailed
      setForwardProxySaveError(message)
      throw err instanceof Error ? err : new Error(message)
    } finally {
      setForwardProxySaving(false)
    }
  }, [
    loadForwardProxyStatsData,
    proxySettingsStrings.config.invalidInterval,
    proxySettingsStrings.config.saveFailed,
  ])

  const revalidateForwardProxy = useCallback(async () => {
    setForwardProxyRevalidateError(null)
    setForwardProxyRevalidating(true)
    setForwardProxyRevalidateProgress(
      createDialogProgressState(proxySettingsStrings.progress, 'subscription', 'revalidate'),
    )

    try {
      const nextSettings = await revalidateForwardProxyWithProgress((event) => {
        setForwardProxyRevalidateProgress((current) => {
          const base = current ?? createDialogProgressState(proxySettingsStrings.progress, 'subscription', 'revalidate')
          return updateDialogProgressState(base, proxySettingsStrings.progress, event)
        })
      })
      setForwardProxySettings(nextSettings)
      setForwardProxySettingsLoadState('ready')
      setForwardProxySettingsError(null)
      setForwardProxySavedAt(Date.now())
      setLastUpdated(new Date())
      forwardProxySettingsLoadedRef.current = true
      await finalizeForwardProxyRevalidate(
        loadForwardProxyStatsData,
        () => {
          setForwardProxyRevalidateProgress((current) => {
            const base = current ?? createDialogProgressState(proxySettingsStrings.progress, 'subscription', 'revalidate')
            return updateDialogProgressState(base, proxySettingsStrings.progress, {
              type: 'phase',
              operation: 'revalidate',
              phaseKey: 'refresh_ui',
              label: proxySettingsStrings.progress.steps.refresh_ui,
            })
          })
        },
        () => {
          setForwardProxyRevalidateProgress((current) => {
            const base = current ?? createDialogProgressState(proxySettingsStrings.progress, 'subscription', 'revalidate')
            return updateDialogProgressState(base, proxySettingsStrings.progress, {
              type: 'complete',
              operation: 'revalidate',
              payload: null,
            })
          })
        },
      )
    } catch (err) {
      console.error(err)
      const message = err instanceof Error ? err.message : proxySettingsStrings.validation.requestFailed
      setForwardProxyRevalidateError(message)
      setForwardProxyRevalidateProgress((current) => {
        const base = current ?? createDialogProgressState(proxySettingsStrings.progress, 'subscription', 'revalidate')
        return updateDialogProgressState(base, proxySettingsStrings.progress, {
          type: 'error',
          operation: 'revalidate',
          message,
        })
      })
    } finally {
      setForwardProxyRevalidating(false)
    }
  }, [
    loadForwardProxyStatsData,
    proxySettingsStrings.progress,
    proxySettingsStrings.validation.requestFailed,
  ])

  useEffect(() => {
    const controller = new AbortController()
    if (!baseDataLoadedRef.current) {
      setLoading(true)
    }
    const reason = baseDataLoadedRef.current ? 'switch' : 'initial'
    if (route.name === 'module' && route.module === 'dashboard') {
      void Promise.all([
        loadShellData({ signal: controller.signal, reason }),
        loadDashboardOverview(controller.signal),
      ]).finally(() => {
        if (!controller.signal.aborted) {
          setLoading(false)
        }
      })
    } else {
      void loadData({
        signal: controller.signal,
        reason,
        showGlobalLoading: !baseDataLoadedRef.current,
      })
    }
    return () => controller.abort()
  }, [loadDashboardOverview, loadData, loadShellData, route])

  useLayoutEffect(() => {
    if (!(route.name === 'module' && route.module === 'dashboard')) {
      return
    }
    dashboardOverviewVersionRef.current += 1
    setDashboardOverviewLoaded(false)
    setDashboardSummaryWindows(null)
    setDashboardSiteStatusSnapshot(null)
  }, [route])

  useEffect(() => {
    if (route.name !== 'unbound-token-usage') {
      return
    }
    const controller = new AbortController()
    const nextQueryKey = [
      unboundTokenUsagePage,
      unboundTokenUsageQuery,
      unboundTokenUsageSort ?? '',
      unboundTokenUsageSortOrder ?? '',
    ].join(':')
    const reason =
      unboundTokenUsageLoadedRef.current && unboundTokenUsageQueryKeyRef.current === nextQueryKey
        ? 'refresh'
        : unboundTokenUsageLoadedRef.current
          ? 'switch'
          : 'initial'
    void loadUnboundTokenUsage({
      signal: controller.signal,
      reason,
    })
    return () => controller.abort()
  }, [
    loadUnboundTokenUsage,
    route,
    unboundTokenUsagePage,
    unboundTokenUsageQuery,
    unboundTokenUsageSort,
    unboundTokenUsageSortOrder,
  ])

  useEffect(() => {
    if (!(route.name === 'module' && route.module === 'system-settings')) {
      return
    }

    const controller = new AbortController()
    void loadSystemSettingsData({
      signal: controller.signal,
      reason: systemSettingsLoadedRef.current ? 'switch' : 'initial',
    })

    return () => controller.abort()
  }, [route, loadSystemSettingsData])

  useEffect(() => {
    if (!(route.name === 'module' && route.module === 'proxy-settings')) {
      return
    }

    const controller = new AbortController()
    void Promise.all([
      loadForwardProxySettingsData({
        signal: controller.signal,
        reason: forwardProxySettingsLoadedRef.current ? 'switch' : 'initial',
      }),
      loadForwardProxyStatsData({
        signal: controller.signal,
        reason: forwardProxyStatsLoadedRef.current ? 'switch' : 'initial',
      }),
    ])

    return () => controller.abort()
  }, [route, loadForwardProxySettingsData, loadForwardProxyStatsData])

  useEffect(() => {
    if (!(route.name === 'module' && route.module === 'proxy-settings')) {
      return
    }

    const timer = window.setInterval(() => {
      void loadForwardProxyStatsData({ reason: 'refresh' })
    }, REFRESH_INTERVAL_MS)

    return () => window.clearInterval(timer)
  }, [route, loadForwardProxyStatsData])

  const requestLogQuickFilters = useMemo(
    () => ({
      billing: requestLogQuickBilling,
      protocol: requestLogQuickProtocol,
    }),
    [requestLogQuickBilling, requestLogQuickProtocol],
  )
  const requestLogSelectedKindsNormalized = useMemo(
    () => Array.from(new Set(requestLogSelectedKinds.map((value) => value.trim()).filter(Boolean))),
    [requestLogSelectedKinds],
  )
  const requestLogQuickSelection = useMemo(
    () => buildRequestKindQuickFilterSelection(requestLogRequestKindOptions, requestLogQuickFilters),
    [requestLogQuickFilters, requestLogRequestKindOptions],
  )
  const requestLogEffectiveKinds = useMemo(
    () =>
      resolveEffectiveRequestKindSelection(
        requestLogSelectedKindsNormalized,
        requestLogQuickFilters,
        requestLogQuickSelection,
      ),
    [requestLogQuickFilters, requestLogQuickSelection, requestLogSelectedKindsNormalized],
  )
  const requestLogEffectiveKindsKey = useMemo(
    () => requestLogEffectiveKinds.join('\u0001'),
    [requestLogEffectiveKinds],
  )
  const requestLogHasEmptyMatch = useMemo(
    () => hasActiveRequestKindQuickFilters(requestLogQuickFilters) && requestLogQuickSelection.length === 0,
    [requestLogQuickFilters, requestLogQuickSelection.length],
  )
  const requestLogResultFilter =
    requestLogOutcomeFilter?.kind === 'result'
      ? (requestLogOutcomeFilter.value as 'success' | 'error' | 'neutral' | 'quota_exhausted')
      : undefined
  const requestLogKeyEffectFilter =
    requestLogOutcomeFilter?.kind === 'keyEffect' ? requestLogOutcomeFilter.value : undefined
  const requestLogBindingEffectFilter =
    requestLogOutcomeFilter?.kind === 'bindingEffect' ? requestLogOutcomeFilter.value : undefined
  const requestLogSelectionEffectFilter =
    requestLogOutcomeFilter?.kind === 'selectionEffect' ? requestLogOutcomeFilter.value : undefined

  // Logs list: cursor pagination, list first and catalog after
  useEffect(() => {
    if (!(route.name === 'module' && route.module === 'requests')) {
      return
    }
    const request = beginManagedRequest(requestsAbortRef)
    setRequestsLoadState(getBlockingLoadState(requestsLoadedRef.current))
    setRequestsError(null)
    setLogs([])
    setLogsPageInfo((current) => ({ ...current, items: [] }))
    setExpandedLogs(new Set())

    if (requestLogHasEmptyMatch) {
      setLogs([])
      setLogsPageInfo({
        ...emptyRequestLogsListPage,
        pageSize: logsPerPage,
      })
      setRequestsLoadState('ready')
      requestsLoadedRef.current = true
      request.cleanup()
      return () => {
        request.abort()
        request.cleanup()
      }
    }

    fetchRequestLogsList(
      {
        limit: logsPerPage,
        cursor: logsCursor,
        direction: logsDirection,
        requestKinds: requestLogEffectiveKinds,
        result: requestLogResultFilter,
        keyEffect: requestLogKeyEffectFilter,
        bindingEffect: requestLogBindingEffectFilter,
        selectionEffect: requestLogSelectionEffectFilter,
        keyId: requestLogKeyFilter ?? undefined,
      },
      request.signal,
    )
      .then((result) => {
        if (request.signal.aborted) return
        setLogs(result.items)
        setLogsPageInfo(result)
        setRequestsLoadState('ready')
        requestsLoadedRef.current = true
      })
      .catch((err) => {
        if (request.signal.aborted) return
        console.error(err)
        setLogs([])
        setLogsPageInfo({
          ...emptyRequestLogsListPage,
          pageSize: logsPerPage,
        })
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
  }, [
    beginManagedRequest,
    logsPerPage,
    logsCursor,
    logsDirection,
    route,
    requestLogEffectiveKindsKey,
    requestLogHasEmptyMatch,
    requestLogKeyEffectFilter,
    requestLogBindingEffectFilter,
    requestLogSelectionEffectFilter,
    requestLogKeyFilter,
    requestLogResultFilter,
  ])

  useEffect(() => {
    if (!(route.name === 'module' && route.module === 'requests')) {
      return
    }
    const controller = new AbortController()
    const catalogPlan = buildRequestLogsCatalogPlan({})

    fetchRequestLogsCatalog(catalogPlan.query, controller.signal)
      .then((catalog) => {
        if (controller.signal.aborted) return
        setRequestLogsCatalog(catalog)
        setRequestLogRequestKindOptions(catalog.requestKindOptions)
        setRequestLogFacets(catalog.facets)
      })
      .catch((err) => {
        if ((err as Error).name === 'AbortError') return
        console.error(err)
        setRequestLogsCatalog(null)
        setRequestLogRequestKindOptions([])
        setRequestLogFacets(emptyRequestLogFacets)
      })

    return () => controller.abort()
  }, [
    route,
  ])

  const handleRequestLogQuickFilters = useCallback(
    (billing: TokenLogRequestKindQuickBilling, protocol: TokenLogRequestKindQuickProtocol) => {
      const nextFilters = { billing, protocol }
      setRequestLogQuickBilling(billing)
      setRequestLogQuickProtocol(protocol)
      setRequestLogSelectedKinds(
        buildRequestKindQuickFilterSelection(requestLogRequestKindOptions, nextFilters),
      )
      setLogsCursor(null)
      setLogsDirection('older')
    },
    [requestLogRequestKindOptions],
  )

  const handleToggleRequestLogKind = useCallback(
    (key: string) => {
      const nextSelected = toggleRequestKindSelection(requestLogEffectiveKinds, key)
      const nextQuickFilters = resolveManualRequestKindQuickFilters(
        nextSelected,
        requestLogQuickFilters,
        requestLogQuickSelection,
        requestLogRequestKindOptions,
      )
      setRequestLogSelectedKinds(nextSelected)
      setRequestLogQuickBilling(nextQuickFilters.billing)
      setRequestLogQuickProtocol(nextQuickFilters.protocol)
      setLogsCursor(null)
      setLogsDirection('older')
    },
    [
      requestLogEffectiveKinds,
      requestLogQuickFilters,
      requestLogQuickSelection,
      requestLogRequestKindOptions,
    ],
  )

  const handleClearRequestLogKinds = useCallback(() => {
    setRequestLogSelectedKinds([])
    setRequestLogQuickBilling(defaultTokenLogRequestKindQuickFilters.billing)
    setRequestLogQuickProtocol(defaultTokenLogRequestKindQuickFilters.protocol)
    setLogsCursor(null)
    setLogsDirection('older')
  }, [])

  const handleRequestLogOutcomeFilter = useCallback((value: RecentRequestsOutcomeFilter | null) => {
    setRequestLogOutcomeFilter(value)
    setLogsCursor(null)
    setLogsDirection('older')
  }, [])

  const handleRequestLogKeyFilter = useCallback((value: string | null) => {
    setRequestLogKeyFilter(value)
    setLogsCursor(null)
    setLogsDirection('older')
  }, [])

  const loadRequestLogBodies = useCallback(
    (log: RequestLog, signal: AbortSignal) => fetchRequestLogDetails(log.id, signal),
    [],
  )

  // Jobs list: refetch when filter or page changes
  useEffect(() => {
    if (!(route.name === 'module' && route.module === 'jobs')) {
      return
    }
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
          setJobsGroupCounts(result.groupCounts)
          setJobsLoadState('ready')
          jobsLoadedRef.current = true
        }
      })
      .catch(() => {
        if (!request.signal.aborted) {
          setJobs([])
          setJobsTotal(0)
          setJobsGroupCounts(emptyAdminJobGroupCounts())
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
  }, [beginManagedRequest, jobFilter, jobsPage, route])

  useEffect(() => {
    if (!(route.name === 'module' && route.module === 'keys')) return

    const request = beginManagedRequest(keysAbortRef)
    const nextQueryKey = [
      keysPage,
      keysPerPage,
      selectedKeyGroups.join('\u0000'),
      selectedKeyStatuses.join('\u0000'),
      selectedKeyRegistrationIp,
      selectedKeyRegions.join('\u0000'),
    ].join(':')
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
        registrationIp: selectedKeyRegistrationIp,
        regions: selectedKeyRegions,
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
        setKeyRegionFacets(result.facets.regions)
        setKeysLoadState('ready')
        keysLoadedRef.current = true
        keysQueryKeyRef.current = nextQueryKey
        const normalizedLocation = buildAdminKeysPath({
          page: result.page,
          perPage: result.perPage,
          groups: selectedKeyGroups,
          statuses: selectedKeyStatuses,
          registrationIp: selectedKeyRegistrationIp,
          regions: selectedKeyRegions,
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
        setKeyRegionFacets([])
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
  }, [
    beginManagedRequest,
    keysPage,
    keysPerPage,
    loadingStateStrings.error,
    route,
    selectedKeyGroups,
    selectedKeyRegistrationIp,
    selectedKeyRegions,
    selectedKeyStatuses,
  ])

  useEffect(() => {
    const usersRouteActive =
      isUsersCollectionRoute || route.name === 'user'
    if (!usersRouteActive) return

    const request = beginManagedRequest(usersAbortRef)
    const nextQueryKey = [
      usersPage,
      usersQuery,
      usersTagFilterId ?? '',
      usersSort ?? '',
      usersSortOrder ?? '',
    ].join(':')
    const sameQueryRefresh = usersLoadedRef.current && usersQueryKeyRef.current === nextQueryKey
    setUsersLoadState(
      sameQueryRefresh ? getRefreshingLoadState(true) : getBlockingLoadState(usersLoadedRef.current),
    )
    setUsersError(null)
    if (!sameQueryRefresh) {
      setUsers([])
      setUsersTotal(0)
    }
    fetchAdminUsers(usersPage, USERS_PER_PAGE, usersQuery, usersTagFilterId, usersSort, usersSortOrder, request.signal)
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
  }, [
    beginManagedRequest,
    isUsersCollectionRoute,
    route,
    usersPage,
    usersQuery,
    usersSort,
    usersSortOrder,
    usersTagFilterId,
  ])

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
      isUsersCollectionRoute
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
  }, [adminStrings.users.registration.loadFailed, isUsersCollectionRoute, route])

  useEffect(() => {
    if (!isUsersCollectionRoute) return
    const locationQuery = getAdminUsersQueryFromLocation()
    const locationTagFilterId = getAdminUsersTagFilterFromLocation()
    const locationPage = getAdminUsersPageFromLocation()
    const locationSort = getAdminUsersSortFromLocation()
    const locationSortOrder = getAdminUsersSortDirectionFromLocation()
    const normalizedSort = isUsersModuleRoute && !isAdminUsersOverviewSortField(locationSort)
      ? null
      : locationSort
    const normalizedSortOrder = normalizedSort ? locationSortOrder : null
    setUsersPage((previous) => (previous === locationPage ? previous : locationPage))
    setUsersQueryInput((previous) => (previous === locationQuery ? previous : locationQuery))
    setUsersQuery((previous) => (previous === locationQuery ? previous : locationQuery))
    setUsersTagFilterId((previous) => (previous === locationTagFilterId ? previous : locationTagFilterId))
    setUsersSort((previous) => (previous === normalizedSort ? previous : normalizedSort))
    setUsersSortOrder((previous) => (previous === normalizedSortOrder ? previous : normalizedSortOrder))
  }, [isUserUsageRoute, isUsersCollectionRoute, isUsersModuleRoute, route])

  useEffect(() => {
    if (route.name !== 'unbound-token-usage') return
    const locationQuery = getAdminUnboundTokenUsageQueryFromLocation()
    const locationPage = getAdminUnboundTokenUsagePageFromLocation()
    const locationSort = getAdminUnboundTokenUsageSortFromLocation()
    const locationSortOrder = getAdminUnboundTokenUsageSortDirectionFromLocation()
    setUnboundTokenUsagePage((previous) => (previous === locationPage ? previous : locationPage))
    setUnboundTokenUsageQueryInput((previous) => (previous === locationQuery ? previous : locationQuery))
    setUnboundTokenUsageQuery((previous) => (previous === locationQuery ? previous : locationQuery))
    setUnboundTokenUsageSort((previous) => (previous === locationSort ? previous : locationSort))
    setUnboundTokenUsageSortOrder((previous) => (previous === locationSortOrder ? previous : locationSortOrder))
  }, [route])

  useEffect(() => {
    if (!(route.name === 'module' && route.module === 'keys')) return
    const locationPage = getAdminKeysPageFromLocation()
    const locationPerPage = getAdminKeysPerPageFromLocation()
    const locationGroups = getAdminKeysValuesFromLocation('group')
    const locationStatuses = getAdminKeysValuesFromLocation('status')
    const locationRegistrationIp = getAdminKeysRegistrationIpFromLocation()
    const locationRegions = getAdminKeysRegionsFromLocation()
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
    setSelectedKeyRegistrationIp((previous) => (previous === locationRegistrationIp ? previous : locationRegistrationIp))
    setKeyRegistrationIpInput((previous) => (previous === locationRegistrationIp ? previous : locationRegistrationIp))
    setSelectedKeyRegions((previous) =>
      previous.length === locationRegions.length && previous.every((value, index) => value === locationRegions[index])
        ? previous
        : locationRegions,
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
        setUserDetailRevision((current) => current + 1)
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

  useEffect(() => {
    if (!monthlyBrokenDrawer) return
    const controller = new AbortController()
    setMonthlyBrokenDrawerLoadState('initial_loading')
    setMonthlyBrokenDrawerError(null)
    fetchAllMonthlyBrokenKeyItems(
      (page, signal) =>
        monthlyBrokenDrawer.subjectKind === 'user'
          ? fetchAdminUserBrokenKeys(
              monthlyBrokenDrawer.id,
              page,
              MONTHLY_BROKEN_DRAWER_PAGE_SIZE,
              signal,
            )
          : fetchTokenBrokenKeys(
              monthlyBrokenDrawer.id,
              page,
              MONTHLY_BROKEN_DRAWER_PAGE_SIZE,
              signal,
            ),
      controller.signal,
    )
      .then((items) => {
        if (controller.signal.aborted) return
        setMonthlyBrokenDrawerItems(items)
        setMonthlyBrokenDrawerLoadState('ready')
      })
      .catch((err) => {
        if (controller.signal.aborted) return
        console.error(err)
        setMonthlyBrokenDrawerItems([])
        setMonthlyBrokenDrawerError(err instanceof Error ? err.message : loadingStateStrings.error)
        setMonthlyBrokenDrawerLoadState('error')
      })
    return () => controller.abort()
  }, [loadingStateStrings.error, monthlyBrokenDrawer])

  // Automatic fallback polling when SSE is not connected
  useEffect(() => {
    if (sseConnected || !sseFallbackActive) {
      if (pollingTimerRef.current != null) {
        window.clearInterval(pollingTimerRef.current)
        pollingTimerRef.current = null
      }
      return
    }

    if (pollingTimerRef.current == null) {
      const refreshFallback = () => {
        const controller = new AbortController()
        const tasks: Array<Promise<unknown>> =
          route.name === 'module' && route.module === 'dashboard'
            ? [
                loadShellData({ signal: controller.signal, reason: 'refresh' }),
                loadDashboardOverview(controller.signal),
              ]
            : [loadData({ signal: controller.signal, reason: 'refresh' })]
        if (route.name === 'unbound-token-usage') {
          tasks.push(loadUnboundTokenUsage({ signal: controller.signal, reason: 'refresh' }))
        }
        if (route.name === 'module' && route.module === 'alerts') {
          setAlertsRefreshToken((current) => current + 1)
        }
        void Promise.all(tasks).finally(() => controller.abort())
      }

      refreshFallback()
      pollingTimerRef.current = window.setInterval(() => {
        refreshFallback()
      }, REFRESH_INTERVAL_MS) as unknown as number
    }

    return () => {
      if (pollingTimerRef.current != null) {
        window.clearInterval(pollingTimerRef.current)
        pollingTimerRef.current = null
      }
    }
  }, [sseConnected, sseFallbackActive, loadData, loadDashboardOverview, loadShellData, loadUnboundTokenUsage, route])

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
    let reconnectTimer: number | null = null

    const scheduleReconnect = () => {
      if (reconnectTimer != null) {
        return
      }
      reconnectTimer = window.setTimeout(() => {
        reconnectTimer = null
        connect()
      }, 1000) as unknown as number
    }

    const connect = () => {
      if (es) {
        try { es.close() } catch {}
        es = null
      }
      es = new EventSource('/api/events')
      es.onopen = () => {
        setSseConnected(true)
        setSseFallbackActive(false)
      }
      es.onerror = () => {
        // Trigger fallback polling; attempt reconnect automatically
        setSseConnected(false)
        setSseFallbackActive(true)
      }
      es.addEventListener('degraded', () => {
        setSseConnected(false)
        setSseFallbackActive(true)
        if (es) {
          try { es.close() } catch {}
          es = null
        }
        scheduleReconnect()
      })
      es.addEventListener('snapshot', (ev: MessageEvent) => {
        try {
          const data = JSON.parse(ev.data) as DashboardSnapshotEvent
          setSseConnected(true)
          setSummary(data.summary)
          if (routeRef.current.name === 'module' && routeRef.current.module === 'dashboard') {
            dashboardOverviewVersionRef.current += 1
            setDashboardSummaryWindows(data.summaryWindows)
            setDashboardSiteStatusSnapshot(data.siteStatus)
            setDashboardTokens(data.disabledTokens)
            setDashboardTokenCoverage(data.tokenCoverage)
            setDashboardTrend(data.trend)
            setDashboardHourlyRequestWindow(data.hourlyRequestWindow)
            setDashboardKeys(data.exhaustedKeys)
            setDashboardLogs(data.recentLogs)
            setDashboardJobs(data.recentJobs)
            setDashboardOverviewLoaded(true)
          }
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
      if (reconnectTimer != null) {
        window.clearTimeout(reconnectTimer)
        reconnectTimer = null
      }
      if (es) {
        try { es.close() } catch {}
      }
      setSseConnected(false)
      setSseFallbackActive(false)
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
    (target: AdminNavTarget) => {
      if (target === 'user-usage') {
        navigateToPath(
          userUsagePath(
            getAdminUsersQueryFromLocation(),
            getAdminUsersTagFilterFromLocation(),
            getAdminUsersPageFromLocation(),
            getAdminUsersSortFromLocation(),
            getAdminUsersSortDirectionFromLocation(),
          ),
        )
        return
      }
      if (target === 'alerts') {
        navigateToPath(alertsPath())
        return
      }
      navigateToPath(modulePath(target))
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
            registrationIp: selectedKeyRegistrationIp,
            regions: selectedKeyRegions,
          }),
        )
        return
      }
      navigateToPath(keyDetailPath(id))
    },
    [
      keysPage,
      keysPerPage,
      navigateToPath,
      selectedKeyGroups,
      selectedKeyRegistrationIp,
      selectedKeyRegions,
      selectedKeyStatuses,
    ],
  )

  const navigateToken = useCallback(
    (id: string, options?: { preserveUnboundUsageContext?: boolean }) => {
      if (options?.preserveUnboundUsageContext) {
        navigateToPath(
          tokenDetailPath(
            id,
            unboundTokenUsageQuery,
            unboundTokenUsagePage,
            unboundTokenUsageSort,
            unboundTokenUsageSortOrder,
            'unbound-usage',
          ),
        )
        return
      }
      navigateToPath(tokenDetailPath(id))
    },
    [
      navigateToPath,
      unboundTokenUsagePage,
      unboundTokenUsageQuery,
      unboundTokenUsageSort,
      unboundTokenUsageSortOrder,
    ],
  )

  const navigateBackFromTokenDetail = useCallback(() => {
    if (getAdminTokensCollectionFromLocation() === 'unbound-usage') {
      navigateToPath(
        unboundTokenUsagePath(
          getAdminUnboundTokenUsageQueryFromLocation(),
          getAdminUnboundTokenUsagePageFromLocation(),
          getAdminUnboundTokenUsageSortFromLocation(),
          getAdminUnboundTokenUsageSortDirectionFromLocation(),
        ),
      )
      return
    }
    navigateModule('tokens')
  }, [navigateModule, navigateToPath])

  const openRequestKeyDrawer = useCallback((id: string) => {
    setRequestEntityDrawer({ kind: 'key', id })
  }, [])

  const openRequestTokenDrawer = useCallback((id: string) => {
    setRequestEntityDrawer({ kind: 'token', id })
  }, [])

  const closeRequestEntityDrawer = useCallback(() => {
    setRequestEntityDrawer(null)
  }, [])

  const navigateUser = useCallback(
    (id: string, options?: { preserveUsersContext?: boolean }) => {
      if (options?.preserveUsersContext) {
        const collection = isUserUsageRoute || getAdminUsersCollectionFromLocation() === 'usage' ? 'usage' : undefined
        navigateToPath(userDetailPath(id, usersQuery, usersTagFilterId, usersPage, usersSort, usersSortOrder, collection))
        return
      }
      navigateToPath(userDetailPath(id))
    },
    [isUserUsageRoute, navigateToPath, usersPage, usersQuery, usersSort, usersSortOrder, usersTagFilterId],
  )

  const buildUsersCollectionPath = useCallback(
    (
      query?: string,
      tagId?: string | null,
      page?: number | null,
      sort?: AdminUsersSortField | null,
      order?: SortDirection | null,
    ) => (
      isUserUsageRoute || getAdminUsersCollectionFromLocation() === 'usage'
        ? userUsagePath(query, tagId, page, sort, order)
        : buildAdminUsersPath(query, tagId, page, sort, order)
    ),
    [isUserUsageRoute],
  )

  const navigateUsersSearch = useCallback(
    (
      query: string,
      options?: {
        tagId?: string | null
        page?: number | null
        sort?: AdminUsersSortField | null
        order?: SortDirection | null
      },
    ) => {
      const normalized = query.trim()
      const normalizedTagId = options?.tagId?.trim() ?? null
      const normalizedPage = options?.page != null ? Math.max(1, options.page) : 1
      const hasExplicitSort = options != null && Object.prototype.hasOwnProperty.call(options, 'sort')
      const hasExplicitOrder = options != null && Object.prototype.hasOwnProperty.call(options, 'order')
      const normalizedSort = hasExplicitSort ? options?.sort ?? null : usersSort
      const normalizedOrder = normalizedSort
        ? hasExplicitOrder
          ? options?.order ?? ADMIN_USERS_DEFAULT_SORT_ORDER
          : usersSortOrder ?? ADMIN_USERS_DEFAULT_SORT_ORDER
        : null
      setUsersPage(normalizedPage)
      setUsersQueryInput(normalized)
      setUsersQuery(normalized)
      setUsersTagFilterId(normalizedTagId)
      setUsersSort(normalizedSort)
      setUsersSortOrder(normalizedOrder)
      navigateToPath(buildUsersCollectionPath(normalized, normalizedTagId, normalizedPage, normalizedSort, normalizedOrder))
    },
    [buildUsersCollectionPath, navigateToPath, usersSort, usersSortOrder],
  )

  const navigateUnboundTokenUsageSearch = useCallback(
    (
      query: string,
      options?: {
        page?: number | null
        sort?: AdminUnboundTokenUsageSortField | null
        order?: SortDirection | null
      },
    ) => {
      const normalized = query.trim()
      const normalizedPage = options?.page != null ? Math.max(1, options.page) : 1
      const hasExplicitSort = options != null && Object.prototype.hasOwnProperty.call(options, 'sort')
      const hasExplicitOrder = options != null && Object.prototype.hasOwnProperty.call(options, 'order')
      const normalizedSort = hasExplicitSort ? options?.sort ?? null : unboundTokenUsageSort
      const normalizedOrder = normalizedSort
        ? hasExplicitOrder
          ? options?.order ?? ADMIN_UNBOUND_TOKEN_USAGE_DEFAULT_SORT_ORDER
          : unboundTokenUsageSortOrder ?? ADMIN_UNBOUND_TOKEN_USAGE_DEFAULT_SORT_ORDER
        : null
      setUnboundTokenUsagePage(normalizedPage)
      setUnboundTokenUsageQueryInput(normalized)
      setUnboundTokenUsageQuery(normalized)
      setUnboundTokenUsageSort(normalizedSort)
      setUnboundTokenUsageSortOrder(normalizedOrder)
      navigateToPath(unboundTokenUsagePath(normalized, normalizedPage, normalizedSort, normalizedOrder))
    },
    [navigateToPath, unboundTokenUsageSort, unboundTokenUsageSortOrder],
  )

  const navigateKeysList = useCallback(
    (options?: {
      page?: number | null
      perPage?: number | null
      groups?: string[] | null
      statuses?: string[] | null
      registrationIp?: string | null
      regions?: string[] | null
    }) => {
      const page = options?.page != null ? Math.max(1, Math.trunc(options.page)) : 1
      const perPage = options?.perPage != null ? Math.max(1, Math.trunc(options.perPage)) : DEFAULT_KEYS_PER_PAGE
      const groups = Array.from(new Set((options?.groups ?? []).map((value) => value.trim())))
      const statuses = Array.from(
        new Set((options?.statuses ?? []).map((value) => value.trim()).filter((value) => value.length > 0)),
      )
      const registrationIp = options?.registrationIp?.trim() ?? ''
      const regions = Array.from(
        new Set((options?.regions ?? []).map((value) => value.trim()).filter((value) => value.length > 0)),
      )

      setKeysPage(page)
      setKeysPerPage(perPage)
      setSelectedKeyGroups(groups)
      setSelectedKeyStatuses(statuses)
      setSelectedKeyRegistrationIp(registrationIp)
      setKeyRegistrationIpInput(registrationIp)
      setSelectedKeyRegions(regions)
      navigateToPath(
        buildAdminKeysPath({
          page,
          perPage,
          groups,
          statuses,
          registrationIp,
          regions,
        }),
      )
    },
    [navigateToPath],
  )

  useEffect(() => {
    if (!(route.name === 'module' && route.module === 'keys')) return
    const timer = window.setTimeout(() => {
      const normalized = keyRegistrationIpInput.trim()
      if (normalized === selectedKeyRegistrationIp) return
      navigateKeysList({
        page: 1,
        perPage: keysPerPage,
        groups: selectedKeyGroups,
        statuses: selectedKeyStatuses,
        registrationIp: normalized,
        regions: selectedKeyRegions,
      })
    }, 250)
    return () => window.clearTimeout(timer)
  }, [
    keyRegistrationIpInput,
    keysPerPage,
    navigateKeysList,
    route,
    selectedKeyGroups,
    selectedKeyRegistrationIp,
    selectedKeyRegions,
    selectedKeyStatuses,
  ])

  const navigateUserTags = useCallback(() => {
    const query = route.name === 'module' && route.module === 'users' ? usersQuery : getAdminUsersQueryFromLocation()
    const tagId = route.name === 'module' && route.module === 'users'
      ? usersTagFilterId
      : getAdminUsersTagFilterFromLocation()
    const page = route.name === 'module' && route.module === 'users' ? usersPage : getAdminUsersPageFromLocation()
    const sort = route.name === 'module' && route.module === 'users' ? usersSort : getAdminUsersSortFromLocation()
    const order = route.name === 'module' && route.module === 'users' ? usersSortOrder : getAdminUsersSortDirectionFromLocation()
    const collection = isUserUsageRoute || getAdminUsersCollectionFromLocation() === 'usage' ? 'usage' : undefined
    navigateToPath(userTagsPath(query, tagId, page, sort, order, collection))
  }, [isUserUsageRoute, navigateToPath, route, usersPage, usersQuery, usersSort, usersSortOrder, usersTagFilterId])

  const navigateUserTagCreate = useCallback(() => {
    setActiveUserTagEditorId(NEW_USER_TAG_CARD_ID)
    setUserTagCatalogDraft({ ...EMPTY_USER_TAG_FORM })
    setTagCatalogError(null)
    navigateToPath(
      userTagCreatePath(
        getAdminUsersQueryFromLocation(),
        getAdminUsersTagFilterFromLocation(),
        getAdminUsersPageFromLocation(),
        getAdminUsersSortFromLocation(),
        getAdminUsersSortDirectionFromLocation(),
        getAdminUsersCollectionFromLocation(),
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
          getAdminUsersSortFromLocation(),
          getAdminUsersSortDirectionFromLocation(),
          getAdminUsersCollectionFromLocation(),
        ),
      )
    },
    [navigateToPath, tagCatalog],
  )

  const navigateUnboundTokenUsage = useCallback(() => {
    navigateToPath(unboundTokenUsagePath())
  }, [navigateToPath])

  const handleManualRefresh = () => {
    const controller = new AbortController()
    setLoading(true)
    if (route.name === 'module' && route.module === 'keys') {
      clearKeySelection()
      setKeysBulkFeedback(null)
    }
    const tasks: Array<Promise<unknown>> =
      route.name === 'module' && route.module === 'dashboard'
        ? [
            loadShellData({ signal: controller.signal, reason: 'refresh' }),
            loadDashboardOverview(controller.signal),
          ]
        : [loadData({ signal: controller.signal, reason: 'refresh', showGlobalLoading: true })]
    if (route.name === 'unbound-token-usage') {
      tasks.push(loadUnboundTokenUsage({ signal: controller.signal, reason: 'refresh' }))
    }
    if (route.name === 'module' && route.module === 'alerts') {
      setAlertsRefreshToken((current) => current + 1)
    }
    if (route.name === 'module' && route.module === 'system-settings') {
      tasks.push(loadSystemSettingsData({ signal: controller.signal, reason: 'refresh' }))
    }
    if (route.name === 'module' && route.module === 'requests') {
      const request = beginManagedRequest(requestsAbortRef, controller.signal)
      setRequestsLoadState(getRefreshingLoadState(requestsLoadedRef.current))
      setRequestsError(null)
      const listPlan = buildRequestLogsListPlan({
        limit: logsPerPage,
        cursor: logsCursor,
        direction: logsDirection,
        hasEmptyMatch: requestLogHasEmptyMatch,
        requestKinds: requestLogEffectiveKinds,
        result: requestLogResultFilter,
        keyEffect: requestLogKeyEffectFilter,
        bindingEffect: requestLogBindingEffectFilter,
        selectionEffect: requestLogSelectionEffectFilter,
        keyId: requestLogKeyFilter,
      })
      if (listPlan.kind === 'empty') {
        setLogs([])
        setLogsPageInfo({
          ...emptyRequestLogsListPage,
          pageSize: logsPerPage,
        })
        setRequestsLoadState('ready')
        request.cleanup()
      } else {
        tasks.push(
          fetchRequestLogsList(listPlan.query, request.signal)
            .then((result) => {
              if (request.signal.aborted) return
              setLogs(result.items)
              setLogsPageInfo(result)
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
      const catalogPlan = buildRequestLogsCatalogPlan({})
      tasks.push(
        fetchRequestLogsCatalog(catalogPlan.query, controller.signal)
          .then((catalog) => {
            if (controller.signal.aborted) return
            setRequestLogsCatalog(catalog)
            setRequestLogRequestKindOptions(catalog.requestKindOptions)
            setRequestLogFacets(catalog.facets)
          })
          .catch((err) => {
            if ((err as Error).name === 'AbortError' || controller.signal.aborted) return
            console.error(err)
            setRequestLogsCatalog(null)
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
          setJobsGroupCounts(result.groupCounts)
          setJobsLoadState('ready')
        }).catch((err) => {
          if (request.signal.aborted) return
          console.error(err)
          setJobsGroupCounts(emptyAdminJobGroupCounts())
          setJobsError(err instanceof Error ? err.message : loadingStateStrings.error)
          setJobsLoadState('error')
        }).finally(() => {
          request.cleanup()
        }),
      )
    }
    if (isUsersCollectionRoute || route.name === 'user') {
      const request = beginManagedRequest(usersAbortRef, controller.signal)
      setUsersLoadState(getRefreshingLoadState(usersLoadedRef.current))
      setUsersError(null)
      tasks.push(
        fetchAdminUsers(
          usersPage,
          USERS_PER_PAGE,
          usersQuery,
          usersTagFilterId,
          usersSort,
          usersSortOrder,
          request.signal,
        ).then((result) => {
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
    if (route.name === 'module' && route.module === 'proxy-settings') {
      tasks.push(
        loadForwardProxySettingsData({ signal: controller.signal, reason: 'refresh' }),
        loadForwardProxyStatsData({ signal: controller.signal, reason: 'refresh' }),
      )
    }
    if (route.name === 'module' && route.module === 'keys') {
      const request = beginManagedRequest(keysAbortRef, controller.signal)
      const nextQueryKey = [
        keysPage,
        keysPerPage,
        selectedKeyGroups.join('\u0000'),
        selectedKeyStatuses.join('\u0000'),
        selectedKeyRegistrationIp,
        selectedKeyRegions.join('\u0000'),
      ].join(':')
      setKeysLoadState(getRefreshingLoadState(keysLoadedRef.current))
      setKeysError(null)
      tasks.push(
        fetchApiKeys(
          keysPage,
          keysPerPage,
          {
            groups: selectedKeyGroups,
            statuses: selectedKeyStatuses,
            registrationIp: selectedKeyRegistrationIp,
            regions: selectedKeyRegions,
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
            setKeyRegionFacets(result.facets.regions)
            setKeysLoadState('ready')
            keysLoadedRef.current = true
            keysQueryKeyRef.current = nextQueryKey
            const normalizedLocation = buildAdminKeysPath({
              page: result.page,
              perPage: result.perPage,
              groups: selectedKeyGroups,
              statuses: selectedKeyStatuses,
              registrationIp: selectedKeyRegistrationIp,
              regions: selectedKeyRegions,
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
    void Promise.all(tasks).finally(() => {
      if (!controller.signal.aborted) {
        setLoading(false)
      }
      controller.abort()
    })
  }

  const todayMetrics = useMemo(() => {
    if (!dashboardSummaryWindows) {
      return []
    }

    return createDashboardTodayMetrics({
      today: dashboardSummaryWindows.today,
      yesterday: dashboardSummaryWindows.yesterday,
      labels: {
        total: metricsStrings.labels.total,
        success: metricsStrings.labels.success,
        failure: metricsStrings.labels.failure,
        unknownCalls: metricsStrings.labels.unknownCalls,
        upstreamExhausted: adminStrings.dashboard.upstreamExhaustedLabel,
        valuableTag: adminStrings.dashboard.valuableTag,
        otherTag: adminStrings.dashboard.otherTag,
        unknownTag: adminStrings.dashboard.unknownTag,
      },
      strings: {
        deltaFromYesterday: adminStrings.dashboard.deltaFromYesterday,
        deltaNoBaseline: adminStrings.dashboard.deltaNoBaseline,
        percentagePointUnit: adminStrings.dashboard.percentagePointUnit,
        asOfNow: adminStrings.dashboard.asOfNow,
        todayShare: adminStrings.dashboard.todayShare,
        todayAdded: adminStrings.dashboard.todayAdded,
      },
      formatters: {
        formatNumber,
        formatPercent,
      },
    })
  }, [
    adminStrings.dashboard.asOfNow,
    adminStrings.dashboard.deltaFromYesterday,
    adminStrings.dashboard.deltaNoBaseline,
    adminStrings.dashboard.percentagePointUnit,
    adminStrings.dashboard.todayAdded,
    adminStrings.dashboard.otherTag,
    adminStrings.dashboard.todayShare,
    adminStrings.dashboard.unknownTag,
    adminStrings.dashboard.upstreamExhaustedLabel,
    adminStrings.dashboard.valuableTag,
    dashboardSummaryWindows,
    metricsStrings.labels.failure,
    metricsStrings.labels.success,
    metricsStrings.labels.total,
    metricsStrings.labels.unknownCalls,
  ])

  const monthMetrics = useMemo(() => {
    if (!dashboardSummaryWindows) {
      return []
    }

    return createDashboardMonthMetrics({
      month: dashboardSummaryWindows.month,
      labels: {
        total: metricsStrings.labels.total,
        success: metricsStrings.labels.success,
        failure: metricsStrings.labels.failure,
        unknownCalls: metricsStrings.labels.unknownCalls,
        upstreamExhausted: adminStrings.dashboard.upstreamExhaustedLabel,
        valuableTag: adminStrings.dashboard.valuableTag,
        otherTag: adminStrings.dashboard.otherTag,
        unknownTag: adminStrings.dashboard.unknownTag,
        newKeys: metricsStrings.labels.newKeys,
        newQuarantines: metricsStrings.labels.newQuarantines,
      },
      strings: {
        monthToDate: adminStrings.dashboard.monthToDate,
        monthShare: adminStrings.dashboard.monthShare,
        monthAdded: adminStrings.dashboard.monthAdded,
      },
      formatters: {
        formatNumber,
        formatPercent,
      },
    })
  }, [
    adminStrings.dashboard.monthAdded,
    adminStrings.dashboard.otherTag,
    adminStrings.dashboard.monthShare,
    adminStrings.dashboard.monthToDate,
    adminStrings.dashboard.unknownTag,
    adminStrings.dashboard.upstreamExhaustedLabel,
    adminStrings.dashboard.valuableTag,
    dashboardSummaryWindows,
    metricsStrings.labels.failure,
    metricsStrings.labels.newKeys,
    metricsStrings.labels.newQuarantines,
    metricsStrings.labels.success,
    metricsStrings.labels.total,
    metricsStrings.labels.unknownCalls,
  ])

  const todayQuotaCharge = useMemo(() => {
    if (!dashboardSummaryWindows) {
      return null
    }

    return buildDashboardQuotaChargeCard(dashboardSummaryWindows.today, {
      title: adminStrings.dashboard.quotaChargeTitle,
      localEstimate: adminStrings.dashboard.quotaChargeLocalEstimate,
      upstreamActual: adminStrings.dashboard.quotaChargeUpstreamActual,
      delta: adminStrings.dashboard.quotaChargeDelta,
      sampledKeys: adminStrings.dashboard.quotaChargeSampledKeys,
      staleKeys: adminStrings.dashboard.quotaChargeStaleKeys,
      latestSync: adminStrings.dashboard.quotaChargeLatestSync,
      noSync: adminStrings.dashboard.quotaChargeNoSync,
    })
  }, [adminStrings.dashboard, dashboardSummaryWindows])

  const monthQuotaCharge = useMemo(() => {
    if (!dashboardSummaryWindows) {
      return null
    }

    return buildDashboardQuotaChargeCard(dashboardSummaryWindows.month, {
      title: adminStrings.dashboard.quotaChargeTitle,
      localEstimate: adminStrings.dashboard.quotaChargeLocalEstimate,
      upstreamActual: adminStrings.dashboard.quotaChargeUpstreamActual,
      delta: adminStrings.dashboard.quotaChargeDelta,
      sampledKeys: adminStrings.dashboard.quotaChargeSampledKeys,
      staleKeys: adminStrings.dashboard.quotaChargeStaleKeys,
      latestSync: adminStrings.dashboard.quotaChargeLatestSync,
      noSync: adminStrings.dashboard.quotaChargeNoSync,
    })
  }, [adminStrings.dashboard, dashboardSummaryWindows])

  const statusMetrics = useMemo(() => {
    if (!dashboardSiteStatusSnapshot) {
      return []
    }

    const allKeysAvailable =
      dashboardSiteStatusSnapshot.quarantinedKeys === 0 &&
      dashboardSiteStatusSnapshot.temporaryIsolatedKeys === 0 &&
      dashboardSiteStatusSnapshot.exhaustedKeys === 0

    return [
      {
        id: 'remaining',
        label: metricsStrings.labels.remaining,
        value: formatNumber(dashboardSiteStatusSnapshot.remainingQuota),
        subtitle:
          dashboardSiteStatusSnapshot.totalQuotaLimit > 0
            ? `${adminStrings.dashboard.currentSnapshot} · ${formatPercent(dashboardSiteStatusSnapshot.remainingQuota, dashboardSiteStatusSnapshot.totalQuotaLimit)}`
            : adminStrings.dashboard.currentSnapshot,
      },
      {
        id: 'keys',
        label: metricsStrings.labels.keys,
        value: formatNumber(dashboardSiteStatusSnapshot.activeKeys),
        subtitle: adminStrings.dashboard.currentSnapshot,
      },
      {
        id: 'quarantined',
        label: metricsStrings.labels.quarantined,
        value: formatNumber(dashboardSiteStatusSnapshot.quarantinedKeys),
        subtitle:
          dashboardSiteStatusSnapshot.quarantinedKeys > 0
            ? keyStrings.quarantine.badge
            : allKeysAvailable
              ? metricsStrings.subtitles.keysAll
              : adminStrings.dashboard.currentSnapshot,
      },
      {
        id: 'temporary-isolated',
        label: metricsStrings.labels.temporaryIsolated,
        value: formatNumber(dashboardSiteStatusSnapshot.temporaryIsolatedKeys),
        subtitle:
          dashboardSiteStatusSnapshot.temporaryIsolatedKeys > 0
            ? metricsStrings.subtitles.keysTemporaryIsolated.replace('{count}', formatNumber(dashboardSiteStatusSnapshot.temporaryIsolatedKeys))
            : allKeysAvailable
              ? metricsStrings.subtitles.keysAll
              : adminStrings.dashboard.currentSnapshot,
      },
      {
        id: 'exhausted',
        label: metricsStrings.labels.exhausted,
        value: formatNumber(dashboardSiteStatusSnapshot.exhaustedKeys),
        subtitle:
          allKeysAvailable
            ? metricsStrings.subtitles.keysAll
            : metricsStrings.subtitles.keysExhausted.replace('{count}', formatNumber(dashboardSiteStatusSnapshot.exhaustedKeys)),
      },
      {
        id: 'proxy-available',
        label: metricsStrings.labels.proxyAvailable,
        value:
          dashboardSiteStatusSnapshot.availableProxyNodes == null
            ? '—'
            : formatNumber(dashboardSiteStatusSnapshot.availableProxyNodes),
        subtitle:
          dashboardSiteStatusSnapshot.availableProxyNodes != null &&
          dashboardSiteStatusSnapshot.totalProxyNodes != null &&
          dashboardSiteStatusSnapshot.totalProxyNodes > 0
            ? `${adminStrings.dashboard.currentSnapshot} · ${formatPercent(dashboardSiteStatusSnapshot.availableProxyNodes, dashboardSiteStatusSnapshot.totalProxyNodes)}`
            : adminStrings.dashboard.currentSnapshot,
      },
      {
        id: 'proxy-total',
        label: metricsStrings.labels.proxyTotal,
        value:
          dashboardSiteStatusSnapshot.totalProxyNodes == null
            ? '—'
            : formatNumber(dashboardSiteStatusSnapshot.totalProxyNodes),
        subtitle: adminStrings.dashboard.currentSnapshot,
      },
    ]
  }, [adminStrings.dashboard.currentSnapshot, dashboardSiteStatusSnapshot, keyStrings.quarantine.badge, metricsStrings])

  const dashboardStatusLoading = !dashboardOverviewLoaded

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

  const keyRegionFilterOptions = useMemo(
    () =>
      keyRegionFacets.map((region) => ({
        value: region.value,
        label: region.value,
        count: region.count,
      })),
    [keyRegionFacets],
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
  const selectedVisibleKeyIds = useMemo(
    () => visibleKeys.filter((item) => selectedKeyIds.has(item.id)).map((item) => item.id),
    [selectedKeyIds, visibleKeys],
  )
  const selectedVisibleKeyCount = selectedVisibleKeyIds.length
  const allVisibleKeysSelected = visibleKeys.length > 0 && selectedVisibleKeyCount === visibleKeys.length
  const someVisibleKeysSelected = selectedVisibleKeyCount > 0 && !allVisibleKeysSelected
  const preservedKeysSelectionQueryKeyRef = useRef<string | null>(null)
  const buildKeysSelectionQueryKey = useCallback(
    (page: number, perPage: number) =>
      [
        route.name === 'module' ? route.module : route.name,
        page,
        perPage,
        selectedKeyGroups.join('\u0000'),
        selectedKeyStatuses.join('\u0000'),
        selectedKeyRegistrationIp,
        selectedKeyRegions.join('\u0000'),
      ].join(':'),
    [
      route,
      selectedKeyGroups,
      selectedKeyRegistrationIp,
      selectedKeyRegions,
      selectedKeyStatuses,
    ],
  )
  const keysSelectionQueryKey = useMemo(
    () => buildKeysSelectionQueryKey(keysPage, keysPerPage),
    [buildKeysSelectionQueryKey, keysPage, keysPerPage],
  )
  const latestKeysSelectionQueryKeyRef = useRef(keysSelectionQueryKey)

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

  useEffect(() => {
    if (keysSelectAllRef.current) {
      keysSelectAllRef.current.indeterminate = someVisibleKeysSelected
    }
  }, [someVisibleKeysSelected])

  useEffect(() => {
    latestKeysSelectionQueryKeyRef.current = keysSelectionQueryKey
    if (preservedKeysSelectionQueryKeyRef.current === keysSelectionQueryKey) {
      preservedKeysSelectionQueryKeyRef.current = null
      return
    }
    setSelectedKeyIds(new Set())
    setKeysBulkFeedback(null)
    setBulkSyncProgress(null)
    setBulkSyncBubbleVisible(false)
    setBulkSyncBubblePinnedPosition(null)
  }, [keysSelectionQueryKey])

  const selectedKeyRegionLabels = useMemo(
    () =>
      keyRegionFilterOptions
        .filter((option) => selectedKeyRegions.includes(option.value))
        .map((option) => option.label),
    [keyRegionFilterOptions, selectedKeyRegions],
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
    return extractApiKeyImportEntriesFromText(newKeysText)
  }, [newKeysText])

  const keyRegionFilterSummary = useMemo(
    () =>
      summarizeFilterSelection(
        keyStrings.filters.region,
        selectedKeyRegionLabels,
        keyStrings.groups.all,
        keyStrings.filters.selectedSuffix,
      ),
    [keyStrings.filters.region, keyStrings.filters.selectedSuffix, keyStrings.groups.all, selectedKeyRegionLabels],
  )

  const jobFilterOptions = useMemo(
    () => buildAdminJobFilterOptions(jobsStrings, jobsGroupCounts),
    [jobsGroupCounts, jobsStrings],
  )

  const jobFilterSummary = useMemo(
    () => summarizeAdminJobFilter(jobFilter, jobsStrings),
    [jobFilter, jobsStrings],
  )

  const handleJobFilterChange = useCallback((value: JobGroup) => {
    setJobFilter(value)
    setJobsPage(1)
  }, [])

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
    let existing = 0
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
        case 'already_exists':
          existing += 1
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
    const checked = existing + ok + exhausted + invalid + errorCount
    const totalToCheck = new Set(
      rows
        .filter((row) => row.status !== 'duplicate_in_input')
        .map((row) => row.api_key),
    ).size
    return { pending, duplicate, existing, ok, exhausted, invalid, error: errorCount, checked, totalToCheck }
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

  const tokensBlocking = isBlockingLoadState(tokensLoadState)
  const tokensRefreshing = isRefreshingLoadState(tokensLoadState)
  const requestsBlocking = isBlockingLoadState(requestsLoadState)
  const requestsRefreshing = isRefreshingLoadState(requestsLoadState)
  const jobsBlocking = isBlockingLoadState(jobsLoadState)
  const jobsRefreshing = isRefreshingLoadState(jobsLoadState)
  const usersBlocking = isBlockingLoadState(usersLoadState)
  const usersRefreshing = isRefreshingLoadState(usersLoadState)
  const effectiveUsersSort = usersSort ?? ADMIN_USERS_DEFAULT_SORT_FIELD
  const effectiveUsersSortOrder = usersSortOrder ?? ADMIN_USERS_DEFAULT_SORT_ORDER
  const unboundTokenUsageBlocking = isBlockingLoadState(unboundTokenUsageLoadState)
  const unboundTokenUsageRefreshing = isRefreshingLoadState(unboundTokenUsageLoadState)
  const effectiveUnboundTokenUsageSort =
    unboundTokenUsageSort ?? ADMIN_UNBOUND_TOKEN_USAGE_DEFAULT_SORT_FIELD
  const effectiveUnboundTokenUsageSortOrder =
    unboundTokenUsageSortOrder ?? ADMIN_UNBOUND_TOKEN_USAGE_DEFAULT_SORT_ORDER
  const requestLogsDescription = useMemo(
    () => formatRequestLogsDescription(adminStrings.logs, requestLogsCatalog?.retentionDays),
    [adminStrings.logs, requestLogsCatalog?.retentionDays],
  )
  const requestLogsPaginationSummary = useMemo(
    () => formatRequestLogsPaginationSummary(adminStrings.logs, requestLogsCatalog?.retentionDays),
    [adminStrings.logs, requestLogsCatalog?.retentionDays],
  )
  const systemSettingsBlocking = isBlockingLoadState(systemSettingsLoadState)
  const forwardProxySettingsBlocking = isBlockingLoadState(forwardProxySettingsLoadState)
  const forwardProxyStatsBlocking = isBlockingLoadState(forwardProxyStatsLoadState)
  const activeModuleBlocking =
    (route.name === 'module' && route.module === 'tokens' && tokensBlocking)
    || (route.name === 'module' && route.module === 'requests' && requestsBlocking)
    || (route.name === 'module' && route.module === 'jobs' && jobsBlocking)
    || (isUsersCollectionRoute || route.name === 'user') && usersBlocking
    || (route.name === 'module' && route.module === 'system-settings' && systemSettingsBlocking)
    || (route.name === 'module' && route.module === 'proxy-settings'
      && (forwardProxySettingsBlocking || forwardProxyStatsBlocking))
    || (route.name === 'unbound-token-usage' && unboundTokenUsageBlocking)

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
      case 'already_exists':
        return 'already_exists'
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
      const byKey = new Map<string, ValidateKeyResult>()
      for (const result of results) {
        const current = byKey.get(result.api_key)
        const isUsable =
          result.status !== 'duplicate_in_input' && result.status !== 'pending'
        if (!current || isUsable) {
          byKey.set(result.api_key, result)
        }
      }
      const nextRows = prev.rows.map((row): KeyValidationRow => {
        const res = byKey.get(row.api_key)
        if (row.status === 'duplicate_in_input') {
          return {
            ...row,
            registration_ip: res?.registration_ip ?? row.registration_ip,
            registration_region: res?.registration_region ?? row.registration_region,
            assigned_proxy_key: res ? res.assigned_proxy_key : row.assigned_proxy_key,
            assigned_proxy_label: res ? res.assigned_proxy_label : row.assigned_proxy_label,
            assigned_proxy_match_kind: res
              ? res.assigned_proxy_match_kind
              : row.assigned_proxy_match_kind,
          }
        }
        if (!res) return row
        const status = coerceValidationStatus(res.status)
        return {
          ...row,
          status,
          registration_ip: res.registration_ip ?? row.registration_ip,
          registration_region: res.registration_region ?? row.registration_region,
          assigned_proxy_key: res.assigned_proxy_key,
          assigned_proxy_label: res.assigned_proxy_label,
          assigned_proxy_match_kind: res.assigned_proxy_match_kind,
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
          assigned_proxy_key: undefined,
          assigned_proxy_label: undefined,
          assigned_proxy_match_kind: undefined,
          detail: undefined,
          quota_limit: undefined,
          quota_remaining: undefined,
          attempts: row.attempts + 1,
        }
      })
      return { ...prev, rows }
    })
  }, [])

  const runValidateKeys = useCallback(async (items: { api_key: string; registration_ip?: string | null }[], runId: number) => {
    const controller = new AbortController()
    keysValidateAbortRef.current?.abort()
    keysValidateAbortRef.current = controller

    const CHUNK_SIZE = 25
    for (let i = 0; i < items.length; i += CHUNK_SIZE) {
      const chunk = items.slice(i, i + CHUNK_SIZE)
      if (chunk.length === 0) continue
      try {
        const resp = await validateApiKeys(chunk, controller.signal)
        applyValidationResults(resp.results, runId)
      } catch (err) {
        const message = err instanceof Error ? err.message : 'Failed to validate keys'
        applyValidationResults(
          chunk.map((item) => ({
            api_key: item.api_key,
            registration_ip: item.registration_ip,
            status: 'error',
            detail: message,
          })),
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
    const parsedEntries = extractApiKeyImportEntriesFromText(newKeysText)
    if (parsedEntries.length === 0) return

    const group = newKeysGroup.trim()

    const seen = new Set<string>()
    const rows: KeyValidationRow[] = []
    const uniqueEntries: { api_key: string; registration_ip?: string | null }[] = []
    const registrationIpByKey: Record<string, string> = {}
    let duplicateCount = 0
    for (const entry of parsedEntries) {
      const api_key = entry.api_key
      if (seen.has(api_key)) {
        duplicateCount += 1
        rows.push({
          api_key,
          registration_ip: entry.registration_ip,
          status: 'duplicate_in_input',
          attempts: 0,
        })
        continue
      }
      seen.add(api_key)
      uniqueEntries.push({
        api_key,
        registration_ip: entry.registration_ip,
      })
      if (entry.registration_ip) {
        registrationIpByKey[api_key] = entry.registration_ip
      }
      rows.push({
        api_key,
        registration_ip: entry.registration_ip,
        status: 'pending',
        attempts: 0,
      })
    }

    keysValidateRunIdRef.current = (keysValidateRunIdRef.current ?? 0) + 1
    const runId = keysValidateRunIdRef.current
    setKeysValidation({
      group,
      input_lines: rawLines.length,
      valid_lines: parsedEntries.length,
      unique_in_input: uniqueEntries.length,
      duplicate_in_input: duplicateCount,
      checking: true,
      importing: false,
      rows,
      imported_api_keys: [],
      registration_ip_by_key: registrationIpByKey,
    })

    // Collapse the in-place overlay once we hand off to the dialog.
    setNewKeysText('')
    setNewKeysGroup('')
    beginKeysBatchClose()

    await runValidateKeys(uniqueEntries, runId)
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
    const failedItems = failedKeys.map((api_key) => ({
      api_key,
      registration_ip: keysValidation.registration_ip_by_key[api_key] ?? undefined,
    }))

    keysValidateRunIdRef.current = (keysValidateRunIdRef.current ?? 0) + 1
    const runId = keysValidateRunIdRef.current
    setKeysValidation((prev) => prev ? ({
      ...prev,
      checking: true,
      importError: undefined,
      importWarning: undefined,
    }) : prev)
    markKeysPendingForRetry(failedKeys, runId)
    await runValidateKeys(failedItems, runId)
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
    await runValidateKeys([
      {
        api_key,
        registration_ip: keysValidation.registration_ip_by_key[api_key] ?? undefined,
      },
    ], runId)
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
      const validationRowByKey = new Map<string, KeyValidationRow>()
      for (const row of keysValidation.rows) {
        const current = validationRowByKey.get(row.api_key)
        const isUsable =
          row.status !== 'duplicate_in_input' && row.status !== 'pending'
        if (!current || isUsable) {
          validationRowByKey.set(row.api_key, row)
        }
      }

      for (let i = 0; i < keysValidationValidKeys.length; i += API_KEYS_IMPORT_CHUNK_SIZE) {
        const chunk = keysValidationValidKeys.slice(i, i + API_KEYS_IMPORT_CHUNK_SIZE)
        const exhaustedInChunk = chunk.filter((apiKey) => exhaustedSet.has(apiKey))
        const chunkItems: AddApiKeysBatchItem[] = chunk.map((api_key) => ({
          api_key,
          registration_ip: keysValidation.registration_ip_by_key[api_key] ?? undefined,
          assigned_proxy_key: validationRowByKey.get(api_key)?.assigned_proxy_key ?? undefined,
        }))
        const chunkResponse = await addApiKeysBatch(chunkItems, normalizedGroup, exhaustedInChunk)
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
      const shouldAutoClose = keysValidation.rows.every(
        (row) =>
          imported.has(row.api_key)
          || row.status === 'already_exists'
          || row.status === 'duplicate_in_input',
      )
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
  const keysHasFilters = (
    selectedKeyGroups.length > 0
    || selectedKeyStatuses.length > 0
    || selectedKeyRegions.length > 0
    || selectedKeyRegistrationIp.length > 0
  )
  const keysBlocking = isBlockingLoadState(keysLoadState)
  const keysRefreshing = isRefreshingLoadState(keysLoadState)

  const goPrevPage = () => {
    setTokensPage((p) => Math.max(1, p - 1))
  }
  const goNextPage = () => {
    setTokensPage((p) => Math.min(totalPages, p + 1))
  }

  const usersTotalPages = useMemo(() => Math.max(1, Math.ceil(usersTotal / USERS_PER_PAGE)), [usersTotal])
  const unboundTokenUsageTotalPages = useMemo(
    () => Math.max(1, Math.ceil(unboundTokenUsageTotal / USERS_PER_PAGE)),
    [unboundTokenUsageTotal],
  )

  const goNewerLogsPage = () => {
    if (!logsPageInfo.prevCursor) return
    setLogsCursor(logsPageInfo.prevCursor)
    setLogsDirection('newer')
  }

  const goOlderLogsPage = () => {
    if (!logsPageInfo.nextCursor) return
    setLogsCursor(logsPageInfo.nextCursor)
    setLogsDirection('older')
  }

  const changeLogsPerPage = (value: number) => {
    setLogsPerPage(value)
    setLogsCursor(null)
    setLogsDirection('older')
  }

  const goPrevUsersPage = () => {
    navigateUsersSearch(usersQuery, {
      tagId: usersTagFilterId,
      page: usersPage - 1,
      sort: usersSort,
      order: usersSortOrder,
    })
  }

  const goNextUsersPage = () => {
    navigateUsersSearch(usersQuery, {
      tagId: usersTagFilterId,
      page: usersPage + 1,
      sort: usersSort,
      order: usersSortOrder,
    })
  }

  const applyUserSearch = () => {
    navigateUsersSearch(usersQueryInput, { tagId: usersTagFilterId, page: 1, sort: usersSort, order: usersSortOrder })
  }

  const resetUserSearch = () => {
    navigateUsersSearch('', { tagId: null, page: 1, sort: usersSort, order: usersSortOrder })
  }

  const toggleUsersSort = useCallback(
    (field: AdminUsersSortField) => {
      const isActive = effectiveUsersSort === field
      let nextSort: AdminUsersSortField | null = field
      let nextOrder: SortDirection | null = ADMIN_USERS_DEFAULT_SORT_ORDER
      if (isActive && effectiveUsersSortOrder === 'desc') {
        nextOrder = 'asc'
      } else if (isActive && effectiveUsersSortOrder === 'asc') {
        nextSort = null
        nextOrder = null
      }
      navigateUsersSearch(usersQuery, { tagId: usersTagFilterId, page: 1, sort: nextSort, order: nextOrder })
    },
    [effectiveUsersSort, effectiveUsersSortOrder, navigateUsersSearch, usersQuery, usersTagFilterId],
  )

  const goPrevUnboundTokenUsagePage = () => {
    navigateUnboundTokenUsageSearch(unboundTokenUsageQuery, {
      page: unboundTokenUsagePage - 1,
      sort: unboundTokenUsageSort,
      order: unboundTokenUsageSortOrder,
    })
  }

  const goNextUnboundTokenUsagePage = () => {
    navigateUnboundTokenUsageSearch(unboundTokenUsageQuery, {
      page: unboundTokenUsagePage + 1,
      sort: unboundTokenUsageSort,
      order: unboundTokenUsageSortOrder,
    })
  }

  const applyUnboundTokenUsageSearch = () => {
    navigateUnboundTokenUsageSearch(unboundTokenUsageQueryInput, {
      page: 1,
      sort: unboundTokenUsageSort,
      order: unboundTokenUsageSortOrder,
    })
  }

  const resetUnboundTokenUsageSearch = () => {
    navigateUnboundTokenUsageSearch('', {
      page: 1,
      sort: unboundTokenUsageSort,
      order: unboundTokenUsageSortOrder,
    })
  }

  const toggleUnboundTokenUsageSort = useCallback(
    (field: AdminUnboundTokenUsageSortField) => {
      const isActive = effectiveUnboundTokenUsageSort === field
      let nextSort: AdminUnboundTokenUsageSortField | null = field
      let nextOrder: SortDirection | null = ADMIN_UNBOUND_TOKEN_USAGE_DEFAULT_SORT_ORDER
      if (isActive && effectiveUnboundTokenUsageSortOrder === 'desc') {
        nextOrder = 'asc'
      } else if (isActive && effectiveUnboundTokenUsageSortOrder === 'asc') {
        nextSort = null
        nextOrder = null
      }
      navigateUnboundTokenUsageSearch(unboundTokenUsageQuery, {
        page: 1,
        sort: nextSort,
        order: nextOrder,
      })
    },
    [
      effectiveUnboundTokenUsageSort,
      effectiveUnboundTokenUsageSortOrder,
      navigateUnboundTokenUsageSearch,
      unboundTokenUsageQuery,
    ],
  )

  const goPrevKeysPage = () => {
    navigateKeysList({
      page: keysPage - 1,
      perPage: keysPerPage,
      groups: selectedKeyGroups,
      statuses: selectedKeyStatuses,
      registrationIp: selectedKeyRegistrationIp,
      regions: selectedKeyRegions,
    })
  }

  const goNextKeysPage = () => {
    navigateKeysList({
      page: keysPage + 1,
      perPage: keysPerPage,
      groups: selectedKeyGroups,
      statuses: selectedKeyStatuses,
      registrationIp: selectedKeyRegistrationIp,
      regions: selectedKeyRegions,
    })
  }

  const changeKeysPerPage = (nextPerPage: number) => {
    navigateKeysList({
      page: 1,
      perPage: nextPerPage,
      groups: selectedKeyGroups,
      statuses: selectedKeyStatuses,
      registrationIp: selectedKeyRegistrationIp,
      regions: selectedKeyRegions,
    })
  }

  const refreshBaseData = async (options?: {
    includeKeys?: boolean
    preserveSelection?: boolean
  }): Promise<PaginatedApiKeys | null> => {
    const controller = new AbortController()
    setLoading(true)
    try {
      await loadData({ signal: controller.signal, reason: 'refresh', showGlobalLoading: true })
      if (options?.includeKeys && route.name === 'module' && route.module === 'keys') {
        return await refreshKeysList({ preserveSelection: options.preserveSelection === true })
      }
      return null
    } finally {
      controller.abort()
    }
  }

  const refreshKeysList = async (options?: { preserveSelection?: boolean }): Promise<PaginatedApiKeys | null> => {
    const requestKeysSelectionQueryKey = buildKeysSelectionQueryKey(keysPage, keysPerPage)
    const pagedKeys = await fetchApiKeys(
      keysPage,
      keysPerPage,
      {
        groups: selectedKeyGroups,
        statuses: selectedKeyStatuses,
        registrationIp: selectedKeyRegistrationIp,
        regions: selectedKeyRegions,
      },
    )
    if (latestKeysSelectionQueryKeyRef.current !== requestKeysSelectionQueryKey) {
      return null
    }
    if (options?.preserveSelection) {
      const refreshedKeysSelectionQueryKey = buildKeysSelectionQueryKey(pagedKeys.page, pagedKeys.perPage)
      preservedKeysSelectionQueryKeyRef.current =
        refreshedKeysSelectionQueryKey === requestKeysSelectionQueryKey
          ? null
          : refreshedKeysSelectionQueryKey
    }
    setKeys(pagedKeys.items)
    setKeysTotal(pagedKeys.total)
    setKeysPage(pagedKeys.page)
    setKeysPerPage(pagedKeys.perPage)
    setKeyGroupFacets(pagedKeys.facets.groups)
    setKeyStatusFacets(pagedKeys.facets.statuses)
    setKeyRegionFacets(pagedKeys.facets.regions)
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
  const saveSystemSettings = useCallback(async (payload: SystemSettings) => {
    setSystemSettingsSaving(true)
    setSystemSettingsError(null)
    try {
      const nextSettings = await updateSystemSettings(payload)
      setSystemSettings(nextSettings)
      setSystemSettingsLoadState('ready')
      setLastUpdated(new Date())
      systemSettingsLoadedRef.current = true
    } catch (err) {
      console.error(err)
      setSystemSettingsError(
        err instanceof Error ? err.message : systemSettingsStrings.form.saveFailed,
      )
      throw err instanceof Error ? err : new Error(systemSettingsStrings.form.saveFailed)
    } finally {
      setSystemSettingsSaving(false)
    }
  }, [systemSettingsStrings.form.saveFailed])
  const refreshUsersList = async () => {
    const pagedUsers = await fetchAdminUsers(
      usersPage,
      USERS_PER_PAGE,
      usersQuery,
      usersTagFilterId,
      usersSort,
      usersSortOrder,
    )
    setUsers(pagedUsers.items)
    setUsersTotal(pagedUsers.total)
    return pagedUsers
  }

  const refreshUserDetail = async (userId: string) => {
    const detail = await fetchAdminUserDetail(userId)
    setSelectedUserDetail(detail)
    setUserDetailRevision((current) => current + 1)
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

  const openMonthlyBrokenDrawer = (subjectKind: 'user' | 'token', id: string, label: string) => {
    setMonthlyBrokenDrawer({ subjectKind, id, label })
  }

  const closeMonthlyBrokenDrawer = () => {
    setMonthlyBrokenDrawer(null)
    setMonthlyBrokenDrawerItems([])
    setMonthlyBrokenDrawerError(null)
    setMonthlyBrokenDrawerLoadState('initial_loading')
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
      hourlyLimit: Number.parseInt(userQuotaDraft.hourlyLimit, 10),
      dailyLimit: Number.parseInt(userQuotaDraft.dailyLimit, 10),
      monthlyLimit: Number.parseInt(userQuotaDraft.monthlyLimit, 10),
    }
    if (
      !Number.isFinite(payload.hourlyLimit) || payload.hourlyLimit < 0
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
      hourlyAnyDelta: 0,
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
      registrationIp: selectedKeyRegistrationIp,
      regions: selectedKeyRegions,
    })
  }

  const handleToggleKeyStatusFilter = (status: string) => {
    navigateKeysList({
      page: 1,
      perPage: keysPerPage,
      groups: selectedKeyGroups,
      statuses: toggleSelection(selectedKeyStatuses, status),
      registrationIp: selectedKeyRegistrationIp,
      regions: selectedKeyRegions,
    })
  }

  const handleToggleKeyRegionFilter = (region: string) => {
    navigateKeysList({
      page: 1,
      perPage: keysPerPage,
      groups: selectedKeyGroups,
      statuses: selectedKeyStatuses,
      registrationIp: selectedKeyRegistrationIp,
      regions: toggleSelection(selectedKeyRegions, region),
    })
  }

  const handleClearKeyGroupFilters = () => {
    navigateKeysList({
      page: 1,
      perPage: keysPerPage,
      groups: [],
      statuses: selectedKeyStatuses,
      registrationIp: selectedKeyRegistrationIp,
      regions: selectedKeyRegions,
    })
  }

  const handleClearKeyStatusFilters = () => {
    navigateKeysList({
      page: 1,
      perPage: keysPerPage,
      groups: selectedKeyGroups,
      statuses: [],
      registrationIp: selectedKeyRegistrationIp,
      regions: selectedKeyRegions,
    })
  }

  const handleClearKeyRegionFilters = () => {
    navigateKeysList({
      page: 1,
      perPage: keysPerPage,
      groups: selectedKeyGroups,
      statuses: selectedKeyStatuses,
      registrationIp: selectedKeyRegistrationIp,
      regions: [],
    })
  }

  const handleClearKeyRegistrationIpFilter = () => {
    setKeyRegistrationIpInput('')
    navigateKeysList({
      page: 1,
      perPage: keysPerPage,
      groups: selectedKeyGroups,
      statuses: selectedKeyStatuses,
      registrationIp: '',
      regions: selectedKeyRegions,
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

  const clearKeySelection = useCallback(() => {
    setSelectedKeyIds(new Set())
  }, [])

  const retainBulkActionSelection = useCallback(
    (selectedIds: Iterable<string>, visibleKeyIds: Iterable<string>) => {
      setSelectedKeyIds(new Set(retainVisibleApiKeySelection(selectedIds, visibleKeyIds)))
    },
    [],
  )

  const dismissBulkSyncBubble = useCallback(() => {
    setBulkSyncBubbleVisible(false)
    setBulkSyncBubblePinnedPosition(null)
  }, [])

  const captureBulkSyncBubblePinnedPosition = useCallback((): ApiKeyBulkSyncBubblePinnedPosition | null => {
    const rect = bulkSyncButtonRef.current?.getBoundingClientRect()
    if (!rect || typeof window === 'undefined') return null

    return createApiKeyBulkSyncBubblePinnedPosition(rect, window.innerWidth)
  }, [])

  const toggleSelectedKey = useCallback((id: string) => {
    if (!id) return
    setSelectedKeyIds((previous) => {
      const next = new Set(previous)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }, [])

  const toggleAllVisibleKeys = useCallback(
    (checked: boolean) => {
      setSelectedKeyIds((previous) => {
        const next = new Set(previous)
        for (const item of visibleKeys) {
          if (checked) {
            next.add(item.id)
          } else {
            next.delete(item.id)
          }
        }
        return next
      })
    },
    [visibleKeys],
  )

  const formatBulkActionSummary = useCallback(
    (
      action: ApiKeyBulkAction,
      summary: { requested: number; succeeded: number; skipped: number; failed: number },
    ) => {
      const actionLabel =
        action === 'delete'
          ? keyStrings.bulkActions.delete
          : action === 'clear_quarantine'
            ? keyStrings.bulkActions.clearQuarantine
            : keyStrings.bulkActions.syncUsage
      return keyStrings.bulkActions.summary
        .replace('{action}', actionLabel)
        .replace('{requested}', String(summary.requested))
        .replace('{succeeded}', String(summary.succeeded))
        .replace('{skipped}', String(summary.skipped))
        .replace('{failed}', String(summary.failed))
    },
    [keyStrings.bulkActions],
  )

  const runBulkKeyAction = useCallback(
    async (action: ApiKeyBulkAction) => {
      if (selectedVisibleKeyIds.length === 0 || bulkKeyActionInFlight) return
      if (action === 'sync_usage') return
      const actionSelectedKeyIds = selectedVisibleKeyIds
      const actionSelectionQueryKey = keysSelectionQueryKey
      setBulkKeyActionInFlight(action)
      setKeysBulkFeedback(null)
      setError(null)
      try {
        const response = await applyApiKeyBulkAction(action, selectedVisibleKeyIds)
        const shouldRetainSelection = latestKeysSelectionQueryKeyRef.current === actionSelectionQueryKey
        const refreshedKeys = await refreshBaseData({
          includeKeys: true,
          preserveSelection: shouldRetainSelection,
        })
        if (refreshedKeys) {
          if (shouldRetainSelection && latestKeysSelectionQueryKeyRef.current === actionSelectionQueryKey) {
            retainBulkActionSelection(actionSelectedKeyIds, refreshedKeys.items.map((item) => item.id))
          }
        }
        setKeysBulkFeedback({
          kind: response.summary.failed > 0 ? 'error' : 'success',
          message: formatBulkActionSummary(action, response.summary),
        })
      } catch (err) {
        console.error(err)
        setKeysBulkFeedback({
          kind: 'error',
          message:
            err instanceof Error
              ? err.message
              : action === 'delete'
                ? errorStrings.deleteKey
                : action === 'clear_quarantine'
                  ? errorStrings.clearQuarantine
                  : errorStrings.syncUsage,
        })
      } finally {
        setBulkKeyActionInFlight(null)
      }
    },
    [
      bulkKeyActionInFlight,
      errorStrings.clearQuarantine,
      errorStrings.deleteKey,
      errorStrings.syncUsage,
      formatBulkActionSummary,
      keysSelectionQueryKey,
      refreshBaseData,
      retainBulkActionSelection,
      selectedVisibleKeyIds,
    ],
  )

  const runBulkSyncUsage = useCallback(async () => {
    if (selectedVisibleKeyIds.length === 0 || bulkKeyActionInFlight) return

    const actionSelectedKeyIds = selectedVisibleKeyIds
    const actionSelectionQueryKey = keysSelectionQueryKey
    const initialProgress = createApiKeyBulkSyncProgressState(selectedVisibleKeyIds.length)
    setBulkKeyActionInFlight('sync_usage')
    setKeysBulkFeedback(null)
    setError(null)
    setBulkSyncProgress(initialProgress)
    setBulkSyncBubbleVisible(true)
    setBulkSyncBubblePinnedPosition(null)

    try {
      const response = await syncApiKeyBulkUsageWithProgress(selectedVisibleKeyIds, (event) => {
        setBulkSyncProgress((current) =>
          updateApiKeyBulkSyncProgressState(
            current ?? createApiKeyBulkSyncProgressState(selectedVisibleKeyIds.length),
            event,
          ),
        )
      })

      setBulkSyncProgress((current) =>
        markApiKeyBulkSyncRefreshRunning(
          current ?? createApiKeyBulkSyncProgressState(selectedVisibleKeyIds.length),
          keyStrings.bulkSyncProgress.refreshingList,
        ),
      )

      const shouldRetainSelection = latestKeysSelectionQueryKeyRef.current === actionSelectionQueryKey
      const refreshedKeys = await refreshBaseData({
        includeKeys: true,
        preserveSelection: shouldRetainSelection,
      })
      setBulkSyncBubblePinnedPosition(captureBulkSyncBubblePinnedPosition())
      if (refreshedKeys) {
        if (shouldRetainSelection && latestKeysSelectionQueryKeyRef.current === actionSelectionQueryKey) {
          retainBulkActionSelection(actionSelectedKeyIds, refreshedKeys.items.map((item) => item.id))
        }
      }

      setBulkSyncProgress((current) =>
        markApiKeyBulkSyncRefreshDone(
          current ?? createApiKeyBulkSyncProgressState(selectedVisibleKeyIds.length),
          response,
        ),
      )
      setKeysBulkFeedback({
        kind: response.summary.failed > 0 ? 'error' : 'success',
        message: formatBulkActionSummary('sync_usage', response.summary),
      })
    } catch (err) {
      console.error(err)
      const message = err instanceof Error ? err.message : errorStrings.syncUsage
      setBulkSyncProgress((current) => {
        if (!current) {
          return updateApiKeyBulkSyncProgressState(
            createApiKeyBulkSyncProgressState(selectedVisibleKeyIds.length),
            { type: 'error', message, phaseKey: 'prepare_request' },
          )
        }
        if (current.response) {
          return markApiKeyBulkSyncRefreshError(current, message)
        }
        if (current.completed) {
          return current
        }
        return updateApiKeyBulkSyncProgressState(current, {
          type: 'error',
          message,
          phaseKey: current.current > 0 ? 'sync_usage' : 'prepare_request',
        })
      })
      setKeysBulkFeedback({
        kind: 'error',
        message,
      })
    } finally {
      setBulkKeyActionInFlight(null)
    }
  }, [
    bulkKeyActionInFlight,
    captureBulkSyncBubblePinnedPosition,
    errorStrings.syncUsage,
    formatBulkActionSummary,
    keysSelectionQueryKey,
    keyStrings.bulkSyncProgress.refreshingList,
    refreshBaseData,
    retainBulkActionSelection,
    selectedVisibleKeyIds,
  ])

  const confirmBulkDelete = async () => {
    if (selectedVisibleKeyIds.length === 0) return
    setPendingBulkDelete(false)
    await runBulkKeyAction('delete')
  }

  const cancelBulkDelete = () => {
    setPendingBulkDelete(false)
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

  const navItems: AdminNavItem[] = [
    { target: 'dashboard', label: adminStrings.nav.dashboard, icon: <Icon icon="mdi:view-dashboard-outline" width={18} height={18} /> },
    { target: 'user-usage', label: adminStrings.nav.usage, icon: <ChartColumnIncreasing size={18} strokeWidth={2.2} /> },
    { target: 'tokens', label: adminStrings.nav.tokens, icon: <Icon icon="mdi:key-chain-variant" width={18} height={18} /> },
    { target: 'keys', label: adminStrings.nav.keys, icon: <Icon icon="mdi:key-outline" width={18} height={18} /> },
    { target: 'requests', label: adminStrings.nav.requests, icon: <Icon icon="mdi:file-document-outline" width={18} height={18} /> },
    { target: 'jobs', label: adminStrings.nav.jobs, icon: <Icon icon="mdi:calendar-clock-outline" width={18} height={18} /> },
    { target: 'users', label: adminStrings.nav.users, icon: <Icon icon="mdi:account-group-outline" width={18} height={18} /> },
    { target: 'alerts', label: adminStrings.nav.alerts, icon: <Icon icon="mdi:bell-ring-outline" width={18} height={18} /> },
    { target: 'system-settings', label: adminStrings.nav.systemSettings, icon: <Icon icon="mdi:cog-outline" width={18} height={18} /> },
    { target: 'proxy-settings', label: adminStrings.nav.proxySettings, icon: <Icon icon="mdi:tune-variant" width={18} height={18} /> },
  ]
  const activeNavItem: AdminNavTarget =
    route.name === 'user-usage'
      ? 'user-usage'
      : route.name === 'module'
        ? route.module
        : route.name === 'key'
          ? 'keys'
          : route.name === 'user'
              || route.name === 'user-tags'
              || route.name === 'user-tag-editor'
            ? 'users'
            : 'tokens'
  const activeModule: AdminModuleId =
    route.name === 'module'
      ? route.module
      : route.name === 'key'
        ? 'keys'
        : route.name === 'user'
            || route.name === 'user-usage'
            || route.name === 'user-tags'
            || route.name === 'user-tag-editor'
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
            onClick={() =>
              tag && navigateUsersSearch(tag.displayName, { tagId: tag.id, sort: usersSort, order: usersSortOrder })
            }
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
      activeItem={activeNavItem}
      navItems={navItems}
      skipToContentLabel={adminStrings.accessibility.skipToContent}
      onSelectItem={navigateModule}
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
                  buildUsersCollectionPath(
                    getAdminUsersQueryFromLocation(),
                    getAdminUsersTagFilterFromLocation(),
                    getAdminUsersPageFromLocation(),
                    getAdminUsersSortFromLocation(),
                    getAdminUsersSortDirectionFromLocation(),
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

  const appFooter = (
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
  )

  const renderAdminGlobalOverlayHost = (): JSX.Element => (
    <>
      <Drawer
        open={monthlyBrokenDrawer != null}
        onOpenChange={(open) => {
          if (!open) closeMonthlyBrokenDrawer()
        }}
        shouldScaleBackground={false}
      >
        <DrawerContent className="request-entity-drawer-content-fit">
          <div className="request-entity-drawer-body-fit">
            <section className="surface panel">
              <div className="panel-header" style={{ gap: 12, flexWrap: 'wrap' }}>
                <div>
                  <h2>{usersStrings.brokenKeys.drawerTitle}</h2>
                  <p className="panel-description">
                    {usersStrings.brokenKeys.drawerDescription.replace('{label}', monthlyBrokenDrawer?.label ?? '—')}
                  </p>
                </div>
              </div>
              <AdminLoadingRegion
                className="table-wrapper jobs-table-wrapper admin-responsive-up"
                loadState={monthlyBrokenDrawerLoadState}
                loadingLabel={usersStrings.brokenKeys.loading}
                errorLabel={monthlyBrokenDrawerError ?? loadingStateStrings.error}
                minHeight={240}
              >
                {monthlyBrokenDrawerItems.length === 0 ? (
                  <div className="empty-state alert">{usersStrings.brokenKeys.empty}</div>
                ) : (
                  <Table className="jobs-table admin-users-table">
                    <thead>
                      <tr>
                        <th>{usersStrings.brokenKeys.table.key}</th>
                        <th>{usersStrings.brokenKeys.table.status}</th>
                        <th>{usersStrings.brokenKeys.table.reason}</th>
                        <th>{usersStrings.brokenKeys.table.latestBreakAt}</th>
                        <th>{usersStrings.brokenKeys.table.breaker}</th>
                        <th>{usersStrings.brokenKeys.table.relatedUsers}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {monthlyBrokenDrawerItems.map((item) => {
                        const stateKey = copyStateKey('brokenKeys', item.keyId)
                        const state = copyState.get(stateKey)
                        return (
                          <tr key={`${item.keyId}:${item.latestBreakAt}`}>
                            <td>
                              <MonthlyBrokenKeyValue
                                keyId={item.keyId}
                                ungroupedLabel={keyStrings.groups.ungrouped}
                                detailLabel={keyStrings.actions.details}
                                copyLabel={usersStrings.brokenKeys.actions.copyKeyId}
                                copiedLabel={usersStrings.brokenKeys.actions.copied}
                                copyState={state}
                                onOpenKey={navigateKey}
                                onCopy={(anchorEl) =>
                                  handleCopyMonthlyBrokenKeyId(item.keyId, stateKey, anchorEl)
                                }
                              />
                            </td>
                            <td>
                              <StatusBadge tone={item.currentStatus === 'quarantined' ? 'warning' : 'error'}>
                                {adminStrings.statuses[item.currentStatus] ?? item.currentStatus}
                              </StatusBadge>
                            </td>
                            <td>{item.reasonSummary || item.reasonCode || usersStrings.brokenKeys.noReason}</td>
                            <td>{formatTimestamp(item.latestBreakAt)}</td>
                            <td>{formatMonthlyBrokenBreaker(item, usersStrings.brokenKeys)}</td>
                            <td>
                              {formatMonthlyBrokenRelatedUsers(
                                item.relatedUsers,
                                usersStrings.brokenKeys.noRelatedUsers,
                              )}
                            </td>
                          </tr>
                        )
                      })}
                    </tbody>
                  </Table>
                )}
              </AdminLoadingRegion>
              <AdminLoadingRegion
                className="admin-mobile-list admin-responsive-down"
                loadState={monthlyBrokenDrawerLoadState}
                loadingLabel={usersStrings.brokenKeys.loading}
                errorLabel={monthlyBrokenDrawerError ?? loadingStateStrings.error}
                minHeight={240}
              >
                {monthlyBrokenDrawerItems.length === 0 ? (
                  <div className="empty-state alert">{usersStrings.brokenKeys.empty}</div>
                ) : (
                  monthlyBrokenDrawerItems.map((item) => {
                    const stateKey = copyStateKey('brokenKeys', item.keyId)
                    const state = copyState.get(stateKey)
                    return (
                      <article key={`${item.keyId}:${item.latestBreakAt}`} className="admin-mobile-card">
                        <div className="admin-mobile-kv">
                          <span>{usersStrings.brokenKeys.table.key}</span>
                          <strong>
                            <MonthlyBrokenKeyValue
                              keyId={item.keyId}
                              ungroupedLabel={keyStrings.groups.ungrouped}
                              detailLabel={keyStrings.actions.details}
                              copyLabel={usersStrings.brokenKeys.actions.copyKeyId}
                              copiedLabel={usersStrings.brokenKeys.actions.copied}
                              copyState={state}
                              onOpenKey={navigateKey}
                              onCopy={(anchorEl) =>
                                handleCopyMonthlyBrokenKeyId(item.keyId, stateKey, anchorEl)
                              }
                            />
                          </strong>
                        </div>
                        <div className="admin-mobile-kv">
                          <span>{usersStrings.brokenKeys.table.status}</span>
                          <strong>{adminStrings.statuses[item.currentStatus] ?? item.currentStatus}</strong>
                        </div>
                        <div className="admin-mobile-kv">
                          <span>{usersStrings.brokenKeys.table.reason}</span>
                          <strong>{item.reasonSummary || item.reasonCode || usersStrings.brokenKeys.noReason}</strong>
                        </div>
                        <div className="admin-mobile-kv">
                          <span>{usersStrings.brokenKeys.table.latestBreakAt}</span>
                          <strong>{formatTimestamp(item.latestBreakAt)}</strong>
                        </div>
                        <div className="admin-mobile-kv">
                          <span>{usersStrings.brokenKeys.table.breaker}</span>
                          <strong>{formatMonthlyBrokenBreaker(item, usersStrings.brokenKeys)}</strong>
                        </div>
                        <div className="admin-mobile-kv">
                          <span>{usersStrings.brokenKeys.table.relatedUsers}</span>
                          <strong>
                            {formatMonthlyBrokenRelatedUsers(
                              item.relatedUsers,
                              usersStrings.brokenKeys.noRelatedUsers,
                            )}
                          </strong>
                        </div>
                      </article>
                    )
                  })
                )}
              </AdminLoadingRegion>
            </section>
          </div>
        </DrawerContent>
      </Drawer>

      <Drawer
        open={requestEntityDrawer != null}
        onOpenChange={(open) => {
          if (!open) closeRequestEntityDrawer()
        }}
        shouldScaleBackground={false}
      >
        <DrawerContent className="request-entity-drawer-content">
          <div className="request-entity-drawer-body">
            {requestEntityDrawer?.kind === 'key' ? (
              <KeyDetails
                key={`drawer-key-${requestEntityDrawer.id}`}
                id={requestEntityDrawer.id}
                onBack={closeRequestEntityDrawer}
                onOpenUser={navigateUser}
                onOpenToken={navigateToken}
              />
            ) : requestEntityDrawer?.kind === 'token' ? (
              <AdminLazyBoundary loadingLabel={loadingStateStrings.switching} minHeight={240}>
                <LazyTokenDetail
                  key={`drawer-token-${requestEntityDrawer.id}`}
                  id={requestEntityDrawer.id}
                  onBack={closeRequestEntityDrawer}
                  onOpenKey={navigateKey}
                  onOpenUser={navigateUser}
                  onSecretRotated={handleTokenSecretRotated}
                />
              </AdminLazyBoundary>
            ) : null}
          </div>
        </DrawerContent>
      </Drawer>

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
{keysValidationVisibleState != null ? (
  <Suspense fallback={null}>
    <LazyApiKeysValidationDialog
      open
      state={keysValidationVisibleState}
      counts={keysValidationCounts}
      validKeys={keysValidationValidKeys}
      exhaustedKeys={keysValidationExhaustedKeys}
      onClose={closeKeysValidationDialog}
      onRetryFailed={() => void handleRetryFailedValidation()}
      onRetryOne={(apiKey) => void handleRetryOneValidation(apiKey)}
      onImportValid={() => void handleImportValidatedKeys()}
    />
  </Suspense>
) : null}

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

<Dialog open={pendingBulkDelete} onOpenChange={(open) => { if (!open) cancelBulkDelete() }}>
  <DialogContent className="max-w-md">
    <DialogHeader>
      <DialogTitle>{keyStrings.dialogs.bulkDelete.title}</DialogTitle>
      <DialogDescription>{keyStrings.dialogs.bulkDelete.description}</DialogDescription>
    </DialogHeader>
    <DialogFooter className="modal-action">
      <Button type="button" variant="outline" onClick={cancelBulkDelete}>
        {keyStrings.dialogs.bulkDelete.cancel}
      </Button>
      <Button
        type="button"
        variant="destructive"
        onClick={() => void confirmBulkDelete()}
        disabled={selectedVisibleKeyCount === 0 || bulkKeyActionInFlight != null}
      >
        {keyStrings.dialogs.bulkDelete.confirm}
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

  const renderAdminPageWithGlobalOverlays = (pageShell: JSX.Element): JSX.Element => (
    <AdminOverlayHost overlays={renderAdminGlobalOverlayHost()}>{pageShell}</AdminOverlayHost>
  )


  if (route.name === 'key') {
    return renderAdminPageWithGlobalOverlays(
      <AdminShell
        activeItem={activeNavItem}
        navItems={navItems}
        skipToContentLabel={adminStrings.accessibility.skipToContent}
        onSelectItem={navigateModule}
      >
        <KeyDetails
          key={route.id}
          id={route.id}
          onOpenUser={(userId) => navigateUser(userId, { preserveUsersContext: true })}
          onBack={() =>
            navigateToPath(
              buildAdminKeysPath({
                page: getAdminKeysPageFromLocation(),
                perPage: getAdminKeysPerPageFromLocation(),
                groups: getAdminKeysValuesFromLocation('group'),
                statuses: getAdminKeysValuesFromLocation('status'),
                registrationIp: getAdminKeysRegistrationIpFromLocation(),
                regions: getAdminKeysRegionsFromLocation(),
              }),
            )
          }
        />
      </AdminShell>
    )
  }
  if (route.name === 'token') {
    return renderAdminPageWithGlobalOverlays(
      <AdminShell
        activeItem={activeNavItem}
        navItems={navItems}
        skipToContentLabel={adminStrings.accessibility.skipToContent}
        onSelectItem={navigateModule}
      >
        <AdminLazyBoundary loadingLabel={loadingStateStrings.switching} minHeight={360}>
          <LazyTokenDetail
            key={route.id}
            id={route.id}
            onBack={navigateBackFromTokenDetail}
            onOpenKey={navigateKey}
            onOpenUser={navigateUser}
            onSecretRotated={handleTokenSecretRotated}
          />
        </AdminLazyBoundary>
      </AdminShell>
    )
  }
  if (route.name === 'user-tags' || route.name === 'user-tag-editor') {
    return renderAdminPageWithGlobalOverlays(renderUserTagCatalogIndexPage())
  }
  if (route.name === 'user') {
    const detail = selectedUserDetail
    const tokenItems = detail?.tokens ?? []
    const boundTags = detail?.tags ?? []
    const hasBlockAllTag = boundTags.some((tag) => tag.effectKind === 'block_all')

    return renderAdminPageWithGlobalOverlays(
      <AdminShell
        activeItem={activeNavItem}
        navItems={navItems}
        skipToContentLabel={adminStrings.accessibility.skipToContent}
        onSelectItem={navigateModule}
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
                    buildUsersCollectionPath(
                      getAdminUsersQueryFromLocation(),
                      getAdminUsersTagFilterFromLocation(),
                      getAdminUsersPageFromLocation(),
                      getAdminUsersSortFromLocation(),
                      getAdminUsersSortDirectionFromLocation(),
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
              <UserDetailQuotaBreakdown
                entries={detail.quotaBreakdown}
                usersStrings={usersStrings}
                formatQuotaLimitValue={formatQuotaLimitValue}
                formatSignedQuotaDelta={formatSignedQuotaDelta}
              />
            </section>

            <section className="surface panel">
              <AdminLazyBoundary loadingLabel={loadingStateStrings.switching} minHeight={280}>
                <LazyUserDetailSharedUsagePanel
                  key={`usage:${detail.userId}:${userDetailRevision}`}
                  usersStrings={usersStrings}
                  language={language}
                  title={usersStrings.detail.sharedUsageTitle}
                  description={usersStrings.detail.sharedUsageDescription}
                  loadSeries={(series, signal) => fetchAdminUserUsageSeries(detail.userId, series, signal)}
                />
              </AdminLazyBoundary>
            </section>

            <section className="surface panel">
              <div className="panel-header">
                <div>
                  <h2>{usersStrings.detail.tokensTitle}</h2>
                  <p className="panel-description">{usersStrings.detail.tokensDescription}</p>
                </div>
              </div>
              <div className="table-wrapper jobs-table-wrapper">
                <AdminLazyBoundary loadingLabel={loadingStateStrings.switching} minHeight={220}>
                  <LazyUserDetailTokenTable
                    tokens={tokenItems}
                    usersStrings={usersStrings}
                    formatNumber={formatNumber}
                    formatTimestamp={(value) => formatTimestamp(value ?? null)}
                    onViewToken={navigateToken}
                  />
                </AdminLazyBoundary>
              </div>
            </section>
          </>
        )}
      </AdminShell>
    )
  }

  if (route.name === 'user-usage') {
    const backSort = isAdminUsersOverviewSortField(usersSort) ? usersSort : null
    const backOrder = backSort ? usersSortOrder : null
    const usageDailyRateLabel = language === 'zh' ? usersStrings.usage.table.dailySuccessRate : 'Daily'
    const usageMonthlyRateLabel = language === 'zh' ? usersStrings.usage.table.monthlySuccessRate : 'Monthly'
    const userUsageUpdatedTime = lastUpdated ? timeOnlyFormatter.format(lastUpdated) : null
    const userUsageDesktopUtility = (
      <AdminShellSidebarUtility>
        <AdminSidebarUtilityStack>
          <AdminSidebarUtilityCard>
            <div className="admin-sidebar-utility-toolbar">
              <ThemeToggle />
              <LanguageSwitcher />
            </div>
            <div className="admin-sidebar-utility-meta">
              {displayName && (
                <div className={`user-badge${isAdmin ? ' user-badge-admin' : ''}`}>
                  {isAdmin && <Icon icon="mdi:crown-outline" className="user-badge-icon" aria-hidden="true" />}
                  <span>{displayName}</span>
                </div>
              )}
              {userUsageUpdatedTime && (
                <span className="admin-panel-header-time" aria-live="polite">
                  <Icon
                    icon="mdi:clock-time-four-outline"
                    width={14}
                    height={14}
                    className="admin-panel-header-time-icon"
                    aria-hidden="true"
                  />
                  <span className="admin-panel-header-time-label">{headerStrings.updatedPrefix}</span>
                  <span className="admin-panel-header-time-value">{userUsageUpdatedTime}</span>
                </span>
              )}
            </div>
          </AdminSidebarUtilityCard>

          <AdminSidebarUtilityCard>
            <div className="admin-sidebar-utility-actions">
              <AdminReturnToConsoleLink
                label={headerStrings.returnToConsole}
                href={userConsoleHref}
                className="admin-sidebar-utility-action"
              />
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="admin-panel-refresh-button admin-sidebar-utility-action"
                onClick={handleManualRefresh}
                disabled={loading || activeModuleBlocking}
              >
                <Icon
                  icon={loading ? 'mdi:loading' : 'mdi:refresh'}
                  width={16}
                  height={16}
                  className={loading ? 'icon-spin' : undefined}
                  aria-hidden="true"
                />
                <span>{loading ? headerStrings.refreshing : headerStrings.refreshNow}</span>
              </Button>
              <Button
                type="button"
                variant="ghost"
                className="admin-sidebar-utility-action"
                onClick={() => navigateToPath(buildAdminUsersPath(usersQuery, usersTagFilterId, usersPage, backSort, backOrder))}
              >
                <Icon icon="mdi:arrow-left" width={16} height={16} aria-hidden="true" />
                <span>{usersStrings.usage.back}</span>
              </Button>
            </div>
          </AdminSidebarUtilityCard>
        </AdminSidebarUtilityStack>
      </AdminShellSidebarUtility>
    )

    return renderAdminPageWithGlobalOverlays(
      <AdminShell
        activeItem={activeNavItem}
        navItems={navItems}
        skipToContentLabel={adminStrings.accessibility.skipToContent}
        onSelectItem={navigateModule}
      >
        {userUsageDesktopUtility}

        <div className="admin-desktop-only">
          <AdminCompactIntro
            title={usersStrings.usage.title}
            description={usersStrings.usage.description}
          />
        </div>

        <section className="surface panel">
          <div className="panel-header" style={{ gap: 12, flexWrap: 'wrap' }}>
            <div className="admin-stacked-only" style={{ flex: '1 1 340px', minWidth: 260 }}>
              <h2>{usersStrings.usage.title}</h2>
              <p className="panel-description">{usersStrings.usage.description}</p>
            </div>
            <div className="admin-inline-actions" style={{ flexWrap: 'wrap', justifyContent: 'flex-end' }}>
              <div className="admin-stacked-only">
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => navigateToPath(buildAdminUsersPath(usersQuery, usersTagFilterId, usersPage, backSort, backOrder))}
                >
                  {usersStrings.usage.back}
                </Button>
              </div>
              <div className="users-search-controls">
                <Input
                  type="text"
                  name="user-usage-search"
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

          <AdminTableShell
            className="jobs-table-wrapper admin-users-usage-table-wrapper"
            tableClassName="jobs-table admin-users-table admin-users-usage-table"
            loadState={usersLoadState}
            loadingLabel={usersRefreshing ? loadingStateStrings.refreshing : usersStrings.empty.loading}
            errorLabel={usersError ?? loadingStateStrings.error}
            minHeight={360}
          >
            {users.length === 0 ? (
              <tbody>
                <tr>
                  <td colSpan={10}>
                    <div className="empty-state alert">{usersStrings.empty.none}</div>
                  </td>
                </tr>
              </tbody>
            ) : (
              <>
                <thead>
                  <tr>
                    <th>{usersStrings.usage.table.user}</th>
                    <th>{usersStrings.usage.table.status}</th>
                    <AdminUsersSortableHeader
                      label={usersStrings.usage.table.hourlyAny}
                      field="hourlyAnyUsed"
                      activeField={effectiveUsersSort}
                      activeOrder={effectiveUsersSortOrder}
                      onToggle={toggleUsersSort}
                    />
                    <AdminUsersSortableHeader
                      label={usersStrings.usage.table.hourly}
                      field="quotaHourlyUsed"
                      activeField={effectiveUsersSort}
                      activeOrder={effectiveUsersSortOrder}
                      onToggle={toggleUsersSort}
                    />
                    <AdminUsersSortableHeader
                      label={usersStrings.usage.table.daily}
                      field="quotaDailyUsed"
                      activeField={effectiveUsersSort}
                      activeOrder={effectiveUsersSortOrder}
                      onToggle={toggleUsersSort}
                    />
                    <AdminUsersSortableHeader
                      label={usersStrings.usage.table.monthly}
                      field="quotaMonthlyUsed"
                      activeField={effectiveUsersSort}
                      activeOrder={effectiveUsersSortOrder}
                      onToggle={toggleUsersSort}
                    />
                    <AdminUsersSortableHeader
                      label={usersStrings.usage.table.monthlyBroken}
                      field="monthlyBrokenCount"
                      activeField={effectiveUsersSort}
                      activeOrder={effectiveUsersSortOrder}
                      onToggle={toggleUsersSort}
                    />
                    <AdminUsersSortableHeader
                      label={usersStrings.usage.table.dailySuccessRate}
                      displayLabel={usageDailyRateLabel}
                      field="dailySuccessRate"
                      activeField={effectiveUsersSort}
                      activeOrder={effectiveUsersSortOrder}
                      onToggle={toggleUsersSort}
                    />
                    <AdminUsersSortableHeader
                      label={usersStrings.usage.table.monthlySuccessRate}
                      displayLabel={usageMonthlyRateLabel}
                      field="monthlySuccessRate"
                      activeField={effectiveUsersSort}
                      activeOrder={effectiveUsersSortOrder}
                      onToggle={toggleUsersSort}
                    />
                    <AdminUsersSortableHeader
                      label={usersStrings.usage.table.lastUsed}
                      field="lastActivity"
                      activeField={effectiveUsersSort}
                      activeOrder={effectiveUsersSortOrder}
                      onToggle={toggleUsersSort}
                    />
                  </tr>
                </thead>
                <tbody>
                  {users.map((item) => {
                    const requestRate = resolveRequestRate(item, 'user')
                    return (
                    <tr key={item.userId}>
                      <td className="admin-users-identity-cell">
                        <button
                          type="button"
                          className="link-button admin-users-identity-button"
                          aria-label={usersStrings.actions.view}
                          onClick={() => navigateUser(item.userId, { preserveUsersContext: true })}
                        >
                          <strong>{item.displayName || item.username || item.userId}</strong>
                        </button>
                        <div className="panel-description admin-users-identity-meta">
                          <code>{item.userId}</code>
                          {item.username ? ` · @${item.username}` : ''}
                        </div>
                      </td>
                      <td>
                        <StatusBadge tone={item.active ? 'success' : 'neutral'}>
                          {item.active ? usersStrings.status.active : usersStrings.status.inactive}
                        </StatusBadge>
                      </td>
                      <td className="admin-users-compact-cell">
                        <AdminTableValueStack {...formatQuotaStackValue(requestRate.used, requestRate.limit)} />
                      </td>
                      <td className="admin-users-compact-cell">
                        <AdminTableValueStack {...formatQuotaStackValue(item.quotaHourlyUsed, item.quotaHourlyLimit)} />
                      </td>
                      <td className="admin-users-compact-cell">
                        <AdminTableValueStack {...formatQuotaStackValue(item.quotaDailyUsed, item.quotaDailyLimit)} />
                      </td>
                      <td className="admin-users-compact-cell">
                        <AdminTableValueStack {...formatQuotaStackValue(item.quotaMonthlyUsed, item.quotaMonthlyLimit)} />
                      </td>
                      <td className="admin-users-compact-cell">
                        {(() => {
                          const metric = formatMonthlyBrokenStackValue(
                            item.monthlyBrokenCount,
                            item.monthlyBrokenLimit,
                          )
                          const label = item.displayName || item.username || item.userId
                          return (
                            <div className="admin-table-value-stack">
                              <MonthlyBrokenCountTrigger
                                count={item.monthlyBrokenCount}
                                onOpen={() => openMonthlyBrokenDrawer('user', item.userId, label)}
                                ariaLabel={usersStrings.brokenKeys.openDetails.replace('{label}', label)}
                                className={metric.primaryClassName}
                              />
                              <span className="admin-table-value-secondary">{metric.secondary}</span>
                            </div>
                          )
                        })()}
                      </td>
                      <td className="admin-users-compact-cell">
                        <AdminTableValueStack {...formatSuccessRateStackValue(item.dailySuccess, item.dailyFailure, language)} />
                      </td>
                      <td className="admin-users-compact-cell">
                        <AdminTableValueStack {...formatSuccessRateStackValue(item.monthlySuccess, item.monthlyFailure, language)} />
                      </td>
                      <td className="admin-users-compact-cell">
                        <AdminTableValueStack {...formatStackedTimestamp(item.lastActivity, language)} />
                      </td>
                    </tr>
                  )})}
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
              users.map((item) => {
                const requestRate = resolveRequestRate(item, 'user')
                return (
                <article key={item.userId} className="admin-mobile-card">
                  <div className="admin-mobile-kv">
                    <span>{usersStrings.usage.table.user}</span>
                    <button
                      type="button"
                      className="link-button admin-users-mobile-link"
                      aria-label={usersStrings.actions.view}
                      onClick={() => navigateUser(item.userId, { preserveUsersContext: true })}
                    >
                      <strong>{item.displayName || item.username || item.userId}</strong>
                    </button>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{usersStrings.usage.table.status}</span>
                    <StatusBadge tone={item.active ? 'success' : 'neutral'}>
                      {item.active ? usersStrings.status.active : usersStrings.status.inactive}
                    </StatusBadge>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{formatRequestRateSummary(requestRate, language)}</span>
                    <strong>{formatQuotaUsagePair(requestRate.used, requestRate.limit)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{usersStrings.usage.table.hourly}</span>
                    <strong>{formatQuotaUsagePair(item.quotaHourlyUsed, item.quotaHourlyLimit)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{usersStrings.usage.table.daily}</span>
                    <strong>{formatQuotaUsagePair(item.quotaDailyUsed, item.quotaDailyLimit)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{usersStrings.usage.table.monthly}</span>
                    <strong>{formatQuotaUsagePair(item.quotaMonthlyUsed, item.quotaMonthlyLimit)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{usersStrings.usage.table.monthlyBroken}</span>
                    {item.monthlyBrokenCount > 0 ? (
                      <button
                        type="button"
                        className="link-button"
                        onClick={() =>
                          openMonthlyBrokenDrawer(
                            'user',
                            item.userId,
                            item.displayName || item.username || item.userId,
                          )
                        }
                      >
                        <strong>{formatQuotaUsagePair(item.monthlyBrokenCount, item.monthlyBrokenLimit)}</strong>
                      </button>
                    ) : (
                      <strong>{formatQuotaUsagePair(item.monthlyBrokenCount, item.monthlyBrokenLimit)}</strong>
                    )}
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{usersStrings.usage.table.dailySuccessRate}</span>
                    <strong>{formatCompactSuccessRateValue(item.dailySuccess, item.dailyFailure, language)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{usersStrings.usage.table.monthlySuccessRate}</span>
                    <strong>{formatCompactSuccessRateValue(item.monthlySuccess, item.monthlyFailure, language)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{usersStrings.usage.table.lastUsed}</span>
                    <strong>{formatTimestamp(item.lastActivity)}</strong>
                  </div>
                </article>
              )})
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
      </AdminShell>
    )
  }

  if (route.name === 'unbound-token-usage') {
    const usageDailyRateLabel = language === 'zh' ? unboundTokenUsageStrings.table.dailySuccessRate : 'Daily'
    const usageMonthlyRateLabel =
      language === 'zh' ? unboundTokenUsageStrings.table.monthlySuccessRate : 'Monthly'

    return renderAdminPageWithGlobalOverlays(
      <AdminShell
        activeItem={activeNavItem}
        navItems={navItems}
        skipToContentLabel={adminStrings.accessibility.skipToContent}
        onSelectItem={navigateModule}
      >
        <AdminShellSidebarUtility>
          <AdminSidebarUtilityStack>
            <AdminSidebarUtilityCard>
              <div className="admin-sidebar-utility-toolbar">
                <ThemeToggle />
              </div>
              <div className="admin-sidebar-utility-actions">
                <AdminReturnToConsoleLink
                  label={headerStrings.returnToConsole}
                  href={userConsoleHref}
                  className="admin-sidebar-utility-action"
                />
                <Button
                  type="button"
                  variant="ghost"
                  className="token-usage-back-button admin-sidebar-utility-action"
                  onClick={() => navigateModule('tokens')}
                >
                  <Icon icon="mdi:arrow-left" width={16} height={16} aria-hidden="true" />
                  <span>{unboundTokenUsageStrings.back}</span>
                </Button>
              </div>
            </AdminSidebarUtilityCard>
          </AdminSidebarUtilityStack>
        </AdminShellSidebarUtility>

        <div className="admin-desktop-only">
          <AdminCompactIntro
            title={unboundTokenUsageStrings.title}
            description={unboundTokenUsageStrings.description}
          />
        </div>

        <section className="surface panel">
          <div className="panel-header" style={{ gap: 12, flexWrap: 'wrap' }}>
            <div className="admin-stacked-only" style={{ flex: '1 1 340px', minWidth: 260 }}>
              <h2>{unboundTokenUsageStrings.title}</h2>
              <p className="panel-description">{unboundTokenUsageStrings.description}</p>
            </div>
            <div className="admin-inline-actions" style={{ flexWrap: 'wrap', justifyContent: 'flex-end' }}>
              <div className="admin-stacked-only">
                <Button type="button" variant="outline" onClick={() => navigateModule('tokens')}>
                  {unboundTokenUsageStrings.back}
                </Button>
              </div>
              <div className="users-search-controls">
                <Input
                  type="text"
                  name="unbound-token-usage-search"
                  className="users-search-input"
                  placeholder={unboundTokenUsageStrings.searchPlaceholder}
                  value={unboundTokenUsageQueryInput}
                  disabled={unboundTokenUsageBlocking}
                  onChange={(event) => setUnboundTokenUsageQueryInput(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter') {
                      event.preventDefault()
                      applyUnboundTokenUsageSearch()
                    }
                  }}
                />
                <Button
                  type="button"
                  variant="outline"
                  onClick={applyUnboundTokenUsageSearch}
                  disabled={unboundTokenUsageBlocking}
                >
                  {usersStrings.search}
                </Button>
                {(unboundTokenUsageQueryInput.length > 0 || unboundTokenUsageQuery.length > 0) && (
                  <Button
                    type="button"
                    variant="ghost"
                    onClick={resetUnboundTokenUsageSearch}
                    disabled={unboundTokenUsageBlocking}
                  >
                    {usersStrings.clear}
                  </Button>
                )}
              </div>
            </div>
          </div>

          <AdminTableShell
            className="jobs-table-wrapper admin-users-usage-table-wrapper"
            tableClassName="jobs-table admin-users-table admin-users-usage-table"
            loadState={unboundTokenUsageLoadState}
            loadingLabel={
              unboundTokenUsageRefreshing
                ? loadingStateStrings.refreshing
                : unboundTokenUsageStrings.empty.loading
            }
            errorLabel={unboundTokenUsageError ?? loadingStateStrings.error}
            minHeight={360}
          >
            {unboundTokenUsage.length === 0 ? (
              <tbody>
                <tr>
                  <td colSpan={10}>
                    <div className="empty-state alert">{unboundTokenUsageStrings.empty.none}</div>
                  </td>
                </tr>
              </tbody>
            ) : (
              <>
                <thead>
                  <tr>
                    <th>{unboundTokenUsageStrings.table.identity}</th>
                    <th>{unboundTokenUsageStrings.table.status}</th>
                    <AdminUsersSortableHeader
                      label={unboundTokenUsageStrings.table.hourlyAny}
                      field="hourlyAnyUsed"
                      activeField={effectiveUnboundTokenUsageSort}
                      activeOrder={effectiveUnboundTokenUsageSortOrder}
                      onToggle={toggleUnboundTokenUsageSort}
                    />
                    <AdminUsersSortableHeader
                      label={unboundTokenUsageStrings.table.hourly}
                      field="quotaHourlyUsed"
                      activeField={effectiveUnboundTokenUsageSort}
                      activeOrder={effectiveUnboundTokenUsageSortOrder}
                      onToggle={toggleUnboundTokenUsageSort}
                    />
                    <AdminUsersSortableHeader
                      label={unboundTokenUsageStrings.table.daily}
                      field="quotaDailyUsed"
                      activeField={effectiveUnboundTokenUsageSort}
                      activeOrder={effectiveUnboundTokenUsageSortOrder}
                      onToggle={toggleUnboundTokenUsageSort}
                    />
                    <AdminUsersSortableHeader
                      label={unboundTokenUsageStrings.table.monthly}
                      field="quotaMonthlyUsed"
                      activeField={effectiveUnboundTokenUsageSort}
                      activeOrder={effectiveUnboundTokenUsageSortOrder}
                      onToggle={toggleUnboundTokenUsageSort}
                    />
                    <AdminUsersSortableHeader
                      label={unboundTokenUsageStrings.table.monthlyBroken}
                      field="monthlyBrokenCount"
                      activeField={effectiveUnboundTokenUsageSort}
                      activeOrder={effectiveUnboundTokenUsageSortOrder}
                      onToggle={toggleUnboundTokenUsageSort}
                    />
                    <AdminUsersSortableHeader
                      label={unboundTokenUsageStrings.table.dailySuccessRate}
                      displayLabel={usageDailyRateLabel}
                      field="dailySuccessRate"
                      activeField={effectiveUnboundTokenUsageSort}
                      activeOrder={effectiveUnboundTokenUsageSortOrder}
                      onToggle={toggleUnboundTokenUsageSort}
                    />
                    <AdminUsersSortableHeader
                      label={unboundTokenUsageStrings.table.monthlySuccessRate}
                      displayLabel={usageMonthlyRateLabel}
                      field="monthlySuccessRate"
                      activeField={effectiveUnboundTokenUsageSort}
                      activeOrder={effectiveUnboundTokenUsageSortOrder}
                      onToggle={toggleUnboundTokenUsageSort}
                    />
                    <AdminUsersSortableHeader
                      label={unboundTokenUsageStrings.table.lastUsed}
                      field="lastUsedAt"
                      activeField={effectiveUnboundTokenUsageSort}
                      activeOrder={effectiveUnboundTokenUsageSortOrder}
                      onToggle={toggleUnboundTokenUsageSort}
                    />
                  </tr>
                </thead>
                <tbody>
                  {unboundTokenUsage.map((item) => {
                    const requestRate = resolveRequestRate(item, 'token')
                    return (
                    <tr key={item.tokenId}>
                      <td className="admin-users-identity-cell">
                        <button
                          type="button"
                          className="link-button admin-users-identity-button"
                          aria-label={keyStrings.actions.details}
                          onClick={() => navigateToken(item.tokenId, { preserveUnboundUsageContext: true })}
                        >
                          <strong>{item.tokenId}</strong>
                        </button>
                        <div className="panel-description admin-users-identity-meta">
                          {formatUnboundTokenIdentityMeta(item.note, item.group, tokenStrings.groups.label)}
                        </div>
                      </td>
                      <td>
                        <StatusBadge tone={item.enabled ? 'success' : 'neutral'}>
                          {item.enabled ? usersStrings.status.enabled : usersStrings.status.disabled}
                        </StatusBadge>
                      </td>
                      <td className="admin-users-compact-cell">
                        <AdminTableValueStack {...formatQuotaStackValue(requestRate.used, requestRate.limit)} />
                      </td>
                      <td className="admin-users-compact-cell">
                        <AdminTableValueStack {...formatQuotaStackValue(item.quotaHourlyUsed, item.quotaHourlyLimit)} />
                      </td>
                      <td className="admin-users-compact-cell">
                        <AdminTableValueStack {...formatQuotaStackValue(item.quotaDailyUsed, item.quotaDailyLimit)} />
                      </td>
                      <td className="admin-users-compact-cell">
                        <AdminTableValueStack {...formatQuotaStackValue(item.quotaMonthlyUsed, item.quotaMonthlyLimit)} />
                      </td>
                      <td className="admin-users-compact-cell">
                        {item.monthlyBrokenCount == null || item.monthlyBrokenLimit == null ? (
                          <AdminTableValueStack primary="—" />
                        ) : (
                          (() => {
                            const metric = formatMonthlyBrokenStackValue(
                              item.monthlyBrokenCount,
                              item.monthlyBrokenLimit,
                            )
                            return (
                              <div className="admin-table-value-stack">
                                <MonthlyBrokenCountTrigger
                                  count={item.monthlyBrokenCount}
                                  onOpen={() => openMonthlyBrokenDrawer('token', item.tokenId, item.tokenId)}
                                  ariaLabel={usersStrings.brokenKeys.openDetails.replace('{label}', item.tokenId)}
                                  className={metric.primaryClassName}
                                />
                                <span className="admin-table-value-secondary">{metric.secondary}</span>
                              </div>
                            )
                          })()
                        )}
                      </td>
                      <td className="admin-users-compact-cell">
                        <AdminTableValueStack {...formatSuccessRateStackValue(item.dailySuccess, item.dailyFailure, language)} />
                      </td>
                      <td className="admin-users-compact-cell">
                        <AdminTableValueStack {...formatSuccessRateStackValue(item.monthlySuccess, item.monthlyFailure, language)} />
                      </td>
                      <td className="admin-users-compact-cell">
                        <AdminTableValueStack {...formatStackedTimestamp(item.lastUsedAt, language)} />
                      </td>
                    </tr>
                  )})}
                </tbody>
              </>
            )}
          </AdminTableShell>

          <AdminLoadingRegion
            className="admin-mobile-list admin-responsive-down"
            loadState={unboundTokenUsageLoadState}
            loadingLabel={
              unboundTokenUsageRefreshing
                ? loadingStateStrings.refreshing
                : unboundTokenUsageStrings.empty.loading
            }
            errorLabel={unboundTokenUsageError ?? loadingStateStrings.error}
            minHeight={260}
          >
            {unboundTokenUsage.length === 0 ? (
              <div className="empty-state alert">{unboundTokenUsageStrings.empty.none}</div>
            ) : (
              unboundTokenUsage.map((item) => {
                const requestRate = resolveRequestRate(item, 'token')
                return (
                <article key={item.tokenId} className="admin-mobile-card">
                  <div className="admin-mobile-identity-block">
                    <div className="admin-mobile-identity-row">
                      <span className="admin-mobile-identity-label">{unboundTokenUsageStrings.table.identity}</span>
                      <button
                        type="button"
                        className="link-button admin-users-mobile-link"
                        aria-label={keyStrings.actions.details}
                        onClick={() => navigateToken(item.tokenId, { preserveUnboundUsageContext: true })}
                      >
                        <strong>{item.tokenId}</strong>
                      </button>
                    </div>
                    <div className="panel-description admin-mobile-identity-meta">
                      {formatUnboundTokenIdentityMeta(item.note, item.group, tokenStrings.groups.label)}
                    </div>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{unboundTokenUsageStrings.table.status}</span>
                    <StatusBadge tone={item.enabled ? 'success' : 'neutral'}>
                      {item.enabled ? usersStrings.status.enabled : usersStrings.status.disabled}
                    </StatusBadge>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{formatRequestRateSummary(requestRate, language)}</span>
                    <strong>{formatQuotaUsagePair(requestRate.used, requestRate.limit)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{unboundTokenUsageStrings.table.hourly}</span>
                    <strong>{formatQuotaUsagePair(item.quotaHourlyUsed, item.quotaHourlyLimit)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{unboundTokenUsageStrings.table.daily}</span>
                    <strong>{formatQuotaUsagePair(item.quotaDailyUsed, item.quotaDailyLimit)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{unboundTokenUsageStrings.table.monthly}</span>
                    <strong>{formatQuotaUsagePair(item.quotaMonthlyUsed, item.quotaMonthlyLimit)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{unboundTokenUsageStrings.table.monthlyBroken}</span>
                    {item.monthlyBrokenCount == null || item.monthlyBrokenLimit == null ? (
                      <strong>—</strong>
                    ) : item.monthlyBrokenCount > 0 ? (
                      <button
                        type="button"
                        className="link-button"
                        onClick={() => openMonthlyBrokenDrawer('token', item.tokenId, item.tokenId)}
                      >
                        <strong>{formatQuotaUsagePair(item.monthlyBrokenCount, item.monthlyBrokenLimit)}</strong>
                      </button>
                    ) : (
                      <strong>{formatQuotaUsagePair(item.monthlyBrokenCount, item.monthlyBrokenLimit)}</strong>
                    )}
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{unboundTokenUsageStrings.table.dailySuccessRate}</span>
                    <strong>{formatCompactSuccessRateValue(item.dailySuccess, item.dailyFailure, language)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{unboundTokenUsageStrings.table.monthlySuccessRate}</span>
                    <strong>{formatCompactSuccessRateValue(item.monthlySuccess, item.monthlyFailure, language)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{unboundTokenUsageStrings.table.lastUsed}</span>
                    <strong>{formatTimestamp(item.lastUsedAt)}</strong>
                  </div>
                </article>
              )})
            )}
          </AdminLoadingRegion>

          {unboundTokenUsageTotal > USERS_PER_PAGE && (
            <AdminTablePagination
              page={unboundTokenUsagePage}
              totalPages={unboundTokenUsageTotalPages}
              pageSummary={
                <span className="panel-description">
                  {usersStrings.pagination
                    .replace('{page}', String(unboundTokenUsagePage))
                    .replace('{total}', String(unboundTokenUsageTotalPages))}
                </span>
              }
              previousLabel={tokenStrings.pagination.prev}
              nextLabel={tokenStrings.pagination.next}
              previousDisabled={unboundTokenUsagePage <= 1}
              nextDisabled={unboundTokenUsagePage >= unboundTokenUsageTotalPages}
              disabled={unboundTokenUsageBlocking}
              onPrevious={goPrevUnboundTokenUsagePage}
              onNext={goNextUnboundTokenUsagePage}
            />
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
  const showSystemSettings = activeModule === 'system-settings'
  const showProxySettings = activeModule === 'proxy-settings'
  const headerUpdatedTime = lastUpdated ? timeOnlyFormatter.format(lastUpdated) : null

  const moduleDesktopUtility = (
    <AdminShellSidebarUtility>
      <AdminSidebarUtilityStack>
        <AdminSidebarUtilityCard>
          <div className="admin-sidebar-utility-toolbar">
            <ThemeToggle />
            <LanguageSwitcher />
          </div>
          <div className="admin-sidebar-utility-meta">
            {displayName && (
              <div className={`user-badge${isAdmin ? ' user-badge-admin' : ''}`}>
                {isAdmin && <Icon icon="mdi:crown-outline" className="user-badge-icon" aria-hidden="true" />}
                <span>{displayName}</span>
              </div>
            )}
            {headerUpdatedTime && (
              <span className="admin-panel-header-time" aria-live="polite">
                <Icon icon="mdi:clock-time-four-outline" width={14} height={14} className="admin-panel-header-time-icon" aria-hidden="true" />
                <span className="admin-panel-header-time-label">{headerStrings.updatedPrefix}</span>
                <span className="admin-panel-header-time-value">{headerUpdatedTime}</span>
              </span>
            )}
          </div>
        </AdminSidebarUtilityCard>

        <AdminSidebarUtilityCard>
          <div className="admin-sidebar-utility-actions">
            <AdminReturnToConsoleLink
              label={headerStrings.returnToConsole}
              href={userConsoleHref}
              className="admin-sidebar-utility-action"
            />
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="admin-panel-refresh-button admin-sidebar-utility-action"
              onClick={handleManualRefresh}
              disabled={loading || activeModuleBlocking}
            >
              <Icon
                icon={loading ? 'mdi:loading' : 'mdi:refresh'}
                width={16}
                height={16}
                className={loading ? 'icon-spin' : undefined}
                aria-hidden="true"
              />
              <span>{loading ? headerStrings.refreshing : headerStrings.refreshNow}</span>
            </Button>
          </div>
        </AdminSidebarUtilityCard>
      </AdminSidebarUtilityStack>
    </AdminShellSidebarUtility>
  )
  const moduleDesktopIntro = (() => {
    switch (activeModule) {
      case 'dashboard':
        return {
          title: headerStrings.title,
          description: headerStrings.subtitle,
        }
      case 'tokens':
        return {
          title: tokenStrings.title,
          description: tokenStrings.description,
        }
      case 'keys':
        return {
          title: keyStrings.title,
          description: keyStrings.description,
        }
      case 'requests':
        return {
          title: logStrings.title,
          description: logStrings.description,
        }
      case 'jobs':
        return {
          title: jobsStrings.title,
          description: jobsStrings.description,
        }
      case 'users':
        return {
          title: usersStrings.title,
          description: usersStrings.description,
        }
      case 'alerts':
        return {
          title: adminStrings.modules.alerts.title,
          description: adminStrings.modules.alerts.description,
        }
      case 'system-settings':
        return {
          title: systemSettingsStrings.title,
          description: systemSettingsStrings.description,
        }
      case 'proxy-settings':
        return {
          title: proxySettingsStrings.title,
          description: proxySettingsStrings.description,
        }
      default:
        return {
          title: headerStrings.title,
          description: headerStrings.subtitle,
        }
    }
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
      {renderAdminPageWithGlobalOverlays(
        <AdminShell
          activeItem={activeNavItem}
          navItems={navItems}
          skipToContentLabel={adminStrings.accessibility.skipToContent}
          onSelectItem={navigateModule}
        >
          {moduleDesktopUtility}

      <div className="admin-stacked-only">
        <AdminPanelHeader
          title={headerStrings.title}
          subtitle={headerStrings.subtitle}
          displayName={displayName}
          isAdmin={isAdmin}
          updatedPrefix={headerStrings.updatedPrefix}
          updatedTime={headerUpdatedTime}
          isRefreshing={loading}
          refreshDisabled={activeModuleBlocking}
          refreshLabel={headerStrings.refreshNow}
          refreshingLabel={headerStrings.refreshing}
          userConsoleLabel={headerStrings.returnToConsole}
          userConsoleHref={userConsoleHref}
          onRefresh={handleManualRefresh}
        />
      </div>

      <div className="admin-desktop-only">
        <AdminCompactIntro
          title={moduleDesktopIntro.title}
          description={moduleDesktopIntro.description}
        />
      </div>

      {showDashboard && (
        <DashboardOverview
          strings={adminStrings.dashboard}
          overviewReady={dashboardOverviewLoaded}
          statusLoading={dashboardStatusLoading}
          todayMetrics={todayMetrics}
          todayQuotaCharge={todayQuotaCharge}
          monthMetrics={monthMetrics}
          monthQuotaCharge={monthQuotaCharge}
          statusMetrics={statusMetrics}
          hourlyRequestWindow={dashboardHourlyRequestWindow}
          chartPersistenceKey={DASHBOARD_HOURLY_CHART_PERSISTENCE_KEY}
          tokenCoverage={dashboardTokenCoverage}
          tokens={dashboardTokens}
          keys={dashboardKeys}
          logs={dashboardLogs}
          jobs={dashboardJobs}
          recentAlerts={dashboardRecentAlerts}
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
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8 rounded-full p-0 shadow-none"
                    aria-label={tokenStrings.actions.viewLeaderboard}
                    onClick={navigateUnboundTokenUsage}
                  >
                    <Icon icon="mdi:chart-timeline-variant" width={20} height={20} />
                  </Button>
                </TooltipTrigger>
                <TooltipContent side="top">{tokenStrings.actions.viewLeaderboard}</TooltipContent>
              </Tooltip>
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
                style={{ ...keysQuickAddCardStyle, position: 'relative', marginLeft: 'auto' }}
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
          <div style={keysUtilityRowStyle}>
            <div style={keysFilterClusterStyle}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <Input
                  type="text"
                  value={keyRegistrationIpInput}
                  onChange={(event) => setKeyRegistrationIpInput(event.target.value)}
                  placeholder={keyStrings.filters.registrationIpPlaceholder}
                  aria-label={keyStrings.filters.registrationIp}
                  style={{ width: 188 }}
                />
                {selectedKeyRegistrationIp ? (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={handleClearKeyRegistrationIpFilter}
                  >
                    {keyStrings.filters.clearRegistrationIp}
                  </Button>
                ) : null}
              </div>
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
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button type="button" variant="outline" size="sm" aria-label={keyRegionFilterSummary}>
                    <Icon icon="mdi:map-marker-radius-outline" width={16} height={16} aria-hidden="true" />
                    <span style={{ whiteSpace: 'nowrap' }}>{keyRegionFilterSummary}</span>
                    {selectedKeyRegions.length > 0 ? (
                      <Badge variant="neutral" className="ml-1 px-1.5 py-0 text-[10px]">
                        {selectedKeyRegions.length}
                      </Badge>
                    ) : null}
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="start" className="w-72">
                  <DropdownMenuLabel>{keyStrings.filters.region}</DropdownMenuLabel>
                  <DropdownMenuItem
                    className="cursor-pointer"
                    disabled={selectedKeyRegions.length === 0}
                    onSelect={(event) => {
                      event.preventDefault()
                      handleClearKeyRegionFilters()
                    }}
                  >
                    {keyStrings.filters.clearRegions}
                  </DropdownMenuItem>
                  <DropdownMenuSeparator />
                  {keyRegionFilterOptions.map((option) => (
                    <DropdownMenuCheckboxItem
                      key={option.value}
                      className="cursor-pointer"
                      checked={selectedKeyRegions.includes(option.value)}
                      onSelect={(event) => event.preventDefault()}
                      onCheckedChange={() => handleToggleKeyRegionFilter(option.value)}
                    >
                      <span>{option.label}</span>
                      <span className="ml-auto text-xs opacity-60">{formatNumber(option.count)}</span>
                    </DropdownMenuCheckboxItem>
                  ))}
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          </div>
          {isAdmin && visibleKeys.length > 0 ? (
            <div style={keysBulkToolbarStyle}>
              <div style={keysBulkSelectionStyle}>
                <span className="panel-description">
                  {keyStrings.selection.selectedCount.replace('{count}', String(selectedVisibleKeyCount))}
                </span>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={() => toggleAllVisibleKeys(true)}
                  disabled={allVisibleKeysSelected || bulkKeyActionInFlight != null}
                >
                  {keyStrings.selection.selectCurrentPage}
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={clearKeySelection}
                  disabled={selectedVisibleKeyCount === 0 || bulkKeyActionInFlight != null}
                >
                  {keyStrings.selection.clear}
                </Button>
              </div>
              <div style={keysBulkActionsStyle}>
                <Button
                  ref={bulkSyncButtonRef}
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => void runBulkSyncUsage()}
                  disabled={selectedVisibleKeyCount === 0 || bulkKeyActionInFlight != null}
                >
                  <Icon
                    icon={bulkKeyActionInFlight === 'sync_usage' ? 'mdi:loading' : 'mdi:refresh'}
                    width={16}
                    height={16}
                    aria-hidden="true"
                    className={bulkKeyActionInFlight === 'sync_usage' ? 'animate-spin' : undefined}
                  />
                  {bulkKeyActionInFlight === 'sync_usage'
                    ? keyStrings.bulkActions.running
                    : keyStrings.bulkActions.syncUsage}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => void runBulkKeyAction('clear_quarantine')}
                  disabled={selectedVisibleKeyCount === 0 || bulkKeyActionInFlight != null}
                >
                  <Icon
                    icon={bulkKeyActionInFlight === 'clear_quarantine' ? 'mdi:loading' : 'mdi:shield-check-outline'}
                    width={16}
                    height={16}
                    aria-hidden="true"
                    className={bulkKeyActionInFlight === 'clear_quarantine' ? 'animate-spin' : undefined}
                  />
                  {bulkKeyActionInFlight === 'clear_quarantine'
                    ? keyStrings.bulkActions.running
                    : keyStrings.bulkActions.clearQuarantine}
                </Button>
                <Button
                  type="button"
                  variant="warning"
                  size="sm"
                  onClick={() => setPendingBulkDelete(true)}
                  disabled={selectedVisibleKeyCount === 0 || bulkKeyActionInFlight != null}
                >
                  <Icon
                    icon={bulkKeyActionInFlight === 'delete' ? 'mdi:loading' : 'mdi:trash-can-outline'}
                    width={16}
                    height={16}
                    aria-hidden="true"
                    className={bulkKeyActionInFlight === 'delete' ? 'animate-spin' : undefined}
                  />
                  {bulkKeyActionInFlight === 'delete'
                    ? keyStrings.bulkActions.running
                    : keyStrings.bulkActions.delete}
                </Button>
              </div>
            </div>
          ) : null}
          {bulkSyncProgress && bulkSyncBubbleVisible ? (
            <AnchoredApiKeyBulkSyncProgressBubble
              anchorEl={bulkSyncButtonRef.current}
              fallbackPosition={bulkSyncBubblePinnedPosition}
              strings={keyStrings.bulkSyncProgress}
              progress={bulkSyncProgress}
              onDismiss={dismissBulkSyncBubble}
            />
          ) : null}
          {keysBulkFeedback ? (
            <div
              className={keysBulkFeedback.kind === 'error' ? 'alert alert-error' : 'alert alert-warning'}
              role={keysBulkFeedback.kind === 'error' ? 'alert' : 'status'}
              style={{ marginBottom: 16 }}
            >
              {keysBulkFeedback.message}
            </div>
          ) : null}
        <AdminTableShell
          className="jobs-table-wrapper admin-responsive-up"
          tableClassName={`jobs-table api-keys-table${isAdmin ? ' api-keys-table--admin' : ''}`}
          loadState={keysLoadState}
          loadingLabel={keysRefreshing ? loadingStateStrings.refreshing : keyStrings.empty.loading}
          errorLabel={keysError ?? loadingStateStrings.error}
          minHeight={320}
        >
          {visibleKeys.length === 0 ? (
            <tbody>
              <tr>
                <td colSpan={isAdmin ? 8 : 6}>
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
                  {isAdmin && (
                    <th style={{ width: 52 }}>
                      <label style={keySelectionCheckboxLabelStyle}>
                        <input
                          ref={keysSelectAllRef}
                          type="checkbox"
                          checked={allVisibleKeysSelected}
                          aria-label={keyStrings.selection.selectAll}
                          onChange={(event) => toggleAllVisibleKeys(event.currentTarget.checked)}
                          disabled={bulkKeyActionInFlight != null}
                        />
                      </label>
                    </th>
                  )}
                  <th>
                    <div style={adminTableHeaderStackStyle}>
                      <span style={adminTableFieldStyle}>{keyStrings.table.keyId}</span>
                      <span style={adminTableSecondaryFieldStyle}>{keyStrings.groups.label}</span>
                    </div>
                  </th>
                  <th>
                    <div style={adminTableHeaderStackStyle}>
                      <span style={adminTableFieldStyle}>{keyStrings.table.registration}</span>
                      <span style={adminTableSecondaryFieldStyle}>{keyStrings.table.registrationRegion}</span>
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
                      {isAdmin && (
                        <td>
                          <label style={keySelectionCheckboxLabelStyle}>
                            <input
                              type="checkbox"
                              checked={selectedKeyIds.has(item.id)}
                              aria-label={`${keyStrings.selection.selectRow}: ${item.id}`}
                              onChange={() => toggleSelectedKey(item.id)}
                              disabled={bulkKeyActionInFlight != null}
                            />
                          </label>
                        </td>
                      )}
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
  style={{ position: 'absolute', right: 0, top: '50%', transform: 'translateY(-50%)' }}
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
                          <span className="api-keys-cell-text-secondary" style={adminTableEllipsisSecondaryFieldStyle}>{keyGroupName}</span>
                        </div>
                      </td>
                      <td>
                        <div style={adminTableStackStyle}>
                          <span className="api-keys-cell-text" style={adminTableEllipsisFieldStyle}>{formatRegistrationValue(item.registration_ip)}</span>
                          <span className="api-keys-cell-text-secondary" style={adminTableEllipsisSecondaryFieldStyle}>
                            {formatRegistrationValue(item.registration_region)}
                          </span>
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
                          <span className="api-keys-cell-text" style={adminTableEllipsisFieldStyle}>{formatNumber(item.success_count)}</span>
                          <span className="api-keys-cell-text-secondary" style={adminTableEllipsisSecondaryFieldStyle}>{formatNumber(item.error_count)}</span>
                        </div>
                      </td>
                      <td>
                        <span className="api-keys-cell-text" style={adminTableEllipsisFieldStyle}>
                          {item.quota_remaining != null && item.quota_limit != null
                            ? `${formatNumber(item.quota_remaining)} / ${formatNumber(item.quota_limit)}`
                            : '—'}
                        </span>
                      </td>
                      <td>
                        <div style={adminTableStackStyle}>
                          <span className="api-keys-cell-text" style={adminTableEllipsisFieldStyle}>{formatTimestampNoYear(item.last_used_at)}</span>
                          <span className="api-keys-cell-text-secondary" style={adminTableEllipsisSecondaryFieldStyle}>{formatTimestampNoYear(item.status_changed_at)}</span>
                        </div>
                      </td>
                      {isAdmin && (
                        <td>
                          <div className="table-actions api-keys-actions">
                            {item.quarantine ? (
  <Button
    type="button"
    variant="ghost"
    size="icon"
    className="api-keys-action-button rounded-full shadow-none"
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
    className="api-keys-action-button rounded-full shadow-none"
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
    className="api-keys-action-button rounded-full shadow-none"
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
  className="api-keys-action-button rounded-full shadow-none"
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
  className="api-keys-action-button rounded-full shadow-none"
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
                  {isAdmin && (
                    <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: 12 }}>
                      <label style={keySelectionCheckboxLabelStyle}>
                        <input
                          type="checkbox"
                          checked={selectedKeyIds.has(item.id)}
                          aria-label={`${keyStrings.selection.selectRow}: ${item.id}`}
                          onChange={() => toggleSelectedKey(item.id)}
                          disabled={bulkKeyActionInFlight != null}
                        />
                        <span className="panel-description">{keyStrings.selection.selectRow}</span>
                      </label>
                    </div>
                  )}
                  <div className="admin-mobile-kv">
                    <span>{keyStrings.table.keyId}</span>
                    <strong>
                      <code>{item.id}</code>
                    </strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{keyStrings.table.registrationIp}</span>
                    <strong>{formatRegistrationValue(item.registration_ip)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{keyStrings.table.registrationRegion}</span>
                    <strong>{formatRegistrationValue(item.registration_region)}</strong>
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
        <AdminLazyBoundary loadingLabel={loadingStateStrings.switching} minHeight={320}>
          <LazyAdminRecentRequestsPanel
          variant="admin"
          language={language}
          strings={adminStrings}
          title={logStrings.title}
          description={requestLogsDescription}
          emptyLabel={logStrings.empty.none}
          loadState={requestsLoadState}
          loadingLabel={requestsRefreshing ? loadingStateStrings.refreshing : logStrings.empty.loading}
          errorLabel={requestsError ?? loadingStateStrings.error}
          logs={logs}
          requestKindOptions={requestLogRequestKindOptions}
          requestKindQuickBilling={requestLogQuickBilling}
          requestKindQuickProtocol={requestLogQuickProtocol}
          selectedRequestKinds={requestLogSelectedKinds}
          onRequestKindQuickFiltersChange={handleRequestLogQuickFilters}
          onToggleRequestKind={handleToggleRequestLogKind}
          onClearRequestKinds={handleClearRequestLogKinds}
          outcomeFilter={requestLogOutcomeFilter}
          resultOptions={requestLogFacets.results}
          keyEffectOptions={requestLogFacets.keyEffects}
          bindingEffectOptions={requestLogFacets.bindingEffects}
          selectionEffectOptions={requestLogFacets.selectionEffects}
          onOutcomeFilterChange={handleRequestLogOutcomeFilter}
          keyOptions={requestLogFacets.keys}
          selectedKeyId={requestLogKeyFilter}
          onKeyFilterChange={handleRequestLogKeyFilter}
          showKeyColumn
          showTokenColumn
          perPage={logsPerPage}
          hasOlder={logsPageInfo.hasOlder}
          hasNewer={logsPageInfo.hasNewer}
          paginationSummary={requestLogsPaginationSummary}
          paginationDisabled={requestsBlocking}
          onNewerPage={goNewerLogsPage}
          onOlderPage={goOlderLogsPage}
          onPerPageChange={changeLogsPerPage}
          formatTime={formatTimestampNoYear}
          formatTimeDetail={(ts) => (ts ? `${formatTimestampWithMs(ts)} · ${formatRelativeTime(ts)}` : '—')}
          onOpenKey={openRequestKeyDrawer}
          onOpenToken={openRequestTokenDrawer}
          loadLogBodies={loadRequestLogBodies}
          />
        </AdminLazyBoundary>
      )}

      {showJobs && (
      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{jobsStrings.title}</h2>
            <p className="panel-description">{jobsStrings.description}</p>
          </div>
          <div className="panel-actions">
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  aria-label={jobFilterSummary}
                  disabled={jobsBlocking}
                >
                  <Icon icon="mdi:filter-outline" width={16} height={16} aria-hidden="true" />
                  <span style={{ whiteSpace: 'nowrap' }}>{jobFilterSummary}</span>
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-80">
                <DropdownMenuLabel>{jobsStrings.table.type}</DropdownMenuLabel>
                <DropdownMenuRadioGroup
                  value={jobFilter}
                  onValueChange={(value) => handleJobFilterChange(value as JobGroup)}
                >
                  {jobFilterOptions.map((option) => (
                    <DropdownMenuRadioItem key={option.value} value={option.value} className="cursor-pointer gap-3 pr-3">
                      <span>{option.label}</span>
                      <span className="ml-auto text-xs font-mono tabular-nums opacity-70">
                        {formatNumber(option.count)}
                      </span>
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuContent>
            </DropdownMenu>
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
                                    <Tooltip>
                                      <TooltipTrigger asChild>
                                        <span className="job-type-pill">
                                          <button
                                            type="button"
                                            className="job-type-trigger"
                                            aria-label={jt}
                                          >
                                            <span className="job-type-main">{jobTypeLabelText}</span>
                                          </button>
                                        </span>
                                      </TooltipTrigger>
                                      <TooltipContent className="job-type-tooltip max-w-[min(20rem,calc(100vw-2rem))]" side="top">
                                        <code>{jt}</code>
                                      </TooltipContent>
                                    </Tooltip>
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
                    <td colSpan={7}>
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
                      <th>{usersStrings.table.tags}</th>
                      <AdminUsersSortableHeader
                        label={usersStrings.table.daily}
                        field="quotaDailyUsed"
                        activeField={effectiveUsersSort}
                        activeOrder={effectiveUsersSortOrder}
                        onToggle={toggleUsersSort}
                      />
                      <AdminUsersSortableHeader
                        label={usersStrings.table.monthly}
                        field="quotaMonthlyUsed"
                        activeField={effectiveUsersSort}
                        activeOrder={effectiveUsersSortOrder}
                        onToggle={toggleUsersSort}
                      />
                      <AdminUsersSortableHeader
                        label={usersStrings.table.lastActivity}
                        field="lastActivity"
                        activeField={effectiveUsersSort}
                        activeOrder={effectiveUsersSortOrder}
                        onToggle={toggleUsersSort}
                      />
                      <AdminUsersSortableHeader
                        label={usersStrings.table.lastLogin}
                        field="lastLoginAt"
                        activeField={effectiveUsersSort}
                        activeOrder={effectiveUsersSortOrder}
                        onToggle={toggleUsersSort}
                      />
                    </tr>
                  </thead>
                  <tbody>
                    {users.map((item) => (
                      <tr key={item.userId}>
                        <td className="admin-users-identity-cell">
                          <button
                            type="button"
                            className="link-button admin-users-identity-button"
                            aria-label={usersStrings.actions.view}
                            onClick={() => navigateUser(item.userId, { preserveUsersContext: true })}
                          >
                            <strong>{item.displayName || item.username || item.userId}</strong>
                          </button>
                          <div className="panel-description admin-users-identity-meta">
                            <code>{item.userId}</code>
                            {item.username ? ` · @${item.username}` : ''}
                          </div>
                        </td>
                        <td>
                          <StatusBadge tone={item.active ? 'success' : 'neutral'}>
                            {item.active ? usersStrings.status.active : usersStrings.status.inactive}
                          </StatusBadge>
                        </td>
                        <td className="admin-users-tags-cell">
                          <UserTagBadgeList
                            tags={item.tags}
                            usersStrings={usersStrings}
                            emptyLabel={usersStrings.userTags.empty}
                          />
                        </td>
                        <td className="admin-users-compact-cell">
                          <AdminTableValueStack {...formatQuotaStackValue(item.quotaDailyUsed, item.quotaDailyLimit)} />
                        </td>
                        <td className="admin-users-compact-cell">
                          <AdminTableValueStack {...formatQuotaStackValue(item.quotaMonthlyUsed, item.quotaMonthlyLimit)} />
                        </td>
                        <td className="admin-users-compact-cell">
                          <AdminTableValueStack {...formatStackedTimestamp(item.lastActivity, language)} />
                        </td>
                        <td className="admin-users-compact-cell">
                          <AdminTableValueStack {...formatStackedTimestamp(item.lastLoginAt, language)} />
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
                      <button
                        type="button"
                        className="link-button admin-users-mobile-link"
                        aria-label={usersStrings.actions.view}
                        onClick={() => navigateUser(item.userId, { preserveUsersContext: true })}
                      >
                        <strong>{item.displayName || item.username || item.userId}</strong>
                      </button>
                    </div>
                    <div className="admin-mobile-kv">
                      <span>{usersStrings.table.status}</span>
                      <StatusBadge tone={item.active ? 'success' : 'neutral'}>
                        {item.active ? usersStrings.status.active : usersStrings.status.inactive}
                      </StatusBadge>
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
                      <span>{usersStrings.table.daily}</span>
                      <strong>{formatQuotaUsagePair(item.quotaDailyUsed, item.quotaDailyLimit)}</strong>
                    </div>
                    <div className="admin-mobile-kv">
                      <span>{usersStrings.table.monthly}</span>
                      <strong>{formatQuotaUsagePair(item.quotaMonthlyUsed, item.quotaMonthlyLimit)}</strong>
                    </div>
                    <div className="admin-mobile-kv">
                      <span>{usersStrings.table.lastActivity}</span>
                      <strong>{formatTimestamp(item.lastActivity)}</strong>
                    </div>
                    <div className="admin-mobile-kv">
                      <span>{usersStrings.table.lastLogin}</span>
                      <strong>{formatTimestamp(item.lastLoginAt)}</strong>
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
        <AdminLazyBoundary loadingLabel={loadingStateStrings.switching} minHeight={320}>
          <LazyAlertsCenter
            language={language}
            search={window.location.search}
            refreshToken={alertsRefreshToken}
            onNavigate={navigateToPath}
            onOpenUser={navigateUser}
            onOpenToken={navigateToken}
            onOpenKey={navigateKey}
            formatTime={formatTimestamp}
            formatTimeDetail={(ts) => (ts ? `${formatTimestampWithMs(ts)} · ${formatRelativeTime(ts)}` : '—')}
          />
        </AdminLazyBoundary>
      )}

      {showSystemSettings && (
        <AdminLazyBoundary loadingLabel={loadingStateStrings.switching} minHeight={260}>
          <LazySystemSettingsModule
            strings={systemSettingsStrings}
            settings={systemSettings}
            loadState={systemSettingsLoadState}
            error={systemSettingsError}
            saving={systemSettingsSaving}
            onApply={saveSystemSettings}
          />
        </AdminLazyBoundary>
      )}

      {showProxySettings && (
        <AdminLazyBoundary loadingLabel={loadingStateStrings.switching} minHeight={320}>
          <LazyForwardProxySettingsModule
            strings={proxySettingsStrings}
            settings={forwardProxySettings}
            stats={forwardProxyStats}
            settingsLoadState={forwardProxySettingsLoadState}
            statsLoadState={forwardProxyStatsLoadState}
            settingsError={forwardProxySettingsError}
            statsError={forwardProxyStatsError}
            saveError={forwardProxySaveError}
            revalidateError={forwardProxyRevalidateError}
            saving={forwardProxySaving}
            revalidating={forwardProxyRevalidating}
            savedAt={forwardProxySavedAt}
            revalidateProgress={forwardProxyRevalidateProgress}
            onPersistDraft={saveForwardProxySettings}
            onValidateCandidates={validateForwardProxyCandidates}
            onRefresh={handleManualRefresh}
            onRevalidate={() => void revalidateForwardProxy()}
          />
        </AdminLazyBoundary>
      )}

          {appFooter}
        </AdminShell>,
      )}
    </>
  )
}

interface LogRowProps {
  log: RequestLog
  expanded: boolean
  onToggle: (id: number) => void
  strings: AdminTranslations
  language: 'en' | 'zh'
  onOpenKey?: (id: string) => void
  onOpenToken?: (id: string) => void
}

function LogRow({ log, expanded, onToggle, strings, language, onOpenKey, onOpenToken }: LogRowProps): JSX.Element {
  const requestButtonLabel = expanded ? strings.logs.toggles.hide : strings.logs.toggles.show
  const tokenId = log.auth_token_id ?? null
  const isRebalanceGateway = isRebalanceGatewayLog(log)
  const timeLabel = formatClockTime(log.created_at)
  const timeDetail =
    log.created_at != null
      ? `${formatTimestampWithMs(log.created_at)} · ${formatRelativeTime(log.created_at)}`
      : strings.logs.errors.none

  return (
    <>
      <tr>
        <td>
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                className="log-time-trigger"
                aria-label={timeDetail}
              >
                <span className="log-time-main">{timeLabel}</span>
              </button>
            </TooltipTrigger>
            <TooltipContent className="log-time-tooltip max-w-[min(20rem,calc(100vw-2rem))]" side="right">
              {timeDetail}
            </TooltipContent>
          </Tooltip>
        </td>
        <td>
          <button
            type="button"
            className={`log-key-pill request-entity-button${isRebalanceGateway ? ' log-key-pill--rebalance' : ''}`}
            title={strings.keys.actions.details}
            aria-label={strings.keys.actions.details}
            onClick={() => {
              if (!log.key_id) return
              onOpenKey?.(log.key_id)
            }}
          >
            {isRebalanceGateway ? (
              <>
                <RebalanceGatewayMarker />
                <span className="sr-only">{strings.logDetails.gatewayMode}: {log.gateway_mode}</span>
              </>
            ) : null}
            <code>{log.key_id}</code>
          </button>
        </td>
        <td>
          {tokenId ? (
            <button
              type="button"
              className="link-button log-token-link request-entity-button"
              title={strings.tokens.table.id}
              aria-label={strings.tokens.table.id}
              onClick={() => onOpenToken?.(tokenId)}
            >
              <code>{tokenId}</code>
            </button>
          ) : (
            '—'
          )}
        </td>
        <td>
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                className="status-pair-trigger"
                aria-label={formatRequestStatusTooltip(log, strings)}
              >
                {formatRequestStatusPair(log.http_status, log.mcp_status)}
              </button>
            </TooltipTrigger>
            <TooltipContent className="max-w-[min(18rem,calc(100vw-2rem))]" side="top">
              {formatRequestStatusTooltip(log, strings)}
            </TooltipContent>
          </Tooltip>
        </td>
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
            <StatusBadge tone={requestLogTone(log)}>
              {requestLogLabel(log, language)}
            </StatusBadge>
            <Icon icon={expanded ? 'mdi:chevron-up' : 'mdi:chevron-down'} width={18} height={18} className="log-result-icon" />
          </button>
        </td>
        <td>
          <StatusBadge
            tone={keyEffectTone(log.key_effect_code)}
            title={formatKeyEffectSummary(log, strings, language)}
          >
            {keyEffectBadgeLabel(log, strings)}
          </StatusBadge>
        </td>
        <td>{formatErrorMessage(log, strings.logs.errors, language)}</td>
      </tr>
      {expanded && (
        <tr className="log-details-row">
          <td colSpan={7} id={`log-details-${log.id}`}>
            <LogDetails log={log} strings={strings} language={language} />
          </td>
        </tr>
      )}
    </>
  )
}

function LogDetails({
  log,
  strings,
  language,
}: {
  log: RequestLog
  strings: AdminTranslations
  language: 'en' | 'zh'
}): JSX.Element {
  const query = log.query ? `?${log.query}` : ''
  const requestLine = `${log.method} ${log.path}${query}`
  const forwarded = (log.forwarded_headers ?? []).filter((value) => value.trim().length > 0)
  const dropped = (log.dropped_headers ?? []).filter((value) => value.trim().length > 0)
  const httpLabel = `${strings.logs.table.httpStatus}: ${log.http_status ?? strings.logs.errors.none}`
  const mcpLabel = `${strings.logs.table.mcpStatus}: ${log.mcp_status ?? strings.logs.errors.none}`
  const requestBody = log.request_body ?? strings.logDetails.noBody
  const responseBody = log.response_body ?? strings.logDetails.noBody
  const keyEffect = formatKeyEffectSummary(log, strings, language)
  const guidance = operationalClassGuidance(log.operationalClass, log.failure_kind, language)

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
          <span className="log-details-value">{requestLogLabel(log, language)}</span>
        </div>
        <div>
          <span className="log-details-label">{strings.logDetails.keyEffect}</span>
          <span className="log-details-value">{keyEffect}</span>
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
        {guidance ? (
          <div className="log-details-section">
            <header>{strings.logDetails.solution}</header>
            <pre>{guidance}</pre>
          </div>
        ) : null}
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

export function KeyDetails({
  id,
  onBack,
  onOpenUser,
  onOpenToken,
}: {
  id: string
  onBack: () => void
  onOpenUser: (userId: string) => void
  onOpenToken?: (tokenId: string) => void
}): JSX.Element {
  const { language } = useLanguage()
  const translations = useTranslate()
  const adminStrings = translations.admin
  const keyStrings = adminStrings.keys
  const keyDetailsStrings = adminStrings.keyDetails
  const logsTableStrings = adminStrings.logs.table
  const loadingStateStrings = adminStrings.loadingStates
  const tokenStrings = adminStrings.tokens
  const [detail, setDetail] = useState<ApiKeyStats | null>(null)
  const [period, setPeriod] = useState<'day' | 'week' | 'month'>('month')
  const [startDate, setStartDate] = useState<string>(() => new Date().toISOString().slice(0, 10))
  const [summary, setSummary] = useState<KeySummary | null>(null)
  const [logs, setLogs] = useState<RequestLog[]>([])
  const [logsPerPage, setLogsPerPage] = useState(20)
  const [logsCursor, setLogsCursor] = useState<string | null>(null)
  const [logsDirection, setLogsDirection] = useState<'older' | 'newer'>('older')
  const [logsPageInfo, setLogsPageInfo] = useState<RequestLogsListPage>({ ...emptyRequestLogsListPage, pageSize: 20 })
  const [logsNonce, setLogsNonce] = useState(0)
  const [keyLogRequestKindOptions, setKeyLogRequestKindOptions] = useState<TokenLogRequestKindOption[]>([])
  const [keyLogSelectedKinds, setKeyLogSelectedKinds] = useState<string[]>([])
  const [keyLogQuickBilling, setKeyLogQuickBilling] = useState<TokenLogRequestKindQuickBilling>('all')
  const [keyLogQuickProtocol, setKeyLogQuickProtocol] = useState<TokenLogRequestKindQuickProtocol>('all')
  const [keyLogFacets, setKeyLogFacets] = useState<RequestLogFacets>(emptyRequestLogFacets)
  const [keyLogsCatalog, setKeyLogsCatalog] = useState<RequestLogsCatalog | null>(null)
  const [keyLogOutcomeFilter, setKeyLogOutcomeFilter] = useState<RecentRequestsOutcomeFilter | null>(null)
  const [stickyUsers, setStickyUsers] = useState<StickyUserRow[]>([])
  const [stickyUsersPage, setStickyUsersPage] = useState(1)
  const [stickyUsersTotal, setStickyUsersTotal] = useState(0)
  const [stickyNodes, setStickyNodes] = useState<StickyNode[]>([])
  const [detailLoadState, setDetailLoadState] = useState<QueryLoadState>('initial_loading')
  const [logsLoadState, setLogsLoadState] = useState<QueryLoadState>('initial_loading')
  const [stickyUsersLoadState, setStickyUsersLoadState] = useState<QueryLoadState>('initial_loading')
  const [stickyNodesLoadState, setStickyNodesLoadState] = useState<QueryLoadState>('initial_loading')
  const [error, setError] = useState<string | null>(null)
  const [logsError, setLogsError] = useState<string | null>(null)
  const [stickyUsersError, setStickyUsersError] = useState<string | null>(null)
  const [stickyNodesError, setStickyNodesError] = useState<string | null>(null)
  const [syncState, setSyncState] = useState<'idle' | 'syncing' | 'success'>('idle')
  const [quarantineState, setQuarantineState] = useState<'idle' | 'clearing'>('idle')
  const [quarantineDetailExpanded, setQuarantineDetailExpanded] = useState(false)
  const syncInFlightRef = useRef(false)
  const syncFeedbackTimerRef = useRef<number | null>(null)
  const loadAbortRef = useRef<AbortController | null>(null)
  const logsAbortRef = useRef<AbortController | null>(null)
  const queryKeyRef = useRef<string | null>(null)
  const logsQueryKeyRef = useRef<string | null>(null)
  const stickyUsersAbortRef = useRef<AbortController | null>(null)
  const stickyUsersQueryKeyRef = useRef<string | null>(null)
  const stickyNodesAbortRef = useRef<AbortController | null>(null)
  const stickyNodesQueryKeyRef = useRef<string | null>(null)
  const queryKey = `${id}:${period}:${startDate}`
  const logsQueryKey = `${id}:${period}:${startDate}:${logsCursor ?? ''}:${logsDirection}:${logsPerPage}:${keyLogSelectedKinds.join(',')}:${keyLogQuickBilling}:${keyLogQuickProtocol}:${keyLogOutcomeFilter?.kind ?? ''}:${keyLogOutcomeFilter?.value ?? ''}:${logsNonce}`
  const logsCatalogQueryKey = `${id}:${period}:${startDate}:${logsNonce}`
  const stickyUsersQueryKey = `${id}:${stickyUsersPage}`
  const stickyUsersPerPage = 20
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
      const panelLoadState =
        reason === 'refresh'
          ? getRefreshingLoadState(queryKeyRef.current != null)
          : getBlockingLoadState(queryKeyRef.current != null)
      setDetailLoadState(panelLoadState)
      setError(null)
      if (reason !== 'refresh') {
        setDetail(null)
        setSummary(null)
      }
      const since = computeSince()
      const [s, d] = await Promise.all([
        fetchKeyMetrics(id, period, since, controller.signal),
        fetchApiKeyDetail(id, controller.signal).catch(() => null),
      ])
      if (controller.signal.aborted) return
      setSummary(s)
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

  const keyLogQuickFilters = useMemo(
    () => ({
      billing: keyLogQuickBilling,
      protocol: keyLogQuickProtocol,
    }),
    [keyLogQuickBilling, keyLogQuickProtocol],
  )
  const keyLogSelectedKindsNormalized = useMemo(
    () => Array.from(new Set(keyLogSelectedKinds.map((value) => value.trim()).filter(Boolean))),
    [keyLogSelectedKinds],
  )
  const keyLogQuickSelection = useMemo(
    () => buildRequestKindQuickFilterSelection(keyLogRequestKindOptions, keyLogQuickFilters),
    [keyLogQuickFilters, keyLogRequestKindOptions],
  )
  const keyLogEffectiveKinds = useMemo(
    () =>
      resolveEffectiveRequestKindSelection(
        keyLogSelectedKindsNormalized,
        keyLogQuickFilters,
        keyLogQuickSelection,
      ),
    [keyLogQuickFilters, keyLogQuickSelection, keyLogSelectedKindsNormalized],
  )
  const keyLogEffectiveKindsKey = useMemo(
    () => keyLogEffectiveKinds.join('\u0001'),
    [keyLogEffectiveKinds],
  )
  const keyLogHasEmptyMatch = useMemo(
    () => hasActiveRequestKindQuickFilters(keyLogQuickFilters) && keyLogQuickSelection.length === 0,
    [keyLogQuickFilters, keyLogQuickSelection.length],
  )
  const keyLogResultFilter =
    keyLogOutcomeFilter?.kind === 'result'
      ? (keyLogOutcomeFilter.value as 'success' | 'error' | 'neutral' | 'quota_exhausted')
      : undefined
  const keyLogKeyEffectFilter =
    keyLogOutcomeFilter?.kind === 'keyEffect' ? keyLogOutcomeFilter.value : undefined
  const keyLogBindingEffectFilter =
    keyLogOutcomeFilter?.kind === 'bindingEffect' ? keyLogOutcomeFilter.value : undefined
  const keyLogSelectionEffectFilter =
    keyLogOutcomeFilter?.kind === 'selectionEffect' ? keyLogOutcomeFilter.value : undefined

  useEffect(() => {
    logsAbortRef.current?.abort()
    const controller = new AbortController()
    logsAbortRef.current = controller
    const panelLoadState =
      logsQueryKeyRef.current == null || logsQueryKeyRef.current !== logsQueryKey
        ? getBlockingLoadState(logsQueryKeyRef.current != null)
        : getRefreshingLoadState(logsQueryKeyRef.current != null)
    setLogsLoadState(panelLoadState)
    setLogsError(null)
    setLogs([])
    setLogsPageInfo({ ...emptyRequestLogsListPage, pageSize: logsPerPage })
    const since = computeSince()

    if (keyLogHasEmptyMatch) {
      setLogs([])
      setLogsPageInfo({ ...emptyRequestLogsListPage, pageSize: logsPerPage })
      setLogsLoadState('ready')
      logsQueryKeyRef.current = logsQueryKey
      return () => {
        logsAbortRef.current?.abort()
      }
    }

    fetchKeyLogsList(
      id,
      {
        limit: logsPerPage,
        cursor: logsCursor,
        direction: logsDirection,
        since,
        requestKinds: keyLogEffectiveKinds,
        result: keyLogResultFilter,
        keyEffect: keyLogKeyEffectFilter,
        bindingEffect: keyLogBindingEffectFilter,
        selectionEffect: keyLogSelectionEffectFilter,
      },
      controller.signal,
    )
      .then((result) => {
        if (controller.signal.aborted) return
        setLogs(result.items)
        setLogsPageInfo(result)
        setLogsLoadState('ready')
        logsQueryKeyRef.current = logsQueryKey
      })
      .catch((err) => {
        if ((err as Error).name === 'AbortError') return
        console.error(err)
        setLogsError(err instanceof Error ? err.message : adminStrings.errors.loadKeyDetails)
        setLogsLoadState('error')
      })

    return () => {
      logsAbortRef.current?.abort()
    }
  }, [
    adminStrings.errors.loadKeyDetails,
    computeSince,
    id,
    keyLogEffectiveKindsKey,
    keyLogHasEmptyMatch,
    keyLogKeyEffectFilter,
    keyLogBindingEffectFilter,
    keyLogSelectionEffectFilter,
    keyLogResultFilter,
    logsPerPage,
    logsCursor,
    logsDirection,
    logsQueryKey,
  ])

  useEffect(() => {
    const controller = new AbortController()
    const since = computeSince()
    const catalogPlan = buildRequestLogsCatalogPlan({
      since,
    })

    fetchKeyLogsCatalog(id, catalogPlan.query, controller.signal)
      .then((catalog) => {
        if (controller.signal.aborted) return
        setKeyLogsCatalog(catalog)
        setKeyLogRequestKindOptions(catalog.requestKindOptions)
        setKeyLogFacets(catalog.facets)
      })
      .catch((err) => {
        if ((err as Error).name === 'AbortError') return
        console.error(err)
        setKeyLogsCatalog(null)
        setKeyLogRequestKindOptions([])
        setKeyLogFacets(emptyRequestLogFacets)
      })

    return () => controller.abort()
  }, [
    computeSince,
    id,
    logsCatalogQueryKey,
  ])

  const loadStickyUsers = useCallback(async (reason: 'initial' | 'switch' | 'refresh' = 'refresh') => {
    stickyUsersAbortRef.current?.abort()
    const controller = new AbortController()
    stickyUsersAbortRef.current = controller
    try {
      const panelLoadState =
        reason === 'refresh'
          ? getRefreshingLoadState(stickyUsersQueryKeyRef.current != null)
          : getBlockingLoadState(stickyUsersQueryKeyRef.current != null)
      setStickyUsersLoadState(panelLoadState)
      setStickyUsersError(null)
      if (reason !== 'refresh') {
        setStickyUsers([])
        setStickyUsersTotal(0)
      }
      const result = await fetchKeyStickyUsers(id, stickyUsersPage, stickyUsersPerPage, controller.signal)
      if (controller.signal.aborted) return
      setStickyUsers(result.items)
      setStickyUsersTotal(result.total)
      setStickyUsersLoadState('ready')
      stickyUsersQueryKeyRef.current = stickyUsersQueryKey
    } catch (err) {
      if ((err as Error).name === 'AbortError') return
      console.error(err)
      setStickyUsersError(err instanceof Error ? err.message : adminStrings.errors.loadKeyDetails)
      setStickyUsersLoadState('error')
    }
  }, [adminStrings.errors.loadKeyDetails, id, stickyUsersPage, stickyUsersPerPage, stickyUsersQueryKey])

  const loadStickyNodes = useCallback(async (reason: 'initial' | 'switch' | 'refresh' = 'refresh') => {
    stickyNodesAbortRef.current?.abort()
    const controller = new AbortController()
    stickyNodesAbortRef.current = controller
    try {
      const panelLoadState =
        reason === 'refresh'
          ? getRefreshingLoadState(stickyNodesQueryKeyRef.current != null)
          : getBlockingLoadState(stickyNodesQueryKeyRef.current != null)
      setStickyNodesLoadState(panelLoadState)
      setStickyNodesError(null)
      if (reason !== 'refresh') {
        setStickyNodes([])
      }
      const result = await fetchKeyStickyNodes(id, controller.signal)
      if (controller.signal.aborted) return
      setStickyNodes(result.nodes)
      setStickyNodesLoadState('ready')
      stickyNodesQueryKeyRef.current = id
    } catch (err) {
      if ((err as Error).name === 'AbortError') return
      console.error(err)
      setStickyNodesError(err instanceof Error ? err.message : adminStrings.errors.loadKeyDetails)
      setStickyNodesLoadState('error')
    }
  }, [adminStrings.errors.loadKeyDetails, id])

  useEffect(() => {
    const reason = queryKeyRef.current == null ? 'initial' : queryKeyRef.current === queryKey ? 'refresh' : 'switch'
    void load(reason)
    return () => {
      loadAbortRef.current?.abort()
    }
  }, [load, queryKey])

  useEffect(() => {
    const reason =
      stickyUsersQueryKeyRef.current == null
        ? 'initial'
        : stickyUsersQueryKeyRef.current === stickyUsersQueryKey
          ? 'refresh'
          : 'switch'
    void loadStickyUsers(reason)
    return () => {
      stickyUsersAbortRef.current?.abort()
    }
  }, [loadStickyUsers, stickyUsersQueryKey])

  useEffect(() => {
    const reason =
      stickyNodesQueryKeyRef.current == null
        ? 'initial'
        : stickyNodesQueryKeyRef.current === id
          ? 'refresh'
          : 'switch'
    void loadStickyNodes(reason)
    return () => {
      stickyNodesAbortRef.current?.abort()
    }
  }, [id, loadStickyNodes])

  useEffect(() => () => {
    if (syncFeedbackTimerRef.current != null) {
      window.clearTimeout(syncFeedbackTimerRef.current)
    }
  }, [])

  useEffect(() => {
    setQuarantineDetailExpanded(false)
    setStickyUsersPage(1)
    setLogsPerPage(20)
    setLogsCursor(null)
    setLogsDirection('older')
    setLogsPageInfo({ ...emptyRequestLogsListPage, pageSize: 20 })
    setKeyLogsCatalog(null)
    setKeyLogRequestKindOptions([])
    setKeyLogSelectedKinds([])
    setKeyLogQuickBilling('all')
    setKeyLogQuickProtocol('all')
    setKeyLogFacets(emptyRequestLogFacets)
    setKeyLogOutcomeFilter(null)
  }, [id])

  const syncUsage = useCallback(async () => {
    if (syncInFlightRef.current) return
    syncInFlightRef.current = true
    try {
      setSyncState('syncing')
      setError(null)
      await syncApiKeyUsage(id)
      await Promise.all([load('refresh'), loadStickyUsers('refresh'), loadStickyNodes('refresh')])
      setLogsNonce((value) => value + 1)
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
  }, [adminStrings.errors.syncUsage, id, load, loadStickyNodes, loadStickyUsers])

  const clearQuarantine = useCallback(async () => {
    if (quarantineState === 'clearing') return
    try {
      setQuarantineState('clearing')
      setError(null)
      await clearApiKeyQuarantine(id)
      await Promise.all([load('refresh'), loadStickyUsers('refresh'), loadStickyNodes('refresh')])
      setLogsNonce((value) => value + 1)
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : adminStrings.errors.clearQuarantine)
    } finally {
      setQuarantineState('idle')
    }
  }, [adminStrings.errors.clearQuarantine, id, load, loadStickyNodes, loadStickyUsers, quarantineState])

  const handleKeyLogQuickFilters = useCallback(
    (billing: TokenLogRequestKindQuickBilling, protocol: TokenLogRequestKindQuickProtocol) => {
      const nextFilters = { billing, protocol }
      setKeyLogQuickBilling(billing)
      setKeyLogQuickProtocol(protocol)
      setKeyLogSelectedKinds(buildRequestKindQuickFilterSelection(keyLogRequestKindOptions, nextFilters))
      setLogsCursor(null)
      setLogsDirection('older')
    },
    [keyLogRequestKindOptions],
  )

  const handleToggleKeyLogKind = useCallback(
    (key: string) => {
      const nextSelected = toggleRequestKindSelection(keyLogEffectiveKinds, key)
      const nextQuickFilters = resolveManualRequestKindQuickFilters(
        nextSelected,
        keyLogQuickFilters,
        keyLogQuickSelection,
        keyLogRequestKindOptions,
      )
      setKeyLogSelectedKinds(nextSelected)
      setKeyLogQuickBilling(nextQuickFilters.billing)
      setKeyLogQuickProtocol(nextQuickFilters.protocol)
      setLogsCursor(null)
      setLogsDirection('older')
    },
    [keyLogEffectiveKinds, keyLogQuickFilters, keyLogQuickSelection, keyLogRequestKindOptions],
  )

  const handleClearKeyLogKinds = useCallback(() => {
    setKeyLogSelectedKinds([])
    setKeyLogQuickBilling(defaultTokenLogRequestKindQuickFilters.billing)
    setKeyLogQuickProtocol(defaultTokenLogRequestKindQuickFilters.protocol)
    setLogsCursor(null)
    setLogsDirection('older')
  }, [])

  const handleKeyLogOutcomeFilter = useCallback((value: RecentRequestsOutcomeFilter | null) => {
    setKeyLogOutcomeFilter(value)
    setLogsCursor(null)
    setLogsDirection('older')
  }, [])

  const handleKeyLogPeriodChange = useCallback((value: string) => {
    setPeriod(value as 'day' | 'week' | 'month')
    setLogsCursor(null)
    setLogsDirection('older')
  }, [])

  const handleKeyLogStartDateChange = useCallback((value: string) => {
    setStartDate(value)
    setLogsCursor(null)
    setLogsDirection('older')
  }, [])

  const loadKeyLogBodies = useCallback(
    (log: RequestLog, signal: AbortSignal) => fetchKeyLogDetails(id, log.id, signal),
    [id],
  )

  const goNewerKeyLogsPage = () => {
    if (!logsPageInfo.prevCursor) return
    setLogsCursor(logsPageInfo.prevCursor)
    setLogsDirection('newer')
  }
  const goOlderKeyLogsPage = () => {
    if (!logsPageInfo.nextCursor) return
    setLogsCursor(logsPageInfo.nextCursor)
    setLogsDirection('older')
  }
  const changeKeyLogsPerPage = (value: number) => {
    setLogsPerPage(value)
    setLogsCursor(null)
    setLogsDirection('older')
  }

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
  const keyLogsDescription = useMemo(
    () => formatRequestLogsDescription(adminStrings.logs, keyLogsCatalog?.retentionDays),
    [adminStrings.logs, keyLogsCatalog?.retentionDays],
  )
  const keyLogsPaginationSummary = useMemo(
    () => formatRequestLogsPaginationSummary(adminStrings.logs, keyLogsCatalog?.retentionDays),
    [adminStrings.logs, keyLogsCatalog?.retentionDays],
  )
  const stickyUsersTotalPages = Math.max(1, Math.ceil(stickyUsersTotal / stickyUsersPerPage))
  const quarantineRawDetail = detail?.quarantine?.reasonDetail?.trim() ?? ''
  const hasQuarantineRawDetail = quarantineRawDetail.length > 0
  const keyDetailSidebarUtility = (
    <AdminShellSidebarUtility>
      <AdminSidebarUtilityStack>
        <AdminSidebarUtilityCard>
          <div className="admin-sidebar-utility-toolbar">
            <ThemeToggle />
          </div>
          <div className="admin-sidebar-utility-actions">
            <AdminReturnToConsoleLink
              label={adminStrings.header.returnToConsole}
              href={ADMIN_USER_CONSOLE_HREF}
              className="admin-sidebar-utility-action"
            />
            <Button type="button" variant="ghost" className="token-usage-back-button admin-sidebar-utility-action" onClick={onBack}>
              <Icon icon="mdi:arrow-left" width={18} height={18} />
              {keyDetailsStrings.back}
            </Button>
          </div>
        </AdminSidebarUtilityCard>

        <AdminSidebarUtilityCard>
          <div className="admin-sidebar-utility-actions">
            <Button
              type="button"
              variant={syncState === 'success' ? 'success' : 'default'}
              className="admin-sidebar-utility-action"
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
          </div>
        </AdminSidebarUtilityCard>
      </AdminSidebarUtilityStack>
    </AdminShellSidebarUtility>
  )

  return (
    <div className="admin-detail-stack">
      {keyDetailSidebarUtility}

      <div className="admin-stacked-only">
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
      </div>

      <div className="admin-desktop-only">
        <AdminCompactIntro
          title={keyDetailsStrings.title}
          description={(
            <>
              {keyDetailsStrings.descriptionPrefix} <code>{id}</code>
            </>
          )}
        />
      </div>

      {error && <div className="surface error-banner" style={{ marginTop: 8, marginBottom: 0 }}>{error}</div>}

      {detail && (
        <section className="surface panel">
          <div className="panel-header">
            <div>
              <h2>{keyDetailsStrings.metadata.title}</h2>
              <p className="panel-description">{keyDetailsStrings.metadata.description}</p>
            </div>
          </div>
          <div style={{ display: 'grid', gap: 12, gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))' }}>
            <div className="admin-mobile-kv">
              <span>{keyDetailsStrings.metadata.group}</span>
              <strong>{formatKeyGroupName(detail.group, keyStrings.groups.ungrouped)}</strong>
            </div>
            <div className="admin-mobile-kv">
              <span>{keyDetailsStrings.metadata.registrationIp}</span>
              <strong>{formatRegistrationValue(detail.registration_ip)}</strong>
            </div>
            <div className="admin-mobile-kv">
              <span>{keyDetailsStrings.metadata.registrationRegion}</span>
              <strong>{formatRegistrationValue(detail.registration_region)}</strong>
            </div>
          </div>
        </section>
      )}

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
            <Select value={period} onValueChange={handleKeyLogPeriodChange} disabled={detailBlocking}>
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
              onChange={(e) => handleKeyLogStartDateChange(e.target.value)}
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

      <AdminLazyBoundary loadingLabel={detailLoadingLabel} minHeight={260}>
        <LazyKeyStickyPanels
          stickyUsers={stickyUsers}
          stickyUsersLoadState={stickyUsersLoadState}
          stickyUsersError={stickyUsersError}
          stickyUsersPage={stickyUsersPage}
          stickyUsersTotal={stickyUsersTotal}
          stickyUsersPerPage={stickyUsersPerPage}
          onStickyUsersPrevious={() => setStickyUsersPage((current) => Math.max(1, current - 1))}
          onStickyUsersNext={() => setStickyUsersPage((current) => Math.min(stickyUsersTotalPages, current + 1))}
          stickyNodes={stickyNodes}
          stickyNodesLoadState={stickyNodesLoadState}
          stickyNodesError={stickyNodesError}
          onOpenUser={onOpenUser}
        />
      </AdminLazyBoundary>
      <AdminLazyBoundary loadingLabel={detailLoadingLabel} minHeight={320}>
        <LazyAdminRecentRequestsPanel
          variant="admin"
          language={language}
          strings={adminStrings}
          title={keyDetailsStrings.logsTitle}
          description={keyLogsDescription}
          emptyLabel={keyDetailsStrings.logsEmpty}
          loadState={logsLoadState}
          loadingLabel={logsLoadState === 'refreshing' ? loadingStateStrings.refreshing : detailLoadingLabel}
          errorLabel={logsError ?? loadingStateStrings.error}
          logs={logs}
          requestKindOptions={keyLogRequestKindOptions}
          requestKindQuickBilling={keyLogQuickBilling}
          requestKindQuickProtocol={keyLogQuickProtocol}
          selectedRequestKinds={keyLogSelectedKinds}
          onRequestKindQuickFiltersChange={handleKeyLogQuickFilters}
          onToggleRequestKind={handleToggleKeyLogKind}
          onClearRequestKinds={handleClearKeyLogKinds}
          outcomeFilter={keyLogOutcomeFilter}
          resultOptions={keyLogFacets.results}
          keyEffectOptions={keyLogFacets.keyEffects}
          bindingEffectOptions={keyLogFacets.bindingEffects}
          selectionEffectOptions={keyLogFacets.selectionEffects}
          onOutcomeFilterChange={handleKeyLogOutcomeFilter}
          keyOptions={[]}
          showKeyColumn={false}
          showTokenColumn
          onOpenToken={onOpenToken}
          perPage={logsPerPage}
          hasOlder={logsPageInfo.hasOlder}
          hasNewer={logsPageInfo.hasNewer}
          paginationSummary={keyLogsPaginationSummary}
          paginationDisabled={logsLoadState !== 'ready' && logsLoadState !== 'error'}
          onNewerPage={goNewerKeyLogsPage}
          onOlderPage={goOlderKeyLogsPage}
          onPerPageChange={changeKeyLogsPerPage}
          formatTime={formatTimestamp}
          formatTimeDetail={(ts) => (ts ? `${formatTimestampWithMs(ts)} · ${formatRelativeTime(ts)}` : '—')}
          loadLogBodies={loadKeyLogBodies}
        />
      </AdminLazyBoundary>
    </div>
  )
}

export default AdminDashboard
