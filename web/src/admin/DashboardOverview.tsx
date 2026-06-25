import { useMemo } from 'react'

import type {
  ApiKeyStats,
  AuthToken,
  DashboardHourlyRequestWindow,
  DashboardMonthSeries,
  FreshnessView,
  JobLogView,
  RecentAlertsSummary,
  RequestLog,
  SummaryWindowsResponse,
} from '../api'
import RollingNumber from '../components/RollingNumber'
import RequestKindBadge from '../components/RequestKindBadge'
import { StatusBadge, type StatusTone } from '../components/StatusBadge'
import type { AdminModuleId } from './routes'
import { Line } from 'react-chartjs-2'
import {
  CategoryScale,
  Chart as ChartJS,
  Filler,
  Legend,
  LinearScale,
  LineElement,
  PointElement,
  Tooltip,
  type ChartData,
  type ChartOptions,
} from 'chart.js'
import DashboardTrendPanel from './DashboardTrendPanel'
import {
  type DashboardDeltaSelection,
  type DashboardHourlyChartMode,
  type DashboardResultSeriesId,
  type DashboardTypeSeriesId,
} from './dashboardHourlyCharts'
import {
  buildMonthSeriesBackdropSeries,
  buildPeriodBackdropSeries,
  getBackdropMetricKey,
  type DashboardBackdropMetricKey,
  type DashboardCardBackdropMap,
  type DashboardCardBackdropSeries,
} from './dashboardCardBackdrops'

ChartJS.register(CategoryScale, LinearScale, LineElement, PointElement, Filler, Tooltip, Legend)

export interface DashboardMetricCard {
  id: string
  label: string
  value: string
  valueNumber?: number
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
  localValueNumber?: number
  upstreamLabel: string
  upstreamValue: string
  upstreamValueNumber?: number
  deltaLabel: string
  deltaValue: string
  deltaValueNumber?: number
  deltaTone?: 'positive' | 'negative' | 'neutral'
  coverage: string
  freshness: string
}

export interface DashboardOverviewStrings {
  loading: string
  summaryUnavailable: string
  statusUnavailable: string
  freshnessFresh: string
  freshnessStale: string
  freshnessDegraded: string
  freshnessPendingRollups: string
  freshnessSqliteContention: string
  freshnessColdStart: string
  freshnessOptionalFeedFailure: string
  todayTitle: string
  todayDescription: string
  monthTitle: string
  monthDescription: string
  monthComparisonEmpty: string
  currentStatusTitle: string
  currentStatusDescription: string
  trendsTitle: string
  trendsDescription: string
  requestTrend: string
  errorTrend: string
  chartModeResults: string
  chartModeTypes: string
  chartModeResultsDelta: string
  chartModeTypesDelta: string
  chartModeResultsArea: string
  chartModeTypesArea: string
  chartVisibleSeries: string
  chartDeltaSeries: string
  chartSelectionAll: string
  chartEmpty: string
  chartUtcWindow: string
  chartRollingWindow: string
  chartDeltaWindow: string
  chartResultSecondarySuccess: string
  chartResultPrimarySuccess: string
  chartResultSecondaryFailure: string
  chartResultPrimaryFailure429: string
  chartResultPrimaryFailureOther: string
  chartResultUnknown: string
  chartTypeMcpNonBillable: string
  chartTypeMcpBillable: string
  chartTypeApiNonBillable: string
  chartTypeApiBillable: string
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
  recentAlertsTitle: string
  recentAlertsDescription: string
  recentAlertsEvents: string
  recentAlertsGroups: string
  recentAlertsEmpty: string
  recentAlertsOpen: string
  recentAlertsTypeLabels: Record<'upstream_rate_limited_429' | 'upstream_usage_limit_432' | 'upstream_key_blocked' | 'user_request_rate_limited' | 'user_quota_exhausted', string>
}

