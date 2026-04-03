import type { SummaryWindowMetrics } from '../api'
import type { DashboardMetricCard } from './DashboardOverview'

type DashboardMetricComparison = NonNullable<DashboardMetricCard['comparison']>
type ComparisonTrend = 'higher-is-better' | 'lower-is-better'

interface DashboardMetricCategoryLabels {
  valuableTag: string
  otherTag: string
  unknownTag: string
}

export interface DashboardTodayMetricLabels extends DashboardMetricCategoryLabels {
  total: string
  success: string
  failure: string
  unknownCalls: string
  upstreamExhausted: string
}

export interface DashboardTodayMetricStrings {
  deltaFromYesterday: string
  deltaNoBaseline: string
  percentagePointUnit: string
  asOfNow: string
  todayShare: string
  todayAdded: string
}

export interface DashboardMonthMetricLabels extends DashboardMetricCategoryLabels {
  total: string
  success: string
  failure: string
  unknownCalls: string
  upstreamExhausted: string
  newKeys: string
  newQuarantines: string
}

export interface DashboardMonthMetricStrings {
  monthToDate: string
  monthShare: string
  monthAdded: string
}

interface DashboardMetricFormatters {
  formatNumber: (value: number) => string
  formatPercent: (numerator: number, denominator: number) => string
}

interface BuildDashboardTodayMetricsOptions {
  today: SummaryWindowMetrics
  yesterday: SummaryWindowMetrics
  labels: DashboardTodayMetricLabels
  strings: DashboardTodayMetricStrings
  formatters: DashboardMetricFormatters
}

interface BuildDashboardMonthMetricsOptions {
  month: SummaryWindowMetrics
  labels: DashboardMonthMetricLabels
  strings: DashboardMonthMetricStrings
  formatters: DashboardMetricFormatters
}

interface BuildCountComparisonOptions {
  currentValue: number
  previousValue: number
  strings: Pick<DashboardTodayMetricStrings, 'deltaFromYesterday' | 'deltaNoBaseline'>
  trend?: ComparisonTrend
}

interface BuildMetricCardOptions {
  id: string
  label: string
  value: string
  marker?: string
  markerTone?: DashboardMetricCard['markerTone']
  valueMeta?: string
  subtitle?: string
  comparison?: DashboardMetricComparison
  fullWidth?: boolean
}

const integerFormatter = new Intl.NumberFormat('en-US', {
  maximumFractionDigits: 0,
})

const percentageFormatter = new Intl.NumberFormat('en-US', {
  style: 'percent',
  minimumFractionDigits: 0,
  maximumFractionDigits: 1,
})

function formatSignedInteger(value: number): string {
  if (value > 0) return `+${integerFormatter.format(value)}`
  return integerFormatter.format(value)
}

function resolveComparisonTone(
  direction: DashboardMetricComparison['direction'],
  trend: ComparisonTrend,
): DashboardMetricComparison['tone'] {
  if (direction === 'flat') {
    return 'neutral'
  }

  if (trend === 'higher-is-better') {
    return direction === 'up' ? 'positive' : 'negative'
  }

  return direction === 'down' ? 'positive' : 'negative'
}

function buildWindowSubtitle(
  label: string,
  value: number,
  total: number,
  formatPercent: (numerator: number, denominator: number) => string,
): string {
  return total > 0 ? `${label} · ${formatPercent(value, total)}` : label
}

function buildMetricCard({
  id,
  label,
  value,
  marker,
  markerTone,
  valueMeta,
  subtitle,
  comparison,
  fullWidth = false,
}: BuildMetricCardOptions): DashboardMetricCard {
  return {
    id,
    label,
    value,
    marker,
    markerTone,
    valueMeta,
    subtitle,
    comparison,
    fullWidth,
  }
}

export function buildTodayCountComparison({
  currentValue,
  previousValue,
  strings,
  trend = 'higher-is-better',
}: BuildCountComparisonOptions): DashboardMetricComparison {
  const deltaValue = currentValue - previousValue
  const direction: DashboardMetricComparison['direction'] =
    deltaValue > 0 ? 'up' : deltaValue < 0 ? 'down' : 'flat'

  let value = formatSignedInteger(deltaValue)
  if (previousValue > 0) {
    value = `${value} (${percentageFormatter.format(deltaValue / previousValue)})`
  } else if (deltaValue !== 0) {
    value = `${value} · ${strings.deltaNoBaseline}`
  }

  return {
    label: strings.deltaFromYesterday,
    value,
    direction,
    tone: resolveComparisonTone(direction, trend),
  }
}

