import type { DashboardHourlyRequestBucket, DashboardHourlyRequestWindow } from '../api'

export type DashboardHourlyChartMode =
  | 'results'
  | 'types'
  | 'resultsDelta'
  | 'typesDelta'
  | 'resultsArea'
  | 'typesArea'

export type DashboardResultSeriesId =
  | 'secondarySuccess'
  | 'primarySuccess'
  | 'secondaryFailure'
  | 'primaryFailure429'
  | 'primaryFailureOther'
  | 'unknown'

export type DashboardTypeSeriesId =
  | 'mcpNonBillable'
  | 'mcpBillable'
  | 'apiNonBillable'
  | 'apiBillable'

export type DashboardDeltaSelection<T extends string> = T | 'all'

export const DASHBOARD_RESULT_SERIES_ORDER = [
  'secondarySuccess',
  'primarySuccess',
  'secondaryFailure',
  'primaryFailure429',
  'primaryFailureOther',
  'unknown',
] as const satisfies ReadonlyArray<DashboardResultSeriesId>

export const DASHBOARD_TYPE_SERIES_ORDER = [
  'mcpNonBillable',
  'mcpBillable',
  'apiNonBillable',
  'apiBillable',
] as const satisfies ReadonlyArray<DashboardTypeSeriesId>

export const DEFAULT_VISIBLE_RESULT_SERIES = [
  ...DASHBOARD_RESULT_SERIES_ORDER,
] as const satisfies ReadonlyArray<DashboardResultSeriesId>

export const DEFAULT_VISIBLE_TYPE_SERIES = [
  ...DASHBOARD_TYPE_SERIES_ORDER,
] as const satisfies ReadonlyArray<DashboardTypeSeriesId>

export const DASHBOARD_AREA_CHART_STACK_ID = 'area'
export const DASHBOARD_AREA_CHART_TENSION = 0.18
export const DASHBOARD_REALTIME_BUCKET_SECONDS = 5 * 60
export const DASHBOARD_REALTIME_VISIBLE_BUCKETS = 73
export const DASHBOARD_REALTIME_RETAINED_BUCKETS = 589
export const DASHBOARD_COMPARISON_BUCKET_SECONDS = 60 * 60

export type DashboardAreaFillTarget = 'origin' | '-1'

export interface DashboardHourlyChartPreferences {
  chartMode: DashboardHourlyChartMode
  visibleResultSeries: DashboardResultSeriesId[]
  visibleTypeSeries: DashboardTypeSeriesId[]
  resultDeltaSeries: DashboardDeltaSelection<DashboardResultSeriesId>
  typeDeltaSeries: DashboardDeltaSelection<DashboardTypeSeriesId>
}

export interface DashboardHourlyChartPreferencesInput {
  chartMode?: DashboardHourlyChartMode
  visibleResultSeries?: ReadonlyArray<DashboardResultSeriesId>
  visibleTypeSeries?: ReadonlyArray<DashboardTypeSeriesId>
  resultDeltaSeries?: DashboardDeltaSelection<DashboardResultSeriesId>
  typeDeltaSeries?: DashboardDeltaSelection<DashboardTypeSeriesId>
}

export interface DashboardHourlyRangeSlot {
  bucketStart: number
  bucket: DashboardHourlyRequestBucket | null
}

export interface DashboardHourlyWindowView {
  bucketSeconds: number
  visibleBuckets: number
  slots: DashboardHourlyRangeSlot[]
}

export interface DashboardVisibleWindow {
  rangeStart: number
  rangeEnd: number
  slots: DashboardHourlyRangeSlot[]
}

export interface DashboardAreaStackLayer<T extends string> {
  seriesId: T
  type: 'line'
  fill: DashboardAreaFillTarget
  stack: typeof DASHBOARD_AREA_CHART_STACK_ID
  tension: typeof DASHBOARD_AREA_CHART_TENSION
  borderWidth: 2
  pointRadius: 0
  pointHoverRadius: 3
  spanGaps: false
}

function positiveModulo(value: number, divisor: number): number {
  return ((value % divisor) + divisor) % divisor
}

