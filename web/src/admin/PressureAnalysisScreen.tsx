import { useMemo } from 'react'

import type { ComposeOption } from 'echarts/core'
import * as echarts from 'echarts/core'
import ReactEChartsCore from 'echarts-for-react/lib/core'
import { LineChart, type LineSeriesOption } from 'echarts/charts'
import { GridComponent, TooltipComponent, type GridComponentOption, type TooltipComponentOption } from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'
import {
  CategoryScale,
  Chart as ChartJS,
  Legend,
  LinearScale,
  LineElement,
  PointElement,
  Tooltip,
  type ChartData,
  type ChartOptions,
} from 'chart.js'
import { Line } from 'react-chartjs-2'

import type {
  AnalysisCurrentUserPressureDistribution,
  AnalysisPressureSnapshot,
} from '../api'
import type { AdminTranslations, Language } from '../i18n'
import AdminLoadingRegion from '../components/AdminLoadingRegion'

ChartJS.register(CategoryScale, LinearScale, LineElement, PointElement, Tooltip, Legend)
echarts.use([LineChart, GridComponent, TooltipComponent, CanvasRenderer])

type PressureDistributionOption = ComposeOption<
  LineSeriesOption | GridComponentOption | TooltipComponentOption
>

type PressureDistributionBucket = {
  start: number
  end: number
  rangeLabel: string
  total: number
  success: number
  failure: number
}

function readChartColorVar(name: string, fallback: string): string {
  if (typeof document === 'undefined') return fallback
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  if (value.length === 0) return fallback
  return value.startsWith('hsl(') || value.startsWith('rgb(') ? value : `hsl(${value})`
}

function withOpacity(color: string, opacity: number): string {
  if (color.startsWith('hsl(') && color.endsWith(')')) {
    const body = color
      .slice(4, -1)
      .split('/')
      .shift()
      ?.trim() ?? ''
    return `hsl(${body} / ${opacity})`
  }
  return color
}

function formatNumber(language: Language, value: number): string {
  return new Intl.NumberFormat(language === 'zh' ? 'zh-CN' : 'en-US').format(value)
}

function formatAxisTime(language: Language, timestamp: number): string {
  return new Intl.DateTimeFormat(language === 'zh' ? 'zh-CN' : 'en-US', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  }).format(new Date(timestamp * 1000))
}

function formatAxisHour(language: Language, timestamp: number): string {
  return new Intl.DateTimeFormat(language === 'zh' ? 'zh-CN' : 'en-US', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    hour12: false,
  }).format(new Date(timestamp * 1000))
}

function formatPressureBucketLabel(language: Language, start: number, end: number): string {
  const formatValue = (value: number) => formatNumber(language, value)
  return `${formatValue(start)}-${formatValue(end)}`
}

function chooseNiceStep(value: number): number {
  if (value <= 1) return 1
  const magnitude = 10 ** Math.floor(Math.log10(value))
  const normalized = value / magnitude
  if (normalized <= 1) return magnitude
  if (normalized <= 2) return 2 * magnitude
  if (normalized <= 5) return 5 * magnitude
  return 10 * magnitude
}

function buildPressureLineOptions(
  language: Language,
  tooltipLabelFormatter: (timestamp: number) => string,
): ChartOptions<'line'> {
  const tickColor = readChartColorVar('--dashboard-chart-tick', '#635f69')
  const legendColor = readChartColorVar('--muted-foreground', '#635f69')
  const gridColor = readChartColorVar('--border', '#d7dfec')
  return {
    responsive: true,
    maintainAspectRatio: false,
    interaction: {
      mode: 'index',
      intersect: false,
    },
    plugins: {
      legend: {
        labels: {
          color: legendColor,
          boxWidth: 18,
          boxHeight: 8,
          usePointStyle: false,
        },
      },
      tooltip: {
        callbacks: {
          title(items) {
            const first = items[0]
            if (!first) return ''
            return tooltipLabelFormatter(Number(first.label))
          },
          label(context) {
            const value = typeof context.raw === 'number' ? context.raw : 0
            return `${context.dataset.label ?? ''}: ${formatNumber(language, value)}`
          },
        },
      },
    },
    scales: {
      x: {
        ticks: {
          color: tickColor,
          callback: function callback(value) {
            const label = typeof this.getLabelForValue === 'function'
              ? this.getLabelForValue(Number(value))
              : String(value)
            return tooltipLabelFormatter(Number(label))
          },
          maxRotation: 0,
          autoSkip: true,
          maxTicksLimit: 8,
        },
        grid: {
          color: withOpacity(gridColor, 0.34),
          drawTicks: false,
        },
      },
      y: {
        beginAtZero: true,
        ticks: {
          color: tickColor,
        },
        grid: {
          color: withOpacity(gridColor, 0.5),
          drawTicks: false,
        },
      },
    },
  }
}

