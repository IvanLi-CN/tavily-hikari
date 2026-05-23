import { useEffect, useMemo, useState } from 'react'

import type {
  ApiKeyStats,
  AuthToken,
  DashboardHourlyRequestWindow,
  JobLogView,
  RecentAlertsSummary,
  RequestLog,
  SummaryWindowsResponse,
} from '../api'
import RollingNumber from '../components/RollingNumber'
import SegmentedTabs from '../components/ui/SegmentedTabs'
import RequestKindBadge from '../components/RequestKindBadge'
import { StatusBadge, type StatusTone } from '../components/StatusBadge'
import type { AdminModuleId } from './routes'
import { Bar, Line } from 'react-chartjs-2'
import {
  BarElement,
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
  type TooltipItem,
} from 'chart.js'
import {
  buildDeltaSeriesValues,
  buildHourlyBucketLookup,
  DASHBOARD_RESULT_SERIES_ORDER,
  DASHBOARD_TYPE_SERIES_ORDER,
  DEFAULT_VISIBLE_RESULT_SERIES,
  DEFAULT_VISIBLE_TYPE_SERIES,
  createDashboardHourlyChartPreferences,
  formatHourlyBucketLabel,
  getResultSeriesValue,
  getTypeSeriesValue,
  getVisibleHourlyBuckets,
  readDashboardHourlyChartPreferences,
  toggleSeriesSelection,
  writeDashboardHourlyChartPreferences,
  type DashboardDeltaSelection,
  type DashboardHourlyChartMode,
  type DashboardHourlyChartPreferences,
  type DashboardResultSeriesId,
  type DashboardTypeSeriesId,
} from './dashboardHourlyCharts'
import {
  buildHourlyBackdropSeries,
  getBackdropMetricKey,
  type DashboardBackdropMetricKey,
  type DashboardCardBackdropMap,
  type DashboardCardBackdropSeries,
} from './dashboardCardBackdrops'

ChartJS.register(CategoryScale, LinearScale, BarElement, LineElement, PointElement, Filler, Tooltip, Legend)

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
  chartModeResults: string
  chartModeTypes: string
  chartModeResultsDelta: string
  chartModeTypesDelta: string
  chartVisibleSeries: string
  chartDeltaSeries: string
  chartSelectionAll: string
  chartEmpty: string
  chartUtcWindow: string
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
  todayMetrics: DashboardMetricCard[]
  todayQuotaCharge?: DashboardQuotaChargeCardData | null
  monthMetrics: DashboardMetricCard[]
  monthQuotaCharge?: DashboardQuotaChargeCardData | null
  statusMetrics: DashboardMetricCard[]
  summaryWindows?: SummaryWindowsResponse | null
  hourlyRequestWindow: DashboardHourlyRequestWindow
  tokenCoverage: 'ok' | 'truncated' | 'error'
  tokens: AuthToken[]
  keys: ApiKeyStats[]
  logs: RequestLog[]
  jobs: JobLogView[]
  recentAlerts: RecentAlertsSummary
  onOpenModule: (module: AdminModuleId) => void
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

interface DashboardChartPalette {
  secondarySuccess: string
  primarySuccess: string
  secondaryFailure: string
  primaryFailure429: string
  primaryFailureOther: string
  unknown: string
  mcpNonBillable: string
  mcpBillable: string
  apiNonBillable: string
  apiBillable: string
  grid: string
  tick: string
  zeroLine: string
}

function readChartColorVar(name: string, fallback: string): string {
  if (typeof document === 'undefined') return fallback
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  return value.length > 0 ? `hsl(${value})` : fallback
}

function readDashboardChartPalette(): DashboardChartPalette {
  return {
    secondarySuccess: readChartColorVar('--dashboard-chart-result-secondary-success', '#34d399'),
    primarySuccess: readChartColorVar('--dashboard-chart-result-primary-success', '#10b981'),
    secondaryFailure: readChartColorVar('--dashboard-chart-result-secondary-failure', '#f59e0b'),
    primaryFailure429: readChartColorVar('--dashboard-chart-result-primary-failure-429', '#f97316'),
    primaryFailureOther: readChartColorVar('--dashboard-chart-result-primary-failure-other', '#ef4444'),
    unknown: readChartColorVar('--dashboard-chart-result-unknown', '#94a3b8'),
    mcpNonBillable: readChartColorVar('--dashboard-chart-type-mcp-non-billable', '#67e8f9'),
    mcpBillable: readChartColorVar('--dashboard-chart-type-mcp-billable', '#22d3ee'),
    apiNonBillable: readChartColorVar('--dashboard-chart-type-api-non-billable', '#93c5fd'),
    apiBillable: readChartColorVar('--dashboard-chart-type-api-billable', '#60a5fa'),
    grid: readChartColorVar('--dashboard-chart-grid', 'rgba(148, 163, 184, 0.18)'),
    tick: readChartColorVar('--dashboard-chart-tick', '#cbd5e1'),
    zeroLine: readChartColorVar('--dashboard-chart-zero-line', 'rgba(148, 163, 184, 0.32)'),
  }
}

