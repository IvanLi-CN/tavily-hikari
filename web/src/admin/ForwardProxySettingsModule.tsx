import { useState } from 'react'
import { Badge } from '../components/ui/badge'
import { Button } from '../components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../components/ui/card'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../components/ui/dialog'
import { Input } from '../components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../components/ui/select'
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
  settings: ForwardProxySettings | null
  stats: ForwardProxyStatsResponse | null
  settingsLoadState: QueryLoadState
  statsLoadState: QueryLoadState
  settingsError: string | null
  statsError: string | null
  saveError: string | null
  saving: boolean
  savedAt: number | null
  onPersistDraft: (draft: ForwardProxyDraft) => Promise<void>
  onValidateCandidates: (
    kind: ForwardProxyValidationKind,
    values: string[],
  ) => Promise<ForwardProxyValidationEntry[]>
  onRefresh: () => void
}

type ForwardProxyDialogKind = 'subscription' | 'manual' | null

const FORWARD_PROXY_INTERVAL_OPTIONS = [
  { value: '60', label: '1m' },
  { value: '300', label: '5m' },
  { value: '900', label: '15m' },
  { value: '3600', label: '1h' },
  { value: '21600', label: '6h' },
  { value: '86400', label: '1d' },
]

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

function normalizeEntries(values: string[]): string[] {
  const seen = new Set<string>()
  const normalized: string[] = []
  for (const value of values) {
    const trimmed = value.trim()
    if (!trimmed || seen.has(trimmed)) continue
    seen.add(trimmed)
    normalized.push(trimmed)
  }
  return normalized
}

function splitMultilineInput(value: string): string[] {
  return normalizeEntries(value.split(/\r?\n/))
}

function buildDraftFromSettings(settings: ForwardProxySettings | null): ForwardProxyDraft {
  return {
    proxyUrlsText: settings?.proxyUrls.join('\n') ?? '',
    subscriptionUrlsText: settings?.subscriptionUrls.join('\n') ?? '',
    subscriptionUpdateIntervalSecs: String(settings?.subscriptionUpdateIntervalSecs ?? 3600),
    insertDirect: settings?.insertDirect ?? true,
  }
}

function withDraftList(
  draft: ForwardProxyDraft,
  key: 'proxyUrlsText' | 'subscriptionUrlsText',
  values: string[],
): ForwardProxyDraft {
  return {
    ...draft,
    [key]: normalizeEntries(values).join('\n'),
  }
}

function extractProtocolName(value: string): string {
  const index = value.indexOf(':')
  if (index <= 0) return 'unknown'
  return value.slice(0, index)
}

