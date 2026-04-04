import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import meta, * as publicHomeStories from './PublicHome.stories'
import { LanguageProvider } from './i18n'
import { ThemeProvider } from './theme'

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
    expect(publicHomeStories.GuideTokenRevealed.parameters).toMatchObject({
      layout: 'fullscreen',
      viewport: { defaultViewport: '1440-device-desktop' },
    })
  })

  it('renders the public metrics hero with an explicit UTC monthly label', () => {
    const renderStory = meta.render as
      | ((args: typeof publicHomeStories.TokenModalOpen.args) => JSX.Element)
      | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        null,
        createElement(
          ThemeProvider,
          null,
          renderStory?.(publicHomeStories.TokenModalOpen.args ?? { showAdminAction: false }),
        ),
      ),
    )

    expect(markup).toContain('Monthly Success (UTC)')
    expect(markup).not.toContain('Today Success (UTC)')
  })
})
