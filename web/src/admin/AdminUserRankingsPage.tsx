import { useEffect, useId, useMemo, useRef, useState } from 'react'

import ReactEChartsCore from 'echarts-for-react/lib/core'
import * as echarts from 'echarts/core'
import { BarChart } from 'echarts/charts'
import { GridComponent, TooltipComponent, type GridComponentOption, type TooltipComponentOption } from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'
import type { ComposeOption } from 'echarts/core'
import type { BarSeriesOption } from 'echarts/charts'

import type { AdminTranslations, Language } from '../i18n'
import type { AdminUserRankingRow, AdminUserRankingsSnapshot, AdminUserRankingWindow } from '../api/adminRankings'
import SegmentedTabs, { type SegmentedTabsOption } from '../components/ui/SegmentedTabs'
import { Button } from '../components/ui/button'
import { useViewportMode } from '../lib/responsive'
import { buildRankingMockAvatarDataUrl, normalizeRankingAvatarUrl } from './rankingAvatar'

echarts.use([GridComponent, TooltipComponent, BarChart, CanvasRenderer])

type RankingWindowKey = 'last24h' | 'last7d' | 'last30d'
type RankingsConnectionState = 'connecting' | 'live' | 'degraded'
type EChartsOption = ComposeOption<GridComponentOption | TooltipComponentOption | BarSeriesOption>
type AvatarLoadState = 'loaded' | 'failed'

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

function buildTopBarDomainMax(topValue: number): number {
  if (topValue <= 0) return 1
  return topValue
}

function measureIdentityColumnMetrics({
  rows,
  strings,
  compact,
}: {
  rows: AdminUserRankingRow[]
  strings: AdminTranslations['rankings']
  compact: boolean
}): { totalWidth: number; nameWidth: number } {
  const fallbackTotalWidth = compact ? 168 : 212
  const fallbackNameWidth = compact ? 104 : 138
  if (rows.length === 0 || typeof document === 'undefined') {
    return { totalWidth: fallbackTotalWidth, nameWidth: fallbackNameWidth }
  }

  const probe = document.createElement('span')
  probe.style.position = 'absolute'
  probe.style.visibility = 'hidden'
  probe.style.pointerEvents = 'none'
  probe.style.whiteSpace = 'nowrap'
  probe.style.fontFamily = '"DM Sans", system-ui, sans-serif'
  probe.style.fontSize = compact ? '12px' : '13px'
  probe.style.fontWeight = '700'
  document.body.appendChild(probe)

  try {
    const topRow = rows[0]
    if (!topRow) {
      return { totalWidth: fallbackTotalWidth, nameWidth: fallbackNameWidth }
    }
    probe.textContent = `${topRow.rank}. ${formatDisplayName(topRow, strings.userFallback)}`
    const firstRowTextWidth = Math.ceil(probe.getBoundingClientRect().width)
    const widestTextWidth = rows.reduce((maxWidth, row) => {
      probe.textContent = `${row.rank}. ${formatDisplayName(row, strings.userFallback)}`
      return Math.max(maxWidth, Math.ceil(probe.getBoundingClientRect().width))
    }, firstRowTextWidth)

    const avatarWidth = compact ? 22 : 24
    const avatarGap = compact ? 10 : 12
    const contentPadding = compact ? 12 : 14
    const textBlockWidth = widestTextWidth
    return {
      totalWidth: avatarWidth + avatarGap + textBlockWidth + contentPadding,
      nameWidth: textBlockWidth,
    }
  } finally {
    probe.remove()
  }
}

function formatTimestamp(unixSeconds: number, language: Language): string {
  return new Intl.DateTimeFormat(language === 'zh' ? 'zh-CN' : 'en-US', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(unixSeconds * 1000))
}

function connectionToneClass(state: RankingsConnectionState): string {
  if (state === 'live') return 'is-live'
  if (state === 'degraded') return 'is-degraded'
  return 'is-connecting'
}

function buildIdentityLabel(row: AdminUserRankingRow, fallback: string): string {
  return `{rank|${row.rank}.} {name|${formatDisplayName(row, fallback)}} {avatar_${row.user.userId}| }`
}

