import { describe, expect, it } from 'bun:test'

import meta, * as rollingNumberStories from './RollingNumber.stories'

describe('RollingNumber Storybook coverage', () => {
  it('exposes the suffix-only carry and borrow proof stories', () => {
    expect(meta).toMatchObject({
      title: 'Components/RollingNumber',
      tags: ['autodocs'],
    })

    expect(rollingNumberStories.Default).toMatchObject({})
    expect(rollingNumberStories.Loading).toMatchObject({})
    expect(rollingNumberStories.Empty).toMatchObject({})
    expect(rollingNumberStories.CarryChainSuffixOnly).toMatchObject({})
    expect(rollingNumberStories.BorrowChainSuffixOnly).toMatchObject({})
    expect(rollingNumberStories.EqualDigitFullCycle).toMatchObject({})
    expect(rollingNumberStories.GroupBoundaryJump).toMatchObject({})
  })
})