interface DashboardOverviewProps {
  strings: DashboardOverviewStrings
  overviewReady: boolean
  statusLoading: boolean
  freshness: FreshnessView | null
  todayMetrics: DashboardMetricCard[]
  todayQuotaCharge?: DashboardQuotaChargeCardData | null
  monthMetrics: DashboardMetricCard[]
  monthQuotaCharge?: DashboardQuotaChargeCardData | null
  statusMetrics: DashboardMetricCard[]
  summaryWindows?: SummaryWindowsResponse | null
  hourlyRequestWindow: DashboardHourlyRequestWindow
  monthSeries?: DashboardMonthSeries | null
  tokenCoverage: 'ok' | 'truncated' | 'error'
  tokens: AuthToken[]
  keys: ApiKeyStats[]
  logs: RequestLog[]
  jobs: JobLogView[]
  recentAlerts: RecentAlertsSummary
  onOpenModule: (module: AdminModuleId) => void
  onOpenRecentAlerts: () => void
  onOpenToken: (id: string) => void
  onOpenKey: (id: string) => void
  initialChartMode?: DashboardHourlyChartMode
  initialVisibleResultSeries?: ReadonlyArray<DashboardResultSeriesId>
  initialVisibleTypeSeries?: ReadonlyArray<DashboardTypeSeriesId>
  initialResultDeltaSeries?: DashboardDeltaSelection<DashboardResultSeriesId>
  initialTypeDeltaSeries?: DashboardDeltaSelection<DashboardTypeSeriesId>
  chartPersistenceKey?: string | null
  chartLabelTimeZone?: string | null
}

function describeFreshness(strings: DashboardOverviewStrings, freshness: FreshnessView | null): string | null {
  if (!freshness) return null
  if (freshness.state === 'fresh') return strings.freshnessFresh
  if (freshness.reason === 'pending_rollups') return strings.freshnessPendingRollups
  if (freshness.reason === 'sqlite_contention') return strings.freshnessSqliteContention
  if (freshness.reason === 'cold_start_no_cache') return strings.freshnessColdStart
  if (freshness.reason === 'optional_feed_failure') return strings.freshnessOptionalFeedFailure
  return freshness.state === 'degraded' ? strings.freshnessDegraded : strings.freshnessStale
}

function readChartColorVar(name: string, fallback: string): string {
  if (typeof document === 'undefined') return fallback
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  return value.length > 0 ? `hsl(${value})` : fallback
}

function withOpacity(color: string, opacity: number): string {
  return color.startsWith('hsl(') && color.endsWith(')')
    ? `${color.slice(0, -1)} / ${opacity})`
    : color
}

function buildCumulativeNullableSeries(
  values: ReadonlyArray<number | null>,
  initialValue = 0,
): Array<number | null> {
  let runningTotal = initialValue
  return values.map((value) => {
    if (value == null) return null
    runningTotal += Math.max(0, value)
    return runningTotal
  })
}

function MetricValue({
  value,
  valueNumber,
  compact = false,
}: {
  value: string
  valueNumber?: number
  compact?: boolean
}): JSX.Element {
  const splitValue = value.split(' / ')
  if (splitValue.length === 2) {
    return (
      <div className={`metric-value dashboard-metric-value-split${compact ? ' dashboard-metric-value-split-compact' : ''}`}>
        <span>{splitValue[0]}</span>
        <span className="dashboard-metric-value-divider">/ {splitValue[1]}</span>
      </div>
    )
  }

  if (typeof valueNumber === 'number' && Number.isFinite(valueNumber)) {
    return (
      <div className={`metric-value dashboard-metric-value${compact ? ' dashboard-metric-value-compact' : ''}`}>
        <RollingNumber value={valueNumber} />
      </div>
    )
  }

  return <div className="metric-value dashboard-metric-value">{value}</div>
}

function SummaryMetricCard({
  metric,
  compact = false,
  backdrop,
  backdropNotice,
}: {
  metric: DashboardMetricCard
  compact?: boolean
  backdrop?: DashboardCardBackdropSeries
  backdropNotice?: string | null
}): JSX.Element {
  const deltaTone = metric.comparison?.tone ?? (
    metric.comparison?.direction === 'flat'
      ? 'neutral'
      : metric.comparison?.direction === 'up'
        ? 'positive'
        : 'negative'
  )

  return (
    <div
      className={`metric-card dashboard-summary-card${backdrop ? ' dashboard-summary-card-with-backdrop' : ''}${compact ? ' dashboard-summary-card-compact' : ''}${metric.fullWidth ? ' dashboard-summary-card-full-width' : ''}`}
    >
      {backdrop ? (
        <DashboardUsageBackdropChart
          ariaLabel={metric.label}
          className="dashboard-summary-card-backdrop"
          primaryValues={backdrop.current}
          comparisonValues={backdrop.comparison}
          primaryColor={backdrop.color ?? readChartColorVar('--primary', 'hsl(262 83% 58%)')}
          comparisonColor={backdrop.comparisonColor}
          primaryInitialValue={backdrop.baseline ?? 0}
          comparisonInitialValue={backdrop.baseline ?? 0}
        />
      ) : null}
      <div className="dashboard-summary-card-content">
        <div className="dashboard-summary-card-heading">
          <h3>{metric.label}</h3>
          {metric.marker ? (
            <span className={`dashboard-summary-card-marker dashboard-summary-card-marker-${metric.markerTone ?? 'neutral'}`}>
              {metric.marker}
            </span>
          ) : null}
        </div>
        <div className="dashboard-summary-card-value-row">
          <MetricValue value={metric.value} valueNumber={metric.valueNumber} compact={compact} />
        </div>
        {metric.comparison ? (
          <div className="dashboard-summary-card-comparison-stack">
            {metric.valueMeta ? <div className="dashboard-summary-card-value-meta">{metric.valueMeta}</div> : null}
            <div className={`metric-delta metric-delta-${deltaTone}`}>
              <span className="metric-delta-label">{metric.comparison.label}</span>
              <span className="metric-delta-value">{metric.comparison.value}</span>
            </div>
          </div>
        ) : metric.subtitle ? (
          <div className="metric-subtitle">{metric.subtitle}</div>
        ) : null}
        {metric.comparison && metric.subtitle ? <div className="metric-subtitle">{metric.subtitle}</div> : null}
        {!metric.comparison && backdropNotice ? <div className="metric-subtitle">{backdropNotice}</div> : null}
      </div>
    </div>
  )
}

