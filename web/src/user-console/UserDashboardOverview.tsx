import type { ReactNode } from 'react'

import type {
  UserDashboardOverview,
  UserDashboardOverviewSeriesPoint,
  UserDashboardProgressCard,
} from '../api'
import { UsageMetricLabel } from '../components/UsageMetricLabel'
import type { Language } from '../i18n'

interface UserDashboardOverviewText {
  usage: string
  description: string
  dailySuccess: string
  dailyFailure: string
  monthlySuccessUtc: string
  hourly: string
  daily: string
  monthly: string
}

interface UserDashboardOverviewProps {
  text: UserDashboardOverviewText
  overview: UserDashboardOverview | null
  loading: boolean
  language: Language
  requestRateLabel: string
  formatNumber: (value: number) => string
}

interface ChartSegment {
  areaPath: string
  linePath: string
  lastPoint: { x: number, y: number } | null
}

interface ChartGeometry {
  actualSegments: ChartSegment[]
  limitPaths: string[]
  width: number
  height: number
  hasData: boolean
}

const CHART_WIDTH = 320
const CHART_HEIGHT = 148
const CHART_INSET_TOP = 10
const CHART_INSET_RIGHT = 10
const CHART_INSET_BOTTOM = 10
const CHART_INSET_LEFT = 10

function chartPathForSegment(
  points: Array<{ x: number, y: number }>,
  baselineY: number,
): ChartSegment | null {
  if (points.length === 0) return null
  const linePath = points
    .map((point, index) => `${index === 0 ? 'M' : 'L'} ${point.x.toFixed(2)} ${point.y.toFixed(2)}`)
    .join(' ')
  const areaPath = `${linePath} L ${points[points.length - 1].x.toFixed(2)} ${baselineY.toFixed(2)} L ${points[0].x.toFixed(2)} ${baselineY.toFixed(2)} Z`
  return {
    areaPath,
    linePath,
    lastPoint: points[points.length - 1] ?? null,
  }
}

function buildSeriesPaths(
  points: UserDashboardOverviewSeriesPoint[],
  pickValue: (point: UserDashboardOverviewSeriesPoint) => number | null,
): string[] {
  if (points.length === 0) return []
  const maxValue = Math.max(
    1,
    ...points.flatMap((point) => {
      const value = pickValue(point)
      return typeof value === 'number' && Number.isFinite(value) ? [value] : []
    }),
  )
  const plotWidth = CHART_WIDTH - CHART_INSET_LEFT - CHART_INSET_RIGHT
  const plotHeight = CHART_HEIGHT - CHART_INSET_TOP - CHART_INSET_BOTTOM
  const xStep = points.length > 1 ? plotWidth / (points.length - 1) : 0
  const segments: string[] = []
  let currentSegment: Array<{ x: number, y: number }> = []

  points.forEach((point, index) => {
    const value = pickValue(point)
    if (value == null) {
      if (currentSegment.length > 1) {
        segments.push(
          currentSegment
            .map((segmentPoint, segmentIndex) => `${segmentIndex === 0 ? 'M' : 'L'} ${segmentPoint.x.toFixed(2)} ${segmentPoint.y.toFixed(2)}`)
            .join(' '),
        )
      }
      currentSegment = []
      return
    }
    const x = CHART_INSET_LEFT + xStep * index
    const y = CHART_INSET_TOP + (1 - value / maxValue) * plotHeight
    currentSegment.push({ x, y })
  })

  if (currentSegment.length > 1) {
    segments.push(
      currentSegment
        .map((segmentPoint, segmentIndex) => `${segmentIndex === 0 ? 'M' : 'L'} ${segmentPoint.x.toFixed(2)} ${segmentPoint.y.toFixed(2)}`)
        .join(' '),
    )
  }

  return segments
}