function normalizeSeriesSelection<T extends string>(
  value: unknown,
  allowed: ReadonlyArray<T>,
  fallback: ReadonlyArray<T>,
): T[] {
  if (!Array.isArray(value)) return [...fallback]
  const seen = new Set<T>()
  const normalized: T[] = []
  for (const item of value) {
    if (typeof item !== 'string') continue
    if (!allowed.includes(item as T)) continue
    const typed = item as T
    if (seen.has(typed)) continue
    seen.add(typed)
    normalized.push(typed)
  }
  return normalized
}

function normalizeDeltaSelection<T extends string>(
  value: unknown,
  allowed: ReadonlyArray<T>,
  fallback: DashboardDeltaSelection<T>,
): DashboardDeltaSelection<T> {
  if (value === 'all') return 'all'
  if (typeof value === 'string' && allowed.includes(value as T)) {
    return value as T
  }
  return fallback
}

export function createDashboardHourlyChartPreferences(
  overrides: DashboardHourlyChartPreferencesInput = {},
): DashboardHourlyChartPreferences {
  return {
    chartMode:
      overrides.chartMode === 'results'
        || overrides.chartMode === 'types'
        || overrides.chartMode === 'resultsDelta'
        || overrides.chartMode === 'typesDelta'
        || overrides.chartMode === 'resultsArea'
        || overrides.chartMode === 'typesArea'
        ? overrides.chartMode
        : 'results',
    visibleResultSeries: normalizeSeriesSelection(
      overrides.visibleResultSeries,
      DASHBOARD_RESULT_SERIES_ORDER,
      DEFAULT_VISIBLE_RESULT_SERIES,
    ),
    visibleTypeSeries: normalizeSeriesSelection(
      overrides.visibleTypeSeries,
      DASHBOARD_TYPE_SERIES_ORDER,
      DEFAULT_VISIBLE_TYPE_SERIES,
    ),
    resultDeltaSeries: normalizeDeltaSelection(
      overrides.resultDeltaSeries,
      DASHBOARD_RESULT_SERIES_ORDER,
      'all',
    ),
    typeDeltaSeries: normalizeDeltaSelection(
      overrides.typeDeltaSeries,
      DASHBOARD_TYPE_SERIES_ORDER,
      'all',
    ),
  }
}

export function readDashboardHourlyChartPreferences(
  storage: Pick<Storage, 'getItem'> | null | undefined,
  key: string | null | undefined,
  legacyKeys: ReadonlyArray<string> = [],
): DashboardHourlyChartPreferences | null {
  if (storage == null || !key) return null
  for (const candidateKey of [key, ...legacyKeys]) {
    const raw = storage.getItem(candidateKey)
    if (!raw) continue
    try {
      const parsed = JSON.parse(raw) as Partial<DashboardHourlyChartPreferences>
      return createDashboardHourlyChartPreferences(parsed)
    } catch {
      continue
    }
  }
  return null
}

export function writeDashboardHourlyChartPreferences(
  storage: Pick<Storage, 'setItem'> | null | undefined,
  key: string | null | undefined,
  value: DashboardHourlyChartPreferences,
): void {
  if (storage == null || !key) return
  storage.setItem(key, JSON.stringify(value))
}

const bucketLabelFormatterCache = new Map<string, {
  dayFormatter: Intl.DateTimeFormat
  hourFormatter: Intl.DateTimeFormat
}>()

const bucketDayKeyFormatterCache = new Map<string, Intl.DateTimeFormat>()

function getHourlyBucketDayKeyFormatter(timeZone?: string): Intl.DateTimeFormat {
  const cacheKey = timeZone ?? '__local__'
  const cached = bucketDayKeyFormatterCache.get(cacheKey)
  if (cached) return cached

  const formatter = new Intl.DateTimeFormat('en-CA', {
    ...(timeZone ? { timeZone } : {}),
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
  })
  bucketDayKeyFormatterCache.set(cacheKey, formatter)
  return formatter
}

export function getHourlyBucketDayKey(bucketStart: number, timeZone?: string): string {
  return getHourlyBucketDayKeyFormatter(timeZone).format(new Date(bucketStart * 1000))
}

