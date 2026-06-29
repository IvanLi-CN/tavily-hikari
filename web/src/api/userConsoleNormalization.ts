import type {
  RequestRate,
  UserDashboard,
  UserDashboardOverview,
  UserDashboardOverviewSeriesPoint,
  UserDashboardProgressCard,
  UserTokenSummary,
} from './runtime'
import type { RechargeConfig, RechargeOrder, RechargeQuote, RechargeQuoteMonth } from './recharge'

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

function normalizeRechargeQuoteMonth(value: unknown): RechargeQuoteMonth {
  const source = isRecordLike(value) ? value : {}
  return {
    monthIndex: readNumber(source, 'monthIndex', 'month_index'),
    monthStart: readNumber(source, 'monthStart', 'month_start'),
    isCurrentMonth: readBoolean(source, 'isCurrentMonth', 'is_current_month'),
    hourlyDelta: readNumber(source, 'hourlyDelta', 'hourly_delta'),
    dailyDelta: readNumber(source, 'dailyDelta', 'daily_delta'),
    monthlyDelta: readNumber(source, 'monthlyDelta', 'monthly_delta'),
    fullMonthlyDelta: readNumber(source, 'fullMonthlyDelta', 'full_monthly_delta'),
    monthMoneyCents: readNumber(source, 'monthMoneyCents', 'month_money_cents'),
    monthDiscountCents: readNumber(source, 'monthDiscountCents', 'month_discount_cents'),
    monthEndClampApplied: readBoolean(source, 'monthEndClampApplied', 'month_end_clamp_applied'),
    discountReason: readNullableString(source, 'discountReason', 'discount_reason'),
  }
}

export function normalizeRechargeQuote(value: unknown): RechargeQuote {
  const source = isRecordLike(value) ? value : {}
  const schedule = Array.isArray(source.schedule) ? source.schedule.map(normalizeRechargeQuoteMonth) : []
  return {
    requestedCredits: readNumber(source, 'requestedCredits', 'requested_credits'),
    requestedMonths: readNumber(source, 'requestedMonths', 'requested_months'),
    quoteMonthStart: readNumber(source, 'quoteMonthStart', 'quote_month_start'),
    remainingDaysInclusive: readNumber(source, 'remainingDaysInclusive', 'remaining_days_inclusive'),
    unitCredits: readNumber(source, 'unitCredits', 'unit_credits'),
    unitPriceCents: readNumber(source, 'unitPriceCents', 'unit_price_cents'),
    fullMonthHourlyDelta: readNumber(source, 'fullMonthHourlyDelta', 'full_month_hourly_delta'),
    fullMonthDailyDelta: readNumber(source, 'fullMonthDailyDelta', 'full_month_daily_delta'),
    fullMonthMonthlyDelta: readNumber(source, 'fullMonthMonthlyDelta', 'full_month_monthly_delta'),
    fullMonthMoneyCents: readNumber(source, 'fullMonthMoneyCents', 'full_month_money_cents'),
    currentMonthFinalHourlyDelta: readNumber(source, 'currentMonthFinalHourlyDelta', 'current_month_final_hourly_delta'),
    currentMonthFinalDailyDelta: readNumber(source, 'currentMonthFinalDailyDelta', 'current_month_final_daily_delta'),
    currentMonthFinalMonthlyDelta: readNumber(source, 'currentMonthFinalMonthlyDelta', 'current_month_final_monthly_delta'),
    currentMonthFinalMoneyCents: readNumber(source, 'currentMonthFinalMoneyCents', 'current_month_final_money_cents'),
    fullOrderMoneyCents: readNumber(source, 'fullOrderMoneyCents', 'full_order_money_cents'),
    finalOrderMoneyCents: readNumber(source, 'finalOrderMoneyCents', 'final_order_money_cents'),
    monthEndClampApplied: readBoolean(source, 'monthEndClampApplied', 'month_end_clamp_applied'),
    orderName: readString(source, 'orderName', 'order_name'),
    schedule,
  }
}

