import { describe, expect, it } from 'bun:test'

import meta, * as stories from './BillingPage.stories'

describe('BillingPage stories', () => {
  it('keeps the acceptance-facing billing route states exported', () => {
    expect(meta.title).toBe('User Console/Billing/Billing Page')
    expect(stories.Default.args).toBeUndefined()
    expect(stories.LifecycleStates.args).toBeUndefined()
    expect(stories.LifecycleStates.play).toBeFunction()
    expect(stories.NoOrdersNoFuture.args).toMatchObject({
      orders: [],
    })
    expect(stories.RechargeDisabled.args).toMatchObject({
      config: {
        enabled: false,
      },
    })
    expect(stories.Loading.args).toMatchObject({
      summary: null,
      loading: true,
    })
    expect(stories.Mobile.parameters).toMatchObject({
      viewport: { defaultViewport: '0390-device-iphone-14' },
    })
  })
})
