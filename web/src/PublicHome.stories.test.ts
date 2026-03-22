import { describe, expect, it } from 'bun:test'

import meta, * as publicHomeStories from './PublicHome.stories'

describe('PublicHome Storybook proofs', () => {
  it('keeps the page stories and mobile guide menu proof export available', () => {
    expect(meta).toMatchObject({
      title: 'Public/PublicHome',
    })

    expect(publicHomeStories.TokenModalOpen.args).toEqual({
      showAdminAction: false,
    })
    expect(publicHomeStories.TokenModalOpenWithAdminAction.args).toEqual({
      showAdminAction: true,
    })
    expect(publicHomeStories.MobileGuideMenuProof.parameters).toMatchObject({
      layout: 'padded',
      viewport: { defaultViewport: '0390-device-iphone-14' },
    })
  })
})
