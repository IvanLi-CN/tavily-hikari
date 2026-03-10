import { Icon } from '@iconify/react'
import type { Meta, StoryObj } from '@storybook/react-vite'
import { addons } from 'storybook/preview-api'
import { SELECT_STORY } from 'storybook/internal/core-events'
import { Fragment, type ReactNode, useState } from 'react'

import type {
  AdminUserDetail,
  AdminUserSummary,
  AdminUserTokenSummary,
  ApiKeyStats,
  AuthToken,
  JobLogView,
  RequestLog,
} from '../api'
import AdminPanelHeader from '../components/AdminPanelHeader'
import QuotaRangeField from '../components/QuotaRangeField'
import { StatusBadge, type StatusTone } from '../components/StatusBadge'
import SegmentedTabs from '../components/ui/SegmentedTabs'
import { useTranslate, type AdminTranslations } from '../i18n'

import AdminShell, { type AdminNavItem } from './AdminShell'
import DashboardOverview, { type DashboardMetricCard } from './DashboardOverview'
import ModulePlaceholder from './ModulePlaceholder'
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
  },
  {
    id: 'asR8',
    status: 'exhausted',
    group: 'production',
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
  },
  {
    id: 'U2vK',
    status: 'active',
    group: 'batch',
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
  },
  {
    id: 'c7Pk',
    status: 'disabled',
    group: 'ops',
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
  },
  {
    id: 'J1nW',
    status: 'active',
    group: 'ops',
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
  },
]

const MOCK_REQUESTS: RequestLog[] = [
  {
    id: 9501,
    key_id: 'MZli',
    auth_token_id: '9vsN',
    method: 'POST',
    path: '/mcp',
    query: null,
    http_status: 200,
    mcp_status: 0,
    result_status: 'success',
    created_at: now - 20,
    error_message: null,
    request_body: '{"tool":"search"}',
    response_body: '{"ok":true}',
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
    http_status: 429,
    mcp_status: -1,
    result_status: 'quota_exhausted',
    created_at: now - 74,
    error_message: 'Upstream quota exhausted',
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
    path: '/mcp',
    query: null,
    http_status: 502,
    mcp_status: -32000,
    result_status: 'error',
    created_at: now - 118,
    error_message: 'Bad gateway from upstream',
    request_body: '{"tool":"extract"}',
    response_body: null,
    forwarded_headers: ['x-request-id'],
    dropped_headers: ['cookie'],
  },
  {
    id: 9498,
    key_id: 'MZli',
    auth_token_id: 'Q4sE',
    method: 'POST',
    path: '/mcp',
    query: null,
    http_status: 200,
    mcp_status: 0,
    result_status: 'success',
    created_at: now - 196,
    error_message: null,
    request_body: '{"tool":"map"}',
    response_body: '{"ok":true}',
    forwarded_headers: ['x-request-id'],
    dropped_headers: [],
  },
  {
    id: 9497,
    key_id: 'J1nW',
    auth_token_id: '9vsN',
    method: 'POST',
    path: '/mcp',
    query: null,
    http_status: 200,
    mcp_status: 0,
    result_status: 'success',
    created_at: now - 310,
    error_message: null,
    request_body: '{"tool":"search"}',
    response_body: '{"ok":true}',
    forwarded_headers: ['x-request-id'],
    dropped_headers: [],
  },
]

const MOCK_JOBS: JobLogView[] = [
  {
    id: 610,
    job_type: 'quota_sync',
    key_id: 'MZli',
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
    status: 'success',
    attempt: 1,
    message: 'Pruned 1,260 old log rows',
    started_at: now - 3_200,
    finished_at: now - 3_090,
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
    hourlyAnyUsed: 312,
    hourlyAnyLimit: 1_200,
    quotaHourlyUsed: 298,
    quotaHourlyLimit: 1_000,
    quotaDailyUsed: 5_201,
    quotaDailyLimit: 24_000,
    quotaMonthlyUsed: 142_922,
    quotaMonthlyLimit: 600_000,
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
    hourlyAnyUsed: 611,
    hourlyAnyLimit: 1_200,
    quotaHourlyUsed: 602,
    quotaHourlyLimit: 1_000,
    quotaDailyUsed: 10_009,
    quotaDailyLimit: 24_000,
    quotaMonthlyUsed: 231_008,
    quotaMonthlyLimit: 600_000,
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
    hourlyAnyUsed: 0,
    hourlyAnyLimit: 600,
    quotaHourlyUsed: 0,
    quotaHourlyLimit: 500,
    quotaDailyUsed: 0,
    quotaDailyLimit: 8_000,
    quotaMonthlyUsed: 0,
    quotaMonthlyLimit: 120_000,
    dailySuccess: 0,
    dailyFailure: 0,
    monthlySuccess: 0,
    lastActivity: null,
  },
]