function buildPressureDistributionOption(
  language: Language,
  seriesLabel: string,
  buckets: PressureDistributionBucket[],
  color: string,
): PressureDistributionOption {
  const tickColor = readChartColorVar('--dashboard-chart-tick', '#635f69')
  const labelColor = readChartColorVar('--foreground', '#332f3a')
  const gridColor = readChartColorVar('--border', '#d7dfec')

  return {
    animation: false,
    grid: {
      top: 18,
      right: 16,
      bottom: 34,
      left: 18,
      containLabel: true,
    },
    tooltip: {
      trigger: 'axis',
      axisPointer: {
        type: 'line',
        lineStyle: {
          color: withOpacity(color, 0.32),
          width: 1,
        },
      },
      formatter(params) {
        const entry = Array.isArray(params) ? params[0] : params
        const bucket = entry?.data as (PressureDistributionBucket & { value: number }) | undefined
        if (!bucket) return ''
        const userLabel = language === 'zh' ? '位用户' : 'users'
        const successLabel = language === 'zh' ? '成功' : 'S'
        const failureLabel = language === 'zh' ? '失败' : 'F'
        return [
          bucket.rangeLabel,
          `${seriesLabel}: ${formatNumber(language, bucket.value)} ${userLabel}`,
          `${successLabel} ${formatNumber(language, bucket.success)} / ${failureLabel} ${formatNumber(language, bucket.failure)}`,
        ].join('<br/>')
      },
      textStyle: {
        fontFamily: '"DM Sans", system-ui, sans-serif',
      },
    },
    xAxis: {
      type: 'category',
      data: buckets.map((bucket) => bucket.rangeLabel),
      axisTick: {
        alignWithLabel: true,
        lineStyle: {
          color: withOpacity(gridColor, 0.5),
        },
      },
      axisLine: {
        lineStyle: {
          color: withOpacity(gridColor, 0.7),
        },
      },
      axisLabel: {
        color: tickColor,
        interval: 0,
        margin: 10,
        fontSize: 12,
      },
    },
    yAxis: {
      type: 'value',
      minInterval: 1,
      axisLabel: {
        color: tickColor,
        margin: 10,
      },
      axisLine: {
        show: false,
      },
      axisTick: {
        show: false,
      },
      splitLine: {
        lineStyle: {
          color: withOpacity(gridColor, 0.42),
        },
      },
    },
    series: [
      {
        type: 'line',
        name: seriesLabel,
        step: 'end',
        smooth: false,
        symbol: 'circle',
        symbolSize: 7,
        itemStyle: {
          color: withOpacity(color, 0.94),
          borderColor: withOpacity(color, 0.92),
          borderWidth: 1.5,
        },
        lineStyle: {
          color: withOpacity(color, 0.96),
          width: 2.25,
        },
        areaStyle: {
          color: {
            type: 'linear',
            x: 0,
            y: 0,
            x2: 0,
            y2: 1,
            colorStops: [
              { offset: 0, color: withOpacity(color, 0.46) },
              { offset: 1, color: withOpacity(color, 0.1) },
            ],
          },
        },
        emphasis: {
          focus: 'series',
        },
        label: {
          show: true,
          position: 'top',
          color: labelColor,
          fontFamily: '"DM Sans", system-ui, sans-serif',
          fontSize: 12,
          fontWeight: 700,
          formatter(params) {
            const value = typeof params.value === 'number'
              ? params.value
              : typeof (params.value as { value?: number })?.value === 'number'
                ? (params.value as { value: number }).value
                : 0
            return value > 0 ? formatNumber(language, value) : ''
          },
        },
        data: buckets.map((bucket) => ({
          ...bucket,
          value: bucket.total,
        })),
      },
    ],
  }
}

function buildUserDistributionData(
  distribution: AnalysisCurrentUserPressureDistribution,
  language: Language,
): PressureDistributionBucket[] {
  const rows = distribution.rows
  if (rows.length === 0) return []

  const peak = Math.max(...rows.map((row) => row.pressure))
  const desiredBucketCount = Math.min(8, Math.max(4, Math.ceil(Math.log2(rows.length) + 1)))
  const rawBucketSize = Math.max(1, Math.ceil((peak + 1) / desiredBucketCount))
  const bucketSize = chooseNiceStep(rawBucketSize)
  const bucketCount = Math.max(1, Math.ceil((peak + 1) / bucketSize))

  const buckets = Array.from({ length: bucketCount }, (_item, index) => {
    const start = index * bucketSize
    const end = start + bucketSize - 1
    return {
      start,
      end,
      rangeLabel: formatPressureBucketLabel(language, start, end),
      total: 0,
      success: 0,
      failure: 0,
    }
  })

  for (const row of rows) {
    const bucketIndex = Math.min(Math.floor(row.pressure / bucketSize), bucketCount - 1)
    const bucket = buckets[bucketIndex]
    if (!bucket) continue
    bucket.total += 1
    bucket.success += row.successCount
    bucket.failure += row.failureCount
  }

  return buckets
}

export interface PressureAnalysisScreenProps {
  snapshot: AnalysisPressureSnapshot | null
  loading: boolean
  error: string | null
  language: Language
  strings: AdminTranslations['pressure']
  onRetry: () => void
}

