import { describe, expect, it } from 'bun:test'

import {
  buildTodayRateComparison,
  createDashboardTodayMetrics,
  type DashboardTodayMetricLabels,
  type DashboardTodayMetricStrings,
} from './dashboardTodayMetrics'

const labels: DashboardTodayMetricLabels = {
  total: 'Total Requests',
  success: 'Successful',
  errors: 'Errors',
  quota: 'Quota Exhausted',
}

const strings: DashboardTodayMetricStrings = {
  deltaFromYesterday: 'vs same time yesterday',
  deltaNoBaseline: 'No yesterday baseline',
  percentagePointUnit: 'pp',
  asOfNow: 'Up to now',
  todayShare: 'Today share',
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
        new_keys: 0,
        new_quarantines: 0,
      },
      yesterday: {
        total_requests: 80,
        success_count: 40,
        error_count: 40,
        quota_exhausted_count: 0,
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
})
