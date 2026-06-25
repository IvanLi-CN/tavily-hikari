import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import meta, * as dashboardStories from './DashboardOverview.stories'

describe('DashboardOverview Storybook coverage', () => {
  it('keeps all six chart modes and the empty-selection story available', () => {
    expect(meta).toMatchObject({ title: 'Admin/Components/DashboardOverview' })
    expect(dashboardStories.Default).toMatchObject({})
    expect(dashboardStories.FreshnessStaleLastGood).toMatchObject({})
    expect(dashboardStories.FreshnessColdStartDegraded).toMatchObject({})
    expect(dashboardStories.FreshnessGallery).toMatchObject({})
    expect(dashboardStories.TypesMode).toMatchObject({})
    expect(dashboardStories.ResultsDeltaMode).toMatchObject({})
    expect(dashboardStories.TypesDeltaMode).toMatchObject({})
    expect(dashboardStories.ResultsAreaMode).toMatchObject({})
    expect(dashboardStories.TypesAreaMode).toMatchObject({})
    expect(dashboardStories.TypesAreaHiddenMiddleSeries).toMatchObject({})
    expect(dashboardStories.HiddenSeriesEmpty).toMatchObject({})
    expect(dashboardStories.FixedRangeWithGaps).toMatchObject({})
    expect(dashboardStories.NoPreviousMonthComparison).toMatchObject({})
  })

  it('renders the empty-selection story with the updated server-time copy', () => {
    const args = dashboardStories.HiddenSeriesEmpty.args
    expect(args).toBeDefined()
    const markup = renderToStaticMarkup(createElement(meta.component, args as never))
    expect(markup).not.toContain('dashboard-hero-panel')
    expect(markup).not.toContain('Operations Dashboard')
    expect(markup).not.toContain('Global health, risk signals, and actionable activity in one place.')
    expect(markup).toContain('No visible chart series for the current selection.')
    expect(markup).toContain('Traffic Trends')
    expect(markup).toContain('Local time axis · Rolling 25 buckets')
    expect(markup).toContain('missing buckets are left blank')
    expect(markup).toContain('dashboard-summary-card-backdrop')
  })

  it('exposes a fixed-range gap story for visual evidence', () => {
    const args = dashboardStories.FixedRangeWithGaps.args
    expect(args?.initialChartMode).toBe('resultsDelta')
    expect(args?.initialResultDeltaSeries).toBe('primarySuccess')
    expect(args?.hourlyRequestWindow.buckets.length).toBeLessThan(args?.hourlyRequestWindow.retainedBuckets ?? 0)
    const expectedCurrentBuckets = Math.ceil(
      ((args?.summaryWindows.today_end ?? 0) - (args?.summaryWindows.today_start ?? 0)) / 3600,
    )
    const expectedComparisonBuckets = Math.ceil(
      ((args?.summaryWindows.yesterday_end ?? 0) - (args?.summaryWindows.yesterday_start ?? 0)) / 3600,
    )
    const markup = renderToStaticMarkup(createElement(meta.component, args as never))
    expect(markup).toContain(
      `Local time axis · Natural-day comparison (${expectedCurrentBuckets} current buckets, ${expectedComparisonBuckets} comparison buckets)`,
    )
  })

  it('keeps the month summary on a full natural-month axis with a visible previous-month comparison line', () => {
    const args = dashboardStories.Default.args
    expect(args?.monthSeries?.current).toHaveLength(31)
    expect(args?.monthSeries?.comparison).toHaveLength(31)
    expect(args?.monthSeries?.current.slice(7).every((point) => point.total == null)).toBe(true)
    expect(args?.monthSeries?.comparison.some((point) => typeof point.total === 'number')).toBe(true)
  })

  it('exposes an explicit empty-state story when previous-month comparison data is absent', () => {
    const args = dashboardStories.NoPreviousMonthComparison.args
    expect(args?.monthSeries?.comparison).toEqual([])
    const markup = renderToStaticMarkup(createElement(meta.component, args as never))
    expect(markup).toContain('No retained previous-month comparison data.')
  })

  it('exposes stale and degraded freshness proof stories', () => {
    expect(dashboardStories.FreshnessStaleLastGood.args?.freshness).toMatchObject({
      state: 'stale',
      source: 'last_good',
      reason: 'sqlite_contention',
    })
    expect(dashboardStories.FreshnessColdStartDegraded.args?.freshness).toMatchObject({
      state: 'degraded',
      source: 'cold_start_fallback',
      reason: 'cold_start_no_cache',
    })
  })

  it('keeps the absolute charts on all-series defaults in the primary stories', () => {
    expect(dashboardStories.Default.args?.initialVisibleResultSeries).toBeUndefined()
    expect(dashboardStories.Default.args?.initialVisibleTypeSeries).toBeUndefined()
    expect(dashboardStories.TypesDeltaMode.args?.initialTypeDeltaSeries).toBe('all')
  })

  it('exposes dedicated area-chart stories for both series families', () => {
    expect(dashboardStories.ResultsAreaMode.args?.initialChartMode).toBe('resultsArea')
    expect(dashboardStories.TypesAreaMode.args?.initialChartMode).toBe('typesArea')
  })

  it('exposes a stacked-area hidden-middle-series regression story', () => {
    expect(dashboardStories.TypesAreaHiddenMiddleSeries.args?.initialChartMode).toBe('typesArea')
    expect(dashboardStories.TypesAreaHiddenMiddleSeries.args?.initialVisibleTypeSeries).toEqual([
      'mcpNonBillable',
      'apiNonBillable',
      'apiBillable',
    ])
  })
})