type RichTextStyle = {
  width?: number
  height?: number
  align?: 'left' | 'center' | 'right'
  verticalAlign?: 'top' | 'middle' | 'bottom'
  color?: string
  fontWeight?: number | 'normal' | 'bold' | 'bolder' | 'lighter'
  backgroundColor?: string | { image: string }
  borderRadius?: number
  padding?: number | number[]
}

function buildRichStyles(
  rows: AdminUserRankingRow[],
  avatarUrlsByUserId: ReadonlyMap<string, string>,
  nameWidth: number,
): Record<string, RichTextStyle> {
  return rows.reduce<Record<string, RichTextStyle>>((acc, row) => {
    const avatarUrl = avatarUrlsByUserId.get(row.user.userId)
    if (avatarUrl) {
      acc[`avatar_${row.user.userId}`] = {
        width: 22,
        height: 22,
        align: 'center',
        verticalAlign: 'middle',
        backgroundColor: {
          image: avatarUrl,
        },
        borderRadius: 11,
      }
    }
    return acc
  }, {
    rank: {
      width: 26,
      align: 'right',
      color: '#4b4454',
      fontWeight: 700,
      padding: [0, 0, 0, 8],
    },
    name: {
      width: nameWidth,
      align: 'left',
      color: '#332f3a',
      fontWeight: 700,
      padding: [0, 10, 0, 0],
    },
  })
}

function useLoadedAvatarUrls(rows: AdminUserRankingRow[]): ReadonlySet<string> {
  const avatarUrls = useMemo(
    () => Array.from(new Set(rows.map((row) => normalizeRankingAvatarUrl(row.user.avatarUrl)).filter((value): value is string => Boolean(value)))),
    [rows],
  )
  const [avatarStates, setAvatarStates] = useState<Record<string, AvatarLoadState>>({})

  useEffect(() => {
    if (typeof Image === 'undefined') return

    const pendingUrls = avatarUrls.filter((url) => avatarStates[url] === undefined)
    if (pendingUrls.length === 0) return

    let cancelled = false
    const images: HTMLImageElement[] = []

    for (const url of pendingUrls) {
      const image = new Image()
      image.referrerPolicy = 'no-referrer'
      image.onload = () => {
        if (cancelled) return
        setAvatarStates((current) => (current[url] === 'loaded' ? current : { ...current, [url]: 'loaded' }))
      }
      image.onerror = () => {
        if (cancelled) return
        setAvatarStates((current) => (current[url] === 'failed' ? current : { ...current, [url]: 'failed' }))
      }
      image.src = url
      images.push(image)
    }

    return () => {
      cancelled = true
      for (const image of images) {
        image.onload = null
        image.onerror = null
      }
    }
  }, [avatarStates, avatarUrls])

  return useMemo(
    () => new Set(avatarUrls.filter((url) => avatarStates[url] === 'loaded')),
    [avatarStates, avatarUrls],
  )
}

function RankingsSemanticList({
  id,
  title,
  rows,
  strings,
}: {
  id: string
  title: string
  rows: AdminUserRankingRow[]
  strings: AdminTranslations['rankings']
}): JSX.Element {
  return (
    <div id={id} className="sr-only">
      <p>{title}</p>
      <ol>
        {rows.map((row) => (
          <li key={`${row.user.userId}-${row.rank}`}>
            {row.rank}. {formatDisplayName(row, strings.userFallback)}: {row.value.toLocaleString()}
          </li>
        ))}
      </ol>
    </div>
  )
}

