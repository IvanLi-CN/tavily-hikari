import { useEffect, useMemo, useState } from 'react'

import {
  BarElement,
  CategoryScale,
  Chart as ChartJS,
  Legend,
  LinearScale,
  Tooltip,
  type ChartData,
  type ChartOptions,
  type Plugin,
} from 'chart.js'
import { Bar } from 'react-chartjs-2'

import type { AdminTranslations } from '../i18n'
import type { AdminUserRankingRow, AdminUserRankingsSnapshot, AdminUserRankingWindow } from '../api/adminRankings'

ChartJS.register(CategoryScale, LinearScale, BarElement, Tooltip, Legend)

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

function formatDisplayName(row: AdminUserRankingRow, fallback: string): string {
  return row.user.displayName?.trim() || row.user.username?.trim() || row.user.userId || fallback
}

function truncateLabel(text: string, maxLength: number): string {
  if (text.length <= maxLength) return text
  return `${text.slice(0, Math.max(1, maxLength - 1))}…`
}

function estimateTickStep(maxValue: number): number {
  if (maxValue <= 0) return 1
  const rough = maxValue / 4
  const magnitude = 10 ** Math.floor(Math.log10(rough))
  const normalized = rough / magnitude
  if (normalized <= 1) return magnitude
  if (normalized <= 2) return 2 * magnitude
  if (normalized <= 5) return 5 * magnitude
  return 10 * magnitude
}

function buildDomainMax(maxValue: number): number {
  const step = estimateTickStep(maxValue)
  return Math.max(step, Math.ceil(maxValue / step) * step)
}

type RankingChartMeta = {
  rows: AdminUserRankingRow[]
  strings: AdminTranslations['rankings']
  compact: boolean
  axisColor: string
  avatarStrokeColor: string
  labelFont: string
  valueFont: string
}

const rankingChartDecorations: Plugin<'bar'> = {
  id: 'ranking-chart-decorations',
  afterDatasetsDraw(chart, _args, pluginOptions) {
    const meta = pluginOptions as RankingChartMeta | undefined
    if (!meta) return

    const { ctx, chartArea, scales } = chart
    const yScale = scales.y
    const xScale = scales.x
    if (!yScale || !xScale) return

    const avatarSize = meta.compact ? 22 : 24
    const avatarRadius = avatarSize / 2
    const labelX = chartArea.left - (meta.compact ? 104 : 150)
    const avatarX = chartArea.left - (meta.compact ? 124 : 172)
    const maxLabelWidth = chartArea.left - (meta.compact ? 24 : 30) - labelX
    const fallbackInitialFill = 'rgba(124, 58, 237, 0.10)'
    const fallbackInitialText = 'rgba(51, 47, 58, 0.92)'

    ctx.save()
    ctx.textBaseline = 'middle'

    meta.rows.forEach((row, index) => {
      const y = yScale.getPixelForValue(index)
      const rawLabel = `${row.rank}. ${formatDisplayName(row, meta.strings.userFallback)}`
      const label = truncateLabel(rawLabel, meta.compact ? 14 : 22)
      const value = row.value
      const valueText = value.toLocaleString()
      const valueX = Math.min(xScale.getPixelForValue(value) + 10, chartArea.right - 6)
      const initial = formatDisplayName(row, meta.strings.userFallback).trim().charAt(0).toUpperCase() || '?'

      ctx.beginPath()
      ctx.arc(avatarX, y, avatarRadius, 0, Math.PI * 2)
      ctx.fillStyle = fallbackInitialFill
      ctx.fill()
      ctx.lineWidth = 1.5
      ctx.strokeStyle = meta.avatarStrokeColor
      ctx.stroke()

      ctx.fillStyle = fallbackInitialText
      ctx.font = `700 ${Math.max(11, Math.floor(avatarSize * 0.45))}px "DM Sans", system-ui, sans-serif`
      ctx.textAlign = 'center'
      ctx.fillText(initial, avatarX, y + 0.5)

      ctx.fillStyle = meta.axisColor
      ctx.font = meta.labelFont
      ctx.textAlign = 'left'
      let renderedLabel = label
      while (ctx.measureText(renderedLabel).width > maxLabelWidth && renderedLabel.length > 2) {
        renderedLabel = `${renderedLabel.slice(0, -2)}…`
      }
      ctx.fillText(renderedLabel, labelX, y)

      ctx.fillStyle = meta.axisColor
      ctx.font = meta.valueFont
      ctx.textAlign = valueX >= chartArea.right - 10 ? 'right' : 'left'
      ctx.fillText(valueText, valueX, y)
    })

    ctx.restore()
  },
}

