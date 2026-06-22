import { useEffect, useId, useMemo, useRef, useState } from 'react'

import ReactEChartsCore from 'echarts-for-react/lib/core'
import * as echarts from 'echarts/core'
import { CustomChart, type CustomSeriesOption } from 'echarts/charts'
import { TooltipComponent, type TooltipComponentOption } from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'
import type { ComposeOption } from 'echarts/core'

import type { AdminTranslations, Language } from '../i18n'
import type { AdminUserRankingRow, AdminUserRankingsSnapshot } from '../api/adminRankings'
import { Icon } from '../lib/icons'
import { useViewportMode } from '../lib/responsive'
import { buildRankingMockAvatarDataUrl, normalizeRankingAvatarUrl } from './rankingAvatar'

echarts.use([TooltipComponent, CustomChart, CanvasRenderer])

type RankingWindowKey = 'last24h' | 'last7d' | 'last30d'
type RankingMetricKey = 'primarySuccess' | 'businessCredits' | 'uniqueIp'
type RankingTabKey = RankingWindowKey | RankingMetricKey
type RankingsConnectionState = 'connecting' | 'live' | 'degraded'
type EChartsOption = ComposeOption<TooltipComponentOption | CustomSeriesOption>
type AvatarLoadState = 'loaded' | 'failed'
const DEFAULT_RANKINGS_REFRESH_INTERVAL_SECS = 10
const DESKTOP_RANKING_ROW_HEIGHT = 32
const MOBILE_RANKING_ROW_HEIGHT = 28
const RANKING_CHART_BASE_HEIGHT = 48
const RANKING_SLOT_COUNT = 20
type RankingCardDefinition = {
  key: string
  title: string
  description: string
  rows: AdminUserRankingRow[]
  color: string
}

type RankingsMetaProps = {
  strings: AdminTranslations['rankings']
  snapshot: AdminUserRankingsSnapshot | null
  connectionState: RankingsConnectionState
  language: Language
}

export type { RankingMetricKey, RankingTabKey, RankingWindowKey }

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

function rankingChartHeight(rowCount: number, compact: boolean): number {
  const clampedRowCount = Math.max(rowCount, RANKING_SLOT_COUNT)
  const rowHeight = compact ? MOBILE_RANKING_ROW_HEIGHT : DESKTOP_RANKING_ROW_HEIGHT
  return Math.max(320, clampedRowCount * rowHeight + RANKING_CHART_BASE_HEIGHT)
}

function connectionToneClass(state: RankingsConnectionState): string {
  if (state === 'live') return 'is-live'
  if (state === 'degraded') return 'is-degraded'
  return 'is-connecting'
}

