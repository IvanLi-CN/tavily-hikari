import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import { LanguageProvider } from '../i18n'
import { ThemeProvider } from '../theme'
import meta, * as heroStories from './PublicHomeHeroCard.stories'

describe('PublicHomeHeroCard Storybook proofs', () => {
  it('exports a stable authentication checking state for slow statistics', () => {
    expect(meta).toMatchObject({
      title: 'Public/PublicHomeHeroCard',
    })

    expect(heroStories.AuthStatusCheckingSlowStats.args).toMatchObject({
      metricsLoading: true,
      summaryLoading: true,
      showAuthStatusLoading: true,
    })
  })

  it('renders explicit authentication checking copy without resolved metrics', () => {
    const renderStory = meta.render as
      | ((args: typeof heroStories.AuthStatusCheckingSlowStats.args) => JSX.Element)
      | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        null,
        createElement(
          ThemeProvider,
          null,
          renderStory?.(heroStories.AuthStatusCheckingSlowStats.args ?? {}),
        ),
      ),
    )

    expect(markup).toContain('Checking sign-in and registration status')
    expect(markup).toContain('Checking sign-in')
    expect(markup).toContain('/assets/relay-mesh-lockup-light.png')
    expect(markup).toContain('/assets/relay-mesh-lockup-dark.png')
    expect(markup).not.toContain('Sign in with Linux DO')
  })

  it('renders the Linux DO login action with the primary button treatment', () => {
    const renderStory = meta.render as
      | ((args: typeof heroStories.LoggedOutNoToken.args) => JSX.Element)
      | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        null,
        createElement(
          ThemeProvider,
          null,
          renderStory?.(heroStories.LoggedOutNoToken.args ?? {}),
        ),
      ),
    )

    expect(markup).toContain('linuxdo-login-button')
    expect(markup).toContain('from-[#A78BFA] to-[#7C3AED]')
    expect(markup).toContain('text-primary-foreground')
    expect(markup).not.toContain('linuxdo-login-button h-auto rounded-full border-foreground/20')
  })
})
