import { Icon } from '../lib/icons'
import type { Meta, StoryObj } from '@storybook/react-vite'
import { addons } from 'storybook/preview-api'
import { SELECT_STORY } from 'storybook/internal/core-events'
import { Fragment, type ReactNode, useEffect, useLayoutEffect, useState } from 'react'

import type {
  AdminUserDetail,
  AdminUserSummary,
  AdminUserTag,
  AdminUserTagBinding,
  AdminUserTokenSummary,
  ApiKeyStats,
  AuthToken,
  JobLogView,
  RequestLog,
} from '../api'
import AdminPanelHeader from '../components/AdminPanelHeader'
import AdminTablePagination from '../components/AdminTablePagination'
import JobKeyLink from '../components/JobKeyLink'
import QuotaRangeField from '../components/QuotaRangeField'
import { StatusBadge, type StatusTone } from '../components/StatusBadge'
import SegmentedTabs from '../components/ui/SegmentedTabs'
import { Button } from '../components/ui/button'
import {
  Drawer,
  DrawerContent,
} from '../components/ui/drawer'
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
import { Card } from '../components/ui/card'
import { Badge } from '../components/ui/badge'
import { Switch } from '../components/ui/switch'
import { LanguageProvider, useLanguage, useTranslate, type AdminTranslations } from '../i18n'
import { KeyDetails } from '../AdminDashboard'
import { TokenDetailStoryCanvas } from '../pages/TokenDetail.stories'

import AdminShell, { type AdminNavItem } from './AdminShell'
import DashboardOverview, { type DashboardMetricCard } from './DashboardOverview'
import ForwardProxySettingsModule from './ForwardProxySettingsModule'
import ModulePlaceholder from './ModulePlaceholder'
import {
  forwardProxyStorySavedAt,
  forwardProxyStorySettings,
  forwardProxyStoryStats,
} from './forwardProxyStoryData'
import {
  stickyNodesStoryData,
  stickyUsersStoryData,
  stickyUsersStoryTotal,
} from './keyStickyStoryData'
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
import type { AdminModuleId } from './routes'

const now = 1_762_380_000

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

const tableSecondaryFieldStyle = {
  ...tableFieldStyle,
  fontSize: '0.92em',
  opacity: 0.68,
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
    result_status: 'success',
    created_at: now - 20,
    error_message: null,
    key_effect_code: 'none',
    key_effect_summary: 'No automatic key state change',
    request_body: '{"query":"tavily observability"}',
    response_body: '{"status":200}',
    forwarded_headers: ['x-request-id', 'x-forwarded-for'],
    dropped_headers: ['authorization'],
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
    result_status: 'quota_exhausted',
    created_at: now - 118,
    error_message: 'Quota exhausted for this API key',
    key_effect_code: 'marked_exhausted',
    key_effect_summary: 'Automatically marked this key as exhausted',
    request_body: '{"query":"site reliability playbook"}',
    response_body: '{"status":432}',
    forwarded_headers: ['x-request-id'],
    dropped_headers: ['cookie'],
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
  },
]

const MOCK_JOBS: JobLogView[] = [
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
    tags: MOCK_ALICE_TAGS,
    hourlyAnyUsed: 312,
    hourlyAnyLimit: 1_770,
    quotaHourlyUsed: 298,
    quotaHourlyLimit: 1_200,
    quotaDailyUsed: 5_201,
    quotaDailyLimit: 25_500,
    quotaMonthlyUsed: 142_922,
    quotaMonthlyLimit: 5_000,
    dailySuccess: 4_998,
    dailyFailure: 203,
    monthlySuccess: 129_442,
    lastActivity: now - 25,
  },
  {
    userId: 'usr_bob',
    displayName: 'Bob Chen',
    username: 'bob',
    active: true,
    lastLoginAt: now - 2_700,
    tokenCount: 1,
    tags: MOCK_BOB_TAGS,
    hourlyAnyUsed: 611,
    hourlyAnyLimit: 0,
    quotaHourlyUsed: 602,
    quotaHourlyLimit: 0,
    quotaDailyUsed: 10_009,
    quotaDailyLimit: 0,
    quotaMonthlyUsed: 231_008,
    quotaMonthlyLimit: 0,
    dailySuccess: 9_800,
    dailyFailure: 209,
    monthlySuccess: 201_402,
    lastActivity: now - 38,
  },
  {
    userId: 'usr_charlie',
    displayName: 'Charlie Li',
    username: 'charlie',
    active: false,
    lastLoginAt: now - 86_400 * 6,
    tokenCount: 0,
    tags: [],
    hourlyAnyUsed: 0,
    hourlyAnyLimit: 600,
    quotaHourlyUsed: 0,
    quotaHourlyLimit: 500,
    quotaDailyUsed: 0,
    quotaDailyLimit: 8_000,
    quotaMonthlyUsed: 0,
    quotaMonthlyLimit: 96_000,
    dailySuccess: 0,
    dailyFailure: 0,
    monthlySuccess: 122,
    lastActivity: null,
  },
]