function QuotaChargeCard({
  card,
  backdrop,
}: {
  card: DashboardQuotaChargeCardData
  backdrop?: DashboardCardBackdropSeries
}): JSX.Element {
  return (
    <article className="metric-card dashboard-summary-card dashboard-summary-card-with-backdrop dashboard-quota-charge-card">
      {backdrop ? (
        <DashboardUsageBackdropChart
          ariaLabel={card.title}
          className="dashboard-summary-card-backdrop"
          primaryValues={backdrop.current}
          comparisonValues={backdrop.comparison}
          primaryColor={backdrop.color ?? readChartColorVar('--primary', 'hsl(262 83% 58%)')}
          comparisonColor={backdrop.comparisonColor}
          primaryInitialValue={backdrop.baseline ?? 0}
          comparisonInitialValue={backdrop.baseline ?? 0}
        />
      ) : null}
      <div className="dashboard-summary-card-content">
        <div className="dashboard-summary-card-heading">
          <h3>{card.title}</h3>
        </div>
        <div className="dashboard-quota-charge-grid">
          <div className="dashboard-quota-charge-value">
            <span className="dashboard-quota-charge-label">{card.localLabel}</span>
            <MetricValue value={card.localValue} valueNumber={card.localValueNumber} />
          </div>
          <div className="dashboard-quota-charge-value">
            <span className="dashboard-quota-charge-label">{card.upstreamLabel}</span>
            <MetricValue value={card.upstreamValue} valueNumber={card.upstreamValueNumber} />
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
      </div>
    </article>
  )
}

function DashboardUsageBackdropChart({
  ariaLabel,
  primaryValues,
  primaryColor,
  comparisonValues,
  comparisonColor,
  primaryInitialValue = 0,
  comparisonInitialValue = 0,
  className,
}: {
  ariaLabel: string
  primaryValues: ReadonlyArray<number | null>
  primaryColor: string
  comparisonValues?: ReadonlyArray<number | null>
  comparisonColor?: string
  primaryInitialValue?: number
  comparisonInitialValue?: number
  className?: string
}): JSX.Element | null {
  const chartData = useMemo<ChartData<'line'>>(() => {
    if (primaryValues.length === 0) {
      return { labels: [], datasets: [] }
    }

    const labels = primaryValues.map((_, index) => String(index + 1))
    const datasets: ChartData<'line'>['datasets'] = [
      {
        label: 'current',
        data: buildCumulativeNullableSeries(primaryValues, primaryInitialValue),
        borderColor: primaryColor,
        backgroundColor: withOpacity(primaryColor, 0.12),
        fill: true,
        borderWidth: 2,
        pointRadius: 0,
        pointHitRadius: 0,
        tension: 0.38,
        spanGaps: false,
        order: 1,
      },
    ]

    if (comparisonValues && comparisonValues.length > 0) {
      datasets.push({
        label: 'comparison',
        data: buildCumulativeNullableSeries(comparisonValues, comparisonInitialValue),
        borderColor: comparisonColor ?? primaryColor,
        backgroundColor: 'transparent',
        fill: false,
        borderWidth: 1.5,
        borderDash: [5, 4],
        pointRadius: 0,
        pointHitRadius: 0,
        tension: 0.34,
        spanGaps: false,
        order: 0,
      })
    }

    return { labels, datasets }
  }, [comparisonColor, comparisonInitialValue, comparisonValues, primaryColor, primaryInitialValue, primaryValues])

  const chartOptions = useMemo<ChartOptions<'line'>>(() => ({
    responsive: true,
    maintainAspectRatio: false,
    animation: {
      duration: 560,
      easing: 'easeOutCubic',
    },
    events: [],
    plugins: {
      legend: { display: false },
      tooltip: { enabled: false },
    },
    layout: {
      padding: 0,
    },
    elements: {
      line: {
        borderCapStyle: 'round',
        borderJoinStyle: 'round',
      },
    },
    scales: {
      x: {
        display: false,
        grid: { display: false },
        border: { display: false },
      },
      y: {
        display: false,
        grid: { display: false },
        border: { display: false },
      },
    },
  }), [])

  if (chartData.datasets.length === 0) return null

  return (
    <div className={className} aria-hidden="true">
      <Line aria-label={ariaLabel} options={chartOptions} data={chartData} />
    </div>
  )
}

