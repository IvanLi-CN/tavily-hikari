import type {
  AdminQuotaLimitSet,
  AdminUserDetail,
  AdminUserQuotaBreakdownEntry,
  AdminUserSummary,
  AdminUserTag,
  AdminUserTagBinding,
  BusinessCalls1hSummary,
  Paginated,
  RequestRate,
} from './runtime'

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

function readBoolean(value: RecordLike, camelKey: string, snakeKey = camelKey, fallback = false): boolean {
  const candidate = value[camelKey] ?? value[snakeKey]
  return typeof candidate === 'boolean' ? candidate : fallback
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

function normalizeBusinessCalls1h(
  value: unknown,
  fallback: Pick<BusinessCalls1hSummary, 'limit'> & Partial<BusinessCalls1hSummary> = { limit: 0 },
): BusinessCalls1hSummary {
  const source = isRecordLike(value) ? value : {}
  return {
    successCount: readNumber(
      source,
      'successCount',
      'success_count',
      fallback.successCount ?? 0,
    ),
    failureCount: readNumber(
      source,
      'failureCount',
      'failure_count',
      fallback.failureCount ?? 0,
    ),
    totalCount: readNumber(source, 'totalCount', 'total_count', fallback.totalCount ?? 0),
    limit: readNumber(source, 'limit', 'limit', fallback.limit),
    windowMinutes: readNumber(
      source,
      'windowMinutes',
      'window_minutes',
      fallback.windowMinutes ?? 60,
    ),
  }
}

function normalizeAdminQuotaLimitSet(value: unknown): AdminQuotaLimitSet {
  const source = isRecordLike(value) ? value : {}
  const businessCalls1hLimit = readNumber(
    source,
    'businessCalls1hLimit',
    'business_calls_1h_limit',
    readNumber(source, 'hourlyLimit', 'hourly_limit'),
  )
  const dailyCreditsLimit = readNumber(
    source,
    'dailyCreditsLimit',
    'daily_credits_limit',
    readNumber(source, 'dailyLimit', 'daily_limit'),
  )
  const monthlyCreditsLimit = readNumber(
    source,
    'monthlyCreditsLimit',
    'monthly_credits_limit',
    readNumber(source, 'monthlyLimit', 'monthly_limit'),
  )
  return {
    businessCalls1hLimit,
    dailyCreditsLimit,
    monthlyCreditsLimit,
    inheritsDefaults: readBoolean(source, 'inheritsDefaults', 'inherits_defaults'),
  }
}

function normalizeAdminUserTag(value: unknown): AdminUserTag {
  const source = isRecordLike(value) ? value : {}
  const businessCalls1hDelta = readNumber(
    source,
    'businessCalls1hDelta',
    'business_calls_1h_delta',
    readNumber(source, 'hourlyDelta', 'hourly_delta'),
  )
  const dailyCreditsDelta = readNumber(
    source,
    'dailyCreditsDelta',
    'daily_credits_delta',
    readNumber(source, 'dailyDelta', 'daily_delta'),
  )
  const monthlyCreditsDelta = readNumber(
    source,
    'monthlyCreditsDelta',
    'monthly_credits_delta',
    readNumber(source, 'monthlyDelta', 'monthly_delta'),
  )
  return {
    id: readString(source, 'id'),
    name: readString(source, 'name'),
    displayName: readString(source, 'displayName', 'display_name'),
    icon: readNullableString(source, 'icon'),
    systemKey: readNullableString(source, 'systemKey', 'system_key'),
    effectKind: readString(source, 'effectKind', 'effect_kind'),
    businessCalls1hDelta,
    dailyCreditsDelta,
    monthlyCreditsDelta,
    userCount: readNumber(source, 'userCount', 'user_count'),
  }
}

function normalizeAdminUserTagBinding(value: unknown): AdminUserTagBinding {
  const source = isRecordLike(value) ? value : {}
  const businessCalls1hDelta = readNumber(
    source,
    'businessCalls1hDelta',
    'business_calls_1h_delta',
    readNumber(source, 'hourlyDelta', 'hourly_delta'),
  )
  const dailyCreditsDelta = readNumber(
    source,
    'dailyCreditsDelta',
    'daily_credits_delta',
    readNumber(source, 'dailyDelta', 'daily_delta'),
  )
  const monthlyCreditsDelta = readNumber(
    source,
    'monthlyCreditsDelta',
    'monthly_credits_delta',
    readNumber(source, 'monthlyDelta', 'monthly_delta'),
  )
  return {
    tagId: readString(source, 'tagId', 'tag_id'),
    name: readString(source, 'name'),
    displayName: readString(source, 'displayName', 'display_name'),
    icon: readNullableString(source, 'icon'),
    systemKey: readNullableString(source, 'systemKey', 'system_key'),
    effectKind: readString(source, 'effectKind', 'effect_kind'),
    businessCalls1hDelta,
    dailyCreditsDelta,
    monthlyCreditsDelta,
    source: readString(source, 'source'),
  }
}

function normalizeAdminUserQuotaBreakdownEntry(value: unknown): AdminUserQuotaBreakdownEntry {
  const source = isRecordLike(value) ? value : {}
  const businessCalls1hDelta = readNumber(
    source,
    'businessCalls1hDelta',
    'business_calls_1h_delta',
    readNumber(source, 'hourlyDelta', 'hourly_delta'),
  )
  const dailyCreditsDelta = readNumber(
    source,
    'dailyCreditsDelta',
    'daily_credits_delta',
    readNumber(source, 'dailyDelta', 'daily_delta'),
  )
  const monthlyCreditsDelta = readNumber(
    source,
    'monthlyCreditsDelta',
    'monthly_credits_delta',
    readNumber(source, 'monthlyDelta', 'monthly_delta'),
  )
  return {
    kind: readString(source, 'kind'),
    label: readString(source, 'label'),
    tagId: readNullableString(source, 'tagId', 'tag_id'),
    tagName: readNullableString(source, 'tagName', 'tag_name'),
    source: readNullableString(source, 'source'),
    effectKind: readString(source, 'effectKind', 'effect_kind'),
    businessCalls1hDelta,
    dailyCreditsDelta,
    monthlyCreditsDelta,
  }
}

export function normalizeAdminUserSummary(value: unknown): AdminUserSummary {
  const source = isRecordLike(value) ? value : {}
  const legacyHourlyAnyUsed = readNumber(source, 'hourlyAnyUsed', 'hourly_any_used')
  const legacyHourlyAnyLimit = readNumber(source, 'hourlyAnyLimit', 'hourly_any_limit', 60)
  const requestRate = normalizeRequestRate(source.requestRate ?? source.request_rate, {
    used: legacyHourlyAnyUsed,
    limit: legacyHourlyAnyLimit,
    windowMinutes: 5,
    scope: 'user',
  })
  const businessCalls1h = normalizeBusinessCalls1h(source.businessCalls1h ?? source.business_calls_1h, {
    totalCount: readNumber(source, 'quotaHourlyUsed', 'quota_hourly_used'),
    limit: readNumber(source, 'quotaHourlyLimit', 'quota_hourly_limit'),
  })
  const dailyCreditsUsed = readNumber(
    source,
    'dailyCreditsUsed',
    'daily_credits_used',
    readNumber(source, 'quotaDailyUsed', 'quota_daily_used'),
  )
  const dailyCreditsLimit = readNumber(
    source,
    'dailyCreditsLimit',
    'daily_credits_limit',
    readNumber(source, 'quotaDailyLimit', 'quota_daily_limit'),
  )
  const monthlyCreditsUsed = readNumber(
    source,
    'monthlyCreditsUsed',
    'monthly_credits_used',
    readNumber(source, 'quotaMonthlyUsed', 'quota_monthly_used'),
  )
  const monthlyCreditsLimit = readNumber(
    source,
    'monthlyCreditsLimit',
    'monthly_credits_limit',
    readNumber(source, 'quotaMonthlyLimit', 'quota_monthly_limit'),
  )
  const tags = Array.isArray(source.tags) ? source.tags.map(normalizeAdminUserTagBinding) : []

  return {
    userId: readString(source, 'userId', 'user_id'),
    displayName: readNullableString(source, 'displayName', 'display_name'),
    username: readNullableString(source, 'username', 'username'),
    active: readBoolean(source, 'active'),
    lastLoginAt: readNullableNumber(source, 'lastLoginAt', 'last_login_at'),
    tokenCount: readNumber(source, 'tokenCount', 'token_count'),
    apiKeyCount: readNumber(source, 'apiKeyCount', 'api_key_count'),
    tags,
    requestRate,
    businessCalls1h,
    dailyCreditsUsed,
    dailyCreditsLimit,
    monthlyCreditsUsed,
    monthlyCreditsLimit,
    dailySuccess: readNumber(source, 'dailySuccess', 'daily_success'),
    dailyFailure: readNumber(source, 'dailyFailure', 'daily_failure'),
    monthlySuccess: readNumber(source, 'monthlySuccess', 'monthly_success'),
    monthlyFailure: readNumber(source, 'monthlyFailure', 'monthly_failure'),
    monthlyBrokenCount: readNumber(source, 'monthlyBrokenCount', 'monthly_broken_count'),
    monthlyBrokenLimit: readNumber(source, 'monthlyBrokenLimit', 'monthly_broken_limit'),
    recentIpCount24h: readNumber(source, 'recentIpCount24h', 'recent_ip_count_24h'),
    recentIpCount7d: readNumber(source, 'recentIpCount7d', 'recent_ip_count_7d'),
    lastActivity: readNullableNumber(source, 'lastActivity', 'last_activity'),
  }
}

export function normalizeAdminUserDetail(value: unknown): AdminUserDetail {
  const source = isRecordLike(value) ? value : {}
  const quotaBreakdownSource = source.quotaBreakdown ?? source.quota_breakdown
  const recentIpAddresses24hSource = source.recentIpAddresses24h ?? source.recent_ip_addresses_24h
  const recentIpAddresses7dSource = source.recentIpAddresses7d ?? source.recent_ip_addresses_7d
  const recentIpTimeline7dSource = source.recentIpTimeline7d ?? source.recent_ip_timeline_7d
  return {
    ...normalizeAdminUserSummary(source),
    tokens: Array.isArray(source.tokens) ? (source.tokens as AdminUserDetail['tokens']) : [],
    quotaBase: normalizeAdminQuotaLimitSet(source.quotaBase ?? source.quota_base),
    effectiveQuota: normalizeAdminQuotaLimitSet(source.effectiveQuota ?? source.effective_quota),
    quotaBreakdown: Array.isArray(quotaBreakdownSource)
      ? quotaBreakdownSource.map(normalizeAdminUserQuotaBreakdownEntry)
      : [],
    recharge: isRecordLike(source.recharge)
      ? (source.recharge as unknown as AdminUserDetail['recharge'])
      : {
          currentMonthEntitlementCredits: 0,
          currentMonthEntitlementHourlyDelta: 0,
          currentMonthEntitlementDailyDelta: 0,
          currentMonthEntitlementMonthlyDelta: 0,
          effectiveUntilMonthStart: null,
          orders: [],
          entitlements: [],
        },
    recentIpAddresses24h: Array.isArray(recentIpAddresses24hSource)
      ? recentIpAddresses24hSource.filter((item: unknown): item is string => typeof item === 'string')
      : [],
    recentIpAddresses7d: Array.isArray(recentIpAddresses7dSource)
      ? recentIpAddresses7dSource.filter((item: unknown): item is string => typeof item === 'string')
      : [],
    recentIpTimeline7d: Array.isArray(recentIpTimeline7dSource)
      ? (recentIpTimeline7dSource as AdminUserDetail['recentIpTimeline7d'])
      : [],
  }
}

export function normalizeAdminUserSummaryPage(value: unknown): Paginated<AdminUserSummary> {
  const source = isRecordLike(value) ? value : {}
  const items = Array.isArray(source.items) ? source.items.map(normalizeAdminUserSummary) : []
  return {
    items,
    total: readNumber(source, 'total'),
    page: readNumber(source, 'page', 'page', 1),
    perPage: readNumber(source, 'perPage', 'per_page', items.length),
  }
}

export function normalizeAdminUserTagList(value: unknown): AdminUserTag[] {
  const source = isRecordLike(value) ? value : {}
  const items = Array.isArray(source.items) ? source.items : Array.isArray(value) ? value : []
  return items.map(normalizeAdminUserTag)
}

export { normalizeAdminUserTag }
