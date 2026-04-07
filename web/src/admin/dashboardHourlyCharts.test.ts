import { describe, expect, it } from 'bun:test'

import {
  buildDashboardHourlyRequestWindowFixture,
  buildDeltaSeriesValues,
  buildHourlyBucketLookup,
  createEmptyDashboardHourlyRequestWindow,
  getVisibleHourlyBuckets,
  toggleSeriesSelection,
} from './dashboardHourlyCharts'

describe('dashboardHourlyCharts helpers', () => {
  it('returns the latest visible bucket slice and keeps retained metadata intact', () => {
    const window = buildDashboardHourlyRequestWindowFixture()

    expect(window.retainedBuckets).toBe(49)
    expect(window.visibleBuckets).toBe(25)
    expect(getVisibleHourlyBuckets(window)).toHaveLength(25)
    expect(getVisibleHourlyBuckets(window)[0]?.bucketStart).toBe(window.buckets[24]?.bucketStart)
    expect(getVisibleHourlyBuckets(window).at(-1)?.bucketStart).toBe(window.buckets.at(-1)?.bucketStart)
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
})
