import type { ReactNode } from 'react'

import type { RequestRate } from '../api'
import { UsageMetricLabel } from '../components/UsageMetricLabel'

interface DashboardQuotaGridText {
  hourly: string
  daily: string
  monthly: string
}

interface DashboardQuotaGridProps {
  text: DashboardQuotaGridText
  rateLabel: string
  rate: RequestRate
  hourlyUsed: number
  hourlyLimit: number
  dailyUsed: number
  dailyLimit: number
  monthlyUsed: number
  monthlyLimit: number
  formatNumber: (value: number) => string
  language: 'en' | 'zh'
}

export default function DashboardQuotaGrid({
  text,
  rateLabel,
  rate,
  hourlyUsed,
  hourlyLimit,
  dailyUsed,
  dailyLimit,
  monthlyUsed,
  monthlyLimit,
  formatNumber,
  language,
}: DashboardQuotaGridProps): JSX.Element {
  const helpLabels: Record<'hourly' | 'daily' | 'monthly', ReactNode> = {
    hourly: (
      <UsageMetricLabel label={text.hourly} kind="businessCalls1h" language={language} className="quota-stat-label" />
    ),
    daily: (
      <UsageMetricLabel label={text.daily} kind="dailyCredits" language={language} className="quota-stat-label" />
    ),
    monthly: (
      <UsageMetricLabel label={text.monthly} kind="monthlyCredits" language={language} className="quota-stat-label" />
    ),
  }

  return (
    <div className="access-stats">
      <div className="access-stat quota-stat-card">
        <div className="quota-stat-label">{rateLabel}</div>
        <div className="quota-stat-value">
          {formatNumber(rate.used)}
          <span>/ {formatNumber(rate.limit)}</span>
        </div>
      </div>
      <div className="access-stat quota-stat-card">
        <div>{helpLabels.hourly}</div>
        <div className="quota-stat-value">
          {formatNumber(hourlyUsed)}
          <span>/ {formatNumber(hourlyLimit)}</span>
        </div>
      </div>
      <div className="access-stat quota-stat-card">
        <div>{helpLabels.daily}</div>
        <div className="quota-stat-value">
          {formatNumber(dailyUsed)}
          <span>/ {formatNumber(dailyLimit)}</span>
        </div>
      </div>
      <div className="access-stat quota-stat-card">
        <div>{helpLabels.monthly}</div>
        <div className="quota-stat-value">
          {formatNumber(monthlyUsed)}
          <span>/ {formatNumber(monthlyLimit)}</span>
        </div>
      </div>
    </div>
  )
}
