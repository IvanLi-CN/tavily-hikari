import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import meta, * as dashboardStories from './DashboardOverview.stories'

describe('DashboardOverview Storybook coverage', () => {
  it('keeps all four chart modes and the empty-selection story available', () => {
    expect(meta).toMatchObject({ title: 'Admin/Components/DashboardOverview' })
    expect(dashboardStories.Default).toMatchObject({})
    expect(dashboardStories.TypesMode).toMatchObject({})
    expect(dashboardStories.ResultsDeltaMode).toMatchObject({})
    expect(dashboardStories.TypesDeltaMode).toMatchObject({})
    expect(dashboardStories.HiddenSeriesEmpty).toMatchObject({})
    expect(dashboardStories.FixedRangeWithGaps).toMatchObject({})
  })

  it('renders the empty-selection story with the updated server-time copy', () => {
    const args = dashboardStories.HiddenSeriesEmpty.args
    expect(args).toBeDefined()
    const markup = renderToStaticMarkup(createElement(meta.component, args as never))
    expect(markup).toContain('No visible chart series for the current selection.')
    expect(markup).toContain('Traffic Trends')
    expect(markup).toContain('Local time axis · Fixed range')
    expect(markup).toContain('missing buckets are left blank')
    expect(markup).toContain('dashboard-summary-card-backdrop')
  })

  it('exposes a fixed-range gap story for visual evidence', () => {
    const args = dashboardStories.FixedRangeWithGaps.args
    expect(args?.initialChartMode).toBe('resultsDelta')
    expect(args?.initialResultDeltaSeries).toBe('primarySuccess')
    expect(args?.hourlyRequestWindow.buckets.length).toBeLessThan(args?.hourlyRequestWindow.retainedBuckets ?? 0)
  })

  it('keeps the absolute charts on all-series defaults in the primary stories', () => {
    expect(dashboardStories.Default.args?.initialVisibleResultSeries).toBeUndefined()
    expect(dashboardStories.Default.args?.initialVisibleTypeSeries).toBeUndefined()
    expect(dashboardStories.TypesDeltaMode.args?.initialTypeDeltaSeries).toBe('all')
  })
})
