import { requestJson } from './runtime'
import { normalizeUserBillingSummary } from './userConsoleNormalization'

export interface BillingQuota {
  hourly: number
  daily: number
  monthly: number
}

export interface UserBillingRecharge {
  credits: number
  quota: BillingQuota
}

export interface UserBillingComposition {
  baseAccess: BillingQuota
  tagAdjustments: BillingQuota
  permanentEntitlements: BillingQuota
  monthlyAdjustments: BillingQuota
  recharge: UserBillingRecharge
}

export interface UserBillingMonth {
  monthStart: number
  isCurrentMonth: boolean
  persistentTotal: BillingQuota
  monthlyAdjustments: BillingQuota
  recharge: UserBillingRecharge
  effectiveTotal: BillingQuota
}

export interface UserBillingSummary {
  currentMonthStart: number
  effectiveUntilMonthStart: number | null
  blockAll: boolean
  currentTotal: BillingQuota
  composition: UserBillingComposition
  timeline: UserBillingMonth[]
}

export function fetchUserBillingSummary(signal?: AbortSignal): Promise<UserBillingSummary> {
  return requestJson<unknown>('/api/user/billing/summary', { signal }).then(normalizeUserBillingSummary)
}
