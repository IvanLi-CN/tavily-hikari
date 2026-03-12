import { Badge } from '../components/ui/badge'
import { Button } from '../components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../components/ui/card'
import { Input } from '../components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '../components/ui/table'
import { Textarea } from '../components/ui/textarea'
import AdminLoadingRegion from '../components/AdminLoadingRegion'
import type {
  ForwardProxyActivityBucket,
  ForwardProxySettings,
  ForwardProxyStatsNode,
  ForwardProxyStatsResponse,
  ForwardProxyValidationKind,
  ForwardProxyValidationResponse,
  ForwardProxyWeightBucket,
  ForwardProxyWindowStats,
} from '../api'
import type { AdminTranslations } from '../i18n'
import type { QueryLoadState } from './queryLoadState'

const numberFormatter = new Intl.NumberFormat()
const decimalFormatter = new Intl.NumberFormat(undefined, {
  minimumFractionDigits: 0,
  maximumFractionDigits: 2,
})
const percentFormatter = new Intl.NumberFormat(undefined, {
  style: 'percent',
  minimumFractionDigits: 0,
  maximumFractionDigits: 0,
})
const dateTimeFormatter = new Intl.DateTimeFormat(undefined, {
  month: 'short',
  day: 'numeric',
  hour: '2-digit',
  minute: '2-digit',
})

type NodeWindowKey = 'oneMinute' | 'fifteenMinutes' | 'oneHour' | 'oneDay' | 'sevenDays'

const WINDOW_KEYS: Array<{ key: NodeWindowKey; translationKey: keyof AdminTranslations['proxySettings']['windows'] }> = [
  { key: 'oneMinute', translationKey: 'oneMinute' },
  { key: 'fifteenMinutes', translationKey: 'fifteenMinutes' },
  { key: 'oneHour', translationKey: 'oneHour' },
  { key: 'oneDay', translationKey: 'oneDay' },
  { key: 'sevenDays', translationKey: 'sevenDays' },
]

export interface ForwardProxyDraft {
  proxyUrlsText: string
  subscriptionUrlsText: string
  subscriptionUpdateIntervalSecs: string
  insertDirect: boolean
}

export interface ForwardProxyValidationEntry {
  id: string
  kind: ForwardProxyValidationKind
  value: string
  result: ForwardProxyValidationResponse
}

interface ForwardProxySettingsModuleProps {
  strings: AdminTranslations['proxySettings']
  draft: ForwardProxyDraft
  settings: ForwardProxySettings | null
  stats: ForwardProxyStatsResponse | null
  settingsLoadState: QueryLoadState
  statsLoadState: QueryLoadState
  settingsError: string | null
  statsError: string | null
  saveError: string | null
  validationError: string | null
  saving: boolean
  validatingKind: ForwardProxyValidationKind | null
  savedAt: number | null
  validationEntries: ForwardProxyValidationEntry[]
  onProxyUrlsTextChange: (value: string) => void
  onSubscriptionUrlsTextChange: (value: string) => void
  onIntervalChange: (value: string) => void
  onInsertDirectChange: (value: boolean) => void
  onSave: () => void
  onValidateSubscriptions: () => void
  onValidateManual: () => void
  onRefresh: () => void
}

interface ForwardProxyBucketRange {
  rangeStartMs: number
  bucketMs: number
  bucketCount: number
}

function formatNumber(value: number | null | undefined): string {
  if (value == null || Number.isNaN(value)) return '—'
  return numberFormatter.format(value)
}

function formatDecimal(value: number | null | undefined): string {
  if (value == null || Number.isNaN(value)) return '—'
  return decimalFormatter.format(value)
}

function formatPercent(value: number | null | undefined): string {
  if (value == null || Number.isNaN(value)) return '—'
  const normalized = value > 1 ? value / 100 : value
  return percentFormatter.format(Math.max(0, normalized))
}

function formatLatency(value: number | null | undefined): string {
  if (value == null || Number.isNaN(value)) return '—'
  return `${decimalFormatter.format(value)} ms`
}

function formatTimeRange(start: string | null | undefined, end: string | null | undefined): string {
  if (!start || !end) return '—'
  const startDate = new Date(start)
  const endDate = new Date(end)
  if (Number.isNaN(startDate.getTime()) || Number.isNaN(endDate.getTime())) return '—'
  return `${dateTimeFormatter.format(startDate)} - ${dateTimeFormatter.format(endDate)}`
}

function computeSuccessRate(stats: ForwardProxyWindowStats): number | null {
  if (typeof stats.successRate === 'number') {
    return stats.successRate
  }
  const success = stats.successCount ?? null
  const failure = stats.failureCount ?? null
  if (success == null || failure == null) {
    return null
  }
  const total = success + failure
  return total > 0 ? success / total : null
}

