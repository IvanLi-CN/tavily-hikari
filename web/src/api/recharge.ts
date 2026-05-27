import { requestJson } from './runtime'
import {
  normalizeRechargeConfig,
  normalizeRechargeOrder,
  normalizeRechargeOrderList,
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
  effectiveUntilMonthStart: number | null
}

export interface RechargeOrder {
  outTradeNo: string
  status: string
  credits: number
  months: number
  money: string
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

export async function createUserRechargeOrder(
  input: { credits: number; months: number },
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