function RankingsBarChart({
  rows,
  strings,
  color,
}: {
  rows: AdminUserRankingRow[]
  strings: AdminTranslations['rankings']
  color: string
}): JSX.Element {
  const [compact, setCompact] = useState(typeof window !== 'undefined' ? window.innerWidth <= 640 : false)

  useEffect(() => {
    if (typeof window === 'undefined') return
    const sync = () => setCompact(window.innerWidth <= 640)
    sync()
    window.addEventListener('resize', sync)
    return () => window.removeEventListener('resize', sync)
  }, [])

  const axisColor = readChartColorVar('--foreground', '#332f3a')
  const tickColor = readChartColorVar('--muted-foreground', '#635f69')
  const gridColor = readChartColorVar('--dashboard-chart-grid', 'rgba(148, 163, 184, 0.18)')
  const trackColor = withOpacity(color, 0.14)
  const maxValue = Math.max(...rows.map((row) => row.value), 1)
  const domainMax = buildDomainMax(maxValue)
  const chartHeight = Math.max(320, rows.length * (compact ? 28 : 32) + 48)

  const data = useMemo<ChartData<'bar'>>(
    () => ({
      labels: rows.map((row) => `${row.rank}. ${formatDisplayName(row, strings.userFallback)}`),
      datasets: [
        {
          data: rows.map(() => domainMax),
          backgroundColor: trackColor,
          borderSkipped: false,
          borderRadius: 999,
          grouped: false,
          order: 0,
          barThickness: compact ? 18 : 20,
          categoryPercentage: 0.72,
          barPercentage: 0.84,
        },
        {
          data: rows.map((row) => row.value),
          backgroundColor: color,
          borderSkipped: false,
          borderRadius: 999,
          grouped: false,
          order: 1,
          barThickness: compact ? 18 : 20,
          categoryPercentage: 0.72,
          barPercentage: 0.84,
        },
      ],
    }),
    [color, compact, domainMax, rows, strings.userFallback, trackColor],
  )

  const options = useMemo<ChartOptions<'bar'>>(
    () => ({
      responsive: true,
      maintainAspectRatio: false,
      indexAxis: 'y',
      animation: false,
      layout: {
        padding: {
          top: 8,
          bottom: 8,
          left: compact ? 132 : 186,
          right: compact ? 54 : 78,
        },
      },
      plugins: {
        legend: { display: false },
        tooltip: {
          callbacks: {
            label(context) {
              if (context.datasetIndex !== 1) return ''
              const value = typeof context.parsed.x === 'number' ? context.parsed.x : 0
              return `${value.toLocaleString()}`
            },
          },
          filter(item) {
            return item.datasetIndex === 1
          },
        },
        'ranking-chart-decorations': {
          rows,
          strings,
          compact,
          axisColor,
          avatarStrokeColor: 'rgba(255, 255, 255, 0.96)',
          labelFont: `700 ${compact ? 11 : 12}px "DM Sans", system-ui, sans-serif`,
          valueFont: `700 ${compact ? 12 : 13}px "DM Sans", system-ui, sans-serif`,
        } as RankingChartMeta,
      },
      scales: {
        y: {
          display: false,
          grid: { display: false, drawBorder: false },
          border: { display: false },
        },
        x: {
          min: 0,
          max: domainMax,
          ticks: {
            color: tickColor,
            font: {
              size: compact ? 11 : 12,
              weight: 600,
            },
            callback(value) {
              return Number(value).toLocaleString()
            },
            maxTicksLimit: 4,
          },
          border: { display: false },
          grid: {
            color: gridColor,
            drawBorder: false,
          },
        },
      },
    }),
    [axisColor, compact, domainMax, gridColor, rows, strings, tickColor],
  )

  return (
    <div className="admin-ranking-chart-canvas" style={{ height: chartHeight }}>
      <Bar data={data} options={options} plugins={[rankingChartDecorations]} />
    </div>
  )
}

function RankingsChartCard({
  title,
  description,
  rows,
  strings,
  color,
}: {
  title: string
  description: string
  rows: AdminUserRankingRow[]
  strings: AdminTranslations['rankings']
  color: string
}): JSX.Element {
  return (
    <article className="surface panel admin-ranking-card">
      <div className="panel-header">
        <div>
          <h3>{title}</h3>
          <p className="panel-description">{description}</p>
        </div>
      </div>
      {rows.length === 0 ? (
        <div className="empty-state alert">{strings.empty}</div>
      ) : (
        <div className="admin-ranking-chart-layout">
          <div className="admin-ranking-chart-shell">
            <RankingsBarChart rows={rows} strings={strings} color={color} />
          </div>
        </div>
      )}
    </article>
  )
}

function RankingsWindowSection({
  title,
  window,
  strings,
}: {
  title: string
  window: AdminUserRankingWindow
  strings: AdminTranslations['rankings']
}): JSX.Element {
  const primaryColor = readChartColorVar('--dashboard-chart-result-primary-success', '#10b981')
  const creditColor = readChartColorVar('--dashboard-chart-type-api-billable', '#60a5fa')

  return (
    <section className="admin-ranking-window">
      <div className="admin-ranking-window-header">
        <h2>{title}</h2>
      </div>
      <div className="admin-ranking-window-grid">
        <RankingsChartCard
          title={strings.primarySuccessTitle}
          description={strings.primarySuccessDescription}
          rows={window.primarySuccessTop}
          strings={strings}
          color={primaryColor}
        />
        <RankingsChartCard
          title={strings.businessCreditsTitle}
          description={strings.businessCreditsDescription}
          rows={window.businessCreditsTop}
          strings={strings}
          color={creditColor}
        />
      </div>
    </section>
  )
}

export default function AdminUserRankingsPage({
  strings,
  snapshot,
  loading,
  error,
}: {
  strings: AdminTranslations['rankings']
  snapshot: AdminUserRankingsSnapshot | null
  loading: boolean
  error: string | null
}): JSX.Element {
  return (
    <section className="admin-rankings-page">
      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{strings.title}</h2>
            <p className="panel-description">{strings.description}</p>
          </div>
          {snapshot ? (
            <div className="panel-description">
              {strings.refreshEvery.replace('{seconds}', String(snapshot.refreshIntervalSecs))}
            </div>
          ) : null}
        </div>
        {loading && !snapshot ? <div className="empty-state alert">{strings.loading}</div> : null}
        {error ? <div className="alert alert-error">{error}</div> : null}
      </section>

      {snapshot ? (
        <>
          <RankingsWindowSection title={strings.windows.last24h} window={snapshot.last24h} strings={strings} />
          <RankingsWindowSection title={strings.windows.last7d} window={snapshot.last7d} strings={strings} />
          <RankingsWindowSection title={strings.windows.last30d} window={snapshot.last30d} strings={strings} />
        </>
      ) : null}
    </section>
  )
}