function normalizeRechargeSummary(value: unknown): UserDashboard['recharge'] {
  const source = isRecordLike(value) ? value : {}
  return {
    currentMonthStart: readNumber(source, 'currentMonthStart', 'current_month_start'),
    currentEntitlementCredits: readNumber(
      source,
      'currentEntitlementCredits',
      'current_entitlement_credits',
    ),
    currentEntitlementHourlyDelta: readNumber(
      source,
      'currentEntitlementHourlyDelta',
      'current_entitlement_hourly_delta',
    ),
    currentEntitlementDailyDelta: readNumber(
      source,
      'currentEntitlementDailyDelta',
      'current_entitlement_daily_delta',
    ),
    currentEntitlementMonthlyDelta: readNumber(
      source,
      'currentEntitlementMonthlyDelta',
      'current_entitlement_monthly_delta',
    ),
    effectiveUntilMonthStart: readNullableNumber(
      source,
      'effectiveUntilMonthStart',
      'effective_until_month_start',
    ),
  }
}

export function normalizeRechargeConfig(value: unknown): RechargeConfig {
  const source = isRecordLike(value) ? value : {}
  return {
    visible: readBoolean(source, 'visible', 'visible', true),
    enabled: readBoolean(source, 'enabled', 'enabled'),
    unitCredits: readNumber(source, 'unitCredits', 'unit_credits', 1000),
    unitPriceLdc: readNumber(source, 'unitPriceLdc', 'unit_price_ldc', 50),
    minCredits: readNumber(source, 'minCredits', 'min_credits', 1000),
    maxCredits: readNumber(source, 'maxCredits', 'max_credits', 20_000),
    creditsStep: readNumber(source, 'creditsStep', 'credits_step', 1000),
    defaultCredits: readNumber(source, 'defaultCredits', 'default_credits', 1000),
    minMonths: readNumber(source, 'minMonths', 'min_months', 1),
    maxMonths: readNumber(source, 'maxMonths', 'max_months', 12),
    quotaDeltaBaseCredits: readNumber(source, 'quotaDeltaBaseCredits', 'quota_delta_base_credits', 1000),
    hourlyDeltaPerQuotaUnit: readNumber(source, 'hourlyDeltaPerQuotaUnit', 'hourly_delta_per_quota_unit', 20),
    dailyDeltaPerQuotaUnit: readNumber(source, 'dailyDeltaPerQuotaUnit', 'daily_delta_per_quota_unit', 100),
    monthlyDeltaPerQuotaUnit: readNumber(source, 'monthlyDeltaPerQuotaUnit', 'monthly_delta_per_quota_unit', 1000),
    testPriceEnabled: readBoolean(source, 'testPriceEnabled', 'test_price_enabled'),
    currentMonthStart: readNumber(source, 'currentMonthStart', 'current_month_start'),
    currentEntitlementCredits: readNumber(
      source,
      'currentEntitlementCredits',
      'current_entitlement_credits',
    ),
    currentEntitlementHourlyDelta: readNumber(
      source,
      'currentEntitlementHourlyDelta',
      'current_entitlement_hourly_delta',
    ),
    currentEntitlementDailyDelta: readNumber(
      source,
      'currentEntitlementDailyDelta',
      'current_entitlement_daily_delta',
    ),
    currentEntitlementMonthlyDelta: readNumber(
      source,
      'currentEntitlementMonthlyDelta',
      'current_entitlement_monthly_delta',
    ),
    effectiveUntilMonthStart: readNullableNumber(
      source,
      'effectiveUntilMonthStart',
      'effective_until_month_start',
    ),
  }
}