function connectionIcon(state: RankingsConnectionState): string {
  if (state === 'live') return 'mdi:check-circle-outline'
  if (state === 'degraded') return 'mdi:alert-circle-outline'
  return 'mdi:loading'
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
  const { totalWidth: measuredIdentityWidth, nameWidth: measuredNameWidth } = measureIdentityColumnMetrics({
    rows,
    strings,
    compact,
  })
  const chartPaddingLeft = compact ? 14 : 18
  const chartPaddingTop = compact ? 10 : 12
  const chartPaddingBottom = compact ? 10 : 12
  const chartPaddingRight = compact ? 12 : 16
  const valueLabelWidth = compact ? 34 : 42
  const valueLabelGap = compact ? 8 : 10
  const barHeight = compact ? 22 : 24
  const avatarSize = compact ? 20 : 22
  const rankWidth = compact ? 24 : 26
  const rowLabelGap = compact ? 8 : 10
  const rowNameFontSize = compact ? 12 : 13
  const rowValueFontSize = compact ? 12 : 13
  const rowRankFontSize = compact ? 11 : 12
  const chartHeight = rankingChartHeight(rows.length, compact)
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
    tooltip: {
      trigger: 'item',
      formatter(params) {
        const index = Array.isArray(params)
          ? -1
          : typeof params.dataIndex === 'number'
            ? params.dataIndex
            : -1
        const row = index >= 0 ? rows[index] : null
        if (!row) return ''
        return `${formatDisplayName(row, strings.userFallback)}<br/>${row.value.toLocaleString()}`
      },
      textStyle: {
        fontFamily: '"DM Sans", system-ui, sans-serif',
      },
    },
    series: [
      {
        type: 'custom',
        coordinateSystem: 'none',
        data: rows.map((row) => row.value),
        renderItem(params, api) {
          const row = rows[params.dataIndexInside]
          if (!row) return

          const fullWidth = api.getWidth()
          const fullHeight = api.getHeight()
          const slotHeight = Math.max(
            compact ? MOBILE_RANKING_ROW_HEIGHT : DESKTOP_RANKING_ROW_HEIGHT,
            (fullHeight - chartPaddingTop - chartPaddingBottom) / RANKING_SLOT_COUNT,
          )
          const centerY = chartPaddingTop + params.dataIndexInside * slotHeight + slotHeight / 2
          const barY = centerY - barHeight / 2
          const plotWidth = Math.max(144, fullWidth - chartPaddingLeft - chartPaddingRight)
          const labelBarWidth = Math.min(
            Math.max(104, rankWidth + avatarSize + rowLabelGap * 3 + (compact ? 54 : 66)),
            Math.max(120, plotWidth * 0.42),
          )
          const variableBarWidth = Math.max(36, plotWidth - labelBarWidth - valueLabelGap - valueLabelWidth)
          const valueRatio = domainMax > 0 ? row.value / domainMax : 0
          const barWidth = labelBarWidth + variableBarWidth * valueRatio
          const maxNameWidth = Math.max(
            compact ? 52 : 64,
            Math.min(measuredNameWidth, labelBarWidth - rankWidth - avatarSize - rowLabelGap * 3 - 12),
          )
          const valueText = row.value.toLocaleString()
          const canShowValueInside = barWidth >= labelBarWidth + valueLabelWidth + 14
          const valueAnchorX = canShowValueInside
            ? chartPaddingLeft + barWidth - 10
            : Math.min(fullWidth - chartPaddingRight, chartPaddingLeft + barWidth + valueLabelGap)
          const avatarX = chartPaddingLeft + rankWidth + rowLabelGap
          const avatarY = centerY - avatarSize / 2
          const avatarUrl =
            avatarUrlsByUserId.get(row.user.userId) ??
            buildRankingMockAvatarDataUrl(row.user, strings.userFallback)

          return {
            type: 'group',
            focus: 'none',
            emphasisDisabled: true,
            children: [
              {
                type: 'rect',
                shape: {
                  x: chartPaddingLeft,
                  y: barY,
                  width: barWidth,
                  height: barHeight,
                  r: [8, 999, 999, 8],
                },
                style: { fill: color },
                silent: true,
              },
              {
                type: 'text',
                style: {
                  x: chartPaddingLeft + 10,
                  y: centerY,
                  text: `${row.rank}.`,
                  fill: 'rgba(255, 255, 255, 0.94)',
                  font: api.font({
                    fontSize: rowRankFontSize,
                    fontWeight: 800,
                    fontFamily: '"DM Sans", system-ui, sans-serif',
                  }),
                  textAlign: 'left',
                  textVerticalAlign: 'middle',
                },
                silent: true,
              },
              {
                type: 'group',
                x: avatarX,
                y: avatarY,
                clipPath: {
                  type: 'circle',
                  shape: {
                    cx: avatarSize / 2,
                    cy: avatarSize / 2,
                    r: avatarSize / 2,
                  },
                },
                silent: true,
                children: [
                  {
                    type: 'image',
                    style: {
                      image: avatarUrl,
                      x: 0,
                      y: 0,
                      width: avatarSize,
                      height: avatarSize,
                    },
                  },
                  {
                    type: 'circle',
                    shape: {
                      cx: avatarSize / 2,
                      cy: avatarSize / 2,
                      r: avatarSize / 2 - 0.5,
                    },
                    style: {
                      fill: 'transparent',
                      stroke: 'rgba(255, 255, 255, 0.78)',
                      lineWidth: 1,
                    },
                    silent: true,
                  },
                ],
              },
              {
                type: 'text',
                style: {
                  x: avatarX + avatarSize + rowLabelGap,
                  y: centerY,
                  width: maxNameWidth,
                  text: formatDisplayName(row, strings.userFallback),
                  overflow: 'truncate',
                  ellipsis: '…',
                  fill: 'rgba(255, 255, 255, 0.96)',
                  font: api.font({
                    fontSize: rowNameFontSize,
                    fontWeight: 800,
                    fontFamily: '"DM Sans", system-ui, sans-serif',
                  }),
                  textAlign: 'left',
                  textVerticalAlign: 'middle',
                },
                silent: true,
              },
              {
                type: 'text',
                style: {
                  x: valueAnchorX,
                  y: centerY,
                  text: valueText,
                  fill: canShowValueInside ? 'rgba(255, 255, 255, 0.98)' : axisColor,
                  font: api.font({
                    fontSize: rowValueFontSize,
                    fontWeight: 800,
                    fontFamily: '"DM Sans", system-ui, sans-serif',
                  }),
                  textAlign: canShowValueInside ? 'right' : 'left',
                  textVerticalAlign: 'middle',
                },
                silent: true,
              },
            ],
          }
        },
      },
    ],
  }), [
    axisColor,
    avatarSize,
    avatarUrlsByUserId,
    barHeight,
    chartPaddingBottom,
    chartPaddingLeft,
    chartPaddingRight,
    chartPaddingTop,
    color,
    compact,
    domainMax,
    measuredIdentityWidth,
    measuredNameWidth,
    rankWidth,
    rowLabelGap,
    rowNameFontSize,
    rowRankFontSize,
    rowValueFontSize,
    rows,
    strings,
    valueLabelGap,
    valueLabelWidth,
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
      <div className="admin-ranking-card-body">
        {rows.length === 0 ? (
          <div className="admin-ranking-empty-state" role="status" aria-live="polite">
            <div className="admin-ranking-empty-orb" aria-hidden="true">
              <Icon icon="mdi:chart-box-outline" width={24} height={24} />
            </div>
            <p className="admin-ranking-empty-copy">{strings.empty}</p>
            <div className="admin-ranking-empty-ghostbars" aria-hidden="true">
              <span className="admin-ranking-empty-ghostbar admin-ranking-empty-ghostbar--long" />
              <span className="admin-ranking-empty-ghostbar admin-ranking-empty-ghostbar--mid" />
              <span className="admin-ranking-empty-ghostbar admin-ranking-empty-ghostbar--short" />
            </div>
          </div>
        ) : (
          <div className="admin-ranking-chart-layout">
            <RankingsSemanticList id={descriptionId} title={title} rows={rows} strings={strings} />
            <div className="admin-ranking-chart-shell">
              <RankingsBarChart rows={rows} strings={strings} color={color} domainMax={domainMax} descriptionId={descriptionId} />
            </div>
          </div>
        )}
      </div>
    </article>
  )
}

