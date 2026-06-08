import { useEffect, useMemo, useState } from 'react'

import type { DashboardHourlyRequestWindow, SummaryWindowsResponse } from '../api'
import SegmentedTabs from '../components/ui/SegmentedTabs'
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
  buildDeltaSeriesSlotValues,
  buildDashboardAreaStackLayers,
  buildHourlyRangeSlots,
  getVisibleHourlyWindow,
  DASHBOARD_RESULT_SERIES_ORDER,
  DASHBOARD_TYPE_SERIES_ORDER,
  DEFAULT_VISIBLE_RESULT_SERIES,
  DEFAULT_VISIBLE_TYPE_SERIES,
  createDashboardHourlyChartPreferences,
  formatHourlyBucketLabel,
  getResultSeriesValue,
  getTypeSeriesValue,
  readDashboardHourlyChartPreferences,
  toggleSeriesSelection,
  writeDashboardHourlyChartPreferences,
  type DashboardDeltaSelection,
  type DashboardHourlyChartMode,
  type DashboardHourlyChartPreferences,
  type DashboardResultSeriesId,
  type DashboardTypeSeriesId,
} from './dashboardHourlyCharts'
import type { DashboardOverviewStrings } from './DashboardOverview'

ChartJS.register(CategoryScale, LinearScale, BarElement, LineElement, PointElement, Filler, Tooltip, Legend)

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

function formatChartWindow(copy: string, count: number, comparisonCount: number): string {
  return copy
    .replace('{count}', String(count))
    .replace('{comparisonCount}', String(comparisonCount))
}

