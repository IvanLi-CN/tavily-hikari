import { Icon } from '../../lib/icons'
import type { Meta, StoryObj } from '@storybook/react-vite'
import { addons } from 'storybook/preview-api'
import { SELECT_STORY } from 'storybook/internal/core-events'
import { ArrowDown, ArrowUp, ArrowUpDown, ChartColumnIncreasing } from 'lucide-react'
import { Fragment, type KeyboardEvent as ReactKeyboardEvent, type ReactNode, useEffect, useLayoutEffect, useMemo, useState } from 'react'

import type {
  AlertCatalog,
  AlertEvent,
  AlertGroup,
  AlertsPage,
  ApiKeyBulkAction,
  AdminUnboundTokenUsageSortField,
  AdminUnboundTokenUsageSummary,
  AdminUserDetail,
  AdminUserSummary,
  AdminUsersSortField,
  AdminUserTag,
  AdminUserTagBinding,
  AdminUserTokenSummary,
  AdminUserUsageSeries,
  AdminUserUsageSeriesKey,
  ApiKeyStats,
  AuthToken,
  JobGroup,
  JobLogView,
  MonthlyBrokenKeyDetail,
  RecentAlertsSummary,
  RequestRate,
  RequestRateScope,
  RequestLog,
  RequestLogBodies,
  RequestLogsCatalog,
  RequestLogsListPage,
  SortDirection,
} from '../../api'
import AdminCompactIntro from '../../components/AdminCompactIntro'
import AdminPanelHeader from '../../components/AdminPanelHeader'
import AdminRecentRequestsPanel, { type RecentRequestsOutcomeFilter } from '../../components/AdminRecentRequestsPanel'
import AdminReturnToConsoleLink from '../../components/AdminReturnToConsoleLink'
import AdminTablePagination from '../../components/AdminTablePagination'
import JobKeyLink from '../../components/JobKeyLink'
import QuotaRangeField from '../../components/QuotaRangeField'
import { AdminSidebarUtilityCard, AdminSidebarUtilityStack } from '../../components/AdminSidebarUtility'
import LanguageSwitcher from '../../components/LanguageSwitcher'
import { StatusBadge, type StatusTone } from '../../components/StatusBadge'
import ThemeToggle from '../../components/ThemeToggle'
import { Button } from '../../components/ui/button'
import {
  Drawer,
  DrawerContent,
} from '../../components/ui/drawer'
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
} from '../../components/ui/dropdown-menu'
import { Input } from '../../components/ui/input'
import { Tooltip, TooltipContent, TooltipTrigger } from '../../components/ui/tooltip'
import { Card } from '../../components/ui/card'
import { Badge } from '../../components/ui/badge'
import { Switch } from '../../components/ui/switch'
import { LanguageProvider, useLanguage, useTranslate, type AdminTranslations } from '../../i18n'
import { KeyDetails } from '../../AdminDashboard'
import { TokenDetailStoryCanvas } from '../../pages/TokenDetail.stories'
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
} from '../../tokenLogRequestKinds'

import AdminShell, { AdminShellSidebarUtility, type AdminNavItem, type AdminNavTarget } from '../AdminShell'
import AlertsCenter from '../AlertsCenter'
import DashboardOverview, { type DashboardMetricCard } from '../DashboardOverview'
import { UserDetailQuotaBreakdown } from '../UserDetailQuotaBreakdown'
import { UserDetailSharedUsagePanel } from '../UserDetailSharedUsagePanel'
import { UserDetailTokenTable } from '../UserDetailTokenTable'
import {
  createDashboardMonthMetrics,
  createDashboardTodayMetrics,
} from '../dashboardTodayMetrics'
import {
  buildAdminJobFilterOptions,
  countAdminJobGroups,
  jobMatchesGroup,
  summarizeAdminJobFilter,
} from '../jobFilters'
import { buildDashboardHourlyRequestWindowFixture } from '../dashboardHourlyCharts'
import { ApiKeyBulkSyncProgressBubble } from '../ApiKeyBulkSyncProgressBubble'
import ForwardProxySettingsModule from '../ForwardProxySettingsModule'
import ModulePlaceholder from '../ModulePlaceholder'
import SystemSettingsModule from '../SystemSettingsModule'
import type { ApiKeyBulkSyncProgressState } from '../apiKeyBulkSyncProgress'
import { retainVisibleApiKeySelection } from '../apiKeySelection'
import {
  forwardProxyStorySavedAt,
  forwardProxyStorySettings,
  forwardProxyStoryStats,
} from '../forwardProxyStoryData'
import {
  stickyNodesStoryData,
  stickyUsersStoryData,
  stickyUsersStoryTotal,
} from '../keyStickyStoryData'
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
} from '../quotaSlider'
import { formatRequestRateSummary, resolveRequestRate } from '../../requestRate'
import AdminOverlayHost from '../AdminOverlayHost'
const now = 1_762_380_000
const ADMIN_USERS_DEFAULT_SORT_FIELD: AdminUsersSortField = 'lastLoginAt'
const ADMIN_USERS_DEFAULT_SORT_ORDER: SortDirection = 'desc'
const ADMIN_UNBOUND_TOKEN_USAGE_DEFAULT_SORT_FIELD: AdminUnboundTokenUsageSortField = 'lastUsedAt'
const ADMIN_UNBOUND_TOKEN_USAGE_DEFAULT_SORT_ORDER: SortDirection = 'desc'

const defaultDashboardHourlyRequestWindow = buildDashboardHourlyRequestWindowFixture({
  mapBucket: ({ index, bucket }) => ({
    secondarySuccess: (index % 4) + 1,
    primarySuccess: bucket.primarySuccess + (index % 2),
    secondaryFailure: index % 3,
    primaryFailure429: index % 7 === 0 ? 2 : bucket.primaryFailure429,
    primaryFailureOther: index % 5 === 0 ? 1 : bucket.primaryFailureOther,
    unknown: index % 13 === 0 ? 1 : 0,
    mcpNonBillable: index % 2,
    mcpBillable: (index % 4) + 3,
    apiNonBillable: index % 3,
    apiBillable: (index % 5) + 4,
  }),
})

function createRequestRate(
  used: number,
  limit: number,
  scope: RequestRateScope,
  windowMinutes = 5,
): RequestRate {
  return {
    used,
    limit,
    windowMinutes,
    scope,
  }
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

const tableStackStyle = {
  display: 'flex',
  flexDirection: 'column',
  gap: 4,
  minWidth: 0,
} as const

const tableFieldStyle = {
  whiteSpace: 'nowrap',
  lineHeight: 1.35,
} as const

const tableEllipsisFieldStyle = {
  ...tableFieldStyle,
  display: 'block',
  minWidth: 0,
  overflow: 'hidden',
  textOverflow: 'ellipsis',
} as const

const tableSecondaryFieldStyle = {
  ...tableFieldStyle,
  fontSize: '0.92em',
  opacity: 0.68,
} as const

const tableEllipsisSecondaryFieldStyle = {
  ...tableSecondaryFieldStyle,
  display: 'block',
  minWidth: 0,
  overflow: 'hidden',
  textOverflow: 'ellipsis',
} as const

const tableInlineFieldStyle = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 8,
  whiteSpace: 'nowrap',
  lineHeight: 1.35,
  position: 'relative',
  paddingRight: 40,
} as const

const tableHeaderStackStyle = {
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

function openAdminStory(storyId: string): void {
  addons.getChannel().emit(SELECT_STORY, { storyId })
}

const MOCK_TOKENS: AuthToken[] = [
  {
    id: '9vsN',
    enabled: true,
    note: 'Core production',
    group: 'production',
    owner: { userId: 'usr_alice', displayName: 'Alice Chen', username: 'alice' },
    total_requests: 32_640,
    created_at: now - 86_400 * 120,
    last_used_at: now - 320,
    quota_state: 'normal',
    quota_hourly_used: 218,
    quota_hourly_limit: 1_000,
    quota_daily_used: 4_833,
    quota_daily_limit: 20_000,
    quota_monthly_used: 143_200,
    quota_monthly_limit: 600_000,
    quota_hourly_reset_at: now + 2_340,
    quota_daily_reset_at: now + 43_200,
    quota_monthly_reset_at: now + 1_209_600,
  },
  {
    id: 'M8kQ',
    enabled: true,
    note: 'Batch enrichment',
    group: 'batch',
    owner: { userId: 'usr_ops_bot', displayName: 'Ops Bot', username: 'ops-bot' },
    total_requests: 21_884,
    created_at: now - 86_400 * 90,
    last_used_at: now - 1_200,
    quota_state: 'hour',
    quota_hourly_used: 980,
    quota_hourly_limit: 1_000,
    quota_daily_used: 10_402,
    quota_daily_limit: 20_000,
    quota_monthly_used: 198_833,
    quota_monthly_limit: 600_000,
    quota_hourly_reset_at: now + 840,
    quota_daily_reset_at: now + 43_200,
    quota_monthly_reset_at: now + 1_209_600,
  },
  {
    id: 'Lt2R',
    enabled: false,
    note: 'Legacy backup token',
    group: 'legacy',
    owner: null,
    total_requests: 7_201,
    created_at: now - 86_400 * 240,
    last_used_at: now - 86_400 * 2,
    quota_state: 'normal',
    quota_hourly_used: 0,
    quota_hourly_limit: 500,
    quota_daily_used: 0,
    quota_daily_limit: 5_000,
    quota_monthly_used: 133,
    quota_monthly_limit: 120_000,
    quota_hourly_reset_at: now + 1_200,
    quota_daily_reset_at: now + 43_200,
    quota_monthly_reset_at: now + 1_209_600,
  },
  {
    id: 'Vn7D',
    enabled: true,
    note: 'Realtime recommendation',
    group: 'production',
    owner: { userId: 'usr_bob', displayName: 'Bob Li', username: 'bobli' },
    total_requests: 19_901,
    created_at: now - 86_400 * 60,
    last_used_at: now - 42,
    quota_state: 'day',
    quota_hourly_used: 740,
    quota_hourly_limit: 1_200,
    quota_daily_used: 19_998,
    quota_daily_limit: 20_000,
    quota_monthly_used: 302_114,
    quota_monthly_limit: 600_000,
    quota_hourly_reset_at: now + 2_100,
    quota_daily_reset_at: now + 43_200,
    quota_monthly_reset_at: now + 1_209_600,
  },
  {
    id: 'Q4sE',
    enabled: true,
    note: 'Risk control',
    group: 'ops',
    owner: { userId: 'usr_risk', displayName: 'Risk Control', username: 'risk-control' },
    total_requests: 11_298,
    created_at: now - 86_400 * 30,
    last_used_at: now - 510,
    quota_state: 'month',
    quota_hourly_used: 415,
    quota_hourly_limit: 700,
    quota_daily_used: 6_410,
    quota_daily_limit: 8_000,
    quota_monthly_used: 95_912,
    quota_monthly_limit: 96_000,
    quota_hourly_reset_at: now + 2_700,
    quota_daily_reset_at: now + 43_200,
    quota_monthly_reset_at: now + 1_209_600,
  },
]

const MOCK_KEYS: ApiKeyStats[] = [
  {
    id: 'MZli',
    status: 'active',
    group: 'production',
    registration_ip: '8.8.8.8',
    registration_region: 'US',
    status_changed_at: now - 2_100,
    last_used_at: now - 61,
    deleted_at: null,
    quota_limit: 12_000,
    quota_remaining: 4_980,
    quota_synced_at: now - 300,
    total_requests: 19_840,
    success_count: 19_102,
    error_count: 631,
    quota_exhausted_count: 107,
    quarantine: null,
  },
  {
    id: 'asR8',
    status: 'exhausted',
    group: 'production',
    registration_ip: '8.8.4.4',
    registration_region: 'US Westfield (MA)',
    status_changed_at: now - 6_480,
    last_used_at: now - 2_300,
    deleted_at: null,
    quota_limit: 10_000,
    quota_remaining: 0,
    quota_synced_at: now - 200,
    total_requests: 16_113,
    success_count: 14_299,
    error_count: 1_142,
    quota_exhausted_count: 672,
    quarantine: null,
  },
  {
    id: 'U2vK',
    status: 'active',
    group: 'batch',
    registration_ip: '2606:4700:4700::1111',
    registration_region: null,
    status_changed_at: now - 4_200,
    last_used_at: now - 410,
    deleted_at: null,
    quota_limit: 25_000,
    quota_remaining: 8_640,
    quota_synced_at: now - 360,
    total_requests: 28_901,
    success_count: 28_211,
    error_count: 541,
    quota_exhausted_count: 149,
    quarantine: null,
  },
  {
    id: 'c7Pk',
    status: 'disabled',
    group: 'ops',
    registration_ip: null,
    registration_region: null,
    status_changed_at: now - 86_400,
    last_used_at: now - 86_400 * 2,
    deleted_at: null,
    quota_limit: 5_000,
    quota_remaining: 4_998,
    quota_synced_at: now - 4_200,
    total_requests: 599,
    success_count: 570,
    error_count: 29,
    quota_exhausted_count: 0,
    quarantine: null,
  },
  {
    id: 'J1nW',
    status: 'active',
    group: 'ops',
    registration_ip: '9.9.9.9',
    registration_region: null,
    status_changed_at: now - 1_800,
    last_used_at: now - 180,
    deleted_at: null,
    quota_limit: 8_000,
    quota_remaining: 1_043,
    quota_synced_at: now - 120,
    total_requests: 9_220,
    success_count: 8_672,
    error_count: 419,
    quota_exhausted_count: 129,
    quarantine: null,
  },
]

const MOCK_KEYS_WITH_QUARANTINE: ApiKeyStats[] = [
  {
    id: 'Qn8R',
    status: 'active',
    group: 'ops',
    registration_ip: '1.0.0.1',
    registration_region: 'HK',
    status_changed_at: now - 5_400,
    last_used_at: now - 196,
    deleted_at: null,
    quota_limit: 9_000,
    quota_remaining: 2_410,
    quota_synced_at: now - 240,
    total_requests: 12_008,
    success_count: 11_302,
    error_count: 622,
    quota_exhausted_count: 84,
    quarantine: {
      source: '/mcp',
      reasonCode: 'account_deactivated',
      reasonSummary: 'Tavily account deactivated (HTTP 401)',
      reasonDetail: 'The account associated with this API key has been deactivated.',
      createdAt: now - 196,
    },
  },
]

const STORY_BULK_SYNC_PROGRESS: ApiKeyBulkSyncProgressState = {
  steps: [
    {
      key: 'prepare_request',
      status: 'done',
      detail: 'Queued 6 key(s) for manual quota sync',
    },
    {
      key: 'sync_usage',
      status: 'running',
      detail: 'Tavily usage request failed with 401: {"error":"unauthorized"}',
    },
    {
      key: 'refresh_ui',
      status: 'pending',
      detail: null,
    },
  ],
  summary: {
    requested: 6,
    succeeded: 3,
    skipped: 1,
    failed: 1,
  },
  current: 5,
  total: 6,
  lastResult: {
    keyId: 'Qn8R',
    status: 'failed',
    detail: 'Tavily usage request failed with 401: {"error":"unauthorized"}',
  },
  message: 'Streaming live key-by-key results from the current request.',
  response: null,
  completed: false,
  error: null,
}

const MOCK_REQUESTS: RequestLog[] = [
  {
    id: 9501,
    key_id: 'MZli',
    auth_token_id: '9vsN',
    method: 'POST',
    path: '/api/tavily/search',
    query: null,
    http_status: 200,
    mcp_status: 200,
    business_credits: 2,
    request_kind_key: 'api:search',
    request_kind_label: 'API | search',
    request_kind_detail: null,
    result_status: 'success',
    created_at: now - 20,
    error_message: null,
    key_effect_code: 'none',
    key_effect_summary: 'No automatic key state change',
    request_body: '{"query":"tavily observability"}',
    response_body: '{"status":200}',
    forwarded_headers: ['x-request-id', 'x-forwarded-for'],
    dropped_headers: ['authorization'],
    operationalClass: 'success',
    requestKindProtocolGroup: 'api',
    requestKindBillingGroup: 'billable',
  },
  {
    id: 9500,
    key_id: 'asR8',
    auth_token_id: 'Vn7D',
    method: 'POST',
    path: '/mcp',
    query: null,
    http_status: 200,
    mcp_status: 429,
    business_credits: null,
    request_kind_key: 'mcp:crawl',
    request_kind_label: 'MCP | crawl',
    request_kind_detail: null,
    result_status: 'error',
    created_at: now - 74,
    error_message: 'Your request has been blocked due to excessive requests.',
    failure_kind: 'upstream_rate_limited_429',
    key_effect_code: 'none',
    key_effect_summary: 'No automatic key state change',
    request_body: '{"tool":"crawl"}',
    response_body: null,
    forwarded_headers: ['x-request-id'],
    dropped_headers: [],
    operationalClass: 'upstream_error',
    requestKindProtocolGroup: 'mcp',
    requestKindBillingGroup: 'billable',
  },
  {
    id: 9499,
    key_id: 'U2vK',
    auth_token_id: 'M8kQ',
    method: 'POST',
    path: '/api/tavily/search',
    query: null,
    http_status: 200,
    mcp_status: 432,
    business_credits: null,
    request_kind_key: 'api:search',
    request_kind_label: 'API | search',
    request_kind_detail: null,
    result_status: 'quota_exhausted',
    created_at: now - 118,
    error_message: 'Quota exhausted for this API key',
    key_effect_code: 'marked_exhausted',
    key_effect_summary: 'Automatically marked this key as exhausted',
    request_body: '{"query":"site reliability playbook"}',
    response_body: '{"status":432}',
    forwarded_headers: ['x-request-id'],
    dropped_headers: ['cookie'],
    operationalClass: 'quota_exhausted',
    requestKindProtocolGroup: 'api',
    requestKindBillingGroup: 'billable',
  },
  {
    id: 9498,
    key_id: 'Qn8R',
    auth_token_id: 'Q4sE',
    method: 'POST',
    path: '/mcp',
    query: null,
    http_status: 200,
    mcp_status: 401,
    business_credits: null,
    request_kind_key: 'mcp:map',
    request_kind_label: 'MCP | map',
    request_kind_detail: null,
    result_status: 'error',
    created_at: now - 196,
    error_message: 'The account associated with this API key has been deactivated.',
    failure_kind: 'upstream_account_deactivated_401',
    key_effect_code: 'quarantined',
    key_effect_summary: 'Automatically quarantined this key',
    request_body: '{"tool":"map"}',
    response_body: '{"status":401}',
    forwarded_headers: ['x-request-id'],
    dropped_headers: [],
    operationalClass: 'upstream_error',
    requestKindProtocolGroup: 'mcp',
    requestKindBillingGroup: 'billable',
  },
  {
    id: 9497,
    key_id: 'J1nW',
    auth_token_id: '9vsN',
    method: 'POST',
    path: '/api/tavily/extract',
    query: null,
    http_status: 502,
    mcp_status: 502,
    business_credits: null,
    request_kind_key: 'api:extract',
    request_kind_label: 'API | extract',
    request_kind_detail: null,
    result_status: 'error',
    created_at: now - 310,
    error_message: 'Bad gateway from upstream',
    failure_kind: 'upstream_gateway_5xx',
    key_effect_code: 'none',
    key_effect_summary: 'No automatic key state change',
    request_body: '{"urls":["https://example.com"]}',
    response_body: null,
    forwarded_headers: ['x-request-id'],
    dropped_headers: [],
    operationalClass: 'upstream_error',
    requestKindProtocolGroup: 'api',
    requestKindBillingGroup: 'billable',
  },
]

const STORY_REQUEST_KIND_OPTIONS: TokenLogRequestKindOption[] = [
  { key: 'api:extract', label: 'API | extract', protocol_group: 'api', billing_group: 'billable' },
  { key: 'api:research-result', label: 'API | research result', protocol_group: 'api', billing_group: 'non_billable' },
  { key: 'api:search', label: 'API | search', protocol_group: 'api', billing_group: 'billable' },
  { key: 'mcp:initialize', label: 'MCP | initialize', protocol_group: 'mcp', billing_group: 'non_billable' },
  { key: 'mcp:ping', label: 'MCP | ping', protocol_group: 'mcp', billing_group: 'non_billable' },
  { key: 'mcp:crawl', label: 'MCP | crawl', protocol_group: 'mcp', billing_group: 'billable' },
  { key: 'mcp:map', label: 'MCP | map', protocol_group: 'mcp', billing_group: 'billable' },
]
const STORY_REQUEST_LOG_RETENTION_DAYS = 32

function buildStoryLogFacetOptions(values: Array<string | null | undefined>): Array<{ value: string; count: number }> {
  const counts = new Map<string, number>()
  for (const raw of values) {
    const value = raw?.trim()
    if (!value) continue
    counts.set(value, (counts.get(value) ?? 0) + 1)
  }
  return Array.from(counts.entries())
    .sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))
    .map(([value, count]) => ({ value, count }))
}

function buildStoryRequestKindOptions(
  logs: RequestLog[],
  options: TokenLogRequestKindOption[],
): TokenLogRequestKindOption[] {
  return options.map((option) => ({
    ...option,
    count: logs.filter((log) => log.request_kind_key === option.key).length,
  }))
}

function stripRequestLogBodies(log: RequestLog): RequestLog {
  return {
    ...log,
    request_body: null,
    response_body: null,
  }
}

function lookupStoryLogBodies(logId: number) {
  const matched = MOCK_REQUESTS.find((log) => log.id === logId)
  return {
    request_body: matched?.request_body ?? null,
    response_body: matched?.response_body ?? null,
  }
}

function storyCursorForPage(page: number): string {
  return `page:${page}`
}

function parseStoryCursor(cursor: string | null | undefined): number | null {
  const normalized = cursor?.trim()
  if (!normalized) return null
  const match = normalized.match(/^page:(\d+)$/)
  if (!match) return null
  const page = Number(match[1])
  return Number.isFinite(page) && page > 0 ? page : null
}

function filterStoryRequestLogs(
  logs: RequestLog[],
  {
    requestKinds = [],
    result,
    keyEffect,
    bindingEffect,
    selectionEffect,
    tokenId,
    keyId,
    forceEmptyMatch = false,
  }: {
    requestKinds?: string[]
    result?: string
    keyEffect?: string
    bindingEffect?: string
    selectionEffect?: string
    tokenId?: string | null
    keyId?: string | null
    forceEmptyMatch?: boolean
  },
) {
  const normalizedRequestKinds = Array.from(new Set(requestKinds.map((value) => value.trim()).filter(Boolean)))
  return forceEmptyMatch
    ? []
    : logs.filter((log) => {
        if (normalizedRequestKinds.length > 0 && !normalizedRequestKinds.includes(log.request_kind_key ?? '')) {
          return false
        }
        if (result && log.result_status !== result) {
          return false
        }
        if (keyEffect && (log.key_effect_code ?? 'none') !== keyEffect) {
          return false
        }
        if (bindingEffect && (log.binding_effect_code ?? 'none') !== bindingEffect) {
          return false
        }
        if (selectionEffect && (log.selection_effect_code ?? 'none') !== selectionEffect) {
          return false
        }
        if (tokenId?.trim() && log.auth_token_id !== tokenId) {
          return false
        }
        if (keyId?.trim() && log.key_id !== keyId) {
          return false
        }
        return true
      })
}

function buildStoryRequestLogsCatalog(
  logs: RequestLog[],
  {
    showTokens,
    showKeys,
    retentionDays = STORY_REQUEST_LOG_RETENTION_DAYS,
  }: {
    showTokens: boolean
    showKeys: boolean
    retentionDays?: number
  },
) : RequestLogsCatalog {
  return {
    retentionDays,
    requestKindOptions: buildStoryRequestKindOptions(logs, STORY_REQUEST_KIND_OPTIONS),
    facets: {
      results: buildStoryLogFacetOptions(logs.map((log) => log.result_status)),
      keyEffects: buildStoryLogFacetOptions(logs.map((log) => log.key_effect_code ?? 'none')),
      bindingEffects: buildStoryLogFacetOptions(logs.map((log) => log.binding_effect_code ?? 'none')),
      selectionEffects: buildStoryLogFacetOptions(logs.map((log) => log.selection_effect_code ?? 'none')),
      tokens: showTokens ? buildStoryLogFacetOptions(logs.map((log) => log.auth_token_id)) : [],
      keys: showKeys ? buildStoryLogFacetOptions(logs.map((log) => log.key_id)) : [],
    },
  }
}

function buildStoryRequestLogsList(
  logs: RequestLog[],
  {
    cursor,
    limit,
    requestKinds = [],
    result,
    keyEffect,
    bindingEffect,
    selectionEffect,
    tokenId,
    keyId,
    forceEmptyMatch = false,
  }: {
    cursor?: string | null
    limit: number
    requestKinds?: string[]
    result?: string
    keyEffect?: string
    bindingEffect?: string
    selectionEffect?: string
    tokenId?: string | null
    keyId?: string | null
    forceEmptyMatch?: boolean
  },
): RequestLogsListPage {
  const filtered = filterStoryRequestLogs(logs, {
    requestKinds,
    result,
    keyEffect,
    bindingEffect,
    selectionEffect,
    tokenId,
    keyId,
    forceEmptyMatch,
  })
  const pageSize = Math.max(1, limit)
  const totalPages = Math.max(1, Math.ceil(filtered.length / pageSize))
  const currentPage = Math.min(parseStoryCursor(cursor) ?? 1, totalPages)
  const start = (currentPage - 1) * pageSize
  const items = filtered.slice(start, start + pageSize).map(stripRequestLogBodies)
  const hasOlder = currentPage < totalPages
  const hasNewer = currentPage > 1

  return {
    items,
    pageSize,
    nextCursor: hasOlder ? storyCursorForPage(currentPage + 1) : null,
    prevCursor: hasNewer ? storyCursorForPage(currentPage - 1) : null,
    hasOlder,
    hasNewer,
  }
}

const MOCK_JOBS: JobLogView[] = [
  {
    id: 612,
    job_type: 'linuxdo_user_status_sync',
    key_id: null,
    key_group: null,
    status: 'error',
    attempt: 1,
    message: 'attempted=18 success=17 skipped=0 failure=1 first_failure=hhf0517: token upstream status 400: {"error":"invalid_grant"}',
    started_at: now - 30,
    finished_at: now - 12,
  },
  {
    id: 611,
    job_type: 'forward_proxy_geo_refresh',
    key_id: null,
    key_group: null,
    status: 'success',
    attempt: 1,
    message: 'refreshed_candidates=11',
    started_at: now - 120,
    finished_at: now - 90,
  },
  {
    id: 610,
    job_type: 'quota_sync',
    key_id: 'MZli',
    key_group: 'ops',
    status: 'success',
    attempt: 1,
    message: 'Synced 125 keys',
    started_at: now - 240,
    finished_at: now - 210,
  },
  {
    id: 609,
    job_type: 'token_usage_rollup',
    key_id: 'U2vK',
    key_group: null,
    status: 'running',
    attempt: 1,
    message: 'Aggregating daily partitions',
    started_at: now - 420,
    finished_at: null,
  },
  {
    id: 608,
    job_type: 'quota_sync',
    key_id: 'asR8',
    key_group: 'batch',
    status: 'error',
    attempt: 3,
    message: 'Provider rejected usage API: HTTP 403',
    started_at: now - 1_620,
    finished_at: now - 1_560,
  },
  {
    id: 607,
    job_type: 'auth_token_logs_gc',
    key_id: null,
    key_group: null,
    status: 'success',
    attempt: 1,
    message: 'Pruned 1,260 old log rows',
    started_at: now - 3_200,
    finished_at: now - 3_090,
  },
]