function alertSummaryTone(type: keyof DashboardOverviewStrings['recentAlertsTypeLabels']): StatusTone {
  switch (type) {
    case 'upstream_key_blocked':
    case 'user_quota_exhausted':
      return 'error'
    case 'upstream_usage_limit_432':
    case 'upstream_rate_limited_429':
    case 'user_request_rate_limited':
      return 'warning'
    default:
      return 'neutral'
  }
}

export default function DashboardOverview({
  strings,
  overviewReady,
  statusLoading,
  freshness,
  todayMetrics,
  todayQuotaCharge,
  monthMetrics,
  monthQuotaCharge,
  statusMetrics,
  summaryWindows,
  hourlyRequestWindow,
  monthSeries,
  tokenCoverage,
  tokens,
  keys,
  logs,
  jobs,
  recentAlerts,
  onOpenModule,
  onOpenRecentAlerts,
  onOpenToken,
  onOpenKey,
  initialChartMode,
  initialVisibleResultSeries,
  initialVisibleTypeSeries,
  initialResultDeltaSeries,
  initialTypeDeltaSeries,
  chartPersistenceKey,
  chartLabelTimeZone,
}: DashboardOverviewProps): JSX.Element {
  const disabledTokens = tokens.filter((item) => !item.enabled).slice(0, 5)
  const exhaustedKeys = keys.filter((item) => item.status === 'exhausted').slice(0, 5)
  const failingJobs = jobs
    .filter((item) => {
      const normalized = item.status.trim().toLowerCase()
      return normalized === 'error' || normalized === 'failed'
    })
    .slice(0, 5)

  const riskItems: Array<{ id: string; label: string; action?: () => void; actionLabel?: string }> = []
  if (tokenCoverage === 'truncated') {
    riskItems.push({
      id: 'token-coverage-truncated',
      label: strings.tokenCoverageTruncated,
      action: () => onOpenModule('tokens'),
      actionLabel: strings.openModule,
    })
  }
  if (tokenCoverage === 'error') {
    riskItems.push({
      id: 'token-coverage-error',
      label: strings.tokenCoverageError,
      action: () => onOpenModule('tokens'),
      actionLabel: strings.openModule,
    })
  }
  for (const token of disabledTokens) {
    riskItems.push({
      id: `token-${token.id}`,
      label: strings.disabledTokenRisk.replace('{id}', token.id),
      action: () => onOpenToken(token.id),
      actionLabel: strings.openToken,
    })
  }
  for (const key of exhaustedKeys) {
    riskItems.push({
      id: `key-${key.id}`,
      label: strings.exhaustedKeyRisk.replace('{id}', key.id),
      action: () => onOpenKey(key.id),
      actionLabel: strings.openKey,
    })
  }
  for (const job of failingJobs) {
    riskItems.push({
      id: `job-${job.id}`,
      label: strings.failedJobRisk.replace('{id}', String(job.id)).replace('{status}', job.status),
      action: () => onOpenModule('jobs'),
      actionLabel: strings.recentJobs,
    })
  }

  const hasTodaySummary = todayMetrics.length > 0
  const hasMonthSummary = monthMetrics.length > 0
  const hasStatusSummary = statusMetrics.length > 0
  const todayTotalMetric = todayMetrics.find((metric) => metric.id === 'today-total') ?? null
  const todayDetailMetrics = todayMetrics.filter((metric) => metric.id !== 'today-total')
  const monthTotalMetric = monthMetrics.find((metric) => metric.id === 'month-total') ?? null
  const monthDetailMetrics = monthMetrics.filter((metric) => metric.id !== 'month-total')
  const summaryWindowValues = summaryWindows ?? {
    today: {
      total_requests: 0,
      success_count: 0,
      error_count: 0,
      quota_exhausted_count: 0,
      valuable_success_count: 0,
      valuable_failure_count: 0,
      other_success_count: 0,
      other_failure_count: 0,
      unknown_count: 0,
      upstream_exhausted_key_count: 0,
      new_keys: 0,
      new_quarantines: 0,
    },
    yesterday: {
      total_requests: 0,
      success_count: 0,
      error_count: 0,
      quota_exhausted_count: 0,
      valuable_success_count: 0,
      valuable_failure_count: 0,
      other_success_count: 0,
      other_failure_count: 0,
      unknown_count: 0,
      upstream_exhausted_key_count: 0,
      new_keys: 0,
      new_quarantines: 0,
    },
    month: {
      total_requests: 0,
      success_count: 0,
      error_count: 0,
      quota_exhausted_count: 0,
      valuable_success_count: 0,
      valuable_failure_count: 0,
      other_success_count: 0,
      other_failure_count: 0,
      unknown_count: 0,
      upstream_exhausted_key_count: 0,
      new_keys: 0,
      new_quarantines: 0,
    },
    today_start: 0,
    today_end: 0,
    yesterday_start: 0,
    yesterday_end: 0,
    month_start: 0,
    month_end: 0,
  }
  const backdropColors = {
    today: readChartColorVar('--primary', 'hsl(262 83% 58%)'),
    yesterday: readChartColorVar('--secondary', 'hsl(330 80% 51%)'),
    month: readChartColorVar('--info', 'hsl(199 89% 48%)'),
  }
  const comparisonRangeStart = summaryWindowValues.yesterday_start
  const comparisonRangeEnd = summaryWindowValues.today_start
  const todayPeriodEnd = summaryWindowValues.today_period_end ?? summaryWindowValues.today_end
  const monthSeriesValue = monthSeries ?? { current: [], comparison: [] }
  const todayBackdrop = useMemo(
    () => buildPeriodBackdropSeries({
      hourlyRequestWindow,
      currentValueRange: {
        rangeStart: summaryWindowValues.today_start,
        rangeEnd: summaryWindowValues.today_end,
      },
      currentDisplayRange: {
        rangeStart: summaryWindowValues.today_start,
        rangeEnd: todayPeriodEnd,
      },
      comparisonValueRange: {
        rangeStart: comparisonRangeStart,
        rangeEnd: comparisonRangeEnd,
      },
      comparisonDisplayRange: {
        rangeStart: comparisonRangeStart,
        rangeEnd: comparisonRangeEnd,
      },
      displayBucketSeconds: 3600,
      metricKey: 'total',
    }),
    [
      comparisonRangeEnd,
      comparisonRangeStart,
      hourlyRequestWindow,
      summaryWindowValues.today_end,
      summaryWindowValues.today_start,
      todayPeriodEnd,
    ],
  )
  const todayCardBackdrops = useMemo<DashboardCardBackdropMap>(() => (
    {
      total: {
        ...todayBackdrop,
        color: backdropColors.today,
        comparisonColor: backdropColors.yesterday,
      },
      valuableSuccess: {
        ...buildPeriodBackdropSeries({
          hourlyRequestWindow,
          currentValueRange: {
            rangeStart: summaryWindowValues.today_start,
            rangeEnd: summaryWindowValues.today_end,
          },
          currentDisplayRange: {
            rangeStart: summaryWindowValues.today_start,
            rangeEnd: todayPeriodEnd,
          },
          comparisonValueRange: {
            rangeStart: comparisonRangeStart,
            rangeEnd: comparisonRangeEnd,
          },
          comparisonDisplayRange: {
            rangeStart: comparisonRangeStart,
            rangeEnd: comparisonRangeEnd,
          },
          displayBucketSeconds: 3600,
          metricKey: 'valuableSuccess',
        }),
        color: backdropColors.today,
        comparisonColor: backdropColors.yesterday,
      },
      valuableFailure: {
        ...buildPeriodBackdropSeries({
          hourlyRequestWindow,
          currentValueRange: {
            rangeStart: summaryWindowValues.today_start,
            rangeEnd: summaryWindowValues.today_end,
          },
          currentDisplayRange: {
            rangeStart: summaryWindowValues.today_start,
            rangeEnd: todayPeriodEnd,
          },
          comparisonValueRange: {
            rangeStart: comparisonRangeStart,
            rangeEnd: comparisonRangeEnd,
          },
          comparisonDisplayRange: {
            rangeStart: comparisonRangeStart,
            rangeEnd: comparisonRangeEnd,
          },
          displayBucketSeconds: 3600,
          metricKey: 'valuableFailure',
        }),
        color: readChartColorVar('--destructive', 'hsl(0 84% 60%)'),
        comparisonColor: backdropColors.yesterday,
      },
      otherSuccess: {
        ...buildPeriodBackdropSeries({
          hourlyRequestWindow,
          currentValueRange: {
            rangeStart: summaryWindowValues.today_start,
            rangeEnd: summaryWindowValues.today_end,
          },
          currentDisplayRange: {
            rangeStart: summaryWindowValues.today_start,
            rangeEnd: todayPeriodEnd,
          },
          comparisonValueRange: {
            rangeStart: comparisonRangeStart,
            rangeEnd: comparisonRangeEnd,
          },
          comparisonDisplayRange: {
            rangeStart: comparisonRangeStart,
            rangeEnd: comparisonRangeEnd,
          },
          displayBucketSeconds: 3600,
          metricKey: 'otherSuccess',
        }),
        color: readChartColorVar('--success', 'hsl(160 84% 39%)'),
        comparisonColor: backdropColors.yesterday,
      },
      otherFailure: {
        ...buildPeriodBackdropSeries({
          hourlyRequestWindow,
          currentValueRange: {
            rangeStart: summaryWindowValues.today_start,
            rangeEnd: summaryWindowValues.today_end,
          },
          currentDisplayRange: {
            rangeStart: summaryWindowValues.today_start,
            rangeEnd: todayPeriodEnd,
          },
          comparisonValueRange: {
            rangeStart: comparisonRangeStart,
            rangeEnd: comparisonRangeEnd,
          },
          comparisonDisplayRange: {
            rangeStart: comparisonRangeStart,
            rangeEnd: comparisonRangeEnd,
          },
          displayBucketSeconds: 3600,
          metricKey: 'otherFailure',
        }),
        color: readChartColorVar('--warning', 'hsl(38 92% 50%)'),
        comparisonColor: backdropColors.yesterday,
      },
      unknown: {
        ...buildPeriodBackdropSeries({
          hourlyRequestWindow,
          currentValueRange: {
            rangeStart: summaryWindowValues.today_start,
            rangeEnd: summaryWindowValues.today_end,
          },
          currentDisplayRange: {
            rangeStart: summaryWindowValues.today_start,
            rangeEnd: todayPeriodEnd,
          },
          comparisonValueRange: {
            rangeStart: comparisonRangeStart,
            rangeEnd: comparisonRangeEnd,
          },
          comparisonDisplayRange: {
            rangeStart: comparisonRangeStart,
            rangeEnd: comparisonRangeEnd,
          },
          displayBucketSeconds: 3600,
          metricKey: 'unknown',
        }),
        color: readChartColorVar('--muted-foreground', 'hsl(215 16% 47%)'),
        comparisonColor: backdropColors.yesterday,
      },
      upstreamExhausted: {
        ...buildPeriodBackdropSeries({
          hourlyRequestWindow,
          currentValueRange: {
            rangeStart: summaryWindowValues.today_start,
            rangeEnd: summaryWindowValues.today_end,
          },
          currentDisplayRange: {
            rangeStart: summaryWindowValues.today_start,
            rangeEnd: todayPeriodEnd,
          },
          comparisonValueRange: {
            rangeStart: comparisonRangeStart,
            rangeEnd: comparisonRangeEnd,
          },
          comparisonDisplayRange: {
            rangeStart: comparisonRangeStart,
            rangeEnd: comparisonRangeEnd,
          },
          displayBucketSeconds: 3600,
          metricKey: 'upstreamExhausted',
        }),
        color: readChartColorVar('--info', 'hsl(199 89% 48%)'),
        comparisonColor: backdropColors.yesterday,
      },
    }
  ), [
    backdropColors.today,
    backdropColors.yesterday,
    comparisonRangeEnd,
    comparisonRangeStart,
    hourlyRequestWindow,
    todayBackdrop,
  ])
  const monthBackdrop = useMemo(
    () => {
      const backdrop = buildMonthSeriesBackdropSeries(monthSeriesValue, 'total')
      return {
        ...backdrop,
        baseline: 0,
      }
    },
    [monthSeriesValue],
  )
  const monthCardBackdrops = useMemo<DashboardCardBackdropMap>(() => {
    const buildMonthCardBackdrop = (
      metricKey: DashboardBackdropMetricKey,
      color = backdropColors.month,
    ): DashboardCardBackdropSeries => {
      const backdrop = buildMonthSeriesBackdropSeries(monthSeriesValue, metricKey)
      return {
        ...backdrop,
        baseline: 0,
        color,
        comparisonColor: backdropColors.yesterday,
      }
    }
    return {
      total: {
        ...monthBackdrop,
        color: backdropColors.month,
        comparisonColor: backdropColors.yesterday,
      },
      valuableSuccess: buildMonthCardBackdrop('valuableSuccess'),
      valuableFailure: buildMonthCardBackdrop('valuableFailure', readChartColorVar('--destructive', 'hsl(0 84% 60%)')),
      otherSuccess: buildMonthCardBackdrop('otherSuccess', readChartColorVar('--success', 'hsl(160 84% 39%)')),
      otherFailure: buildMonthCardBackdrop('otherFailure', readChartColorVar('--warning', 'hsl(38 92% 50%)')),
      unknown: buildMonthCardBackdrop('unknown', readChartColorVar('--muted-foreground', 'hsl(215 16% 47%)')),
      upstreamExhausted: buildMonthCardBackdrop('upstreamExhausted', readChartColorVar('--info', 'hsl(199 89% 48%)')),
    }
  }, [
    backdropColors.month,
    backdropColors.yesterday,
    monthBackdrop,
    monthSeriesValue,
  ])
  const monthComparisonNotice = monthBackdrop.hasVisibleComparison ? null : strings.monthComparisonEmpty
  const alertGroupCount = overviewReady && recentAlerts.totalEvents > 0 ? 1 : 0
  const priorityCount = riskItems.length + alertGroupCount
  const focusMetric = todayTotalMetric ?? monthTotalMetric ?? statusMetrics[0] ?? null
  const freshnessLabel = describeFreshness(strings, freshness)

  return (
    <div className="dashboard-overview-stack">
      {freshnessLabel ? (
        <section className={`surface panel ${freshness?.state === 'fresh' ? '' : 'dashboard-summary-fallback'}`}>
          <div className="panel-description">{freshnessLabel}</div>
        </section>
      ) : null}
      <section className="surface panel dashboard-priority-panel" aria-label={strings.riskTitle}>
        <div className="dashboard-priority-copy">
          <span className={`dashboard-priority-kicker${priorityCount > 0 ? ' dashboard-priority-kicker-warn' : ''}`}>
            {priorityCount > 0 ? strings.riskTitle : strings.riskEmpty}
          </span>
          <div className="dashboard-priority-count" aria-label={`${strings.riskTitle}: ${overviewReady ? priorityCount : 0}`}>
            {overviewReady
              ? priorityCount > 0
                ? String(priorityCount)
                : '0'
              : '—'}
          </div>
          <p className="panel-description">
            {priorityCount > 0
              ? `${strings.riskDescription} · ${strings.recentAlertsEvents}: ${recentAlerts.totalEvents}`
              : strings.currentStatusDescription}
          </p>
        </div>
        <div className="dashboard-priority-strip">
          {focusMetric ? (
            <div className="dashboard-priority-metric">
              <span>{focusMetric.label}</span>
              <strong>{focusMetric.value}</strong>
            </div>
          ) : null}
          <div className="dashboard-priority-actions">
            <button type="button" className="btn btn-outline btn-sm" onClick={onOpenRecentAlerts}>
              {strings.recentAlertsOpen}
            </button>
            <button type="button" className="btn btn-ghost btn-sm" onClick={() => onOpenModule('tokens')}>
              {strings.openModule}
            </button>
          </div>
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
                <div className="dashboard-summary-block-content">
                  <header className="dashboard-summary-header">
                    <div>
                      <h2>{strings.todayTitle}</h2>
                      <p className="panel-description">{strings.todayDescription}</p>
                    </div>
                  </header>
                  {hasTodaySummary ? (
                    <div className="dashboard-summary-section-stack">
                      {todayTotalMetric ? (
                        <SummaryMetricCard
                          metric={todayTotalMetric}
                          backdrop={todayCardBackdrops[getBackdropMetricKey(todayTotalMetric.id) ?? 'total']}
                        />
                      ) : null}
                      {todayQuotaCharge ? <QuotaChargeCard card={todayQuotaCharge} backdrop={todayCardBackdrops.total} /> : null}
                      <div className="dashboard-summary-metrics dashboard-summary-metrics-primary dashboard-today-grid">
                        {todayDetailMetrics.map((metric) => (
                          <SummaryMetricCard
                            key={metric.id}
                            metric={metric}
                            backdrop={todayCardBackdrops[getBackdropMetricKey(metric.id) ?? 'total']}
                          />
                        ))}
                      </div>
                    </div>
                  ) : (
                    <div className="empty-state alert dashboard-summary-empty">{strings.summaryUnavailable}</div>
                  )}
                </div>
              </article>

              <article className="dashboard-summary-block dashboard-summary-block-secondary">
                <div className="dashboard-summary-block-content">
                  <header className="dashboard-summary-header">
                    <div>
                      <h2>{strings.monthTitle}</h2>
                      <p className="panel-description">{strings.monthDescription}</p>
                    </div>
                  </header>
                  {hasMonthSummary ? (
                    <div className="dashboard-summary-section-stack">
                      {monthTotalMetric ? (
                        <SummaryMetricCard
                          metric={monthTotalMetric}
                          backdrop={monthCardBackdrops[getBackdropMetricKey(monthTotalMetric.id) ?? 'total']}
                          backdropNotice={monthComparisonNotice}
                        />
                      ) : null}
                      {monthQuotaCharge ? <QuotaChargeCard card={monthQuotaCharge} backdrop={monthCardBackdrops.total} /> : null}
                      <div className="dashboard-summary-metrics dashboard-summary-metrics-compact dashboard-summary-metrics-month">
                        {monthDetailMetrics.map((metric) => (
                          <SummaryMetricCard
                            key={metric.id}
                            metric={metric}
                            compact
                            backdrop={monthCardBackdrops[getBackdropMetricKey(metric.id) ?? 'total']}
                            backdropNotice={monthComparisonNotice}
                          />
                        ))}
                      </div>
                    </div>
                  ) : (
                    <div className="empty-state alert dashboard-summary-empty">{strings.summaryUnavailable}</div>
                  )}
                </div>
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

      <DashboardTrendPanel
        strings={strings}
        overviewReady={overviewReady}
        hourlyRequestWindow={hourlyRequestWindow}
        summaryWindows={summaryWindowValues}
        initialChartMode={initialChartMode}
        initialVisibleResultSeries={initialVisibleResultSeries}
        initialVisibleTypeSeries={initialVisibleTypeSeries}
        initialResultDeltaSeries={initialResultDeltaSeries}
        initialTypeDeltaSeries={initialTypeDeltaSeries}
        chartPersistenceKey={chartPersistenceKey}
        chartLabelTimeZone={chartLabelTimeZone}
      />

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
                    {item.actionLabel ?? strings.openModule}
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
            <h2>{strings.recentAlertsTitle}</h2>
            <p className="panel-description">{strings.recentAlertsDescription}</p>
          </div>
          <button type="button" className="btn btn-outline" onClick={onOpenRecentAlerts}>
            {strings.recentAlertsOpen}
          </button>
        </div>
        {!overviewReady ? (
          <div className="empty-state alert">{strings.loading}</div>
        ) : recentAlerts.totalEvents === 0 ? (
          <div className="empty-state alert">{strings.recentAlertsEmpty}</div>
        ) : (
          <div className="dashboard-alerts-summary">
            <div className="dashboard-alerts-summary__metrics">
              <article className="dashboard-alerts-summary__metric-card">
                <span>{strings.recentAlertsEvents}</span>
                <strong>{recentAlerts.totalEvents}</strong>
              </article>
              <article className="dashboard-alerts-summary__metric-card">
                <span>{strings.recentAlertsGroups}</span>
                <strong>{recentAlerts.groupedCount}</strong>
              </article>
              {recentAlerts.countsByType.map((item) => (
                <article className="dashboard-alerts-summary__metric-card" key={item.type}>
                  <span>{strings.recentAlertsTypeLabels[item.type]}</span>
                  <strong>{item.count}</strong>
                </article>
              ))}
            </div>
            <div className="dashboard-alerts-summary__groups">
              {recentAlerts.topGroups.map((group) => (
                <article key={group.id} className="dashboard-alerts-summary__group-card">
                  <div className="dashboard-alerts-summary__group-header">
                    <StatusBadge tone={alertSummaryTone(group.type)}>
                      {strings.recentAlertsTypeLabels[group.type]}
                    </StatusBadge>
                    <strong>{group.subjectLabel}</strong>
                    <span>x{group.count}</span>
                  </div>
                  <div className="dashboard-alerts-summary__group-body">
                    {group.requestKind ? (
                      <RequestKindBadge
                        requestKindKey={group.requestKind.key}
                        requestKindLabel={group.requestKind.label}
                        size="sm"
                      />
                    ) : null}
                    <span>{group.latestEvent.summary}</span>
                  </div>
                </article>
              ))}
            </div>
          </div>
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