function extractListDisplayName(value: string, fallback: string): string {
  const hashIndex = value.lastIndexOf('#')
  if (hashIndex >= 0 && hashIndex < value.length - 1) {
    try {
      return decodeURIComponent(value.slice(hashIndex + 1))
    } catch {
      return value.slice(hashIndex + 1)
    }
  }

  try {
    const url = new URL(value)
    if (url.hostname) {
      return url.port ? `${url.hostname}:${url.port}` : url.hostname
    }
  } catch {
    // Ignore parse failures for share links and fall back to the raw value.
  }

  return value.length > 72 ? `${value.slice(0, 69)}...` : value || fallback
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
    <div className="flex h-10 items-end gap-px">
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

type StatusBadgeVariant = 'success' | 'warning' | 'info' | 'neutral' | 'destructive'

function mapValidationErrorLabel(
  strings: AdminTranslations['proxySettings'],
  errorCode: string | null | undefined,
): string {
  switch (errorCode) {
    case 'proxy_timeout':
      return strings.validation.timeout
    case 'proxy_unreachable':
      return strings.validation.unreachable
    case 'xray_missing':
      return strings.validation.xrayMissing
    case 'subscription_unreachable':
      return strings.validation.subscriptionUnreachable
    default:
      return strings.validation.validationFailed
  }
}

function getNodeStateBadge(
  strings: AdminTranslations['proxySettings'],
  node: ForwardProxyStatsNode,
): { label: string; variant: StatusBadgeVariant } {
  if (node.source === 'direct') {
    return { label: strings.states.direct, variant: 'info' }
  }
  if (node.penalized) {
    return { label: strings.states.penalized, variant: 'warning' }
  }
  if (!node.available) {
    switch (node.lastError) {
      case 'proxy_timeout':
        return { label: strings.states.timeout, variant: 'destructive' }
      case 'proxy_unreachable':
        return { label: strings.states.unreachable, variant: 'destructive' }
      case 'xray_missing':
        return { label: strings.states.xrayMissing, variant: 'warning' }
      default:
        return { label: strings.states.unavailable, variant: 'neutral' }
    }
  }
  return { label: strings.states.ready, variant: 'success' }
}

export default function ForwardProxySettingsModule({
  strings,
  settings,
  stats,
  settingsLoadState,
  statsLoadState,
  settingsError,
  statsError,
  saveError,
  saving,
  savedAt,
  onPersistDraft,
  onValidateCandidates,
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
  const readyCount = mergedNodes.filter((node) => node.available && !node.penalized).length
  const manualUrls = settings?.proxyUrls ?? []
  const subscriptionUrls = settings?.subscriptionUrls ?? []
  const draft = buildDraftFromSettings(settings)
  const [dialogKind, setDialogKind] = useState<ForwardProxyDialogKind>(null)
  const [dialogInput, setDialogInput] = useState('')
  const [dialogError, setDialogError] = useState<string | null>(null)
  const [dialogValidating, setDialogValidating] = useState(false)
  const [dialogResults, setDialogResults] = useState<ForwardProxyValidationEntry[]>([])
  const dialogIsSubscription = dialogKind === 'subscription'
  const dialogAvailableResults = dialogResults.filter((entry) => entry.result.ok)
  const selectedInterval =
    FORWARD_PROXY_INTERVAL_OPTIONS.find(
      (option) => option.value === String(settings?.subscriptionUpdateIntervalSecs ?? 3600),
    )?.value ?? '3600'

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
      value: formatNumber(subscriptionUrls.length),
      hint: strings.summary.subscriptionsHint,
    },
    {
      key: 'manual',
      label: strings.summary.manualNodes,
      value: formatNumber(manualUrls.length),
      hint: strings.summary.manualNodesHint,
    },
    {
      key: 'assignments',
      label: strings.summary.assignmentSpread,
      value: `${formatNumber(totalPrimaryAssignments)} / ${formatNumber(totalSecondaryAssignments)}`,
      hint: strings.summary.assignmentSpreadHint,
    },
  ]

  const openDialog = (kind: Exclude<ForwardProxyDialogKind, null>) => {
    setDialogKind(kind)
    setDialogInput('')
    setDialogError(null)
    setDialogResults([])
  }

  const closeDialog = () => {
    if (dialogValidating) return
    setDialogKind(null)
    setDialogInput('')
    setDialogError(null)
    setDialogResults([])
  }

  const persistDraft = async (nextDraft: ForwardProxyDraft) => {
    setDialogError(null)
    await onPersistDraft(nextDraft)
  }

  const persistManualUrls = async (nextManualUrls: string[]) => {
    await persistDraft(withDraftList(draft, 'proxyUrlsText', nextManualUrls))
  }

  const persistSubscriptionUrls = async (nextSubscriptionUrls: string[]) => {
    await persistDraft(withDraftList(draft, 'subscriptionUrlsText', nextSubscriptionUrls))
  }

  const handleRemoveManual = async (value: string) => {
    try {
      await persistManualUrls(manualUrls.filter((candidate) => candidate !== value))
    } catch {
      // Parent state already exposes the error banner.
    }
  }

  const handleRemoveSubscription = async (value: string) => {
    try {
      await persistSubscriptionUrls(subscriptionUrls.filter((candidate) => candidate !== value))
    } catch {
      // Parent state already exposes the error banner.
    }
  }

  const handleIntervalChange = async (value: string) => {
    try {
      await persistDraft({
        ...draft,
        subscriptionUpdateIntervalSecs: value,
      })
    } catch {
      // Parent state already exposes the error banner.
    }
  }

  const handleInsertDirectChange = async (checked: boolean) => {
    try {
      await persistDraft({
        ...draft,
        insertDirect: checked,
      })
    } catch {
      // Parent state already exposes the error banner.
    }
  }

  const handleValidateDialog = async () => {
    const values = dialogIsSubscription ? normalizeEntries([dialogInput]) : splitMultilineInput(dialogInput)
    if (values.length === 0) {
      setDialogResults([])
      setDialogError(
        dialogIsSubscription ? strings.validation.emptySubscriptions : strings.validation.emptyManual,
      )
      return
    }

    setDialogError(null)
    setDialogValidating(true)
    try {
      const kind: ForwardProxyValidationKind = dialogIsSubscription ? 'subscriptionUrl' : 'proxyUrl'
      const results = await onValidateCandidates(kind, values)
      setDialogResults(results)
      if (results.length === 0) {
        setDialogError(strings.validation.requestFailed)
      }
    } catch (err) {
      setDialogResults([])
      setDialogError(err instanceof Error ? err.message : strings.validation.requestFailed)
    } finally {
      setDialogValidating(false)
    }
  }

  const handleAddSubscription = async () => {
    const candidate = dialogAvailableResults[0]
    if (!candidate) return
    const nextValue = candidate.result.normalizedValue ?? candidate.value
    try {
      await persistSubscriptionUrls([...subscriptionUrls, nextValue])
      closeDialog()
    } catch (err) {
      setDialogError(err instanceof Error ? err.message : strings.config.saveFailed)
    }
  }

  const handleAddManualBatch = async () => {
    const nextValues = normalizeEntries([
      ...manualUrls,
      ...dialogAvailableResults.map((entry) => entry.result.normalizedValue ?? entry.value),
    ])
    if (nextValues.length === manualUrls.length) return
    try {
      await persistManualUrls(nextValues)
      closeDialog()
    } catch (err) {
      setDialogError(err instanceof Error ? err.message : strings.config.saveFailed)
    }
  }

  const handleAddManualEntry = async (entry: ForwardProxyValidationEntry) => {
    const nextValue = entry.result.normalizedValue ?? entry.value
    try {
      await persistManualUrls([...manualUrls, nextValue])
      setDialogResults((previous) =>
        previous.map((item) =>
          item.id === entry.id
            ? {
                ...item,
                result: {
                  ...item.result,
                  message: strings.config.addedToList,
                },
              }
            : item,
        ),
      )
    } catch (err) {
      setDialogError(err instanceof Error ? err.message : strings.config.saveFailed)
    }
  }

  return (
    <div className="forward-proxy-stack">
      <Card className="surface panel forward-proxy-summary-panel">
        <CardHeader className="forward-proxy-panel-header forward-proxy-summary-header">
          <div className="forward-proxy-panel-heading">
            <CardTitle>{strings.title}</CardTitle>
            <CardDescription className="panel-description">{strings.description}</CardDescription>
          </div>
          <div className="forward-proxy-panel-meta">
            <div className="forward-proxy-toolbar">
              <Button type="button" variant="outline" onClick={onRefresh}>
                {strings.actions.refresh}
              </Button>
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
          </div>
        </CardHeader>
        <CardContent className="forward-proxy-panel-content forward-proxy-summary-content">
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
                          </div>
                          <div className="forward-proxy-node-chip-row">
                            <span className="forward-proxy-node-chip-text">
                              {strings.nodes.primary}: <strong>{formatNumber(node.primaryAssignmentCount)}</strong>
                            </span>
                            <span className="forward-proxy-node-chip-text">
                              {strings.nodes.secondary}: <strong>{formatNumber(node.secondaryAssignmentCount)}</strong>
                            </span>
                            <Badge variant={node.source === 'subscription' ? 'info' : node.source === 'manual' ? 'outline' : 'neutral'}>
                              {getSourceLabel(strings, node.source)}
                            </Badge>
                            <Badge variant={stateBadge.variant}>{stateBadge.label}</Badge>
                          </div>
                        </CardHeader>
                        <CardContent className="forward-proxy-node-mobile-content">
                          <div className="forward-proxy-node-mobile-grid">
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
                          <TableHead
                            className={`${getWindowColumnClassName(index)} forward-proxy-table-head-nowrap`}
                            key={`head-${windowDefinition.key}`}
                          >
                            {strings.windows[windowDefinition.translationKey]}
                          </TableHead>
                        ))}
                        <TableHead className="w-[18%] forward-proxy-table-head-nowrap">
                          {strings.nodes.table.activity24h}
                        </TableHead>
                        <TableHead className="w-[18%] forward-proxy-table-head-nowrap">
                          {strings.nodes.table.weight24h}
                        </TableHead>
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
                                </div>
                                <div className="forward-proxy-node-chip-row">
                                  <span className="forward-proxy-node-chip-text">
                                    {strings.nodes.primary}: <strong>{formatNumber(node.primaryAssignmentCount)}</strong>
                                  </span>
                                  <span className="forward-proxy-node-chip-text">
                                    {strings.nodes.secondary}: <strong>{formatNumber(node.secondaryAssignmentCount)}</strong>
                                  </span>
                                  <Badge variant={node.source === 'subscription' ? 'info' : node.source === 'manual' ? 'outline' : 'neutral'}>
                                    {getSourceLabel(strings, node.source)}
                                  </Badge>
                                  <Badge variant={stateBadge.variant}>{stateBadge.label}</Badge>
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
            <div className="rounded-xl border border-border/70 bg-card/45 px-3.5 py-3">
              <div className="flex flex-wrap items-center gap-3">
                <Button type="button" variant="secondary" size="sm" onClick={() => openDialog('manual')} disabled={saving}>
                  {strings.config.addManual}
                </Button>
                <span className="text-xs text-muted-foreground">
                  {strings.config.manualCount.replace('{count}', formatNumber(manualUrls.length))}
                </span>
                <Button type="button" variant="secondary" size="sm" onClick={() => openDialog('subscription')} disabled={saving}>
                  {strings.config.addSubscription}
                </Button>
                <span className="text-xs text-muted-foreground">
                  {strings.config.subscriptionCount.replace('{count}', formatNumber(subscriptionUrls.length))}
                </span>
              </div>
            </div>

            <div className="grid gap-3 lg:grid-cols-2">
              <Card className="forward-proxy-editor-card">
                <CardHeader className="forward-proxy-editor-head">
                  <div>
                    <CardTitle className="text-base">{strings.config.subscriptionsTitle}</CardTitle>
                    <CardDescription className="panel-description">{strings.config.subscriptionsDescription}</CardDescription>
                  </div>
                  <Badge variant="info">{formatNumber(subscriptionUrls.length)}</Badge>
                </CardHeader>
                <CardContent className="forward-proxy-editor-card-content">
                  {subscriptionUrls.length === 0 ? (
                    <div className="empty-state alert">{strings.config.subscriptionListEmpty}</div>
                  ) : (
                    <ul className="space-y-2">
                      {subscriptionUrls.map((subscriptionUrl, index) => (
                        <li
                          key={`subscription-${subscriptionUrl}`}
                          className="flex items-center gap-3 rounded-xl border border-border/70 bg-card/65 px-3 py-2"
                        >
                          <div className="min-w-0 flex-1">
                            <div className="truncate text-sm font-semibold">
                              {extractListDisplayName(
                                subscriptionUrl,
                                strings.config.subscriptionItemFallback.replace('{index}', String(index + 1)),
                              )}
                            </div>
                            <div className="truncate text-xs text-muted-foreground">{subscriptionUrl}</div>
                          </div>
                          <Badge variant="outline">{extractProtocolName(subscriptionUrl)}</Badge>
                          <Button
                            type="button"
                            size="xs"
                            variant="ghost"
                            onClick={() => void handleRemoveSubscription(subscriptionUrl)}
                            disabled={saving}
                          >
                            {strings.config.remove}
                          </Button>
                        </li>
                      ))}
                    </ul>
                  )}
                </CardContent>
              </Card>

              <Card className="forward-proxy-editor-card">
                <CardHeader className="forward-proxy-editor-head">
                  <div>
                    <CardTitle className="text-base">{strings.config.manualTitle}</CardTitle>
                    <CardDescription className="panel-description">{strings.config.manualDescription}</CardDescription>
                  </div>
                  <Badge variant="outline">{formatNumber(manualUrls.length)}</Badge>
                </CardHeader>
                <CardContent className="forward-proxy-editor-card-content">
                  {manualUrls.length === 0 ? (
                    <div className="empty-state alert">{strings.config.manualListEmpty}</div>
                  ) : (
                    <ul className="space-y-2">
                      {manualUrls.map((proxyUrl, index) => (
                        <li
                          key={`manual-${proxyUrl}`}
                          className="flex items-center gap-3 rounded-xl border border-border/70 bg-card/65 px-3 py-2"
                        >
                          <div className="min-w-0 flex-1">
                            <div className="truncate text-sm font-semibold">
                              {extractListDisplayName(
                                proxyUrl,
                                strings.config.manualItemFallback.replace('{index}', String(index + 1)),
                              )}
                            </div>
                            <div className="truncate text-xs text-muted-foreground">{proxyUrl}</div>
                          </div>
                          <Badge variant="outline">{extractProtocolName(proxyUrl)}</Badge>
                          <Button
                            type="button"
                            size="xs"
                            variant="ghost"
                            onClick={() => void handleRemoveManual(proxyUrl)}
                            disabled={saving}
                          >
                            {strings.config.remove}
                          </Button>
                        </li>
                      ))}
                    </ul>
                  )}
                </CardContent>
              </Card>
            </div>

            <div className="grid gap-3 lg:grid-cols-[minmax(0,280px)_1fr]">
              <Card className="forward-proxy-field-card">
                <CardContent className="forward-proxy-field-card-content">
                  <label className="forward-proxy-field">
                    <span className="forward-proxy-field-label">{strings.config.subscriptionIntervalLabel}</span>
                    <Select value={selectedInterval} onValueChange={(value) => void handleIntervalChange(value)} disabled={saving}>
                      <SelectTrigger>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {FORWARD_PROXY_INTERVAL_OPTIONS.map((option) => (
                          <SelectItem key={option.value} value={option.value}>
                            {option.label}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
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
                      checked={settings?.insertDirect ?? true}
                      onChange={(event) => void handleInsertDirectChange(event.target.checked)}
                      disabled={saving}
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

      <Dialog open={dialogKind != null} onOpenChange={(open) => (!open ? closeDialog() : undefined)}>
        <DialogContent className="max-w-3xl border-border/90 bg-background shadow-2xl">
          <DialogHeader>
            <DialogTitle>
              {dialogIsSubscription ? strings.config.subscriptionDialogTitle : strings.config.manualDialogTitle}
            </DialogTitle>
            <DialogDescription>
              {dialogIsSubscription
                ? strings.config.subscriptionDialogDescription
                : strings.config.manualDialogDescription}
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-4">
            <div className="space-y-2">
              <label className="text-sm font-medium text-foreground" htmlFor="forward-proxy-dialog-input">
                {dialogIsSubscription
                  ? strings.config.subscriptionDialogInputLabel
                  : strings.config.manualDialogInputLabel}
              </label>
              {dialogIsSubscription ? (
                <Input
                  id="forward-proxy-dialog-input"
                  value={dialogInput}
                  placeholder={strings.config.subscriptionsPlaceholder}
                  onChange={(event) => {
                    setDialogInput(event.target.value)
                    setDialogResults([])
                    setDialogError(null)
                  }}
                />
              ) : (
                <Textarea
                  id="forward-proxy-dialog-input"
                  rows={7}
                  value={dialogInput}
                  placeholder={strings.config.manualPlaceholder}
                  onChange={(event) => {
                    setDialogInput(event.target.value)
                    setDialogResults([])
                    setDialogError(null)
                  }}
                />
              )}
            </div>

            {dialogError && (
              <div className="alert alert-error" role="alert">
                {dialogError}
              </div>
            )}

            {dialogValidating && (
              <div className="alert" role="status">
                {strings.config.validating}
              </div>
            )}

            {!dialogIsSubscription && dialogResults.length > 0 && (
              <div className="rounded-2xl border border-border/70 bg-card/35">
                <Table className="table-fixed text-xs">
                  <TableHeader className="bg-muted/40 uppercase tracking-[0.08em] text-[11px] text-muted-foreground">
                    <TableRow className="hover:bg-transparent">
                      <TableHead className="w-12">#</TableHead>
                      <TableHead>{strings.config.resultNode}</TableHead>
                      <TableHead className="w-24">{strings.config.resultStatus}</TableHead>
                      <TableHead className="w-28 text-right">{strings.config.resultLatency}</TableHead>
                      <TableHead className="w-24 text-right">{strings.config.resultAction}</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {dialogResults.map((entry, index) => (
                      <TableRow key={entry.id} className="border-border/65">
                        <TableCell>{index + 1}</TableCell>
                        <TableCell className="min-w-0">
                          <div className="truncate font-medium">
                            {extractListDisplayName(
                              entry.result.normalizedValue ?? entry.value,
                              strings.config.manualItemFallback.replace('{index}', String(index + 1)),
                            )}
                          </div>
                          <div className="truncate text-[11px] text-muted-foreground">
                            {entry.result.normalizedValue ?? entry.value}
                          </div>
                        </TableCell>
                        <TableCell>
                          <div className="flex flex-col items-start gap-1">
                            <Badge variant={entry.result.ok ? 'success' : 'destructive'}>
                              {entry.result.ok
                                ? strings.validation.ok
                                : mapValidationErrorLabel(strings, entry.result.errorCode)}
                            </Badge>
                            {!entry.result.ok && (
                              <span className="text-[11px] text-muted-foreground">{entry.result.message}</span>
                            )}
                          </div>
                        </TableCell>
                        <TableCell className="text-right text-muted-foreground">{formatLatency(entry.result.latencyMs)}</TableCell>
                        <TableCell className="text-right">
                          <Button
                            type="button"
                            size="xs"
                            onClick={() => void handleAddManualEntry(entry)}
                            disabled={!entry.result.ok || saving}
                          >
                            {strings.config.add}
                          </Button>
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </div>
            )}

            {dialogIsSubscription && dialogResults[0] && (
              <Card className="forward-proxy-validation-card">
                <CardContent className="forward-proxy-validation-card-content">
                  <div className="forward-proxy-validation-head">
                    <Badge variant={dialogResults[0].result.ok ? 'success' : 'destructive'}>
                      {dialogResults[0].result.ok
                        ? strings.validation.ok
                        : mapValidationErrorLabel(strings, dialogResults[0].result.errorCode)}
                    </Badge>
                    <Badge variant="outline">{strings.validation.subscriptionKind}</Badge>
                  </div>
                  <code className="forward-proxy-code-block">
                    {dialogResults[0].result.normalizedValue ?? dialogResults[0].value}
                  </code>
                  <p className="forward-proxy-validation-message">{dialogResults[0].result.message}</p>
                  <div className="forward-proxy-validation-meta">
                    <span>
                      {strings.validation.discoveredNodes}: {formatNumber(dialogResults[0].result.discoveredNodes ?? 0)}
                    </span>
                    <span>
                      {strings.validation.latency}: {formatLatency(dialogResults[0].result.latencyMs)}
                    </span>
                  </div>
                </CardContent>
              </Card>
            )}
          </div>

          <DialogFooter>
            <Button type="button" variant="ghost" onClick={closeDialog} disabled={dialogValidating || saving}>
              {strings.config.cancel}
            </Button>
            <Button
              type="button"
              variant="secondary"
              onClick={() => void handleValidateDialog()}
              disabled={dialogValidating || saving}
            >
              {strings.config.validate}
            </Button>
            {dialogIsSubscription ? (
              <Button
                type="button"
                onClick={() => void handleAddSubscription()}
                disabled={dialogAvailableResults.length === 0 || saving}
              >
                {strings.config.add}
              </Button>
            ) : (
              <Button
                type="button"
                onClick={() => void handleAddManualBatch()}
                disabled={dialogAvailableResults.length === 0 || saving}
              >
                {strings.config.importAvailable.replace('{count}', formatNumber(dialogAvailableResults.length))}
              </Button>
            )}
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