function getHourlyBucketLabelFormatters(timeZone?: string): {
  dayFormatter: Intl.DateTimeFormat
  hourFormatter: Intl.DateTimeFormat
} {
  const cacheKey = timeZone ?? '__local__'
  const cached = bucketLabelFormatterCache.get(cacheKey)
  if (cached) return cached

  const formatOptions = timeZone ? { timeZone } : {}
  const formatters = {
    dayFormatter: new Intl.DateTimeFormat('en-US', {
      ...formatOptions,
      month: '2-digit',
      day: '2-digit',
    }),
    hourFormatter: new Intl.DateTimeFormat('en-US', {
      ...formatOptions,
      hour: '2-digit',
      minute: '2-digit',
      hour12: false,
    }),
  }
  bucketLabelFormatterCache.set(cacheKey, formatters)
  return formatters
}

export function createEmptyDashboardHourlyRequestWindow(): DashboardHourlyRequestWindow {
  return {
    bucketSeconds: DASHBOARD_REALTIME_BUCKET_SECONDS,
    visibleBuckets: DASHBOARD_REALTIME_VISIBLE_BUCKETS,
    retainedBuckets: DASHBOARD_REALTIME_RETAINED_BUCKETS,
    buckets: [],
  }
}

export function getVisibleHourlyBuckets(window: DashboardHourlyRequestWindow): DashboardHourlyRequestBucket[] {
  const retained = Number.isFinite(window.visibleBuckets) && window.visibleBuckets > 0
    ? Math.trunc(window.visibleBuckets)
    : window.buckets.length
  if (retained <= 0) return []
  return window.buckets.slice(-retained)
}

export function getVisibleHourlyWindow(window: DashboardHourlyRequestWindow): DashboardVisibleWindow {
  const visibleBuckets = getVisibleHourlyBuckets(window)
  const visibleBucketCount = Number.isFinite(window.visibleBuckets) && window.visibleBuckets > 0
    ? Math.trunc(window.visibleBuckets)
    : visibleBuckets.length
  const bucketSeconds = Number.isFinite(window.bucketSeconds) && window.bucketSeconds > 0
    ? Math.trunc(window.bucketSeconds)
    : 3600

  if (visibleBuckets.length === 0 || visibleBucketCount <= 0) {
    return {
      rangeStart: 0,
      rangeEnd: 0,
      slots: [],
    }
  }

  const latestBucketStart = visibleBuckets.at(-1)?.bucketStart ?? 0
  const rangeStart = latestBucketStart - (visibleBucketCount - 1) * bucketSeconds
  const rangeEnd = latestBucketStart + bucketSeconds

  return {
    rangeStart,
    rangeEnd,
    slots: buildHourlyRangeSlots(window, rangeStart, rangeEnd),
  }
}

function floorToLocalHourStart(timestamp: number): number {
  const date = new Date(timestamp * 1000)
  date.setMinutes(0, 0, 0)
  return Math.trunc(date.getTime() / 1000)
}

export function buildRollingHourlyWindow(window: DashboardHourlyRequestWindow): DashboardHourlyWindowView {
  const visibleBuckets = getVisibleHourlyBuckets(window)
  if (visibleBuckets.length === 0) {
    return {
      bucketSeconds: DASHBOARD_COMPARISON_BUCKET_SECONDS,
      visibleBuckets: 0,
      slots: [],
    }
  }

  const bucketSeconds = DASHBOARD_COMPARISON_BUCKET_SECONDS
  const latestBucketStart = visibleBuckets.at(-1)?.bucketStart ?? 0
  const latestHourStart = floorToLocalHourStart(latestBucketStart)
  const rangeEnd = latestHourStart + bucketSeconds
  const rangeStart = latestHourStart - 24 * bucketSeconds

  return buildAggregatedHourlySlots(window, rangeStart, rangeEnd, bucketSeconds)
}

export function getCurrentPartialHourHighlightIndex(
  chartMode: DashboardHourlyChartMode,
  slots: ReadonlyArray<DashboardHourlyRangeSlot>,
): number | null {
  if (slots.length === 0) return null
  return chartMode === 'results' || chartMode === 'types' ? slots.length - 1 : null
}

export function getDashboardHourlyBarChartKey(
  chartMode: DashboardHourlyChartMode,
  slots: ReadonlyArray<DashboardHourlyRangeSlot>,
  markerStyleToken = '',
): string {
  const highlightIndex = getCurrentPartialHourHighlightIndex(chartMode, slots)
  const styleSuffix = markerStyleToken.length > 0 ? `:${markerStyleToken}` : ''
  return highlightIndex == null
    ? `${chartMode}:no-current-partial-hour:${slots.length}${styleSuffix}`
    : `${chartMode}:current-partial-hour-${highlightIndex}:${slots.length}${styleSuffix}`
}