function RankingsBarChart({
  rows,
  strings,
  color,
  domainMax,
  descriptionId,
}: {
  rows: AdminUserRankingRow[]
  strings: AdminTranslations['rankings']
  color: string
  domainMax: number
  descriptionId: string
}): JSX.Element {
  const compact = useViewportMode() === 'small'
  const loadedAvatarUrls = useLoadedAvatarUrls(rows)
  const axisColor = readChartColorVar('--foreground', '#332f3a')
  const tickColor = readChartColorVar('--muted-foreground', '#635f69')
  const gridColor = readChartColorVar('--dashboard-chart-grid', 'rgba(148, 163, 184, 0.18)')
  const trackColor = withOpacity(color, 0.14)
  const { totalWidth: identityColumnWidth, nameWidth } = measureIdentityColumnMetrics({ rows, strings, compact })
  const chartPaddingLeft = compact ? 14 : 18
  const axisLabelMargin = compact ? 14 : 18
  const chartHeight = Math.max(320, rows.length * (compact ? 28 : 32) + 48)
  const avatarUrlsByUserId = useMemo(() => new Map(
    rows.map((row) => {
      const realAvatarUrl = normalizeRankingAvatarUrl(row.user.avatarUrl)
      const avatarUrl = realAvatarUrl && loadedAvatarUrls.has(realAvatarUrl)
        ? realAvatarUrl
        : buildRankingMockAvatarDataUrl(row.user, strings.userFallback)
      return [row.user.userId, avatarUrl]
    }),
  ), [loadedAvatarUrls, rows, strings.userFallback])

  const option = useMemo<EChartsOption>(() => ({
    animation: false,
    grid: {
      top: 8,
      bottom: 26,
      left: chartPaddingLeft + identityColumnWidth + axisLabelMargin,
      right: compact ? 54 : 78,
      containLabel: false,
    },
    tooltip: {
      trigger: 'axis',
      axisPointer: { type: 'none' },
      formatter(params) {
        const valueItem = Array.isArray(params)
          ? params.find((item) => item.seriesName === 'value')
          : params
        const index = typeof valueItem?.dataIndex === 'number' ? valueItem.dataIndex : -1
        const row = index >= 0 ? rows[index] : null
        if (!row) return ''
        return `${formatDisplayName(row, strings.userFallback)}<br/>${row.value.toLocaleString()}`
      },
      textStyle: {
        fontFamily: '"DM Sans", system-ui, sans-serif',
      },
    },
    xAxis: {
      type: 'value',
      min: 0,
      max: domainMax,
      splitNumber: 4,
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: {
        color: tickColor,
        fontSize: compact ? 11 : 12,
        fontWeight: 600,
        margin: compact ? 8 : 10,
        formatter(value) {
          return Number(value).toLocaleString()
        },
      },
      splitLine: {
        lineStyle: {
          color: gridColor,
        },
      },
    },
    yAxis: {
      type: 'category',
      inverse: true,
      data: rows.map((row) => buildIdentityLabel(row, strings.userFallback)),
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: {
        color: axisColor,
        fontSize: compact ? 11 : 12,
        margin: axisLabelMargin,
        align: 'right',
        rich: buildRichStyles(rows, avatarUrlsByUserId, nameWidth),
      },
    },
    series: [
      {
        name: 'track',
        type: 'bar',
        silent: true,
        data: rows.map(() => domainMax),
        barWidth: compact ? 18 : 20,
        barGap: '-100%',
        itemStyle: {
          color: trackColor,
          borderRadius: 999,
        },
        z: 1,
      },
      {
        name: 'value',
        type: 'bar',
        data: rows.map((row) => row.value),
        barWidth: compact ? 18 : 20,
        itemStyle: {
          color,
          borderRadius: 999,
        },
        label: {
          show: true,
          position: 'right',
          color: axisColor,
          fontWeight: 700,
          fontSize: compact ? 12 : 13,
          formatter(params) {
            return Number(params.value).toLocaleString()
          },
        },
        z: 2,
      },
    ],
  }), [
    axisColor,
    axisLabelMargin,
    chartPaddingLeft,
    color,
    compact,
    domainMax,
    gridColor,
    identityColumnWidth,
    avatarUrlsByUserId,
    loadedAvatarUrls,
    nameWidth,
    rows,
    strings,
    tickColor,
    trackColor,
  ])

  return (
    <div className="admin-ranking-chart-canvas" style={{ height: chartHeight }} aria-describedby={descriptionId}>
      <ReactEChartsCore
        echarts={echarts}
        option={option}
        notMerge
        lazyUpdate
        autoResize
        style={{ height: '100%', width: '100%' }}
        opts={{ renderer: 'canvas' }}
      />
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
  const descriptionId = useId()
  const domainMax = rows.length > 0 ? buildTopBarDomainMax(rows[0]?.value ?? 0) : 1

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
          <RankingsSemanticList id={descriptionId} title={title} rows={rows} strings={strings} />
          <div className="admin-ranking-chart-shell">
            <RankingsBarChart rows={rows} strings={strings} color={color} domainMax={domainMax} descriptionId={descriptionId} />
          </div>
        </div>
      )}
    </article>
  )
}