function buildMergedNodes(
  settings: ForwardProxySettings | null,
  stats: ForwardProxyStatsResponse | null,
): ForwardProxyStatsNode[] {
  const nodeMap = new Map<string, ForwardProxyStatsNode>()

  for (const node of settings?.nodes ?? []) {
    nodeMap.set(node.key, {
      ...node,
      last24h: [],
      weight24h: [],
    })
  }

  for (const node of stats?.nodes ?? []) {
    const previous = nodeMap.get(node.key)
    nodeMap.set(node.key, {
      ...(previous ?? node),
      ...node,
      stats: node.stats ?? previous?.stats ?? {
        oneMinute: { attempts: 0 },
        fifteenMinutes: { attempts: 0 },
        oneHour: { attempts: 0 },
        oneDay: { attempts: 0 },
        sevenDays: { attempts: 0 },
      },
      last24h: node.last24h ?? previous?.last24h ?? [],
      weight24h: node.weight24h ?? previous?.weight24h ?? [],
    })
  }

  const bucketRange = resolveBucketRange(stats)

  return Array.from(nodeMap.values())
    .map((node) => normalizeNodeBuckets(node, bucketRange))
    .sort((left, right) => {
    if (left.source === 'direct' && right.source !== 'direct') return 1
    if (left.source !== 'direct' && right.source === 'direct') return -1
    if (left.penalized !== right.penalized) return left.penalized ? 1 : -1
    return right.weight - left.weight || left.displayName.localeCompare(right.displayName)
    })
}

function resolveBucketRange(stats: ForwardProxyStatsResponse | null): ForwardProxyBucketRange | null {
  if (!stats) return null
  const rangeStartMs = Date.parse(stats.rangeStart)
  const rangeEndMs = Date.parse(stats.rangeEnd)
  const bucketMs = stats.bucketSeconds * 1000
  if (!Number.isFinite(rangeStartMs) || !Number.isFinite(rangeEndMs) || !Number.isFinite(bucketMs) || bucketMs <= 0) {
    return null
  }
  const bucketCount = Math.max(0, Math.round((rangeEndMs - rangeStartMs) / bucketMs))
  if (bucketCount <= 0) return null
  return { rangeStartMs, bucketMs, bucketCount }
}

function normalizeActivityBuckets(
  buckets: ForwardProxyActivityBucket[],
  bucketRange: ForwardProxyBucketRange | null,
): ForwardProxyActivityBucket[] {
  if (!bucketRange) return buckets
  const bucketMap = new Map<number, ForwardProxyActivityBucket>()
  for (const bucket of buckets) {
    const startMs = Date.parse(bucket.bucketStart)
    if (Number.isFinite(startMs)) {
      bucketMap.set(startMs, bucket)
    }
  }

  return Array.from({ length: bucketRange.bucketCount }, (_, index) => {
    const bucketStartMs = bucketRange.rangeStartMs + index * bucketRange.bucketMs
    const existing = bucketMap.get(bucketStartMs)
    if (existing) return existing
    return {
      bucketStart: new Date(bucketStartMs).toISOString(),
      bucketEnd: new Date(bucketStartMs + bucketRange.bucketMs).toISOString(),
      successCount: 0,
      failureCount: 0,
    }
  })
}

function normalizeWeightBuckets(
  buckets: ForwardProxyWeightBucket[],
  bucketRange: ForwardProxyBucketRange | null,
  fallbackWeight: number,
): ForwardProxyWeightBucket[] {
  if (!bucketRange) return buckets
  const bucketMap = new Map<number, ForwardProxyWeightBucket>()
  for (const bucket of buckets) {
    const startMs = Date.parse(bucket.bucketStart)
    if (Number.isFinite(startMs)) {
      bucketMap.set(startMs, bucket)
    }
  }

  let carryWeight = fallbackWeight
  return Array.from({ length: bucketRange.bucketCount }, (_, index) => {
    const bucketStartMs = bucketRange.rangeStartMs + index * bucketRange.bucketMs
    const existing = bucketMap.get(bucketStartMs)
    if (existing) {
      carryWeight = existing.lastWeight
      return existing
    }
    return {
      bucketStart: new Date(bucketStartMs).toISOString(),
      bucketEnd: new Date(bucketStartMs + bucketRange.bucketMs).toISOString(),
      sampleCount: 0,
      minWeight: carryWeight,
      maxWeight: carryWeight,
      avgWeight: carryWeight,
      lastWeight: carryWeight,
    }
  })
}

function normalizeNodeBuckets(
  node: ForwardProxyStatsNode,
  bucketRange: ForwardProxyBucketRange | null,
): ForwardProxyStatsNode {
  if (!bucketRange) return node
  return {
    ...node,
    last24h: normalizeActivityBuckets(node.last24h, bucketRange),
    weight24h: normalizeWeightBuckets(node.weight24h, bucketRange, node.weight),
  }
}

