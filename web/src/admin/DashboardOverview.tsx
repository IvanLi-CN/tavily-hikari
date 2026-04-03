import type { ApiKeyStats, AuthToken, JobLogView, RequestLog } from '../api'
import type { AdminModuleId } from './routes'

export interface DashboardMetricCard {
  id: string
  label: string
  value: string
  marker?: string
  markerTone?: 'primary' | 'secondary' | 'neutral'
  valueMeta?: string
  subtitle?: string
  fullWidth?: boolean
  comparison?: {
    label: string
    value: string
    direction: 'up' | 'down' | 'flat'
    tone?: 'positive' | 'negative' | 'neutral'
  }
}

export interface DashboardQuotaChargeCardData {
  title: string
  localLabel: string
  localValue: string
  upstreamLabel: string
  upstreamValue: string
  deltaLabel: string
  deltaValue: string
  deltaTone?: 'positive' | 'negative' | 'neutral'
  coverage: string
  freshness: string
}

export interface DashboardOverviewStrings {
  title: string
  description: string
  loading: string
  summaryUnavailable: string
  statusUnavailable: string
  todayTitle: string
  todayDescription: string
  monthTitle: string
  monthDescription: string
  currentStatusTitle: string
  currentStatusDescription: string
  trendsTitle: string
  trendsDescription: string
  requestTrend: string
  errorTrend: string
  riskTitle: string
  riskDescription: string
  riskEmpty: string
  actionsTitle: string
  actionsDescription: string
  recentRequests: string
  recentJobs: string
  openModule: string
  openToken: string
  openKey: string
  disabledTokenRisk: string
  exhaustedKeyRisk: string
  failedJobRisk: string
  tokenCoverageTruncated: string
  tokenCoverageError: string
}

interface DashboardOverviewProps {
  strings: DashboardOverviewStrings
  overviewReady: boolean
  statusLoading: boolean
  todayMetrics: DashboardMetricCard[]
  todayQuotaCharge?: DashboardQuotaChargeCardData | null
  monthMetrics: DashboardMetricCard[]
  monthQuotaCharge?: DashboardQuotaChargeCardData | null
  statusMetrics: DashboardMetricCard[]
  trend: {
    request: number[]
    error: number[]
  }
  tokenCoverage: 'ok' | 'truncated' | 'error'
  tokens: AuthToken[]
  keys: ApiKeyStats[]
  logs: RequestLog[]
  jobs: JobLogView[]
  onOpenModule: (module: AdminModuleId) => void
  onOpenToken: (id: string) => void
  onOpenKey: (id: string) => void
}

function toPoints(values: number[]): string {
  if (values.length === 0) return ''
  const max = Math.max(...values, 1)
  const min = Math.min(...values, 0)
  const width = 220
  const height = 64
  return values
    .map((value, index) => {
      const x = values.length === 1 ? 0 : (index / (values.length - 1)) * width
      const normalized = (value - min) / Math.max(max - min, 1)
      const y = height - normalized * height
      return `${x.toFixed(2)},${y.toFixed(2)}`
    })
    .join(' ')
}

function Sparkline({ values }: { values: number[] }): JSX.Element {
  const points = toPoints(values)
  return (
    <svg viewBox="0 0 220 64" className="dashboard-sparkline" aria-hidden="true" preserveAspectRatio="none">
      <polyline points={points} fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" />
    </svg>
  )
}

function MetricValue({ value, compact = false }: { value: string; compact?: boolean }): JSX.Element {
  const splitValue = value.split(' / ')
  if (splitValue.length === 2) {
    return (
      <div className={`metric-value dashboard-metric-value-split${compact ? ' dashboard-metric-value-split-compact' : ''}`}>
        <span>{splitValue[0]}</span>
        <span className="dashboard-metric-value-divider">/ {splitValue[1]}</span>
      </div>
    )
  }

  return <div className="metric-value dashboard-metric-value">{value}</div>
}

function SummaryMetricCard({ metric, compact = false }: { metric: DashboardMetricCard; compact?: boolean }): JSX.Element {
  const deltaTone = metric.comparison?.tone ?? (
    metric.comparison?.direction === 'flat'
      ? 'neutral'
      : metric.comparison?.direction === 'up'
        ? 'positive'
        : 'negative'
  )

  return (
    <div
      className={`metric-card dashboard-summary-card${compact ? ' dashboard-summary-card-compact' : ''}${metric.fullWidth ? ' dashboard-summary-card-full-width' : ''}`}
    >
      <div className="dashboard-summary-card-heading">
        <h3>{metric.label}</h3>
        {metric.marker ? (
          <span className={`dashboard-summary-card-marker dashboard-summary-card-marker-${metric.markerTone ?? 'neutral'}`}>
            {metric.marker}
          </span>
        ) : null}
      </div>
      <div className="dashboard-summary-card-value-row">
        <MetricValue value={metric.value} compact={compact} />
        {metric.valueMeta ? <div className="dashboard-summary-card-value-meta">{metric.valueMeta}</div> : null}
      </div>
      {metric.comparison ? (
        <div className={`metric-delta metric-delta-${deltaTone}`}>
          <span className="metric-delta-label">{metric.comparison.label}</span>
          <span className="metric-delta-value">{metric.comparison.value}</span>
        </div>
      ) : metric.subtitle ? (
        <div className="metric-subtitle">{metric.subtitle}</div>
      ) : null}
      {metric.comparison && metric.subtitle ? <div className="metric-subtitle">{metric.subtitle}</div> : null}
    </div>
  )
}

