import { useEffect, useMemo, useRef, useState } from 'react'

import {
  BarElement,
  CategoryScale,
  Chart as ChartJS,
  Legend,
  LineController,
  LinearScale,
  LineElement,
  PointElement,
  Tooltip,
  type ActiveElement,
  type ChartData,
  type ChartOptions,
  type TooltipModel,
} from 'chart.js'
import { Chart } from 'react-chartjs-2'

import type { AdminUserUsageSeries, AdminUserUsageSeriesKey, AdminUserUsageSeriesPoint } from '../api'
import type { AdminTranslations } from '../i18n'
import SegmentedTabs from '../components/ui/SegmentedTabs'
import { Button } from '../components/ui/button'
import { useTheme } from '../theme'

ChartJS.register(CategoryScale, LinearScale, BarElement, LineController, LineElement, PointElement, Tooltip, Legend)

const USAGE_TAB_ORDER: readonly AdminUserUsageSeriesKey[] = ['rate5m', 'quota1h', 'quota24h', 'quotaMonth']

type LoadStatus = 'idle' | 'loading' | 'success' | 'error'
type TooltipVerticalPlacement = 'top' | 'bottom'
type TooltipHorizontalPlacement = 'left' | 'right'

const HOVER_TOOLTIP_POSITION_STEP = 4

interface SharedUsageTooltipState {
  index: number
  x: number
  y: number
  verticalPlacement: TooltipVerticalPlacement
  horizontalPlacement: TooltipHorizontalPlacement
}

interface UserDetailSharedUsagePanelProps {
  usersStrings: AdminTranslations['users']
  language: string
  loadSeries: (series: AdminUserUsageSeriesKey, signal: AbortSignal) => Promise<AdminUserUsageSeries>
  initialSeries?: AdminUserUsageSeriesKey
  title?: string
  description?: string
}

function readChartColorVar(name: string, fallback: string): string {
  if (typeof document === 'undefined') return fallback
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  return value.length > 0 ? `hsl(${value})` : fallback
}

function formatNumber(locale: string, value: number): string {
  return new Intl.NumberFormat(locale).format(value)
}

function formatBucketAxisLabel(
  locale: string,
  series: AdminUserUsageSeriesKey,
  point: AdminUserUsageSeriesPoint,
): string {
  const date = new Date((point.displayBucketStart ?? point.bucketStart) * 1000)
  if (series === 'quotaMonth') {
    return new Intl.DateTimeFormat(locale, {
      year: '2-digit',
      month: '2-digit',
      timeZone: 'UTC',
    }).format(date)
  }
  if (series === 'quota24h') {
    return new Intl.DateTimeFormat(locale, {
      month: '2-digit',
      day: '2-digit',
      timeZone: 'UTC',
    }).format(date)
  }
  return new Intl.DateTimeFormat(locale, {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  }).format(date)
}

function monthBucketEnd(bucketStart: Date): Date {
  return new Date(Date.UTC(bucketStart.getUTCFullYear(), bucketStart.getUTCMonth() + 1, 1))
}

function bucketDurationSeconds(series: AdminUserUsageSeriesKey, bucketStart: number): number {
  switch (series) {
    case 'rate5m':
      return 5 * 60
    case 'quota1h':
      return 60 * 60
    case 'quota24h':
      return 24 * 60 * 60
    case 'quotaMonth': {
      const start = new Date(bucketStart * 1000)
      return Math.max(1, Math.round((monthBucketEnd(start).getTime() - start.getTime()) / 1000))
    }
  }
}

function formatBucketTooltipLabel(
  locale: string,
  series: AdminUserUsageSeriesKey,
  point: AdminUserUsageSeriesPoint,
): string {
  const displayStart = point.displayBucketStart ?? point.bucketStart
  const start = new Date(displayStart * 1000)
  if (series === 'quotaMonth') {
    return new Intl.DateTimeFormat(locale, {
      year: 'numeric',
      month: 'long',
      timeZone: 'UTC',
    }).format(start)
  }
  if (series === 'quota24h') {
    return new Intl.DateTimeFormat(locale, {
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
      timeZone: 'UTC',
    }).format(start)
  }

  const end = new Date((point.bucketStart + bucketDurationSeconds(series, point.bucketStart) - 1) * 1000)
  const dateLabel = new Intl.DateTimeFormat(locale, {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
  }).format(start)
  const timeLabel = new Intl.DateTimeFormat(locale, {
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  })
  return `${dateLabel} ${timeLabel.format(start)} – ${timeLabel.format(end)}`
}

