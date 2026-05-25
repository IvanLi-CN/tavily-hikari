import type { DashboardHourlyRequestBucket, DashboardHourlyRequestWindow } from '../api'
import { buildHourlyRangeSlots } from './dashboardHourlyCharts'

export type DashboardBackdropMetricKey =
  | 'total'
  | 'valuableSuccess'
  | 'valuableFailure'
  | 'otherSuccess'
  | 'otherFailure'
  | 'unknown'
  | 'upstreamExhausted'
  | 'newKeys'
  | 'newQuarantines'

export interface DashboardCardBackdropSeries {
  current: Array<number | null>
  comparison: Array<number | null>
  baseline?: number
  color?: string
  comparisonColor?: string
}

export type DashboardCardBackdropMap = Partial<Record<DashboardBackdropMetricKey, DashboardCardBackdropSeries>>

export function getBackdropMetricKey(id: string): DashboardBackdropMetricKey | null {
  const normalizedId = id.replace(/^(today|month)-/, '')
  switch (normalizedId) {
    case 'total':
      return 'total'
    case 'valuable-success':
      return 'valuableSuccess'
    case 'valuable-failure':
      return 'valuableFailure'
    case 'other-success':
      return 'otherSuccess'
    case 'other-failure':
      return 'otherFailure'
    case 'unknown':
      return 'unknown'
    case 'upstream-exhausted':
      return 'upstreamExhausted'
    case 'new-keys':
      return 'newKeys'
    case 'new-quarantines':
      return 'newQuarantines'
    default:
      return null
  }
}

export function buildHourlyBackdropSeries(
  hourlyRequestWindow: DashboardHourlyRequestWindow,
  rangeStart: number,
  rangeEnd: number,
  metricKey: DashboardBackdropMetricKey = 'total',
  comparisonRangeStart = rangeStart,
  comparisonRangeEnd = rangeEnd,
): { current: Array<number | null>; comparison: Array<number | null> } {
  const visibleSlots = buildHourlyRangeSlots(hourlyRequestWindow, rangeStart, rangeEnd)
  const comparisonSlots = buildHourlyRangeSlots(hourlyRequestWindow, comparisonRangeStart, comparisonRangeEnd)
  const slotCount = Math.max(visibleSlots.length, comparisonSlots.length)
  const current = Array.from({ length: slotCount }, (_, index) => {
    const bucket = visibleSlots[index]?.bucket ?? null
    return bucket ? getBackdropMetricValue(bucket, metricKey) : null
  })
  const comparison = Array.from({ length: slotCount }, (_, index) => {
    const comparisonBucket = comparisonSlots[index]?.bucket ?? null
    return comparisonBucket ? getBackdropMetricValue(comparisonBucket, metricKey) : null
  })
  return { current, comparison }
}

function getBackdropMetricValue(
  bucket: DashboardHourlyRequestBucket,
  metricKey: DashboardBackdropMetricKey,
): number {
  switch (metricKey) {
    case 'total':
      return (
        bucket.secondarySuccess
        + bucket.primarySuccess
        + bucket.secondaryFailure
        + bucket.primaryFailure429
        + bucket.primaryFailureOther
        + bucket.unknown
      )
    case 'valuableSuccess':
      return bucket.primarySuccess
    case 'valuableFailure':
      return bucket.primaryFailure429 + bucket.primaryFailureOther
    case 'otherSuccess':
      return bucket.secondarySuccess
    case 'otherFailure':
      return bucket.secondaryFailure
    case 'unknown':
      return bucket.unknown
    case 'upstreamExhausted':
      return bucket.primaryFailure429
    case 'newKeys':
      return Math.max(0, Math.round((bucket.primarySuccess + bucket.secondarySuccess) / 220))
    case 'newQuarantines':
      return Math.max(0, Math.round((bucket.primaryFailure429 + bucket.primaryFailureOther + bucket.secondaryFailure) / 90))
  }
}