export default function PressureAnalysisScreen({
  snapshot,
  loading,
  error,
  language,
  strings,
  onRetry,
}: PressureAnalysisScreenProps): JSX.Element {
  const currentColor = readChartColorVar('--primary', '#7c3aed')
  const previousColor = readChartColorVar('--secondary', '#db2777')
  const hourlyColor = readChartColorVar('--info', '#0ea5e9')

  const current24hLabels = useMemo(
    () => snapshot?.server24h.current.map((point) => String(point.displayBucketStart)) ?? [],
    [snapshot],
  )
  const current24hData = useMemo<ChartData<'line'>>(() => ({
    labels: current24hLabels,
    datasets: [
      {
        label: strings.charts.last24h.currentLabel,
        data: snapshot?.server24h.current.map((point) => point.pressure) ?? [],
        borderColor: currentColor,
        backgroundColor: withOpacity(currentColor, 0.1),
        pointRadius: 0,
        tension: 0.28,
        borderWidth: 2.5,
      },
      {
        label: strings.charts.last24h.previousLabel,
        data: snapshot?.server24h.previous.map((point) => point.pressure) ?? [],
        borderColor: previousColor,
        backgroundColor: withOpacity(previousColor, 0.08),
        pointRadius: 0,
        tension: 0.28,
        borderWidth: 1.75,
        borderDash: [5, 6],
      },
    ],
  }), [current24hLabels, currentColor, previousColor, snapshot, strings.charts.last24h.currentLabel, strings.charts.last24h.previousLabel])

  const server7dLabels = useMemo(
    () => snapshot?.server7d.points.map((point) => String(point.displayBucketStart)) ?? [],
    [snapshot],
  )
  const server7dData = useMemo<ChartData<'line'>>(() => ({
    labels: server7dLabels,
    datasets: [
      {
        label: strings.charts.last7d.seriesLabel,
        data: snapshot?.server7d.points.map((point) => point.pressure) ?? [],
        borderColor: hourlyColor,
        backgroundColor: withOpacity(hourlyColor, 0.1),
        pointRadius: 0,
        tension: 0.24,
        borderWidth: 2.25,
      },
    ],
  }), [hourlyColor, server7dLabels, snapshot, strings.charts.last7d.seriesLabel])

  const distributionData = useMemo(() => buildUserDistributionData(
    snapshot?.currentUserDistribution ?? { windowMinutes: 60, rows: [], summary: {
      activeUsers: 0,
      zeroPressureUsers: 0,
      median: 0,
      p90: 0,
      peak: 0,
      currentPressure: 0,
      vsYesterdayDelta: 0,
    } },
    language,
  ), [language, snapshot])

  const userDistributionOption = useMemo(
    () => buildPressureDistributionOption(
      language,
      strings.charts.userDistribution.seriesLabel,
      distributionData,
      currentColor,
    ),
    [currentColor, distributionData, language, strings.charts.userDistribution.seriesLabel],
  )

  if (loading && !snapshot) {
    return (
      <AdminLoadingRegion
        loadState="initial_loading"
        loadingLabel={strings.loading}
        minHeight={420}
      />
    )
  }

  if (error && !snapshot) {
    return (
      <section className="surface panel pressure-analysis-empty-state" role="alert">
        <h2>{strings.errorTitle}</h2>
        <p className="panel-description">{error}</p>
        <button type="button" className="btn btn-outline" onClick={onRetry}>
          {strings.retry}
        </button>
      </section>
    )
  }

  if (!snapshot) {
    return (
      <section className="surface panel pressure-analysis-empty-state">
        <h2>{strings.emptyTitle}</h2>
        <p className="panel-description">{strings.emptyDescription}</p>
      </section>
    )
  }

  return (
    <div className="pressure-analysis-page" data-testid="pressure-analysis-screen">
      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{strings.charts.last24h.title}</h2>
            <p className="panel-description">{strings.charts.last24h.description}</p>
          </div>
        </div>
        <div className="pressure-chart-shell pressure-chart-shell-line">
          <Line
            data={current24hData}
            options={buildPressureLineOptions(language, (value) => formatAxisTime(language, value))}
          />
        </div>
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{strings.charts.userDistribution.title}</h2>
            <p className="panel-description">{strings.charts.userDistribution.description}</p>
          </div>
        </div>
        {distributionData.length === 0 ? (
          <div className="empty-state alert">{strings.charts.userDistribution.empty}</div>
        ) : (
          <div
            className="pressure-chart-shell pressure-chart-shell-distribution pressure-distribution-histogram"
            data-testid="pressure-distribution-histogram"
          >
            <ReactEChartsCore
              echarts={echarts}
              option={userDistributionOption}
              notMerge
              lazyUpdate
              autoResize
              style={{ height: '100%', width: '100%' }}
              opts={{ renderer: 'canvas' }}
            />
          </div>
        )}
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{strings.charts.last7d.title}</h2>
            <p className="panel-description">{strings.charts.last7d.description}</p>
          </div>
        </div>
        <div className="pressure-chart-shell pressure-chart-shell-line">
          <Line
            data={server7dData}
            options={buildPressureLineOptions(language, (value) => formatAxisHour(language, value))}
          />
        </div>
      </section>
    </div>
  )
}