function axisTickStride(series: AdminUserUsageSeriesKey): number {
  switch (series) {
    case 'rate5m':
      return 24
    case 'quota1h':
      return 6
    case 'quota24h':
      return 1
    case 'quotaMonth':
      return 1
  }
}

function areTooltipStatesEqual(a: SharedUsageTooltipState | null, b: SharedUsageTooltipState | null): boolean {
  if (a === b) return true
  if (!a || !b) return false
  return (
    a.index === b.index &&
    a.x === b.x &&
    a.y === b.y &&
    a.verticalPlacement === b.verticalPlacement &&
    a.horizontalPlacement === b.horizontalPlacement
  )
}

function quantizeHoverCoordinate(value: number): number {
  return Math.round(value / HOVER_TOOLTIP_POSITION_STEP) * HOVER_TOOLTIP_POSITION_STEP
}

function resolveTooltipAnchor(index: number, fallback: { x: number; y: number }): { index: number; x: number; y: number } {
  return {
    index,
    x: fallback.x,
    y: fallback.y,
  }
}

function isTooltipWithinHoverBounds(chart: ChartJS, source: { x: number; y: number }): boolean {
  const chartArea = chart.chartArea
  if (!chartArea) return false
  return (
    Number.isFinite(source.x) &&
    Number.isFinite(source.y) &&
    source.x >= chartArea.left &&
    source.x <= chartArea.right &&
    source.y >= chartArea.top &&
    source.y <= chartArea.bottom
  )
}

function clampTooltipState(
  chart: ChartJS,
  source: { index: number; x: number; y: number },
): SharedUsageTooltipState {
  const width = chart.canvas.clientWidth || chart.width || 320
  const height = chart.canvas.clientHeight || chart.height || 220
  const rawX = source.x
  const rawY = source.y
  const x = Math.round(Math.min(Math.max(rawX, 12), Math.max(12, width - 12)))
  const y = Math.round(Math.min(Math.max(rawY, 12), Math.max(12, height - 12)))
  const horizontalPlacement: TooltipHorizontalPlacement = rawX > width * 0.62 ? 'left' : 'right'
  const verticalPlacement: TooltipVerticalPlacement = rawY < height * 0.42 ? 'bottom' : 'top'
  return {
    index: source.index,
    x,
    y,
    verticalPlacement,
    horizontalPlacement,
  }
}