function RankingsLoadingCard({
  title,
  description,
  strings,
}: {
  title: string
  description: string
  strings: AdminTranslations['rankings']
}): JSX.Element {
  const compact = useViewportMode() === 'small'
  const chartHeight = rankingChartHeight(RANKING_SLOT_COUNT, compact)
  const skeletonRows = Array.from({ length: RANKING_SLOT_COUNT }, (_, index) => ({
    rank: index + 1,
    nameWidth: `${44 + ((index * 7) % 32)}%`,
    barWidth: `${Math.max(18, 100 - index * 3.6)}%`,
  }))

  return (
    <article className="surface panel admin-ranking-card">
      <div className="panel-header">
        <div>
          <h3>{title}</h3>
          <p className="panel-description">{description}</p>
        </div>
      </div>
      <div className="admin-ranking-card-body">
        <div
          className="admin-ranking-skeleton-stage"
          role="status"
          aria-live="polite"
          style={{ minHeight: chartHeight, height: chartHeight }}
        >
          <span className="sr-only">{strings.loading}</span>
          <div className="admin-ranking-skeleton-list" aria-hidden="true">
            {skeletonRows.map((row) => (
              <div key={`${title}-${row.rank}`} className="admin-ranking-skeleton-item">
                <span className="admin-ranking-skeleton-rank">{row.rank}.</span>
                <span className="admin-ranking-skeleton-avatar" />
                <span className="admin-ranking-skeleton-name" style={{ width: row.nameWidth }} />
                <span className="admin-ranking-skeleton-track">
                  <span className="admin-ranking-skeleton-bar" style={{ width: row.barWidth }} />
                </span>
              </div>
            ))}
          </div>
        </div>
      </div>
    </article>
  )
}