function summarizeActivity(node: ForwardProxyStatsNode): { success: number; failure: number } {
  return node.last24h.reduce(
    (accumulator, bucket) => {
      accumulator.success += bucket.successCount
      accumulator.failure += bucket.failureCount
      return accumulator
    },
    { success: 0, failure: 0 },
  )
}

function summarizeWeight(weight24h: ForwardProxyWeightBucket[]): {
  avgWeight: number | null
  minWeight: number | null
  maxWeight: number | null
  lastWeight: number | null
} {
  if (weight24h.length === 0) {
    return {
      avgWeight: null,
      minWeight: null,
      maxWeight: null,
      lastWeight: null,
    }
  }

  let totalSamples = 0
  let weightedTotal = 0
  let minWeight = Number.POSITIVE_INFINITY
  let maxWeight = Number.NEGATIVE_INFINITY

  for (const bucket of weight24h) {
    const sampleCount = Math.max(1, bucket.sampleCount)
    totalSamples += sampleCount
    weightedTotal += bucket.avgWeight * sampleCount
    minWeight = Math.min(minWeight, bucket.minWeight)
    maxWeight = Math.max(maxWeight, bucket.maxWeight)
  }

  const lastBucket = weight24h[weight24h.length - 1]
  return {
    avgWeight: totalSamples > 0 ? weightedTotal / totalSamples : null,
    minWeight: Number.isFinite(minWeight) ? minWeight : null,
    maxWeight: Number.isFinite(maxWeight) ? maxWeight : null,
    lastWeight: lastBucket?.lastWeight ?? null,
  }
}

function formatWeight(value: number | null | undefined): string {
  if (value == null || Number.isNaN(value)) return '—'
  return decimalFormatter.format(value)
}

function resolveWeightBuckets(node: ForwardProxyStatsNode): ForwardProxyWeightBucket[] {
  if (node.weight24h.length > 0) return node.weight24h
  if (node.last24h.length === 0) return []
  return node.last24h.map((bucket) => ({
    bucketStart: bucket.bucketStart,
    bucketEnd: bucket.bucketEnd,
    sampleCount: 0,
    minWeight: node.weight,
    maxWeight: node.weight,
    avgWeight: node.weight,
    lastWeight: node.weight,
  }))
}

function buildVisibleBarHeights(successCount: number, failureCount: number, scaleMax: number, totalHeightPx: number) {
  if (scaleMax <= 0 || totalHeightPx <= 0) {
    return { empty: totalHeightPx, failure: 0, success: 0 }
  }

  let success = successCount > 0 ? Math.max((successCount / scaleMax) * totalHeightPx, 1) : 0
  let failure = failureCount > 0 ? Math.max((failureCount / scaleMax) * totalHeightPx, 1) : 0
  const maxVisible = Math.max(totalHeightPx, 0)
  let overflow = success + failure - maxVisible

  const shrink = (value: number, minVisible: number, amount: number) => {
    if (amount <= 0 || value <= minVisible) return { nextValue: value, remaining: amount }
    const delta = Math.min(value - minVisible, amount)
    return { nextValue: value - delta, remaining: amount - delta }
  }

  if (overflow > 0) {
    const first = success >= failure ? 'success' : 'failure'
    const second = first === 'success' ? 'failure' : 'success'
    for (const key of [first, second] as const) {
      const minVisible = key === 'success' ? (successCount > 0 ? 1 : 0) : failureCount > 0 ? 1 : 0
      const current = key === 'success' ? success : failure
      const result = shrink(current, minVisible, overflow)
      if (key === 'success') {
        success = result.nextValue
      } else {
        failure = result.nextValue
      }
      overflow = result.remaining
    }
  }

  const used = Math.min(success + failure, maxVisible)
  return {
    empty: Math.max(maxVisible - used, 0),
    failure,
    success,
  }
}

interface WeightTrendScale {
  minValue: number
  maxValue: number
}

interface WeightTrendGeometry {
  chartWidth: number
  chartHeight: number
  linePath: string
  areaPath: string
  zeroY: number
}

