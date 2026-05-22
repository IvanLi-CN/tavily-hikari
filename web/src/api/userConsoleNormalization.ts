import type { RequestRate, UserDashboard, UserTokenSummary } from './runtime'

type RecordLike = Record<string, unknown>

function isRecordLike(value: unknown): value is RecordLike {
  return typeof value === 'object' && value !== null
}

function readString(value: RecordLike, camelKey: string, snakeKey = camelKey): string {
  const candidate = value[camelKey] ?? value[snakeKey]
  return typeof candidate === 'string' ? candidate : ''
}

function readNullableString(value: RecordLike, camelKey: string, snakeKey = camelKey): string | null {
  const candidate = value[camelKey] ?? value[snakeKey]
  return typeof candidate === 'string' ? candidate : null
}

function readBoolean(value: RecordLike, camelKey: string, snakeKey = camelKey): boolean {
  return Boolean(value[camelKey] ?? value[snakeKey])
}

function readNumber(value: RecordLike, camelKey: string, snakeKey = camelKey, fallback = 0): number {
  const candidate = value[camelKey] ?? value[snakeKey]
  return typeof candidate === 'number' && Number.isFinite(candidate) ? candidate : fallback
}

function readNullableNumber(value: RecordLike, camelKey: string, snakeKey = camelKey): number | null {
  const candidate = value[camelKey] ?? value[snakeKey]
  return typeof candidate === 'number' && Number.isFinite(candidate) ? candidate : null
}

function normalizeRequestRate(value: unknown, fallback: RequestRate): RequestRate {
  if (!isRecordLike(value)) return fallback
  const scope = value.scope === 'user' || value.scope === 'token' ? value.scope : fallback.scope
  return {
    used: readNumber(value, 'used', 'used', fallback.used),
    limit: readNumber(value, 'limit', 'limit', fallback.limit),
    windowMinutes: readNumber(value, 'windowMinutes', 'window_minutes', fallback.windowMinutes),
    scope,
  }
}

export function normalizeUserDashboard(value: unknown): UserDashboard {
  const source = isRecordLike(value) ? value : {}
  const hourlyAnyUsed = readNumber(source, 'hourlyAnyUsed', 'hourly_any_used')
  const hourlyAnyLimit = readNumber(source, 'hourlyAnyLimit', 'hourly_any_limit', 60)
  return {
    requestRate: normalizeRequestRate(source.requestRate ?? source.request_rate, {
      used: hourlyAnyUsed,
      limit: hourlyAnyLimit,
      windowMinutes: 5,
      scope: 'user',
    }),
    hourlyAnyUsed,
    hourlyAnyLimit,
    quotaHourlyUsed: readNumber(source, 'quotaHourlyUsed', 'quota_hourly_used'),
    quotaHourlyLimit: readNumber(source, 'quotaHourlyLimit', 'quota_hourly_limit'),
    quotaDailyUsed: readNumber(source, 'quotaDailyUsed', 'quota_daily_used'),
    quotaDailyLimit: readNumber(source, 'quotaDailyLimit', 'quota_daily_limit'),
    quotaMonthlyUsed: readNumber(source, 'quotaMonthlyUsed', 'quota_monthly_used'),
    quotaMonthlyLimit: readNumber(source, 'quotaMonthlyLimit', 'quota_monthly_limit'),
    dailySuccess: readNumber(source, 'dailySuccess', 'daily_success'),
    dailyFailure: readNumber(source, 'dailyFailure', 'daily_failure'),
    monthlySuccess: readNumber(source, 'monthlySuccess', 'monthly_success'),
    lastActivity: readNullableNumber(source, 'lastActivity', 'last_activity'),
  }
}

export function normalizeUserTokenSummary(value: unknown): UserTokenSummary {
  const source = isRecordLike(value) ? value : {}
  const tokenId = readString(source, 'tokenId', 'id') || readString(source, 'tokenId', 'token_id')
  const hourlyAnyUsed = readNumber(source, 'hourlyAnyUsed', 'hourly_any_used')
  const hourlyAnyLimit = readNumber(source, 'hourlyAnyLimit', 'hourly_any_limit', 60)
  return {
    tokenId,
    enabled: readBoolean(source, 'enabled', 'enabled'),
    note: readNullableString(source, 'note', 'note'),
    lastUsedAt: readNullableNumber(source, 'lastUsedAt', 'last_used_at'),
    requestRate: normalizeRequestRate(source.requestRate ?? source.request_rate, {
      used: hourlyAnyUsed,
      limit: hourlyAnyLimit,
      windowMinutes: 5,
      scope: 'token',
    }),
    hourlyAnyUsed,
    hourlyAnyLimit,
    quotaHourlyUsed: readNumber(source, 'quotaHourlyUsed', 'quota_hourly_used'),
    quotaHourlyLimit: readNumber(source, 'quotaHourlyLimit', 'quota_hourly_limit'),
    quotaDailyUsed: readNumber(source, 'quotaDailyUsed', 'quota_daily_used'),
    quotaDailyLimit: readNumber(source, 'quotaDailyLimit', 'quota_daily_limit'),
    quotaMonthlyUsed: readNumber(source, 'quotaMonthlyUsed', 'quota_monthly_used'),
    quotaMonthlyLimit: readNumber(source, 'quotaMonthlyLimit', 'quota_monthly_limit'),
    dailySuccess: readNumber(
      source,
      'dailySuccess',
      'daily_success',
      readNumber(source, 'quotaDailyUsed', 'quota_daily_used'),
    ),
    dailyFailure: readNumber(source, 'dailyFailure', 'daily_failure'),
    monthlySuccess: readNumber(
      source,
      'monthlySuccess',
      'monthly_success',
      readNumber(source, 'quotaMonthlyUsed', 'quota_monthly_used'),
    ),
  }
}

export function normalizeUserTokenSummaryList(value: unknown): UserTokenSummary[] {
  const rawItems = Array.isArray(value) ? value : isRecordLike(value) && Array.isArray(value.items) ? value.items : []
  return rawItems.map(normalizeUserTokenSummary)
}
