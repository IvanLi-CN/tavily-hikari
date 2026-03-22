import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import meta, * as forwardProxyStories from './ForwardProxySettingsModule.stories'
import { LanguageProvider } from '../i18n'

describe('ForwardProxySettingsModule Storybook proofs', () => {
  it('keeps the progress card and status detail bubble stories available', () => {
    expect(meta).toMatchObject({
      title: 'Admin/ForwardProxySettingsModule',
    })

    expect(forwardProxyStories.RevalidateProgressBubble).toMatchObject({
      name: 'Revalidate Progress Card',
    })
    expect(forwardProxyStories.StatusDetailBubble).toMatchObject({
      name: 'Status Detail Bubble Proof',
    })
  })

  it('renders the status detail bubble proof without throwing outside Storybook runtime', () => {
    const renderStory = forwardProxyStories.StatusDetailBubble.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(LanguageProvider, null, renderStory?.()),
    )

    expect(markup).toContain('Status detail bubble proof')
  })
})