export function createDashboardTodayMetrics({
  today,
  yesterday,
  labels,
  strings,
  formatters,
}: BuildDashboardTodayMetricsOptions): DashboardMetricCard[] {
  const { formatNumber, formatPercent } = formatters

  return [
    buildMetricCard({
      id: 'today-total',
      label: labels.total,
      value: formatNumber(today.total_requests),
      subtitle: strings.asOfNow,
      comparison: buildTodayCountComparison({
        currentValue: today.total_requests,
        previousValue: yesterday.total_requests,
        strings,
      }),
      fullWidth: true,
    }),
    buildMetricCard({
      id: 'today-valuable-success',
      label: labels.success,
      marker: labels.valuableTag,
      markerTone: 'primary',
      value: formatNumber(today.valuable_success_count),
      valueMeta: buildWindowSubtitle(
        strings.todayShare,
        today.valuable_success_count,
        today.total_requests,
        formatPercent,
      ),
      comparison: buildTodayCountComparison({
        currentValue: today.valuable_success_count,
        previousValue: yesterday.valuable_success_count,
        strings,
      }),
    }),
    buildMetricCard({
      id: 'today-valuable-failure',
      label: labels.failure,
      marker: labels.valuableTag,
      markerTone: 'primary',
      value: formatNumber(today.valuable_failure_count),
      valueMeta: buildWindowSubtitle(
        strings.todayShare,
        today.valuable_failure_count,
        today.total_requests,
        formatPercent,
      ),
      comparison: buildTodayCountComparison({
        currentValue: today.valuable_failure_count,
        previousValue: yesterday.valuable_failure_count,
        strings,
        trend: 'lower-is-better',
      }),
    }),
    buildMetricCard({
      id: 'today-other-success',
      label: labels.success,
      marker: labels.otherTag,
      markerTone: 'secondary',
      value: formatNumber(today.other_success_count),
      valueMeta: buildWindowSubtitle(
        strings.todayShare,
        today.other_success_count,
        today.total_requests,
        formatPercent,
      ),
      comparison: buildTodayCountComparison({
        currentValue: today.other_success_count,
        previousValue: yesterday.other_success_count,
        strings,
      }),
    }),
    buildMetricCard({
      id: 'today-other-failure',
      label: labels.failure,
      marker: labels.otherTag,
      markerTone: 'secondary',
      value: formatNumber(today.other_failure_count),
      valueMeta: buildWindowSubtitle(
        strings.todayShare,
        today.other_failure_count,
        today.total_requests,
        formatPercent,
      ),
      comparison: buildTodayCountComparison({
        currentValue: today.other_failure_count,
        previousValue: yesterday.other_failure_count,
        strings,
        trend: 'lower-is-better',
      }),
    }),
    buildMetricCard({
      id: 'today-unknown',
      label: labels.unknownCalls,
      value: formatNumber(today.unknown_count),
      valueMeta: buildWindowSubtitle(
        strings.todayShare,
        today.unknown_count,
        today.total_requests,
        formatPercent,
      ),
      comparison: buildTodayCountComparison({
        currentValue: today.unknown_count,
        previousValue: yesterday.unknown_count,
        strings,
        trend: 'lower-is-better',
      }),
    }),
    buildMetricCard({
      id: 'today-upstream-exhausted',
      label: labels.upstreamExhausted,
      value: formatNumber(today.upstream_exhausted_key_count),
      subtitle: strings.todayAdded,
      comparison: buildTodayCountComparison({
        currentValue: today.upstream_exhausted_key_count,
        previousValue: yesterday.upstream_exhausted_key_count,
        strings,
        trend: 'lower-is-better',
      }),
    }),
  ]
}

export function createDashboardMonthMetrics({
  month,
  labels,
  strings,
  formatters,
}: BuildDashboardMonthMetricsOptions): DashboardMetricCard[] {
  const { formatNumber, formatPercent } = formatters

  return [
    buildMetricCard({
      id: 'month-total',
      label: labels.total,
      value: formatNumber(month.total_requests),
      subtitle: strings.monthToDate,
      fullWidth: true,
    }),
    buildMetricCard({
      id: 'month-valuable-success',
      label: labels.success,
      marker: labels.valuableTag,
      markerTone: 'primary',
      value: formatNumber(month.valuable_success_count),
      subtitle: buildWindowSubtitle(
        strings.monthShare,
        month.valuable_success_count,
        month.total_requests,
        formatPercent,
      ),
    }),
    buildMetricCard({
      id: 'month-valuable-failure',
      label: labels.failure,
      marker: labels.valuableTag,
      markerTone: 'primary',
      value: formatNumber(month.valuable_failure_count),
      subtitle: buildWindowSubtitle(
        strings.monthShare,
        month.valuable_failure_count,
        month.total_requests,
        formatPercent,
      ),
    }),
    buildMetricCard({
      id: 'month-other-success',
      label: labels.success,
      marker: labels.otherTag,
      markerTone: 'secondary',
      value: formatNumber(month.other_success_count),
      subtitle: buildWindowSubtitle(
        strings.monthShare,
        month.other_success_count,
        month.total_requests,
        formatPercent,
      ),
    }),
    buildMetricCard({
      id: 'month-other-failure',
      label: labels.failure,
      marker: labels.otherTag,
      markerTone: 'secondary',
      value: formatNumber(month.other_failure_count),
      subtitle: buildWindowSubtitle(
        strings.monthShare,
        month.other_failure_count,
        month.total_requests,
        formatPercent,
      ),
    }),
    buildMetricCard({
      id: 'month-unknown',
      label: labels.unknownCalls,
      value: formatNumber(month.unknown_count),
      subtitle: buildWindowSubtitle(
        strings.monthShare,
        month.unknown_count,
        month.total_requests,
        formatPercent,
      ),
    }),
    buildMetricCard({
      id: 'month-upstream-exhausted',
      label: labels.upstreamExhausted,
      value: formatNumber(month.upstream_exhausted_key_count),
      subtitle: strings.monthAdded,
    }),
    buildMetricCard({
      id: 'month-new-keys',
      label: labels.newKeys,
      value: formatNumber(month.new_keys),
      subtitle: strings.monthAdded,
    }),
    buildMetricCard({
      id: 'month-new-quarantines',
      label: labels.newQuarantines,
      value: formatNumber(month.new_quarantines),
      subtitle: strings.monthAdded,
    }),
  ]
}