export function buildDashboardAreaStackLayers<T extends string>(
  visibleSeries: ReadonlyArray<T>,
): DashboardAreaStackLayer<T>[] {
  return visibleSeries.map((seriesId, index) => ({
    seriesId,
    type: 'line',
    fill: index === 0 ? 'origin' : '-1',
    stack: DASHBOARD_AREA_CHART_STACK_ID,
    tension: DASHBOARD_AREA_CHART_TENSION,
    borderWidth: 2,
    pointRadius: 0,
    pointHoverRadius: 3,
    spanGaps: false,
  }))
}

export function getCurrentDayHourlyBuckets(
  window: DashboardHourlyRequestWindow,
  timeZone?: string,
): DashboardHourlyRequestBucket[] {
  const latestBucket = window.buckets.at(-1)
  if (!latestBucket) return []
  const latestDayKey = getHourlyBucketDayKey(latestBucket.bucketStart, timeZone)
  return window.buckets.filter((bucket) => getHourlyBucketDayKey(bucket.bucketStart, timeZone) === latestDayKey)
}

export function getHourlyBucketsInRange(
  window: DashboardHourlyRequestWindow,
  rangeStart: number,
  rangeEnd: number,
): DashboardHourlyRequestBucket[] {
  if (!Number.isFinite(rangeStart) || !Number.isFinite(rangeEnd) || rangeEnd <= rangeStart) return []
  return window.buckets.filter((bucket) => bucket.bucketStart >= rangeStart && bucket.bucketStart < rangeEnd)
}

export function buildHourlyRangeSlots(
  window: DashboardHourlyRequestWindow,
  rangeStart: number,
  rangeEnd: number,
): DashboardHourlyRangeSlot[] {
  if (!Number.isFinite(rangeStart) || !Number.isFinite(rangeEnd) || rangeEnd <= rangeStart) return []
  const bucketSeconds = Number.isFinite(window.bucketSeconds) && window.bucketSeconds > 0
    ? Math.trunc(window.bucketSeconds)
    : 3600
  const lookup = buildHourlyBucketLookup(window.buckets)
  const alignmentOffset = window.buckets[0]
    ? positiveModulo(window.buckets[0].bucketStart, bucketSeconds)
    : positiveModulo(rangeStart, bucketSeconds)
  const rangeOffset = positiveModulo(rangeStart - alignmentOffset, bucketSeconds)
  const firstBucketStart = rangeOffset === 0
    ? rangeStart
    : rangeStart + bucketSeconds - rangeOffset
  const slots: DashboardHourlyRangeSlot[] = []
  for (let bucketStart = firstBucketStart; bucketStart < rangeEnd; bucketStart += bucketSeconds) {
    slots.push({
      bucketStart,
      bucket: lookup.get(bucketStart) ?? null,
    })
  }
  return slots
}

export function buildHourlyBucketLookup(
  buckets: ReadonlyArray<DashboardHourlyRequestBucket>,
): Map<number, DashboardHourlyRequestBucket> {
  return new Map(buckets.map((bucket) => [bucket.bucketStart, bucket]))
}

function createEmptyBucket(bucketStart: number): DashboardHourlyRequestBucket {
  return {
    bucketStart,
    secondarySuccess: 0,
    primarySuccess: 0,
    secondaryFailure: 0,
    primaryFailure429: 0,
    primaryFailureOther: 0,
    unknown: 0,
    mcpNonBillable: 0,
    mcpBillable: 0,
    apiNonBillable: 0,
    apiBillable: 0,
  }
}

function addBucketValues(target: DashboardHourlyRequestBucket, source: DashboardHourlyRequestBucket): void {
  target.secondarySuccess += source.secondarySuccess
  target.primarySuccess += source.primarySuccess
  target.secondaryFailure += source.secondaryFailure
  target.primaryFailure429 += source.primaryFailure429
  target.primaryFailureOther += source.primaryFailureOther
  target.unknown += source.unknown
  target.mcpNonBillable += source.mcpNonBillable
  target.mcpBillable += source.mcpBillable
  target.apiNonBillable += source.apiNonBillable
  target.apiBillable += source.apiBillable
}

