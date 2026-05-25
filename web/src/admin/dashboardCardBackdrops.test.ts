import { describe, expect, it } from 'bun:test'

import { buildDashboardHourlyRequestWindowFixture } from './dashboardHourlyCharts'
import { buildHourlyBackdropSeries } from './dashboardCardBackdrops'

describe('dashboardCardBackdrops helpers', () => {
  it('uses explicit comparison window bounds instead of a fixed 24h offset', () => {
    const currentHourStart = Date.UTC(2026, 3, 7, 12, 0, 0) / 1000
    const window = buildDashboardHourlyRequestWindowFixture({
      currentHourStart,
      mapBucket: ({ index, bucket }) => ({
        secondarySuccess:
          index === 0 ? 10
            : index === 1 ? 20
              : index === 25 ? 100
                : index === 26 ? 200
                  : bucket.secondarySuccess,
      }),
    })

    const currentRangeStart = window.buckets[25]?.bucketStart ?? currentHourStart
    const currentRangeEnd = window.buckets[27]?.bucketStart ?? currentHourStart
    const comparisonRangeStart = window.buckets[0]?.bucketStart ?? currentHourStart
    const comparisonRangeEnd = window.buckets[2]?.bucketStart ?? currentHourStart

    const { current, comparison } = buildHourlyBackdropSeries(
      window,
      currentRangeStart,
      currentRangeEnd,
      'otherSuccess',
      comparisonRangeStart,
      comparisonRangeEnd,
    )

    expect(current).toEqual([100, 200])
    expect(comparison).toEqual([10, 20])
  })

  it('leaves missing backdrop buckets empty instead of zero-filling them', () => {
    const currentHourStart = Date.UTC(2026, 3, 7, 12, 0, 0) / 1000
    const window = buildDashboardHourlyRequestWindowFixture({
      currentHourStart,
      retainedBuckets: 2,
      mapBucket: ({ index }) => ({
        secondarySuccess: index === 0 ? 10 : 20,
      }),
    })

    const { current, comparison } = buildHourlyBackdropSeries(
      window,
      currentHourStart - 3600,
      currentHourStart + 2 * 3600,
      'otherSuccess',
      currentHourStart - 3600,
      currentHourStart + 2 * 3600,
    )

    expect(current).toEqual([10, 20, null])
    expect(comparison).toEqual([10, 20, null])
  })
})
