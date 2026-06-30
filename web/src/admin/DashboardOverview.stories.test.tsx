import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import meta, * as dashboardStories from './DashboardOverview.stories'

describe('DashboardOverview Storybook coverage', () => {
  it('keeps all six chart modes and the empty-selection story available', () => {
    expect(meta).toMatchObject({ title: 'Admin/Components/DashboardOverview' })
    expect(dashboardStories.Default).toMatchObject({})
    expect(dashboardStories.TypesMode).toMatchObject({})
    expect(dashboardStories.ResultsDeltaMode).toMatchObject({})
    expect(dashboardStories.TypesDeltaMode).toMatchObject({})
    expect(dashboardStories.ResultsAreaMode).toMatchObject({})
    expect(dashboardStories.TypesAreaMode).toMatchObject({})
    expect(dashboardStories.TypesAreaHiddenMiddleSeries).toMatchObject({})
    expect(dashboardStories.HiddenSeriesEmpty).toMatchObject({})
    expect(dashboardStories.FixedRangeWithGaps).toMatchObject({})
    expect(dashboardStories.RecentAlertsDesktopEvidence).toMatchObject({})
    expect(dashboardStories.NoPreviousMonthComparison).toMatchObject({})
  })

  it('renders the empty-selection story with the updated server-time copy', () => {
    const args = dashboardStories.HiddenSeriesEmpty.args
    expect(args).toBeDefined()
    const markup = renderToStaticMarkup(createElement(meta.component, args as never))
    const chartBucketSeconds = 3600
    const expectedCurrentBuckets = Math.ceil(
      ((args?.summaryWindows.today_period_end ?? args?.summaryWindows.today_end ?? 0) - (args?.summaryWindows.today_start ?? 0)) / chartBucketSeconds,
    )
    expect(markup).not.toContain('dashboard-hero-panel')
    expect(markup).not.toContain('Operations Dashboard')
    expect(markup).not.toContain('Global health, risk signals, and actionable activity in one place.')
    expect(markup).toContain('No visible chart series for the current selection.')
    expect(markup).toContain('Traffic Trends')
    expect(markup).toContain(
      `Local time axis · Rolling 24 hours (${expectedCurrentBuckets} current buckets`,
    )
    expect(markup).toContain('missing buckets are left blank')
    expect(markup).toContain('dashboard-summary-card-backdrop')
  })

  it('renders the grouped alert queue summary in the default story', () => {
    const args = dashboardStories.Default.args
    const markup = renderToStaticMarkup(createElement(meta.component, args as never))
    expect(markup).toContain('1 hour')
    expect(markup).toContain('24 hours')
    expect(markup).toContain('7 days')
    expect(markup).toContain('24-hour queue')
    expect(markup).toContain('Alert window')
    expect(markup).toContain('Review')
    expect(markup).toContain('Alice Wang')
    expect(markup).toContain('Tavily Search')
    expect(markup).toContain('Tavily usage limit 432')
    expect(markup).toContain('Queue below')
    expect(markup).toContain('Review group')
    expect(markup).toContain('aria-label="Review group: Alice Wang"')
    expect(markup).toContain('dashboard-alerts-summary__count-badge')
    expect(markup).toContain('status-pill-warning')
    expect(markup).toContain('>4<')
    expect(markup).toContain('dashboard-alerts-summary__field-label')
    expect(markup).not.toContain('deep-link into the grouped Alerts view')
  })

  it('exposes a fixed-range gap story for visual evidence', () => {
    const args = dashboardStories.FixedRangeWithGaps.args
    expect(args?.initialChartMode).toBe('resultsDelta')
    expect(args?.initialResultDeltaSeries).toBe('primarySuccess')
    expect(args?.hourlyRequestWindow.buckets.length).toBeLessThan(args?.hourlyRequestWindow.retainedBuckets ?? 0)
    const chartBucketSeconds = 3600
    const expectedCurrentBuckets = Math.ceil(
      ((args?.summaryWindows.today_end ?? 0) - (args?.summaryWindows.today_start ?? 0)) / chartBucketSeconds,
    )
    const expectedComparisonBuckets = Math.ceil(
      ((args?.summaryWindows.yesterday_end ?? 0) - (args?.summaryWindows.yesterday_start ?? 0)) / chartBucketSeconds,
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