export function buildAggregatedHourlySlots(
  window: DashboardHourlyRequestWindow,
  rangeStart: number,
  rangeEnd: number,
  outputBucketSeconds = DASHBOARD_COMPARISON_BUCKET_SECONDS,
): DashboardHourlyWindowView {
  if (!Number.isFinite(rangeStart) || !Number.isFinite(rangeEnd) || rangeEnd <= rangeStart) {
    return { bucketSeconds: outputBucketSeconds, visibleBuckets: 0, slots: [] }
  }
  const bucketSeconds = Number.isFinite(outputBucketSeconds) && outputBucketSeconds > 0
    ? Math.trunc(outputBucketSeconds)
    : DASHBOARD_COMPARISON_BUCKET_SECONDS
  const firstBucketStart = rangeStart
  const buckets = new Map<number, DashboardHourlyRequestBucket>()
  const hasValues = new Set<number>()
  for (const bucket of window.buckets) {
    if (bucket.bucketStart < rangeStart || bucket.bucketStart >= rangeEnd) continue
    const bucketStart = rangeStart + Math.floor((bucket.bucketStart - rangeStart) / bucketSeconds) * bucketSeconds
    const aggregate = buckets.get(bucketStart) ?? createEmptyBucket(bucketStart)
    addBucketValues(aggregate, bucket)
    buckets.set(bucketStart, aggregate)
    hasValues.add(bucketStart)
  }
  const slots: DashboardHourlyRangeSlot[] = []
  for (let bucketStart = firstBucketStart; bucketStart < rangeEnd; bucketStart += bucketSeconds) {
    slots.push({
      bucketStart,
      bucket: hasValues.has(bucketStart) ? buckets.get(bucketStart) ?? createEmptyBucket(bucketStart) : null,
    })
  }
  return {
    bucketSeconds,
    visibleBuckets: slots.length,
    slots,
  }
}

export function formatHourlyBucketLabel(bucketStart: number, timeZone?: string): [string, string] {
  const date = new Date(bucketStart * 1000)
  const { dayFormatter, hourFormatter } = getHourlyBucketLabelFormatters(timeZone)
  return [dayFormatter.format(date), hourFormatter.format(date)]
}

export function getResultSeriesValue(bucket: DashboardHourlyRequestBucket, series: DashboardResultSeriesId): number {
  switch (series) {
    case 'secondarySuccess':
      return bucket.secondarySuccess
    case 'primarySuccess':
      return bucket.primarySuccess
    case 'secondaryFailure':
      return bucket.secondaryFailure
    case 'primaryFailure429':
      return bucket.primaryFailure429
    case 'primaryFailureOther':
      return bucket.primaryFailureOther
    case 'unknown':
      return bucket.unknown
  }
}

export function getTypeSeriesValue(bucket: DashboardHourlyRequestBucket, series: DashboardTypeSeriesId): number {
  switch (series) {
    case 'mcpNonBillable':
      return bucket.mcpNonBillable
    case 'mcpBillable':
      return bucket.mcpBillable
    case 'apiNonBillable':
      return bucket.apiNonBillable
    case 'apiBillable':
      return bucket.apiBillable
  }
}

export function toggleSeriesSelection<T extends string>(
  selected: ReadonlyArray<T>,
  value: T,
): T[] {
  return selected.includes(value)
    ? selected.filter((item) => item !== value)
    : [...selected, value]
}

export function buildDeltaSeriesValues<T extends DashboardResultSeriesId | DashboardTypeSeriesId>(
  buckets: ReadonlyArray<DashboardHourlyRequestBucket>,
  lookup: ReadonlyMap<number, DashboardHourlyRequestBucket>,
  series: T,
): number[] {
  return buckets.map((bucket) => {
    const baseline = lookup.get(bucket.bucketStart - 24 * 3600)
    if (!baseline) return 0
    if ((DASHBOARD_RESULT_SERIES_ORDER as readonly string[]).includes(series)) {
      return getResultSeriesValue(bucket, series as DashboardResultSeriesId)
        - getResultSeriesValue(baseline, series as DashboardResultSeriesId)
    }
    return getTypeSeriesValue(bucket, series as DashboardTypeSeriesId)
      - getTypeSeriesValue(baseline, series as DashboardTypeSeriesId)
  })
}