export function normalizeRechargeOrder(value: unknown): RechargeOrder {
  const source = isRecordLike(value) ? value : {}
  return {
    outTradeNo: readString(source, 'outTradeNo', 'out_trade_no'),
    status: readString(source, 'status', 'status'),
    credits: readNumber(source, 'credits', 'credits'),
    months: readNumber(source, 'months', 'months'),
    money: readString(source, 'money', 'money'),
    quoteMonthStart: readNumber(source, 'quoteMonthStart', 'quote_month_start'),
    finalMoneyCents: readNumber(source, 'finalMoneyCents', 'final_money_cents'),
    finalHourlyDelta: readNumber(source, 'finalHourlyDelta', 'final_hourly_delta'),
    finalDailyDelta: readNumber(source, 'finalDailyDelta', 'final_daily_delta'),
    finalMonthlyDelta: readNumber(source, 'finalMonthlyDelta', 'final_monthly_delta'),
    monthEndClampApplied: readBoolean(source, 'monthEndClampApplied', 'month_end_clamp_applied'),
    tradeNo: readNullableString(source, 'tradeNo', 'trade_no'),
    paymentUrl: readNullableString(source, 'paymentUrl', 'payment_url'),
    createdAt: readNumber(source, 'createdAt', 'created_at'),
    updatedAt: readNumber(source, 'updatedAt', 'updated_at'),
    paidAt: readNullableNumber(source, 'paidAt', 'paid_at'),
    lastNotifyAt: readNullableNumber(source, 'lastNotifyAt', 'last_notify_at'),
    lastError: readNullableString(source, 'lastError', 'last_error'),
  }
}