function QuotaChargeCard({ card }: { card: DashboardQuotaChargeCardData }): JSX.Element {
  return (
    <article className="metric-card dashboard-summary-card dashboard-quota-charge-card">
      <div className="dashboard-summary-card-heading">
        <h3>{card.title}</h3>
      </div>
      <div className="dashboard-quota-charge-grid">
        <div className="dashboard-quota-charge-value">
          <span className="dashboard-quota-charge-label">{card.localLabel}</span>
          <span className="metric-value dashboard-metric-value">{card.localValue}</span>
        </div>
        <div className="dashboard-quota-charge-value">
          <span className="dashboard-quota-charge-label">{card.upstreamLabel}</span>
          <span className="metric-value dashboard-metric-value">{card.upstreamValue}</span>
        </div>
      </div>
      <div className="dashboard-quota-charge-footer">
        <div className={`metric-delta metric-delta-${card.deltaTone ?? 'neutral'}`}>
          <span className="metric-delta-label">{card.deltaLabel}</span>
          <span className="metric-delta-value">{card.deltaValue}</span>
        </div>
        <div className="dashboard-quota-charge-meta">
          <span>{card.coverage}</span>
          <span>{card.freshness}</span>
        </div>
      </div>
    </article>
  )
}

export default function DashboardOverview({
  strings,
  overviewReady,
  statusLoading,
  todayMetrics,
  todayQuotaCharge,
  monthMetrics,
  monthQuotaCharge,
  statusMetrics,
  trend,
  tokenCoverage,
  tokens,
  keys,
  logs,
  jobs,
  onOpenModule,
  onOpenToken,
  onOpenKey,
}: DashboardOverviewProps): JSX.Element {
  const disabledTokens = tokens.filter((item) => !item.enabled).slice(0, 5)
  const exhaustedKeys = keys.filter((item) => item.status === 'exhausted').slice(0, 5)
  const failingJobs = jobs
    .filter((item) => {
      const normalized = item.status.trim().toLowerCase()
      return normalized === 'error' || normalized === 'failed'
    })
    .slice(0, 5)

  const riskItems: Array<{ id: string; label: string; action?: () => void }> = []
  if (tokenCoverage === 'truncated') {
    riskItems.push({
      id: 'token-coverage-truncated',
      label: strings.tokenCoverageTruncated,
      action: () => onOpenModule('tokens'),
    })
  }
  if (tokenCoverage === 'error') {
    riskItems.push({
      id: 'token-coverage-error',
      label: strings.tokenCoverageError,
      action: () => onOpenModule('tokens'),
    })
  }
  for (const token of disabledTokens) {
    riskItems.push({
      id: `token-${token.id}`,
      label: strings.disabledTokenRisk.replace('{id}', token.id),
      action: () => onOpenToken(token.id),
    })
  }
  for (const key of exhaustedKeys) {
    riskItems.push({
      id: `key-${key.id}`,
      label: strings.exhaustedKeyRisk.replace('{id}', key.id),
      action: () => onOpenKey(key.id),
    })
  }
  for (const job of failingJobs) {
    riskItems.push({
      id: `job-${job.id}`,
      label: strings.failedJobRisk.replace('{id}', String(job.id)).replace('{status}', job.status),
      action: () => onOpenModule('jobs'),
    })
  }

  const hasTodaySummary = todayMetrics.length > 0
  const hasMonthSummary = monthMetrics.length > 0
  const hasStatusSummary = statusMetrics.length > 0
  const todayTotalMetric = todayMetrics.find((metric) => metric.id === 'today-total') ?? null
  const todayDetailMetrics = todayMetrics.filter((metric) => metric.id !== 'today-total')
  const monthTotalMetric = monthMetrics.find((metric) => metric.id === 'month-total') ?? null
  const monthDetailMetrics = monthMetrics.filter((metric) => metric.id !== 'month-total')

  return (
    <div className="dashboard-overview-stack">
      <section className="surface panel dashboard-hero-panel">
        <div className="panel-header">
          <div>
            <h2>{strings.title}</h2>
            <p className="panel-description">{strings.description}</p>
          </div>
          <button type="button" className="btn btn-outline" onClick={() => onOpenModule('tokens')}>
            {strings.openModule}
          </button>
        </div>
      </section>

      <section className="dashboard-summary-panel">
        {!overviewReady ? (
          <div className="surface panel dashboard-summary-fallback">
            <div className="empty-state alert">{strings.loading}</div>
          </div>
        ) : !hasTodaySummary && !hasMonthSummary && !hasStatusSummary ? (
          <div className="surface panel dashboard-summary-fallback">
            <div className="empty-state alert">{overviewReady ? strings.summaryUnavailable : strings.loading}</div>
          </div>
        ) : (
          <div className="dashboard-summary-layout">
            <div className="dashboard-summary-top-row">
              <article className="dashboard-summary-block dashboard-summary-block-primary">
                <header className="dashboard-summary-header">
                  <div>
                    <h2>{strings.todayTitle}</h2>
                    <p className="panel-description">{strings.todayDescription}</p>
                  </div>
                </header>
                {hasTodaySummary ? (
                  <div className="dashboard-summary-section-stack">
                    {todayTotalMetric ? <SummaryMetricCard metric={todayTotalMetric} /> : null}
                    {todayQuotaCharge ? <QuotaChargeCard card={todayQuotaCharge} /> : null}
                    <div className="dashboard-summary-metrics dashboard-summary-metrics-primary dashboard-today-grid">
                      {todayDetailMetrics.map((metric) => (
                        <SummaryMetricCard key={metric.id} metric={metric} />
                      ))}
                    </div>
                  </div>
                ) : (
                  <div className="empty-state alert dashboard-summary-empty">{strings.summaryUnavailable}</div>
                )}
              </article>

              <article className="dashboard-summary-block dashboard-summary-block-secondary">
                <header className="dashboard-summary-header">
                  <div>
                    <h2>{strings.monthTitle}</h2>
                    <p className="panel-description">{strings.monthDescription}</p>
                  </div>
                </header>
                {hasMonthSummary ? (
                  <div className="dashboard-summary-section-stack">
                    {monthTotalMetric ? <SummaryMetricCard metric={monthTotalMetric} /> : null}
                    {monthQuotaCharge ? <QuotaChargeCard card={monthQuotaCharge} /> : null}
                    <div className="dashboard-summary-metrics dashboard-summary-metrics-compact dashboard-summary-metrics-month">
                      {monthDetailMetrics.map((metric) => (
                        <SummaryMetricCard key={metric.id} metric={metric} compact />
                      ))}
                    </div>
                  </div>
                ) : (
                  <div className="empty-state alert dashboard-summary-empty">{strings.summaryUnavailable}</div>
                )}
              </article>
            </div>

            <article className="dashboard-summary-block dashboard-summary-block-status">
              <header className="dashboard-summary-header">
                <div>
                  <h2>{strings.currentStatusTitle}</h2>
                  <p className="panel-description">{strings.currentStatusDescription}</p>
                </div>
              </header>
              {hasStatusSummary ? (
                <div className="dashboard-summary-metrics dashboard-summary-metrics-compact dashboard-summary-metrics-status">
                  {statusMetrics.map((metric) => (
                    <SummaryMetricCard key={metric.id} metric={metric} compact />
                  ))}
                </div>
              ) : (
                <div className="empty-state alert dashboard-summary-empty">
                  {statusLoading ? strings.loading : strings.statusUnavailable}
                </div>
              )}
            </article>
          </div>
        )}
      </section>

      <section className="surface panel dashboard-trend-panel">
        <div className="panel-header">
          <div>
            <h2>{strings.trendsTitle}</h2>
            <p className="panel-description">{strings.trendsDescription}</p>
          </div>
        </div>
        <div className="dashboard-trend-grid">
          <article className="dashboard-trend-card">
            <header>{strings.requestTrend}</header>
            <Sparkline values={trend.request} />
          </article>
          <article className="dashboard-trend-card">
            <header>{strings.errorTrend}</header>
            <Sparkline values={trend.error} />
          </article>
        </div>
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{strings.riskTitle}</h2>
            <p className="panel-description">{strings.riskDescription}</p>
          </div>
        </div>
        {!overviewReady ? (
          <div className="empty-state alert">{strings.loading}</div>
        ) : riskItems.length === 0 ? (
          <div className="empty-state alert">{strings.riskEmpty}</div>
        ) : (
          <ul className="dashboard-risk-list">
            {riskItems.map((item) => (
              <li key={item.id}>
                <span>{item.label}</span>
                {item.action && (
                  <button type="button" className="btn btn-ghost btn-sm" onClick={item.action}>
                    {strings.openModule}
                  </button>
                )}
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{strings.actionsTitle}</h2>
            <p className="panel-description">{strings.actionsDescription}</p>
          </div>
        </div>
        <div className="dashboard-actions-grid">
          <article className="dashboard-actions-card">
            <h3>{strings.recentRequests}</h3>
            <ul>
              {logs.slice(0, 5).map((log) => (
                <li key={log.id}>
                  <code>{log.key_id}</code>
                  <span>{log.result_status}</span>
                </li>
              ))}
            </ul>
          </article>
          <article className="dashboard-actions-card">
            <h3>{strings.recentJobs}</h3>
            <ul>
              {jobs.slice(0, 5).map((job) => (
                <li key={job.id}>
                  <span>#{job.id}</span>
                  <span>{job.status}</span>
                </li>
              ))}
            </ul>
          </article>
        </div>
      </section>
    </div>
  )
}
