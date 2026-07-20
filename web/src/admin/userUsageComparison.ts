import type { AdminTranslations } from '../i18n'

export function formatShadowDailyUsageComparison(args: {
  actualUsed: number
  shadowUsed: number | null
  usersStrings: AdminTranslations['users']
  formatNumber: (value: number) => string
}): string | null {
  const { actualUsed, shadowUsed, usersStrings, formatNumber } = args
  if (shadowUsed == null) return null

  const delta = shadowUsed - actualUsed
  if (delta === 0) return null
  const deltaText = `${delta >= 0 ? '+' : '-'}${formatNumber(Math.abs(delta))}`

  return usersStrings.usage.shadowComparisonValue.replace('{delta}', deltaText)
}

export function buildShadowDailyUsageStack(args: {
  actualUsed: number
  shadowUsed: number | null
  shadowAvailability: 'confirmed' | 'unavailable' | null
  limit: number
  usersStrings: AdminTranslations['users']
  formatNumber: (value: number) => string
  formatQuotaStackValue: (
    used: number,
    limit: number,
  ) => {
    primary: string
    secondary?: string | null
    primaryClassName?: string | null
  }
}): {
  primary: string
  secondary?: string | null
  primaryClassName?: string | null
} {
  const { actualUsed, shadowUsed, shadowAvailability, limit, usersStrings, formatNumber, formatQuotaStackValue } = args

  if (shadowAvailability === 'unavailable') {
    return {
      primary: usersStrings.usage.shadowUnavailable,
      secondary: null,
    }
  }
  if (shadowAvailability !== 'confirmed' || shadowUsed == null) {
    return {
      primary: '—',
      secondary: null,
    }
  }

  const shadowMetric = formatQuotaStackValue(shadowUsed, limit)
  return {
    primary: shadowMetric.primary,
    primaryClassName: shadowMetric.primaryClassName ?? null,
    secondary: formatShadowDailyUsageComparison({
      actualUsed,
      shadowUsed,
      usersStrings,
      formatNumber,
    }),
  }
}
