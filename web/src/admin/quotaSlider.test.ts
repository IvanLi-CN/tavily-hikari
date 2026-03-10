import { describe, expect, it } from 'bun:test'

import {
  createQuotaSliderSeed,
  getQuotaSliderStagePosition,
  getQuotaSliderStageValue,
  parseQuotaDraftValue,
} from './quotaSlider'

describe('quota slider zero support', () => {
  it('keeps zero draft values instead of coercing them back to one', () => {
    expect(parseQuotaDraftValue('0', 12)).toBe(0)
    expect(parseQuotaDraftValue(undefined, 0)).toBe(0)
  })

  it('includes a zero stage for zero baseline quotas', () => {
    const seed = createQuotaSliderSeed('hourlyLimit', 0, 0)
    expect(seed.initialLimit).toBe(0)
    expect(seed.stages[0]).toBe(0)
    expect(getQuotaSliderStageValue(seed.stages, 0)).toBe(0)
    expect(getQuotaSliderStagePosition(seed.stages, 0)).toBe(0)
  })
})
