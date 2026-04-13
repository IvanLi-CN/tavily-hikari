import type { RequestRate, RequestRateScope } from './api'

interface LegacyRequestRateLike {
  requestRate?: RequestRate
  hourlyAnyUsed?: number | null
  hourlyAnyLimit?: number | null
}

export function resolveRequestRate(
  value: LegacyRequestRateLike | null | undefined,
  fallbackScope: RequestRateScope,
): RequestRate {
  if (value?.requestRate) {
    return value.requestRate
  }
  return {
    used: value?.hourlyAnyUsed ?? 0,
    limit: value?.hourlyAnyLimit ?? 60,
    windowMinutes: 5,
    scope: fallbackScope,
  }
}

export function defaultRequestRateLabel(language: 'zh' | 'en', windowMinutes = 5): string {
  if (language === 'zh') {
    return `${windowMinutes} 分钟请求频率`
  }
  return `${windowMinutes}m request rate`
}

export function formatRequestRateLabel(rate: RequestRate, language: 'zh' | 'en'): string {
  return defaultRequestRateLabel(language, rate.windowMinutes)
}

export function formatRequestRateScope(rate: RequestRate, language: 'zh' | 'en'): string {
  if (language === 'zh') {
    return rate.scope === 'user' ? '用户共享' : 'Token 独立'
  }
  return rate.scope === 'user' ? 'shared by user' : 'token-local'
}

export function formatRequestRateSummary(rate: RequestRate, language: 'zh' | 'en'): string {
  if (language === 'zh') {
    return `${formatRequestRateLabel(rate, language)} · ${formatRequestRateScope(rate, language)}`
  }
  return `${formatRequestRateLabel(rate, language)} · ${formatRequestRateScope(rate, language)}`
}
