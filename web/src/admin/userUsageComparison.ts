import type { AdminTranslations } from '../i18n'

export function formatShadowDailyUsageComparison(args: {
  actualUsed: number
  shadowUsed: number | null
  limit: number
  usersStrings: AdminTranslations['users']
  formatQuotaUsagePair: (used: number, limit: number) => string
  formatNumber: (value: number) => string
}): string | null {
  const { actualUsed, shadowUsed, limit, usersStrings, formatQuotaUsagePair, formatNumber } = args
  if (shadowUsed == null) return null

  const delta = shadowUsed - actualUsed
  const deltaText = `${delta >= 0 ? '+' : '-'}${formatNumber(Math.abs(delta))}`

  return usersStrings.usage.shadowComparisonValue
    .replace('{usage}', formatQuotaUsagePair(shadowUsed, limit))
    .replace('{delta}', deltaText)
}