function buildChartGeometry(points: UserDashboardOverviewSeriesPoint[]): ChartGeometry {
  if (points.length === 0) {
    return {
      actualSegments: [],
      limitPaths: [],
      width: CHART_WIDTH,
      height: CHART_HEIGHT,
      hasData: false,
    }
  }

  const maxValue = Math.max(
    1,
    ...points.flatMap((point) => {
      const out: number[] = []
      if (typeof point.value === 'number' && Number.isFinite(point.value)) out.push(point.value)
      if (typeof point.limitValue === 'number' && Number.isFinite(point.limitValue)) out.push(point.limitValue)
      return out
    }),
  )
  const plotWidth = CHART_WIDTH - CHART_INSET_LEFT - CHART_INSET_RIGHT
  const plotHeight = CHART_HEIGHT - CHART_INSET_TOP - CHART_INSET_BOTTOM
  const baselineY = CHART_HEIGHT - CHART_INSET_BOTTOM
  const xStep = points.length > 1 ? plotWidth / (points.length - 1) : 0
  const actualSegments: ChartSegment[] = []
  let currentActualSegment: Array<{ x: number, y: number }> = []

  points.forEach((point, index) => {
    if (point.value == null) {
      const segment = chartPathForSegment(currentActualSegment, baselineY)
      if (segment) actualSegments.push(segment)
      currentActualSegment = []
      return
    }

    const x = CHART_INSET_LEFT + xStep * index
    const y = CHART_INSET_TOP + (1 - point.value / maxValue) * plotHeight
    currentActualSegment.push({ x, y })
  })

  const tailSegment = chartPathForSegment(currentActualSegment, baselineY)
  if (tailSegment) actualSegments.push(tailSegment)

  return {
    actualSegments,
    limitPaths: buildSeriesPaths(points, (point) => point.limitValue),
    width: CHART_WIDTH,
    height: CHART_HEIGHT,
    hasData: actualSegments.length > 0,
  }
}

function ProgressChart({
  card,
  accentId,
}: {
  card: UserDashboardProgressCard | null
  accentId: string
}): JSX.Element {
  if (!card) {
    return <div className="user-console-progress-chart user-console-progress-chart-empty" aria-hidden="true" />
  }

  const geometry = buildChartGeometry(card.points)

  if (!geometry.hasData && geometry.limitPaths.length === 0) {
    return <div className="user-console-progress-chart user-console-progress-chart-empty" aria-hidden="true" />
  }

  const lastPoint = geometry.actualSegments[geometry.actualSegments.length - 1]?.lastPoint ?? null

  return (
    <div className="user-console-progress-chart" aria-hidden="true">
      <svg
        className="user-console-progress-chart-svg"
        viewBox={`0 0 ${geometry.width} ${geometry.height}`}
        preserveAspectRatio="none"
        data-accent={accentId}
      >
        <defs>
          <linearGradient id={`user-console-${accentId}-area`} x1="0%" x2="0%" y1="0%" y2="100%">
            <stop offset="0%" stopColor="currentColor" stopOpacity="0.24" />
            <stop offset="85%" stopColor="currentColor" stopOpacity="0.02" />
          </linearGradient>
        </defs>
        {geometry.limitPaths.map((path, index) => (
          <path
            key={`limit-${index}`}
            d={path}
            className="user-console-progress-limit-path"
          />
        ))}
        {geometry.actualSegments.map((segment, index) => (
          <g key={`actual-${index}`}>
            <path d={segment.areaPath} fill={`url(#user-console-${accentId}-area)`} />
            <path d={segment.linePath} className="user-console-progress-line-path" />
          </g>
        ))}
        {lastPoint ? (
          <circle
            cx={lastPoint.x}
            cy={lastPoint.y}
            r="4"
            className="user-console-progress-line-cap"
          />
        ) : null}
      </svg>
    </div>
  )
}

function SummaryCard({
  label,
  value,
  loading,
  marker,
  tone,
  formatNumber,
}: {
  label: string
  value: number
  loading: boolean
  marker: string
  tone: 'success' | 'failure' | 'month'
  formatNumber: (value: number) => string
}): JSX.Element {
  return (
    <article className={`user-console-summary-card user-console-summary-card-${tone}`}>
      <div className="user-console-summary-card-header">
        <span className="user-console-summary-card-label">{label}</span>
        <span className="user-console-summary-card-marker">{marker}</span>
      </div>
      <div className="user-console-summary-card-value">
        <span>{loading ? '--' : formatNumber(value)}</span>
      </div>
      <div className="user-console-summary-card-foot">
        <span>{marker}</span>
      </div>
    </article>
  )
}

