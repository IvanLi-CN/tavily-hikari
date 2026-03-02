import { Icon } from '@iconify/react'
import type { Meta, StoryObj } from '@storybook/react-vite'
import { Fragment, type ReactNode, useState } from 'react'

import type { ApiKeyStats, AuthToken, JobLogView, RequestLog } from '../api'
import AdminPanelHeader from '../components/AdminPanelHeader'
import { StatusBadge, type StatusTone } from '../components/StatusBadge'
import SegmentedTabs from '../components/ui/SegmentedTabs'
import { useTranslate, type AdminTranslations } from '../i18n'

import AdminShell, { type AdminNavItem } from './AdminShell'
import DashboardOverview, { type DashboardMetricCard } from './DashboardOverview'
import ModulePlaceholder from './ModulePlaceholder'
import type { AdminModuleId } from './routes'

const now = 1_762_380_000

const MOCK_TOKENS: AuthToken[] = [
  {
    id: '9vsN',
    enabled: true,
    note: 'Core production',
    group: 'production',
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
                    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                      <code>{token.id}</code>
                      {!token.enabled && <StatusBadge tone="warning">{tokenStrings.statusBadges.disabled}</StatusBadge>}
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
  return (
    <AdminPageFrame activeModule="users">
      <ModulePlaceholder
        title={admin.modules.users.title}
        description={admin.modules.users.description}
        sections={[admin.modules.users.sections.list, admin.modules.users.sections.roles, admin.modules.users.sections.status]}
        comingSoonLabel={admin.modules.comingSoon}
      />
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
}

export const Tokens: Story = {
  render: () => <TokensPageCanvas />,
}

export const ApiKeys: Story = {
  render: () => <KeysPageCanvas />,
}

export const Requests: Story = {
  render: () => <RequestsPageCanvas />,
}

export const Jobs: Story = {
  render: () => <JobsPageCanvas />,
}

export const Users: Story = {
  render: () => <UsersPageCanvas />,
}

export const Alerts: Story = {
  render: () => <AlertsPageCanvas />,
}

export const ProxySettings: Story = {
  render: () => <ProxySettingsPageCanvas />,
}