function formatChartWindowWithLabels(
  chartMode: DashboardHourlyChartMode,
  strings: Pick<DashboardOverviewStrings, 'chartUtcWindow' | 'chartRollingWindow' | 'chartDeltaWindow'>,
  count: number,
  comparisonCount: number,
): string {
  const template = chartMode === 'resultsDelta' || chartMode === 'typesDelta'
    ? strings.chartDeltaWindow
    : strings.chartRollingWindow
  return formatChartWindow(template, count, comparisonCount)
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

function isAreaChartMode(mode: DashboardHourlyChartMode): mode is 'resultsArea' | 'typesArea' {
  return mode === 'resultsArea' || mode === 'typesArea'
}

function isDeltaChartMode(mode: DashboardHourlyChartMode): mode is 'resultsDelta' | 'typesDelta' {
  return mode === 'resultsDelta' || mode === 'typesDelta'
}

export default function DashboardTrendPanel({
  strings,
  overviewReady,
  hourlyRequestWindow,
  summaryWindows,
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
  summaryWindows: SummaryWindowsResponse
  initialChartMode?: DashboardHourlyChartMode
  initialVisibleResultSeries?: ReadonlyArray<DashboardResultSeriesId>
  initialVisibleTypeSeries?: ReadonlyArray<DashboardTypeSeriesId>
  initialResultDeltaSeries?: DashboardDeltaSelection<DashboardResultSeriesId>
  initialTypeDeltaSeries?: DashboardDeltaSelection<DashboardTypeSeriesId>
  chartPersistenceKey?: string | null
  chartLabelTimeZone?: string | null
}): JSX.Element {
  const legacyChartPersistenceKeys = useMemo(
    () => (
      chartPersistenceKey === 'admin.dashboard.hourly-request-charts.v2'
        ? ['admin.dashboard.hourly-request-charts.v1']
        : []
    ),
    [chartPersistenceKey],
  )
  const initialPreferences = useMemo<DashboardHourlyChartPreferences>(() => {
    const fallback = createDashboardHourlyChartPreferences({
      chartMode: initialChartMode,
      visibleResultSeries: initialVisibleResultSeries,
      visibleTypeSeries: initialVisibleTypeSeries,
      resultDeltaSeries: initialResultDeltaSeries,
      typeDeltaSeries: initialTypeDeltaSeries,
    })
    if (typeof window === 'undefined') return fallback
    return readDashboardHourlyChartPreferences(
      window.localStorage,
      chartPersistenceKey,
      legacyChartPersistenceKeys,
    ) ?? fallback
  }, [
    chartPersistenceKey,
    initialChartMode,
    initialResultDeltaSeries,
    initialTypeDeltaSeries,
    initialVisibleResultSeries,
    initialVisibleTypeSeries,
    legacyChartPersistenceKeys,
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
  const visibleWindow = useMemo(
    () => getVisibleHourlyWindow(hourlyRequestWindow),
    [hourlyRequestWindow],
  )
  const comparisonRangeStart = summaryWindows.yesterday_start
  const comparisonRangeEnd = summaryWindows.yesterday_end
  const isDeltaMode = isDeltaChartMode(chartMode)
  const isAreaMode = isAreaChartMode(chartMode)
  const rollingRangeSlots = visibleWindow.slots
  const naturalDayRangeSlots = useMemo(
    () => buildHourlyRangeSlots(hourlyRequestWindow, summaryWindows.today_start, summaryWindows.today_end),
    [hourlyRequestWindow, summaryWindows.today_end, summaryWindows.today_start],
  )
  const rangeSlots = isDeltaMode ? naturalDayRangeSlots : rollingRangeSlots
  const comparisonRangeSlots = useMemo(
    () => buildHourlyRangeSlots(hourlyRequestWindow, comparisonRangeStart, comparisonRangeEnd),
    [comparisonRangeEnd, comparisonRangeStart, hourlyRequestWindow],
  )
  const labels = useMemo(
    () => {
      const slotCount = isDeltaMode ? Math.max(rangeSlots.length, comparisonRangeSlots.length) : rangeSlots.length
      return Array.from({ length: slotCount }, (_, index) => {
        const bucketStart = rangeSlots[index]?.bucketStart ?? comparisonRangeSlots[index]?.bucketStart
        return bucketStart == null ? ['', ''] : formatHourlyBucketLabel(bucketStart, chartLabelTimeZone ?? undefined)
      })
    },
    [chartLabelTimeZone, comparisonRangeSlots, isDeltaMode, rangeSlots],
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
      case 'resultsArea':
        return visibleResultSeries
      case 'types':
      case 'typesArea':
        return visibleTypeSeries
      case 'resultsDelta':
        return resultDeltaSeries === 'all' ? [...DASHBOARD_RESULT_SERIES_ORDER] : [resultDeltaSeries]
      case 'typesDelta':
        return typeDeltaSeries === 'all' ? [...DASHBOARD_TYPE_SERIES_ORDER] : [typeDeltaSeries]
    }
  }, [chartMode, resultDeltaSeries, typeDeltaSeries, visibleResultSeries, visibleTypeSeries])

  const chartData = useMemo<ChartData<'bar' | 'line'>>(() => {
    if (rangeSlots.length === 0 || activeSeries.length === 0) {
      return { labels, datasets: [] }
    }

    if (chartMode === 'results') {
      return {
        labels,
        datasets: activeSeries.map((seriesId) => ({
          label: resultSeriesLabels[seriesId as DashboardResultSeriesId],
          data: labels.map((_, index) => {
            const bucket = rangeSlots[index]?.bucket ?? null
            return bucket ? getResultSeriesValue(bucket, seriesId as DashboardResultSeriesId) : null
          }),
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
          data: labels.map((_, index) => {
            const bucket = rangeSlots[index]?.bucket ?? null
            return bucket ? getTypeSeriesValue(bucket, seriesId as DashboardTypeSeriesId) : null
          }),
          backgroundColor: seriesColors[seriesId as DashboardTypeSeriesId],
          borderRadius: 4,
          borderSkipped: false,
          stack: 'requests',
        })),
      }
    }

    if (chartMode === 'resultsArea') {
      return {
        labels,
        datasets: buildDashboardAreaStackLayers(activeSeries as DashboardResultSeriesId[]).map((layer) => {
          const seriesId = layer.seriesId
          return {
            type: 'line' as const,
            label: resultSeriesLabels[seriesId],
            data: labels.map((_, index) => {
              const bucket = rangeSlots[index]?.bucket ?? null
              return bucket ? getResultSeriesValue(bucket, seriesId) : null
            }),
            borderColor: seriesColors[seriesId],
            backgroundColor: withOpacity(seriesColors[seriesId], 0.22),
            fill: layer.fill,
            borderWidth: layer.borderWidth,
            pointRadius: layer.pointRadius,
            pointHoverRadius: layer.pointHoverRadius,
            tension: layer.tension,
            spanGaps: layer.spanGaps,
            stack: layer.stack,
          }
        }),
      }
    }

    if (chartMode === 'typesArea') {
      return {
        labels,
        datasets: buildDashboardAreaStackLayers(activeSeries as DashboardTypeSeriesId[]).map((layer) => {
          const seriesId = layer.seriesId
          return {
            type: 'line' as const,
            label: typeSeriesLabels[seriesId],
            data: labels.map((_, index) => {
              const bucket = rangeSlots[index]?.bucket ?? null
              return bucket ? getTypeSeriesValue(bucket, seriesId) : null
            }),
            borderColor: seriesColors[seriesId],
            backgroundColor: withOpacity(seriesColors[seriesId], 0.22),
            fill: layer.fill,
            borderWidth: layer.borderWidth,
            pointRadius: layer.pointRadius,
            pointHoverRadius: layer.pointHoverRadius,
            tension: layer.tension,
            spanGaps: layer.spanGaps,
            stack: layer.stack,
          }
        }),
      }
    }

    return {
      labels,
      datasets: activeSeries.map((seriesId) => ({
        label: chartMode === 'resultsDelta'
          ? resultSeriesLabels[seriesId as DashboardResultSeriesId]
          : typeSeriesLabels[seriesId as DashboardTypeSeriesId],
        data: buildDeltaSeriesSlotValues(
          rangeSlots,
          comparisonRangeSlots,
          seriesId as DashboardResultSeriesId | DashboardTypeSeriesId,
        ),
        backgroundColor: seriesColors[seriesId as DashboardResultSeriesId | DashboardTypeSeriesId],
        borderRadius: 4,
        borderSkipped: false,
        stack: 'delta',
      })),
    }
  }, [activeSeries, chartMode, comparisonRangeSlots, labels, rangeSlots, resultSeriesLabels, seriesColors, typeSeriesLabels])

  const chartOptions = useMemo<ChartOptions<'bar' | 'line'>>(() => {
    const isDelta = isDeltaMode
    return {
      responsive: true,
      maintainAspectRatio: false,
      animation: {
        duration: 560,
        easing: 'easeOutCubic',
      },
      plugins: {
        legend: { display: false },
        filler: {
          propagate: false,
        },
        tooltip: {
          mode: 'index',
          intersect: false,
          callbacks: {
            label(context: TooltipItem<'bar' | 'line'>) {
              const prefix = `${context.dataset.label}: `
              if (context.raw == null) return `${prefix}—`
              const value = typeof context.raw === 'number' ? context.raw : Number(context.raw)
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
  }, [isDeltaMode, palette.grid, palette.tick, palette.zeroLine])

  const barChartData = chartData as ChartData<'bar'>
  const lineChartData = chartData as ChartData<'line'>
  const barChartOptions = chartOptions as ChartOptions<'bar'>
  const lineChartOptions = chartOptions as ChartOptions<'line'>

  const modeOptions = [
    { value: 'results' as const, label: strings.chartModeResults },
    { value: 'types' as const, label: strings.chartModeTypes },
    { value: 'resultsDelta' as const, label: strings.chartModeResultsDelta },
    { value: 'typesDelta' as const, label: strings.chartModeTypesDelta },
    { value: 'resultsArea' as const, label: strings.chartModeResultsArea },
    { value: 'typesArea' as const, label: strings.chartModeTypesArea },
  ]

  const showEmpty = overviewReady && (rangeSlots.length === 0 || activeSeries.length === 0)
  const chartSeriesLabel = isDeltaMode ? strings.chartDeltaSeries : strings.chartVisibleSeries
  const chartMeta = formatChartWindowWithLabels(
    chartMode,
    strings,
    rangeSlots.length,
    comparisonRangeSlots.length,
  )

  return (
    <section className="surface panel dashboard-trend-panel">
      <div className="panel-header dashboard-trend-header">
        <div>
          <h2>{strings.trendsTitle}</h2>
          <p className="panel-description">{strings.trendsDescription}</p>
        </div>
        <div className="dashboard-trend-meta">{chartMeta}</div>
      </div>

      <SegmentedTabs<DashboardHourlyChartMode>
        className="dashboard-trend-segmented"
        value={chartMode}
        onChange={setChartMode}
        options={modeOptions}
        ariaLabel={strings.trendsTitle}
      />

      <div className="dashboard-chart-toolbar">
        <span className="dashboard-chart-toolbar-label">{chartSeriesLabel}</span>
        <div className="dashboard-chart-series-list" role="group" aria-label={chartSeriesLabel}>
          {(chartMode === 'results' || chartMode === 'resultsArea'
            ? DASHBOARD_RESULT_SERIES_ORDER.map((seriesId) => (
                <DashboardChartSeriesButton
                  key={seriesId}
                  active={visibleResultSeries.includes(seriesId)}
                  label={resultSeriesLabels[seriesId]}
                  color={seriesColors[seriesId]}
                  onClick={() => setVisibleResultSeries((current) => toggleSeriesSelection(current, seriesId))}
                />
              ))
            : chartMode === 'types' || chartMode === 'typesArea'
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
            {isAreaMode ? (
              <Line options={lineChartOptions} data={lineChartData} />
            ) : (
              <Bar options={barChartOptions} data={barChartData} />
            )}
          </div>
        )}
      </div>
    </section>
  )
}
