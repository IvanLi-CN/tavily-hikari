import { describe, expect, it } from 'bun:test'

import {
  buildTodayCountComparison,
  createDashboardMonthMetrics,
  createDashboardTodayMetrics,
  type DashboardMonthMetricLabels,
  type DashboardTodayMetricLabels,
  type DashboardTodayMetricStrings,
} from './dashboardTodayMetrics'

const todayLabels: DashboardTodayMetricLabels = {
  total: 'Total Requests',
  success: 'Success',
  failure: 'Failure',
  unknownCalls: 'Unknown Calls',
  upstreamExhausted: 'Upstream Keys Exhausted',
  valuableTag: 'Primary',
  otherTag: 'Secondary',
  unknownTag: 'Unknown',
}

const todayStrings: DashboardTodayMetricStrings = {
  deltaFromYesterday: 'vs same time yesterday',
  deltaNoBaseline: 'No yesterday baseline',
  percentagePointUnit: 'pp',
  asOfNow: 'Up to now',
  todayShare: 'Today share',
  todayAdded: 'Added today',
}

const monthLabels: DashboardMonthMetricLabels = {
  ...todayLabels,
  newKeys: 'New Keys',
  newQuarantines: 'New Quarantines',
}

const formatters = {
  formatNumber: (value: number) => value.toString(),
  formatPercent: (numerator: number, denominator: number) =>
    denominator === 0 ? '—' : `${((numerator / denominator) * 100).toFixed(1)}%`,
}

describe('dashboard request-value metric helpers', () => {
  it('keeps total requests on a full-width row and builds 7 today cards', () => {
    const metrics = createDashboardTodayMetrics({
      today: {
        total_requests: 100,
        success_count: 0,
        error_count: 0,
        quota_exhausted_count: 0,
        valuable_success_count: 50,
        valuable_failure_count: 20,
        other_success_count: 15,
        other_failure_count: 10,
        unknown_count: 5,
        upstream_exhausted_key_count: 2,
        new_keys: 0,
        new_quarantines: 0,
      },
      yesterday: {
        total_requests: 80,
        success_count: 0,
        error_count: 0,
        quota_exhausted_count: 0,
        valuable_success_count: 40,
        valuable_failure_count: 15,
        other_success_count: 14,
        other_failure_count: 8,
        unknown_count: 3,
        upstream_exhausted_key_count: 1,
        new_keys: 0,
        new_quarantines: 0,
      },
      labels: todayLabels,
      strings: todayStrings,
      formatters,
    })

    expect(metrics).toHaveLength(7)
    expect(metrics[0]).toMatchObject({
      id: 'today-total',
      label: 'Total Requests',
      fullWidth: true,
      comparison: {
        label: 'vs same time yesterday',
        value: '+20 (25%)',
        direction: 'up',
        tone: 'positive',
      },
    })
  })

  it('treats a lower failure count as a positive shift', () => {
    const comparison = buildTodayCountComparison({
      currentValue: 10,
      previousValue: 20,
      strings: todayStrings,
      trend: 'lower-is-better',
    })

    expect(comparison).toEqual({
      label: 'vs same time yesterday',
      value: '-10 (-50%)',
      direction: 'down',
      tone: 'positive',
    })
  })

  it('adds primary/secondary markers and does not split unknown calls', () => {
    const metrics = createDashboardTodayMetrics({
      today: {
        total_requests: 24,
        success_count: 0,
        error_count: 0,
        quota_exhausted_count: 0,
        valuable_success_count: 12,
        valuable_failure_count: 4,
        other_success_count: 5,
        other_failure_count: 1,
        unknown_count: 2,
        upstream_exhausted_key_count: 0,
        new_keys: 0,
        new_quarantines: 0,
      },
      yesterday: {
        total_requests: 0,
        success_count: 0,
        error_count: 0,
        quota_exhausted_count: 0,
        valuable_success_count: 0,
        valuable_failure_count: 0,
        other_success_count: 0,
        other_failure_count: 0,
        unknown_count: 0,
        upstream_exhausted_key_count: 0,
        new_keys: 0,
        new_quarantines: 0,
      },
      labels: todayLabels,
      strings: todayStrings,
      formatters,
    })

    expect(metrics.find((metric) => metric.id === 'today-valuable-success')).toMatchObject({
      id: 'today-valuable-success',
      marker: 'Primary',
      markerTone: 'primary',
      valueMeta: 'Today share · 50.0%',
      subtitle: undefined,
    })

    expect(metrics.find((metric) => metric.id === 'today-other-failure')).toMatchObject({
      id: 'today-other-failure',
      marker: 'Secondary',
      markerTone: 'secondary',
      valueMeta: 'Today share · 4.2%',
      subtitle: undefined,
    })

    expect(metrics.find((metric) => metric.id === 'today-unknown')).toMatchObject({
      id: 'today-unknown',
      label: 'Unknown Calls',
      value: '2',
      valueMeta: 'Today share · 8.3%',
      subtitle: undefined,
      comparison: {
        label: 'vs same time yesterday',
        value: '+2 · No yesterday baseline',
        direction: 'up',
        tone: 'negative',
      },
    })
  })

  it('builds 9 compact month cards with preserved lifecycle cards', () => {
    const metrics = createDashboardMonthMetrics({
      month: {
        total_requests: 800,
        success_count: 0,
        error_count: 0,
        quota_exhausted_count: 0,
        valuable_success_count: 420,
        valuable_failure_count: 120,
        other_success_count: 160,
        other_failure_count: 60,
        unknown_count: 40,
        upstream_exhausted_key_count: 6,
        new_keys: 4,
        new_quarantines: 2,
      },
      labels: monthLabels,
      strings: {
        monthToDate: 'Month to date',
        monthShare: 'Month share',
        monthAdded: 'Added this month',
      },
      formatters,
    })

    expect(metrics).toHaveLength(9)
    expect(metrics.map((metric) => metric.id)).toEqual([
      'month-total',
      'month-valuable-success',
      'month-valuable-failure',
      'month-other-success',
      'month-other-failure',
      'month-unknown',
      'month-upstream-exhausted',
      'month-new-keys',
      'month-new-quarantines',
    ])
    expect(metrics.find((metric) => metric.id === 'month-upstream-exhausted')).toMatchObject({
      id: 'month-upstream-exhausted',
      label: 'Upstream Keys Exhausted',
      value: '6',
      subtitle: 'Added this month',
      comparison: undefined,
    })
    expect(metrics.find((metric) => metric.id === 'month-valuable-success')).toMatchObject({
      marker: 'Primary',
      markerTone: 'primary',
      subtitle: 'Month share · 52.5%',
    })
    expect(metrics.find((metric) => metric.id === 'month-other-failure')).toMatchObject({
      marker: 'Secondary',
      markerTone: 'secondary',
      subtitle: 'Month share · 7.5%',
    })
  })
})