function formatSignedValue(value: number): string {
  if (value > 0) return `+${value}`
  return String(value)
}

function withOpacity(color: string, opacity: number): string {
  return color.startsWith('hsl(') && color.endsWith(')')
    ? `${color.slice(0, -1)} / ${opacity})`
    : color
}

function buildCumulativeSeries(values: ReadonlyArray<number>, initialValue = 0): number[] {
  let runningTotal = initialValue
  return values.map((value) => {
    runningTotal += Math.max(0, value)
    return runningTotal
  })
}

function formatChartWindow(copy: string, count: number): string {
  return copy.replace('{count}', String(count))
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
}: {
  metric: DashboardMetricCard
  compact?: boolean
  backdrop?: DashboardCardBackdropSeries
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
  primaryValues: ReadonlyArray<number>
  primaryColor: string
  comparisonValues?: ReadonlyArray<number>
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
        data: buildCumulativeSeries(primaryValues, primaryInitialValue),
        borderColor: primaryColor,
        backgroundColor: withOpacity(primaryColor, 0.12),
        fill: true,
        borderWidth: 2,
        pointRadius: 0,
        pointHitRadius: 0,
        tension: 0.38,
        spanGaps: true,
        order: 1,
      },
    ]

    if (comparisonValues && comparisonValues.length > 0) {
      datasets.push({
        label: 'comparison',
        data: buildCumulativeSeries(comparisonValues, comparisonInitialValue),
        borderColor: comparisonColor ?? primaryColor,
        backgroundColor: 'transparent',
        fill: false,
        borderWidth: 1.5,
        borderDash: [5, 4],
        pointRadius: 0,
        pointHitRadius: 0,
        tension: 0.34,
        spanGaps: true,
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

function DashboardChartSeriesButton({
  active,
  label,
  color,
  onClick,
}: {
  active: boolean
  label: string
  color: string
  onClick: () => void
}): JSX.Element {
  return (
    <button
      type="button"
      className={`dashboard-chart-series-chip${active ? ' is-active' : ''}`}
      onClick={onClick}
      aria-pressed={active}
    >
      <span className="dashboard-chart-series-chip-swatch" style={{ backgroundColor: color }} aria-hidden="true" />
      <span>{label}</span>
    </button>
  )
}

function DashboardTrendPanel({
  strings,
  overviewReady,
  hourlyRequestWindow,
  initialChartMode = 'results',
  initialVisibleResultSeries = DEFAULT_VISIBLE_RESULT_SERIES,
  initialVisibleTypeSeries = DEFAULT_VISIBLE_TYPE_SERIES,
  initialResultDeltaSeries = 'all',
  initialTypeDeltaSeries = 'all',
  chartPersistenceKey = null,
  chartLabelTimeZone = null,
}: {
  strings: DashboardOverviewStrings
  overviewReady: boolean
  hourlyRequestWindow: DashboardHourlyRequestWindow
  initialChartMode?: DashboardHourlyChartMode
  initialVisibleResultSeries?: ReadonlyArray<DashboardResultSeriesId>
  initialVisibleTypeSeries?: ReadonlyArray<DashboardTypeSeriesId>
  initialResultDeltaSeries?: DashboardDeltaSelection<DashboardResultSeriesId>
  initialTypeDeltaSeries?: DashboardDeltaSelection<DashboardTypeSeriesId>
  chartPersistenceKey?: string | null
  chartLabelTimeZone?: string | null
}): JSX.Element {
  const initialPreferences = useMemo<DashboardHourlyChartPreferences>(() => {
    const fallback = createDashboardHourlyChartPreferences({
      chartMode: initialChartMode,
      visibleResultSeries: initialVisibleResultSeries,
      visibleTypeSeries: initialVisibleTypeSeries,
      resultDeltaSeries: initialResultDeltaSeries,
      typeDeltaSeries: initialTypeDeltaSeries,
    })
    if (typeof window === 'undefined') return fallback
    return readDashboardHourlyChartPreferences(window.localStorage, chartPersistenceKey) ?? fallback
  }, [
    chartPersistenceKey,
    initialChartMode,
    initialResultDeltaSeries,
    initialTypeDeltaSeries,
    initialVisibleResultSeries,
    initialVisibleTypeSeries,
  ])

  const [chartMode, setChartMode] = useState<DashboardHourlyChartMode>(initialPreferences.chartMode)
  const [visibleResultSeries, setVisibleResultSeries] = useState<DashboardResultSeriesId[]>(initialPreferences.visibleResultSeries)
  const [visibleTypeSeries, setVisibleTypeSeries] = useState<DashboardTypeSeriesId[]>(initialPreferences.visibleTypeSeries)
  const [resultDeltaSeries, setResultDeltaSeries] = useState<DashboardDeltaSelection<DashboardResultSeriesId>>(initialPreferences.resultDeltaSeries)
  const [typeDeltaSeries, setTypeDeltaSeries] = useState<DashboardDeltaSelection<DashboardTypeSeriesId>>(initialPreferences.typeDeltaSeries)

  useEffect(() => {
    if (typeof window === 'undefined') return
    writeDashboardHourlyChartPreferences(window.localStorage, chartPersistenceKey, {
      chartMode,
      visibleResultSeries,
      visibleTypeSeries,
      resultDeltaSeries,
      typeDeltaSeries,
    })
  }, [
    chartMode,
    chartPersistenceKey,
    resultDeltaSeries,
    typeDeltaSeries,
    visibleResultSeries,
    visibleTypeSeries,
  ])

  const palette = readDashboardChartPalette()
  const visibleBuckets = useMemo(() => getVisibleHourlyBuckets(hourlyRequestWindow), [hourlyRequestWindow])
  const retainedLookup = useMemo(() => buildHourlyBucketLookup(hourlyRequestWindow.buckets), [hourlyRequestWindow.buckets])
  const labels = useMemo(
    () => visibleBuckets.map((bucket) => formatHourlyBucketLabel(bucket.bucketStart, chartLabelTimeZone ?? undefined)),
    [chartLabelTimeZone, visibleBuckets],
  )

  const resultSeriesLabels: Record<DashboardResultSeriesId, string> = {
    secondarySuccess: strings.chartResultSecondarySuccess,
    primarySuccess: strings.chartResultPrimarySuccess,
    secondaryFailure: strings.chartResultSecondaryFailure,
    primaryFailure429: strings.chartResultPrimaryFailure429,
    primaryFailureOther: strings.chartResultPrimaryFailureOther,
    unknown: strings.chartResultUnknown,
  }
  const typeSeriesLabels: Record<DashboardTypeSeriesId, string> = {
    mcpNonBillable: strings.chartTypeMcpNonBillable,
    mcpBillable: strings.chartTypeMcpBillable,
    apiNonBillable: strings.chartTypeApiNonBillable,
    apiBillable: strings.chartTypeApiBillable,
  }
  const seriesColors: Record<DashboardResultSeriesId | DashboardTypeSeriesId, string> = {
    secondarySuccess: palette.secondarySuccess,
    primarySuccess: palette.primarySuccess,
    secondaryFailure: palette.secondaryFailure,
    primaryFailure429: palette.primaryFailure429,
    primaryFailureOther: palette.primaryFailureOther,
    unknown: palette.unknown,
    mcpNonBillable: palette.mcpNonBillable,
    mcpBillable: palette.mcpBillable,
    apiNonBillable: palette.apiNonBillable,
    apiBillable: palette.apiBillable,
  }

  const activeSeries = useMemo(() => {
    switch (chartMode) {
      case 'results':
        return visibleResultSeries
      case 'types':
        return visibleTypeSeries
      case 'resultsDelta':
        return resultDeltaSeries === 'all' ? [...DASHBOARD_RESULT_SERIES_ORDER] : [resultDeltaSeries]
      case 'typesDelta':
        return typeDeltaSeries === 'all' ? [...DASHBOARD_TYPE_SERIES_ORDER] : [typeDeltaSeries]
    }
  }, [chartMode, resultDeltaSeries, typeDeltaSeries, visibleResultSeries, visibleTypeSeries])

  const chartData = useMemo<ChartData<'bar'>>(() => {
    if (visibleBuckets.length === 0 || activeSeries.length === 0) {
      return { labels, datasets: [] }
    }

    if (chartMode === 'results') {
      return {
        labels,
        datasets: activeSeries.map((seriesId) => ({
          label: resultSeriesLabels[seriesId as DashboardResultSeriesId],
          data: visibleBuckets.map((bucket) => getResultSeriesValue(bucket, seriesId as DashboardResultSeriesId)),
          backgroundColor: seriesColors[seriesId as DashboardResultSeriesId],
          borderRadius: 4,
          borderSkipped: false,
          stack: 'requests',
        })),
      }
    }

    if (chartMode === 'types') {
      return {
        labels,
        datasets: activeSeries.map((seriesId) => ({
          label: typeSeriesLabels[seriesId as DashboardTypeSeriesId],
          data: visibleBuckets.map((bucket) => getTypeSeriesValue(bucket, seriesId as DashboardTypeSeriesId)),
          backgroundColor: seriesColors[seriesId as DashboardTypeSeriesId],
          borderRadius: 4,
          borderSkipped: false,
          stack: 'requests',
        })),
      }
    }

    return {
      labels,
      datasets: activeSeries.map((seriesId) => ({
        label: (chartMode === 'resultsDelta'
          ? resultSeriesLabels[seriesId as DashboardResultSeriesId]
          : typeSeriesLabels[seriesId as DashboardTypeSeriesId]),
        data: buildDeltaSeriesValues(visibleBuckets, retainedLookup, seriesId as DashboardResultSeriesId | DashboardTypeSeriesId),
        backgroundColor: seriesColors[seriesId as DashboardResultSeriesId | DashboardTypeSeriesId],
        borderRadius: 4,
        borderSkipped: false,
        stack: 'delta',
      })),
    }
  }, [activeSeries, chartMode, labels, resultSeriesLabels, retainedLookup, seriesColors, typeSeriesLabels, visibleBuckets])

  const chartOptions = useMemo<ChartOptions<'bar'>>(() => {
    const isDelta = chartMode === 'resultsDelta' || chartMode === 'typesDelta'
    return {
    responsive: true,
    maintainAspectRatio: false,
    animation: {
      duration: 560,
      easing: 'easeOutCubic',
    },
      plugins: {
        legend: { display: false },
        tooltip: {
          mode: 'index',
          intersect: false,
          callbacks: {
            label(context: TooltipItem<'bar'>) {
              const prefix = `${context.dataset.label}: `
              const value = typeof context.raw === 'number' ? context.raw : Number(context.raw ?? 0)
              return prefix + (isDelta ? formatSignedValue(value) : value)
            },
          },
        },
      },
      scales: {
        x: {
          stacked: true,
          grid: { display: false },
          ticks: {
            color: palette.tick,
            maxRotation: 0,
            autoSkipPadding: 14,
          },
        },
        y: {
          stacked: true,
          beginAtZero: !isDelta,
          ticks: {
            color: palette.tick,
            callback(value) {
              return isDelta ? formatSignedValue(Number(value)) : String(value)
            },
          },
          grid: {
            color(context) {
              return Number(context.tick.value) === 0 ? palette.zeroLine : palette.grid
            },
          },
        },
      },
    }
  }, [chartMode, palette.grid, palette.tick, palette.zeroLine])

  const modeOptions = [
    { value: 'results' as const, label: strings.chartModeResults },
    { value: 'types' as const, label: strings.chartModeTypes },
    { value: 'resultsDelta' as const, label: strings.chartModeResultsDelta },
    { value: 'typesDelta' as const, label: strings.chartModeTypesDelta },
  ]

  const showEmpty = overviewReady && (visibleBuckets.length === 0 || activeSeries.length === 0)

  return (
    <section className="surface panel dashboard-trend-panel">
      <div className="panel-header dashboard-trend-header">
        <div>
          <h2>{strings.trendsTitle}</h2>
          <p className="panel-description">{strings.trendsDescription}</p>
        </div>
        <div className="dashboard-trend-meta">{formatChartWindow(strings.chartUtcWindow, hourlyRequestWindow.visibleBuckets)}</div>
      </div>

      <SegmentedTabs<DashboardHourlyChartMode>
        className="dashboard-trend-segmented"
        value={chartMode}
        onChange={setChartMode}
        options={modeOptions}
        ariaLabel={strings.trendsTitle}
      />

      <div className="dashboard-chart-toolbar">
        <span className="dashboard-chart-toolbar-label">
          {chartMode === 'results' || chartMode === 'types' ? strings.chartVisibleSeries : strings.chartDeltaSeries}
        </span>
        <div className="dashboard-chart-series-list" role="group" aria-label={strings.chartVisibleSeries}>
          {(chartMode === 'results'
            ? DASHBOARD_RESULT_SERIES_ORDER.map((seriesId) => (
                <DashboardChartSeriesButton
                  key={seriesId}
                  active={visibleResultSeries.includes(seriesId)}
                  label={resultSeriesLabels[seriesId]}
                  color={seriesColors[seriesId]}
                  onClick={() => setVisibleResultSeries((current) => toggleSeriesSelection(current, seriesId))}
                />
              ))
            : chartMode === 'types'
              ? DASHBOARD_TYPE_SERIES_ORDER.map((seriesId) => (
                  <DashboardChartSeriesButton
                    key={seriesId}
                    active={visibleTypeSeries.includes(seriesId)}
                    label={typeSeriesLabels[seriesId]}
                    color={seriesColors[seriesId]}
                    onClick={() => setVisibleTypeSeries((current) => toggleSeriesSelection(current, seriesId))}
                  />
                ))
              : chartMode === 'resultsDelta'
                ? [
                    <DashboardChartSeriesButton
                      key="all"
                      active={resultDeltaSeries === 'all'}
                      label={strings.chartSelectionAll}
                      color={palette.tick}
                      onClick={() => setResultDeltaSeries('all')}
                    />,
                    ...DASHBOARD_RESULT_SERIES_ORDER.map((seriesId) => (
                      <DashboardChartSeriesButton
                        key={seriesId}
                        active={resultDeltaSeries === seriesId}
                        label={resultSeriesLabels[seriesId]}
                        color={seriesColors[seriesId]}
                        onClick={() => setResultDeltaSeries(seriesId)}
                      />
                    )),
                  ]
                : [
                    <DashboardChartSeriesButton
                      key="all"
                      active={typeDeltaSeries === 'all'}
                      label={strings.chartSelectionAll}
                      color={palette.tick}
                      onClick={() => setTypeDeltaSeries('all')}
                    />,
                    ...DASHBOARD_TYPE_SERIES_ORDER.map((seriesId) => (
                      <DashboardChartSeriesButton
                        key={seriesId}
                        active={typeDeltaSeries === seriesId}
                        label={typeSeriesLabels[seriesId]}
                        color={seriesColors[seriesId]}
                        onClick={() => setTypeDeltaSeries(seriesId)}
                      />
                    )),
                  ])}
        </div>
      </div>

      <div className="dashboard-chart-shell">
        {!overviewReady ? (
          <div className="empty-state alert">{strings.loading}</div>
        ) : showEmpty ? (
          <div className="empty-state alert">{strings.chartEmpty}</div>
        ) : (
          <div className="dashboard-chart-canvas">
            <Bar options={chartOptions} data={chartData} />
          </div>
        )}
      </div>
    </section>
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
  summaryWindows,
  hourlyRequestWindow,
  tokenCoverage,
  tokens,
  keys,
  logs,
  jobs,
  recentAlerts,
  onOpenModule,
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
  const comparisonRangeEnd = summaryWindowValues.yesterday_end
  const todayBackdrop = useMemo(
    () => buildHourlyBackdropSeries(
      hourlyRequestWindow,
      summaryWindowValues.today_start,
      summaryWindowValues.today_end,
      'total',
      comparisonRangeStart,
      comparisonRangeEnd,
    ),
    [
      comparisonRangeEnd,
      comparisonRangeStart,
      hourlyRequestWindow,
      summaryWindowValues.today_end,
      summaryWindowValues.today_start,
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
        ...buildHourlyBackdropSeries(
          hourlyRequestWindow,
          summaryWindowValues.today_start,
          summaryWindowValues.today_end,
          'valuableSuccess',
          comparisonRangeStart,
          comparisonRangeEnd,
        ),
        color: backdropColors.today,
        comparisonColor: backdropColors.yesterday,
      },
      valuableFailure: {
        ...buildHourlyBackdropSeries(
          hourlyRequestWindow,
          summaryWindowValues.today_start,
          summaryWindowValues.today_end,
          'valuableFailure',
          comparisonRangeStart,
          comparisonRangeEnd,
        ),
        color: readChartColorVar('--destructive', 'hsl(0 84% 60%)'),
        comparisonColor: backdropColors.yesterday,
      },
      otherSuccess: {
        ...buildHourlyBackdropSeries(
          hourlyRequestWindow,
          summaryWindowValues.today_start,
          summaryWindowValues.today_end,
          'otherSuccess',
          comparisonRangeStart,
          comparisonRangeEnd,
        ),
        color: readChartColorVar('--success', 'hsl(160 84% 39%)'),
        comparisonColor: backdropColors.yesterday,
      },
      otherFailure: {
        ...buildHourlyBackdropSeries(
          hourlyRequestWindow,
          summaryWindowValues.today_start,
          summaryWindowValues.today_end,
          'otherFailure',
          comparisonRangeStart,
          comparisonRangeEnd,
        ),
        color: readChartColorVar('--warning', 'hsl(38 92% 50%)'),
        comparisonColor: backdropColors.yesterday,
      },
      unknown: {
        ...buildHourlyBackdropSeries(
          hourlyRequestWindow,
          summaryWindowValues.today_start,
          summaryWindowValues.today_end,
          'unknown',
          comparisonRangeStart,
          comparisonRangeEnd,
        ),
        color: readChartColorVar('--muted-foreground', 'hsl(215 16% 47%)'),
        comparisonColor: backdropColors.yesterday,
      },
      upstreamExhausted: {
        ...buildHourlyBackdropSeries(
          hourlyRequestWindow,
          summaryWindowValues.today_start,
          summaryWindowValues.today_end,
          'upstreamExhausted',
          comparisonRangeStart,
          comparisonRangeEnd,
        ),
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
    summaryWindowValues.today_end,
    summaryWindowValues.today_start,
    todayBackdrop,
  ])
  const monthBackdrop = useMemo(
    () => buildHourlyBackdropSeries(
      hourlyRequestWindow,
      summaryWindowValues.today_start,
      summaryWindowValues.today_end,
      'total',
      comparisonRangeStart,
      comparisonRangeEnd,
    ),
    [
      comparisonRangeEnd,
      comparisonRangeStart,
      hourlyRequestWindow,
      summaryWindowValues.today_end,
      summaryWindowValues.today_start,
    ],
  )
  const monthBackdropBaseline = Math.max(
    summaryWindowValues.month.total_requests - summaryWindowValues.today.total_requests,
    0,
  )
  const monthCardBackdrops = useMemo<DashboardCardBackdropMap>(() => {
    const buildMonthCardBackdrop = (
      metricKey: DashboardBackdropMetricKey,
      color = backdropColors.month,
    ): DashboardCardBackdropSeries => ({
      ...buildHourlyBackdropSeries(
        hourlyRequestWindow,
        summaryWindowValues.today_start,
        summaryWindowValues.today_end,
        metricKey,
        comparisonRangeStart,
        comparisonRangeEnd,
      ),
      baseline: metricKey === 'total' ? monthBackdropBaseline : 0,
      color,
      comparisonColor: backdropColors.today,
    })
    return {
      total: {
        ...monthBackdrop,
        baseline: monthBackdropBaseline,
        color: backdropColors.month,
        comparisonColor: backdropColors.today,
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
    backdropColors.today,
    comparisonRangeEnd,
    comparisonRangeStart,
    hourlyRequestWindow,
    monthBackdrop,
    monthBackdropBaseline,
    summaryWindowValues.today_end,
    summaryWindowValues.today_start,
  ])
  const alertGroupCount = overviewReady && recentAlerts.totalEvents > 0 ? 1 : 0
  const priorityCount = riskItems.length + alertGroupCount
  const focusMetric = todayTotalMetric ?? monthTotalMetric ?? statusMetrics[0] ?? null

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
            <button type="button" className="btn btn-outline btn-sm" onClick={() => onOpenModule('alerts')}>
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
          <button type="button" className="btn btn-outline" onClick={() => onOpenModule('alerts')}>
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