const STORY_ALERT_EVENTS_PAGE: AlertsPage<AlertEvent> = {
  page: 1,
  perPage: 20,
  total: 5,
  items: [
    {
      id: 'alert_evt_401',
      type: 'upstream_usage_limit_432',
      title: '上游用量限制 432',
      summary: 'Alice Wang 的 Tavily Search 请求被 Tavily usage limit 432 拒绝。',
      occurredAt: now - 180,
      subjectKind: 'user',
      subjectId: 'usr_alice',
      subjectLabel: 'Alice Wang',
      user: { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
      token: { id: 'tok_req_001', label: 'tok_req_001' },
      key: { id: 'MZli', label: 'MZli' },
      request: { id: 3001, method: 'POST', path: '/api/tavily/search', query: null },
      requestKind: { key: 'tavily_search', label: 'Tavily Search', detail: 'POST /api/tavily/search' },
      failureKind: null,
      resultStatus: 'quota_exhausted',
      errorMessage: "This request exceeds your plan's set usage limit.",
      reasonCode: null,
      reasonSummary: null,
      reasonDetail: null,
      source: { kind: 'auth_token_log', id: 'log_3001' },
    },
    {
      id: 'alert_evt_402',
      type: 'upstream_rate_limited_429',
      title: '上游返回 429',
      summary: '上游对 tok_req_001 的 Tavily Search 请求返回 429。',
      occurredAt: now - 520,
      subjectKind: 'user',
      subjectId: 'usr_alice',
      subjectLabel: 'Alice Wang',
      user: { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
      token: { id: 'tok_req_001', label: 'tok_req_001' },
      key: { id: 'MZli', label: 'MZli' },
      request: { id: 3002, method: 'POST', path: '/api/tavily/search', query: 'max_results=5' },
      requestKind: { key: 'tavily_search', label: 'Tavily Search', detail: 'POST /api/tavily/search' },
      failureKind: 'upstream_rate_limited_429',
      resultStatus: 'error',
      errorMessage: 'HTTP 429 from upstream',
      reasonCode: null,
      reasonSummary: null,
      reasonDetail: null,
      source: { kind: 'auth_token_log', id: 'log_3002' },
    },
    {
      id: 'alert_evt_403',
      type: 'user_request_rate_limited',
      title: '用户请求限流',
      summary: 'Bob Chen 的 MCP Search 请求命中本地 request-rate 限流。',
      occurredAt: now - 1_440,
      subjectKind: 'user',
      subjectId: 'usr_bob',
      subjectLabel: 'Bob Chen',
      user: { userId: 'usr_bob', displayName: 'Bob Chen', username: 'bob' },
      token: { id: 'tok_req_002', label: 'tok_req_002' },
      key: null,
      request: { id: 3003, method: 'POST', path: '/mcp', query: null },
      requestKind: { key: 'mcp_search', label: 'MCP Search', detail: 'POST /mcp' },
      failureKind: null,
      resultStatus: 'quota_exhausted',
      errorMessage: 'hourly any-request limit exceeded',
      reasonCode: null,
      reasonSummary: null,
      reasonDetail: null,
      source: { kind: 'auth_token_log', id: 'log_3003' },
    },
    {
      id: 'alert_evt_404',
      type: 'upstream_key_blocked',
      title: '上游 Key 封禁',
      summary: 'MZli 因上游账号停用被隔离。',
      occurredAt: now - 2_040,
      subjectKind: 'key',
      subjectId: 'MZli',
      subjectLabel: 'MZli',
      user: null,
      token: null,
      key: { id: 'MZli', label: 'MZli' },
      request: null,
      requestKind: { key: 'mcp_search', label: 'MCP Search', detail: 'POST /mcp' },
      failureKind: null,
      resultStatus: null,
      errorMessage: null,
      reasonCode: 'account_deactivated',
      reasonSummary: 'Upstream account deactivated',
      reasonDetail: 'The upstream provider returned a deactivated-account response and the key entered quarantine.',
      source: { kind: 'api_key_maintenance_record', id: 'maint_3004' },
    },
    {
      id: 'alert_evt_405',
      type: 'user_quota_exhausted',
      title: '用户额度耗尽',
      summary: 'Alice Wang 的 Tavily Extract 请求触发本地业务额度上限。',
      occurredAt: now - 2_280,
      subjectKind: 'user',
      subjectId: 'usr_alice',
      subjectLabel: 'Alice Wang',
      user: { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
      token: { id: 'tok_req_003', label: 'tok_req_003' },
      key: null,
      request: { id: 3004, method: 'POST', path: '/api/tavily/extract', query: null },
      requestKind: { key: 'tavily_extract', label: 'Tavily Extract', detail: 'POST /api/tavily/extract' },
      failureKind: null,
      resultStatus: 'quota_exhausted',
      errorMessage: 'monthly business quota exhausted',
      reasonCode: null,
      reasonSummary: null,
      reasonDetail: null,
      source: { kind: 'auth_token_log', id: 'log_3004' },
    },
  ],
}

const STORY_ALERT_GROUPS_PAGE: AlertsPage<AlertGroup> = {
  page: 1,
  perPage: 20,
  total: 4,
  items: [
    {
      id: 'group:upstream_usage_limit_432:user:usr_alice:tavily_search',
      type: 'upstream_usage_limit_432',
      subjectKind: 'user',
      subjectId: 'usr_alice',
      subjectLabel: 'Alice Wang',
      user: { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
      token: { id: 'tok_req_001', label: 'tok_req_001' },
      key: { id: 'MZli', label: 'MZli' },
      requestKind: { key: 'tavily_search', label: 'Tavily Search', detail: 'POST /api/tavily/search' },
      count: 2,
      firstSeen: now - 3_600,
      lastSeen: now - 180,
      latestEvent: STORY_ALERT_EVENTS_PAGE.items[0],
    },
    {
      id: 'group:upstream_rate_limited_429:user:usr_alice:tavily_search',
      type: 'upstream_rate_limited_429',
      subjectKind: 'user',
      subjectId: 'usr_alice',
      subjectLabel: 'Alice Wang',
      user: { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
      token: { id: 'tok_req_001', label: 'tok_req_001' },
      key: { id: 'MZli', label: 'MZli' },
      requestKind: { key: 'tavily_search', label: 'Tavily Search', detail: 'POST /api/tavily/search' },
      count: 1,
      firstSeen: now - 520,
      lastSeen: now - 520,
      latestEvent: STORY_ALERT_EVENTS_PAGE.items[1],
    },
    {
      id: 'group:upstream_key_blocked:key:MZli:mcp_search',
      type: 'upstream_key_blocked',
      subjectKind: 'key',
      subjectId: 'MZli',
      subjectLabel: 'MZli',
      user: null,
      token: null,
      key: { id: 'MZli', label: 'MZli' },
      requestKind: { key: 'mcp_search', label: 'MCP Search', detail: 'POST /mcp' },
      count: 1,
      firstSeen: now - 2_040,
      lastSeen: now - 2_040,
      latestEvent: STORY_ALERT_EVENTS_PAGE.items[3],
    },
    {
      id: 'group:user_quota_exhausted:user:usr_alice:tavily_extract',
      type: 'user_quota_exhausted',
      subjectKind: 'user',
      subjectId: 'usr_alice',
      subjectLabel: 'Alice Wang',
      user: { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
      token: { id: 'tok_req_003', label: 'tok_req_003' },
      key: null,
      requestKind: { key: 'tavily_extract', label: 'Tavily Extract', detail: 'POST /api/tavily/extract' },
      count: 1,
      firstSeen: now - 2_280,
      lastSeen: now - 2_280,
      latestEvent: STORY_ALERT_EVENTS_PAGE.items[4],
    },
  ],
}

const STORY_ALERT_CATALOG: AlertCatalog = {
  retentionDays: 30,
  types: [
    { value: 'upstream_rate_limited_429', count: 1 },
    { value: 'upstream_usage_limit_432', count: 1 },
    { value: 'upstream_key_blocked', count: 1 },
    { value: 'user_request_rate_limited', count: 1 },
    { value: 'user_quota_exhausted', count: 1 },
  ],
  requestKindOptions: [
    { key: 'tavily_search', label: 'Tavily Search', protocol_group: 'api', billing_group: 'billable', count: 2 },
    { key: 'tavily_extract', label: 'Tavily Extract', protocol_group: 'api', billing_group: 'billable', count: 1 },
    { key: 'mcp_search', label: 'MCP Search', protocol_group: 'mcp', billing_group: 'billable', count: 2 },
  ],
  users: [
    { value: 'usr_alice', label: 'Alice Wang', count: 3 },
    { value: 'usr_bob', label: 'Bob Chen', count: 1 },
  ],
  tokens: [
    { value: 'tok_req_001', label: 'tok_req_001', count: 2 },
    { value: 'tok_req_002', label: 'tok_req_002', count: 1 },
    { value: 'tok_req_003', label: 'tok_req_003', count: 1 },
  ],
  keys: [
    { value: 'MZli', label: 'MZli', count: 2 },
  ],
}

const STORY_ALERT_REQUEST_BODIES: Record<number, RequestLogBodies> = {
  3001: {
    request_body: JSON.stringify({ query: 'quota exhausted', max_results: 5 }, null, 2),
    response_body: JSON.stringify({ error: 'quota exhausted' }, null, 2),
  },
  3002: {
    request_body: JSON.stringify({ query: '429', max_results: 5 }, null, 2),
    response_body: JSON.stringify({ status: 429, detail: 'rate limit' }, null, 2),
  },
  3003: {
    request_body: JSON.stringify({ method: 'tools/call', params: { name: 'tavily_search' } }, null, 2),
    response_body: JSON.stringify({ error: 'hourly any-request limit exceeded' }, null, 2),
  },
  3004: {
    request_body: JSON.stringify({ urls: ['https://example.com/plan'], extract_depth: 'advanced' }, null, 2),
    response_body: JSON.stringify({ error: 'monthly business quota exhausted' }, null, 2),
  },
}

const STORY_RECENT_ALERTS: RecentAlertsSummary = {
  windowHours: 24,
  totalEvents: STORY_ALERT_EVENTS_PAGE.total,
  groupedCount: STORY_ALERT_GROUPS_PAGE.total,
  countsByType: [
    { type: 'upstream_rate_limited_429', count: 1 },
    { type: 'upstream_usage_limit_432', count: 1 },
    { type: 'upstream_key_blocked', count: 1 },
    { type: 'user_request_rate_limited', count: 1 },
    { type: 'user_quota_exhausted', count: 1 },
  ],
  topGroups: STORY_ALERT_GROUPS_PAGE.items,
}

const DEFAULT_LINUXDO_TAG_DELTA = {
  hourlyAnyDelta: 500,
  hourlyDelta: 100,
  dailyDelta: 500,
  monthlyDelta: 5_000,
} as const

const MOCK_TAG_CATALOG: AdminUserTag[] = [
  {
    id: 'linuxdo_l0',
    name: 'linuxdo_l0',
    displayName: 'L0',
    icon: 'linuxdo',
    systemKey: 'linuxdo_l0',
    effectKind: 'quota_delta',
    ...DEFAULT_LINUXDO_TAG_DELTA,
    userCount: 0,
  },
  {
    id: 'linuxdo_l1',
    name: 'linuxdo_l1',
    displayName: 'L1',
    icon: 'linuxdo',
    systemKey: 'linuxdo_l1',
    effectKind: 'quota_delta',
    ...DEFAULT_LINUXDO_TAG_DELTA,
    userCount: 0,
  },
  {
    id: 'linuxdo_l2',
    name: 'linuxdo_l2',
    displayName: 'L2',
    icon: 'linuxdo',
    systemKey: 'linuxdo_l2',
    effectKind: 'quota_delta',
    ...DEFAULT_LINUXDO_TAG_DELTA,
    userCount: 1,
  },
  {
    id: 'linuxdo_l3',
    name: 'linuxdo_l3',
    displayName: 'L3',
    icon: 'linuxdo',
    systemKey: 'linuxdo_l3',
    effectKind: 'quota_delta',
    ...DEFAULT_LINUXDO_TAG_DELTA,
    userCount: 0,
  },
  {
    id: 'linuxdo_l4',
    name: 'linuxdo_l4',
    displayName: 'L4',
    icon: 'linuxdo',
    systemKey: 'linuxdo_l4',
    effectKind: 'quota_delta',
    ...DEFAULT_LINUXDO_TAG_DELTA,
    userCount: 1,
  },
  {
    id: 'team_lead',
    name: 'team_lead',
    displayName: 'Team Lead',
    icon: 'sparkles',
    systemKey: null,
    effectKind: 'quota_delta',
    hourlyAnyDelta: 120,
    hourlyDelta: 180,
    dailyDelta: 2_000,
    monthlyDelta: 100_000,
    userCount: 1,
  },
  {
    id: 'debt_cap',
    name: 'debt_cap',
    displayName: 'Debt Cap',
    icon: 'minus-circle',
    systemKey: null,
    effectKind: 'quota_delta',
    hourlyAnyDelta: -50,
    hourlyDelta: -80,
    dailyDelta: -1_000,
    monthlyDelta: -700_000,
    userCount: 1,
  },
  {
    id: 'suspended_manual',
    name: 'suspended_manual',
    displayName: 'Suspended',
    icon: 'ban',
    systemKey: null,
    effectKind: 'block_all',
    hourlyAnyDelta: 0,
    hourlyDelta: 0,
    dailyDelta: 0,
    monthlyDelta: 0,
    userCount: 1,
  },
]

const MOCK_ALICE_TAGS: AdminUserTagBinding[] = [
  {
    tagId: 'linuxdo_l2',
    name: 'linuxdo_l2',
    displayName: 'L2',
    icon: 'linuxdo',
    systemKey: 'linuxdo_l2',
    effectKind: 'quota_delta',
    ...DEFAULT_LINUXDO_TAG_DELTA,
    source: 'system_linuxdo',
  },
  {
    tagId: 'team_lead',
    name: 'team_lead',
    displayName: 'Team Lead',
    icon: 'sparkles',
    systemKey: null,
    effectKind: 'quota_delta',
    hourlyAnyDelta: 120,
    hourlyDelta: 180,
    dailyDelta: 2_000,
    monthlyDelta: 100_000,
    source: 'manual',
  },
  {
    tagId: 'debt_cap',
    name: 'debt_cap',
    displayName: 'Debt Cap',
    icon: 'minus-circle',
    systemKey: null,
    effectKind: 'quota_delta',
    hourlyAnyDelta: -50,
    hourlyDelta: -80,
    dailyDelta: -1_000,
    monthlyDelta: -700_000,
    source: 'manual',
  },
]

const MOCK_BOB_TAGS: AdminUserTagBinding[] = [
  {
    tagId: 'linuxdo_l4',
    name: 'linuxdo_l4',
    displayName: 'L4',
    icon: 'linuxdo',
    systemKey: 'linuxdo_l4',
    effectKind: 'quota_delta',
    ...DEFAULT_LINUXDO_TAG_DELTA,
    source: 'system_linuxdo',
  },
  {
    tagId: 'suspended_manual',
    name: 'suspended_manual',
    displayName: 'Suspended',
    icon: 'ban',
    systemKey: null,
    effectKind: 'block_all',
    hourlyAnyDelta: 0,
    hourlyDelta: 0,
    dailyDelta: 0,
    monthlyDelta: 0,
    source: 'manual',
  },
]

const MOCK_USERS: AdminUserSummary[] = [
  {
    userId: 'usr_alice',
    displayName: 'Alice Wang',
    username: 'alice',
    active: true,
    lastLoginAt: now - 420,
    tokenCount: 2,
    apiKeyCount: 3,
    tags: MOCK_ALICE_TAGS,
    requestRate: createRequestRate(58, 60, 'user'),
    hourlyAnyUsed: 58,
    hourlyAnyLimit: 60,
    quotaHourlyUsed: 1_118,
    quotaHourlyLimit: 1_200,
    quotaDailyUsed: 5_201,
    quotaDailyLimit: 25_500,
    quotaMonthlyUsed: 142_922,
    quotaMonthlyLimit: 5_000,
    dailySuccess: 4_998,
    dailyFailure: 203,
    monthlySuccess: 129_442,
    monthlyFailure: 3_180,
    monthlyBrokenCount: 3,
    monthlyBrokenLimit: 5,
    lastActivity: now - 25,
  },
  {
    userId: 'usr_bob',
    displayName: 'Bob Chen',
    username: 'bob',
    active: true,
    lastLoginAt: now - 2_700,
    tokenCount: 1,
    apiKeyCount: 2,
    tags: MOCK_BOB_TAGS,
    requestRate: createRequestRate(60, 60, 'user'),
    hourlyAnyUsed: 60,
    hourlyAnyLimit: 60,
    quotaHourlyUsed: 602,
    quotaHourlyLimit: 0,
    quotaDailyUsed: 10_009,
    quotaDailyLimit: 0,
    quotaMonthlyUsed: 231_008,
    quotaMonthlyLimit: 0,
    dailySuccess: 9_800,
    dailyFailure: 209,
    monthlySuccess: 201_402,
    monthlyFailure: 8_614,
    monthlyBrokenCount: 5,
    monthlyBrokenLimit: 6,
    lastActivity: now - 38,
  },
  {
    userId: 'usr_charlie',
    displayName: 'Charlie Li',
    username: 'charlie',
    active: false,
    lastLoginAt: now - 86_400 * 6,
    tokenCount: 0,
    apiKeyCount: 0,
    tags: [],
    requestRate: createRequestRate(0, 60, 'user'),
    hourlyAnyUsed: 0,
    hourlyAnyLimit: 60,
    quotaHourlyUsed: 0,
    quotaHourlyLimit: 500,
    quotaDailyUsed: 0,
    quotaDailyLimit: 8_000,
    quotaMonthlyUsed: 0,
    quotaMonthlyLimit: 96_000,
    dailySuccess: 0,
    dailyFailure: 0,
    monthlySuccess: 122,
    monthlyFailure: 7,
    monthlyBrokenCount: 0,
    monthlyBrokenLimit: 5,
    lastActivity: null,
  },
]

const MOCK_USER_TOKENS: AdminUserTokenSummary[] = [
  {
    tokenId: 'V3P2',
    enabled: true,
    note: 'Primary production',
    createdAt: now - 120 * 86_400,
    lastUsedAt: now - 24,
    totalRequests: 48_204,
    dailySuccess: 2_701,
    dailyFailure: 139,
    monthlySuccess: 39_420,
  },
  {
    tokenId: 'R8K1',
    enabled: true,
    note: 'Batch backfill',
    createdAt: now - 46 * 86_400,
    lastUsedAt: now - 400,
    totalRequests: 16_288,
    dailySuccess: 2_297,
    dailyFailure: 64,
    monthlySuccess: 90_022,
  },
]

const MOCK_USER_USAGE_SERIES: Record<AdminUserUsageSeriesKey, AdminUserUsageSeries> = {
  quota1h: {
    limit: 500,
    points: Array.from({ length: 72 }, (_, index) => ({
      bucketStart: now - (71 - index) * 3_600,
      value: [44, 88, 129, 160, 202, 233][index % 6] + Math.floor(index / 10) * 6,
      limitValue: index < 18 ? 360 : index < 42 ? 420 : 500,
    })),
  },
  rate5m: {
    limit: 100,
    points: Array.from({ length: 288 }, (_, index) => ({
      bucketStart: now - (287 - index) * 300,
      value: index % 17 === 0 ? 92 : 24 + ((index * 7) % 39),
      limitValue: index < 144 ? 80 : 100,
    })),
  },
  quota24h: {
    limit: 12_000,
    points: Array.from({ length: 7 }, (_, index) => ({
      bucketStart: now - (6 - index) * 86_400,
      value: [5_120, 5_840, 6_210, 7_080, 8_420, 6_980, 4_912][index],
      limitValue: index < 3 ? 9_000 : 12_000,
    })),
  },
  quotaMonth: {
    limit: 160_000,
    points: Array.from({ length: 12 }, (_, index) => ({
      bucketStart: Date.UTC(2025, 4 + index, 1, 0, 0, 0) / 1000,
      value: index < 6
        ? null
        : [42_000, 58_200, 66_400, 73_880, 89_120, 97_360][index - 6],
      limitValue: index < 6
        ? null
        : index < 9
          ? 120_000
          : 160_000,
    })),
  },
}

const MOCK_UNBOUND_TOKEN_USAGE: AdminUnboundTokenUsageSummary[] = [
  {
    tokenId: 'qa13',
    enabled: true,
    note: 'Sandbox smoke traffic',
    group: 'sandbox',
    requestRate: createRequestRate(12, 60, 'token'),
    hourlyAnyUsed: 12,
    hourlyAnyLimit: 60,
    quotaHourlyUsed: 9,
    quotaHourlyLimit: 300,
    quotaDailyUsed: 124,
    quotaDailyLimit: 1_500,
    quotaMonthlyUsed: 2_912,
    quotaMonthlyLimit: 30_000,
    dailySuccess: 118,
    dailyFailure: 6,
    monthlySuccess: 2_744,
    monthlyFailure: 168,
    monthlyBrokenCount: 2,
    monthlyBrokenLimit: 2,
    lastUsedAt: now - 180,
  },
  {
    tokenId: 'ops7',
    enabled: false,
    note: 'Legacy import runner',
    group: 'ops',
    requestRate: createRequestRate(60, 60, 'token'),
    hourlyAnyUsed: 60,
    hourlyAnyLimit: 60,
    quotaHourlyUsed: 61,
    quotaHourlyLimit: 120,
    quotaDailyUsed: 402,
    quotaDailyLimit: 800,
    quotaMonthlyUsed: 7_884,
    quotaMonthlyLimit: 12_000,
    dailySuccess: 310,
    dailyFailure: 92,
    monthlySuccess: 5_874,
    monthlyFailure: 2_010,
    monthlyBrokenCount: 1,
    monthlyBrokenLimit: 2,
    lastUsedAt: now - 3_600,
  },
  {
    tokenId: 'tmp4',
    enabled: true,
    note: null,
    group: null,
    requestRate: createRequestRate(4, 60, 'token'),
    hourlyAnyUsed: 4,
    hourlyAnyLimit: 60,
    quotaHourlyUsed: 3,
    quotaHourlyLimit: 80,
    quotaDailyUsed: 28,
    quotaDailyLimit: 400,
    quotaMonthlyUsed: 301,
    quotaMonthlyLimit: 6_000,
    dailySuccess: 27,
    dailyFailure: 1,
    monthlySuccess: 296,
    monthlyFailure: 5,
    monthlyBrokenCount: null,
    monthlyBrokenLimit: null,
    lastUsedAt: now - 86_400,
  },
]

const MOCK_USER_DETAIL: AdminUserDetail = {
  ...MOCK_USERS[0],
  tokens: MOCK_USER_TOKENS,
  quotaBase: {
    hourlyAnyLimit: 1_200,
    hourlyLimit: 1_000,
    dailyLimit: 24_000,
    monthlyLimit: 600_000,
    inheritsDefaults: false,
  },
  effectiveQuota: {
    hourlyAnyLimit: 1_770,
    hourlyLimit: 1_200,
    dailyLimit: 25_500,
    monthlyLimit: 5_000,
    inheritsDefaults: false,
  },
  quotaBreakdown: [
    {
      kind: 'base',
      label: 'base',
      tagId: null,
      tagName: null,
      source: null,
      effectKind: 'base',
      hourlyAnyDelta: 1_200,
      hourlyDelta: 1_000,
      dailyDelta: 24_000,
      monthlyDelta: 600_000,
    },
    {
      kind: 'tag',
      label: 'L2',
      tagId: 'linuxdo_l2',
      tagName: 'linuxdo_l2',
      source: 'system_linuxdo',
      effectKind: 'quota_delta',
      ...DEFAULT_LINUXDO_TAG_DELTA,
    },
    {
      kind: 'tag',
      label: 'Team Lead',
      tagId: 'team_lead',
      tagName: 'team_lead',
      source: 'manual',
      effectKind: 'quota_delta',
      hourlyAnyDelta: 120,
      hourlyDelta: 180,
      dailyDelta: 2_000,
      monthlyDelta: 100_000,
    },
    {
      kind: 'tag',
      label: 'Debt Cap',
      tagId: 'debt_cap',
      tagName: 'debt_cap',
      source: 'manual',
      effectKind: 'quota_delta',
      hourlyAnyDelta: -50,
      hourlyDelta: -80,
      dailyDelta: -1_000,
      monthlyDelta: -700_000,
    },
    {
      kind: 'effective',
      label: 'effective',
      tagId: null,
      tagName: null,
      source: null,
      effectKind: 'effective',
      hourlyAnyDelta: 1_770,
      hourlyDelta: 1_200,
      dailyDelta: 25_500,
      monthlyDelta: 5_000,
    },
  ],
}

const MOCK_MONTHLY_BROKEN_ITEMS: Record<string, MonthlyBrokenKeyDetail[]> = {
  'user:usr_alice': [
    {
      keyId: 'key_prod_a',
      currentStatus: 'quarantined',
      reasonCode: 'manual_quarantine',
      reasonSummary: '确认该 Key 被上游封禁',
      latestBreakAt: now - 3_600,
      source: 'manual',
      breakerTokenId: '9vsN',
      breakerUserId: 'usr_alice',
      breakerUserDisplayName: 'Alice Wang',
      manualActorDisplayName: null,
      relatedUsers: [{ userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' }],
    },
    {
      keyId: 'key_prod_c',
      currentStatus: 'exhausted',
      reasonCode: 'quota_exhausted',
      reasonSummary: '本月额度已耗尽',
      latestBreakAt: now - 7_200,
      source: 'auto',
      breakerTokenId: '9vsN',
      breakerUserId: 'usr_alice',
      breakerUserDisplayName: 'Alice Wang',
      manualActorDisplayName: null,
      relatedUsers: [{ userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' }],
    },
    {
      keyId: 'key_batch_f',
      currentStatus: 'quarantined',
      reasonCode: 'manual_quarantine',
      reasonSummary: '发现同账户已被风控',
      latestBreakAt: now - 14_400,
      source: 'manual',
      breakerTokenId: null,
      breakerUserId: 'usr_alice',
      breakerUserDisplayName: 'Alice Wang',
      manualActorDisplayName: null,
      relatedUsers: [
        { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
        { userId: 'usr_bob', displayName: 'Bob Chen', username: 'bob' },
      ],
    },
  ],
  'user:usr_bob': [
    {
      keyId: 'key_prod_b',
      currentStatus: 'quarantined',
      reasonCode: 'manual_quarantine',
      reasonSummary: '确认该 Key 被上游封禁',
      latestBreakAt: now - 2_000,
      source: 'manual',
      breakerTokenId: 'Vn7D',
      breakerUserId: 'usr_bob',
      breakerUserDisplayName: 'Bob Chen',
      manualActorDisplayName: null,
      relatedUsers: [{ userId: 'usr_bob', displayName: 'Bob Chen', username: 'bob' }],
    },
    {
      keyId: 'key_prod_d',
      currentStatus: 'exhausted',
      reasonCode: 'quota_exhausted',
      reasonSummary: '本月额度已耗尽',
      latestBreakAt: now - 4_600,
      source: 'auto',
      breakerTokenId: 'Vn7D',
      breakerUserId: 'usr_bob',
      breakerUserDisplayName: 'Bob Chen',
      manualActorDisplayName: null,
      relatedUsers: [{ userId: 'usr_bob', displayName: 'Bob Chen', username: 'bob' }],
    },
    {
      keyId: 'key_ops_j',
      currentStatus: 'quarantined',
      reasonCode: 'manual_quarantine',
      reasonSummary: '发现同账户多个 Key 同时失效',
      latestBreakAt: now - 9_200,
      source: 'manual',
      breakerTokenId: 'Vn7D',
      breakerUserId: 'usr_bob',
      breakerUserDisplayName: 'Bob Chen',
      manualActorDisplayName: null,
      relatedUsers: [
        { userId: 'usr_bob', displayName: 'Bob Chen', username: 'bob' },
        { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
      ],
    },
    {
      keyId: 'key_ops_k',
      currentStatus: 'exhausted',
      reasonCode: 'quota_exhausted',
      reasonSummary: '本月额度已耗尽',
      latestBreakAt: now - 16_200,
      source: 'auto',
      breakerTokenId: 'Vn7D',
      breakerUserId: 'usr_bob',
      breakerUserDisplayName: 'Bob Chen',
      manualActorDisplayName: null,
      relatedUsers: [{ userId: 'usr_bob', displayName: 'Bob Chen', username: 'bob' }],
    },
    {
      keyId: 'key_ops_l',
      currentStatus: 'quarantined',
      reasonCode: 'manual_quarantine',
      reasonSummary: '确认该 Key 被上游封禁',
      latestBreakAt: now - 28_800,
      source: 'manual',
      breakerTokenId: null,
      breakerUserId: 'usr_bob',
      breakerUserDisplayName: 'Bob Chen',
      manualActorDisplayName: null,
      relatedUsers: [{ userId: 'usr_bob', displayName: 'Bob Chen', username: 'bob' }],
    },
  ],
  'token:qa13': [
    {
      keyId: 'key_sandbox_a',
      currentStatus: 'quarantined',
      reasonCode: 'manual_quarantine',
      reasonSummary: '确认该 Key 被上游封禁',
      latestBreakAt: now - 4_800,
      source: 'manual',
      breakerTokenId: 'qa13',
      breakerUserId: null,
      breakerUserDisplayName: null,
      manualActorDisplayName: null,
      relatedUsers: [],
    },
    {
      keyId: 'key_sandbox_c',
      currentStatus: 'exhausted',
      reasonCode: 'quota_exhausted',
      reasonSummary: '本月额度已耗尽',
      latestBreakAt: now - 9_600,
      source: 'auto',
      breakerTokenId: 'qa13',
      breakerUserId: null,
      breakerUserDisplayName: null,
      manualActorDisplayName: null,
      relatedUsers: [],
    },
  ],
  'token:ops7': [
    {
      keyId: 'key_ops_z',
      currentStatus: 'quarantined',
      reasonCode: 'manual_quarantine',
      reasonSummary: '确认该 Key 被上游封禁',
      latestBreakAt: now - 6_200,
      source: 'manual',
      breakerTokenId: 'ops7',
      breakerUserId: null,
      breakerUserDisplayName: null,
      manualActorDisplayName: null,
      relatedUsers: [],
    },
  ],
}

const MONTHLY_BROKEN_DRAWER_SINGLE_ITEM: MonthlyBrokenKeyDetail[] = MOCK_MONTHLY_BROKEN_ITEMS['user:usr_alice'].slice(
  0,
  1,
)

const MONTHLY_BROKEN_DRAWER_LONG_CONTENT_ITEMS: MonthlyBrokenKeyDetail[] = [
  {
    ...MOCK_MONTHLY_BROKEN_ITEMS['user:usr_alice'][0],
    keyId: 'key_enterprise_cn_001',
    latestBreakAt: now - 1_500,
    reasonSummary:
      '系统确认同一上游账号下多个区域节点在短时间内连续进入风控，本主体当前仍关联该 Key，因此继续计入本月蹬坏统计。',
    relatedUsers: [
      { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
      { userId: 'usr_bob', displayName: 'Bob Chen', username: 'bob' },
      { userId: 'usr_evelyn', displayName: 'Evelyn Zhang', username: 'evelyn.ops' },
    ],
  },
  {
    ...MOCK_MONTHLY_BROKEN_ITEMS['user:usr_alice'][1],
    keyId: 'key_enterprise_cn_002',
    latestBreakAt: now - 4_200,
    breakerTokenId: 'A2Q9',
    reasonSummary:
      '该 Key 对应的上游配额在本月批量任务中被耗尽，系统保留最后一次触发记录，便于管理员回溯是谁把额度踩空。',
    relatedUsers: [
      { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
      { userId: 'usr_nina', displayName: 'Nina Zhou', username: 'nina' },
    ],
  },
]

const MONTHLY_BROKEN_DRAWER_OVERFLOW_ITEMS: MonthlyBrokenKeyDetail[] = Array.from({ length: 16 }, (_, index) => ({
  keyId: `key_overflow_${String(index + 1).padStart(2, '0')}`,
  currentStatus: index % 3 === 0 ? 'quarantined' : 'exhausted',
  reasonCode: index % 3 === 0 ? 'manual_quarantine' : 'quota_exhausted',
  reasonSummary:
    index % 3 === 0
      ? `第 ${index + 1} 把 Key 被系统判定为仍处于上游封禁态，用于验证多条记录时抽屉高度会上限收口，并且需要依赖抽屉内部滚动浏览后续条目。`
      : `第 ${index + 1} 把 Key 在批量请求中耗尽本月额度，用于验证超长列表时表格区域改为内部滚动，而不是继续把抽屉整体无限拉高。`,
  latestBreakAt: now - 1_200 * (index + 1),
  source: index % 3 === 0 ? 'manual' : 'auto',
  breakerTokenId: index % 2 === 0 ? 'qa13' : 'ops7',
  breakerUserId: null,
  breakerUserDisplayName: null,
  manualActorDisplayName: null,
  relatedUsers:
    index % 2 === 0
      ? []
      : [
          { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
          { userId: 'usr_bob', displayName: 'Bob Chen', username: 'bob' },
        ],
}))

const numberFormatter = new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 })
const percentFormatter = new Intl.NumberFormat('en-US', { style: 'percent', minimumFractionDigits: 1, maximumFractionDigits: 1 })
const dateTimeFormatter = new Intl.DateTimeFormat(undefined, {
  month: 'short',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit',
  second: '2-digit',
})
const englishDateOnlyFormatter = new Intl.DateTimeFormat('en-US', {
  year: 'numeric',
  month: 'short',
  day: '2-digit',
})

function formatNumber(value: number): string {
  return numberFormatter.format(value)
}

function formatPercent(numerator: number, denominator: number): string {
  if (denominator <= 0) return '0%'
  return percentFormatter.format(numerator / denominator)
}

function formatTimestamp(value: number | null): string {
  if (!value) return '—'
  return dateTimeFormatter.format(new Date(value * 1000))
}

function formatClockTime(value: number | null): string {
  if (!value) return '—'
  return new Intl.DateTimeFormat(undefined, {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  }).format(new Date(value * 1000))
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

function clampDisplayedQuota(value: number): number {
  return Math.max(0, value)
}

function formatQuotaLimitValue(value: number): string {
  return formatNumber(clampDisplayedQuota(value))
}

function formatQuotaUsagePair(used: number, limit: number): string {
  return `${formatNumber(Math.max(0, used))} / ${formatQuotaLimitValue(limit)}`
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

function formatQuotaStackValue(used: number, limit: number): { primary: string; secondary: string; primaryClassName?: string } {
  return {
    primary: formatNumber(Math.max(0, used)),
    secondary: formatQuotaLimitValue(limit),
    primaryClassName: quotaUsagePrimaryClassName(used, limit) ?? undefined,
  }
}

function formatSuccessRateStackValue(
  success: number,
  failure: number,
  language: 'en' | 'zh',
): { primary: string; secondary: string } {
  return {
    primary: success + failure > 0 ? formatPercent(success, success + failure) : '—',
    secondary: language === 'zh' ? `失败 ${formatNumber(failure)}` : `Fail ${formatNumber(failure)}`,
  }
}

function formatCompactSuccessRateValue(success: number, failure: number, language: 'en' | 'zh'): string {
  const total = success + failure
  const rate = total > 0 ? formatPercent(success, total) : '—'
  const failureLabel = language === 'zh' ? '失败' : 'Fail'
  return `${rate} · ${failureLabel} ${formatNumber(failure)}`
}

function formatStackedTimestamp(value: number | null, language: 'en' | 'zh'): { primary: string; secondary?: string } {
  if (!value) return { primary: '—' }
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

function formatMonthlyBrokenStackValue(
  count: number,
  limit: number,
): { primary: string; secondary: string; primaryClassName?: string } {
  return {
    primary: formatNumber(Math.max(0, count)),
    secondary: formatQuotaLimitValue(limit),
    primaryClassName: monthlyBrokenPrimaryClassName(count, limit) ?? undefined,
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
  strings: Pick<AdminTranslations['users']['brokenKeys'], 'breakerSystem' | 'breakerUnknown'>,
): string {
  if (item.breakerUserDisplayName) return item.breakerUserDisplayName
  if (item.breakerUserId) return item.breakerUserId
  if (item.breakerTokenId) return item.breakerTokenId
  if (item.source === 'manual') return strings.breakerSystem
  return strings.breakerUnknown
}

function StoryMonthlyBrokenKeyValue({
  keyId,
  ungroupedLabel,
  detailLabel,
  copyLabel,
  copiedLabel,
  copied,
  onCopy,
}: {
  keyId: string
  ungroupedLabel: string
  detailLabel: string
  copyLabel: string
  copiedLabel: string
  copied: boolean
  onCopy: () => void | Promise<void>
}): JSX.Element {
  return (
    <div className="monthly-broken-key-value">
      <JobKeyLink
        keyId={keyId}
        keyGroup={null}
        ungroupedLabel={ungroupedLabel}
        detailLabel={detailLabel}
        showBubble={false}
        onOpenKey={() => openAdminStory('admin-pages-keydetailroute--c-bo-x-review')}
      />
      <Button
        type="button"
        variant={copied ? 'success' : 'ghost'}
        size="icon"
        className="monthly-broken-key-copy-button shadow-none"
        title={copied ? copiedLabel : copyLabel}
        aria-label={copied ? copiedLabel : copyLabel}
        onClick={() => void onCopy()}
      >
        <Icon
          icon={copied ? 'mdi:check' : 'mdi:content-copy'}
          width={16}
          height={16}
          aria-hidden="true"
        />
      </Button>
    </div>
  )
}

function StoryMonthlyBrokenDrawer({
  open,
  label,
  items,
  onOpenChange,
}: {
  open: boolean
  label: string
  items: MonthlyBrokenKeyDetail[]
  onOpenChange: (open: boolean) => void
}): JSX.Element {
  const admin = useTranslate().admin
  const users = admin.users
  const keyStrings = admin.keys
  const [copiedKeyId, setCopiedKeyId] = useState<string | null>(null)

  useEffect(() => {
    if (!copiedKeyId) return
    const timer = window.setTimeout(() => setCopiedKeyId(null), 2_000)
    return () => window.clearTimeout(timer)
  }, [copiedKeyId])

  const handleCopy = async (keyId: string) => {
    try {
      if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(keyId)
      }
    } catch {
      // Storybook proof only needs deterministic copied-state feedback.
    }
    setCopiedKeyId(keyId)
  }

  return (
    <Drawer open={open} onOpenChange={onOpenChange} shouldScaleBackground={false}>
      <DrawerContent className="request-entity-drawer-content-fit">
        <div className="request-entity-drawer-body-fit">
          <section className="surface panel">
            <div className="panel-header" style={{ gap: 12, flexWrap: 'wrap' }}>
              <div>
                <h2>{users.brokenKeys.drawerTitle}</h2>
                <p className="panel-description">
                  {users.brokenKeys.drawerDescription.replace('{label}', label)}
                </p>
              </div>
            </div>
            {items.length === 0 ? (
              <div className="empty-state alert">{users.brokenKeys.empty}</div>
            ) : (
              <>
                <div className="table-wrapper jobs-table-wrapper admin-responsive-up">
                  <table className="jobs-table admin-users-table">
                    <thead>
                      <tr>
                        <th>{users.brokenKeys.table.key}</th>
                        <th>{users.brokenKeys.table.status}</th>
                        <th>{users.brokenKeys.table.reason}</th>
                        <th>{users.brokenKeys.table.latestBreakAt}</th>
                        <th>{users.brokenKeys.table.breaker}</th>
                        <th>{users.brokenKeys.table.relatedUsers}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {items.map((item) => (
                        <tr key={`${item.keyId}:${item.latestBreakAt}`}>
                          <td>
                            <StoryMonthlyBrokenKeyValue
                              keyId={item.keyId}
                              ungroupedLabel={keyStrings.groups.ungrouped}
                              detailLabel={keyStrings.actions.details}
                              copyLabel={users.brokenKeys.actions.copyKeyId}
                              copiedLabel={users.brokenKeys.actions.copied}
                              copied={copiedKeyId === item.keyId}
                              onCopy={() => handleCopy(item.keyId)}
                            />
                          </td>
                          <td>
                            <StatusBadge tone={item.currentStatus === 'quarantined' ? 'warning' : 'error'}>
                              {admin.statuses[item.currentStatus] ?? item.currentStatus}
                            </StatusBadge>
                          </td>
                          <td>{item.reasonSummary || item.reasonCode || users.brokenKeys.noReason}</td>
                          <td>{formatTimestamp(item.latestBreakAt)}</td>
                          <td>{formatMonthlyBrokenBreaker(item, users.brokenKeys)}</td>
                          <td>
                            {formatMonthlyBrokenRelatedUsers(
                              item.relatedUsers,
                              users.brokenKeys.noRelatedUsers,
                            )}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
                <div className="admin-mobile-list admin-responsive-down">
                  {items.map((item) => (
                    <article key={`${item.keyId}:${item.latestBreakAt}`} className="admin-mobile-card">
                      <div className="admin-mobile-kv">
                        <span>{users.brokenKeys.table.key}</span>
                        <strong>
                          <StoryMonthlyBrokenKeyValue
                            keyId={item.keyId}
                            ungroupedLabel={keyStrings.groups.ungrouped}
                            detailLabel={keyStrings.actions.details}
                            copyLabel={users.brokenKeys.actions.copyKeyId}
                            copiedLabel={users.brokenKeys.actions.copied}
                            copied={copiedKeyId === item.keyId}
                            onCopy={() => handleCopy(item.keyId)}
                          />
                        </strong>
                      </div>
                      <div className="admin-mobile-kv">
                        <span>{users.brokenKeys.table.status}</span>
                        <strong>{admin.statuses[item.currentStatus] ?? item.currentStatus}</strong>
                      </div>
                      <div className="admin-mobile-kv">
                        <span>{users.brokenKeys.table.reason}</span>
                        <strong>{item.reasonSummary || item.reasonCode || users.brokenKeys.noReason}</strong>
                      </div>
                      <div className="admin-mobile-kv">
                        <span>{users.brokenKeys.table.latestBreakAt}</span>
                        <strong>{formatTimestamp(item.latestBreakAt)}</strong>
                      </div>
                      <div className="admin-mobile-kv">
                        <span>{users.brokenKeys.table.breaker}</span>
                        <strong>{formatMonthlyBrokenBreaker(item, users.brokenKeys)}</strong>
                      </div>
                      <div className="admin-mobile-kv">
                        <span>{users.brokenKeys.table.relatedUsers}</span>
                        <strong>
                          {formatMonthlyBrokenRelatedUsers(
                            item.relatedUsers,
                            users.brokenKeys.noRelatedUsers,
                          )}
                        </strong>
                      </div>
                    </article>
                  ))}
                </div>
              </>
            )}
          </section>
        </div>
      </DrawerContent>
    </Drawer>
  )
}

function MonthlyBrokenDrawerStoryCanvas({
  label,
  items,
}: {
  label: string
  items: MonthlyBrokenKeyDetail[]
}): JSX.Element {
  return (
    <AdminPageFrame
      activeModule="users"
      overlays={<StoryMonthlyBrokenDrawer open label={label} items={items} onOpenChange={() => undefined} />}
    >
      <section className="surface panel">
        <div className="panel-header" style={{ gap: 12, flexWrap: 'wrap' }}>
          <div>
            <h2>Monthly Broken Drawer Sandbox</h2>
            <p className="panel-description">Focused Storybook surface for verifying adaptive drawer height.</p>
          </div>
        </div>
        <div className="empty-state alert">Background reference only. Use the open drawer to inspect sizing.</div>
      </section>
    </AdminPageFrame>
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

function compareScalar(left: number, right: number): number {
  if (left < right) return -1
  if (left > right) return 1
  return 0
}

function compareBigInt(left: bigint, right: bigint): number {
  if (left < right) return -1
  if (left > right) return 1
  return 0
}

function applySortDirection(ordering: number, direction: SortDirection): number {
  return direction === 'asc' ? ordering : -ordering
}

function compareOptionalTimestamp(
  left: number | null,
  right: number | null,
  direction: SortDirection,
): number {
  if (left != null && right != null) {
    return applySortDirection(compareScalar(left, right), direction)
  }
  if (left != null) return -1
  if (right != null) return 1
  return 0
}

function compareQuotaUsage(
  leftUsed: number,
  leftLimit: number,
  rightUsed: number,
  rightLimit: number,
  direction: SortDirection,
): number {
  const usedOrder = applySortDirection(compareScalar(leftUsed, rightUsed), direction)
  if (usedOrder !== 0) return usedOrder
  return applySortDirection(compareScalar(leftLimit, rightLimit), direction)
}

function compareOptionalQuotaUsage(
  leftUsed: number | null,
  leftLimit: number | null,
  rightUsed: number | null,
  rightLimit: number | null,
  direction: SortDirection,
): number {
  if (leftUsed != null && leftLimit != null && rightUsed != null && rightLimit != null) {
    return compareQuotaUsage(leftUsed, leftLimit, rightUsed, rightLimit, direction)
  }
  if (leftUsed != null && leftLimit != null) return -1
  if (rightUsed != null && rightLimit != null) return 1
  return 0
}

function compareSuccessRate(
  leftSuccess: number,
  leftFailure: number,
  rightSuccess: number,
  rightFailure: number,
  direction: SortDirection,
): number {
  const leftTotal = leftSuccess + leftFailure
  const rightTotal = rightSuccess + rightFailure
  if (leftTotal === 0 && rightTotal === 0) return 0
  if (leftTotal === 0) return 1
  if (rightTotal === 0) return -1

  const leftRatio = BigInt(leftSuccess) * BigInt(rightTotal)
  const rightRatio = BigInt(rightSuccess) * BigInt(leftTotal)
  const ratioOrder = applySortDirection(compareBigInt(leftRatio, rightRatio), direction)
  if (ratioOrder !== 0) return ratioOrder

  return applySortDirection(compareScalar(leftFailure, rightFailure), direction)
}

function compareUserId(left: string, right: string): number {
  return left.localeCompare(right)
}

function compareAdminUserSummaryRows(
  left: AdminUserSummary,
  right: AdminUserSummary,
  sort: AdminUsersSortField | null,
  order: SortDirection | null,
): number {
  const sortField = sort ?? ADMIN_USERS_DEFAULT_SORT_FIELD
  const direction = order ?? ADMIN_USERS_DEFAULT_SORT_ORDER

  const ordering = (() => {
    switch (sortField) {
      case 'hourlyAnyUsed':
        return compareQuotaUsage(
          left.hourlyAnyUsed,
          left.hourlyAnyLimit,
          right.hourlyAnyUsed,
          right.hourlyAnyLimit,
          direction,
        )
      case 'quotaHourlyUsed':
        return compareQuotaUsage(
          left.quotaHourlyUsed,
          left.quotaHourlyLimit,
          right.quotaHourlyUsed,
          right.quotaHourlyLimit,
          direction,
        )
      case 'quotaDailyUsed':
        return compareQuotaUsage(
          left.quotaDailyUsed,
          left.quotaDailyLimit,
          right.quotaDailyUsed,
          right.quotaDailyLimit,
          direction,
        )
      case 'quotaMonthlyUsed':
        return compareQuotaUsage(
          left.quotaMonthlyUsed,
          left.quotaMonthlyLimit,
          right.quotaMonthlyUsed,
          right.quotaMonthlyLimit,
          direction,
        )
      case 'monthlyBrokenCount':
        return compareOptionalQuotaUsage(
          left.monthlyBrokenCount,
          left.monthlyBrokenLimit,
          right.monthlyBrokenCount,
          right.monthlyBrokenLimit,
          direction,
        )
      case 'dailySuccessRate':
        return compareSuccessRate(
          left.dailySuccess,
          left.dailyFailure,
          right.dailySuccess,
          right.dailyFailure,
          direction,
        )
      case 'monthlySuccessRate':
        return compareSuccessRate(
          left.monthlySuccess,
          left.monthlyFailure,
          right.monthlySuccess,
          right.monthlyFailure,
          direction,
        )
      case 'lastActivity':
        return compareOptionalTimestamp(left.lastActivity, right.lastActivity, direction)
      case 'lastLoginAt':
        return compareOptionalTimestamp(left.lastLoginAt, right.lastLoginAt, direction)
      default:
        return 0
    }
  })()

  if (ordering !== 0) return ordering
  return compareUserId(left.userId, right.userId)
}

function normalizeStorySearchQuery(query: string): string {
  return query.trim()
}

function useStorySearchController(initialQuery = ''): {
  queryInput: string
  query: string
  applySearch: () => void
  resetSearch: () => void
  handleQueryInputChange: (value: string) => void
  handleQueryInputKeyDown: (event: ReactKeyboardEvent<HTMLInputElement>) => void
} {
  const [queryInput, setQueryInput] = useState(initialQuery)
  const [query, setQuery] = useState(initialQuery)

  const applySearch = () => {
    const normalized = normalizeStorySearchQuery(queryInput)
    setQueryInput(normalized)
    setQuery(normalized)
  }

  const resetSearch = () => {
    setQueryInput('')
    setQuery('')
  }

  const handleQueryInputChange = (value: string) => {
    setQueryInput(value)
  }

  const handleQueryInputKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') {
      event.preventDefault()
      applySearch()
    }
  }

  return {
    queryInput,
    query,
    applySearch,
    resetSearch,
    handleQueryInputChange,
    handleQueryInputKeyDown,
  }
}

function compareAdminUnboundTokenUsageRows(
  left: AdminUnboundTokenUsageSummary,
  right: AdminUnboundTokenUsageSummary,
  sort: AdminUnboundTokenUsageSortField | null,
  order: SortDirection | null,
): number {
  const sortField = sort ?? ADMIN_UNBOUND_TOKEN_USAGE_DEFAULT_SORT_FIELD
  const direction = order ?? ADMIN_UNBOUND_TOKEN_USAGE_DEFAULT_SORT_ORDER

  const ordering = (() => {
    switch (sortField) {
      case 'hourlyAnyUsed':
        return compareQuotaUsage(
          left.hourlyAnyUsed,
          left.hourlyAnyLimit,
          right.hourlyAnyUsed,
          right.hourlyAnyLimit,
          direction,
        )
      case 'quotaHourlyUsed':
        return compareQuotaUsage(
          left.quotaHourlyUsed,
          left.quotaHourlyLimit,
          right.quotaHourlyUsed,
          right.quotaHourlyLimit,
          direction,
        )
      case 'quotaDailyUsed':
        return compareQuotaUsage(
          left.quotaDailyUsed,
          left.quotaDailyLimit,
          right.quotaDailyUsed,
          right.quotaDailyLimit,
          direction,
        )
      case 'quotaMonthlyUsed':
        return compareQuotaUsage(
          left.quotaMonthlyUsed,
          left.quotaMonthlyLimit,
          right.quotaMonthlyUsed,
          right.quotaMonthlyLimit,
          direction,
        )
      case 'monthlyBrokenCount':
        return compareOptionalQuotaUsage(
          left.monthlyBrokenCount,
          left.monthlyBrokenLimit,
          right.monthlyBrokenCount,
          right.monthlyBrokenLimit,
          direction,
        )
      case 'dailySuccessRate':
        return compareSuccessRate(
          left.dailySuccess,
          left.dailyFailure,
          right.dailySuccess,
          right.dailyFailure,
          direction,
        )
      case 'monthlySuccessRate':
        return compareSuccessRate(
          left.monthlySuccess,
          left.monthlyFailure,
          right.monthlySuccess,
          right.monthlyFailure,
          direction,
        )
      case 'lastUsedAt':
        return compareOptionalTimestamp(left.lastUsedAt, right.lastUsedAt, direction)
      default:
        return 0
    }
  })()

  if (ordering !== 0) return ordering
  return compareUserId(left.tokenId, right.tokenId)
}

function StoryAdminUsersSortableHeader<Field extends string>({
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
  activeField: Field
  activeOrder: SortDirection
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
      data-sort-field={field}
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
          <TooltipContent side="top">{bubbleLabel}</TooltipContent>
        </Tooltip>
      ) : (
        trigger
      )}
    </th>
  )
}

function formatSignedQuotaDelta(value: number): string {
  if (value > 0) return `+${formatNumber(value)}`
  return formatNumber(value)
}

function getUserTagIconSrc(icon: string | null | undefined): string | null {
  return icon === 'linuxdo' ? '/linuxdo-logo.svg' : null
}

function isSystemUserTag(tag: { systemKey?: string | null; source?: string | null }): boolean {
  return Boolean(tag.systemKey) || tag.source === 'system_linuxdo'
}

function StoryUserTagBadge({
  tag,
  users,
}: {
  tag: Pick<AdminUserTagBinding, 'displayName' | 'icon' | 'systemKey' | 'effectKind'> & { source?: string | null }
  users: AdminTranslations['users']
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
      {isSystem && <span className="user-tag-pill-meta">{users.catalog.scopeSystemShort}</span>}
      {isBlockAll && <span className="user-tag-pill-meta">{users.catalog.blockShort}</span>}
    </Badge>
  )
}

function StoryUserTagBadgeList({
  tags,
  users,
  emptyLabel,
  limit,
}: {
  tags: AdminUserTagBinding[]
  users: AdminTranslations['users']
  emptyLabel: string
  limit?: number
}): JSX.Element {
  if (tags.length === 0) {
    return <span className="panel-description">{emptyLabel}</span>
  }
  const visibleTags = limit == null ? tags : tags.slice(0, limit)
  const overflow = limit == null ? 0 : Math.max(0, tags.length - visibleTags.length)
  return (
    <div className="user-tag-pill-list">
      {visibleTags.map((tag) => (
        <StoryUserTagBadge key={`${tag.tagId}:${tag.source}`} tag={tag} users={users} />
      ))}
      {overflow > 0 && <Badge variant="outline" className="user-tag-pill-overflow">+{overflow}</Badge>}
    </div>
  )
}

type StoryQuotaSnapshot = Record<QuotaSliderField, QuotaSliderSeed>

function buildStoryQuotaSnapshot(detail: AdminUserDetail): StoryQuotaSnapshot {
  return {
    hourlyAnyLimit: createQuotaSliderSeed('hourlyAnyLimit', detail.hourlyAnyUsed, detail.quotaBase.hourlyAnyLimit),
    hourlyLimit: createQuotaSliderSeed('hourlyLimit', detail.quotaHourlyUsed, detail.quotaBase.hourlyLimit),
    dailyLimit: createQuotaSliderSeed('dailyLimit', detail.quotaDailyUsed, detail.quotaBase.dailyLimit),
    monthlyLimit: createQuotaSliderSeed('monthlyLimit', detail.quotaMonthlyUsed, detail.quotaBase.monthlyLimit),
  }
}

type StoryTagCardMode = 'view' | 'edit' | 'new'

function StoryUserTagEffectToggle({ users, active }: { users: AdminTranslations['users']; active: 'quota_delta' | 'block_all' }): JSX.Element {
  return (
    <div className="user-tag-effect-toggle" role="group" aria-label={users.catalog.fields.effect}>
      {([
        ['quota_delta', users.catalog.effectKinds.quotaDelta],
        ['block_all', users.catalog.effectKinds.blockAll],
      ] as const).map(([effectKind, label]) => (
        <Button
          key={effectKind}
          type="button"
          variant={active === effectKind ? 'secondary' : 'outline'}
          size="xs"
          className={`user-tag-effect-chip${active === effectKind ? ' is-active' : ''}`}
        >
          {label}
        </Button>
      ))}
    </div>
  )
}

function StoryUserTagCatalogCard({
  tag,
  users,
  mode = 'view',
}: {
  tag?: AdminUserTag | null
  users: AdminTranslations['users']
  mode?: StoryTagCardMode
}): JSX.Element {
  const isNewCard = mode === 'new'
  const isEditing = mode === 'edit' || mode === 'new'
  const draft = tag ?? {
    id: 'draft',
    name: '',
    displayName: '',
    icon: '',
    systemKey: null,
    effectKind: 'quota_delta',
    hourlyAnyDelta: 0,
    hourlyDelta: 0,
    dailyDelta: 0,
    monthlyDelta: 0,
    userCount: 0,
  }
  const isSystem = Boolean(draft.systemKey)
  const isBlockAll = draft.effectKind === 'block_all'
  const iconSrc = getUserTagIconSrc(draft.icon)
  const classes = [
    'user-tag-catalog-card',
    isEditing ? 'user-tag-catalog-card-active' : '',
    isNewCard ? 'user-tag-catalog-card-draft' : '',
  ]
    .filter(Boolean)
    .join(' ')

  return (
    <Card className={classes}>
      <div className="user-tag-catalog-card-head">
        <div className="user-tag-catalog-name">
          {isEditing ? (
            <div className="user-tag-inline-fields">
              <Input
                type="text"
                className="user-tag-inline-input user-tag-inline-input-display"
                defaultValue={draft.displayName}
                disabled={isSystem}
                placeholder={users.catalog.fields.displayName}
              />
              <div className="user-tag-inline-fields-row">
                <Input
                  type="text"
                  className="user-tag-inline-input"
                  defaultValue={draft.name}
                  disabled={isSystem}
                  placeholder={users.catalog.fields.name}
                />
                <Input
                  type="text"
                  className="user-tag-inline-input"
                  defaultValue={draft.icon ?? ''}
                  disabled={isSystem}
                  placeholder={users.catalog.iconPlaceholder}
                />
              </div>
            </div>
          ) : (
            <>
              <div className="user-tag-pill-list">
                <StoryUserTagBadge tag={{ ...draft }} users={users} />
              </div>
              <div className="panel-description user-tag-catalog-subtitle">
                <code>{draft.name}</code>
                {iconSrc ? ` · ${draft.icon}` : ''}
              </div>
            </>
          )}
        </div>
        <div className="user-tag-catalog-actions">
          {isEditing ? (
            <>
              <Button type="button" variant="ghost" size="sm" className="user-tag-catalog-icon-button" aria-label={users.catalog.actions.save}>
                <Icon icon="mdi:check" width={16} height={16} />
              </Button>
              <Button type="button" variant="ghost" size="sm" className="user-tag-catalog-icon-button" aria-label={users.catalog.actions.cancelEdit}>
                <Icon icon="mdi:close" width={16} height={16} />
              </Button>
              {!isSystem && !isNewCard && (
                <Button type="button" variant="ghost" size="sm" className="user-tag-catalog-icon-button" aria-label={users.catalog.actions.delete}>
                  <Icon icon="mdi:trash-can-outline" width={16} height={16} />
                </Button>
              )}
            </>
          ) : (
            <>
              <Button type="button" variant="ghost" size="sm" className="user-tag-catalog-icon-button" aria-label={users.catalog.actions.edit}>
                <Icon icon="mdi:pencil-outline" width={16} height={16} />
              </Button>
              {!isSystem && (
                <Button type="button" variant="ghost" size="sm" className="user-tag-catalog-icon-button" aria-label={users.catalog.actions.delete}>
                  <Icon icon="mdi:trash-can-outline" width={16} height={16} />
                </Button>
              )}
            </>
          )}
        </div>
      </div>

      <div className="user-tag-catalog-card-meta">
        <Badge variant={isSystem ? 'info' : 'neutral'} className="user-tag-meta-badge">
          {isSystem ? users.catalog.scopeSystem : users.catalog.scopeCustom}
        </Badge>
        {isEditing ? (
          <StoryUserTagEffectToggle users={users} active={isBlockAll ? 'block_all' : 'quota_delta'} />
        ) : (
          <Badge variant={isBlockAll ? 'destructive' : 'success'} className="user-tag-meta-badge">
            {isBlockAll ? users.catalog.effectKinds.blockAll : users.catalog.effectKinds.quotaDelta}
          </Badge>
        )}
        <Button type="button" variant="secondary" size="xs" className="user-tag-catalog-users user-tag-catalog-users-button" disabled={isNewCard}>
          <span className="user-tag-catalog-users-label">{users.catalog.columns.users}</span>
          <strong>{formatNumber(draft.userCount)}</strong>
        </Button>
      </div>

      <div className="user-tag-catalog-body">
        {isBlockAll ? (
          <div className="alert alert-warning user-tag-catalog-block-note" role="note">
            {users.catalog.blockDescription}
          </div>
        ) : (
          <dl className="user-tag-catalog-delta-grid">
            {([
              [users.quota.hourly, draft.hourlyDelta],
              [users.quota.daily, draft.dailyDelta],
              [users.quota.monthly, draft.monthlyDelta],
            ] as const).map(([label, value]) => (
              <div className="user-tag-catalog-delta-item" key={label}>
                <dt>{label}</dt>
                <dd>
                  {isEditing ? (
                    <Input type="number" className="user-tag-delta-input" defaultValue={String(value)} />
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

function keyStatusTone(status: string): StatusTone {
  const normalized = status.trim().toLowerCase()
  if (normalized === 'active' || normalized === 'success' || normalized === 'completed') return 'success'
  if (normalized === 'exhausted' || normalized === 'quota_exhausted' || normalized === 'retry_exhausted') return 'warning'
  if (normalized === 'running' || normalized === 'queued' || normalized === 'pending') return 'info'
  if (normalized === 'error' || normalized === 'failed' || normalized === 'timeout' || normalized === 'cancelled') {
    return 'error'
  }
  return 'neutral'
}

function tokenQuotaTone(state: AuthToken['quota_state']): StatusTone {
  if (state === 'hour') return 'warning'
  if (state === 'day') return 'error'
  if (state === 'month') return 'info'
  return 'success'
}

function logResultTone(status: string): StatusTone {
  if (status === 'success') return 'success'
  if (status === 'quota_exhausted') return 'warning'
  if (status === 'error') return 'error'
  return 'neutral'
}

function requestErrorText(log: RequestLog, strings: AdminTranslations): string {
  const message = log.error_message?.trim()
  if (message) return message
  const status = log.result_status.toLowerCase()
  if (status === 'quota_exhausted') return strings.logs.errors.quotaExhausted
  if (status === 'error') return strings.logs.errors.requestFailedGeneric
  return strings.logs.errors.none
}

function requestKeyEffectText(log: RequestLog, strings: AdminTranslations): string {
  const summary = log.key_effect_summary?.trim()
  if (summary) return summary
  return strings.logDetails.noKeyEffect
}

function requestStatusPair(log: RequestLog): string {
  return `${log.http_status ?? '—'} / ${log.mcp_status ?? '—'}`
}

function requestStatusTip(log: RequestLog, strings: AdminTranslations): string {
  return `${strings.logs.table.httpStatus}: ${log.http_status ?? '—'} · ${strings.logs.table.mcpStatus}: ${log.mcp_status ?? '—'}`
}

function jsonStoryResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

function buildRequestStoryTokenDetail(id: string): {
  id: string
  enabled: boolean
  note: string | null
  owner: { userId: string; displayName: string; username: string } | null
  total_requests: number
  created_at: number
  last_used_at: number
  quota_state: string
  quota_hourly_used: number
  quota_hourly_limit: number
  quota_daily_used: number
  quota_daily_limit: number
  quota_monthly_used: number
  quota_monthly_limit: number
  quota_hourly_reset_at: number
  quota_daily_reset_at: number
  quota_monthly_reset_at: number
} {
  const token = MOCK_TOKENS.find((item) => item.id === id) ?? MOCK_TOKENS[0]
  return {
    id: token.id,
    enabled: token.enabled,
    note: token.note,
    owner: token.owner
      ? {
          userId: token.owner.userId,
          displayName: token.owner.displayName ?? token.owner.username ?? token.owner.userId,
          username: token.owner.username ?? token.owner.userId,
        }
      : null,
    total_requests: token.total_requests,
    created_at: token.created_at,
    last_used_at: token.last_used_at ?? token.created_at,
    quota_state: token.quota_state,
    quota_hourly_used: token.quota_hourly_used ?? 0,
    quota_hourly_limit: token.quota_hourly_limit ?? 0,
    quota_daily_used: token.quota_daily_used ?? 0,
    quota_daily_limit: token.quota_daily_limit ?? 0,
    quota_monthly_used: token.quota_monthly_used ?? 0,
    quota_monthly_limit: token.quota_monthly_limit ?? 0,
    quota_hourly_reset_at: token.quota_hourly_reset_at ?? token.created_at,
    quota_daily_reset_at: token.quota_daily_reset_at ?? token.created_at,
    quota_monthly_reset_at: token.quota_monthly_reset_at ?? token.created_at,
  }
}

function StoryKeyDetailsCanvas({ id, logs }: { id: string; logs: RequestLog[] }): JSX.Element {
  useLayoutEffect(() => {
    const originalFetch = window.fetch.bind(window)
    const key = [...MOCK_KEYS_WITH_QUARANTINE, ...MOCK_KEYS].find((item) => item.id === id) ?? MOCK_KEYS[0]
    const keyLogs = logs.filter((item) => item.key_id === id)
    const keyLogsCatalog = buildStoryRequestLogsCatalog(keyLogs, {
      showTokens: true,
      showKeys: false,
    })
    const keySummary = {
      total_requests: key.total_requests,
      success_count: key.success_count,
      error_count: key.error_count,
      quota_exhausted_count: key.quota_exhausted_count,
      active_keys: key.status === 'active' ? 1 : 0,
      exhausted_keys: key.status === 'exhausted' ? 1 : 0,
      last_activity: key.last_used_at,
    }

    window.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = typeof input === 'string' ? input : input instanceof URL ? input.toString() : input.url
      if (url.includes(`/api/keys/${encodeURIComponent(id)}/metrics`)) {
        return jsonStoryResponse(keySummary)
      }
      if (url.includes(`/api/keys/${encodeURIComponent(id)}/logs/list`)) {
        const requestUrl = new URL(url, window.location.origin)
        return jsonStoryResponse(
          buildStoryRequestLogsList(keyLogs, {
            cursor: requestUrl.searchParams.get('cursor'),
            limit: Number(requestUrl.searchParams.get('limit') ?? '20'),
            requestKinds: requestUrl.searchParams.getAll('request_kind'),
            result: requestUrl.searchParams.get('result') ?? undefined,
            keyEffect: requestUrl.searchParams.get('key_effect') ?? undefined,
            tokenId: requestUrl.searchParams.get('auth_token_id'),
          }),
        )
      }
      if (url.includes(`/api/keys/${encodeURIComponent(id)}/logs/catalog`)) {
        return jsonStoryResponse(keyLogsCatalog)
      }
      if (url.includes(`/api/keys/${encodeURIComponent(id)}/logs`)) {
        return jsonStoryResponse(keyLogs)
      }
      if (url.endsWith(`/api/keys/${encodeURIComponent(id)}`)) {
        return jsonStoryResponse(key)
      }
      if (url.includes(`/api/keys/${encodeURIComponent(id)}/sticky-users`)) {
        return jsonStoryResponse({ items: stickyUsersStoryData, total: stickyUsersStoryTotal })
      }
      if (url.includes(`/api/keys/${encodeURIComponent(id)}/sticky-nodes`)) {
        return jsonStoryResponse({
          rangeStart: '2026-03-10T00:00:00Z',
          rangeEnd: '2026-03-17T00:00:00Z',
          bucketSeconds: 86400,
          nodes: stickyNodesStoryData,
        })
      }
      return originalFetch(input, init)
    }) as typeof window.fetch

    return () => {
      window.fetch = originalFetch
    }
  }, [id, logs])

  return <KeyDetails id={id} onBack={() => undefined} onOpenUser={() => undefined} />
}

function requestKeyEffectTone(code: string | null | undefined): StatusTone {
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

function requestKeyEffectBadgeLabel(log: RequestLog, strings: AdminTranslations): string {
  switch ((log.key_effect_code ?? '').trim()) {
    case 'quarantined':
      return strings.logs.keyEffects.quarantined
    case 'marked_exhausted':
      return strings.logs.keyEffects.markedExhausted
    case 'restored_active':
      return strings.logs.keyEffects.restoredActive
    case 'cleared_quarantine':
      return strings.logs.keyEffects.clearedQuarantine
    case 'none':
    case '':
      return strings.logs.keyEffects.none
    default:
      return strings.logs.keyEffects.unknown
  }
}

function requestFailureGuidance(kind: string | null | undefined, language: 'en' | 'zh'): string | null {
  switch (kind) {
    case 'upstream_gateway_5xx':
      return language === 'zh'
        ? '上游网关暂时不可用，建议稍后重试，并检查上游健康状态与超时设置。'
        : 'The upstream gateway is temporarily unavailable. Retry later and verify upstream health and timeout settings.'
    case 'upstream_rate_limited_429':
      return language === 'zh'
        ? '这是上游限流，建议降低请求速率、切换其他 Key，或等待冷却后再试。'
        : 'This is upstream rate limiting. Reduce request rate, switch to another key, or retry after cooldown.'
    case 'upstream_account_deactivated_401':
      return language === 'zh'
        ? '该 Key 对应账户已停用，建议联系 Tavily 支持并停止继续分配该 Key。'
        : 'The account behind this key is deactivated. Contact Tavily support and stop assigning this key.'
    case 'transport_send_error':
      return language === 'zh'
        ? '这是链路发送失败，建议检查 DNS、TLS、代理和网络连通性。'
        : 'This is a transport send failure. Check DNS, TLS, proxy settings, and network connectivity.'
    case 'mcp_accept_406':
      return language === 'zh'
        ? '客户端需要同时接受 application/json 和 text/event-stream。'
        : 'The client must accept both application/json and text/event-stream.'
    default:
      return null
  }
}

function buildNavItems(strings: AdminTranslations): AdminNavItem[] {
  return [
    { target: 'dashboard', label: strings.nav.dashboard, icon: <Icon icon="mdi:view-dashboard-outline" width={18} height={18} /> },
    { target: 'user-usage', label: strings.nav.usage, icon: <ChartColumnIncreasing size={18} strokeWidth={2.2} /> },
    { target: 'tokens', label: strings.nav.tokens, icon: <Icon icon="mdi:key-chain-variant" width={18} height={18} /> },
    { target: 'keys', label: strings.nav.keys, icon: <Icon icon="mdi:key-outline" width={18} height={18} /> },
    { target: 'requests', label: strings.nav.requests, icon: <Icon icon="mdi:file-document-outline" width={18} height={18} /> },
    { target: 'jobs', label: strings.nav.jobs, icon: <Icon icon="mdi:calendar-clock-outline" width={18} height={18} /> },
    { target: 'users', label: strings.nav.users, icon: <Icon icon="mdi:account-group-outline" width={18} height={18} /> },
    { target: 'alerts', label: strings.nav.alerts, icon: <Icon icon="mdi:bell-ring-outline" width={18} height={18} /> },
    { target: 'system-settings', label: strings.nav.systemSettings, icon: <Icon icon="mdi:cog-outline" width={18} height={18} /> },
    { target: 'proxy-settings', label: strings.nav.proxySettings, icon: <Icon icon="mdi:tune-variant" width={18} height={18} /> },
  ]
}

interface AdminPageFrameProps {
  activeModule: AdminNavTarget
  children: ReactNode
  overlays?: ReactNode
  showDefaultShellChrome?: boolean
}

function AdminPageFrame({
  activeModule,
  children,
  overlays,
  showDefaultShellChrome = true,
}: AdminPageFrameProps): JSX.Element {
  const admin = useTranslate().admin
  const intro = (() => {
    switch (activeModule) {
      case 'dashboard':
        return {
          title: admin.header.title,
          description: admin.header.subtitle,
        }
      case 'tokens':
        return {
          title: admin.tokens.title,
          description: admin.tokens.description,
        }
      case 'keys':
        return {
          title: admin.keys.title,
          description: admin.keys.description,
        }
      case 'requests':
        return {
          title: admin.logs.title,
          description: admin.logs.description,
        }
      case 'jobs':
        return {
          title: admin.jobs.title,
          description: admin.jobs.description,
        }
      case 'users':
        return {
          title: admin.users.title,
          description: admin.users.description,
        }
      case 'alerts':
        return {
          title: admin.modules.alerts.title,
          description: admin.modules.alerts.description,
        }
      case 'system-settings':
        return {
          title: admin.systemSettings.title,
          description: admin.systemSettings.description,
        }
      case 'proxy-settings':
        return {
          title: admin.proxySettings.title,
          description: admin.proxySettings.description,
        }
      default:
        return {
          title: admin.header.title,
          description: admin.header.subtitle,
        }
    }
  })()

  return (
    <AdminOverlayHost overlays={overlays}>
      <AdminShell
        activeItem={activeModule}
        navItems={buildNavItems(admin)}
        skipToContentLabel={admin.accessibility.skipToContent}
        onSelectItem={() => {}}
      >
        {showDefaultShellChrome && (
          <>
            <AdminShellSidebarUtility>
              <AdminSidebarUtilityStack>
                <AdminSidebarUtilityCard>
                  <div className="admin-sidebar-utility-toolbar">
                    <ThemeToggle />
                    <LanguageSwitcher />
                  </div>
                  <div className="admin-sidebar-utility-meta">
                    <div className="user-badge user-badge-admin">
                      <Icon icon="mdi:crown-outline" className="user-badge-icon" aria-hidden="true" />
                      <span>Ops Admin</span>
                    </div>
                    <span className="admin-panel-header-time" aria-live="polite">
                      <Icon icon="mdi:clock-time-four-outline" width={14} height={14} className="admin-panel-header-time-icon" aria-hidden="true" />
                      <span className="admin-panel-header-time-label">{admin.header.updatedPrefix}</span>
                      <span className="admin-panel-header-time-value">11:42:10</span>
                    </span>
                  </div>
                </AdminSidebarUtilityCard>
                <AdminSidebarUtilityCard>
                  <div className="admin-sidebar-utility-actions">
                    <AdminReturnToConsoleLink
                      label={admin.header.returnToConsole}
                      href="/console"
                      className="admin-sidebar-utility-action"
                    />
                    <Button type="button" variant="outline" size="sm" className="admin-panel-refresh-button admin-sidebar-utility-action">
                      <Icon icon="mdi:refresh" width={16} height={16} aria-hidden="true" />
                      <span>{admin.header.refreshNow}</span>
                    </Button>
                  </div>
                </AdminSidebarUtilityCard>
              </AdminSidebarUtilityStack>
            </AdminShellSidebarUtility>

            <div className="admin-stacked-only">
              <AdminPanelHeader
                title={admin.header.title}
                subtitle={admin.header.subtitle}
                displayName="Ops Admin"
                isAdmin
                updatedPrefix={admin.header.updatedPrefix}
                updatedTime="11:42:10"
                isRefreshing={false}
                refreshLabel={admin.header.refreshNow}
                refreshingLabel={admin.header.refreshing}
                userConsoleLabel={admin.header.returnToConsole}
                userConsoleHref="/console"
                onRefresh={() => {}}
              />
            </div>
            <div className="admin-desktop-only">
              <AdminCompactIntro title={intro.title} description={intro.description} />
            </div>
          </>
        )}
        {children}
      </AdminShell>
    </AdminOverlayHost>
  )
}

function DashboardPageCanvas(): JSX.Element {
  const admin = useTranslate().admin

  const totalRequests = MOCK_KEYS.reduce((sum, item) => sum + item.total_requests, 0)
  const successCount = MOCK_KEYS.reduce((sum, item) => sum + item.success_count, 0)
  const errorCount = MOCK_KEYS.reduce((sum, item) => sum + item.error_count, 0)
  const quotaExhaustedCount = MOCK_KEYS.reduce((sum, item) => sum + item.quota_exhausted_count, 0)
  const totalQuotaLimit = MOCK_KEYS.reduce((sum, item) => sum + (item.quota_limit ?? 0), 0)
  const totalQuotaRemaining = MOCK_KEYS.reduce((sum, item) => sum + (item.quota_remaining ?? 0), 0)
  const exhaustedKeys = MOCK_KEYS.filter((item) => item.status === 'exhausted').length
  const activeKeys = MOCK_KEYS.filter((item) => item.status === 'active').length

  const todayMetrics: DashboardMetricCard[] = createDashboardTodayMetrics({
    today: {
      total_requests: totalRequests,
      success_count: 0,
      error_count: 0,
      quota_exhausted_count: 0,
      valuable_success_count: Math.max(0, successCount - 40),
      valuable_failure_count: Math.max(0, errorCount + quotaExhaustedCount),
      other_success_count: 32,
      other_failure_count: 8,
      unknown_count: 5,
      upstream_exhausted_key_count: Math.max(0, Math.ceil(quotaExhaustedCount / 4)),
      new_keys: 0,
      new_quarantines: 0,
    },
    yesterday: {
      total_requests: totalRequests - 128,
      success_count: 0,
      error_count: 0,
      quota_exhausted_count: 0,
      valuable_success_count: Math.max(0, successCount - 136),
      valuable_failure_count: Math.max(0, errorCount + quotaExhaustedCount + 6),
      other_success_count: 28,
      other_failure_count: 7,
      unknown_count: 2,
      upstream_exhausted_key_count: Math.max(0, Math.ceil(quotaExhaustedCount / 5)),
      new_keys: 0,
      new_quarantines: 0,
    },
    labels: {
      total: admin.metrics.labels.total,
      success: admin.metrics.labels.success,
      failure: admin.metrics.labels.failure,
      unknownCalls: admin.metrics.labels.unknownCalls,
      upstreamExhausted: admin.dashboard.upstreamExhaustedLabel,
      valuableTag: admin.dashboard.valuableTag,
      otherTag: admin.dashboard.otherTag,
      unknownTag: admin.dashboard.unknownTag,
    },
    strings: {
      deltaFromYesterday: admin.dashboard.deltaFromYesterday,
      deltaNoBaseline: admin.dashboard.deltaNoBaseline,
      percentagePointUnit: admin.dashboard.percentagePointUnit,
      asOfNow: admin.dashboard.asOfNow,
      todayShare: admin.dashboard.todayShare,
      todayAdded: admin.dashboard.todayAdded,
    },
    formatters: {
      formatNumber,
      formatPercent,
    },
  })

  const monthMetrics: DashboardMetricCard[] = createDashboardMonthMetrics({
    month: {
      total_requests: totalRequests * 14,
      success_count: 0,
      error_count: 0,
      quota_exhausted_count: 0,
      valuable_success_count: Math.max(0, successCount * 12),
      valuable_failure_count: Math.max(0, (errorCount + quotaExhaustedCount) * 10),
      other_success_count: 32 * 14,
      other_failure_count: 8 * 14,
      unknown_count: 5 * 14,
      upstream_exhausted_key_count: Math.max(0, Math.ceil(quotaExhaustedCount / 2)),
      new_keys: 3,
      new_quarantines: 1,
    },
    labels: {
      total: admin.metrics.labels.total,
      success: admin.metrics.labels.success,
      failure: admin.metrics.labels.failure,
      unknownCalls: admin.metrics.labels.unknownCalls,
      upstreamExhausted: admin.dashboard.upstreamExhaustedLabel,
      valuableTag: admin.dashboard.valuableTag,
      otherTag: admin.dashboard.otherTag,
      unknownTag: admin.dashboard.unknownTag,
      newKeys: admin.metrics.labels.newKeys,
      newQuarantines: admin.metrics.labels.newQuarantines,
    },
    strings: {
      monthToDate: admin.dashboard.monthToDate,
      monthShare: admin.dashboard.monthShare,
      monthAdded: admin.dashboard.monthAdded,
    },
    formatters: {
      formatNumber,
      formatPercent,
    },
  })

  const totalProxyNodes = forwardProxyStorySettings.nodes.length
  const availableProxyNodes = forwardProxyStorySettings.nodes.filter((node) => node.available).length

  const statusMetrics: DashboardMetricCard[] = [
    {
      id: 'remaining',
      label: admin.metrics.labels.remaining,
      value: `${formatNumber(totalQuotaRemaining)} / ${formatNumber(totalQuotaLimit)}`,
      subtitle: `${admin.dashboard.currentSnapshot} · ${formatPercent(totalQuotaRemaining, totalQuotaLimit)}`,
    },
    {
      id: 'keys',
      label: admin.metrics.labels.keys,
      value: formatNumber(activeKeys),
      subtitle: admin.dashboard.currentSnapshot,
    },
    {
      id: 'quarantined',
      label: admin.metrics.labels.quarantined,
      value: '0',
      subtitle: admin.metrics.subtitles.keysAll,
    },
    {
      id: 'exhausted',
      label: admin.metrics.labels.exhausted,
      value: formatNumber(exhaustedKeys),
      subtitle: admin.metrics.subtitles.keysExhausted.replace('{count}', formatNumber(exhaustedKeys)),
    },
    {
      id: 'proxy-available',
      label: admin.metrics.labels.proxyAvailable,
      value: formatNumber(availableProxyNodes),
      subtitle: `${admin.dashboard.currentSnapshot} · ${formatPercent(availableProxyNodes, totalProxyNodes)}`,
    },
    {
      id: 'proxy-total',
      label: admin.metrics.labels.proxyTotal,
      value: formatNumber(totalProxyNodes),
      subtitle: admin.dashboard.currentSnapshot,
    },
  ]

  return (
    <AdminPageFrame activeModule="dashboard">
      <DashboardOverview
        strings={admin.dashboard}
        overviewReady
        statusLoading={false}
        todayMetrics={todayMetrics}
        monthMetrics={monthMetrics}
        statusMetrics={statusMetrics}
        hourlyRequestWindow={defaultDashboardHourlyRequestWindow}
        tokenCoverage="truncated"
        tokens={MOCK_TOKENS}
        keys={MOCK_KEYS}
        logs={MOCK_REQUESTS}
        jobs={MOCK_JOBS}
        recentAlerts={STORY_RECENT_ALERTS}
        onOpenModule={() => {}}
        onOpenToken={() => {}}
        onOpenKey={() => {}}
      />
    </AdminPageFrame>
  )
}

function TokensPageCanvas(): JSX.Element {
  const admin = useTranslate().admin
  const tokenStrings = admin.tokens

  return (
    <AdminPageFrame activeModule="tokens">
      <section className="surface panel">
        <div className="panel-header" style={{ flexWrap: 'wrap', gap: 12, alignItems: 'flex-start' }}>
          <div style={{ flex: '1 1 320px', minWidth: 240 }}>
            <h2 style={{ margin: 0 }}>{tokenStrings.title}</h2>
            <p className="panel-description">{tokenStrings.description}</p>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap', marginLeft: 'auto' }}>
            <input
              type="text"
              className="input input-bordered"
              readOnly
              value="marketing-ab-test"
              aria-label={tokenStrings.notePlaceholder}
            />
            <button type="button" className="btn btn-primary">
              {tokenStrings.newToken}
            </button>
            <button type="button" className="btn btn-outline">
              {tokenStrings.batchCreate}
            </button>
          </div>
        </div>

        <div className="token-groups-container">
          <div className="token-groups-label">
            <span>{tokenStrings.groups.label}</span>
          </div>
          <div className="token-groups-row">
            <div className="token-groups-list token-groups-list-expanded">
              <button type="button" className="token-group-chip token-group-chip-active">
                <span className="token-group-name">{tokenStrings.groups.all}</span>
              </button>
              <button type="button" className="token-group-chip">
                <span className="token-group-name">production</span>
                <span className="token-group-count">2</span>
              </button>
              <button type="button" className="token-group-chip">
                <span className="token-group-name">ops</span>
                <span className="token-group-count">2</span>
              </button>
              <button type="button" className="token-group-chip">
                <span className="token-group-name">batch</span>
                <span className="token-group-count">1</span>
              </button>
            </div>
          </div>
        </div>

        <div className="table-wrapper jobs-table-wrapper admin-users-usage-table-wrapper">
          <table className="jobs-table tokens-table">
            <thead>
              <tr>
                <th>{tokenStrings.table.id}</th>
                <th>{tokenStrings.table.owner}</th>
                <th>{tokenStrings.table.note}</th>
                <th>{tokenStrings.table.usage}</th>
                <th>{tokenStrings.table.quota}</th>
                <th>{tokenStrings.table.lastUsed}</th>
                <th>{tokenStrings.table.actions}</th>
              </tr>
            </thead>
            <tbody>
              {MOCK_TOKENS.map((token) => (
                <tr key={token.id}>
                  <td>
                    <div className="token-id-cell">
                      <code className="token-id-code">{token.id}</code>
                      <span
                        className="token-status-slot"
                        aria-hidden={token.enabled ? true : undefined}
                        title={token.enabled ? undefined : tokenStrings.statusBadges.disabled}
                      >
                        {!token.enabled && (
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
                    <div className="token-owner-block">
                      {token.owner ? (
                        <button
                          type="button"
                          className="link-button token-owner-trigger"
                          onClick={() => openAdminStory('admin-pages--user-detail')}
                        >
                          <span className="token-owner-link">{token.owner.displayName || token.owner.userId}</span>
                          {token.owner.username ? <span className="token-owner-secondary">@{token.owner.username}</span> : null}
                        </button>
                      ) : (
                        <span className="token-owner-empty">{tokenStrings.owner.unbound}</span>
                      )}
                    </div>
                  </td>
                  <td>{token.note ?? '—'}</td>
                  <td>{formatNumber(token.total_requests)}</td>
                  <td>
                    <StatusBadge tone={tokenQuotaTone(token.quota_state)}>{tokenStrings.quotaStates[token.quota_state]}</StatusBadge>
                  </td>
                  <td>{formatTimestamp(token.last_used_at)}</td>
                  <td className="jobs-message-cell">
                    <div className="table-actions">
                      <button type="button" className="btn btn-circle btn-ghost btn-sm" aria-label={tokenStrings.actions.copy}>
                        C
                      </button>
                      <button type="button" className="btn btn-circle btn-ghost btn-sm" aria-label={tokenStrings.actions.share}>
                        S
                      </button>
                      <button type="button" className="btn btn-circle btn-ghost btn-sm" aria-label={tokenStrings.actions.delete}>
                        D
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        <div className="table-pagination">
          <span className="panel-description">{tokenStrings.pagination.page.replace('{page}', '1').replace('{total}', '3')}</span>
          <div style={{ display: 'inline-flex', gap: 8 }}>
            <button type="button" className="btn btn-outline">
              {tokenStrings.pagination.prev}
            </button>
            <button type="button" className="btn btn-outline">
              {tokenStrings.pagination.next}
            </button>
          </div>
        </div>
      </section>
    </AdminPageFrame>
  )
}

function KeysPageCanvas({
  initialRegistrationIp = '',
  initialRegions = [],
  initialSelectedIds = [],
  bulkActionInFlight = null,
  bulkSyncProgress = null,
  bulkFeedback = null,
}: {
  initialRegistrationIp?: string
  initialRegions?: string[]
  initialSelectedIds?: string[]
  bulkActionInFlight?: ApiKeyBulkAction | null
  bulkSyncProgress?: ApiKeyBulkSyncProgressState | null
  bulkFeedback?: { kind: 'success' | 'error'; message: string } | null
} = {}): JSX.Element {
  const admin = useTranslate().admin
  const keyStrings = admin.keys
  const [selectedGroups, setSelectedGroups] = useState<string[]>([])
  const [selectedStatuses, setSelectedStatuses] = useState<string[]>([])
  const [selectedRegistrationIp, setSelectedRegistrationIp] = useState(initialRegistrationIp)
  const [selectedRegions, setSelectedRegions] = useState<string[]>(initialRegions)
  const [selectedKeyIds, setSelectedKeyIds] = useState<string[]>(initialSelectedIds)
  const [page, setPage] = useState(1)
  const [perPage, setPerPage] = useState(20)
  const keys = MOCK_KEYS
  const groupOptions = Array.from(
    keys.reduce((map, item) => {
      const key = (item.group ?? '').trim()
      map.set(key, {
        value: key,
        label: key.length > 0 ? key : keyStrings.groups.ungrouped,
        count: (map.get(key)?.count ?? 0) + 1,
      })
      return map
    }, new Map<string, { value: string; label: string; count: number }>()),
  ).map(([, value]) => value)
  const statusOptions = Array.from(
    keys.reduce((map, item) => {
      const value = item.quarantine ? 'quarantined' : item.status
      map.set(value, {
        value,
        label: admin.statuses[value] ?? value,
        count: (map.get(value)?.count ?? 0) + 1,
      })
      return map
    }, new Map<string, { value: string; label: string; count: number }>()),
  )
    .map(([, value]) => value)
    .sort((left, right) => left.label.localeCompare(right.label))
  const regionOptions = Array.from(
    keys.reduce((map, item) => {
      const value = item.registration_region?.trim() ?? ''
      if (!value) return map
      map.set(value, {
        value,
        label: value,
        count: (map.get(value)?.count ?? 0) + 1,
      })
      return map
    }, new Map<string, { value: string; label: string; count: number }>()),
  )
    .map(([, value]) => value)
    .sort((left, right) => left.label.localeCompare(right.label))
  const filteredKeys = keys.filter((item) => {
    const groupKey = (item.group ?? '').trim()
    const statusKey = item.quarantine ? 'quarantined' : item.status
    const registrationIp = item.registration_ip?.trim() ?? ''
    const regionKey = item.registration_region?.trim() ?? ''
    const groupMatched = selectedGroups.length === 0 || selectedGroups.includes(groupKey)
    const statusMatched = selectedStatuses.length === 0 || selectedStatuses.includes(statusKey)
    const registrationIpMatched =
      selectedRegistrationIp.trim().length === 0 || registrationIp === selectedRegistrationIp.trim()
    const regionMatched = selectedRegions.length === 0 || selectedRegions.includes(regionKey)
    return groupMatched && statusMatched && registrationIpMatched && regionMatched
  })
  const totalPages = Math.max(1, Math.ceil(filteredKeys.length / perPage))
  const safePage = Math.min(page, totalPages)
  const pagedKeys = filteredKeys.slice((safePage - 1) * perPage, safePage * perPage)
  const selectedVisibleKeys = pagedKeys.filter((item) => selectedKeyIds.includes(item.id))
  const selectedVisibleKeyCount = selectedVisibleKeys.length
  const allVisibleKeysSelected = pagedKeys.length > 0 && selectedVisibleKeyCount === pagedKeys.length
  const groupSummary = summarizeFilterSelection(
    keyStrings.groups.label,
    groupOptions.filter((option) => selectedGroups.includes(option.value)).map((option) => option.label),
    keyStrings.groups.all,
    keyStrings.filters.selectedSuffix,
  )
  const statusSummary = summarizeFilterSelection(
    keyStrings.filters.status,
    statusOptions.filter((option) => selectedStatuses.includes(option.value)).map((option) => option.label),
    keyStrings.groups.all,
    keyStrings.filters.selectedSuffix,
  )
  const regionSummary = summarizeFilterSelection(
    keyStrings.filters.region,
    regionOptions.filter((option) => selectedRegions.includes(option.value)).map((option) => option.label),
    keyStrings.groups.all,
    keyStrings.filters.selectedSuffix,
  )

  useEffect(() => {
    if (page !== safePage) {
      setPage(safePage)
    }
  }, [page, safePage])

  return (
    <AdminPageFrame activeModule="keys">
      <section className="surface panel">
        <div className="panel-header" style={{ flexWrap: 'wrap', gap: 12, alignItems: 'flex-start' }}>
          <div style={{ flex: '1 1 320px', minWidth: 240 }}>
            <h2>{keyStrings.title}</h2>
            <p className="panel-description">{keyStrings.description}</p>
          </div>
          <div style={{ ...keysQuickAddCardStyle, marginLeft: 'auto' }}>
            <div style={keysQuickAddActionsStyle}>
              <input
                type="text"
                className="input input-bordered"
                readOnly
                value="tvly-prod-******"
                aria-label={keyStrings.placeholder}
                style={{ flex: '1 1 260px', minWidth: 260, maxWidth: '100%' }}
              />
              <button type="button" className="btn btn-primary btn-sm" style={{ whiteSpace: 'nowrap' }}>
                {keyStrings.addButton}
              </button>
            </div>
          </div>
        </div>

        <div style={keysUtilityRowStyle}>
          <div style={keysFilterClusterStyle}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <Input
                type="text"
                value={selectedRegistrationIp}
                onChange={(event) => setSelectedRegistrationIp(event.target.value)}
                placeholder={keyStrings.filters.registrationIpPlaceholder}
                aria-label={keyStrings.filters.registrationIp}
                style={{ width: 188 }}
              />
              {selectedRegistrationIp ? (
                <Button type="button" variant="ghost" size="sm" onClick={() => setSelectedRegistrationIp('')}>
                  {keyStrings.filters.clearRegistrationIp}
                </Button>
              ) : null}
            </div>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button type="button" variant="outline" size="sm" aria-label={groupSummary}>
                  <Icon icon="mdi:filter-variant" width={16} height={16} aria-hidden="true" />
                  <span style={{ whiteSpace: 'nowrap' }}>{groupSummary}</span>
                  {selectedGroups.length > 0 ? (
                    <Badge variant="neutral" className="ml-1 px-1.5 py-0 text-[10px]">
                      {selectedGroups.length}
                    </Badge>
                  ) : null}
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="w-64">
                <DropdownMenuLabel>{keyStrings.groups.label}</DropdownMenuLabel>
                <DropdownMenuItem
                  className="cursor-pointer"
                  disabled={selectedGroups.length === 0}
                  onSelect={(event) => {
                    event.preventDefault()
                    setSelectedGroups([])
                  }}
                >
                  {keyStrings.filters.clearGroups}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                {groupOptions.map((option) => (
                  <DropdownMenuCheckboxItem
                    key={option.value || '__ungrouped__'}
                    className="cursor-pointer"
                    checked={selectedGroups.includes(option.value)}
                    onSelect={(event) => event.preventDefault()}
                    onCheckedChange={() => setSelectedGroups((current) => toggleSelection(current, option.value))}
                  >
                    <span>{option.label}</span>
                    <span className="ml-auto text-xs opacity-60">{formatNumber(option.count)}</span>
                  </DropdownMenuCheckboxItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button type="button" variant="outline" size="sm" aria-label={statusSummary}>
                  <Icon icon="mdi:filter-outline" width={16} height={16} aria-hidden="true" />
                  <span style={{ whiteSpace: 'nowrap' }}>{statusSummary}</span>
                  {selectedStatuses.length > 0 ? (
                    <Badge variant="neutral" className="ml-1 px-1.5 py-0 text-[10px]">
                      {selectedStatuses.length}
                    </Badge>
                  ) : null}
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="w-64">
                <DropdownMenuLabel>{keyStrings.filters.status}</DropdownMenuLabel>
                <DropdownMenuItem
                  className="cursor-pointer"
                  disabled={selectedStatuses.length === 0}
                  onSelect={(event) => {
                    event.preventDefault()
                    setSelectedStatuses([])
                  }}
                >
                  {keyStrings.filters.clearStatuses}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                {statusOptions.map((option) => (
                  <DropdownMenuCheckboxItem
                    key={option.value}
                    className="cursor-pointer"
                    checked={selectedStatuses.includes(option.value)}
                    onSelect={(event) => event.preventDefault()}
                    onCheckedChange={() => setSelectedStatuses((current) => toggleSelection(current, option.value))}
                  >
                    <span>{option.label}</span>
                    <span className="ml-auto text-xs opacity-60">{formatNumber(option.count)}</span>
                  </DropdownMenuCheckboxItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button type="button" variant="outline" size="sm" aria-label={regionSummary}>
                  <Icon icon="mdi:map-marker-radius-outline" width={16} height={16} aria-hidden="true" />
                  <span style={{ whiteSpace: 'nowrap' }}>{regionSummary}</span>
                  {selectedRegions.length > 0 ? (
                    <Badge variant="neutral" className="ml-1 px-1.5 py-0 text-[10px]">
                      {selectedRegions.length}
                    </Badge>
                  ) : null}
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="w-72">
                <DropdownMenuLabel>{keyStrings.filters.region}</DropdownMenuLabel>
                <DropdownMenuItem
                  className="cursor-pointer"
                  disabled={selectedRegions.length === 0}
                  onSelect={(event) => {
                    event.preventDefault()
                    setSelectedRegions([])
                  }}
                >
                  {keyStrings.filters.clearRegions}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                {regionOptions.map((option) => (
                  <DropdownMenuCheckboxItem
                    key={option.value}
                    className="cursor-pointer"
                    checked={selectedRegions.includes(option.value)}
                    onSelect={(event) => event.preventDefault()}
                    onCheckedChange={() => setSelectedRegions((current) => toggleSelection(current, option.value))}
                  >
                    <span>{option.label}</span>
                    <span className="ml-auto text-xs opacity-60">{formatNumber(option.count)}</span>
                  </DropdownMenuCheckboxItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </div>
        <div style={keysBulkToolbarStyle}>
          <div style={keysBulkSelectionStyle}>
            <span className="panel-description">
              {keyStrings.selection.selectedCount.replace('{count}', String(selectedVisibleKeyCount))}
            </span>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => setSelectedKeyIds(pagedKeys.map((item) => item.id))}
              disabled={allVisibleKeysSelected}
            >
              {keyStrings.selection.selectCurrentPage}
            </Button>
            <Button type="button" variant="ghost" size="sm" onClick={() => setSelectedKeyIds([])} disabled={selectedVisibleKeyCount === 0}>
              {keyStrings.selection.clear}
            </Button>
          </div>
          <div style={keysBulkActionsStyle}>
            <Button type="button" variant="outline" size="sm" disabled={bulkActionInFlight != null}>
              <Icon
                icon={bulkActionInFlight === 'sync_usage' ? 'mdi:loading' : 'mdi:refresh'}
                width={16}
                height={16}
                aria-hidden="true"
                className={bulkActionInFlight === 'sync_usage' ? 'animate-spin' : undefined}
              />
              {bulkActionInFlight === 'sync_usage'
                ? keyStrings.bulkActions.running
                : keyStrings.bulkActions.syncUsage}
            </Button>
            <Button type="button" variant="outline" size="sm" disabled={bulkActionInFlight != null}>
              <Icon
                icon={bulkActionInFlight === 'clear_quarantine' ? 'mdi:loading' : 'mdi:shield-check-outline'}
                width={16}
                height={16}
                aria-hidden="true"
                className={bulkActionInFlight === 'clear_quarantine' ? 'animate-spin' : undefined}
              />
              {bulkActionInFlight === 'clear_quarantine'
                ? keyStrings.bulkActions.running
                : keyStrings.bulkActions.clearQuarantine}
            </Button>
            <Button type="button" variant="warning" size="sm" disabled={bulkActionInFlight != null}>
              <Icon
                icon={bulkActionInFlight === 'delete' ? 'mdi:loading' : 'mdi:trash-can-outline'}
                width={16}
                height={16}
                aria-hidden="true"
                className={bulkActionInFlight === 'delete' ? 'animate-spin' : undefined}
              />
              {bulkActionInFlight === 'delete'
                ? keyStrings.bulkActions.running
                : keyStrings.bulkActions.delete}
            </Button>
          </div>
        </div>
        {bulkSyncProgress ? (
          <div style={{ display: 'flex', justifyContent: 'flex-end', marginTop: -8, marginBottom: 16 }}>
            <ApiKeyBulkSyncProgressBubble
              strings={keyStrings.bulkSyncProgress}
              progress={bulkSyncProgress}
              style={{ width: 'min(26rem, 100%)' }}
            />
          </div>
        ) : null}
        {bulkFeedback ? (
          <div
            className={bulkFeedback.kind === 'error' ? 'alert alert-error' : 'alert alert-warning'}
            role={bulkFeedback.kind === 'error' ? 'alert' : 'status'}
            style={{ marginBottom: 16 }}
          >
            {bulkFeedback.message}
          </div>
        ) : null}

        <div className="table-wrapper jobs-table-wrapper">
          <table className="jobs-table api-keys-table api-keys-table--admin">
            <thead>
              <tr>
                <th style={{ width: 52 }}>
                  <label style={keySelectionCheckboxLabelStyle}>
                    <input
                      type="checkbox"
                      checked={allVisibleKeysSelected}
                      aria-label={keyStrings.selection.selectAll}
                      onChange={(event) =>
                        setSelectedKeyIds(event.currentTarget.checked ? pagedKeys.map((item) => item.id) : [])
                      }
                    />
                  </label>
                </th>
                <th>
                  <div style={tableHeaderStackStyle}>
                    <span style={tableFieldStyle}>{keyStrings.table.keyId}</span>
                    <span style={tableSecondaryFieldStyle}>{keyStrings.groups.label}</span>
                  </div>
                </th>
                <th>
                  <div style={tableHeaderStackStyle}>
                    <span style={tableFieldStyle}>{keyStrings.table.registration}</span>
                    <span style={tableSecondaryFieldStyle}>{keyStrings.table.registrationRegion}</span>
                  </div>
                </th>
                <th>
                  <div style={tableHeaderStackStyle}>
                    <span style={tableFieldStyle}>{keyStrings.table.status}</span>
                    <span style={tableSecondaryFieldStyle} aria-hidden="true">&nbsp;</span>
                  </div>
                </th>
                <th>
                  <div style={tableHeaderStackStyle}>
                    <span style={tableFieldStyle}>{keyStrings.table.success}</span>
                    <span style={tableSecondaryFieldStyle}>{keyStrings.table.errors}</span>
                  </div>
                </th>
                <th>
                  <div style={tableHeaderStackStyle}>
                    <span style={tableFieldStyle}>{keyStrings.table.quotaLeft}</span>
                    <span style={tableSecondaryFieldStyle} aria-hidden="true">&nbsp;</span>
                  </div>
                </th>
                <th>
                  <div style={tableHeaderStackStyle}>
                    <span style={tableFieldStyle}>{keyStrings.table.lastUsed}</span>
                    <span style={tableSecondaryFieldStyle}>{keyStrings.table.statusChanged}</span>
                  </div>
                </th>
                <th>
                  <div style={tableHeaderStackStyle}>
                    <span style={tableFieldStyle}>{keyStrings.table.actions}</span>
                    <span style={tableSecondaryFieldStyle} aria-hidden="true">&nbsp;</span>
                  </div>
                </th>
              </tr>
            </thead>
            <tbody>
              {pagedKeys.map((item) => (
                <tr key={item.id}>
                  <td>
                    <label style={keySelectionCheckboxLabelStyle}>
                      <input
                        type="checkbox"
                        checked={selectedKeyIds.includes(item.id)}
                        aria-label={`${keyStrings.selection.selectRow}: ${item.id}`}
                        onChange={() =>
                          setSelectedKeyIds((current) =>
                            current.includes(item.id)
                              ? current.filter((value) => value !== item.id)
                              : [...current, item.id],
                          )
                        }
                      />
                    </label>
                  </td>
                  <td>
                    <div style={tableStackStyle}>
                      <div style={tableInlineFieldStyle}>
                        <code>{item.id}</code>
                        <button
                          type="button"
                          className="btn btn-ghost btn-xs btn-circle"
                          aria-label={keyStrings.actions.copy}
                          title={keyStrings.actions.copy}
                          style={{
                            position: 'absolute',
                            right: 0,
                            top: '50%',
                            transform: 'translateY(-50%)',
                            width: 32,
                            height: 32,
                            minHeight: 32,
                            padding: 0,
                          }}
                        >
                          <Icon icon="mdi:content-copy" width={18} height={18} aria-hidden="true" />
                        </button>
                      </div>
                      <span className="api-keys-cell-text-secondary" style={tableEllipsisSecondaryFieldStyle}>{formatKeyGroupName(item.group, keyStrings.groups.ungrouped)}</span>
                    </div>
                  </td>
                  <td>
                    <div style={tableStackStyle}>
                      <span className="api-keys-cell-text" style={tableEllipsisFieldStyle}>{formatRegistrationValue(item.registration_ip)}</span>
                      <span className="api-keys-cell-text-secondary" style={tableEllipsisSecondaryFieldStyle}>
                        {formatRegistrationValue(item.registration_region)}
                      </span>
                    </div>
                  </td>
                  <td>
                    <div style={tableStackStyle}>
                      <span style={tableFieldStyle}>
                        <StatusBadge tone={keyStatusTone(item.quarantine ? 'quarantined' : item.status)}>
                          {admin.statuses[item.quarantine ? 'quarantined' : item.status] ?? item.status}
                        </StatusBadge>
                      </span>
                    </div>
                  </td>
                  <td>
                    <div style={tableStackStyle}>
                      <span className="api-keys-cell-text" style={tableEllipsisFieldStyle}>{formatNumber(item.success_count)}</span>
                      <span className="api-keys-cell-text-secondary" style={tableEllipsisSecondaryFieldStyle}>{formatNumber(item.error_count)}</span>
                    </div>
                  </td>
                  <td>
                    <span className="api-keys-cell-text" style={tableEllipsisFieldStyle}>
                      {item.quota_remaining != null && item.quota_limit != null
                        ? `${formatNumber(item.quota_remaining)} / ${formatNumber(item.quota_limit)}`
                        : '—'}
                    </span>
                  </td>
                  <td>
                    <div style={tableStackStyle}>
                      <span className="api-keys-cell-text" style={tableEllipsisFieldStyle}>{formatTimestamp(item.last_used_at)}</span>
                      <span className="api-keys-cell-text-secondary" style={tableEllipsisSecondaryFieldStyle}>{formatTimestamp(item.status_changed_at)}</span>
                    </div>
                  </td>
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
                        >
                          <Icon icon="mdi:shield-check-outline" width={18} height={18} />
                        </Button>
                      ) : item.status === 'disabled' ? (
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          className="api-keys-action-button rounded-full shadow-none"
                          title={keyStrings.actions.enable}
                          aria-label={keyStrings.actions.enable}
                        >
                          <Icon icon="mdi:play-circle-outline" width={18} height={18} />
                        </Button>
                      ) : (
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          className="api-keys-action-button rounded-full shadow-none"
                          title={keyStrings.actions.disable}
                          aria-label={keyStrings.actions.disable}
                        >
                          <Icon icon="mdi:pause-circle-outline" width={18} height={18} />
                        </Button>
                      )}
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        className="api-keys-action-button rounded-full shadow-none"
                        title={keyStrings.actions.delete}
                        aria-label={keyStrings.actions.delete}
                      >
                        <Icon icon="mdi:trash-outline" width={18} height={18} color="#ef4444" />
                      </Button>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        className="api-keys-action-button rounded-full shadow-none"
                        title={keyStrings.actions.details}
                        aria-label={keyStrings.actions.details}
                      >
                        <Icon icon="mdi:eye-outline" width={18} height={18} />
                      </Button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        {filteredKeys.length > perPage ? (
          <AdminTablePagination
            page={safePage}
            totalPages={totalPages}
            pageSummary={
              <span className="panel-description">
                {keyStrings.pagination.page.replace('{page}', String(safePage)).replace('{total}', String(totalPages))}
              </span>
            }
            perPage={perPage}
            perPageLabel={keyStrings.pagination.perPage}
            perPageAriaLabel={keyStrings.pagination.perPage}
            previousLabel={admin.tokens.pagination.prev}
            nextLabel={admin.tokens.pagination.next}
            previousDisabled={safePage <= 1}
            nextDisabled={safePage >= totalPages}
            onPrevious={() => setPage((current) => Math.max(1, current - 1))}
            onNext={() => setPage((current) => Math.min(totalPages, current + 1))}
            onPerPageChange={(value) => {
              setPerPage(value)
              setPage(1)
            }}
          />
        ) : null}
      </section>
    </AdminPageFrame>
  )
}

function RequestsPageCanvas({
  initialDrawerTarget = null,
}: {
  initialDrawerTarget?: { kind: 'key' | 'token'; id: string } | null
} = {}): JSX.Element {
  const admin = useTranslate().admin
  const { language } = useLanguage()
  const logStrings = admin.logs
  const [currentPage, setCurrentPage] = useState(1)
  const [perPage, setPerPage] = useState(3)
  const [selectedRequestKinds, setSelectedRequestKinds] = useState<string[]>([])
  const [requestKindQuickBilling, setRequestKindQuickBilling] =
    useState<TokenLogRequestKindQuickBilling>('all')
  const [requestKindQuickProtocol, setRequestKindQuickProtocol] =
    useState<TokenLogRequestKindQuickProtocol>('all')
  const [outcomeFilter, setOutcomeFilter] = useState<RecentRequestsOutcomeFilter | null>(null)
  const [selectedKeyId, setSelectedKeyId] = useState<string | null>(null)
  const [drawerTarget, setDrawerTarget] = useState<{ kind: 'key' | 'token'; id: string } | null>(initialDrawerTarget)
  const requestKindQuickFilters = useMemo(
    () => ({
      billing: requestKindQuickBilling,
      protocol: requestKindQuickProtocol,
    }),
    [requestKindQuickBilling, requestKindQuickProtocol],
  )
  const requestKindQuickSelection = useMemo(
    () => buildRequestKindQuickFilterSelection(STORY_REQUEST_KIND_OPTIONS, requestKindQuickFilters),
    [requestKindQuickFilters],
  )
  const effectiveSelectedRequestKinds = useMemo(
    () =>
      resolveEffectiveRequestKindSelection(
        selectedRequestKinds,
        requestKindQuickFilters,
        requestKindQuickSelection,
      ),
    [requestKindQuickFilters, requestKindQuickSelection, selectedRequestKinds],
  )
  const hasEmptyRequestKindMatch = useMemo(
    () =>
      hasActiveRequestKindQuickFilters(requestKindQuickFilters) &&
      requestKindQuickSelection.length === 0,
    [requestKindQuickFilters, requestKindQuickSelection.length],
  )
  const catalog = useMemo(
    () =>
      buildStoryRequestLogsCatalog(MOCK_REQUESTS, {
        showTokens: true,
        showKeys: true,
      }),
    [],
  )
  const listData = useMemo(
    () =>
      buildStoryRequestLogsList(MOCK_REQUESTS, {
        cursor: currentPage > 1 ? storyCursorForPage(currentPage) : null,
        limit: perPage,
        requestKinds: effectiveSelectedRequestKinds,
        result: outcomeFilter?.kind === 'result' ? outcomeFilter.value : undefined,
        keyEffect: outcomeFilter?.kind === 'keyEffect' ? outcomeFilter.value : undefined,
        bindingEffect: outcomeFilter?.kind === 'bindingEffect' ? outcomeFilter.value : undefined,
        selectionEffect: outcomeFilter?.kind === 'selectionEffect' ? outcomeFilter.value : undefined,
        keyId: selectedKeyId,
        forceEmptyMatch: hasEmptyRequestKindMatch,
      }),
    [
      currentPage,
      effectiveSelectedRequestKinds,
      hasEmptyRequestKindMatch,
      outcomeFilter,
      perPage,
      selectedKeyId,
    ],
  )

  const handleRequestKindQuickFiltersChange = (
    billing: TokenLogRequestKindQuickBilling,
    protocol: TokenLogRequestKindQuickProtocol,
  ) => {
    const nextFilters = { billing, protocol }
    setRequestKindQuickBilling(billing)
    setRequestKindQuickProtocol(protocol)
    setSelectedRequestKinds(buildRequestKindQuickFilterSelection(STORY_REQUEST_KIND_OPTIONS, nextFilters))
    setCurrentPage(1)
  }

  const handleToggleRequestKind = (key: string) => {
    const nextSelected = toggleRequestKindSelection(effectiveSelectedRequestKinds, key)
    const nextQuickFilters = resolveManualRequestKindQuickFilters(
      nextSelected,
      requestKindQuickFilters,
      requestKindQuickSelection,
      STORY_REQUEST_KIND_OPTIONS,
    )
    setSelectedRequestKinds(nextSelected)
    setRequestKindQuickBilling(nextQuickFilters.billing)
    setRequestKindQuickProtocol(nextQuickFilters.protocol)
    setCurrentPage(1)
  }

  const handleClearRequestKinds = () => {
    setSelectedRequestKinds([])
    setRequestKindQuickBilling(defaultTokenLogRequestKindQuickFilters.billing)
    setRequestKindQuickProtocol(defaultTokenLogRequestKindQuickFilters.protocol)
    setCurrentPage(1)
  }

  return (
    <AdminPageFrame activeModule="requests">
      <AdminRecentRequestsPanel
        variant="admin"
        language={language}
        strings={admin}
        title={logStrings.title}
        description={logStrings.descriptionWithRetention.replace('{days}', '32')}
        emptyLabel={logStrings.empty.none}
        loadState="ready"
        loadingLabel={logStrings.empty.loading}
        logs={listData.items}
        requestKindOptions={catalog.requestKindOptions}
        requestKindQuickBilling={requestKindQuickBilling}
        requestKindQuickProtocol={requestKindQuickProtocol}
        selectedRequestKinds={selectedRequestKinds}
        onRequestKindQuickFiltersChange={handleRequestKindQuickFiltersChange}
        onToggleRequestKind={handleToggleRequestKind}
        onClearRequestKinds={handleClearRequestKinds}
        outcomeFilter={outcomeFilter}
        resultOptions={catalog.facets.results}
        keyEffectOptions={catalog.facets.keyEffects}
        bindingEffectOptions={catalog.facets.bindingEffects}
        selectionEffectOptions={catalog.facets.selectionEffects}
        onOutcomeFilterChange={(value) => {
          setOutcomeFilter(value)
          setCurrentPage(1)
        }}
        keyOptions={catalog.facets.keys}
        selectedKeyId={selectedKeyId}
        onKeyFilterChange={(value) => {
          setSelectedKeyId(value)
          setCurrentPage(1)
        }}
        showKeyColumn
        showTokenColumn
        perPage={listData.pageSize}
        hasOlder={listData.hasOlder}
        hasNewer={listData.hasNewer}
        paginationSummary={logStrings.pagination.summaryWithRetention.replace('{days}', '32')}
        onNewerPage={() => setCurrentPage((value) => Math.max(1, value - 1))}
        onOlderPage={() => setCurrentPage((value) => value + 1)}
        onPerPageChange={(value) => {
          setPerPage(value)
          setCurrentPage(1)
        }}
        formatTime={formatTimestamp}
        formatTimeDetail={formatTimestamp}
        loadLogBodies={(log) => Promise.resolve(lookupStoryLogBodies(log.id))}
        onOpenKey={(id) => setDrawerTarget({ kind: 'key', id })}
        onOpenToken={(id) => setDrawerTarget({ kind: 'token', id })}
      />

      <Drawer
        open={drawerTarget != null}
        onOpenChange={(open) => {
          if (!open) setDrawerTarget(null)
        }}
        shouldScaleBackground={false}
      >
        <DrawerContent className="request-entity-drawer-content">
          <div className="request-entity-drawer-body">
            {drawerTarget?.kind === 'key' ? (
              <StoryKeyDetailsCanvas id={drawerTarget.id} logs={MOCK_REQUESTS} />
            ) : drawerTarget?.kind === 'token' ? (
              <TokenDetailStoryCanvas detail={buildRequestStoryTokenDetail(drawerTarget.id)} />
            ) : null}
          </div>
        </DrawerContent>
      </Drawer>
    </AdminPageFrame>
  )
}

function JobsPageCanvas(): JSX.Element {
  const admin = useTranslate().admin
  const jobsStrings = admin.jobs
  const keyStrings = admin.keys
  const [jobFilter, setJobFilter] = useState<JobGroup>('all')
  const [expandedJobs, setExpandedJobs] = useState<Set<number>>(() => new Set([608]))
  const jobGroupCounts = useMemo(() => countAdminJobGroups(MOCK_JOBS), [])
  const jobFilterOptions = useMemo(
    () => buildAdminJobFilterOptions(jobsStrings, jobGroupCounts),
    [jobGroupCounts, jobsStrings],
  )
  const jobFilterSummary = useMemo(() => summarizeAdminJobFilter(jobFilter, jobsStrings), [jobFilter, jobsStrings])
  const visibleJobs = useMemo(
    () => MOCK_JOBS.filter((job) => jobMatchesGroup(job.job_type, jobFilter)),
    [jobFilter],
  )

  const toggleJob = (id: number) => {
    setExpandedJobs((prev) => {
      const next = new Set(prev)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }

  return (
    <AdminPageFrame activeModule="jobs">
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
                  data-testid="storybook-jobs-filter-trigger"
                >
                  <Icon icon="mdi:filter-outline" width={16} height={16} aria-hidden="true" />
                  <span style={{ whiteSpace: 'nowrap' }}>{jobFilterSummary}</span>
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-80">
                <DropdownMenuLabel>{jobsStrings.table.type}</DropdownMenuLabel>
                <DropdownMenuRadioGroup
                  value={jobFilter}
                  onValueChange={(value) => setJobFilter(value as JobGroup)}
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

        <div className="table-wrapper jobs-table-wrapper jobs-module-table-wrapper">
          <table className="jobs-table jobs-module-table">
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
              {visibleJobs.map((job) => {
                const expanded = expandedJobs.has(job.id)
                const hasMessage = Boolean(job.message?.trim())
                const jobTypeText = jobsStrings.types?.[job.job_type] ?? job.job_type
                const jobTypeDetail = jobTypeText === job.job_type ? jobTypeText : `${jobTypeText} (${job.job_type})`

                return (
                  <Fragment key={job.id}>
                    <tr>
                      <td>{job.id}</td>
                      <td>{jobTypeText}</td>
                      <td>
                        <JobKeyLink
                          keyId={job.key_id}
                          keyGroup={job.key_group}
                          ungroupedLabel={keyStrings.groups.ungrouped}
                          detailLabel={keyStrings.actions.details}
                        />
                      </td>
                      <td>
                        <StatusBadge tone={keyStatusTone(job.status)}>{admin.statuses[job.status] ?? job.status}</StatusBadge>
                      </td>
                      <td>{job.attempt}</td>
                      <td>{formatTimestamp(job.started_at)}</td>
                      <td className="jobs-message-cell">
                        {hasMessage ? (
                          <button
                            type="button"
                            className={`jobs-message-button${expanded ? ' jobs-message-button-active' : ''}`}
                            onClick={() => toggleJob(job.id)}
                            aria-expanded={expanded}
                            aria-controls={`storybook-job-details-${job.id}`}
                          >
                            <span className="jobs-message-text">{job.message}</span>
                            <Icon
                              icon={expanded ? 'mdi:chevron-up' : 'mdi:chevron-down'}
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
                    </tr>
                    {expanded && hasMessage && (
                      <tr className="log-details-row">
                        <td colSpan={7} id={`storybook-job-details-${job.id}`}>
                          <div className="log-details-panel">
                            <div className="log-details-summary">
                              <div>
                                <div className="log-details-label">{jobsStrings.table.id}</div>
                                <div className="log-details-value">{job.id}</div>
                              </div>
                              <div>
                                <div className="log-details-label">{jobsStrings.table.type}</div>
                                <div className="log-details-value">{jobTypeDetail}</div>
                              </div>
                              <div>
                                <div className="log-details-label">{jobsStrings.table.key}</div>
                                <div className="log-details-value">
                                  <JobKeyLink
                                    keyId={job.key_id}
                                    keyGroup={job.key_group}
                                    ungroupedLabel={keyStrings.groups.ungrouped}
                                    detailLabel={keyStrings.actions.details}
                                  />
                                </div>
                              </div>
                              <div>
                                <div className="log-details-label">{jobsStrings.table.status}</div>
                                <div className="log-details-value">{admin.statuses[job.status] ?? job.status}</div>
                              </div>
                            </div>
                            <div className="log-details-body">
                              <section className="log-details-section">
                                <header>{jobsStrings.table.message}</header>
                                <pre>{job.message}</pre>
                              </section>
                            </div>
                          </div>
                        </td>
                      </tr>
                    )}
                  </Fragment>
                )
              })}
              {visibleJobs.length === 0 ? (
                <tr>
                  <td colSpan={7}>
                    <div className="empty-state alert">{jobsStrings.empty.none}</div>
                  </td>
                </tr>
              ) : null}
            </tbody>
          </table>
        </div>

        <div className="table-pagination">
          <span className="panel-description">{jobsStrings.description} (1 / 2)</span>
          <div style={{ display: 'inline-flex', gap: 8 }}>
            <button type="button" className="btn btn-outline">
              {admin.tokens.pagination.prev}
            </button>
            <button type="button" className="btn btn-outline">
              {admin.tokens.pagination.next}
            </button>
          </div>
        </div>
      </section>
    </AdminPageFrame>
  )
}

function UsersPageCanvas(): JSX.Element {
  const admin = useTranslate().admin
  const { language } = useLanguage()
  const users = admin.users
  const [allowRegistration, setAllowRegistration] = useState(true)
  const [sortField, setSortField] = useState<AdminUsersSortField | null>(null)
  const [sortOrder, setSortOrder] = useState<SortDirection | null>(null)
  const {
    queryInput,
    query,
    applySearch,
    resetSearch,
    handleQueryInputChange,
    handleQueryInputKeyDown,
  } = useStorySearchController()
  const normalizedQuery = query.trim().toLowerCase()
  const effectiveSortField = sortField ?? ADMIN_USERS_DEFAULT_SORT_FIELD
  const effectiveSortOrder = sortOrder ?? ADMIN_USERS_DEFAULT_SORT_ORDER
  const filteredUsers = MOCK_USERS.filter((item) => {
    if (!normalizedQuery) return true
    const displayName = item.displayName?.toLowerCase() ?? ''
    const username = item.username?.toLowerCase() ?? ''
    return (
      item.userId.toLowerCase().includes(normalizedQuery)
      || displayName.includes(normalizedQuery)
      || username.includes(normalizedQuery)
    )
  })
  const sortedUsers = [...filteredUsers].sort((left, right) =>
    compareAdminUserSummaryRows(left, right, sortField, sortOrder)
  )

  const toggleSort = (field: AdminUsersSortField) => {
    const isActive = effectiveSortField === field
    let nextSort: AdminUsersSortField | null = field
    let nextOrder: SortDirection | null = ADMIN_USERS_DEFAULT_SORT_ORDER
    if (isActive && effectiveSortOrder === 'desc') {
      nextOrder = 'asc'
    } else if (isActive && effectiveSortOrder === 'asc') {
      nextSort = null
      nextOrder = null
    }
    setSortField(nextSort)
    setSortOrder(nextOrder)
  }

  return (
    <AdminPageFrame activeModule="users">
      <section className="surface panel">
        <div className="panel-header" style={{ gap: 12, flexWrap: 'wrap' }}>
          <div>
            <h2>{users.catalog.summaryTitle}</h2>
            <p className="panel-description">{users.catalog.summaryDescription}</p>
          </div>
          <button type="button" className="btn btn-outline">
            {users.userTags.manageCatalog}
          </button>
        </div>
        <div className="user-tag-summary-grid">
          {MOCK_TAG_CATALOG.map((tag) => {
            const isSystem = tag.systemKey != null
            const isBlockAll = tag.effectKind === 'block_all'
            const cardClasses = ['user-tag-summary-card', isBlockAll ? 'user-tag-summary-card-block' : '']
              .filter(Boolean)
              .join(' ')
            return (
              <article className={cardClasses} key={tag.id}>
                <div className="user-tag-summary-card-head">
                  <StoryUserTagBadge tag={{ ...tag }} users={users} />
                  <StatusBadge tone={isSystem ? 'info' : isBlockAll ? 'error' : 'neutral'}>
                    {isSystem ? users.catalog.scopeSystem : users.catalog.scopeCustom}
                  </StatusBadge>
                </div>
                <div className="user-tag-summary-count">
                  <strong>{formatNumber(tag.userCount)}</strong>
                  <span className="panel-description">{users.catalog.summaryAccounts}</span>
                </div>
              </article>
            )
          })}
        </div>
      </section>

      <section className="surface panel">
        <div className="panel-header" style={{ gap: 12, flexWrap: 'wrap' }}>
          <div>
            <h2>{users.title}</h2>
            <p className="panel-description">{users.description}</p>
          </div>
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
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                <div className="text-sm font-semibold">{users.registration.title}</div>
                <Badge variant={allowRegistration ? 'success' : 'warning'}>
                  {allowRegistration ? users.status.enabled : users.status.disabled}
                </Badge>
              </div>
              <p className="text-xs font-medium" role="status" aria-live="polite" style={{ margin: '6px 0 0' }}>
                {allowRegistration ? users.registration.enabled : users.registration.disabled}
              </p>
            </div>
            <Switch
              checked={allowRegistration}
              aria-label={users.registration.title}
              onCheckedChange={() => setAllowRegistration((current) => !current)}
              style={{ flex: '0 0 auto' }}
            />
          </div>
          <div className="users-search-controls">
            <Input
              type="text"
              name="users-search"
              className="users-search-input"
              placeholder={users.searchPlaceholder}
              value={queryInput}
              onChange={(event) => handleQueryInputChange(event.target.value)}
              onKeyDown={handleQueryInputKeyDown}
            />
            <Button type="button" variant="outline" onClick={applySearch}>
              {users.search}
            </Button>
            {(queryInput.length > 0 || query.length > 0) && (
              <Button type="button" variant="ghost" onClick={resetSearch}>
                {users.clear}
              </Button>
            )}
          </div>
        </div>

        <div className="table-wrapper jobs-table-wrapper">
          {filteredUsers.length === 0 ? (
            <div className="empty-state alert">{users.empty.none}</div>
          ) : (
            <table className="jobs-table admin-users-table admin-users-list-table">
              <thead>
                <tr>
                  <th>{users.table.user}</th>
                  <th>{users.table.status}</th>
                  <th>{users.table.tags}</th>
                  <StoryAdminUsersSortableHeader
                    label={users.table.daily}
                    field="quotaDailyUsed"
                    activeField={effectiveSortField}
                    activeOrder={effectiveSortOrder}
                    onToggle={toggleSort}
                  />
                  <StoryAdminUsersSortableHeader
                    label={users.table.monthly}
                    field="quotaMonthlyUsed"
                    activeField={effectiveSortField}
                    activeOrder={effectiveSortOrder}
                    onToggle={toggleSort}
                  />
                  <StoryAdminUsersSortableHeader
                    label={users.table.lastActivity}
                    field="lastActivity"
                    activeField={effectiveSortField}
                    activeOrder={effectiveSortOrder}
                    onToggle={toggleSort}
                  />
                  <StoryAdminUsersSortableHeader
                    label={users.table.lastLogin}
                    field="lastLoginAt"
                    activeField={effectiveSortField}
                    activeOrder={effectiveSortOrder}
                    onToggle={toggleSort}
                  />
                </tr>
              </thead>
              <tbody>
                {sortedUsers.map((item) => {
                  const dailyQuotaMetric = formatQuotaStackValue(item.quotaDailyUsed, item.quotaDailyLimit)
                  const monthlyQuotaMetric = formatQuotaStackValue(item.quotaMonthlyUsed, item.quotaMonthlyLimit)
                  const lastActivityMetric = formatStackedTimestamp(item.lastActivity, language)
                  const lastLoginMetric = formatStackedTimestamp(item.lastLoginAt, language)
                  return (
                  <tr key={item.userId}>
                    <td className="admin-users-identity-cell">
                      <button
                        type="button"
                        className="link-button admin-users-identity-button"
                        aria-label={users.actions.view}
                        onClick={() => openAdminStory('admin-pages--user-detail')}
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
                        {item.active ? users.status.active : users.status.inactive}
                      </StatusBadge>
                    </td>
                    <td className="admin-users-tags-cell">
                      <StoryUserTagBadgeList tags={item.tags} users={users} emptyLabel={users.userTags.empty} />
                    </td>
                    <td className="admin-users-compact-cell">
                      <div className="admin-table-value-stack">
                        <span className={`admin-table-value-primary${dailyQuotaMetric.primaryClassName ? ` ${dailyQuotaMetric.primaryClassName}` : ''}`}>{dailyQuotaMetric.primary}</span>
                        <span className="admin-table-value-secondary">{dailyQuotaMetric.secondary}</span>
                      </div>
                    </td>
                    <td className="admin-users-compact-cell">
                      <div className="admin-table-value-stack">
                        <span className={`admin-table-value-primary${monthlyQuotaMetric.primaryClassName ? ` ${monthlyQuotaMetric.primaryClassName}` : ''}`}>{monthlyQuotaMetric.primary}</span>
                        <span className="admin-table-value-secondary">{monthlyQuotaMetric.secondary}</span>
                      </div>
                    </td>
                    <td className="admin-users-compact-cell">
                      <div className="admin-table-value-stack">
                        <span className="admin-table-value-primary">{lastActivityMetric.primary}</span>
                        {lastActivityMetric.secondary && (
                          <span className="admin-table-value-secondary">{lastActivityMetric.secondary}</span>
                        )}
                      </div>
                    </td>
                    <td className="admin-users-compact-cell">
                      <div className="admin-table-value-stack">
                        <span className="admin-table-value-primary">{lastLoginMetric.primary}</span>
                        {lastLoginMetric.secondary && (
                          <span className="admin-table-value-secondary">{lastLoginMetric.secondary}</span>
                        )}
                      </div>
                    </td>
                  </tr>
                )})}
              </tbody>
            </table>
          )}
        </div>
      </section>
    </AdminPageFrame>
  )
}

function UsersUsagePageCanvas({
  initialDrawerUserId,
}: {
  initialDrawerUserId?: string
} = {}): JSX.Element {
  const admin = useTranslate().admin
  const { language } = useLanguage()
  const users = admin.users
  const usageDailyRateLabel = language === 'zh' ? users.usage.table.dailySuccessRate : 'Daily'
  const usageMonthlyRateLabel = language === 'zh' ? users.usage.table.monthlySuccessRate : 'Monthly'
  const [sortField, setSortField] = useState<AdminUsersSortField | null>(null)
  const [sortOrder, setSortOrder] = useState<SortDirection | null>(null)
  const {
    queryInput,
    query,
    applySearch,
    resetSearch,
    handleQueryInputChange,
    handleQueryInputKeyDown,
  } = useStorySearchController()
  const [monthlyBrokenDrawer, setMonthlyBrokenDrawer] = useState<{
    label: string
    items: MonthlyBrokenKeyDetail[]
  } | null>(() => {
    if (!initialDrawerUserId) return null
    const user = MOCK_USERS.find((item) => item.userId === initialDrawerUserId)
    if (!user) return null
    return {
      label: user.displayName || user.username || user.userId,
      items: MOCK_MONTHLY_BROKEN_ITEMS[`user:${user.userId}`] ?? [],
    }
  })
  const normalizedQuery = query.trim().toLowerCase()
  const effectiveSortField = sortField ?? ADMIN_USERS_DEFAULT_SORT_FIELD
  const effectiveSortOrder = sortOrder ?? ADMIN_USERS_DEFAULT_SORT_ORDER
  const filteredUsers = MOCK_USERS.filter((item) => {
    if (!normalizedQuery) return true
    const displayName = item.displayName?.toLowerCase() ?? ''
    const username = item.username?.toLowerCase() ?? ''
    return (
      item.userId.toLowerCase().includes(normalizedQuery)
      || displayName.includes(normalizedQuery)
      || username.includes(normalizedQuery)
    )
  })
  const sortedUsers = [...filteredUsers].sort((left, right) =>
    compareAdminUserSummaryRows(left, right, sortField, sortOrder)
  )

  const toggleSort = (field: AdminUsersSortField) => {
    const isActive = effectiveSortField === field
    let nextSort: AdminUsersSortField | null = field
    let nextOrder: SortDirection | null = ADMIN_USERS_DEFAULT_SORT_ORDER
    if (isActive && effectiveSortOrder === 'desc') {
      nextOrder = 'asc'
    } else if (isActive && effectiveSortOrder === 'asc') {
      nextSort = null
      nextOrder = null
    }
    setSortField(nextSort)
    setSortOrder(nextOrder)
  }

  return (
    <AdminPageFrame
      activeModule="user-usage"
      overlays={
        <StoryMonthlyBrokenDrawer
          open={monthlyBrokenDrawer != null}
          label={monthlyBrokenDrawer?.label ?? '—'}
          items={monthlyBrokenDrawer?.items ?? []}
          onOpenChange={(open) => {
            if (!open) setMonthlyBrokenDrawer(null)
          }}
        />
      }
      showDefaultShellChrome={false}
    >
      <AdminShellSidebarUtility>
        <AdminSidebarUtilityStack>
          <AdminSidebarUtilityCard>
            <div className="admin-sidebar-utility-toolbar">
              <ThemeToggle />
              <LanguageSwitcher />
            </div>
            <div className="admin-sidebar-utility-meta">
              <div className="user-badge user-badge-admin">
                <Icon icon="mdi:crown-outline" className="user-badge-icon" aria-hidden="true" />
                <span>Ops Admin</span>
              </div>
              <span className="admin-panel-header-time" aria-live="polite">
                <Icon icon="mdi:clock-time-four-outline" width={14} height={14} className="admin-panel-header-time-icon" aria-hidden="true" />
                <span className="admin-panel-header-time-label">{admin.header.updatedPrefix}</span>
                <span className="admin-panel-header-time-value">11:42:10</span>
              </span>
            </div>
          </AdminSidebarUtilityCard>
          <AdminSidebarUtilityCard>
            <div className="admin-sidebar-utility-actions">
              <AdminReturnToConsoleLink
                label={admin.header.returnToConsole}
                href="/console"
                className="admin-sidebar-utility-action"
              />
              <Button type="button" variant="outline" size="sm" className="admin-panel-refresh-button admin-sidebar-utility-action">
                <Icon icon="mdi:refresh" width={16} height={16} aria-hidden="true" />
                <span>{admin.header.refreshNow}</span>
              </Button>
              <Button
                type="button"
                variant="ghost"
                className="admin-sidebar-utility-action"
                onClick={() => openAdminStory('admin-pages--users')}
              >
                <Icon icon="mdi:arrow-left" width={16} height={16} aria-hidden="true" />
                <span>{users.usage.back}</span>
              </Button>
            </div>
          </AdminSidebarUtilityCard>
        </AdminSidebarUtilityStack>
      </AdminShellSidebarUtility>

      <div className="admin-desktop-only">
        <AdminCompactIntro
          title={users.usage.title}
          description={users.usage.description}
        />
      </div>

      <section className="surface panel">
        <div className="panel-header" style={{ gap: 12, flexWrap: 'wrap' }}>
          <div className="admin-stacked-only" style={{ flex: '1 1 340px', minWidth: 260 }}>
            <h2>{users.usage.title}</h2>
            <p className="panel-description">{users.usage.description}</p>
          </div>
          <div className="admin-inline-actions" style={{ flexWrap: 'wrap', justifyContent: 'flex-end' }}>
            <div className="admin-stacked-only">
              <Button type="button" variant="outline" onClick={() => openAdminStory('admin-pages--users')}>
                {users.usage.back}
              </Button>
            </div>
            <div className="users-search-controls">
              <Input
                type="text"
                name="user-usage-search"
                className="users-search-input"
                placeholder={users.searchPlaceholder}
                value={queryInput}
                onChange={(event) => handleQueryInputChange(event.target.value)}
                onKeyDown={handleQueryInputKeyDown}
              />
              <Button type="button" variant="outline" onClick={applySearch}>
                {users.search}
              </Button>
              {(queryInput.length > 0 || query.length > 0) && (
                <Button type="button" variant="ghost" onClick={resetSearch}>
                  {users.clear}
                </Button>
              )}
            </div>
          </div>
        </div>

        <div className="table-wrapper jobs-table-wrapper">
          {filteredUsers.length === 0 ? (
            <div className="empty-state alert">{users.empty.none}</div>
          ) : (
            <table className="jobs-table admin-users-table admin-users-usage-table">
              <thead>
                <tr>
                  <th>{users.usage.table.user}</th>
                  <th>{users.usage.table.status}</th>
                  <StoryAdminUsersSortableHeader
                    label={users.usage.table.hourlyAny}
                    field="hourlyAnyUsed"
                    activeField={effectiveSortField}
                    activeOrder={effectiveSortOrder}
                    onToggle={toggleSort}
                  />
                  <StoryAdminUsersSortableHeader
                    label={users.usage.table.hourly}
                    field="quotaHourlyUsed"
                    activeField={effectiveSortField}
                    activeOrder={effectiveSortOrder}
                    onToggle={toggleSort}
                  />
                  <StoryAdminUsersSortableHeader
                    label={users.usage.table.daily}
                    field="quotaDailyUsed"
                    activeField={effectiveSortField}
                    activeOrder={effectiveSortOrder}
                    onToggle={toggleSort}
                  />
                  <StoryAdminUsersSortableHeader
                    label={users.usage.table.monthly}
                    field="quotaMonthlyUsed"
                    activeField={effectiveSortField}
                    activeOrder={effectiveSortOrder}
                    onToggle={toggleSort}
                  />
                  <StoryAdminUsersSortableHeader
                    label={users.usage.table.monthlyBroken}
                    field="monthlyBrokenCount"
                    activeField={effectiveSortField}
                    activeOrder={effectiveSortOrder}
                    onToggle={toggleSort}
                  />
                  <StoryAdminUsersSortableHeader
                    label={users.usage.table.dailySuccessRate}
                    displayLabel={usageDailyRateLabel}
                    field="dailySuccessRate"
                    activeField={effectiveSortField}
                    activeOrder={effectiveSortOrder}
                    onToggle={toggleSort}
                  />
                  <StoryAdminUsersSortableHeader
                    label={users.usage.table.monthlySuccessRate}
                    displayLabel={usageMonthlyRateLabel}
                    field="monthlySuccessRate"
                    activeField={effectiveSortField}
                    activeOrder={effectiveSortOrder}
                    onToggle={toggleSort}
                  />
                  <StoryAdminUsersSortableHeader
                    label={users.usage.table.lastUsed}
                    field="lastActivity"
                    activeField={effectiveSortField}
                    activeOrder={effectiveSortOrder}
                    onToggle={toggleSort}
                  />
                </tr>
              </thead>
              <tbody>
                {sortedUsers.map((item) => {
                  const requestRate = resolveRequestRate(item, 'user')
                  const requestRateMetric = formatQuotaStackValue(requestRate.used, requestRate.limit)
                  const hourlyMetric = formatQuotaStackValue(item.quotaHourlyUsed, item.quotaHourlyLimit)
                  const dailyQuotaMetric = formatQuotaStackValue(item.quotaDailyUsed, item.quotaDailyLimit)
                  const monthlyQuotaMetric = formatQuotaStackValue(item.quotaMonthlyUsed, item.quotaMonthlyLimit)
                  const monthlyBrokenMetric = formatMonthlyBrokenStackValue(
                    item.monthlyBrokenCount,
                    item.monthlyBrokenLimit,
                  )
                  const dailySuccessMetric = formatSuccessRateStackValue(item.dailySuccess, item.dailyFailure, language)
                  const monthlySuccessMetric = formatSuccessRateStackValue(item.monthlySuccess, item.monthlyFailure, language)
                  const lastActivityMetric = formatStackedTimestamp(item.lastActivity, language)
                  const userLabel = item.displayName || item.username || item.userId
                  return (
                    <tr key={item.userId}>
                      <td className="admin-users-identity-cell">
                        <button
                          type="button"
                          className="link-button admin-users-identity-button"
                          aria-label={users.actions.view}
                          onClick={() => openAdminStory('admin-pages--user-detail')}
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
                          {item.active ? users.status.active : users.status.inactive}
                        </StatusBadge>
                      </td>
                      <td className="admin-users-compact-cell">
                        <div className="admin-table-value-stack">
                          <span className={`admin-table-value-primary${requestRateMetric.primaryClassName ? ` ${requestRateMetric.primaryClassName}` : ''}`}>{requestRateMetric.primary}</span>
                          <span className="admin-table-value-secondary">{requestRateMetric.secondary}</span>
                        </div>
                      </td>
                      <td className="admin-users-compact-cell">
                        <div className="admin-table-value-stack">
                          <span className={`admin-table-value-primary${hourlyMetric.primaryClassName ? ` ${hourlyMetric.primaryClassName}` : ''}`}>{hourlyMetric.primary}</span>
                          <span className="admin-table-value-secondary">{hourlyMetric.secondary}</span>
                        </div>
                      </td>
                      <td className="admin-users-compact-cell">
                        <div className="admin-table-value-stack">
                          <span className={`admin-table-value-primary${dailyQuotaMetric.primaryClassName ? ` ${dailyQuotaMetric.primaryClassName}` : ''}`}>{dailyQuotaMetric.primary}</span>
                          <span className="admin-table-value-secondary">{dailyQuotaMetric.secondary}</span>
                        </div>
                      </td>
                      <td className="admin-users-compact-cell">
                        <div className="admin-table-value-stack">
                          <span className={`admin-table-value-primary${monthlyQuotaMetric.primaryClassName ? ` ${monthlyQuotaMetric.primaryClassName}` : ''}`}>{monthlyQuotaMetric.primary}</span>
                          <span className="admin-table-value-secondary">{monthlyQuotaMetric.secondary}</span>
                        </div>
                      </td>
                      <td className="admin-users-compact-cell">
                        <div className="admin-table-value-stack">
                          <MonthlyBrokenCountTrigger
                            count={item.monthlyBrokenCount}
                            onOpen={() =>
                              setMonthlyBrokenDrawer({
                                label: userLabel,
                                items: MOCK_MONTHLY_BROKEN_ITEMS[`user:${item.userId}`] ?? [],
                              })}
                            ariaLabel={users.brokenKeys.openDetails.replace('{label}', userLabel)}
                            className={monthlyBrokenMetric.primaryClassName}
                          />
                          <span className="admin-table-value-secondary">{monthlyBrokenMetric.secondary}</span>
                        </div>
                      </td>
                      <td className="admin-users-compact-cell">
                        <div className="admin-table-value-stack">
                          <span className="admin-table-value-primary">{dailySuccessMetric.primary}</span>
                          <span className="admin-table-value-secondary">{dailySuccessMetric.secondary}</span>
                        </div>
                      </td>
                      <td className="admin-users-compact-cell">
                        <div className="admin-table-value-stack">
                          <span className="admin-table-value-primary">{monthlySuccessMetric.primary}</span>
                          <span className="admin-table-value-secondary">{monthlySuccessMetric.secondary}</span>
                        </div>
                      </td>
                      <td className="admin-users-compact-cell">
                        <div className="admin-table-value-stack">
                          <span className="admin-table-value-primary">{lastActivityMetric.primary}</span>
                          {lastActivityMetric.secondary && (
                            <span className="admin-table-value-secondary">{lastActivityMetric.secondary}</span>
                          )}
                        </div>
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          )}
        </div>
      </section>
    </AdminPageFrame>
  )
}

function UnboundTokenUsagePageCanvas({
  items = MOCK_UNBOUND_TOKEN_USAGE,
  errorMessage = null,
  initialDrawerTokenId,
  initialSortField = null,
  initialSortOrder = null,
}: {
  items?: AdminUnboundTokenUsageSummary[]
  errorMessage?: string | null
  initialDrawerTokenId?: string
  initialSortField?: AdminUnboundTokenUsageSortField | null
  initialSortOrder?: SortDirection | null
} = {}): JSX.Element {
  const admin = useTranslate().admin
  const { language } = useLanguage()
  const users = admin.users
  const tokenStrings = admin.tokens
  const strings = admin.unboundTokenUsage
  const dailyRateLabel = language === 'zh' ? strings.table.dailySuccessRate : 'Daily'
  const monthlyRateLabel = language === 'zh' ? strings.table.monthlySuccessRate : 'Monthly'
  const [query, setQuery] = useState('')
  const [page, setPage] = useState(1)
  const [sortField, setSortField] = useState<AdminUnboundTokenUsageSortField | null>(initialSortField)
  const [sortOrder, setSortOrder] = useState<SortDirection | null>(initialSortOrder)
  const [selectedTokenId, setSelectedTokenId] = useState<string | null>(null)
  const [monthlyBrokenDrawer, setMonthlyBrokenDrawer] = useState<{
    label: string
    items: MonthlyBrokenKeyDetail[]
  } | null>(() => {
    if (!initialDrawerTokenId) return null
    return {
      label: initialDrawerTokenId,
      items: MOCK_MONTHLY_BROKEN_ITEMS[`token:${initialDrawerTokenId}`] ?? [],
    }
  })
  const pageSize = 2
  const normalizedQuery = query.trim().toLowerCase()
  const effectiveSortField = sortField ?? ADMIN_UNBOUND_TOKEN_USAGE_DEFAULT_SORT_FIELD
  const effectiveSortOrder = sortOrder ?? ADMIN_UNBOUND_TOKEN_USAGE_DEFAULT_SORT_ORDER
  const filteredItems = items.filter((item) => {
    if (!normalizedQuery) return true
    return (
      item.tokenId.toLowerCase().includes(normalizedQuery)
      || (item.note?.toLowerCase() ?? '').includes(normalizedQuery)
      || (item.group?.toLowerCase() ?? '').includes(normalizedQuery)
    )
  })
  const sortedItems = [...filteredItems].sort((left, right) =>
    compareAdminUnboundTokenUsageRows(left, right, sortField, sortOrder)
  )
  const totalPages = Math.max(1, Math.ceil(sortedItems.length / pageSize))
  const safePage = Math.min(page, totalPages)
  const pagedItems = sortedItems.slice((safePage - 1) * pageSize, safePage * pageSize)

  const toggleSort = (field: AdminUnboundTokenUsageSortField) => {
    const isActive = effectiveSortField === field
    let nextSort: AdminUnboundTokenUsageSortField | null = field
    let nextOrder: SortDirection | null = ADMIN_UNBOUND_TOKEN_USAGE_DEFAULT_SORT_ORDER
    if (isActive && effectiveSortOrder === 'desc') {
      nextOrder = 'asc'
    } else if (isActive && effectiveSortOrder === 'asc') {
      nextSort = null
      nextOrder = null
    }
    setSortField(nextSort)
    setSortOrder(nextOrder)
    setPage(1)
  }

  return (
    <AdminPageFrame
      activeModule="tokens"
      overlays={
        <StoryMonthlyBrokenDrawer
          open={monthlyBrokenDrawer != null}
          label={monthlyBrokenDrawer?.label ?? '—'}
          items={monthlyBrokenDrawer?.items ?? []}
          onOpenChange={(open) => {
            if (!open) setMonthlyBrokenDrawer(null)
          }}
        />
      }
    >
      <section className="surface panel">
        <div className="panel-header" style={{ gap: 12, flexWrap: 'wrap' }}>
          <div className="admin-stacked-only" style={{ flex: '1 1 340px', minWidth: 260 }}>
            <h2>{strings.title}</h2>
            <p className="panel-description">{strings.description}</p>
            <p className="panel-description" data-selected-token>{selectedTokenId ? `Opened ${selectedTokenId}` : 'No token opened yet'}</p>
          </div>
          <div className="admin-inline-actions" style={{ flexWrap: 'wrap', justifyContent: 'flex-end' }}>
            <div className="admin-stacked-only">
              <button type="button" className="btn btn-outline" onClick={() => openAdminStory('admin-pages--tokens')}>
                {strings.back}
              </button>
            </div>
            <div className="users-search-controls">
              <input
                type="text"
                className="input input-bordered users-search-input"
                placeholder={strings.searchPlaceholder}
                value={query}
                onChange={(event) => {
                  setQuery(event.target.value)
                  setPage(1)
                }}
              />
              <button type="button" className="btn btn-outline">
                {users.search}
              </button>
              {query.length > 0 && (
                <button
                  type="button"
                  className="btn btn-ghost"
                  onClick={() => {
                    setQuery('')
                    setPage(1)
                  }}
                >
                  {users.clear}
                </button>
              )}
            </div>
          </div>
        </div>

        <div className="admin-desktop-only">
          <p className="panel-description" data-selected-token>{selectedTokenId ? `Opened ${selectedTokenId}` : 'No token opened yet'}</p>
        </div>

        <div className="table-wrapper jobs-table-wrapper admin-users-usage-table-wrapper admin-responsive-up">
          {pagedItems.length === 0 ? (
            <div className="empty-state alert">{errorMessage ?? strings.empty.none}</div>
          ) : (
            <table className="jobs-table admin-users-table admin-users-usage-table">
              <thead>
                <tr>
                  <th>{strings.table.identity}</th>
                  <th>{strings.table.status}</th>
                  <StoryAdminUsersSortableHeader
                    label={strings.table.hourlyAny}
                    field="hourlyAnyUsed"
                    activeField={effectiveSortField}
                    activeOrder={effectiveSortOrder}
                    onToggle={toggleSort}
                  />
                  <StoryAdminUsersSortableHeader
                    label={strings.table.hourly}
                    field="quotaHourlyUsed"
                    activeField={effectiveSortField}
                    activeOrder={effectiveSortOrder}
                    onToggle={toggleSort}
                  />
                  <StoryAdminUsersSortableHeader
                    label={strings.table.daily}
                    field="quotaDailyUsed"
                    activeField={effectiveSortField}
                    activeOrder={effectiveSortOrder}
                    onToggle={toggleSort}
                  />
                  <StoryAdminUsersSortableHeader
                    label={strings.table.monthly}
                    field="quotaMonthlyUsed"
                    activeField={effectiveSortField}
                    activeOrder={effectiveSortOrder}
                    onToggle={toggleSort}
                  />
                  <StoryAdminUsersSortableHeader
                    label={strings.table.monthlyBroken}
                    field="monthlyBrokenCount"
                    activeField={effectiveSortField}
                    activeOrder={effectiveSortOrder}
                    onToggle={toggleSort}
                  />
                  <StoryAdminUsersSortableHeader
                    label={strings.table.dailySuccessRate}
                    displayLabel={dailyRateLabel}
                    field="dailySuccessRate"
                    activeField={effectiveSortField}
                    activeOrder={effectiveSortOrder}
                    onToggle={toggleSort}
                  />
                  <StoryAdminUsersSortableHeader
                    label={strings.table.monthlySuccessRate}
                    displayLabel={monthlyRateLabel}
                    field="monthlySuccessRate"
                    activeField={effectiveSortField}
                    activeOrder={effectiveSortOrder}
                    onToggle={toggleSort}
                  />
                  <StoryAdminUsersSortableHeader
                    label={strings.table.lastUsed}
                    field="lastUsedAt"
                    activeField={effectiveSortField}
                    activeOrder={effectiveSortOrder}
                    onToggle={toggleSort}
                  />
                </tr>
              </thead>
              <tbody>
                {pagedItems.map((item) => {
                  const requestRate = resolveRequestRate(item, 'token')
                  const requestRateMetric = formatQuotaStackValue(requestRate.used, requestRate.limit)
                  const hourlyMetric = formatQuotaStackValue(item.quotaHourlyUsed, item.quotaHourlyLimit)
                  const dailyQuotaMetric = formatQuotaStackValue(item.quotaDailyUsed, item.quotaDailyLimit)
                  const monthlyQuotaMetric = formatQuotaStackValue(item.quotaMonthlyUsed, item.quotaMonthlyLimit)
                  const monthlyBrokenMetric =
                    item.monthlyBrokenCount == null || item.monthlyBrokenLimit == null
                      ? null
                      : formatMonthlyBrokenStackValue(item.monthlyBrokenCount, item.monthlyBrokenLimit)
                  const dailySuccessMetric = formatSuccessRateStackValue(item.dailySuccess, item.dailyFailure, language)
                  const monthlySuccessMetric = formatSuccessRateStackValue(item.monthlySuccess, item.monthlyFailure, language)
                  const lastUsedMetric = formatStackedTimestamp(item.lastUsedAt, language)
                  return (
                    <tr key={item.tokenId} data-token-row={item.tokenId}>
                      <td className="admin-users-identity-cell">
                        <button
                          type="button"
                          className="link-button admin-users-identity-button"
                          data-token-identity={item.tokenId}
                          onClick={() => setSelectedTokenId(item.tokenId)}
                        >
                          <strong>{item.tokenId}</strong>
                        </button>
                        <div className="panel-description admin-users-identity-meta">
                          {formatUnboundTokenIdentityMeta(item.note, item.group, tokenStrings.groups.label)}
                        </div>
                      </td>
                      <td>
                        <StatusBadge tone={item.enabled ? 'success' : 'neutral'}>
                          {item.enabled ? users.status.enabled : users.status.disabled}
                        </StatusBadge>
                      </td>
                      <td className="admin-users-compact-cell">
                        <div className="admin-table-value-stack">
                          <span className={`admin-table-value-primary${requestRateMetric.primaryClassName ? ` ${requestRateMetric.primaryClassName}` : ''}`}>{requestRateMetric.primary}</span>
                          <span className="admin-table-value-secondary">{requestRateMetric.secondary}</span>
                        </div>
                      </td>
                      <td className="admin-users-compact-cell">
                        <div className="admin-table-value-stack">
                          <span className={`admin-table-value-primary${hourlyMetric.primaryClassName ? ` ${hourlyMetric.primaryClassName}` : ''}`}>{hourlyMetric.primary}</span>
                          <span className="admin-table-value-secondary">{hourlyMetric.secondary}</span>
                        </div>
                      </td>
                      <td className="admin-users-compact-cell">
                        <div className="admin-table-value-stack">
                          <span className={`admin-table-value-primary${dailyQuotaMetric.primaryClassName ? ` ${dailyQuotaMetric.primaryClassName}` : ''}`}>{dailyQuotaMetric.primary}</span>
                          <span className="admin-table-value-secondary">{dailyQuotaMetric.secondary}</span>
                        </div>
                      </td>
                      <td className="admin-users-compact-cell">
                        <div className="admin-table-value-stack">
                          <span className={`admin-table-value-primary${monthlyQuotaMetric.primaryClassName ? ` ${monthlyQuotaMetric.primaryClassName}` : ''}`}>{monthlyQuotaMetric.primary}</span>
                          <span className="admin-table-value-secondary">{monthlyQuotaMetric.secondary}</span>
                        </div>
                      </td>
                      <td className="admin-users-compact-cell">
                        {monthlyBrokenMetric == null ? (
                          <div className="admin-table-value-stack">
                            <span className="admin-table-value-primary">—</span>
                          </div>
                        ) : (
                          <div className="admin-table-value-stack">
                            <MonthlyBrokenCountTrigger
                              count={item.monthlyBrokenCount ?? 0}
                              onOpen={() =>
                                setMonthlyBrokenDrawer({
                                  label: item.tokenId,
                                  items: MOCK_MONTHLY_BROKEN_ITEMS[`token:${item.tokenId}`] ?? [],
                                })}
                              ariaLabel={users.brokenKeys.openDetails.replace('{label}', item.tokenId)}
                              className={monthlyBrokenMetric.primaryClassName}
                            />
                            <span className="admin-table-value-secondary">{monthlyBrokenMetric.secondary}</span>
                          </div>
                        )}
                      </td>
                      <td className="admin-users-compact-cell">
                        <div className="admin-table-value-stack">
                          <span className="admin-table-value-primary">{dailySuccessMetric.primary}</span>
                          <span className="admin-table-value-secondary">{dailySuccessMetric.secondary}</span>
                        </div>
                      </td>
                      <td className="admin-users-compact-cell">
                        <div className="admin-table-value-stack">
                          <span className="admin-table-value-primary">{monthlySuccessMetric.primary}</span>
                          <span className="admin-table-value-secondary">{monthlySuccessMetric.secondary}</span>
                        </div>
                      </td>
                      <td className="admin-users-compact-cell">
                        <div className="admin-table-value-stack">
                          <span className="admin-table-value-primary">{lastUsedMetric.primary}</span>
                          {lastUsedMetric.secondary && (
                            <span className="admin-table-value-secondary">{lastUsedMetric.secondary}</span>
                          )}
                        </div>
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          )}
        </div>

        <div className="admin-mobile-list admin-responsive-down">
          {pagedItems.length === 0 ? (
            <div className="empty-state alert">{errorMessage ?? strings.empty.none}</div>
          ) : (
            pagedItems.map((item) => {
              const requestRate = resolveRequestRate(item, 'token')
              return (
              <article key={item.tokenId} className="admin-mobile-card">
                <div className="admin-mobile-identity-block">
                  <div className="admin-mobile-identity-row">
                    <span className="admin-mobile-identity-label">{strings.table.identity}</span>
                    <button
                      type="button"
                      className="link-button admin-users-mobile-link"
                      onClick={() => setSelectedTokenId(item.tokenId)}
                    >
                      <strong>{item.tokenId}</strong>
                    </button>
                  </div>
                  <div className="panel-description admin-mobile-identity-meta">
                    {formatUnboundTokenIdentityMeta(item.note, item.group, tokenStrings.groups.label)}
                  </div>
                </div>
                <div className="admin-mobile-kv">
                  <span>{strings.table.status}</span>
                  <StatusBadge tone={item.enabled ? 'success' : 'neutral'}>
                    {item.enabled ? users.status.enabled : users.status.disabled}
                  </StatusBadge>
                </div>
                <div className="admin-mobile-kv">
                  <span>{formatRequestRateSummary(requestRate, language)}</span>
                  <strong>{formatQuotaUsagePair(requestRate.used, requestRate.limit)}</strong>
                </div>
                <div className="admin-mobile-kv">
                  <span>{strings.table.hourly}</span>
                  <strong>{formatQuotaUsagePair(item.quotaHourlyUsed, item.quotaHourlyLimit)}</strong>
                </div>
                <div className="admin-mobile-kv">
                  <span>{strings.table.daily}</span>
                  <strong>{formatQuotaUsagePair(item.quotaDailyUsed, item.quotaDailyLimit)}</strong>
                </div>
                <div className="admin-mobile-kv">
                  <span>{strings.table.monthly}</span>
                  <strong>{formatQuotaUsagePair(item.quotaMonthlyUsed, item.quotaMonthlyLimit)}</strong>
                </div>
                <div className="admin-mobile-kv">
                  <span>{strings.table.monthlyBroken}</span>
                  {item.monthlyBrokenCount == null || item.monthlyBrokenLimit == null ? (
                    <strong>—</strong>
                  ) : item.monthlyBrokenCount > 0 ? (
                    <button
                      type="button"
                      className="link-button"
                      onClick={() =>
                        setMonthlyBrokenDrawer({
                          label: item.tokenId,
                          items: MOCK_MONTHLY_BROKEN_ITEMS[`token:${item.tokenId}`] ?? [],
                        })}
                    >
                      <strong>{formatQuotaUsagePair(item.monthlyBrokenCount, item.monthlyBrokenLimit)}</strong>
                    </button>
                  ) : (
                    <strong>{formatQuotaUsagePair(item.monthlyBrokenCount, item.monthlyBrokenLimit)}</strong>
                  )}
                </div>
                <div className="admin-mobile-kv">
                  <span>{strings.table.dailySuccessRate}</span>
                  <strong>{formatCompactSuccessRateValue(item.dailySuccess, item.dailyFailure, language)}</strong>
                </div>
                <div className="admin-mobile-kv">
                  <span>{strings.table.monthlySuccessRate}</span>
                  <strong>{formatCompactSuccessRateValue(item.monthlySuccess, item.monthlyFailure, language)}</strong>
                </div>
                <div className="admin-mobile-kv">
                  <span>{strings.table.lastUsed}</span>
                  <strong>{formatTimestamp(item.lastUsedAt)}</strong>
                </div>
              </article>
            )})
          )}
        </div>

        {errorMessage && (
          <div className="surface error-banner" style={{ marginTop: 12 }}>
            {errorMessage}
          </div>
        )}

        {sortedItems.length > pageSize && (
          <AdminTablePagination
            page={safePage}
            totalPages={totalPages}
            pageSummary={
              <span className="panel-description">
                {users.pagination.replace('{page}', String(safePage)).replace('{total}', String(totalPages))}
              </span>
            }
            previousLabel={tokenStrings.pagination.prev}
            nextLabel={tokenStrings.pagination.next}
            previousDisabled={safePage <= 1}
            nextDisabled={safePage >= totalPages}
            disabled={false}
            onPrevious={() => setPage((current) => Math.max(1, current - 1))}
            onNext={() => setPage((current) => Math.min(totalPages, current + 1))}
          />
        )}
      </section>
    </AdminPageFrame>
  )
}

function UsersUsageTooltipProofCanvas(): JSX.Element {
  const { language } = useLanguage()
  const users = useTranslate().admin.users
  const dailySuccessLabel = language === 'zh' ? users.usage.table.dailySuccessRate : 'Daily'
  const monthlySuccessLabel = language === 'zh' ? users.usage.table.monthlySuccessRate : 'Monthly'
  const dailySuccessTooltip = language === 'zh' ? '按最近 24 小时成功率排序' : 'Sort by 24h success rate'
  const monthlySuccessTooltip = language === 'zh' ? '按最近 30 天成功率排序' : 'Sort by 30d success rate'
  const dailyFailureText = language === 'zh' ? '失败 1' : '1 failed'
  const monthlyFailureText = language === 'zh' ? '失败 147' : '147 failed'

  return (
    <div style={{ display: 'grid', gap: 20, maxWidth: 840, margin: '0 auto' }}>
      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>Users usage tooltip proof</h2>
            <p className="panel-description">
              The table shell is intentionally clipped to reproduce the original overlap bug. Shared tooltips must
              render above the sticky header and scroll frame.
            </p>
          </div>
        </div>
        <div
          style={{
            overflow: 'hidden',
            maxHeight: 260,
            borderRadius: 28,
            border: '1px dashed hsl(var(--accent) / 0.42)',
            background: 'linear-gradient(180deg, hsl(var(--card) / 0.98), hsl(var(--muted) / 0.24))',
            padding: 18,
          }}
        >
          <div className="table-wrapper jobs-table-wrapper" style={{ maxHeight: 180, overflow: 'auto' }}>
            <table className="jobs-table admin-users-table admin-users-usage-table">
              <thead>
                <tr>
                  <th>{users.usage.table.user}</th>
                  <th>{users.usage.table.status}</th>
                  <th aria-sort="descending">
                    <Tooltip open>
                      <TooltipTrigger asChild>
                        <Button type="button" variant="ghost" size="sm" className="admin-table-sort-button is-active">
                          <span className="admin-table-sort-label">{dailySuccessLabel}</span>
                          <ArrowDown className="admin-table-sort-indicator" aria-hidden="true" />
                        </Button>
                      </TooltipTrigger>
                      <TooltipContent side="top">{dailySuccessTooltip}</TooltipContent>
                    </Tooltip>
                  </th>
                  <th aria-sort="descending">
                    <Tooltip open>
                      <TooltipTrigger asChild>
                        <Button type="button" variant="ghost" size="sm" className="admin-table-sort-button is-active">
                          <span className="admin-table-sort-label">{monthlySuccessLabel}</span>
                          <ArrowDown className="admin-table-sort-indicator" aria-hidden="true" />
                        </Button>
                      </TooltipTrigger>
                      <TooltipContent side="top">{monthlySuccessTooltip}</TooltipContent>
                    </Tooltip>
                  </th>
                </tr>
              </thead>
              <tbody>
                <tr>
                  <td>
                    <div className="admin-users-identity-cell">
                      <strong>unclejimao</strong>
                    </div>
                  </td>
                  <td>
                    <StatusBadge tone="success">{users.status.active}</StatusBadge>
                  </td>
                  <td>
                    <div className="admin-table-value-stack">
                      <span className="admin-table-value-primary">97.5%</span>
                      <span className="admin-table-value-secondary">{dailyFailureText}</span>
                    </div>
                  </td>
                  <td>
                    <div className="admin-table-value-stack">
                      <span className="admin-table-value-primary">94.1%</span>
                      <span className="admin-table-value-secondary">{monthlyFailureText}</span>
                    </div>
                  </td>
                </tr>
                <tr>
                  <td colSpan={4} style={{ height: 120 }} />
                </tr>
              </tbody>
            </table>
          </div>
        </div>
      </section>
    </div>
  )
}

function UserTagsPageCanvas({ editorMode = 'view' }: { editorMode?: StoryTagCardMode }): JSX.Element {
  const users = useTranslate().admin.users
  const cards: Array<AdminUserTag | null> = editorMode === 'new' ? [null, ...MOCK_TAG_CATALOG] : MOCK_TAG_CATALOG
  const editableTagId = 'team_lead'

  return (
    <AdminPageFrame activeModule="users">
      <section className="surface panel">
        <div className="panel-header" style={{ gap: 12, flexWrap: 'wrap' }}>
          <div>
            <h2>{users.catalog.title}</h2>
            <p className="panel-description">{users.catalog.description}</p>
          </div>
          <div className="user-tag-page-actions">
            <button type="button" className="btn btn-outline">{users.catalog.backToUsers}</button>
            <button type="button" className="btn btn-primary" disabled={editorMode === 'new'}>
              {users.catalog.actions.create}
            </button>
          </div>
        </div>
      </section>

      <section className="surface panel">
        <div className="user-tag-catalog-grid">
          {cards.map((tag, index) => {
            const mode: StoryTagCardMode = editorMode === 'new' && index === 0
              ? 'new'
              : editorMode === 'edit' && tag?.id === editableTagId
                ? 'edit'
                : 'view'
            return (
              <StoryUserTagCatalogCard
                key={tag?.id ?? `draft-${index}`}
                tag={tag}
                users={users}
                mode={mode}
              />
            )
          })}
        </div>
      </section>
    </AdminPageFrame>
  )
}

function UserDetailPageCanvas({
  initialUsageSeries = 'quota1h',
}: {
  initialUsageSeries?: AdminUserUsageSeriesKey
} = {}): JSX.Element {
  const users = useTranslate().admin.users
  const { language } = useLanguage()
  const detail = MOCK_USER_DETAIL
  const quotaSnapshot = buildStoryQuotaSnapshot(detail)
  const [quotaDraft, setQuotaDraft] = useState<Record<QuotaSliderField, string>>({
    hourlyAnyLimit: String(detail.quotaBase.hourlyAnyLimit),
    hourlyLimit: String(detail.quotaBase.hourlyLimit),
    dailyLimit: String(detail.quotaBase.dailyLimit),
    monthlyLimit: String(detail.quotaBase.monthlyLimit),
  })
  const [brokenLimitDraft, setBrokenLimitDraft] = useState(String(detail.monthlyBrokenLimit))
  const [brokenLimitSavedAt, setBrokenLimitSavedAt] = useState<number | null>(null)
  const [brokenLimitError, setBrokenLimitError] = useState<string | null>(null)
  const [monthlyBrokenDrawerOpen, setMonthlyBrokenDrawerOpen] = useState(false)
  const hasBlockAllTag = detail.tags.some((tag) => tag.effectKind === 'block_all')

  return (
    <AdminPageFrame
      activeModule="users"
      overlays={
        <StoryMonthlyBrokenDrawer
          open={monthlyBrokenDrawerOpen}
          label={detail.displayName || detail.username || detail.userId}
          items={MOCK_MONTHLY_BROKEN_ITEMS[`user:${detail.userId}`] ?? []}
          onOpenChange={setMonthlyBrokenDrawerOpen}
        />
      }
    >
      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{users.detail.title}</h2>
            <p className="panel-description">{users.detail.subtitle.replace('{id}', detail.userId)}</p>
          </div>
        </div>

        <div className="token-info-grid">
          <div className="token-info-card">
            <span className="token-info-label">{users.detail.userId}</span>
            <span className="token-info-value">
              <code>{detail.userId}</code>
            </span>
          </div>
          <div className="token-info-card">
            <span className="token-info-label">{users.table.displayName}</span>
            <span className="token-info-value">{detail.displayName ?? '—'}</span>
          </div>
          <div className="token-info-card">
            <span className="token-info-label">{users.table.username}</span>
            <span className="token-info-value">{detail.username ?? '—'}</span>
          </div>
          <div className="token-info-card">
            <span className="token-info-label">{users.table.status}</span>
            <span className="token-info-value">
              <StatusBadge tone={detail.active ? 'success' : 'neutral'}>
                {detail.active ? users.status.active : users.status.inactive}
              </StatusBadge>
            </span>
          </div>
          <div className="token-info-card">
            <span className="token-info-label">{users.table.lastLogin}</span>
            <span className="token-info-value">{formatTimestamp(detail.lastLoginAt)}</span>
          </div>
          <div className="token-info-card">
            <span className="token-info-label">{users.table.tokenCount}</span>
            <span className="token-info-value">{formatNumber(detail.tokenCount)}</span>
          </div>
        </div>
      </section>

      <section className="surface panel">
        <div className="panel-header" style={{ gap: 12, flexWrap: 'wrap' }}>
          <div>
            <h2>{users.brokenKeys.limitTitle}</h2>
            <p className="panel-description">{users.brokenKeys.limitDescription}</p>
          </div>
          {detail.monthlyBrokenCount > 0 ? (
            <Button type="button" variant="outline" onClick={() => setMonthlyBrokenDrawerOpen(true)}>
              {users.brokenKeys.openAction}
            </Button>
          ) : null}
        </div>
        <div className="token-info-grid">
          <div className="token-info-card">
            <span className="token-info-label">{users.usage.table.monthlyBroken}</span>
            <span className="token-info-value">{formatNumber(detail.monthlyBrokenCount)}</span>
          </div>
          <div className="token-info-card">
            <span className="token-info-label">{users.brokenKeys.limitField}</span>
            <span className="token-info-value">{formatNumber(detail.monthlyBrokenLimit)}</span>
          </div>
        </div>
        <div
          style={{
            marginTop: 16,
            display: 'flex',
            gap: 12,
            alignItems: 'flex-end',
            flexWrap: 'wrap',
          }}
        >
          <label style={{ display: 'grid', gap: 6, minWidth: 220 }}>
            <span className="token-info-label">{users.brokenKeys.limitField}</span>
            <Input
              type="text"
              inputMode="numeric"
              value={brokenLimitDraft}
              onChange={(event) => setBrokenLimitDraft(event.target.value)}
              aria-label={users.brokenKeys.limitField}
            />
          </label>
          <Button
            type="button"
            onClick={() => {
              const parsed = Number.parseInt(brokenLimitDraft, 10)
              if (!Number.isFinite(parsed) || parsed < 0) {
                setBrokenLimitError(users.brokenKeys.invalid)
                return
              }
              setBrokenLimitError(null)
              setBrokenLimitSavedAt(Date.now())
            }}
          >
            {users.brokenKeys.save}
          </Button>
        </div>
        <div style={{ marginTop: 12 }}>
          <span className="panel-description">
            {brokenLimitSavedAt
              ? users.brokenKeys.savedAt.replace('{time}', new Date(brokenLimitSavedAt).toLocaleTimeString())
              : users.brokenKeys.hint}
          </span>
        </div>
        {brokenLimitError ? (
          <div className="alert alert-error" role="alert" style={{ marginTop: 12 }}>
            {brokenLimitError}
          </div>
        ) : null}
      </section>

      <section className="surface panel">
        <div className="panel-header" style={{ gap: 12, flexWrap: 'wrap' }}>
          <div>
            <h2>{users.userTags.title}</h2>
            <p className="panel-description">{users.userTags.description}</p>
          </div>
          <button type="button" className="btn btn-outline">
            {users.userTags.manageCatalog}
          </button>
        </div>
        <div className="user-tag-binding-toolbar">
          <StoryUserTagBadgeList tags={detail.tags} users={users} emptyLabel={users.userTags.empty} />
          <div className="user-tag-bind-controls">
            <select className="select select-bordered" defaultValue="">
              <option value="">{users.userTags.bindPlaceholder}</option>
              <option value="suspended_manual">Suspended</option>
            </select>
            <button type="button" className="btn btn-primary">{users.userTags.bindAction}</button>
          </div>
        </div>
        <div className="user-tag-binding-list">
          {detail.tags.map((tag) => {
            const isSystem = isSystemUserTag(tag)
            return (
              <article className="user-tag-binding-card" key={`${tag.tagId}:${tag.source}`}>
                <div className="user-tag-binding-card-head">
                  <div className="user-tag-pill-list">
                    <StoryUserTagBadge tag={tag} users={users} />
                    <StatusBadge tone={isSystem ? 'info' : 'neutral'}>
                      {tag.source === 'system_linuxdo' ? users.userTags.sourceSystem : users.userTags.sourceManual}
                    </StatusBadge>
                  </div>
                  <button type="button" className="btn btn-ghost btn-sm" disabled={isSystem}>
                    {isSystem ? users.userTags.readOnly : users.userTags.unbindAction}
                  </button>
                </div>
                <div className="token-compact-pair">
                  <div className="token-compact-field">
                    <span className="token-compact-label">{users.quota.hourly}</span>
                    <span className="token-compact-value">{formatSignedQuotaDelta(tag.hourlyDelta)}</span>
                  </div>
                  <div className="token-compact-field">
                    <span className="token-compact-label">{users.quota.daily}</span>
                    <span className="token-compact-value">{formatSignedQuotaDelta(tag.dailyDelta)}</span>
                  </div>
                  <div className="token-compact-field">
                    <span className="token-compact-label">{users.quota.monthly}</span>
                    <span className="token-compact-value">{formatSignedQuotaDelta(tag.monthlyDelta)}</span>
                  </div>
                </div>
              </article>
            )
          })}
        </div>
      </section>

      <section className="surface panel">
        <div className="panel-header" style={{ gap: 12, flexWrap: 'wrap' }}>
          <div>
            <h2>{users.quota.title}</h2>
            <p className="panel-description">{users.quota.description}</p>
          </div>
          <StatusBadge tone={detail.quotaBase.inheritsDefaults ? 'info' : 'neutral'}>
            {detail.quotaBase.inheritsDefaults ? users.quota.inheritsDefaults : users.quota.customized}
          </StatusBadge>
        </div>
        <div className="quota-grid" style={{ marginTop: 12 }}>
          {([
            {
              field: 'hourlyLimit',
              label: users.quota.hourly,
              used: detail.quotaHourlyUsed,
              currentLimit: detail.quotaBase.hourlyLimit,
            },
            {
              field: 'dailyLimit',
              label: users.quota.daily,
              used: detail.quotaDailyUsed,
              currentLimit: detail.quotaBase.dailyLimit,
            },
            {
              field: 'monthlyLimit',
              label: users.quota.monthly,
              used: detail.quotaMonthlyUsed,
              currentLimit: detail.quotaBase.monthlyLimit,
            },
          ] as const).map((item) => {
            const sliderSeed = quotaSnapshot[item.field]
            const draftValue = quotaDraft[item.field]
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
                helperText={
                  <>
                    {formatNumber(sliderSeed.used)} / {formatNumber(parsedDraft)}
                  </>
                }
                sliderStyle={{ background: buildQuotaSliderTrack(sliderSeed.stages, sliderSeed.used, parsedDraft) }}
                onSliderChange={(nextValue) => setQuotaDraft((prev) => ({
                  ...prev,
                  [item.field]: String(
                    getQuotaSliderStageValue(
                      sliderSeed.stages,
                      clampQuotaSliderStageIndex(sliderSeed.stages, nextValue),
                    ),
                  ),
                }))}
                inputName={item.field}
                inputValue={formatQuotaDraftInput(draftValue)}
                inputAriaLabel={`${item.label} input`}
                onInputChange={(nextValue) => {
                  const normalizedValue = normalizeQuotaDraftInput(nextValue)
                  if (normalizedValue == null) return
                  setQuotaDraft((prev) => ({
                    ...prev,
                    [item.field]: normalizedValue,
                  }))
                }}
              />            )
          })}
        </div>
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{users.effectiveQuota.title}</h2>
            <p className="panel-description">{users.effectiveQuota.description}</p>
          </div>
        </div>
        {hasBlockAllTag && <div className="alert alert-warning">{users.effectiveQuota.blockAllNotice}</div>}
        <div className="token-info-grid">
          {([
            ['hourly', users.quota.hourly, detail.effectiveQuota.hourlyLimit],
            ['daily', users.quota.daily, detail.effectiveQuota.dailyLimit],
            ['monthly', users.quota.monthly, detail.effectiveQuota.monthlyLimit],
          ] as const).map(([key, label, value]) => (
            <div className="token-info-card" key={key}>
              <span className="token-info-label">{label}</span>
              <span className="token-info-value">{formatQuotaLimitValue(value)}</span>
            </div>
          ))}
        </div>
        <UserDetailQuotaBreakdown
          entries={detail.quotaBreakdown}
          usersStrings={users}
          formatQuotaLimitValue={formatQuotaLimitValue}
          formatSignedQuotaDelta={formatSignedQuotaDelta}
        />
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{users.detail.sharedUsageTitle}</h2>
            <p className="panel-description">{users.detail.sharedUsageDescription}</p>
          </div>
        </div>
        <UserDetailSharedUsagePanel
          usersStrings={users}
          language={language}
          initialSeries={initialUsageSeries}
          loadSeries={async (series) => {
            await new Promise((resolve) => window.setTimeout(resolve, 20))
            return MOCK_USER_USAGE_SERIES[series]
          }}
        />
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{users.detail.tokensTitle}</h2>
            <p className="panel-description">{users.detail.tokensDescription}</p>
          </div>
        </div>
        <div className="table-wrapper jobs-table-wrapper">
          <UserDetailTokenTable
            tokens={detail.tokens}
            usersStrings={users}
            formatNumber={formatNumber}
            formatTimestamp={(value) => formatTimestamp(value ?? null)}
            onViewToken={() => {}}
          />
        </div>
      </section>
    </AdminPageFrame>
  )
}

function AlertsPageCanvas(): JSX.Element {
  const admin = useTranslate().admin
  const { language } = useLanguage()
  const [search, setSearch] = useState(() => '?view=events')
  return (
    <AdminPageFrame activeModule="alerts">
      <AlertsCenter
        language={language}
        search={search}
        refreshToken={0}
        onNavigate={setSearch}
        onOpenUser={() => {}}
        onOpenToken={() => {}}
        onOpenKey={() => {}}
        formatTime={formatTimestamp}
        formatTimeDetail={formatTimestamp}
        catalogLoader={async () => STORY_ALERT_CATALOG}
        eventsLoader={async () => STORY_ALERT_EVENTS_PAGE}
        groupsLoader={async () => STORY_ALERT_GROUPS_PAGE}
        requestLoader={async (requestId) => STORY_ALERT_REQUEST_BODIES[requestId] ?? { request_body: null, response_body: null }}
      />
    </AdminPageFrame>
  )
}

function ProxySettingsPageCanvas(): JSX.Element {
  const admin = useTranslate().admin

  return (
    <AdminPageFrame activeModule="proxy-settings">
      <ForwardProxySettingsModule
        strings={admin.proxySettings}
        settings={forwardProxyStorySettings}
        stats={forwardProxyStoryStats}
        settingsLoadState="ready"
        statsLoadState="ready"
        settingsError={null}
        statsError={null}
        saveError={null}
        revalidateError={null}
        saving={false}
        revalidating={false}
        savedAt={forwardProxyStorySavedAt}
        revalidateProgress={null}
        onPersistDraft={async () => {}}
        onValidateCandidates={async () => []}
        onRefresh={() => {}}
        onRevalidate={() => {}}
      />
    </AdminPageFrame>
  )
}

function SystemSettingsPageCanvas(): JSX.Element {
  const admin = useTranslate().admin

  return (
    <AdminPageFrame activeModule="system-settings">
      <SystemSettingsModule
        strings={admin.systemSettings}
        settings={{
          requestRateLimit: 100,
          mcpSessionAffinityKeyCount: 5,
          rebalanceMcpEnabled: false,
          rebalanceMcpSessionPercent: 100,
        }}
        loadState="ready"
        error={null}
        saving={false}
        onApply={() => {}}
      />
    </AdminPageFrame>
  )
}

const meta = {
  title: 'Admin/Pages',
  tags: ['autodocs'],
  parameters: {
    docs: {
      description: {
        component: [
          'Route-level admin review surface covering dashboard, keys, tokens, users, jobs, system settings, and forward proxy settings.',
          '',
          'Public docs: [Configuration & Access](../configuration-access.html) · [Deployment & Anonymity](../deployment-anonymity.html) · [Storybook Guide](../storybook-guide.html)',
        ].join('\n'),
      },
    },
    layout: 'fullscreen',
  },
  decorators: [
    (Story) => (
      <LanguageProvider>
        <div
          style={{
            minHeight: '100vh',
            padding: 24,
            color: 'hsl(var(--foreground))',
            background: [
              'radial-gradient(1000px 520px at 6% -8%, hsl(var(--primary) / 0.14), transparent 62%)',
              'radial-gradient(900px 460px at 95% -14%, hsl(var(--accent) / 0.12), transparent 64%)',
              'linear-gradient(180deg, hsl(var(--background)) 0%, hsl(var(--background)) 62%, hsl(var(--muted) / 0.58) 100%)',
              'hsl(var(--background))',
            ].join(', '),
          }}
        >
          <Story />
        </div>
      </LanguageProvider>
    ),
  ],
} satisfies Meta

export default meta

type Story = StoryObj<typeof meta>

export const Dashboard: Story = {
  render: () => <DashboardPageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 80))
    const root = canvasElement.ownerDocument
    const utility = root.querySelector<HTMLElement>('.admin-sidebar-utility')
    const intro = root.querySelector<HTMLElement>('.admin-compact-intro')
    const stackedChrome = root.querySelector<HTMLElement>('.admin-stacked-only')

    if (!utility || !intro || !stackedChrome) {
      throw new Error('Expected admin page chrome fixtures to render for dashboard story.')
    }
    if (window.getComputedStyle(intro).display === 'none') {
      throw new Error('Expected compact intro to remain visible at desktop width.')
    }
    if (window.getComputedStyle(stackedChrome).display !== 'none') {
      throw new Error('Expected stacked header chrome to be hidden at desktop width.')
    }
  },
}

export const DashboardStacked: Story = {
  render: () => <DashboardPageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1100-breakpoint-admin-stack-max' },
  },
}

export const Tokens: Story = {
  render: () => <TokensPageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const Keys: Story = {
  render: () => <KeysPageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const KeysSelected: Story = {
  render: () => <KeysPageCanvas initialSelectedIds={['MZli', 'c7Pk']} />,
  parameters: {
    viewport: { defaultViewport: '1280-breakpoint-tailwind-xl' },
  },
}

export const KeysSyncUsageInProgress: Story = {
  render: () => (
    <KeysPageCanvas
      initialSelectedIds={['MZli', 'c7Pk', 'Qn8R', 'asR8', 'U2vK', 'J1nW']}
      bulkActionInFlight="sync_usage"
      bulkSyncProgress={STORY_BULK_SYNC_PROGRESS}
    />
  ),
  parameters: {
    viewport: { defaultViewport: '1280-breakpoint-tailwind-xl' },
  },
}

export const KeysSelectionRetainedAfterSync: Story = {
  render: () => (
    <KeysPageCanvas
      initialSelectedIds={retainVisibleApiKeySelection(['MZli', 'c7Pk', 'Qn8R', 'asR8'], ['MZli', 'c7Pk'])}
      bulkFeedback={{
        kind: 'success',
        message: '同步额度完成：列表已刷新，仍在当前页中的 2 个密钥继续保持勾选。',
      }}
    />
  ),
  parameters: {
    viewport: { defaultViewport: '1280-breakpoint-tailwind-xl' },
  },
}

export const KeysRegistrationFilters: Story = {
  render: () => (
    <KeysPageCanvas
      initialRegistrationIp="8.8.8.8"
      initialRegions={['US']}
    />
  ),
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const Requests: Story = {
  render: () => <RequestsPageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 120))
    const text = canvasElement.ownerDocument.body.textContent ?? ''
    for (const expected of ['结果与影响', '限额', '已耗尽']) {
      if (!text.includes(expected)) {
        throw new Error(`Expected requests story to contain: ${expected}`)
      }
    }
  },
}

export const RequestsResultFilterOpen: Story = {
  render: () => <RequestsPageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 120))
    const root = canvasElement.ownerDocument
    const trigger = Array.from(root.querySelectorAll<HTMLButtonElement>('button')).find((button) =>
      button.getAttribute('aria-label')?.startsWith('结果与影响:'),
    )
    if (!trigger) {
      throw new Error('Expected result/effect filter trigger to render.')
    }
    trigger.click()
    await new Promise((resolve) => window.setTimeout(resolve, 120))
    const text = root.body.textContent ?? ''
    for (const expected of ['结果', '限额', 'Key 影响', '已耗尽']) {
      if (!text.includes(expected)) {
        throw new Error(`Expected open result filter to contain: ${expected}`)
      }
    }
  },
}

export const KeyDetailRecentRequests: Story = {
  render: () => <StoryKeyDetailsCanvas id="MZli" logs={MOCK_REQUESTS} />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const TokenDetailRecentRequests: Story = {
  render: () => <TokenDetailStoryCanvas detail={buildRequestStoryTokenDetail('tok_req_001')} />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const RequestsTokenDrawerDesktop: Story = {
  render: () => <RequestsPageCanvas initialDrawerTarget={{ kind: 'token', id: 'tok_req_001' }} />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 200))
    const drawerBody = canvasElement.ownerDocument.querySelector<HTMLElement>('.request-entity-drawer-body')
    if (!drawerBody) {
      throw new Error('Expected request drawer body to be mounted.')
    }
    const utility = drawerBody?.querySelector<HTMLElement>('.admin-sidebar-utility')
    const intro = drawerBody?.querySelector<HTMLElement>('.admin-compact-intro')
    if (!utility || !intro) {
      throw new Error('Expected request drawer token detail to render desktop utility fallback and compact intro.')
    }
    const text = drawerBody.textContent ?? ''
    for (const expected of ['Regenerate Secret', 'Back']) {
      if (!text.includes(expected)) {
        throw new Error(`Expected request drawer token detail to contain: ${expected}`)
      }
    }
  },
}

export const Jobs: Story = {
  render: () => <JobsPageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 80))
    const root = canvasElement.ownerDocument
    const trigger = root.querySelector<HTMLButtonElement>('[data-testid="storybook-jobs-filter-trigger"]')
    if (!trigger) {
      throw new Error('Expected jobs filter trigger to render for jobs story.')
    }
    trigger.click()
    await new Promise((resolve) => window.setTimeout(resolve, 80))
    const linuxdoOption = Array.from(root.querySelectorAll<HTMLElement>('[role="menuitemradio"]')).find(
      (item) => item.textContent?.includes('LinuxDo'),
    )
    if (!linuxdoOption) {
      throw new Error('Expected LinuxDo filter option to render inside the jobs filter menu.')
    }
    if (!linuxdoOption.textContent?.includes('1')) {
      throw new Error('Expected LinuxDo filter option to render its group count inside the jobs filter menu.')
    }
  },
}

async function waitForStoryUi(ms = 80): Promise<void> {
  await new Promise((resolve) => window.setTimeout(resolve, ms))
}

function updateStorySearchInput(input: HTMLInputElement, value: string): void {
  input.focus()
  const valueSetter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set
  if (valueSetter) {
    valueSetter.call(input, value)
  } else {
    input.value = value
  }
  input.dispatchEvent(new Event('input', { bubbles: true, cancelable: true }))
}

function submitStorySearchInput(input: HTMLInputElement): void {
  input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))
}

function getFirstRenderedUserLabel(root: ParentNode): string | null {
  return root.querySelector<HTMLElement>('tbody tr strong')?.textContent?.trim() ?? null
}

export const Users: Story = {
  render: () => <UsersPageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
  play: async ({ canvasElement }) => {
    await waitForStoryUi()
    const searchInput = canvasElement.querySelector<HTMLInputElement>('input[name="users-search"]')
    if (!searchInput) {
      throw new Error('Expected users story to render the users search input.')
    }
    if (getFirstRenderedUserLabel(canvasElement) !== 'Alice Wang') {
      throw new Error('Expected users story to start with Alice Wang as the first rendered row.')
    }

    updateStorySearchInput(searchInput, 'bob')
    await waitForStoryUi(320)

    if (canvasElement.ownerDocument.activeElement !== searchInput) {
      throw new Error('Expected users search input to keep focus while typing before submitting.')
    }
    if (getFirstRenderedUserLabel(canvasElement) !== 'Alice Wang') {
      throw new Error('Expected users story to keep the unfiltered order before the search is submitted.')
    }

    submitStorySearchInput(searchInput)
    await waitForStoryUi()
    if (getFirstRenderedUserLabel(canvasElement) !== 'Bob Chen') {
      throw new Error('Expected users story to filter rows only after pressing Enter.')
    }

    const clearButton = canvasElement.querySelector<HTMLButtonElement>('.users-search-controls button:last-of-type')
    clearButton?.click()
    await waitForStoryUi()
    if (searchInput.value !== '') {
      throw new Error('Expected users story clear action to reset the search draft.')
    }
    if (getFirstRenderedUserLabel(canvasElement) !== 'Alice Wang') {
      throw new Error('Expected users story clear action to restore the full list.')
    }
  },
}

export const UsersUsage: Story = {
  render: () => <UsersUsagePageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
  play: async ({ canvasElement }) => {
    await waitForStoryUi()
    const utility = canvasElement.querySelector<HTMLElement>('.admin-sidebar-utility')
    const intro = canvasElement.querySelector<HTMLElement>('.admin-compact-intro')
    if (!utility) {
      throw new Error('Expected user usage page to render desktop sidebar utility.')
    }
    if (!intro || !intro.textContent?.includes('用量')) {
      throw new Error('Expected user usage page to render a compact intro with the page title.')
    }

    const searchInput = canvasElement.querySelector<HTMLInputElement>('input[name="user-usage-search"]')
    if (!searchInput) {
      throw new Error('Expected users usage story to render the usage search input.')
    }
    if (getFirstRenderedUserLabel(canvasElement) !== 'Alice Wang') {
      throw new Error('Expected users usage story to start with Alice Wang as the first rendered row.')
    }

    updateStorySearchInput(searchInput, 'charlie')
    await waitForStoryUi(320)

    if (canvasElement.ownerDocument.activeElement !== searchInput) {
      throw new Error('Expected users usage search input to keep focus while typing before submitting.')
    }
    if (getFirstRenderedUserLabel(canvasElement) !== 'Alice Wang') {
      throw new Error('Expected users usage story to keep the unfiltered order before the search is submitted.')
    }

    submitStorySearchInput(searchInput)
    await waitForStoryUi()
    if (getFirstRenderedUserLabel(canvasElement) !== 'Charlie Li') {
      throw new Error('Expected users usage story to filter rows only after pressing Enter.')
    }

    const clearButton = canvasElement.querySelector<HTMLButtonElement>('.users-search-controls button:last-of-type')
    clearButton?.click()
    await waitForStoryUi()
    if (searchInput.value !== '') {
      throw new Error('Expected users usage story clear action to reset the search draft.')
    }
    if (getFirstRenderedUserLabel(canvasElement) !== 'Alice Wang') {
      throw new Error('Expected users usage story clear action to restore the full list.')
    }
  },
}

export const UsersUsageStacked: Story = {
  render: () => <UsersUsagePageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1100-breakpoint-admin-stack-max' },
  },
}

export const UsersUsageBreakageDrawerProof: Story = {
  render: () => <UsersUsagePageCanvas initialDrawerUserId="usr_alice" />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const UnboundTokenUsage: Story = {
  render: () => <UnboundTokenUsagePageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 80))
    const sortButton = canvasElement.querySelector<HTMLButtonElement>('[data-sort-field="dailySuccessRate"]')
    sortButton?.click()
    await new Promise((resolve) => window.setTimeout(resolve, 80))
    const firstIdentity = canvasElement.querySelector<HTMLElement>('[data-token-identity]')
    if (firstIdentity?.textContent?.trim() !== 'tmp4') {
      throw new Error('Expected daily success sort to move tmp4 to the first row.')
    }
  },
}

export const UnboundTokenUsageMonthlyBrokenSortProof: Story = {
  render: () => (
    <UnboundTokenUsagePageCanvas initialSortField="monthlyBrokenCount" initialSortOrder="desc" />
  ),
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 80))
    const firstIdentity = canvasElement.querySelector<HTMLElement>('[data-token-identity]')
    if (firstIdentity?.textContent?.trim() !== 'qa13') {
      throw new Error('Expected monthly broken sort to move qa13 to the first row.')
    }
    const sortHeader = canvasElement.querySelector<HTMLElement>('th[aria-sort="descending"] [data-sort-field="monthlyBrokenCount"]')
    if (!sortHeader) {
      throw new Error('Expected monthly broken sort header to remain active in descending order.')
    }
  },
}

export const UnboundTokenUsageBreakageDrawerProof: Story = {
  render: () => <UnboundTokenUsagePageCanvas initialDrawerTokenId="qa13" />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const UnboundTokenUsageMobile: Story = {
  render: () => <UnboundTokenUsagePageCanvas />,
  parameters: {
    viewport: { defaultViewport: '0390-device-iphone-14' },
  },
}

export const UnboundTokenUsageStacked: Story = {
  render: () => <UnboundTokenUsagePageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1100-breakpoint-admin-stack-max' },
  },
}

export const UnboundTokenUsageEmpty: Story = {
  render: () => <UnboundTokenUsagePageCanvas items={[]} />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const UnboundTokenUsageError: Story = {
  render: () => <UnboundTokenUsagePageCanvas items={[]} errorMessage="Unable to load unbound token usage" />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const UnboundTokenUsageTokenDetailTrigger: Story = {
  render: () => <UnboundTokenUsagePageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 80))
    const tokenButton = canvasElement.querySelector<HTMLButtonElement>('[data-token-identity="qa13"]')
    tokenButton?.click()
    await new Promise((resolve) => window.setTimeout(resolve, 80))
    const status = canvasElement.querySelector<HTMLElement>('[data-selected-token]')
    if (!status?.textContent?.includes('qa13')) {
      throw new Error('Expected token detail trigger story to record qa13 as the opened token.')
    }
  },
}

export const UsersUsageTooltipProof: Story = {
  render: () => <UsersUsageTooltipProofCanvas />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const MonthlyBrokenDrawerEmpty: Story = {
  render: () => <MonthlyBrokenDrawerStoryCanvas label="Alice Wang" items={[]} />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const MonthlyBrokenDrawerSingleRow: Story = {
  render: () => <MonthlyBrokenDrawerStoryCanvas label="Alice Wang" items={MONTHLY_BROKEN_DRAWER_SINGLE_ITEM} />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const MonthlyBrokenDrawerLongContent: Story = {
  render: () => <MonthlyBrokenDrawerStoryCanvas label="Alice Wang" items={MONTHLY_BROKEN_DRAWER_LONG_CONTENT_ITEMS} />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const MonthlyBrokenDrawerOverflow: Story = {
  render: () => <MonthlyBrokenDrawerStoryCanvas label="Alice Wang" items={MONTHLY_BROKEN_DRAWER_OVERFLOW_ITEMS} />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const MonthlyBrokenDrawerMobile: Story = {
  render: () => <MonthlyBrokenDrawerStoryCanvas label="Alice Wang" items={MOCK_MONTHLY_BROKEN_ITEMS['user:usr_alice']} />,
  parameters: {
    viewport: { defaultViewport: '0390-device-iphone-14' },
  },
}

export const UserTags: Story = {
  render: () => <UserTagsPageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const UserTagNew: Story = {
  render: () => <UserTagsPageCanvas editorMode="new" />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const UserTagEdit: Story = {
  render: () => <UserTagsPageCanvas editorMode="edit" />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const UserDetailSharedUsageTooltip: Story = {
  render: () => <UserDetailPageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 120))
    const canvas = canvasElement.querySelector<HTMLCanvasElement>('.admin-user-shared-usage-chart canvas')
    const usagePanel = canvasElement.querySelector<HTMLElement>('.admin-user-shared-usage-panel')
    if (!canvas || !usagePanel) {
      throw new Error('Expected the tooltip proof story to render the shared usage chart canvas.')
    }

    const rect = canvas.getBoundingClientRect()
    const clientX = rect.left + rect.width * 0.72
    const clientY = rect.top + rect.height * 0.32
    const pointerTarget = canvas.ownerDocument.elementFromPoint(clientX, clientY) ?? canvas

    pointerTarget.dispatchEvent(
      new MouseEvent('mousemove', {
        bubbles: true,
        cancelable: true,
        clientX,
        clientY,
      }),
    )
    await new Promise((resolve) => window.setTimeout(resolve, 80))

    const hoverTooltip = canvasElement.querySelector<HTMLElement>('.admin-user-shared-usage-tooltip')
    const tooltipOpenWhileHovering = usagePanel.getAttribute('data-tooltip-open')
    if (!hoverTooltip?.textContent?.includes('已用') || tooltipOpenWhileHovering !== 'true') {
      throw new Error('Expected hovering the shared usage chart to open the floating detail bubble.')
    }

    const outsidePlotClientY = rect.top + 6
    pointerTarget.dispatchEvent(
      new MouseEvent('mousemove', {
        bubbles: true,
        cancelable: true,
        clientX,
        clientY: outsidePlotClientY,
      }),
    )
    await new Promise((resolve) => window.setTimeout(resolve, 80))

    const tooltipOpenAfterLeaving = usagePanel.getAttribute('data-tooltip-open')
    if (tooltipOpenAfterLeaving !== 'false') {
      throw new Error('Expected hover tooltip to disappear after leaving the plot area vertically.')
    }

    pointerTarget.dispatchEvent(
      new MouseEvent('mousemove', {
        bubbles: true,
        cancelable: true,
        clientX,
        clientY,
      }),
    )
    await new Promise((resolve) => window.setTimeout(resolve, 80))

    pointerTarget.dispatchEvent(
      new MouseEvent('click', {
        bubbles: true,
        cancelable: true,
        clientX,
        clientY,
      }),
    )
    await new Promise((resolve) => window.setTimeout(resolve, 80))

    if (usagePanel.dataset.tooltipPinned !== 'true') {
      throw new Error('Expected clicking the shared usage chart to pin the floating detail bubble.')
    }
  },
}

export const UserDetail: Story = {
  render: () => <UserDetailPageCanvas />,
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 80))
    const usagePanel = canvasElement.querySelector<HTMLElement>('.admin-user-shared-usage-panel')
    if (!usagePanel) {
      throw new Error('Expected user detail story to render the shared usage panel.')
    }
    if (usagePanel.dataset.loadedSeries !== 'quota1h') {
      throw new Error(`Expected the default story to lazy-load only quota1h, received ${usagePanel.dataset.loadedSeries ?? '<empty>'}.`)
    }

    const headerText = canvasElement.querySelector('.admin-user-tokens-table thead')?.textContent ?? ''
    if (headerText.includes('限流') || headerText.includes('24h') || headerText.includes('月度')) {
      throw new Error('Expected the token table headers to drop shared quota columns.')
    }
    if (!headerText.includes('累计请求') || !headerText.includes('创建时间')) {
      throw new Error('Expected the token table headers to include total requests and created time.')
    }

    const tokenTableWrapper = canvasElement.querySelector('.admin-user-tokens-table')?.closest<HTMLElement>('.admin-responsive-up')
    const breakdownTableWrapper = canvasElement.querySelector('.user-tag-breakdown-table')?.closest<HTMLElement>('.admin-responsive-up')
    for (const [label, wrapper] of [
      ['token table', tokenTableWrapper],
      ['quota breakdown table', breakdownTableWrapper],
    ] as const) {
      if (!wrapper) {
        throw new Error(`Expected the ${label} wrapper to exist in the desktop story.`)
      }
      if (wrapper.scrollWidth > wrapper.clientWidth + 1) {
        throw new Error(`Expected the desktop ${label} wrapper to avoid horizontal overflow.`)
      }
    }

    ;['5m', '24h', '月'].forEach((label) => {
      const button = Array.from(canvasElement.querySelectorAll<HTMLButtonElement>('.segmented-tab'))
        .find((item) => item.textContent?.trim() === label)
      button?.click()
    })
    await new Promise((resolve) => window.setTimeout(resolve, 80))

    const loadedSeries = usagePanel.dataset.loadedSeries?.split(',').filter(Boolean) ?? []
    const expected = ['quota1h', 'rate5m', 'quota24h', 'quotaMonth']
    if (expected.some((value) => !loadedSeries.includes(value))) {
      throw new Error(`Expected shared usage tabs to lazy-load all series after interaction, received ${loadedSeries.join(',')}.`)
    }
  },
}

export const UserDetailCompact: Story = {
  render: () => <UserDetailPageCanvas />,
  parameters: {
    viewport: { defaultViewport: '0390-device-iphone-14' },
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 80))

    const mainContent = canvasElement.querySelector<HTMLElement>('.admin-main-content')
    if (!mainContent?.classList.contains('is-compact-layout')) {
      throw new Error('Expected the compact user detail story to activate the compact admin layout.')
    }

    const tokenCards = Array.from(canvasElement.querySelectorAll<HTMLElement>('.admin-user-token-card'))
    const breakdownCards = Array.from(canvasElement.querySelectorAll<HTMLElement>('.admin-user-breakdown-card'))
    if (tokenCards.length === 0 || breakdownCards.length === 0) {
      throw new Error('Expected the compact user detail story to render both token cards and quota breakdown cards.')
    }
    if (
      tokenCards.some((card) => !card.querySelector('.admin-user-mobile-metric-grid')) ||
      breakdownCards.some((card) => !card.querySelector('.admin-user-mobile-metric-grid'))
    ) {
      throw new Error('Expected compact user detail cards to render the denser metric-grid layout.')
    }

    const desktopTokenWrapper = canvasElement.querySelector('.admin-user-tokens-table')?.closest<HTMLElement>('.admin-responsive-up')
    const desktopBreakdownWrapper = canvasElement.querySelector('.user-tag-breakdown-table')?.closest<HTMLElement>('.admin-responsive-up')
    if (
      (desktopTokenWrapper && getComputedStyle(desktopTokenWrapper).display !== 'none') ||
      (desktopBreakdownWrapper && getComputedStyle(desktopBreakdownWrapper).display !== 'none')
    ) {
      throw new Error('Expected compact user detail story to hide desktop tables.')
    }

    for (const [label, cards] of [
      ['token', tokenCards],
      ['quota breakdown', breakdownCards],
    ] as const) {
      if (cards.some((card) => card.scrollWidth > card.clientWidth + 1)) {
        throw new Error(`Expected compact ${label} cards to avoid horizontal overflow.`)
      }
    }
  },
}

export const UserDetailMonthlyGap: Story = {
  render: () => <UserDetailPageCanvas initialUsageSeries="quotaMonth" />,
}

export const Alerts: Story = {
  render: () => <AlertsPageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const SystemSettings: Story = {
  render: () => <SystemSettingsPageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 80))
    const activeNavItem = canvasElement.ownerDocument.querySelector<HTMLElement>('.admin-nav-item-active')
    if (!activeNavItem) {
      throw new Error('Expected system settings page to mark the matching nav item as active.')
    }
    if (!activeNavItem.textContent?.includes('系统设置')) {
      throw new Error('Expected active nav item to remain on system settings.')
    }
    const navIcon = activeNavItem.querySelector<SVGElement>('.admin-nav-item-icon svg')
    if (!navIcon) {
      throw new Error('Expected system settings nav item to render its bundled SVG icon.')
    }
  },
}

export const ProxySettings: Story = {
  render: () => <ProxySettingsPageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}