function ProgressCard({
  label,
  card,
  loading,
  accent,
  marker,
  formatNumber,
}: {
  label: ReactNode
  card: UserDashboardProgressCard | null
  loading: boolean
  accent: 'request' | 'hour' | 'day' | 'month'
  marker: string
  formatNumber: (value: number) => string
}): JSX.Element {
  const fillRatio = !loading && card && card.limit > 0
    ? Math.max(0, Math.min(1, card.used / card.limit))
    : null

  return (
    <article className={`user-console-progress-card user-console-progress-card-${accent}${loading ? ' is-loading' : ''}`}>
      <div className="user-console-progress-card-copy">
        <div className="user-console-progress-card-header">
          <span className="user-console-progress-card-label">{label}</span>
          <span className="user-console-progress-card-marker">{marker}</span>
        </div>
        <div className="user-console-progress-card-value">
          <strong>{loading || !card ? '--' : formatNumber(card.used)}</strong>
          <span>{loading || !card ? '/ --' : `/ ${formatNumber(card.limit)}`}</span>
        </div>
        <div className="user-console-progress-card-foot">
          <span>{marker}</span>
          <strong>{fillRatio == null ? '--' : `${Math.round(fillRatio * 100)}%`}</strong>
        </div>
      </div>
      <ProgressChart card={card} accentId={accent} />
    </article>
  )
}

export default function UserDashboardOverview({
  text,
  overview,
  loading,
  language,
  requestRateLabel,
  formatNumber,
}: UserDashboardOverviewProps): JSX.Element {
  const summary = overview?.summary ?? null
  const progress = overview?.progress ?? null
  const markerText = language === 'zh'
    ? {
        today: '今日',
        monthUtc: 'UTC 月',
        rolling: '滚动 5 分钟',
        hour: '当前小时',
        day: '当前自然日',
      }
    : {
        today: 'Today',
        monthUtc: 'UTC month',
        rolling: 'Rolling 5m',
        hour: 'Current hour',
        day: 'Current day',
      }

  return (
    <div className="user-console-overview-grid">
      <div className="user-console-summary-grid">
        <SummaryCard
          label={text.dailySuccess}
          value={summary?.dailySuccess ?? 0}
          loading={loading}
          marker={markerText.today}
          tone="success"
          formatNumber={formatNumber}
        />
        <SummaryCard
          label={text.dailyFailure}
          value={summary?.dailyFailure ?? 0}
          loading={loading}
          marker={markerText.today}
          tone="failure"
          formatNumber={formatNumber}
        />
        <SummaryCard
          label={text.monthlySuccessUtc}
          value={summary?.monthlySuccess ?? 0}
          loading={loading}
          marker={markerText.monthUtc}
          tone="month"
          formatNumber={formatNumber}
        />
      </div>

      <div className="user-console-progress-grid">
        <ProgressCard
          label={requestRateLabel}
          card={progress?.requestRate ?? null}
          loading={loading}
          accent="request"
          marker={markerText.rolling}
          formatNumber={formatNumber}
        />
        <ProgressCard
          label={
            <UsageMetricLabel
              label={text.hourly}
              kind="businessCalls1h"
              language={language}
              className="user-console-progress-card-label"
            />
          }
          card={progress?.quotaHourly ?? null}
          loading={loading}
          accent="hour"
          marker={markerText.hour}
          formatNumber={formatNumber}
        />
        <ProgressCard
          label={
            <UsageMetricLabel
              label={text.daily}
              kind="dailyCredits"
              language={language}
              className="user-console-progress-card-label"
            />
          }
          card={progress?.quotaDaily ?? null}
          loading={loading}
          accent="day"
          marker={markerText.day}
          formatNumber={formatNumber}
        />
        <ProgressCard
          label={
            <UsageMetricLabel
              label={text.monthly}
              kind="monthlyCredits"
              language={language}
              className="user-console-progress-card-label"
            />
          }
          card={progress?.quotaMonthly ?? null}
          loading={loading}
          accent="month"
          marker={markerText.monthUtc}
          formatNumber={formatNumber}
        />
      </div>
    </div>
  )
}
