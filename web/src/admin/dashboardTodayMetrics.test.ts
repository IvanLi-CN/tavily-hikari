import { describe, expect, it } from 'bun:test'

import {
  buildTodayRateComparison,
  createDashboardMonthMetrics,
  createDashboardTodayMetrics,
  type DashboardTodayMetricLabels,
  type DashboardTodayMetricStrings,
} from './dashboardTodayMetrics'

const labels: DashboardTodayMetricLabels = {
  total: 'Total Requests',
  success: 'Successful',
  errors: 'Errors',
  upstreamExhausted: 'Upstream Keys Exhausted',
}

const strings: DashboardTodayMetricStrings = {
  deltaFromYesterday: 'vs same time yesterday',
  deltaNoBaseline: 'No yesterday baseline',
  percentagePointUnit: 'pp',
  asOfNow: 'Up to now',
  todayShare: 'Today share',
  todayAdded: 'Added today',
}

const formatters = {
  formatNumber: (value: number) => value.toString(),
  formatPercent: (numerator: number, denominator: number) =>
    denominator === 0 ? '—' : `${((numerator / denominator) * 100).toFixed(1)}%`,
}

describe('dashboard today metrics helpers', () => {
  it('compares success by rate instead of raw count delta', () => {
    const metrics = createDashboardTodayMetrics({
      today: {
        total_requests: 100,
        success_count: 50,
        error_count: 50,
        quota_exhausted_count: 0,
        upstream_exhausted_key_count: 0,
        new_keys: 0,
        new_quarantines: 0,
      },
      yesterday: {
        total_requests: 80,
        success_count: 40,
        error_count: 40,
        quota_exhausted_count: 0,
        upstream_exhausted_key_count: 0,
        new_keys: 0,
        new_quarantines: 0,
      },
      labels,
      strings,
      formatters,
    })

    expect(metrics.find((metric) => metric.id === 'today-success')?.comparison).toEqual({
      label: 'vs same time yesterday',
      value: '0.0 pp',
      direction: 'flat',
      tone: 'neutral',
    })
  })

  it('treats a lower error rate as a positive shift', () => {
    const comparison = buildTodayRateComparison({
      currentNumerator: 10,
      currentDenominator: 100,
      previousNumerator: 20,
      previousDenominator: 100,
      strings,
      trend: 'lower-is-better',
    })

    expect(comparison).toEqual({
      label: 'vs same time yesterday',
      value: '-10.0 pp',
      direction: 'down',
      tone: 'positive',
    })
  })

  it('falls back to no baseline when yesterday has no traffic but today does', () => {
    const comparison = buildTodayRateComparison({
      currentNumerator: 8,
      currentDenominator: 20,
      previousNumerator: 0,
      previousDenominator: 0,
      strings,
    })

    expect(comparison).toEqual({
      label: 'vs same time yesterday',
      value: 'No yesterday baseline',
      direction: 'flat',
      tone: 'neutral',
    })
  })

  it('returns a flat 0.0 pp delta when both windows are empty', () => {
    const comparison = buildTodayRateComparison({
      currentNumerator: 0,
      currentDenominator: 0,
      previousNumerator: 0,
      previousDenominator: 0,
      strings,
    })

    expect(comparison).toEqual({
      label: 'vs same time yesterday',
      value: '0.0 pp',
      direction: 'flat',
      tone: 'neutral',
    })
  })

  it('uses upstream exhausted key counts for the today lifecycle card', () => {
    const metrics = createDashboardTodayMetrics({
      today: {
        total_requests: 120,
        success_count: 100,
        error_count: 20,
        quota_exhausted_count: 15,
        upstream_exhausted_key_count: 3,
        new_keys: 0,
        new_quarantines: 0,
      },
      yesterday: {
        total_requests: 90,
        success_count: 81,
        error_count: 9,
        quota_exhausted_count: 4,
        upstream_exhausted_key_count: 1,
        new_keys: 0,
        new_quarantines: 0,
      },
      labels,
      strings,
      formatters,
    })

    expect(metrics.find((metric) => metric.id === 'today-upstream-exhausted')).toEqual({
      id: 'today-upstream-exhausted',
      label: 'Upstream Keys Exhausted',
      value: '3',
      subtitle: 'Added today',
      comparison: {
        label: 'vs same time yesterday',
        value: '+2 (200%)',
        direction: 'up',
        tone: 'negative',
      },
    })
  })

  it('uses month-added subtitles for upstream exhausted key counts', () => {
    const metrics = createDashboardMonthMetrics({
      month: {
        total_requests: 800,
        success_count: 640,
        error_count: 80,
        quota_exhausted_count: 48,
        upstream_exhausted_key_count: 6,
        new_keys: 4,
        new_quarantines: 2,
      },
      labels: {
        total: 'Total Requests',
        success: 'Successful',
        errors: 'Errors',
        upstreamExhausted: 'Upstream Keys Exhausted',
        newKeys: 'New Keys',
        newQuarantines: 'New Quarantines',
      },
      strings: {
        monthToDate: 'Month to date',
        monthShare: 'Month share',
        monthAdded: 'Added this month',
      },
      formatters,
    })

    expect(metrics.find((metric) => metric.id === 'month-upstream-exhausted')).toEqual({
      id: 'month-upstream-exhausted',
      label: 'Upstream Keys Exhausted',
      value: '6',
      subtitle: 'Added this month',
    })
  })
})