const MOCK_USER_TOKENS: AdminUserTokenSummary[] = [
  {
    tokenId: 'V3P2',
    enabled: true,
    note: 'Primary production',
    lastUsedAt: now - 24,
    hourlyAnyUsed: 188,
    hourlyAnyLimit: 600,
    quotaHourlyUsed: 180,
    quotaHourlyLimit: 500,
    quotaDailyUsed: 2_840,
    quotaDailyLimit: 12_000,
    quotaMonthlyUsed: 42_000,
    quotaMonthlyLimit: 160_000,
    dailySuccess: 2_701,
    dailyFailure: 139,
    monthlySuccess: 39_420,
  },
  {
    tokenId: 'R8K1',
    enabled: true,
    note: 'Batch backfill',
    lastUsedAt: now - 400,
    hourlyAnyUsed: 124,
    hourlyAnyLimit: 600,
    quotaHourlyUsed: 118,
    quotaHourlyLimit: 500,
    quotaDailyUsed: 2_361,
    quotaDailyLimit: 12_000,
    quotaMonthlyUsed: 100_922,
    quotaMonthlyLimit: 440_000,
    dailySuccess: 2_297,
    dailyFailure: 64,
    monthlySuccess: 90_022,
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

const numberFormatter = new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 })
const percentFormatter = new Intl.NumberFormat('en-US', { style: 'percent', minimumFractionDigits: 1, maximumFractionDigits: 1 })
const dateTimeFormatter = new Intl.DateTimeFormat(undefined, {
  month: 'short',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit',
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

function clampDisplayedQuota(value: number): number {
  return Math.max(0, value)
}

function formatQuotaLimitValue(value: number): string {
  return formatNumber(clampDisplayedQuota(value))
}

function formatQuotaUsagePair(used: number, limit: number): string {
  return `${formatNumber(Math.max(0, used))} / ${formatQuotaLimitValue(limit)}`
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
              [users.quota.hourlyAny, draft.hourlyAnyDelta],
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
    { module: 'dashboard', label: strings.nav.dashboard, icon: 'mdi:view-dashboard-outline' },
    { module: 'tokens', label: strings.nav.tokens, icon: 'mdi:key-chain-variant' },
    { module: 'keys', label: strings.nav.keys, icon: 'mdi:key-outline' },
    { module: 'requests', label: strings.nav.requests, icon: 'mdi:file-document-outline' },
    { module: 'jobs', label: strings.nav.jobs, icon: 'mdi:calendar-clock-outline' },
    { module: 'users', label: strings.nav.users, icon: 'mdi:account-group-outline' },
    { module: 'alerts', label: strings.nav.alerts, icon: 'mdi:bell-ring-outline' },
    { module: 'proxy-settings', label: strings.nav.proxySettings, icon: 'mdi:tune-variant' },
  ]
}

interface AdminPageFrameProps {
  activeModule: AdminModuleId
  children: ReactNode
}

function AdminPageFrame({ activeModule, children }: AdminPageFrameProps): JSX.Element {
  const admin = useTranslate().admin

  return (
    <AdminShell
      activeModule={activeModule}
      navItems={buildNavItems(admin)}
      skipToContentLabel={admin.accessibility.skipToContent}
      onSelectModule={() => {}}
    >
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
      {children}
    </AdminShell>
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

  const todayMetrics: DashboardMetricCard[] = [
    {
      id: 'today-total',
      label: admin.metrics.labels.total,
      value: formatNumber(totalRequests),
      subtitle: admin.dashboard.asOfNow,
      comparison: {
        label: admin.dashboard.deltaFromYesterday,
        value: '+128 (+6.1%)',
        direction: 'up',
      },
    },
    {
      id: 'today-success',
      label: admin.metrics.labels.success,
      value: formatNumber(successCount),
      subtitle: `${admin.dashboard.todayShare} · ${formatPercent(successCount, totalRequests)}`,
      comparison: {
        label: admin.dashboard.deltaFromYesterday,
        value: '+96 (+5.4%)',
        direction: 'up',
      },
    },
    {
      id: 'today-errors',
      label: admin.metrics.labels.errors,
      value: formatNumber(errorCount),
      subtitle: `${admin.dashboard.todayShare} · ${formatPercent(errorCount, totalRequests)}`,
      comparison: {
        label: admin.dashboard.deltaFromYesterday,
        value: '-12 (-9.8%)',
        direction: 'down',
      },
    },
    {
      id: 'today-quota',
      label: admin.metrics.labels.quota,
      value: formatNumber(quotaExhaustedCount),
      subtitle: `${admin.dashboard.todayShare} · ${formatPercent(quotaExhaustedCount, totalRequests)}`,
      comparison: {
        label: admin.dashboard.deltaFromYesterday,
        value: '+4 (+12.5%)',
        direction: 'up',
      },
    },
  ]

  const monthMetrics: DashboardMetricCard[] = [
    {
      id: 'month-total',
      label: admin.metrics.labels.total,
      value: formatNumber(totalRequests * 14),
      subtitle: admin.dashboard.monthToDate,
    },
    {
      id: 'month-success',
      label: admin.metrics.labels.success,
      value: formatNumber(successCount * 14),
      subtitle: `${admin.dashboard.monthShare} · ${formatPercent(successCount, totalRequests)}`,
    },
    {
      id: 'month-errors',
      label: admin.metrics.labels.errors,
      value: formatNumber(errorCount * 14),
      subtitle: `${admin.dashboard.monthShare} · ${formatPercent(errorCount, totalRequests)}`,
    },
    {
      id: 'month-quota',
      label: admin.metrics.labels.quota,
      value: formatNumber(quotaExhaustedCount * 14),
      subtitle: `${admin.dashboard.monthShare} · ${formatPercent(quotaExhaustedCount, totalRequests)}`,
    },
  ]

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
        trend={{
          request: [86, 94, 101, 112, 97, 121, 133, 126],
          error: [3, 5, 4, 8, 7, 6, 9, 5],
        }}
        tokenCoverage="truncated"
        tokens={MOCK_TOKENS}
        keys={MOCK_KEYS}
        logs={MOCK_REQUESTS}
        jobs={MOCK_JOBS}
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

        <div className="table-wrapper jobs-table-wrapper">
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
}: {
  initialRegistrationIp?: string
  initialRegions?: string[]
} = {}): JSX.Element {
  const admin = useTranslate().admin
  const keyStrings = admin.keys
  const [selectedGroups, setSelectedGroups] = useState<string[]>([])
  const [selectedStatuses, setSelectedStatuses] = useState<string[]>([])
  const [selectedRegistrationIp, setSelectedRegistrationIp] = useState(initialRegistrationIp)
  const [selectedRegions, setSelectedRegions] = useState<string[]>(initialRegions)
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

        <div className="table-wrapper jobs-table-wrapper">
          <table className="jobs-table">
            <thead>
              <tr>
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
                      <span style={tableSecondaryFieldStyle}>{formatKeyGroupName(item.group, keyStrings.groups.ungrouped)}</span>
                    </div>
                  </td>
                  <td>
                    <div style={tableStackStyle}>
                      <span style={tableFieldStyle}>{formatRegistrationValue(item.registration_ip)}</span>
                      <span style={tableSecondaryFieldStyle}>
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
                      <span style={tableFieldStyle}>{formatNumber(item.success_count)}</span>
                      <span style={tableSecondaryFieldStyle}>{formatNumber(item.error_count)}</span>
                    </div>
                  </td>
                  <td>
                    <span style={tableFieldStyle}>
                      {item.quota_remaining != null && item.quota_limit != null
                        ? `${formatNumber(item.quota_remaining)} / ${formatNumber(item.quota_limit)}`
                        : '—'}
                    </span>
                  </td>
                  <td>
                    <div style={tableStackStyle}>
                      <span style={tableFieldStyle}>{formatTimestamp(item.last_used_at)}</span>
                      <span style={tableSecondaryFieldStyle}>{formatTimestamp(item.status_changed_at)}</span>
                    </div>
                  </td>
                  <td>
                    <div className="table-actions" style={{ flexWrap: 'nowrap' }}>
                      <button type="button" className="btn btn-circle btn-ghost btn-sm" aria-label={keyStrings.actions.disable}>
                        P
                      </button>
                      <button type="button" className="btn btn-circle btn-ghost btn-sm" aria-label={keyStrings.actions.delete}>
                        D
                      </button>
                      <button type="button" className="btn btn-circle btn-ghost btn-sm" aria-label={keyStrings.actions.details}>
                        V
                      </button>
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

function RequestsPageCanvas(): JSX.Element {
  const admin = useTranslate().admin
  const { language } = useLanguage()
  const logStrings = admin.logs
  const logDetailsStrings = admin.logDetails
  const [expandedLogs, setExpandedLogs] = useState<Set<number>>(() => new Set([9499, 9498]))
  const [drawerTarget, setDrawerTarget] = useState<{ kind: 'key' | 'token'; id: string } | null>(null)

  const toggleLog = (id: number) => {
    setExpandedLogs((prev) => {
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
    <AdminPageFrame activeModule="requests">
      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{logStrings.title}</h2>
            <p className="panel-description">{logStrings.description}</p>
          </div>
          <div className="panel-actions">
            <SegmentedTabs<'all' | 'success' | 'error' | 'quota_exhausted'>
              value="all"
              onChange={() => {}}
              options={[
                { value: 'all', label: logStrings.filters.all },
                { value: 'success', label: logStrings.filters.success },
                { value: 'error', label: logStrings.filters.error },
                { value: 'quota_exhausted', label: logStrings.filters.quota },
              ]}
              ariaLabel={logStrings.title}
            />
          </div>
        </div>

        <div className="table-wrapper jobs-table-wrapper">
          <table className="admin-logs-table admin-request-logs-table">
            <thead>
              <tr>
                <th>{logStrings.table.time}</th>
                <th>{logStrings.table.key}</th>
                <th>{logStrings.table.token}</th>
                <th>Status</th>
                <th>{logStrings.table.result}</th>
                <th>{logStrings.table.keyEffect}</th>
                <th>{logStrings.table.error}</th>
              </tr>
            </thead>
            <tbody>
              {MOCK_REQUESTS.map((log) => {
                const expanded = expandedLogs.has(log.id)
                const errorText = requestErrorText(log, admin)
                const keyEffectText = requestKeyEffectText(log, admin)
                const guidance = requestFailureGuidance(log.failure_kind, language)
                const hasDetails =
                  errorText !== logStrings.errors.none ||
                  keyEffectText !== logDetailsStrings.noKeyEffect ||
                  guidance != null

                return (
                  <Fragment key={log.id}>
                    <tr>
                      <td>{formatTimestamp(log.created_at)}</td>
                      <td>
                        <button
                          type="button"
                          className="log-key-pill request-entity-button"
                          onClick={() => setDrawerTarget({ kind: 'key', id: log.key_id })}
                        >
                          <code>{log.key_id}</code>
                        </button>
                      </td>
                      <td>
                        {log.auth_token_id ? (
                          <button
                            type="button"
                            className="link-button log-token-link request-entity-button"
                            onClick={() => setDrawerTarget({ kind: 'token', id: log.auth_token_id! })}
                          >
                            <code>{log.auth_token_id}</code>
                          </button>
                        ) : (
                          <code>—</code>
                        )}
                      </td>
                      <td>
                        <span className="tooltip" data-tip={requestStatusTip(log, admin)}>
                          <button
                            type="button"
                            className="status-pair-trigger"
                            aria-label={requestStatusTip(log, admin)}
                          >
                            {requestStatusPair(log)}
                          </button>
                        </span>
                      </td>
                      <td>
                        <StatusBadge tone={logResultTone(log.result_status)}>
                          {admin.statuses[log.result_status] ?? log.result_status}
                        </StatusBadge>
                      </td>
                      <td>
                        <StatusBadge
                          tone={requestKeyEffectTone(log.key_effect_code)}
                          title={keyEffectText}
                        >
                          {requestKeyEffectBadgeLabel(log, admin)}
                        </StatusBadge>
                      </td>
                      <td>
                        {hasDetails ? (
                          <button
                            type="button"
                            className={`jobs-message-button${expanded ? ' jobs-message-button-active' : ''}`}
                            onClick={() => toggleLog(log.id)}
                            aria-expanded={expanded}
                            aria-controls={`storybook-log-details-${log.id}`}
                          >
                            <span className="jobs-message-text">{errorText}</span>
                            <Icon
                              icon={expanded ? 'mdi:chevron-up' : 'mdi:chevron-down'}
                              width={16}
                              height={16}
                              className="jobs-message-icon"
                              aria-hidden="true"
                            />
                          </button>
                        ) : (
                          errorText
                        )}
                      </td>
                    </tr>
                    {expanded && (
                      <tr className="log-details-row">
                        <td colSpan={7} id={`storybook-log-details-${log.id}`}>
                          <div className="log-details-panel">
                            <div className="log-details-summary">
                              <div>
                                <div className="log-details-label">{logStrings.table.time}</div>
                                <div className="log-details-value">{formatTimestamp(log.created_at)}</div>
                              </div>
                              <div>
                                <div className="log-details-label">{logStrings.table.result}</div>
                                <div className="log-details-value">{admin.statuses[log.result_status] ?? log.result_status}</div>
                              </div>
                              <div>
                                <div className="log-details-label">{logDetailsStrings.keyEffect}</div>
                                <div className="log-details-value">{keyEffectText}</div>
                              </div>
                              <div>
                                <div className="log-details-label">{logStrings.table.error}</div>
                                <div className="log-details-value">{errorText}</div>
                              </div>
                            </div>
                            {guidance ? (
                              <div className="log-details-block">
                                <div className="log-details-label">{logDetailsStrings.solution}</div>
                                <pre className="log-details-pre">{guidance}</pre>
                              </div>
                            ) : null}
                          </div>
                        </td>
                      </tr>
                    )}
                  </Fragment>
                )
              })}
            </tbody>
          </table>
        </div>

        <div className="table-pagination">
          <span className="panel-description">{logStrings.description} (1 / 4)</span>
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
  const [expandedJobs, setExpandedJobs] = useState<Set<number>>(() => new Set([608]))

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
            <SegmentedTabs<'all' | 'quota' | 'usage' | 'logs' | 'geo'>
              value="all"
              onChange={() => {}}
              options={[
                { value: 'all', label: jobsStrings.filters.all },
                { value: 'quota', label: jobsStrings.filters.quota },
                { value: 'usage', label: jobsStrings.filters.usage },
                { value: 'logs', label: jobsStrings.filters.logs },
                { value: 'geo', label: jobsStrings.filters.geo },
              ]}
              ariaLabel={jobsStrings.title}
            />
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
              {MOCK_JOBS.map((job) => {
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
  const users = admin.users
  const [query, setQuery] = useState('')
  const [allowRegistration, setAllowRegistration] = useState(true)
  const normalizedQuery = query.trim().toLowerCase()
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
            <input
              type="text"
              className="input input-bordered users-search-input"
              placeholder={users.searchPlaceholder}
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
            <button type="button" className="btn btn-outline">
              {users.search}
            </button>
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
                  <th>{users.table.tokenCount}</th>
                  <th>{users.table.tags}</th>
                  <th>{users.table.hourlyAny}</th>
                  <th>{users.table.hourly}</th>
                  <th>{users.table.daily}</th>
                  <th>{users.table.monthly}</th>
                  <th>{users.table.successDaily}</th>
                  <th>{users.table.successMonthly}</th>
                  <th>{users.table.lastActivity}</th>
                  <th>{users.table.lastLogin}</th>
                  <th>{users.table.actions}</th>
                </tr>
              </thead>
              <tbody>
                {filteredUsers.map((item) => (
                  <tr key={item.userId}>
                    <td>
                      <button type="button" className="link-button">
                        <strong>{item.displayName || item.username || item.userId}</strong>
                      </button>
                      <div className="panel-description" style={{ marginTop: 4 }}>
                        <code>{item.userId}</code>
                        {item.username ? ` · @${item.username}` : ''}
                      </div>
                    </td>
                    <td>
                      <StatusBadge tone={item.active ? 'success' : 'neutral'}>
                        {item.active ? users.status.active : users.status.inactive}
                      </StatusBadge>
                    </td>
                    <td>{formatNumber(item.tokenCount)}</td>
                    <td>
                      <StoryUserTagBadgeList tags={item.tags} users={users} emptyLabel={users.userTags.empty} />
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
                      <button type="button" className="btn btn-circle btn-ghost btn-sm" aria-label={users.actions.view}>
                        <Icon icon="mdi:open-in-new" width={16} height={16} />
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </section>
    </AdminPageFrame>
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

function UserDetailPageCanvas(): JSX.Element {
  const users = useTranslate().admin.users
  const detail = MOCK_USER_DETAIL
  const quotaSnapshot = buildStoryQuotaSnapshot(detail)
  const [quotaDraft, setQuotaDraft] = useState<Record<QuotaSliderField, string>>({
    hourlyAnyLimit: String(detail.quotaBase.hourlyAnyLimit),
    hourlyLimit: String(detail.quotaBase.hourlyLimit),
    dailyLimit: String(detail.quotaBase.dailyLimit),
    monthlyLimit: String(detail.quotaBase.monthlyLimit),
  })
  const hasBlockAllTag = detail.tags.some((tag) => tag.effectKind === 'block_all')

  return (
    <AdminPageFrame activeModule="users">
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
                    <span className="token-compact-label">{users.quota.hourlyAny}</span>
                    <span className="token-compact-value">{formatSignedQuotaDelta(tag.hourlyAnyDelta)}</span>
                  </div>
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
              field: 'hourlyAnyLimit',
              label: users.quota.hourlyAny,
              used: detail.hourlyAnyUsed,
              currentLimit: detail.quotaBase.hourlyAnyLimit,
            },
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
            ['hourlyAny', users.quota.hourlyAny, detail.effectiveQuota.hourlyAnyLimit],
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
        <div className="table-wrapper jobs-table-wrapper" style={{ marginTop: 12 }}>
          <table className="jobs-table admin-users-table user-tag-breakdown-table">
            <thead>
              <tr>
                <th>{users.effectiveQuota.columns.item}</th>
                <th>{users.effectiveQuota.columns.source}</th>
                <th>{users.effectiveQuota.columns.effect}</th>
                <th>{users.quota.hourlyAny}</th>
                <th>{users.quota.hourly}</th>
                <th>{users.quota.daily}</th>
                <th>{users.quota.monthly}</th>
              </tr>
            </thead>
            <tbody>
              {detail.quotaBreakdown.map((entry, index) => {
                const isAbsoluteRow = entry.kind === 'base' || entry.kind === 'effective'
                const breakdownLabel =
                  entry.kind === 'base'
                    ? users.effectiveQuota.baseLabel
                    : entry.kind === 'effective'
                      ? users.effectiveQuota.effectiveLabel
                      : entry.label
                const formatBreakdownValue = (value: number) =>
                  isAbsoluteRow ? formatQuotaLimitValue(value) : formatSignedQuotaDelta(value)
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
                    <td>
                      {entry.source
                        ? entry.source === 'system_linuxdo'
                          ? users.userTags.sourceSystem
                          : users.userTags.sourceManual
                        : '—'}
                    </td>
                    <td>
                      <StatusBadge tone={entry.effectKind === 'block_all' ? 'error' : 'neutral'}>
                        {entry.effectKind === 'block_all'
                          ? users.catalog.effectKinds.blockAll
                          : entry.effectKind === 'base'
                            ? users.effectiveQuota.baseLabel
                            : entry.kind === 'effective' || entry.effectKind === 'effective'
                              ? users.effectiveQuota.effectiveLabel
                              : users.catalog.effectKinds.quotaDelta}
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
            <h2>{users.detail.tokensTitle}</h2>
            <p className="panel-description">{users.detail.tokensDescription}</p>
          </div>
        </div>
        <div className="table-wrapper jobs-table-wrapper">
          <table className="jobs-table admin-users-table admin-user-tokens-table">
            <thead>
              <tr>
                <th>{`${users.tokens.table.id} · ${users.tokens.table.note}`}</th>
                <th>{`${users.tokens.table.status} · ${users.tokens.table.lastUsed}`}</th>
                <th>{`${users.tokens.table.hourlyAny} · ${users.tokens.table.hourly}`}</th>
                <th>{`${users.tokens.table.daily} · ${users.tokens.table.monthly}`}</th>
                <th>{`${users.tokens.table.successDaily} · ${users.tokens.table.successMonthly}`}</th>
                <th>{users.tokens.table.actions}</th>
              </tr>
            </thead>
            <tbody>
              {detail.tokens.map((token) => (
                <tr key={token.tokenId}>
                  <td>
                    <div className="token-compact-pair">
                      <div className="token-compact-field">
                        <span className="token-compact-value"><code>{token.tokenId}</code></span>
                      </div>
                      <div className="token-compact-field">
                        <span className="token-compact-value">{token.note ?? '—'}</span>
                      </div>
                    </div>
                  </td>
                  <td>
                    <div className="token-compact-pair">
                      <div className="token-compact-field">
                        <StatusBadge tone={token.enabled ? 'success' : 'neutral'}>
                          {token.enabled ? users.status.enabled : users.status.disabled}
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
                        <span className="token-compact-label">{users.tokens.table.hourlyAny}</span>
                        <span className="token-compact-value">{formatQuotaUsagePair(token.hourlyAnyUsed, token.hourlyAnyLimit)}</span>
                      </div>
                      <div className="token-compact-field">
                        <span className="token-compact-label">{users.tokens.table.hourly}</span>
                        <span className="token-compact-value">{formatQuotaUsagePair(token.quotaHourlyUsed, token.quotaHourlyLimit)}</span>
                      </div>
                    </div>
                  </td>
                  <td>
                    <div className="token-compact-pair">
                      <div className="token-compact-field">
                        <span className="token-compact-label">{users.tokens.table.daily}</span>
                        <span className="token-compact-value">{formatQuotaUsagePair(token.quotaDailyUsed, token.quotaDailyLimit)}</span>
                      </div>
                      <div className="token-compact-field">
                        <span className="token-compact-label">{users.tokens.table.monthly}</span>
                        <span className="token-compact-value">{formatQuotaUsagePair(token.quotaMonthlyUsed, token.quotaMonthlyLimit)}</span>
                      </div>
                    </div>
                  </td>
                  <td>
                    <div className="token-compact-pair">
                      <div className="token-compact-field">
                        <span className="token-compact-label">{users.tokens.table.successDaily}</span>
                        <span className="token-compact-value">{formatNumber(token.dailySuccess)} / {formatNumber(token.dailyFailure)}</span>
                      </div>
                      <div className="token-compact-field">
                        <span className="token-compact-label">{users.tokens.table.successMonthly}</span>
                        <span className="token-compact-value">{formatNumber(token.monthlySuccess)}</span>
                      </div>
                    </div>
                  </td>
                  <td>
                    <button type="button" className="btn btn-circle btn-ghost btn-sm" title={users.tokens.actions.view}>
                      <Icon icon="mdi:eye-outline" width={16} height={16} />
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </AdminPageFrame>
  )
}

function AlertsPageCanvas(): JSX.Element {
  const admin = useTranslate().admin
  return (
    <AdminPageFrame activeModule="alerts">
      <ModulePlaceholder
        title={admin.modules.alerts.title}
        description={admin.modules.alerts.description}
        sections={[
          admin.modules.alerts.sections.rules,
          admin.modules.alerts.sections.thresholds,
          admin.modules.alerts.sections.channels,
        ]}
        comingSoonLabel={admin.modules.comingSoon}
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

const meta = {
  title: 'Admin/Pages',
  parameters: {
    docs: {
      description: {
        component: [
          'Route-level admin review surface covering dashboard, keys, tokens, users, jobs, and forward proxy settings.',
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
}

export const Jobs: Story = {
  render: () => <JobsPageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const Users: Story = {
  render: () => <UsersPageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
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

export const UserDetail: Story = {
  render: () => <UserDetailPageCanvas />,
}

export const Alerts: Story = {
  render: () => <AlertsPageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const ProxySettings: Story = {
  render: () => <ProxySettingsPageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}
