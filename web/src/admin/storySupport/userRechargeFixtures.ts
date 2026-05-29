import type { AdminUserRechargeAudit } from '../../api'

function monthStartFor(nowSeconds: number): number {
  const now = new Date(nowSeconds * 1000)
  return Math.floor(new Date(now.getFullYear(), now.getMonth(), 1).getTime() / 1000)
}

function addMonths(monthStart: number, months: number): number {
  const date = new Date(monthStart * 1000)
  date.setMonth(date.getMonth() + months)
  return Math.floor(date.getTime() / 1000)
}

export function createStoryUserRechargeAudit(nowSeconds: number): AdminUserRechargeAudit {
  const monthStart = monthStartFor(nowSeconds)
  const paidAt = nowSeconds - 86_400 * 8 + 600
  const refundOnlyPaidAt = nowSeconds - 86_400 * 14 + 900
  return {
    currentMonthEntitlementCredits: 5_000,
    effectiveUntilMonthStart: addMonths(monthStart, 2),
    orders: [
      {
        outTradeNo: 'ldc_story_paid_001',
        status: 'paid',
        credits: 3_000,
        months: 3,
        money: '450.00',
        tradeNo: 'linuxdo_story_001',
        paymentUrl: null,
        createdAt: nowSeconds - 86_400 * 8,
        updatedAt: paidAt,
        paidAt,
        refundedAt: null,
        refundActor: null,
        lastNotifyAt: paidAt + 60,
        lastError: null,
      },
      {
        outTradeNo: 'ldc_story_only_002',
        status: 'refundOnly',
        credits: 2_000,
        months: 2,
        money: '200.00',
        tradeNo: 'linuxdo_story_002',
        paymentUrl: null,
        createdAt: nowSeconds - 86_400 * 14,
        updatedAt: nowSeconds - 86_400 * 2,
        paidAt: refundOnlyPaidAt,
        refundedAt: nowSeconds - 86_400 * 2,
        refundActor: 'story-admin',
        lastNotifyAt: refundOnlyPaidAt + 60,
        lastError: null,
      },
    ],
    entitlements: [
      { id: 1, outTradeNo: 'ldc_story_paid_001', monthStart, credits: 3_000, createdAt: paidAt },
      { id: 2, outTradeNo: 'ldc_story_paid_001', monthStart: addMonths(monthStart, 1), credits: 3_000, createdAt: paidAt },
      { id: 3, outTradeNo: 'ldc_story_paid_001', monthStart: addMonths(monthStart, 2), credits: 3_000, createdAt: paidAt },
      { id: 4, outTradeNo: 'ldc_story_only_002', monthStart, credits: 2_000, createdAt: refundOnlyPaidAt },
      { id: 5, outTradeNo: 'ldc_story_only_002', monthStart: addMonths(monthStart, 1), credits: 2_000, createdAt: refundOnlyPaidAt },
    ],
  }
}
