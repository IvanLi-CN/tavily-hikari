import type { DashboardHourlyRequestBucket, DashboardHourlyRequestWindow } from '../api'

export type DashboardHourlyChartMode = 'results' | 'types' | 'resultsDelta' | 'typesDelta'

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
): DashboardHourlyChartPreferences | null {
  if (storage == null || !key) return null
  const raw = storage.getItem(key)
  if (!raw) return null
  try {
    const parsed = JSON.parse(raw) as Partial<DashboardHourlyChartPreferences>
    return createDashboardHourlyChartPreferences(parsed)
  } catch {
    return null
  }
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
    bucketSeconds: 3600,
    visibleBuckets: 25,
    retainedBuckets: 49,
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

export function buildHourlyBucketLookup(
  buckets: ReadonlyArray<DashboardHourlyRequestBucket>,
): Map<number, DashboardHourlyRequestBucket> {
  return new Map(buckets.map((bucket) => [bucket.bucketStart, bucket]))
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

export function buildDashboardHourlyRequestWindowFixture({
  currentHourStart = Date.UTC(2026, 3, 7, 12, 0, 0) / 1000,
  bucketSeconds = 3600,
  visibleBuckets = 25,
  retainedBuckets = 49,
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
