import { requestJson } from './runtime'
import {
  normalizeRechargeConfig,
  normalizeRechargeOrder,
  normalizeRechargeOrderList,
  normalizeRechargeQuote,
} from './userConsoleNormalization'

export interface RechargeConfig {
  visible: boolean
  enabled: boolean
  unitCredits: number
  unitPriceLdc: number
  minCredits: number
  maxCredits: number
  creditsStep: number
  defaultCredits: number
  minMonths: number
  maxMonths: number
  quotaDeltaBaseCredits: number
  hourlyDeltaPerQuotaUnit: number
  dailyDeltaPerQuotaUnit: number
  monthlyDeltaPerQuotaUnit: number
  testPriceEnabled: boolean
  currentMonthStart: number
  currentEntitlementCredits: number
  currentEntitlementHourlyDelta: number
  currentEntitlementDailyDelta: number
  currentEntitlementMonthlyDelta: number
  effectiveUntilMonthStart: number | null
}

export interface RechargeQuoteMonth {
  monthIndex: number
  monthStart: number
  isCurrentMonth: boolean
  hourlyDelta: number
  dailyDelta: number
  monthlyDelta: number
  fullMonthlyDelta: number
  monthMoneyCents: number
  monthDiscountCents: number
  monthEndClampApplied: boolean
  discountReason: string | null
}

export interface RechargeQuote {
  requestedCredits: number
  requestedMonths: number
  quoteMonthStart: number
  remainingDaysInclusive: number
  unitCredits: number
  unitPriceCents: number
  fullMonthHourlyDelta: number
  fullMonthDailyDelta: number
  fullMonthMonthlyDelta: number
  fullMonthMoneyCents: number
  currentMonthFinalHourlyDelta: number
  currentMonthFinalDailyDelta: number
  currentMonthFinalMonthlyDelta: number
  currentMonthFinalMoneyCents: number
  fullOrderMoneyCents: number
  finalOrderMoneyCents: number
  monthEndClampApplied: boolean
  orderName: string
  schedule: RechargeQuoteMonth[]
}

export interface RechargeOrder {
  outTradeNo: string
  status: string
  credits: number
  months: number
  money: string
  quoteMonthStart: number
  finalMoneyCents: number
  finalHourlyDelta: number
  finalDailyDelta: number
  finalMonthlyDelta: number
  monthEndClampApplied: boolean
  tradeNo: string | null
  paymentUrl: string | null
  createdAt: number
  updatedAt: number
  paidAt: number | null
  lastNotifyAt: number | null
  lastError: string | null
}

export function fetchUserRechargeConfig(signal?: AbortSignal): Promise<RechargeConfig> {
  return requestJson<unknown>('/api/user/recharge/config', { signal }).then(normalizeRechargeConfig)
}

export function fetchUserRechargeOrders(signal?: AbortSignal): Promise<RechargeOrder[]> {
  return requestJson<unknown>('/api/user/recharge/orders', { signal }).then(normalizeRechargeOrderList)
}

export function fetchUserRechargeOrder(outTradeNo: string, signal?: AbortSignal): Promise<RechargeOrder> {
  return requestJson<unknown>(`/api/user/recharge/orders/${encodeURIComponent(outTradeNo)}`, { signal })
    .then(normalizeRechargeOrder)
}

export function fetchUserRechargeQuote(
  input: { credits: number; months: number },
  signal?: AbortSignal,
): Promise<RechargeQuote> {
  return requestJson<unknown>('/api/user/recharge/quote', {
    method: 'POST',
    signal,
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(input),
  }).then(normalizeRechargeQuote)
}

export async function createUserRechargeOrder(
  input: { credits: number; months: number; quote: RechargeQuote },
  signal?: AbortSignal,
): Promise<{ order: RechargeOrder; paymentUrl: string }> {
  const response = await requestJson<{ order: unknown; paymentUrl: string }>('/api/user/recharge/orders', {
    method: 'POST',
    signal,
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(input),
  })
  return {
    order: normalizeRechargeOrder(response.order),
    paymentUrl: response.paymentUrl,
  }
}
