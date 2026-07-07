import { describe, expect, it } from 'bun:test'

import {
  buildDashboardHourlyRequestWindowFixture,
  buildAggregatedHourlySlots,
  buildDashboardAreaStackLayers,
  buildDeltaSeriesValues,
  buildDeltaSeriesSlotValues,
  buildHourlyBucketLookup,
  buildHourlyRangeSlots,
  createDashboardHourlyChartPreferences,
  createEmptyDashboardHourlyRequestWindow,
  DASHBOARD_REALTIME_BUCKET_SECONDS,
  DASHBOARD_REALTIME_RETAINED_BUCKETS,
  DASHBOARD_REALTIME_VISIBLE_BUCKETS,
  DASHBOARD_AREA_CHART_STACK_ID,
  DASHBOARD_AREA_CHART_TENSION,
  DASHBOARD_RESULT_SERIES_ORDER,
  DASHBOARD_TYPE_SERIES_ORDER,
  getCurrentDayHourlyBuckets,
  formatHourlyBucketLabel,
  formatDashboardRealtimeWindowLabel,
  getHourlyBucketsInRange,
  buildRollingHourlyWindow,
  getDashboardHourlyBarChartKey,
  getCurrentPartialHourHighlightIndex,
  getVisibleHourlyBuckets,
  getVisibleHourlyWindow,
  readDashboardHourlyChartPreferences,
  toggleSeriesSelection,
  writeDashboardHourlyChartPreferences,
} from './dashboardHourlyCharts'