function buildWeightTrendGeometry(
  buckets: ForwardProxyWeightBucket[],
  scale: WeightTrendScale,
): WeightTrendGeometry | null {
  if (buckets.length === 0) return null

  const chartWidth = 216
  const chartHeight = 40
  const span = Math.max(scale.maxValue - scale.minValue, Number.EPSILON)
  const bucketWidth = chartWidth / buckets.length
  const points = buckets.map((bucket, index) => {
    const ratio = Math.max(0, Math.min(1, (bucket.lastWeight - scale.minValue) / span))
    const x = bucketWidth * index + bucketWidth / 2
    const y = chartHeight - ratio * chartHeight
    return { x, y }
  })
  const firstPoint = points[0]
  const lastPoint = points[points.length - 1]
  if (!firstPoint || !lastPoint) return null

  const zeroRatio = (0 - scale.minValue) / span
  const zeroY = chartHeight - Math.max(0, Math.min(1, zeroRatio)) * chartHeight
  const linePath = points
    .map((point, index) => `${index === 0 ? 'M' : 'L'} ${point.x.toFixed(2)} ${point.y.toFixed(2)}`)
    .join(' ')
  const areaPath = `${linePath} L ${lastPoint.x.toFixed(2)} ${zeroY.toFixed(2)} L ${firstPoint.x.toFixed(2)} ${zeroY.toFixed(2)} Z`

  return {
    chartWidth,
    chartHeight,
    linePath,
    areaPath,
    zeroY,
  }
}

function getWindowColumnClassName(index: number): string {
  if (index === 1 || index === 2) return 'hidden text-center md:table-cell'
  if (index === 3 || index === 4) return 'hidden text-center xl:table-cell'
  return 'text-center'
}

function RequestTrendCell({
  buckets,
  scaleMax,
}: {
  buckets: ForwardProxyActivityBucket[]
  scaleMax: number
}): JSX.Element {
  if (buckets.length === 0) {
    return <span className="text-[11px] text-muted-foreground">—</span>
  }

  return (
    <div className="flex h-11 items-end gap-px py-0.5">
      {buckets.map((bucket) => {
        const total = bucket.successCount + bucket.failureCount
        const heights = buildVisibleBarHeights(bucket.successCount, bucket.failureCount, scaleMax, 40)
        return (
          <div
            key={bucket.bucketStart}
            className="relative flex h-10 min-w-0 flex-1 flex-col overflow-hidden rounded-[3px] border border-border/40 bg-muted/35"
            title={`${formatTimeRange(bucket.bucketStart, bucket.bucketEnd)} · ${bucket.successCount}/${bucket.failureCount}`}
          >
            <div style={{ height: `${heights.empty}px` }} />
            <div
              className={total > 0 ? 'bg-destructive/80' : 'bg-transparent'}
              style={{ height: `${heights.failure}px` }}
            />
            <div
              className={total > 0 ? 'bg-success/85' : 'bg-transparent'}
              style={{ height: `${heights.success}px` }}
            />
          </div>
        )
      })}
    </div>
  )
}

function WeightTrendCell({
  buckets,
  scale,
}: {
  buckets: ForwardProxyWeightBucket[]
  scale: WeightTrendScale
}): JSX.Element {
  const geometry = buildWeightTrendGeometry(buckets, scale)
  if (!geometry) {
    return <span className="text-[11px] text-muted-foreground">—</span>
  }

  return (
    <svg
      viewBox={`0 0 ${geometry.chartWidth} ${geometry.chartHeight}`}
      className="block h-10 w-full rounded-md border border-border/55 bg-background/45"
      aria-hidden="true"
    >
      <line
        x1={0}
        y1={geometry.zeroY}
        x2={geometry.chartWidth}
        y2={geometry.zeroY}
        stroke="hsl(var(--foreground) / 0.14)"
        strokeWidth="1"
      />
      <path d={geometry.areaPath} fill="hsl(var(--success) / 0.18)" />
      <path
        d={geometry.linePath}
        fill="none"
        stroke="hsl(var(--success))"
        strokeWidth="1.8"
        strokeLinejoin="round"
        strokeLinecap="round"
      />
    </svg>
  )
}

function getSourceLabel(strings: AdminTranslations['proxySettings'], source: string): string {
  if (source === 'manual') return strings.sources.manual
  if (source === 'subscription') return strings.sources.subscription
  if (source === 'direct') return strings.sources.direct
  return strings.sources.unknown
}

function getNodeStateBadge(
  strings: AdminTranslations['proxySettings'],
  node: ForwardProxyStatsNode,
): { label: string; variant: 'success' | 'warning' | 'info' | 'neutral' } {
  if (node.source === 'direct') {
    return { label: strings.states.direct, variant: 'info' }
  }
  if (node.penalized) {
    return { label: strings.states.penalized, variant: 'warning' }
  }
  return { label: strings.states.ready, variant: 'success' }
}