const MOCK_USER_TOKENS: AdminUserTokenSummary[] = [
  {
    tokenId: '9vsN',
    enabled: true,
    note: 'Core production',
    lastUsedAt: now - 19,
    hourlyAnyUsed: 176,
    hourlyAnyLimit: 600,
    quotaHourlyUsed: 162,
    quotaHourlyLimit: 500,
    quotaDailyUsed: 2_811,
    quotaDailyLimit: 12_000,
    quotaMonthlyUsed: 73_102,
    quotaMonthlyLimit: 300_000,
    dailySuccess: 2_704,
    dailyFailure: 107,
    monthlySuccess: 66_914,
  },
  {
    tokenId: 'Vn7D',
    enabled: true,
    note: 'Realtime recommendation',
    lastUsedAt: now - 42,
    hourlyAnyUsed: 136,
    hourlyAnyLimit: 600,
    quotaHourlyUsed: 136,
    quotaHourlyLimit: 500,
    quotaDailyUsed: 2_390,
    quotaDailyLimit: 12_000,
    quotaMonthlyUsed: 69_820,
    quotaMonthlyLimit: 300_000,
    dailySuccess: 2_294,
    dailyFailure: 96,
    monthlySuccess: 62_528,
  },
]

const MOCK_USER_DETAIL: AdminUserDetail = {
  ...MOCK_USERS[0],
  hourlyAnyUsed: 340,
  hourlyAnyLimit: 500,
  quotaHourlyUsed: 134,
  quotaHourlyLimit: 262,
  quotaDailyUsed: 528,
  quotaDailyLimit: 1_022,
  quotaMonthlyUsed: 528,
  quotaMonthlyLimit: 5_000,
  tokens: MOCK_USER_TOKENS,
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

type StoryQuotaSnapshot = Record<QuotaSliderField, QuotaSliderSeed>

function buildStoryQuotaSnapshot(detail: AdminUserDetail): StoryQuotaSnapshot {
  return {
    hourlyAnyLimit: createQuotaSliderSeed('hourlyAnyLimit', detail.hourlyAnyUsed, detail.hourlyAnyLimit),
    hourlyLimit: createQuotaSliderSeed('hourlyLimit', detail.quotaHourlyUsed, detail.quotaHourlyLimit),
    dailyLimit: createQuotaSliderSeed('dailyLimit', detail.quotaDailyUsed, detail.quotaDailyLimit),
    monthlyLimit: createQuotaSliderSeed('monthlyLimit', detail.quotaMonthlyUsed, detail.quotaMonthlyLimit),
  }
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

  const metrics: DashboardMetricCard[] = [
    {
      id: 'total',
      label: admin.metrics.labels.total,
      value: formatNumber(totalRequests),
      subtitle: '—',
    },
    {
      id: 'success',
      label: admin.metrics.labels.success,
      value: formatNumber(successCount),
      subtitle: formatPercent(successCount, totalRequests),
    },
    {
      id: 'errors',
      label: admin.metrics.labels.errors,
      value: formatNumber(errorCount),
      subtitle: formatPercent(errorCount, totalRequests),
    },
    {
      id: 'quota',
      label: admin.metrics.labels.quota,
      value: formatNumber(quotaExhaustedCount),
      subtitle: formatPercent(quotaExhaustedCount, totalRequests),
    },
    {
      id: 'remaining',
      label: admin.metrics.labels.remaining,
      value: `${formatNumber(totalQuotaRemaining)} / ${formatNumber(totalQuotaLimit)}`,
      subtitle: formatPercent(totalQuotaRemaining, totalQuotaLimit),
    },
    {
      id: 'keys',
      label: admin.metrics.labels.keys,
      value: `${formatNumber(activeKeys)} / ${formatNumber(MOCK_KEYS.length)}`,
      subtitle: admin.metrics.subtitles.keysExhausted.replace('{count}', String(exhaustedKeys)),
    },
  ]

  return (
    <AdminPageFrame activeModule="dashboard">
      <DashboardOverview
        strings={admin.dashboard}
        overviewReady
        metrics={metrics}
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

function KeysPageCanvas(): JSX.Element {
  const admin = useTranslate().admin
  const keyStrings = admin.keys

  return (
    <AdminPageFrame activeModule="keys">
      <section className="surface panel">
        <div className="panel-header" style={{ flexWrap: 'wrap', gap: 12, alignItems: 'flex-start' }}>
          <div style={{ flex: '1 1 320px', minWidth: 240 }}>
            <h2>{keyStrings.title}</h2>
            <p className="panel-description">{keyStrings.description}</p>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap', marginLeft: 'auto' }}>
            <input type="text" className="input input-bordered" readOnly value="tvly-prod-******" aria-label={keyStrings.placeholder} />
            <button type="button" className="btn btn-primary">
              {keyStrings.addButton}
            </button>
          </div>
        </div>

        <div className="token-groups-container">
          <div className="token-groups-label">
            <span>{keyStrings.groups.label}</span>
          </div>
          <div className="token-groups-row">
            <div className="token-groups-list token-groups-list-expanded">
              <button type="button" className="token-group-chip token-group-chip-active">
                <span className="token-group-name">{keyStrings.groups.all}</span>
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
          <table className="jobs-table">
            <thead>
              <tr>
                <th>{keyStrings.table.keyId}</th>
                <th>{keyStrings.table.status}</th>
                <th>{keyStrings.table.total}</th>
                <th>{keyStrings.table.success}</th>
                <th>{keyStrings.table.errors}</th>
                <th>{keyStrings.table.quotaLeft}</th>
                <th>{keyStrings.table.lastUsed}</th>
                <th>{keyStrings.table.statusChanged}</th>
                <th>{keyStrings.table.actions}</th>
              </tr>
            </thead>
            <tbody>
              {MOCK_KEYS.map((item) => (
                <tr key={item.id}>
                  <td>
                    <code>{item.id}</code>
                  </td>
                  <td>
                    <StatusBadge tone={keyStatusTone(item.status)}>{admin.statuses[item.status] ?? item.status}</StatusBadge>
                  </td>
                  <td>{formatNumber(item.total_requests)}</td>
                  <td>{formatNumber(item.success_count)}</td>
                  <td>{formatNumber(item.error_count)}</td>
                  <td>
                    {item.quota_remaining != null && item.quota_limit != null
                      ? `${formatNumber(item.quota_remaining)} / ${formatNumber(item.quota_limit)}`
                      : '—'}
                  </td>
                  <td>{formatTimestamp(item.last_used_at)}</td>
                  <td>{formatTimestamp(item.status_changed_at)}</td>
                  <td>
                    <div className="table-actions">
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
      </section>
    </AdminPageFrame>
  )
}

function RequestsPageCanvas(): JSX.Element {
  const admin = useTranslate().admin
  const logStrings = admin.logs
  const [expandedLogs, setExpandedLogs] = useState<Set<number>>(() => new Set([9499]))

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
          <table className="admin-logs-table">
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
              {MOCK_REQUESTS.map((log) => {
                const expanded = expandedLogs.has(log.id)
                const errorText = requestErrorText(log, admin)
                const hasDetails = errorText !== logStrings.errors.none

                return (
                  <Fragment key={log.id}>
                    <tr>
                      <td>{formatTimestamp(log.created_at)}</td>
                      <td>
                        <code>{log.key_id}</code>
                      </td>
                      <td>
                        <code>{log.auth_token_id ?? '—'}</code>
                      </td>
                      <td>{log.http_status ?? '—'}</td>
                      <td>{log.mcp_status ?? '—'}</td>
                      <td>
                        <StatusBadge tone={logResultTone(log.result_status)}>
                          {admin.statuses[log.result_status] ?? log.result_status}
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
                                <div className="log-details-label">{logStrings.table.error}</div>
                                <div className="log-details-value">{errorText}</div>
                              </div>
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
    </AdminPageFrame>
  )
}

function JobsPageCanvas(): JSX.Element {
  const admin = useTranslate().admin
  const jobsStrings = admin.jobs
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
            <SegmentedTabs<'all' | 'quota' | 'usage' | 'logs'>
              value="all"
              onChange={() => {}}
              options={[
                { value: 'all', label: jobsStrings.filters.all },
                { value: 'quota', label: jobsStrings.filters.quota },
                { value: 'usage', label: jobsStrings.filters.usage },
                { value: 'logs', label: jobsStrings.filters.logs },
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
                      <td>{job.key_id ? <code>{job.key_id}</code> : '—'}</td>
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
  const normalizedQuery = query.trim().toLowerCase()
  const filteredUsers = MOCK_USERS.filter((item) => {
    if (!normalizedQuery) return true
    const displayName = item.displayName?.toLowerCase() ?? ''
    const username = item.username?.toLowerCase() ?? ''
    return (
      item.userId.toLowerCase().includes(normalizedQuery) ||
      displayName.includes(normalizedQuery) ||
      username.includes(normalizedQuery)
    )
  })

  return (
    <AdminPageFrame activeModule="users">
      <section className="surface panel">
        <div className="panel-header" style={{ gap: 12, flexWrap: 'wrap' }}>
          <div>
            <h2>{users.title}</h2>
            <p className="panel-description">{users.description}</p>
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
                      <strong>{item.displayName || item.username || item.userId}</strong>
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
                      {formatNumber(item.hourlyAnyUsed)} / {formatNumber(item.hourlyAnyLimit)}
                    </td>
                    <td>
                      {formatNumber(item.quotaHourlyUsed)} / {formatNumber(item.quotaHourlyLimit)}
                    </td>
                    <td>
                      {formatNumber(item.quotaDailyUsed)} / {formatNumber(item.quotaDailyLimit)}
                    </td>
                    <td>
                      {formatNumber(item.quotaMonthlyUsed)} / {formatNumber(item.quotaMonthlyLimit)}
                    </td>
                    <td>
                      {formatNumber(item.dailySuccess)} / {formatNumber(item.dailyFailure)}
                    </td>
                    <td>{formatNumber(item.monthlySuccess)}</td>
                    <td>{formatTimestamp(item.lastActivity)}</td>
                    <td>{formatTimestamp(item.lastLoginAt)}</td>
                    <td>
                      <button
                        type="button"
                        className="btn btn-circle btn-ghost btn-sm"
                        title={users.actions.view}
                        aria-label={users.actions.view}
                      >
                        <Icon icon="mdi:eye-outline" width={16} height={16} />
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

function UserDetailPageCanvas(): JSX.Element {
  const admin = useTranslate().admin
  const users = admin.users
  const detail = MOCK_USER_DETAIL
  const quotaSnapshot = buildStoryQuotaSnapshot(detail)
  const [quotaDraft, setQuotaDraft] = useState<Record<QuotaSliderField, string>>({
    hourlyAnyLimit: String(detail.hourlyAnyLimit),
    hourlyLimit: String(detail.quotaHourlyLimit),
    dailyLimit: String(detail.quotaDailyLimit),
    monthlyLimit: String(detail.quotaMonthlyLimit),
  })

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
        <div className="panel-header">
          <div>
            <h2>{users.quota.title}</h2>
            <p className="panel-description">{users.quota.description}</p>
          </div>
        </div>
        <div className="quota-grid" style={{ marginTop: 12 }}>
          {([
            {
              field: 'hourlyAnyLimit',
              label: users.quota.hourlyAny,
              used: detail.hourlyAnyUsed,
              currentLimit: detail.hourlyAnyLimit,
            },
            {
              field: 'hourlyLimit',
              label: users.quota.hourly,
              used: detail.quotaHourlyUsed,
              currentLimit: detail.quotaHourlyLimit,
            },
            {
              field: 'dailyLimit',
              label: users.quota.daily,
              used: detail.quotaDailyUsed,
              currentLimit: detail.quotaDailyLimit,
            },
            {
              field: 'monthlyLimit',
              label: users.quota.monthly,
              used: detail.quotaMonthlyUsed,
              currentLimit: detail.quotaMonthlyLimit,
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
              />
            )
          })}
        </div>
        <div style={{ marginTop: 16, display: 'flex', justifyContent: 'space-between', gap: 12, flexWrap: 'wrap' }}>
          <span className="panel-description">{users.quota.hint}</span>
          <button type="button" className="btn btn-primary">
            {users.quota.save}
          </button>
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
              {detail.tokens.map((token) => {
                const hourlyAnyText = `${formatNumber(token.hourlyAnyUsed)} / ${formatNumber(token.hourlyAnyLimit)}`
                const hourlyText = `${formatNumber(token.quotaHourlyUsed)} / ${formatNumber(token.quotaHourlyLimit)}`
                const dailyText = `${formatNumber(token.quotaDailyUsed)} / ${formatNumber(token.quotaDailyLimit)}`
                const monthlyText = `${formatNumber(token.quotaMonthlyUsed)} / ${formatNumber(token.quotaMonthlyLimit)}`
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
                          <span className="token-compact-value">{hourlyAnyText}</span>
                        </div>
                        <div className="token-compact-field">
                          <span className="token-compact-label">{users.tokens.table.hourly}</span>
                          <span className="token-compact-value">{hourlyText}</span>
                        </div>
                      </div>
                    </td>
                    <td>
                      <div className="token-compact-pair">
                        <div className="token-compact-field">
                          <span className="token-compact-label">{users.tokens.table.daily}</span>
                          <span className="token-compact-value">{dailyText}</span>
                        </div>
                        <div className="token-compact-field">
                          <span className="token-compact-label">{users.tokens.table.monthly}</span>
                          <span className="token-compact-value">{monthlyText}</span>
                        </div>
                      </div>
                    </td>
                    <td>
                      <div className="token-compact-pair">
                        <div className="token-compact-field">
                          <span className="token-compact-label">{users.tokens.table.successDaily}</span>
                          <span className="token-compact-value">{successDailyText}</span>
                        </div>
                        <div className="token-compact-field">
                          <span className="token-compact-label">{users.tokens.table.successMonthly}</span>
                          <span className="token-compact-value">{successMonthlyText}</span>
                        </div>
                      </div>
                    </td>
                    <td>
                      <button
                        type="button"
                        className="btn btn-circle btn-ghost btn-sm"
                        title={users.tokens.actions.view}
                        aria-label={users.tokens.actions.view}
                      >
                        <Icon icon="mdi:eye-outline" width={16} height={16} />
                      </button>
                    </td>
                  </tr>
                )
              })}
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
        sections={[admin.modules.alerts.sections.rules, admin.modules.alerts.sections.thresholds, admin.modules.alerts.sections.channels]}
        comingSoonLabel={admin.modules.comingSoon}
      />
    </AdminPageFrame>
  )
}

function ProxySettingsPageCanvas(): JSX.Element {
  const admin = useTranslate().admin
  return (
    <AdminPageFrame activeModule="proxy-settings">
      <ModulePlaceholder
        title={admin.modules.proxySettings.title}
        description={admin.modules.proxySettings.description}
        sections={[
          admin.modules.proxySettings.sections.upstream,
          admin.modules.proxySettings.sections.routing,
          admin.modules.proxySettings.sections.rateLimit,
        ]}
        comingSoonLabel={admin.modules.comingSoon}
      />
    </AdminPageFrame>
  )
}

const meta = {
  title: 'Admin/Pages',
  component: DashboardPageCanvas,
  parameters: {
    layout: 'fullscreen',
  },
} satisfies Meta<typeof DashboardPageCanvas>

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

export const ApiKeys: Story = {
  render: () => <KeysPageCanvas />,
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