function buildWindowOptions(strings: AdminTranslations['rankings']): ReadonlyArray<SegmentedTabsOption<RankingWindowKey>> {
  return [
    { value: 'last24h', label: strings.windows.last24h },
    { value: 'last7d', label: strings.windows.last7d },
    { value: 'last30d', label: strings.windows.last30d },
  ]
}

function statusLabel(strings: AdminTranslations['rankings'], state: RankingsConnectionState): string {
  if (state === 'live') return strings.statusLive
  if (state === 'degraded') return strings.statusDegraded
  return strings.statusConnecting
}

export default function AdminUserRankingsPage({
  strings,
  language,
  snapshot,
  loading,
  error,
  connectionState,
  onRetry,
}: {
  strings: AdminTranslations['rankings']
  language: Language
  snapshot: AdminUserRankingsSnapshot | null
  loading: boolean
  error: string | null
  connectionState: RankingsConnectionState
  onRetry: () => void
}): JSX.Element {
  const [activeWindow, setActiveWindow] = useState<RankingWindowKey>('last24h')
  const pageRef = useRef<HTMLElement | null>(null)
  const primaryColor = readChartColorVar('--dashboard-chart-result-primary-success', '#10b981')
  const creditColor = readChartColorVar('--dashboard-chart-type-api-billable', '#60a5fa')
  const windowOptions = useMemo(() => buildWindowOptions(strings), [strings])

  const activeWindowData: AdminUserRankingWindow | null = snapshot ? snapshot[activeWindow] : null
  const lastUpdated = snapshot ? formatTimestamp(snapshot.generatedAt, language) : null

  return (
    <section ref={pageRef} className="admin-rankings-page">
      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{strings.title}</h2>
            <p className="panel-description">{strings.description}</p>
          </div>
          <div className="admin-rankings-meta">
            {snapshot ? (
              <span className="panel-description">
                {strings.refreshEvery.replace('{seconds}', String(snapshot.refreshIntervalSecs))}
              </span>
            ) : null}
            {lastUpdated ? (
              <span className="panel-description">
                {strings.lastUpdated.replace('{time}', lastUpdated)}
              </span>
            ) : null}
            <span className={`admin-ranking-connection ${connectionToneClass(connectionState)}`}>
              {statusLabel(strings, connectionState)}
            </span>
            {connectionState !== 'live' ? (
              <Button type="button" variant="outline" size="sm" onClick={onRetry}>
                {strings.retry}
              </Button>
            ) : null}
          </div>
        </div>
        {snapshot ? (
          <div className="admin-rankings-toolbar">
            <SegmentedTabs<RankingWindowKey>
              value={activeWindow}
              onChange={setActiveWindow}
              options={windowOptions}
              ariaLabel={strings.windowSelector}
            />
          </div>
        ) : null}
        {loading && !snapshot ? <div className="empty-state alert">{strings.loading}</div> : null}
        {error ? (
          <div className={`alert ${snapshot ? '' : 'alert-error'}`}>
            <div>{error}</div>
            {snapshot ? <div className="admin-ranking-stale-hint">{strings.staleHint}</div> : null}
          </div>
        ) : null}
      </section>

      {snapshot && activeWindowData ? (
        <section className="admin-ranking-window">
          <div className="admin-ranking-window-header">
            <h2>{strings.windows[activeWindow]}</h2>
          </div>
          <div className="admin-ranking-window-grid">
            <RankingsChartCard
              title={strings.primarySuccessTitle}
              description={strings.primarySuccessDescription}
              rows={activeWindowData.primarySuccessTop}
              strings={strings}
              color={primaryColor}
            />
            <RankingsChartCard
              title={strings.businessCreditsTitle}
              description={strings.businessCreditsDescription}
              rows={activeWindowData.businessCreditsTop}
              strings={strings}
              color={creditColor}
            />
          </div>
        </section>
      ) : null}
    </section>
  )
}