export default function ForwardProxySettingsModule({
  strings,
  draft,
  settings,
  stats,
  settingsLoadState,
  statsLoadState,
  settingsError,
  statsError,
  saveError,
  validationError,
  saving,
  validatingKind,
  savedAt,
  validationEntries,
  onProxyUrlsTextChange,
  onSubscriptionUrlsTextChange,
  onIntervalChange,
  onInsertDirectChange,
  onSave,
  onValidateSubscriptions,
  onValidateManual,
  onRefresh,
}: ForwardProxySettingsModuleProps): JSX.Element {
  const mergedNodes = buildMergedNodes(settings, stats)
  const nodeRows = mergedNodes.map((node) => ({
    node,
    activity: summarizeActivity(node),
    weight: summarizeWeight(node.weight24h),
    weightBuckets: resolveWeightBuckets(node),
  }))
  const requestBucketScaleMax = Math.max(
    ...nodeRows.flatMap(({ node }) => node.last24h.map((bucket) => bucket.successCount + bucket.failureCount)),
    0,
  )
  const allWeightValues = nodeRows.flatMap(({ weightBuckets }) =>
    weightBuckets.flatMap((bucket) => [bucket.minWeight, bucket.maxWeight, bucket.lastWeight]),
  )
  const minWeightValue = Math.min(...allWeightValues, 0)
  const maxWeightValue = Math.max(...allWeightValues, 0)
  const weightPadding = Math.max((maxWeightValue - minWeightValue) * 0.08, 0.2)
  const weightTrendScale: WeightTrendScale = {
    minValue: minWeightValue - weightPadding,
    maxValue: maxWeightValue + weightPadding,
  }
  const totalPrimaryAssignments = mergedNodes.reduce((sum, node) => sum + node.primaryAssignmentCount, 0)
  const totalSecondaryAssignments = mergedNodes.reduce((sum, node) => sum + node.secondaryAssignmentCount, 0)
  const penalizedCount = mergedNodes.filter((node) => node.penalized).length
  const readyCount = mergedNodes.length - penalizedCount
  const validatingSubscriptions = validatingKind === 'subscriptionUrl'
  const validatingManual = validatingKind === 'proxyUrl'

  const summaryCards = [
    {
      key: 'nodes',
      label: strings.summary.configuredNodes,
      value: formatNumber(mergedNodes.length),
      hint: strings.summary.configuredNodesHint,
    },
    {
      key: 'ready',
      label: strings.summary.readyNodes,
      value: formatNumber(readyCount),
      hint: strings.summary.readyNodesHint,
    },
    {
      key: 'penalized',
      label: strings.summary.penalizedNodes,
      value: formatNumber(penalizedCount),
      hint: strings.summary.penalizedNodesHint,
    },
    {
      key: 'subscriptions',
      label: strings.summary.subscriptions,
      value: formatNumber(settings?.subscriptionUrls.length ?? 0),
      hint: strings.summary.subscriptionsHint,
    },
    {
      key: 'manual',
      label: strings.summary.manualNodes,
      value: formatNumber(settings?.proxyUrls.length ?? 0),
      hint: strings.summary.manualNodesHint,
    },
    {
      key: 'assignments',
      label: strings.summary.assignmentSpread,
      value: `${formatNumber(totalPrimaryAssignments)} / ${formatNumber(totalSecondaryAssignments)}`,
      hint: strings.summary.assignmentSpreadHint,
    },
  ]

  return (
    <>
      <Card className="surface panel">
        <CardHeader className="forward-proxy-panel-header">
          <div className="forward-proxy-panel-heading">
            <CardTitle>{strings.title}</CardTitle>
            <CardDescription className="panel-description">{strings.description}</CardDescription>
          </div>
          <div className="forward-proxy-toolbar">
            <Button type="button" variant="outline" onClick={onRefresh}>
              {strings.actions.refresh}
            </Button>
            <Button type="button" onClick={onSave} disabled={saving}>
              {saving ? strings.actions.saving : strings.actions.save}
            </Button>
          </div>
        </CardHeader>
        <CardContent className="forward-proxy-panel-content">
          <div className="forward-proxy-summary-grid">
            {summaryCards.map((card) => (
              <Card key={card.key} className="forward-proxy-summary-card">
                <CardContent className="forward-proxy-summary-card-content">
                  <span className="forward-proxy-summary-label">{card.label}</span>
                  <strong className="forward-proxy-summary-value">{card.value}</strong>
                  <span className="forward-proxy-summary-hint">{card.hint}</span>
                </CardContent>
              </Card>
            ))}
          </div>

          <div className="forward-proxy-range-row">
            <Badge variant="outline">{strings.summary.range}</Badge>
            <span className="panel-description">{formatTimeRange(stats?.rangeStart, stats?.rangeEnd)}</span>
            {savedAt != null && (
              <span className="panel-description">
                {strings.summary.savedAt.replace('{time}', dateTimeFormatter.format(new Date(savedAt)))}
              </span>
            )}
          </div>
        </CardContent>
      </Card>

      <Card className="surface panel">
        <CardHeader className="forward-proxy-panel-header">
          <div className="forward-proxy-panel-heading">
            <CardTitle>{strings.nodes.title}</CardTitle>
            <CardDescription className="panel-description">{strings.nodes.description}</CardDescription>
          </div>
        </CardHeader>
        <CardContent className="forward-proxy-panel-content">
          {statsError && (
            <div className="alert alert-error" role="alert">
              {statsError}
            </div>
          )}

          <AdminLoadingRegion
            loadState={statsLoadState}
            loadingLabel={strings.nodes.loading}
            errorLabel={statsError || undefined}
            minHeight={240}
          >
            {mergedNodes.length === 0 ? (
              <div className="empty-state alert">{strings.nodes.empty}</div>
            ) : (
              <>
                <div className="forward-proxy-node-list-mobile">
                  {nodeRows.map(({ node, activity, weight }) => {
                    const stateBadge = getNodeStateBadge(strings, node)
                    return (
                      <Card className="forward-proxy-node-mobile-card" key={`mobile-${node.key}`}>
                        <CardHeader className="forward-proxy-node-mobile-header">
                          <div className="forward-proxy-node-mobile-title-row">
                            <CardTitle className="text-base">{node.displayName}</CardTitle>
                            <Badge variant={node.source === 'subscription' ? 'info' : node.source === 'manual' ? 'outline' : 'neutral'}>
                              {getSourceLabel(strings, node.source)}
                            </Badge>
                            <Badge variant={stateBadge.variant}>{stateBadge.label}</Badge>
                          </div>
                          <code className="forward-proxy-code-inline">{node.key}</code>
                        </CardHeader>
                        <CardContent className="forward-proxy-node-mobile-content">
                          <div className="forward-proxy-node-mobile-grid">
                            <div className="forward-proxy-node-mobile-block">
                              <span className="forward-proxy-node-metric-label">{strings.nodes.table.assignments}</span>
                              <span>
                                {strings.nodes.primary}: <strong>{formatNumber(node.primaryAssignmentCount)}</strong>
                              </span>
                              <span>
                                {strings.nodes.secondary}: <strong>{formatNumber(node.secondaryAssignmentCount)}</strong>
                              </span>
                            </div>
                            <div className="forward-proxy-node-mobile-block">
                              <span className="forward-proxy-node-metric-label">{strings.nodes.table.activity24h}</span>
                              <span>
                                {strings.nodes.successCountLabel}: <strong>{formatNumber(activity.success)}</strong>
                              </span>
                              <span>
                                {strings.nodes.failureCountLabel}: <strong>{formatNumber(activity.failure)}</strong>
                              </span>
                            </div>
                            <div className="forward-proxy-node-mobile-block">
                              <span className="forward-proxy-node-metric-label">{strings.nodes.table.weight24h}</span>
                              <span>
                                {strings.nodes.lastWeightLabel}: <strong>{formatDecimal(weight.lastWeight)}</strong>
                              </span>
                              <span>
                                {strings.nodes.avgWeightLabel}: <strong>{formatDecimal(weight.avgWeight)}</strong>
                              </span>
                              <span>
                                {strings.nodes.minMaxWeightLabel}: <strong>{`${formatDecimal(weight.minWeight)} / ${formatDecimal(weight.maxWeight)}`}</strong>
                              </span>
                            </div>
                          </div>

                          <div className="forward-proxy-window-grid">
                            {WINDOW_KEYS.map((windowDefinition) => {
                              const statsForWindow = node.stats[windowDefinition.key]
                              return (
                                <Card className="forward-proxy-window-card" key={`${node.key}-${windowDefinition.key}`}>
                                  <CardContent className="forward-proxy-window-card-content">
                                    <span className="forward-proxy-window-label">{strings.windows[windowDefinition.translationKey]}</span>
                                    <strong>{formatPercent(computeSuccessRate(statsForWindow))}</strong>
                                    <span>{formatLatency(statsForWindow.avgLatencyMs)}</span>
                                  </CardContent>
                                </Card>
                              )
                            })}
                          </div>
                        </CardContent>
                      </Card>
                    )
                  })}
                </div>

                <div className="forward-proxy-table-wrapper rounded-2xl border border-border/75 bg-card/50">
                  <Table className="forward-proxy-table min-w-[980px] table-fixed text-xs xl:min-w-0">
                    <TableHeader className="bg-muted/40 uppercase tracking-[0.08em] text-[11px] text-muted-foreground">
                      <TableRow className="hover:bg-transparent">
                        <TableHead className="w-[30%]">{strings.nodes.table.node}</TableHead>
                        {WINDOW_KEYS.map((windowDefinition, index) => (
                          <TableHead className={getWindowColumnClassName(index)} key={`head-${windowDefinition.key}`}>
                            {strings.windows[windowDefinition.translationKey]}
                          </TableHead>
                        ))}
                        <TableHead className="w-[18%]">{strings.nodes.table.activity24h}</TableHead>
                        <TableHead className="w-[18%]">{strings.nodes.table.weight24h}</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody className="divide-y divide-border/65 [&_tr:last-child]:border-0">
                      {nodeRows.map(({ node, activity, weight, weightBuckets }) => {
                        const stateBadge = getNodeStateBadge(strings, node)
                        return (
                          <TableRow key={node.key} className="forward-proxy-table-row border-0 align-top">
                            <TableCell className="forward-proxy-node-cell py-3">
                              <div className="forward-proxy-node-cell-main min-w-0">
                                <div className="forward-proxy-node-cell-title-row">
                                  <strong className="truncate text-sm">{node.displayName}</strong>
                                  <Badge variant={node.source === 'subscription' ? 'info' : node.source === 'manual' ? 'outline' : 'neutral'}>
                                    {getSourceLabel(strings, node.source)}
                                  </Badge>
                                  <Badge variant={stateBadge.variant}>{stateBadge.label}</Badge>
                                </div>
                                <code className="forward-proxy-code-inline">{node.key}</code>
                                <div className="forward-proxy-node-cell-meta">
                                  <span>
                                    {strings.nodes.primary}: <strong>{formatNumber(node.primaryAssignmentCount)}</strong>
                                  </span>
                                  <span>
                                    {strings.nodes.secondary}: <strong>{formatNumber(node.secondaryAssignmentCount)}</strong>
                                  </span>
                                </div>
                              </div>
                            </TableCell>
                            {WINDOW_KEYS.map((windowDefinition, index) => {
                              const statsForWindow = node.stats[windowDefinition.key]
                              return (
                                <TableCell className={getWindowColumnClassName(index)} key={`${node.key}-${windowDefinition.key}`}>
                                  <div className="flex flex-col items-center gap-0.5 py-1">
                                    <span>{formatPercent(computeSuccessRate(statsForWindow))}</span>
                                    <span className="text-[11px] text-muted-foreground">{formatLatency(statsForWindow.avgLatencyMs)}</span>
                                  </div>
                                </TableCell>
                              )
                            })}
                            <TableCell className="py-3">
                              <div className="space-y-2">
                                <RequestTrendCell buckets={node.last24h} scaleMax={requestBucketScaleMax} />
                                <div className="text-[11px] text-muted-foreground">
                                  {strings.nodes.successCountLabel}: <strong>{formatNumber(activity.success)}</strong>
                                  {' · '}
                                  {strings.nodes.failureCountLabel}: <strong>{formatNumber(activity.failure)}</strong>
                                </div>
                              </div>
                            </TableCell>
                            <TableCell className="py-3">
                              <div className="space-y-2">
                                <WeightTrendCell buckets={weightBuckets} scale={weightTrendScale} />
                                <div className="text-[11px] text-muted-foreground">
                                  {strings.nodes.lastWeightLabel}: <strong>{formatWeight(weight.lastWeight)}</strong>
                                  {' · '}
                                  {strings.nodes.avgWeightLabel}: <strong>{formatWeight(weight.avgWeight)}</strong>
                                </div>
                              </div>
                            </TableCell>
                          </TableRow>
                        )
                      })}
                    </TableBody>
                  </Table>
                </div>
              </>
            )}
          </AdminLoadingRegion>
        </CardContent>
      </Card>
      <Card className="surface panel">
        <CardHeader className="forward-proxy-panel-header">
          <div className="forward-proxy-panel-heading">
            <CardTitle>{strings.config.title}</CardTitle>
            <CardDescription className="panel-description">{strings.config.description}</CardDescription>
          </div>
          <div className="forward-proxy-toolbar">
            <Button type="button" variant="outline" onClick={onValidateSubscriptions} disabled={validatingSubscriptions}>
              {validatingSubscriptions ? strings.actions.validatingSubscriptions : strings.actions.validateSubscriptions}
            </Button>
            <Button type="button" variant="outline" onClick={onValidateManual} disabled={validatingManual}>
              {validatingManual ? strings.actions.validatingManual : strings.actions.validateManual}
            </Button>
          </div>
        </CardHeader>
        <CardContent className="forward-proxy-panel-content">
          {saveError && (
            <div className="alert alert-error" role="alert">
              {saveError}
            </div>
          )}
          {settingsError && (
            <div className="alert alert-error" role="alert">
              {settingsError}
            </div>
          )}

          <AdminLoadingRegion
            loadState={settingsLoadState}
            loadingLabel={strings.config.loading}
            errorLabel={settingsError || undefined}
            minHeight={220}
          >
            <div className="forward-proxy-config-grid">
              <Card className="forward-proxy-editor-card">
                <CardHeader className="forward-proxy-editor-head">
                  <div>
                    <CardTitle className="text-base">{strings.config.subscriptionsTitle}</CardTitle>
                    <CardDescription className="panel-description">
                      {strings.config.subscriptionsDescription}
                    </CardDescription>
                  </div>
                  <Badge variant="info">{strings.sources.subscription}</Badge>
                </CardHeader>
                <CardContent className="forward-proxy-editor-card-content">
                  <Textarea
                    id="forward-proxy-subscription-urls"
                    name="subscriptionUrls"
                    rows={8}
                    value={draft.subscriptionUrlsText}
                    onChange={(event) => onSubscriptionUrlsTextChange(event.target.value)}
                    placeholder={strings.config.subscriptionsPlaceholder}
                    className="forward-proxy-textarea"
                  />
                </CardContent>
              </Card>

              <Card className="forward-proxy-editor-card">
                <CardHeader className="forward-proxy-editor-head">
                  <div>
                    <CardTitle className="text-base">{strings.config.manualTitle}</CardTitle>
                    <CardDescription className="panel-description">{strings.config.manualDescription}</CardDescription>
                  </div>
                  <Badge variant="outline">{strings.sources.manual}</Badge>
                </CardHeader>
                <CardContent className="forward-proxy-editor-card-content">
                  <Textarea
                    id="forward-proxy-manual-urls"
                    name="proxyUrls"
                    rows={8}
                    value={draft.proxyUrlsText}
                    onChange={(event) => onProxyUrlsTextChange(event.target.value)}
                    placeholder={strings.config.manualPlaceholder}
                    className="forward-proxy-textarea"
                  />
                </CardContent>
              </Card>
            </div>

            <div className="forward-proxy-form-footer">
              <Card className="forward-proxy-field-card">
                <CardContent className="forward-proxy-field-card-content">
                  <label className="forward-proxy-field">
                    <span className="forward-proxy-field-label">{strings.config.subscriptionIntervalLabel}</span>
                    <Input
                      id="forward-proxy-subscription-interval"
                      name="subscriptionUpdateIntervalSecs"
                      type="number"
                      inputMode="numeric"
                      min={60}
                      step={60}
                      value={draft.subscriptionUpdateIntervalSecs}
                      onChange={(event) => onIntervalChange(event.target.value)}
                    />
                    <span className="panel-description">{strings.config.subscriptionIntervalHint}</span>
                  </label>
                </CardContent>
              </Card>

              <Card className="forward-proxy-checkbox-card">
                <CardContent className="forward-proxy-checkbox-card-content">
                  <label className="forward-proxy-checkbox" htmlFor="forward-proxy-insert-direct">
                    <input
                      id="forward-proxy-insert-direct"
                      type="checkbox"
                      checked={draft.insertDirect}
                      onChange={(event) => onInsertDirectChange(event.target.checked)}
                    />
                    <div>
                      <strong>{strings.config.insertDirectLabel}</strong>
                      <p className="panel-description">{strings.config.insertDirectHint}</p>
                    </div>
                  </label>
                </CardContent>
              </Card>
            </div>
          </AdminLoadingRegion>
        </CardContent>
      </Card>

      <Card className="surface panel">
        <CardHeader className="forward-proxy-panel-header">
          <div className="forward-proxy-panel-heading">
            <CardTitle>{strings.validation.title}</CardTitle>
            <CardDescription className="panel-description">{strings.validation.description}</CardDescription>
          </div>
        </CardHeader>
        <CardContent className="forward-proxy-panel-content">
          {validationError && (
            <div className="alert alert-error" role="alert">
              {validationError}
            </div>
          )}

          {validationEntries.length === 0 ? (
            <div className="empty-state alert">{strings.validation.empty}</div>
          ) : (
            <div className="forward-proxy-validation-grid">
              {validationEntries.map((entry) => {
                const resultTone = entry.result.ok ? 'success' : 'destructive'
                return (
                  <Card className="forward-proxy-validation-card" key={entry.id}>
                    <CardContent className="forward-proxy-validation-card-content">
                      <div className="forward-proxy-validation-head">
                        <Badge variant={resultTone}>
                          {entry.result.ok ? strings.validation.ok : strings.validation.failed}
                        </Badge>
                        <Badge variant="outline">
                          {entry.kind === 'subscriptionUrl'
                            ? strings.validation.subscriptionKind
                            : strings.validation.proxyKind}
                        </Badge>
                      </div>
                      <code className="forward-proxy-code-block">{entry.result.normalizedValue ?? entry.value}</code>
                      <p className="forward-proxy-validation-message">{entry.result.message}</p>
                      <div className="forward-proxy-validation-meta">
                        <span>
                          {strings.validation.discoveredNodes}: {formatNumber(entry.result.discoveredNodes ?? 0)}
                        </span>
                        <span>
                          {strings.validation.latency}: {formatLatency(entry.result.latencyMs)}
                        </span>
                      </div>
                    </CardContent>
                  </Card>
                )
              })}
            </div>
          )}
        </CardContent>
      </Card>

    </>
  )
}