function buildLoadingCards(strings: AdminTranslations['rankings']): Array<Pick<RankingCardDefinition, 'key' | 'title' | 'description'>> {
  return [
    {
      key: 'loading-primary-success',
      title: strings.metrics.primarySuccess,
      description: strings.primarySuccessDescription,
    },
    {
      key: 'loading-business-credits',
      title: strings.metrics.businessCredits,
      description: strings.businessCreditsDescription,
    },
    {
      key: 'loading-unique-ip',
      title: strings.metrics.uniqueIp,
      description: strings.uniqueIpDescription,
    },
  ]
}

function isWindowTab(value: RankingTabKey): value is RankingWindowKey {
  return value === 'last24h' || value === 'last7d' || value === 'last30d'
}

function defaultWindowForTab(value: RankingTabKey): RankingWindowKey {
  return isWindowTab(value) ? value : 'last24h'
}

function buildTabLabel(strings: AdminTranslations['rankings'], value: RankingTabKey): string {
  if (value === 'last24h' || value === 'last7d' || value === 'last30d') {
    return strings.windows[value]
  }

  return strings.metrics[value]
}

function buildRankingCards({
  selectedWindow,
  snapshot,
  strings,
  primaryColor,
  creditColor,
  uniqueIpColor,
}: {
  selectedWindow: RankingWindowKey
  snapshot: AdminUserRankingsSnapshot
  strings: AdminTranslations['rankings']
  primaryColor: string
  creditColor: string
  uniqueIpColor: string
}): RankingCardDefinition[] {
  const windowData = snapshot[selectedWindow]
  return [
    {
      key: `${selectedWindow}-primary-success`,
      title: strings.metrics.primarySuccess,
      description: strings.primarySuccessDescription,
      rows: windowData.primarySuccessTop,
      color: primaryColor,
    },
    {
      key: `${selectedWindow}-business-credits`,
      title: strings.metrics.businessCredits,
      description: strings.businessCreditsDescription,
      rows: windowData.businessCreditsTop,
      color: creditColor,
    },
    {
      key: `${selectedWindow}-unique-ip`,
      title: strings.metrics.uniqueIp,
      description: strings.uniqueIpDescription,
      rows: windowData.uniqueIpTop,
      color: uniqueIpColor,
    },
  ]
}

function statusLabel(strings: AdminTranslations['rankings'], state: RankingsConnectionState): string {
  if (state === 'live') return strings.statusLive
  if (state === 'degraded') return strings.statusDegraded
  return strings.statusConnecting
}

export function RankingsMeta({
  strings,
  snapshot,
  connectionState,
  language,
}: RankingsMetaProps): JSX.Element {
  const lastUpdated = snapshot ? formatTimestamp(snapshot.generatedAt, language) : null
  const refreshCopy = strings.refreshEvery.replace(
    '{seconds}',
    String(snapshot?.refreshIntervalSecs ?? DEFAULT_RANKINGS_REFRESH_INTERVAL_SECS),
  )
  const updatedCopy = lastUpdated
    ? strings.lastUpdated.replace('{time}', lastUpdated)
    : null
  const pendingCopy = !snapshot ? strings.awaitingFirstSnapshot : null

  return (
    <div className="admin-rankings-meta" aria-live="polite">
      <span className="admin-rankings-meta-item">
        <Icon icon="mdi:refresh" width={16} height={16} className="admin-rankings-meta-icon" aria-hidden="true" />
        <span className="admin-rankings-meta-copy">{refreshCopy}</span>
      </span>
      {updatedCopy || pendingCopy ? (
        <span className="admin-rankings-meta-item">
          <Icon
            icon="mdi:clock-time-four-outline"
            width={16}
            height={16}
            className="admin-rankings-meta-icon"
            aria-hidden="true"
          />
          <span className="admin-rankings-meta-copy">{updatedCopy ?? pendingCopy}</span>
        </span>
      ) : null}
      <span className={`admin-ranking-connection ${connectionToneClass(connectionState)}`}>
        <Icon
          icon={connectionIcon(connectionState)}
          width={16}
          height={16}
          className={connectionState === 'connecting' ? 'icon-spin' : undefined}
          aria-hidden="true"
        />
        {statusLabel(strings, connectionState)}
      </span>
    </div>
  )
}

