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
    expect(markup).not.toContain('Sign in with Linux DO')
  })

  it('exposes stale and degraded freshness proof stories', () => {
    expect(heroStories.MetricsStaleFallback.args?.freshness).toMatchObject({
      state: 'stale',
      reason: 'sqlite_contention',
    })
    expect(heroStories.MetricsColdStartDegraded.args?.freshness).toMatchObject({
      state: 'degraded',
      reason: 'cold_start_no_cache',
    })
    expect(heroStories.FreshnessGallery).toMatchObject({})
  })
})
