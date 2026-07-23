import { describe, expect, it } from 'bun:test'

import { buildShadowDailyUsageStack } from './userUsageComparison'
import { translations } from '../i18n'

describe('buildShadowDailyUsageStack', () => {
  const usersStrings = translations.zh.admin.users
  const formatNumber = (value: number) => new Intl.NumberFormat('zh-CN').format(value)
  const formatQuotaStackValue = (used: number, limit: number) => ({
    primary: `${formatNumber(used)} / ${formatNumber(limit)}`,
    secondary: null,
  })

  it('keeps confirmed equal values visible without a delta note', () => {
    const metric = buildShadowDailyUsageStack({
      actualUsed: 50,
      shadowUsed: 50,
      shadowAvailability: 'confirmed',
      limit: 100,
      usersStrings,
      formatNumber,
      formatQuotaStackValue,
    })

    expect(metric.primary).toBe('50 / 100')
    expect(metric.secondary).toBeNull()
  })

  it('shows projected copy when the value still includes unreconciled estimates', () => {
    const metric = buildShadowDailyUsageStack({
      actualUsed: 50,
      shadowUsed: 58,
      shadowAvailability: 'projected',
      limit: 100,
      usersStrings,
      formatNumber,
      formatQuotaStackValue,
    })

    expect(metric.primary).toBe('58 / 100')
    expect(metric.secondary).toBe('较当前 +8 · 含未对账估算')
  })

  it('keeps the delta note when the confirmed shadow value differs', () => {
    const metric = buildShadowDailyUsageStack({
      actualUsed: 50,
      shadowUsed: 58,
      shadowAvailability: 'confirmed',
      limit: 100,
      usersStrings,
      formatNumber,
      formatQuotaStackValue,
    })

    expect(metric.primary).toBe('58 / 100')
    expect(metric.secondary).toBe('较当前 +8')
  })

  it('keeps only the estimate note when a projected value matches the current value', () => {
    const metric = buildShadowDailyUsageStack({
      actualUsed: 50,
      shadowUsed: 50,
      shadowAvailability: 'projected',
      limit: 100,
      usersStrings,
      formatNumber,
      formatQuotaStackValue,
    })

    expect(metric.primary).toBe('50 / 100')
    expect(metric.secondary).toBe('含未对账估算')
  })
})
