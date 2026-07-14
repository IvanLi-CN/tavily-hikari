import { describe, expect, it } from 'bun:test'

import { getRechargeMonthUsedQuota } from './rechargeQuotaCalendarUsage.ts'

describe('getRechargeMonthUsedQuota', () => {
  it('only shows used quota on the current billing month row', () => {
    const currentMonthStart = 1_782_835_200

    expect(getRechargeMonthUsedQuota(currentMonthStart, currentMonthStart, 988)).toBe(988)
    expect(getRechargeMonthUsedQuota(currentMonthStart - 2_592_000, currentMonthStart, 988)).toBe(0)
    expect(getRechargeMonthUsedQuota(currentMonthStart + 2_678_400, currentMonthStart, 988)).toBe(0)
  })

  it('falls back to zero when the current month is unknown', () => {
    expect(getRechargeMonthUsedQuota(1_782_835_200, null, 988)).toBe(0)
  })
})
