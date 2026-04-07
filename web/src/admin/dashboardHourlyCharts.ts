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
  'secondarySuccess',
  'primarySuccess',
  'secondaryFailure',
  'primaryFailure429',
  'primaryFailureOther',
] as const satisfies ReadonlyArray<DashboardResultSeriesId>

export const DEFAULT_VISIBLE_TYPE_SERIES = [
  'mcpNonBillable',
  'mcpBillable',
  'apiNonBillable',
  'apiBillable',
] as const satisfies ReadonlyArray<DashboardTypeSeriesId>

const bucketLabelDayFormatter = new Intl.DateTimeFormat('en-US', {
  timeZone: 'UTC',
  month: '2-digit',
  day: '2-digit',
})

const bucketLabelHourFormatter = new Intl.DateTimeFormat('en-US', {
  timeZone: 'UTC',
  hour: '2-digit',
  minute: '2-digit',
  hour12: false,
})

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

export function formatHourlyBucketLabel(bucketStart: number): [string, string] {
  const date = new Date(bucketStart * 1000)
  return [bucketLabelDayFormatter.format(date), bucketLabelHourFormatter.format(date)]
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
  const seriesStart = currentHourStart - bucketSeconds * retainedBuckets
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
