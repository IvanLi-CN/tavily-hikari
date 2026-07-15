import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import meta, * as systemStatusStories from './UpstreamPrivacyStatusModule.stories'
import UpstreamPrivacyStatusModule from './UpstreamPrivacyStatusModule'
import { translations } from '../i18n'

describe('SystemStatusModule Storybook proofs', () => {
  it('keeps the pending, draining, active, degraded, empty, error, and gallery stories available', () => {
    expect(meta).toMatchObject({
      title: 'Admin/Modules/SystemStatusModule',
    })

    expect(systemStatusStories.Pending).toMatchObject({})
    expect(systemStatusStories.Draining).toMatchObject({})
    expect(systemStatusStories.Active).toMatchObject({})
    expect(systemStatusStories.Degraded).toMatchObject({})
    expect(systemStatusStories.EmptyState).toMatchObject({})
    expect(systemStatusStories.ErrorState).toMatchObject({})
    expect(systemStatusStories.LoadingState).toMatchObject({})
    expect(systemStatusStories.Mobile).toMatchObject({})
    expect(systemStatusStories.Gallery).toMatchObject({})
    expect(systemStatusStories.InteractionContract).toMatchObject({})
  })

  it('renders the base module without a duplicate route title and keeps the auto-refresh label wiring', () => {
    const markup = renderToStaticMarkup(createElement(UpstreamPrivacyStatusModule, meta.args))

    expect(markup).not.toContain('<h2>系统状态</h2>')
    expect(markup).toContain('自动刷新')
    expect(markup).toContain('aria-labelledby')
    expect(markup).toContain('需要关注')
  })

  it('renders the gallery story with the state matrix and error fallback', () => {
    const renderStory = systemStatusStories.Gallery.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(createElement(renderStory!))
    expect(markup).toContain('Pending')
    expect(markup).toContain('Degraded')
    expect(markup).toContain(translations.zh.admin.systemSettings.privacy.loadFailed)
  })
})
