import type { SummaryWindowMetrics } from '../api'
import type { DashboardMetricCard } from './DashboardOverview'

type DashboardMetricComparison = NonNullable<DashboardMetricCard['comparison']>
type ComparisonTrend = 'higher-is-better' | 'lower-is-better'

export interface DashboardTodayMetricLabels {
  total: string
  success: string
  errors: string
  quota: string
}

export interface DashboardTodayMetricStrings {
  deltaFromYesterday: string
  deltaNoBaseline: string
  percentagePointUnit: string
  asOfNow: string
  todayShare: string
}

interface DashboardTodayMetricFormatters {
  formatNumber: (value: number) => string
  formatPercent: (numerator: number, denominator: number) => string
}

interface BuildDashboardTodayMetricsOptions {
  today: SummaryWindowMetrics
  yesterday: SummaryWindowMetrics
  labels: DashboardTodayMetricLabels
  strings: DashboardTodayMetricStrings
  formatters: DashboardTodayMetricFormatters
}

interface BuildCountComparisonOptions {
  currentValue: number
  previousValue: number
  strings: Pick<DashboardTodayMetricStrings, 'deltaFromYesterday' | 'deltaNoBaseline'>
  trend?: ComparisonTrend
}

interface BuildRateComparisonOptions {
  currentNumerator: number
  currentDenominator: number
  previousNumerator: number
  previousDenominator: number
  strings: Pick<
    DashboardTodayMetricStrings,
    'deltaFromYesterday' | 'deltaNoBaseline' | 'percentagePointUnit'
  >
  trend?: ComparisonTrend
}

const integerFormatter = new Intl.NumberFormat('en-US', {
  maximumFractionDigits: 0,
})

const percentageFormatter = new Intl.NumberFormat('en-US', {
  style: 'percent',
  minimumFractionDigits: 0,
  maximumFractionDigits: 1,
})

const percentagePointFormatter = new Intl.NumberFormat('en-US', {
  minimumFractionDigits: 1,
  maximumFractionDigits: 1,
})

function formatSignedInteger(value: number): string {
  if (value > 0) return `+${integerFormatter.format(value)}`
  return integerFormatter.format(value)
}

function formatSignedPercentagePoint(value: number): string {
  const normalized = Object.is(value, -0) ? 0 : value
  const formatted = percentagePointFormatter.format(Math.abs(normalized))
  if (normalized > 0) return `+${formatted}`
  if (normalized < 0) return `-${formatted}`
  return formatted
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

export function buildTodayRateComparison({
  currentNumerator,
  currentDenominator,
  previousNumerator,
  previousDenominator,
  strings,
  trend = 'higher-is-better',
}: BuildRateComparisonOptions): DashboardMetricComparison {
  if (previousDenominator === 0 && currentDenominator > 0) {
    return {
      label: strings.deltaFromYesterday,
      value: strings.deltaNoBaseline,
      direction: 'flat',
      tone: 'neutral',
    }
  }

  const currentRate = currentDenominator > 0 ? currentNumerator / currentDenominator : 0
  const previousRate = previousDenominator > 0 ? previousNumerator / previousDenominator : 0
  const deltaPercentagePoints = (currentRate - previousRate) * 100
  const direction: DashboardMetricComparison['direction'] =
    deltaPercentagePoints > 0 ? 'up' : deltaPercentagePoints < 0 ? 'down' : 'flat'

  return {
    label: strings.deltaFromYesterday,
    value: `${formatSignedPercentagePoint(deltaPercentagePoints)} ${strings.percentagePointUnit}`,
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
    {
      id: 'today-total',
      label: labels.total,
      value: formatNumber(today.total_requests),
      subtitle: strings.asOfNow,
      comparison: buildTodayCountComparison({
        currentValue: today.total_requests,
        previousValue: yesterday.total_requests,
        strings,
      }),
    },
    {
      id: 'today-success',
      label: labels.success,
      value: formatNumber(today.success_count),
      subtitle: buildWindowSubtitle(
        strings.todayShare,
        today.success_count,
        today.total_requests,
        formatPercent,
      ),
      comparison: buildTodayRateComparison({
        currentNumerator: today.success_count,
        currentDenominator: today.total_requests,
        previousNumerator: yesterday.success_count,
        previousDenominator: yesterday.total_requests,
        strings,
      }),
    },
    {
      id: 'today-errors',
      label: labels.errors,
      value: formatNumber(today.error_count),
      subtitle: buildWindowSubtitle(
        strings.todayShare,
        today.error_count,
        today.total_requests,
        formatPercent,
      ),
      comparison: buildTodayRateComparison({
        currentNumerator: today.error_count,
        currentDenominator: today.total_requests,
        previousNumerator: yesterday.error_count,
        previousDenominator: yesterday.total_requests,
        strings,
        trend: 'lower-is-better',
      }),
    },
    {
      id: 'today-quota',
      label: labels.quota,
      value: formatNumber(today.quota_exhausted_count),
      subtitle: buildWindowSubtitle(
        strings.todayShare,
        today.quota_exhausted_count,
        today.total_requests,
        formatPercent,
      ),
      comparison: buildTodayCountComparison({
        currentValue: today.quota_exhausted_count,
        previousValue: yesterday.quota_exhausted_count,
        strings,
        trend: 'lower-is-better',
      }),
    },
  ]
}
