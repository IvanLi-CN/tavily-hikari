import { describe, expect, it } from 'bun:test'

import {
  buildDashboardHourlyRequestWindowFixture,
  buildDeltaSeriesValues,
  buildHourlyBucketLookup,
  createDashboardHourlyChartPreferences,
  createEmptyDashboardHourlyRequestWindow,
  DASHBOARD_RESULT_SERIES_ORDER,
  DASHBOARD_TYPE_SERIES_ORDER,
  formatHourlyBucketLabel,
  getVisibleHourlyBuckets,
  readDashboardHourlyChartPreferences,
  toggleSeriesSelection,
  writeDashboardHourlyChartPreferences,
} from './dashboardHourlyCharts'

describe('dashboardHourlyCharts helpers', () => {
  it('returns the latest visible bucket slice and keeps retained metadata intact', () => {
    const window = buildDashboardHourlyRequestWindowFixture()

    expect(window.retainedBuckets).toBe(49)
    expect(window.visibleBuckets).toBe(25)
    expect(getVisibleHourlyBuckets(window)).toHaveLength(25)
    expect(getVisibleHourlyBuckets(window)[0]?.bucketStart).toBe(window.buckets[24]?.bucketStart)
    expect(getVisibleHourlyBuckets(window).at(-1)?.bucketStart).toBe(window.buckets.at(-1)?.bucketStart)
    expect(window.buckets[0]?.bucketStart).toBe(window.buckets.at(-1)!.bucketStart - 48 * 3600)
  })

  it('anchors the latest bucket to the current hour instead of the previous closed hour', () => {
    const currentHourStart = Date.UTC(2026, 3, 7, 12, 0, 0) / 1000
    const window = buildDashboardHourlyRequestWindowFixture({ currentHourStart })

    expect(window.buckets.at(-1)?.bucketStart).toBe(currentHourStart)
    expect(getVisibleHourlyBuckets(window).at(-1)?.bucketStart).toBe(currentHourStart)
  })

  it('computes yesterday deltas from aligned hourly buckets', () => {
    const window = buildDashboardHourlyRequestWindowFixture({
      mapBucket: ({ index }) => ({
        primarySuccess: index === 6 ? 10 : index === 30 ? 50 : 0,
      }),
    })
    const visible = getVisibleHourlyBuckets(window)
    const lookup = buildHourlyBucketLookup(window.buckets)

    const delta = buildDeltaSeriesValues(visible, lookup, 'primarySuccess')
    const targetVisibleIndex = visible.findIndex((bucket) => bucket.bucketStart === window.buckets[30]?.bucketStart)

    expect(delta).toHaveLength(25)
    expect(targetVisibleIndex).toBeGreaterThanOrEqual(0)
    expect(delta[targetVisibleIndex]).toBe(40)
    expect(delta.filter((value) => value !== 0)).toEqual([40])
  })

  it('formats hourly bucket labels in the requested local timezone', () => {
    const bucketStart = Date.UTC(2026, 3, 10, 22, 0, 0) / 1000

    expect(formatHourlyBucketLabel(bucketStart, 'UTC')).toEqual(['04/10', '22:00'])
    expect(formatHourlyBucketLabel(bucketStart, 'Asia/Shanghai')).toEqual(['04/11', '06:00'])
  })

  it('toggles absolute-series visibility without mutating the source array', () => {
    const source = ['primarySuccess', 'secondaryFailure'] as const

    const removed = toggleSeriesSelection(source, 'primarySuccess')
    const added = toggleSeriesSelection(source, 'primaryFailure429')

    expect(removed).toEqual(['secondaryFailure'])
    expect(added).toEqual(['primarySuccess', 'secondaryFailure', 'primaryFailure429'])
    expect(source).toEqual(['primarySuccess', 'secondaryFailure'])
  })

  it('creates an empty fallback window for dashboard boot', () => {
    expect(createEmptyDashboardHourlyRequestWindow()).toEqual({
      bucketSeconds: 3600,
      visibleBuckets: 25,
      retainedBuckets: 49,
      buckets: [],
    })
  })

  it('defaults both absolute charts to all visible series', () => {
    const preferences = createDashboardHourlyChartPreferences()

    expect(preferences.visibleResultSeries).toEqual([...DASHBOARD_RESULT_SERIES_ORDER])
    expect(preferences.visibleTypeSeries).toEqual([...DASHBOARD_TYPE_SERIES_ORDER])
  })

  it('round-trips persisted chart preferences and preserves explicit empty absolute selections', () => {
    const storage = new Map<string, string>()
    const storageApi = {
      getItem(key: string) {
        return storage.get(key) ?? null
      },
      setItem(key: string, value: string) {
        storage.set(key, value)
      },
    }
    const key = 'admin.dashboard.hourly-request-charts.v1'

    writeDashboardHourlyChartPreferences(storageApi, key, {
      chartMode: 'results',
      visibleResultSeries: [],
      visibleTypeSeries: ['apiBillable'],
      resultDeltaSeries: 'primaryFailure429',
      typeDeltaSeries: 'all',
    })

    expect(readDashboardHourlyChartPreferences(storageApi, key)).toEqual({
      chartMode: 'results',
      visibleResultSeries: [],
      visibleTypeSeries: ['apiBillable'],
      resultDeltaSeries: 'primaryFailure429',
      typeDeltaSeries: 'all',
    })
  })
})
