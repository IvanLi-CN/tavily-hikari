import { describe, expect, it } from 'bun:test'

import meta, * as stories from './OAuthCallbackPanel.stories'

describe('OAuthCallbackPanel stories', () => {
  it('keeps the callback fragment storybook surface focused on review states', () => {
    expect(meta.parameters?.controls).toEqual({ disable: true })
    expect(stories.Gallery.name).toBe('State Gallery')
    expect(stories.TimeoutRecovery.args?.model).toMatchObject({
      tone: 'warning',
      showActions: true,
    })
    expect(stories.SuccessHandoff.args?.model).toMatchObject({
      tone: 'success',
      showActions: false,
    })
    expect(stories.DesktopTimeout.parameters?.viewport).toEqual({
      defaultViewport: 'desktop',
    })
    expect(stories.DesktopConnecting.parameters?.viewport).toEqual({
      defaultViewport: 'desktop',
    })
    expect(stories.MobileTimeout.parameters?.viewport).toEqual({
      defaultViewport: 'mobile1',
    })
    expect(stories.MobileSuccess.parameters?.viewport).toEqual({
      defaultViewport: 'mobile1',
    })
  })
})