export function UserDetailSharedUsagePanel({
  usersStrings,
  language,
  loadSeries,
  initialSeries = 'quota1h',
  title,
  description,
}: UserDetailSharedUsagePanelProps): JSX.Element {
  const { resolvedTheme } = useTheme()
  const [activeSeries, setActiveSeries] = useState<AdminUserUsageSeriesKey>(initialSeries)
  const [seriesCache, setSeriesCache] = useState<Partial<Record<AdminUserUsageSeriesKey, AdminUserUsageSeries>>>({})
  const [statusBySeries, setStatusBySeries] = useState<Partial<Record<AdminUserUsageSeriesKey, LoadStatus>>>({})
  const [hoverTooltip, setHoverTooltip] = useState<SharedUsageTooltipState | null>(null)
  const [pinnedTooltip, setPinnedTooltip] = useState<SharedUsageTooltipState | null>(null)
  const chartAreaRef = useRef<HTMLDivElement>(null)
  const loadSeriesRef = useRef(loadSeries)
  const inflightControllersRef = useRef<Partial<Record<AdminUserUsageSeriesKey, AbortController>>>({})
  const currentSeries = seriesCache[activeSeries] ?? null
  const activeStatus = statusBySeries[activeSeries] ?? 'idle'

  useEffect(() => {
    loadSeriesRef.current = loadSeries
  }, [loadSeries])

  useEffect(() => {
    return () => {
      Object.values(inflightControllersRef.current).forEach((controller) => controller?.abort())
      inflightControllersRef.current = {}
    }
  }, [])

  useEffect(() => {
    setHoverTooltip(null)
    setPinnedTooltip(null)
  }, [activeSeries])

  useEffect(() => {
    if (!pinnedTooltip && !hoverTooltip) return
    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target
      if (!(target instanceof Node)) return
      if (chartAreaRef.current?.contains(target)) return
      setPinnedTooltip(null)
      setHoverTooltip(null)
    }
    window.addEventListener('pointerdown', handlePointerDown)
    return () => window.removeEventListener('pointerdown', handlePointerDown)
  }, [hoverTooltip, pinnedTooltip])

  useEffect(() => {
    if (currentSeries) return
    if (activeStatus !== 'idle') return

    const controller = new AbortController()
    inflightControllersRef.current[activeSeries]?.abort()
    inflightControllersRef.current[activeSeries] = controller
    setStatusBySeries((current) => ({ ...current, [activeSeries]: 'loading' }))
    loadSeriesRef.current(activeSeries, controller.signal)
      .then((payload) => {
        if (controller.signal.aborted) return
        if (inflightControllersRef.current[activeSeries] === controller) {
          delete inflightControllersRef.current[activeSeries]
        }
        setSeriesCache((current) => ({ ...current, [activeSeries]: payload }))
        setStatusBySeries((current) => ({ ...current, [activeSeries]: 'success' }))
      })
      .catch((error) => {
        if (inflightControllersRef.current[activeSeries] === controller) {
          delete inflightControllersRef.current[activeSeries]
        }
        if (controller.signal.aborted) return
        console.error('load admin user usage series failed', error)
        setStatusBySeries((current) => ({ ...current, [activeSeries]: 'error' }))
      })
  }, [activeSeries, activeStatus, currentSeries])

  const loadedSeries = useMemo(
    () => USAGE_TAB_ORDER.filter((key) => seriesCache[key] != null),
    [seriesCache],
  )
  const hasPartialHistory = currentSeries?.points.some((point) => point.value == null || point.limitValue == null) ?? false
  const hasRenderablePoints = currentSeries?.points.some((point) => point.value != null || point.limitValue != null) ?? false
  const chartPalette = useMemo(
    () => ({
      bar: readChartColorVar('--primary', '#38bdf8'),
      barBorder: readChartColorVar('--primary', '#0ea5e9'),
      line: readChartColorVar('--warning', '#f59e0b'),
      grid: readChartColorVar('--dashboard-chart-grid', 'rgba(148, 163, 184, 0.18)'),
      tick: readChartColorVar('--dashboard-chart-tick', '#cbd5e1'),
    }),
    [resolvedTheme],
  )

  const activeTooltip = pinnedTooltip ?? hoverTooltip
  const activeTooltipPoint = activeTooltip ? currentSeries?.points[activeTooltip.index] ?? null : null
  const tooltipHasGap = activeTooltipPoint ? activeTooltipPoint.value == null || activeTooltipPoint.limitValue == null : false

  const retryActiveSeries = () => {
    inflightControllersRef.current[activeSeries]?.abort()
    delete inflightControllersRef.current[activeSeries]
    setStatusBySeries((current) => ({ ...current, [activeSeries]: 'idle' }))
    setHoverTooltip(null)
    setPinnedTooltip(null)
  }

  const chartData = useMemo(() => {
    const labels = currentSeries?.points.map((point) => formatBucketAxisLabel(language, activeSeries, point)) ?? []
    return {
      labels,
      datasets: [
        {
          type: 'bar',
          label: usersStrings.detail.sharedUsageLegendUsed,
          data: currentSeries?.points.map((point) => point.value) ?? [],
          backgroundColor: chartPalette.bar,
          borderColor: chartPalette.barBorder,
          borderWidth: 1,
          borderRadius: 6,
          barPercentage: activeSeries === 'quotaMonth' ? 0.62 : 0.72,
          categoryPercentage: activeSeries === 'quotaMonth' ? 0.72 : 0.82,
        },
        {
          type: 'line',
          label: usersStrings.detail.sharedUsageLegendLimit,
          data: currentSeries?.points.map((point) => point.limitValue) ?? [],
          borderColor: chartPalette.line,
          borderWidth: 2,
          borderDash: [8, 6],
          pointRadius: 0,
          pointHoverRadius: 0,
          tension: 0,
        },
      ],
    } as unknown as ChartData<'bar', (number | null)[], string>
  }, [
    activeSeries,
    chartPalette.bar,
    chartPalette.barBorder,
    chartPalette.line,
    currentSeries,
    language,
    usersStrings.detail.sharedUsageLegendLimit,
    usersStrings.detail.sharedUsageLegendUsed,
  ])

  const chartOptions = useMemo(() => {
    const points = currentSeries?.points ?? []
    const stride = axisTickStride(activeSeries)
    return {
      responsive: true,
      maintainAspectRatio: false,
      interaction: { mode: 'index', intersect: false },
      onClick(event, elements: ActiveElement[], chart) {
        const directAnchor = elements.find((item) => item.datasetIndex === 0) ?? elements[0]
        const tooltipModel = chart.tooltip
        const hoveredPoint = tooltipModel?.dataPoints?.[0] ?? null
        const eventX = typeof event.x === 'number' ? event.x : tooltipModel?.caretX
        const eventY = typeof event.y === 'number' ? event.y : tooltipModel?.caretY
        const source = directAnchor
          ? resolveTooltipAnchor(directAnchor.index, {
              x: eventX ?? directAnchor.element.x,
              y: eventY ?? directAnchor.element.y,
            })
          : hoveredPoint && tooltipModel
            ? resolveTooltipAnchor(hoveredPoint.dataIndex, { x: tooltipModel.caretX, y: tooltipModel.caretY })
            : null
        if (!source) {
          setPinnedTooltip(null)
          return
        }
        const nextTooltip = clampTooltipState(chart, source)
        setPinnedTooltip((current) => (current?.index === nextTooltip.index ? null : nextTooltip))
      },
      onHover(event, elements: ActiveElement[], chart) {
        if (pinnedTooltip) return
        const x = typeof event.x === 'number' ? quantizeHoverCoordinate(event.x) : Number.NaN
        const y = typeof event.y === 'number' ? quantizeHoverCoordinate(event.y) : Number.NaN
        const hoverSource = Number.isFinite(x) && Number.isFinite(y) ? { x, y } : null
        if (!hoverSource || !isTooltipWithinHoverBounds(chart, hoverSource)) {
          setHoverTooltip((current) => (current == null ? current : null))
          return
        }
        const directAnchor = elements.find((item) => item.datasetIndex === 0) ?? elements[0]
        const hoveredPoint = chart.tooltip?.dataPoints?.[0] ?? null
        const source = directAnchor
          ? resolveTooltipAnchor(directAnchor.index, hoverSource)
          : hoveredPoint
            ? resolveTooltipAnchor(hoveredPoint.dataIndex, hoverSource)
            : null
        if (!source) {
          setHoverTooltip((current) => (current == null ? current : null))
          return
        }
        const nextTooltip = clampTooltipState(chart, source)
        setHoverTooltip((current) => (areTooltipStatesEqual(current, nextTooltip) ? current : nextTooltip))
      },
      plugins: {
        legend: { display: false },
        tooltip: {
          enabled: false,
          external({ tooltip }: { chart: ChartJS; tooltip: TooltipModel<'bar'> }) {
            if (pinnedTooltip) return
            if (tooltip.opacity === 0) {
              setHoverTooltip((current) => (current == null ? current : null))
            }
          },
        },
      },
      scales: {
        x: {
          grid: { display: false },
          ticks: {
            autoSkip: false,
            maxRotation: 0,
            minRotation: 0,
            color: chartPalette.tick,
            callback(_value, index) {
              if (index === points.length - 1 || index % stride === 0) {
                const label = chartData.labels?.[index]
                return typeof label === 'string' ? label : ''
              }
              return ''
            },
          },
        },
        y: {
          beginAtZero: true,
          grid: { color: chartPalette.grid },
          ticks: {
            color: chartPalette.tick,
            callback(value) {
              return formatNumber(language, Number(value))
            },
          },
        },
      },
    } as ChartOptions<'bar'>
  }, [activeSeries, chartData.labels, chartPalette.grid, chartPalette.tick, currentSeries?.points, language, pinnedTooltip])

  return (
    <div
      className="admin-user-shared-usage-panel"
      data-active-series={activeSeries}
      data-loaded-series={loadedSeries.join(',')}
      data-resolved-theme={resolvedTheme}
      data-tooltip-open={activeTooltip != null ? 'true' : 'false'}
      data-tooltip-pinned={pinnedTooltip != null ? 'true' : 'false'}
    >
      {title || description ? (
        <div className="panel-header admin-user-shared-usage-panel-header">
          <div className="admin-user-shared-usage-heading">
            {title ? <h2>{title}</h2> : null}
            {description ? <p className="panel-description">{description}</p> : null}
          </div>
          <SegmentedTabs<AdminUserUsageSeriesKey>
            value={activeSeries}
            onChange={setActiveSeries}
            options={[
              { value: 'rate5m', label: usersStrings.detail.sharedUsageTabs.fiveMinute },
              { value: 'quota1h', label: usersStrings.detail.sharedUsageTabs.oneHour },
              { value: 'quota24h', label: usersStrings.detail.sharedUsageTabs.daily },
              { value: 'quotaMonth', label: usersStrings.detail.sharedUsageTabs.monthly },
            ]}
            ariaLabel={usersStrings.detail.sharedUsageTitle}
            className="admin-user-shared-usage-tabs"
          />
        </div>
      ) : (
        <div className="admin-user-shared-usage-panel-header">
          <SegmentedTabs<AdminUserUsageSeriesKey>
            value={activeSeries}
            onChange={setActiveSeries}
            options={[
              { value: 'rate5m', label: usersStrings.detail.sharedUsageTabs.fiveMinute },
              { value: 'quota1h', label: usersStrings.detail.sharedUsageTabs.oneHour },
              { value: 'quota24h', label: usersStrings.detail.sharedUsageTabs.daily },
              { value: 'quotaMonth', label: usersStrings.detail.sharedUsageTabs.monthly },
            ]}
            ariaLabel={usersStrings.detail.sharedUsageTitle}
            className="admin-user-shared-usage-tabs"
          />
        </div>
      )}

      <div className="admin-user-shared-usage-meta">
        <div className="admin-user-shared-usage-legend">
          <span className="admin-user-shared-usage-legend-item">
            <span className="admin-user-shared-usage-legend-chip admin-user-shared-usage-legend-chip-bar" />
            {usersStrings.detail.sharedUsageLegendUsed}
          </span>
          <span className="admin-user-shared-usage-legend-item">
            <span className="admin-user-shared-usage-legend-chip admin-user-shared-usage-legend-chip-line" />
            {usersStrings.detail.sharedUsageLegendLimit}
          </span>
        </div>
        {hasPartialHistory ? (
          <span className="panel-description admin-user-shared-usage-hint">
            {usersStrings.detail.sharedUsagePartialHint}
          </span>
        ) : null}
      </div>

      <div
        ref={chartAreaRef}
        className="admin-user-shared-usage-chart"
        onPointerLeave={() => {
          if (pinnedTooltip) return
          setHoverTooltip(null)
        }}
      >
        {(statusBySeries[activeSeries] ?? 'idle') === 'loading' && !currentSeries ? (
          <div className="empty-state alert">{usersStrings.detail.sharedUsageLoading}</div>
        ) : (statusBySeries[activeSeries] ?? 'idle') === 'error' && !currentSeries ? (
          <div className="empty-state alert">
            <div>{usersStrings.detail.sharedUsageLoadFailed}</div>
            <Button
              type="button"
              variant="outline"
              size="xs"
              onClick={retryActiveSeries}
              style={{ marginTop: 12 }}
            >
              {usersStrings.detail.sharedUsageRetryAction}
            </Button>
          </div>
        ) : !hasRenderablePoints ? (
          <div className="empty-state alert">{usersStrings.detail.sharedUsageEmpty}</div>
        ) : (
          <>
            <Chart type="bar" data={chartData} options={chartOptions} />
            {activeTooltip && activeTooltipPoint ? (
              <div
                className="admin-user-shared-usage-tooltip layer-popover"
                data-vertical-placement={activeTooltip.verticalPlacement}
                data-horizontal-placement={activeTooltip.horizontalPlacement}
                data-tooltip-mode={pinnedTooltip ? 'pinned' : 'hover'}
                style={{
                  left: `${activeTooltip.x}px`,
                  top: `${activeTooltip.y}px`,
                }}
              >
                <div className="admin-user-shared-usage-tooltip-header">
                  <strong>{formatBucketTooltipLabel(language, activeSeries, activeTooltipPoint)}</strong>
                </div>
                <dl className="admin-user-shared-usage-tooltip-grid">
                  <div>
                    <dt>{usersStrings.detail.sharedUsageLegendUsed}</dt>
                    <dd>
                      {activeTooltipPoint.value == null ? '—' : formatNumber(language, activeTooltipPoint.value)}
                    </dd>
                  </div>
                  <div>
                    <dt>{usersStrings.detail.sharedUsageLegendLimit}</dt>
                    <dd>
                      {activeTooltipPoint.limitValue == null ? '—' : formatNumber(language, activeTooltipPoint.limitValue)}
                    </dd>
                  </div>
                </dl>
                {tooltipHasGap ? (
                  <p className="admin-user-shared-usage-tooltip-note">{usersStrings.detail.sharedUsagePartialHint}</p>
                ) : null}
              </div>
            ) : null}
          </>
        )}
      </div>
    </div>
  )
}