describe('dashboardHourlyCharts helpers', () => {
  it('returns the latest visible bucket slice and keeps retained metadata intact', () => {
    const window = buildDashboardHourlyRequestWindowFixture()

    expect(window.retainedBuckets).toBe(DASHBOARD_REALTIME_RETAINED_BUCKETS)
    expect(window.visibleBuckets).toBe(DASHBOARD_REALTIME_VISIBLE_BUCKETS)
    expect(getVisibleHourlyBuckets(window)).toHaveLength(DASHBOARD_REALTIME_VISIBLE_BUCKETS)
    expect(getVisibleHourlyBuckets(window)[0]?.bucketStart).toBe(window.buckets[516]?.bucketStart)
    expect(getVisibleHourlyBuckets(window).at(-1)?.bucketStart).toBe(window.buckets.at(-1)?.bucketStart)
    expect(window.buckets[0]?.bucketStart).toBe(
      window.buckets.at(-1)!.bucketStart - (DASHBOARD_REALTIME_RETAINED_BUCKETS - 1) * DASHBOARD_REALTIME_BUCKET_SECONDS,
    )
  })

  it('anchors the latest bucket to the current five-minute bucket instead of the previous closed bucket', () => {
    const currentHourStart = Date.UTC(2026, 3, 7, 12, 0, 0) / 1000
    const window = buildDashboardHourlyRequestWindowFixture({ currentHourStart })

    expect(window.buckets.at(-1)?.bucketStart).toBe(currentHourStart)
    expect(getVisibleHourlyBuckets(window).at(-1)?.bucketStart).toBe(currentHourStart)
  })

  it('builds a rolling 24-hour hourly window plus the current partial hour', () => {
    const currentHourStart = Date.UTC(2026, 3, 7, 12, 0, 0) / 1000
    const window = buildDashboardHourlyRequestWindowFixture({
      currentHourStart,
      bucketSeconds: 3600,
      visibleBuckets: 25,
      retainedBuckets: 49,
    })

    const rolling = buildRollingHourlyWindow(window)

    expect(rolling.slots).toHaveLength(25)
    expect(rolling.slots[0]?.bucketStart).toBe(currentHourStart - 24 * 3600)
    expect(rolling.slots.at(-1)?.bucketStart).toBe(currentHourStart)
  })

  it('computes yesterday deltas from aligned hourly buckets', () => {
    const window = buildDashboardHourlyRequestWindowFixture({
      bucketSeconds: 3600,
      visibleBuckets: 25,
      retainedBuckets: 49,
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

  it('filters current-day buckets using the requested timezone', () => {
    const currentHourStart = Date.UTC(2026, 3, 7, 4, 0, 0) / 1000
    const window = buildDashboardHourlyRequestWindowFixture({
      currentHourStart,
      bucketSeconds: 3600,
      visibleBuckets: 25,
      retainedBuckets: 49,
    })

    const utcBuckets = getCurrentDayHourlyBuckets(window, 'UTC')
    const shanghaiBuckets = getCurrentDayHourlyBuckets(window, 'Asia/Shanghai')

    expect(utcBuckets).toHaveLength(5)
    expect(utcBuckets[0]?.bucketStart).toBe(Date.UTC(2026, 3, 7, 0, 0, 0) / 1000)
    expect(utcBuckets.at(-1)?.bucketStart).toBe(currentHourStart)
    expect(shanghaiBuckets).toHaveLength(13)
    expect(shanghaiBuckets[0]?.bucketStart).toBe(Date.UTC(2026, 3, 6, 16, 0, 0) / 1000)
    expect(shanghaiBuckets.at(-1)?.bucketStart).toBe(currentHourStart)
  })

  it('filters buckets using explicit server epoch boundaries', () => {
    const currentHourStart = Date.UTC(2026, 3, 7, 4, 0, 0) / 1000
    const window = buildDashboardHourlyRequestWindowFixture({
      currentHourStart,
      bucketSeconds: 3600,
      visibleBuckets: 25,
      retainedBuckets: 49,
    })
    const rangeStart = Date.UTC(2026, 3, 6, 16, 0, 0) / 1000

    const buckets = getHourlyBucketsInRange(window, rangeStart, currentHourStart + 1)

    expect(buckets).toHaveLength(13)
    expect(buckets[0]?.bucketStart).toBe(rangeStart)
    expect(buckets.at(-1)?.bucketStart).toBe(currentHourStart)
    expect(getHourlyBucketsInRange(window, rangeStart, rangeStart)).toEqual([])
  })

  it('builds fixed hourly slots and leaves missing buckets empty', () => {
    const currentHourStart = Date.UTC(2026, 3, 7, 4, 0, 0) / 1000
    const window = buildDashboardHourlyRequestWindowFixture({
      currentHourStart,
      bucketSeconds: 3600,
      visibleBuckets: 4,
      retainedBuckets: 4,
    })
    const rangeStart = currentHourStart - 3 * 3600
    const rangeEnd = currentHourStart + 2 * 3600

    const slots = buildHourlyRangeSlots(window, rangeStart, rangeEnd)

    expect(slots.map((slot) => slot.bucketStart)).toEqual([
      currentHourStart - 3 * 3600,
      currentHourStart - 2 * 3600,
      currentHourStart - 1 * 3600,
      currentHourStart,
      currentHourStart + 3600,
    ])
    expect(slots.slice(0, 4).every((slot) => slot.bucket != null)).toBe(true)
    expect(slots[4]?.bucket).toBeNull()
  })

  it('builds fixed slots using the server bucket alignment offset', () => {
    const kathmanduOffsetSeconds = 45 * 60
    const currentHourStart = Date.UTC(2026, 3, 7, 12, 0, 0) / 1000 + kathmanduOffsetSeconds
    const window = buildDashboardHourlyRequestWindowFixture({
      currentHourStart,
      bucketSeconds: 3600,
      visibleBuckets: 4,
      retainedBuckets: 4,
    })
    const slots = buildHourlyRangeSlots(window, currentHourStart - 2 * 3600 - 60, currentHourStart + 3600)

    expect(slots.map((slot) => slot.bucketStart)).toEqual([
      currentHourStart - 2 * 3600,
      currentHourStart - 3600,
      currentHourStart,
    ])
    expect(slots.every((slot) => slot.bucket?.bucketStart === slot.bucketStart)).toBe(true)
  })

  it('returns null deltas when either fixed-range side is missing', () => {
    const currentHourStart = Date.UTC(2026, 3, 7, 4, 0, 0) / 1000
    const window = buildDashboardHourlyRequestWindowFixture({
      currentHourStart,
      bucketSeconds: 3600,
      visibleBuckets: 6,
      retainedBuckets: 6,
      mapBucket: ({ index }) => ({
        primarySuccess: index === 0 ? 10 : index === 4 ? 50 : 0,
      }),
    })
    window.buckets = window.buckets.filter((bucket) => bucket.bucketStart !== currentHourStart)
    const currentSlots = buildHourlyRangeSlots(window, currentHourStart - 3600, currentHourStart + 2 * 3600)
    const comparisonSlots = buildHourlyRangeSlots(window, currentHourStart - 5 * 3600, currentHourStart - 2 * 3600)

    expect(buildDeltaSeriesSlotValues(currentSlots, comparisonSlots, 'primarySuccess')).toEqual([
      40,
      null,
      null,
    ])
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
      bucketSeconds: DASHBOARD_REALTIME_BUCKET_SECONDS,
      visibleBuckets: DASHBOARD_REALTIME_VISIBLE_BUCKETS,
      retainedBuckets: DASHBOARD_REALTIME_RETAINED_BUCKETS,
      buckets: [],
    })
  })

  it('formats the realtime window label from bucket metadata', () => {
    expect(
      formatDashboardRealtimeWindowLabel(
        'Local time axis · Last {range} · {bucket} buckets ({count} current buckets)',
        300,
        73,
        73,
      ),
    ).toBe('Local time axis · Last 6h · 5m buckets (73 current buckets)')
  })

  it('defaults both absolute charts to all visible series', () => {
    const preferences = createDashboardHourlyChartPreferences()

    expect(preferences.visibleResultSeries).toEqual([...DASHBOARD_RESULT_SERIES_ORDER])
    expect(preferences.visibleTypeSeries).toEqual([...DASHBOARD_TYPE_SERIES_ORDER])
  })

  it('supports the expanded chart mode set including area charts', () => {
    expect(createDashboardHourlyChartPreferences({ chartMode: 'resultsArea' }).chartMode).toBe('resultsArea')
    expect(createDashboardHourlyChartPreferences({ chartMode: 'typesArea' }).chartMode).toBe('typesArea')
  })

  it('highlights only the current partial hour in absolute bar modes', () => {
    const slots = [
      { bucketStart: 100, bucket: null },
      { bucketStart: 200, bucket: null },
      { bucketStart: 300, bucket: null },
    ]

    expect(getCurrentPartialHourHighlightIndex('results', slots)).toBe(2)
    expect(getCurrentPartialHourHighlightIndex('types', slots)).toBe(2)
    expect(getCurrentPartialHourHighlightIndex('resultsDelta', slots)).toBeNull()
    expect(getCurrentPartialHourHighlightIndex('typesDelta', slots)).toBeNull()
    expect(getCurrentPartialHourHighlightIndex('resultsArea', slots)).toBeNull()
    expect(getCurrentPartialHourHighlightIndex('typesArea', slots)).toBeNull()
    expect(getCurrentPartialHourHighlightIndex('results', [])).toBeNull()
  })

  it('changes the bar chart instance key when current partial-hour marking changes', () => {
    const slots = [
      { bucketStart: 100, bucket: null },
      { bucketStart: 200, bucket: null },
      { bucketStart: 300, bucket: null },
    ]

    expect(getDashboardHourlyBarChartKey('results', slots)).toBe('results:current-partial-hour-2:3')
    expect(getDashboardHourlyBarChartKey('types', slots)).toBe('types:current-partial-hour-2:3')
    expect(getDashboardHourlyBarChartKey('resultsDelta', slots)).toBe('resultsDelta:no-current-partial-hour:3')
    expect(getDashboardHourlyBarChartKey('typesArea', slots)).toBe('typesArea:no-current-partial-hour:3')
    expect(getDashboardHourlyBarChartKey('results', [])).toBe('results:no-current-partial-hour:0')
  })

  it('builds non-overlapping stacked area fill targets for all visible result series', () => {
    const layers = buildDashboardAreaStackLayers(DASHBOARD_RESULT_SERIES_ORDER)

    expect(layers.map((layer) => layer.seriesId)).toEqual([...DASHBOARD_RESULT_SERIES_ORDER])
    expect(layers.every((layer) => layer.type === 'line')).toBe(true)
    expect(layers.map((layer) => layer.fill)).toEqual(['origin', '-1', '-1', '-1', '-1', '-1'])
    expect(layers.every((layer) => layer.stack === DASHBOARD_AREA_CHART_STACK_ID)).toBe(true)
    expect(layers.every((layer) => layer.tension === DASHBOARD_AREA_CHART_TENSION)).toBe(true)
    expect(layers.every((layer) => layer.borderWidth === 2)).toBe(true)
    expect(layers.every((layer) => layer.pointRadius === 0)).toBe(true)
    expect(layers.every((layer) => layer.pointHoverRadius === 3)).toBe(true)
    expect(layers.every((layer) => layer.spanGaps === false)).toBe(true)
  })

  it('rebuilds stacked area fill targets from the currently visible type series only', () => {
    const visibleWithoutMiddle = DASHBOARD_TYPE_SERIES_ORDER.filter((seriesId) => seriesId !== 'mcpBillable')

    const layers = buildDashboardAreaStackLayers(visibleWithoutMiddle)

    expect(layers.map((layer) => layer.seriesId)).toEqual([
      'mcpNonBillable',
      'apiNonBillable',
      'apiBillable',
    ])
    expect(layers.map((layer) => layer.fill)).toEqual(['origin', '-1', '-1'])
    expect(layers.map((layer) => layer.stack)).toEqual(['area', 'area', 'area'])
    expect(layers.map((layer) => layer.tension)).toEqual([0.18, 0.18, 0.18])
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

  it('falls back to a legacy persistence key when the new key is empty', () => {
    const storage = new Map<string, string>()
    const storageApi = {
      getItem(key: string) {
        return storage.get(key) ?? null
      },
      setItem(key: string, value: string) {
        storage.set(key, value)
      },
    }

    storage.set('admin.dashboard.hourly-request-charts.v1', JSON.stringify({
      chartMode: 'resultsArea',
      visibleResultSeries: ['primarySuccess'],
      visibleTypeSeries: ['apiBillable'],
      resultDeltaSeries: 'primaryFailure429',
      typeDeltaSeries: 'all',
    }))

    expect(
      readDashboardHourlyChartPreferences(
        storageApi,
        'admin.dashboard.hourly-request-charts.v2',
        ['admin.dashboard.hourly-request-charts.v1'],
      ),
    ).toEqual({
      chartMode: 'resultsArea',
      visibleResultSeries: ['primarySuccess'],
      visibleTypeSeries: ['apiBillable'],
      resultDeltaSeries: 'primaryFailure429',
      typeDeltaSeries: 'all',
    })
  })

  it('builds the rolling visible window directly from visibleBuckets metadata', () => {
    const currentHourStart = Date.UTC(2026, 3, 7, 12, 0, 0) / 1000
    const window = buildDashboardHourlyRequestWindowFixture({ currentHourStart })

    const visible = getVisibleHourlyWindow(window)

    expect(visible.rangeStart).toBe(currentHourStart - 6 * 3600)
    expect(visible.rangeEnd).toBe(currentHourStart + DASHBOARD_REALTIME_BUCKET_SECONDS)
    expect(visible.slots).toHaveLength(DASHBOARD_REALTIME_VISIBLE_BUCKETS)
    expect(visible.slots[0]?.bucketStart).toBe(currentHourStart - 6 * 3600)
    expect(visible.slots.at(-1)?.bucketStart).toBe(currentHourStart)
  })

  it('keeps the rolling window fixed to the latest visible slot count even when buckets are sparse', () => {
    const currentHourStart = Date.UTC(2026, 3, 7, 12, 0, 0) / 1000
    const window = buildDashboardHourlyRequestWindowFixture({ currentHourStart })
    const missingBucketStart = currentHourStart - 3 * 3600
    window.buckets = window.buckets.filter((bucket) => bucket.bucketStart !== missingBucketStart)

    const visible = getVisibleHourlyWindow(window)

    expect(visible.rangeStart).toBe(currentHourStart - 6 * 3600)
    expect(visible.rangeEnd).toBe(currentHourStart + DASHBOARD_REALTIME_BUCKET_SECONDS)
    expect(visible.slots).toHaveLength(DASHBOARD_REALTIME_VISIBLE_BUCKETS)
    expect(visible.slots[36]?.bucketStart).toBe(missingBucketStart)
    expect(visible.slots[36]?.bucket).toBeNull()
  })

  it('aggregates five-minute buckets into hourly slots for fixed and delta charts', () => {
    const currentHourStart = Date.UTC(2026, 3, 7, 12, 0, 0) / 1000
    const window = buildDashboardHourlyRequestWindowFixture({
      currentHourStart,
      bucketSeconds: 300,
      visibleBuckets: 73,
      retainedBuckets: 73,
      mapBucket: () => ({
        primarySuccess: 1,
        apiBillable: 2,
      }),
    })

    const aggregated = buildAggregatedHourlySlots(window, currentHourStart - 2 * 3600, currentHourStart + 300)

    expect(aggregated.bucketSeconds).toBe(3600)
    expect(aggregated.slots.map((slot) => slot.bucketStart)).toEqual([
      currentHourStart - 2 * 3600,
      currentHourStart - 3600,
      currentHourStart,
    ])
    expect(aggregated.slots[0]?.bucket?.primarySuccess).toBe(12)
    expect(aggregated.slots[0]?.bucket?.apiBillable).toBe(24)
    expect(aggregated.slots[2]?.bucket?.primarySuccess).toBe(1)
  })

  it('aggregates fixed slots from the requested range start alignment', () => {
    const kathmanduOffsetSeconds = 5.75 * 3600
    const currentBucketStart = Date.UTC(2026, 3, 7, 12, 0, 0) / 1000 + kathmanduOffsetSeconds
    const rangeStart = currentBucketStart - 2 * 3600
    const window = buildDashboardHourlyRequestWindowFixture({
      currentHourStart: currentBucketStart,
      bucketSeconds: 300,
      visibleBuckets: 25,
      retainedBuckets: 25,
      mapBucket: () => ({
        primarySuccess: 1,
      }),
    })

    const aggregated = buildAggregatedHourlySlots(window, rangeStart, currentBucketStart + 300)

    expect(aggregated.slots.map((slot) => slot.bucketStart)).toEqual([
      rangeStart,
      rangeStart + 3600,
      rangeStart + 2 * 3600,
    ])
    expect(aggregated.slots[0]?.bucket?.primarySuccess).toBe(12)
    expect(aggregated.slots[1]?.bucket?.primarySuccess).toBe(12)
    expect(aggregated.slots[2]?.bucket?.primarySuccess).toBe(1)
  })
})