export function normalizeRechargeOrderList(value: unknown): RechargeOrder[] {
  const source = isRecordLike(value) ? value : {}
  const items = Array.isArray(source.items) ? source.items : []
  return items.map(normalizeRechargeOrder)
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

function normalizeUserDashboardOverviewSeriesPoint(
  value: unknown,
): UserDashboardOverviewSeriesPoint {
  const source = isRecordLike(value) ? value : {}
  return {
    bucketStart: readNumber(source, 'bucketStart', 'bucket_start'),
    displayBucketStart: readNullableNumber(source, 'displayBucketStart', 'display_bucket_start'),
    value: readNullableNumber(source, 'value', 'value'),
    limitValue: readNullableNumber(source, 'limitValue', 'limit_value'),
  }
}

function normalizeUserDashboardProgressCard(value: unknown): UserDashboardProgressCard {
  const source = isRecordLike(value) ? value : {}
  const points = Array.isArray(source.points) ? source.points : []
  return {
    used: readNumber(source, 'used', 'used'),
    limit: readNumber(source, 'limit', 'limit'),
    points: points.map(normalizeUserDashboardOverviewSeriesPoint),
  }
}

export function normalizeUserDashboard(value: unknown): UserDashboard {
  const source = isRecordLike(value) ? value : {}
  const hourlyAnyUsed = readNumber(source, 'hourlyAnyUsed', 'hourly_any_used')
  const hourlyAnyLimit = readNumber(source, 'hourlyAnyLimit', 'hourly_any_limit', 60)
  const businessCalls1hSource = source.businessCalls1h ?? source.business_calls_1h
  const businessCalls1h = isRecordLike(businessCalls1hSource) ? businessCalls1hSource : {}
  const businessCalls1hSummary = {
    successCount: readNumber(
      businessCalls1h,
      'successCount',
      'success_count',
    ),
    failureCount: readNumber(
      businessCalls1h,
      'failureCount',
      'failure_count',
    ),
    totalCount: readNumber(
      businessCalls1h,
      'totalCount',
      'total_count',
      readNumber(source, 'quotaHourlyUsed', 'quota_hourly_used'),
    ),
    limit: readNumber(
      businessCalls1h,
      'limit',
      'limit',
      readNumber(source, 'quotaHourlyLimit', 'quota_hourly_limit'),
    ),
    windowMinutes: readNumber(
      businessCalls1h,
      'windowMinutes',
      'window_minutes',
      60,
    ),
  }
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
  return {
    debugInfoShared: readBoolean(source, 'debugInfoShared', 'debug_info_shared'),
    requestRate: normalizeRequestRate(source.requestRate ?? source.request_rate, {
      used: hourlyAnyUsed,
      limit: hourlyAnyLimit,
      windowMinutes: 5,
      scope: 'user',
    }),
    businessCalls1h: businessCalls1hSummary,
    dailyCreditsUsed,
    dailyCreditsLimit,
    monthlyCreditsUsed,
    monthlyCreditsLimit,
    dailySuccess: readNumber(source, 'dailySuccess', 'daily_success'),
    dailyFailure: readNumber(source, 'dailyFailure', 'daily_failure'),
    monthlySuccess: readNumber(source, 'monthlySuccess', 'monthly_success'),
    lastActivity: readNullableNumber(source, 'lastActivity', 'last_activity'),
    recharge: normalizeRechargeSummary(source.recharge),
  }
}

export function normalizeUserDashboardOverview(value: unknown): UserDashboardOverview {
  const source = isRecordLike(value) ? value : {}
  const progress = isRecordLike(source.progress) ? source.progress : {}
  const businessCalls1h = normalizeUserDashboardProgressCard(
    progress.businessCalls1h
      ?? progress.business_calls_1h
      ?? progress.quotaHourly
      ?? progress.quota_hourly,
  )
  const dailyCredits = normalizeUserDashboardProgressCard(
    progress.dailyCredits
      ?? progress.daily_credits
      ?? progress.quotaDaily
      ?? progress.quota_daily,
  )
  const monthlyCredits = normalizeUserDashboardProgressCard(
    progress.monthlyCredits
      ?? progress.monthly_credits
      ?? progress.quotaMonthly
      ?? progress.quota_monthly,
  )
  return {
    summary: normalizeUserDashboard(source.summary),
    progress: {
      requestRate: normalizeUserDashboardProgressCard(progress.requestRate ?? progress.request_rate),
      businessCalls1h,
      dailyCredits,
      monthlyCredits,
    },
  }
}

export function normalizeUserTokenSummary(value: unknown): UserTokenSummary {
  const source = isRecordLike(value) ? value : {}
  const tokenId = readString(source, 'tokenId', 'id') || readString(source, 'tokenId', 'token_id')
  const hourlyAnyUsed = readNumber(source, 'hourlyAnyUsed', 'hourly_any_used')
  const hourlyAnyLimit = readNumber(source, 'hourlyAnyLimit', 'hourly_any_limit', 60)
  const businessCalls1hSource = source.businessCalls1h ?? source.business_calls_1h
  const businessCalls1h = isRecordLike(businessCalls1hSource) ? businessCalls1hSource : {}
  const businessCalls1hSummary = {
    successCount: readNumber(
      businessCalls1h,
      'successCount',
      'success_count',
    ),
    failureCount: readNumber(
      businessCalls1h,
      'failureCount',
      'failure_count',
    ),
    totalCount: readNumber(
      businessCalls1h,
      'totalCount',
      'total_count',
      readNumber(source, 'quotaHourlyUsed', 'quota_hourly_used'),
    ),
    limit: readNumber(
      businessCalls1h,
      'limit',
      'limit',
      readNumber(source, 'quotaHourlyLimit', 'quota_hourly_limit'),
    ),
    windowMinutes: readNumber(
      businessCalls1h,
      'windowMinutes',
      'window_minutes',
      60,
    ),
  }
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
    businessCalls1h: businessCalls1hSummary,
    dailyCreditsUsed,
    dailyCreditsLimit,
    monthlyCreditsUsed,
    monthlyCreditsLimit,
    dailySuccess: readNumber(
      source,
      'dailySuccess',
      'daily_success',
      readNumber(
        source,
        'dailyCreditsUsed',
        'daily_credits_used',
        readNumber(source, 'quotaDailyUsed', 'quota_daily_used'),
      ),
    ),
    dailyFailure: readNumber(source, 'dailyFailure', 'daily_failure'),
    monthlySuccess: readNumber(
      source,
      'monthlySuccess',
      'monthly_success',
      readNumber(
        source,
        'monthlyCreditsUsed',
        'monthly_credits_used',
        readNumber(source, 'quotaMonthlyUsed', 'quota_monthly_used'),
      ),
    ),
  }
}

export function normalizeUserTokenSummaryList(value: unknown): UserTokenSummary[] {
  const rawItems = Array.isArray(value) ? value : isRecordLike(value) && Array.isArray(value.items) ? value.items : []
  return rawItems.map(normalizeUserTokenSummary)
}