export default function AdminUserRankingsPage({
  strings,
  language,
  snapshot,
  loading,
  error,
  connectionState,
  onRetry,
  initialTab = 'last24h',
  showHeader = true,
}: {
  strings: AdminTranslations['rankings']
  language: Language
  snapshot: AdminUserRankingsSnapshot | null
  loading: boolean
  error: string | null
  connectionState: RankingsConnectionState
  onRetry: () => void
  initialTab?: RankingTabKey
  showHeader?: boolean
}): JSX.Element {
  const [activeTab, setActiveTab] = useState<RankingTabKey>(initialTab)
  const [selectedWindow, setSelectedWindow] = useState<RankingWindowKey>(defaultWindowForTab(initialTab))
  const pageRef = useRef<HTMLElement | null>(null)
  const viewportMode = useViewportMode()
  const primaryColor = readChartColorVar('--dashboard-chart-result-primary-success', '#10b981')
  const creditColor = readChartColorVar('--dashboard-chart-type-api-billable', '#60a5fa')
  const uniqueIpColor = readChartColorVar('--info', '#0ea5e9')
  const rankingTabs = useMemo<ReadonlyArray<RankingTabKey>>(
    () => ['last24h', 'last7d', 'last30d', 'primarySuccess', 'businessCredits', 'uniqueIp'],
    [],
  )
  useEffect(() => {
    setActiveTab(initialTab)
    setSelectedWindow(defaultWindowForTab(initialTab))
  }, [initialTab])

  const handleTabSelect = (tab: RankingTabKey): void => {
    setActiveTab(tab)
    if (isWindowTab(tab)) {
      setSelectedWindow(tab)
    }
  }

  const renderedCards = useMemo(
    () => (snapshot ? buildRankingCards({
      selectedWindow,
      snapshot,
      strings,
      primaryColor,
      creditColor,
      uniqueIpColor,
    }) : []),
    [creditColor, primaryColor, selectedWindow, snapshot, strings, uniqueIpColor],
  )
  const loadingCards = useMemo(() => buildLoadingCards(strings), [strings])
  const showLoadingSkeleton = loading && !snapshot
  const showStackedTabs = viewportMode === 'small'

  return (
    <section ref={pageRef} className="admin-rankings-page">
      {showHeader ? (
        <section className="surface panel">
          <div className="panel-header admin-rankings-header">
            <div className="admin-rankings-header-row">
              <h2>{strings.title}</h2>
              <RankingsMeta
                strings={strings}
                snapshot={snapshot}
                connectionState={connectionState}
                language={language}
              />
            </div>
          </div>
          {error ? (
            <div className={`alert ${snapshot ? '' : 'alert-error'}`}>
              <div>{error}</div>
              {snapshot ? <div className="admin-ranking-stale-hint">{strings.staleHint}</div> : null}
            </div>
          ) : null}
        </section>
      ) : null}

      {snapshot || showLoadingSkeleton ? (
        <section className="admin-rankings-toolbar-band" aria-label={strings.tabsLabel}>
          <div className="admin-rankings-tab-strip" role="radiogroup" aria-label={strings.tabsLabel}>
            {rankingTabs.map((tab) => {
              const active = tab === activeTab
              return (
                <button
                  key={tab}
                  type="button"
                  role="radio"
                  aria-checked={active}
                  aria-disabled={showLoadingSkeleton}
                  disabled={showLoadingSkeleton}
                  className={`admin-rankings-tab ${active ? 'is-active' : ''}`}
                  onClick={() => handleTabSelect(tab)}
                >
                  {buildTabLabel(strings, tab)}
                </button>
              )
            })}
          </div>
        </section>
      ) : null}

      {!showHeader && error ? (
        <div className={`alert ${snapshot ? '' : 'alert-error'}`}>
          <div>{error}</div>
          {snapshot ? <div className="admin-ranking-stale-hint">{strings.staleHint}</div> : null}
        </div>
      ) : null}

      {showLoadingSkeleton ? (
        <section className="admin-ranking-window">
          <div className="admin-ranking-window-grid">
            {loadingCards.map((card) => (
              <RankingsLoadingCard
                key={card.key}
                title={card.title}
                description={card.description}
                strings={strings}
              />
            ))}
          </div>
        </section>
      ) : snapshot && renderedCards.length > 0 ? (
        <section className="admin-ranking-window">
          <div className="admin-ranking-window-grid">
            {renderedCards.map((card) => (
              <RankingsChartCard
                key={card.key}
                title={card.title}
                description={card.description}
                rows={card.rows}
                strings={strings}
                color={card.color}
              />
            ))}
          </div>
        </section>
      ) : null}
    </section>
  )
}