export function buildDeltaSeriesSlotValues<T extends DashboardResultSeriesId | DashboardTypeSeriesId>(
  slots: ReadonlyArray<DashboardHourlyRangeSlot>,
  comparisonSlots: ReadonlyArray<DashboardHourlyRangeSlot>,
  series: T,
): Array<number | null> {
  const slotCount = Math.max(slots.length, comparisonSlots.length)
  return Array.from({ length: slotCount }, (_, index) => {
    const slot = slots[index]
    const comparisonBucket = comparisonSlots[index]?.bucket ?? null
    if (!slot?.bucket || !comparisonBucket) return null
    if ((DASHBOARD_RESULT_SERIES_ORDER as readonly string[]).includes(series)) {
      return getResultSeriesValue(slot.bucket, series as DashboardResultSeriesId)
        - getResultSeriesValue(comparisonBucket, series as DashboardResultSeriesId)
    }
    return getTypeSeriesValue(slot.bucket, series as DashboardTypeSeriesId)
      - getTypeSeriesValue(comparisonBucket, series as DashboardTypeSeriesId)
  })
}

export function formatDashboardRealtimeWindowLabel(
  copy: string,
  bucketSeconds: number,
  visibleBuckets: number,
  count: number,
): string {
  const safeBucketSeconds = Number.isFinite(bucketSeconds) && bucketSeconds > 0
    ? Math.trunc(bucketSeconds)
    : DASHBOARD_REALTIME_BUCKET_SECONDS
  const safeVisibleBuckets = Number.isFinite(visibleBuckets) && visibleBuckets > 0
    ? Math.trunc(visibleBuckets)
    : count
  const durationMinutes = Math.max(0, Math.round((Math.max(0, safeVisibleBuckets - 1) * safeBucketSeconds) / 60))
  const bucketMinutes = Math.max(1, Math.round(safeBucketSeconds / 60))
  const durationLabel = durationMinutes % 60 === 0 && durationMinutes >= 60
    ? `${durationMinutes / 60}h`
    : `${durationMinutes}m`
  const bucketLabel = bucketMinutes >= 60 && bucketMinutes % 60 === 0
    ? `${bucketMinutes / 60}h`
    : `${bucketMinutes}m`
  return copy
    .replace('{range}', durationLabel)
    .replace('{bucket}', bucketLabel)
    .replace('{count}', String(count))
}

export function buildDashboardHourlyRequestWindowFixture({
  currentHourStart = Date.UTC(2026, 3, 7, 12, 0, 0) / 1000,
  bucketSeconds = DASHBOARD_REALTIME_BUCKET_SECONDS,
  visibleBuckets = DASHBOARD_REALTIME_VISIBLE_BUCKETS,
  retainedBuckets = DASHBOARD_REALTIME_RETAINED_BUCKETS,
  mapBucket,
}: {
  currentHourStart?: number
  bucketSeconds?: number
  visibleBuckets?: number
  retainedBuckets?: number
  mapBucket?: (args: { index: number; bucketStart: number; bucket: DashboardHourlyRequestBucket }) => Partial<DashboardHourlyRequestBucket>
} = {}): DashboardHourlyRequestWindow {
  const seriesStart = currentHourStart - bucketSeconds * (retainedBuckets - 1)
  const buckets: DashboardHourlyRequestBucket[] = Array.from({ length: retainedBuckets }, (_, index) => {
    const bucketStart = seriesStart + index * bucketSeconds
    const base = index + 1
    const bucket: DashboardHourlyRequestBucket = {
      bucketStart,
      secondarySuccess: (base % 4) + 1,
      primarySuccess: (base % 7) + 4,
      secondaryFailure: base % 3,
      primaryFailure429: base % 5 === 0 ? 2 : base % 4 === 0 ? 1 : 0,
      primaryFailureOther: base % 6 === 0 ? 2 : base % 3 === 0 ? 1 : 0,
      unknown: base % 8 === 0 ? 1 : 0,
      mcpNonBillable: base % 2,
      mcpBillable: (base % 5) + 2,
      apiNonBillable: base % 3,
      apiBillable: (base % 6) + 3,
    }
    return {
      ...bucket,
      ...(mapBucket?.({ index, bucketStart, bucket }) ?? {}),
    }
  })

  return {
    bucketSeconds,
    visibleBuckets,
    retainedBuckets,
    buckets,
  }
}
