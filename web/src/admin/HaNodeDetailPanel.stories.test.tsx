import '../../test/happydom'

import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import meta, * as stories from './HaNodeDetailPanel.stories'
import { LanguageProvider, translations } from '../i18n'
import { ThemeProvider } from '../theme'

describe('HaNodeDetailPanel Storybook proofs', () => {
  it('keeps the node detail story available', () => {
    expect(meta).toMatchObject({
      title: 'Admin/HaNodeDetailPanel',
    })
    expect(stories.Default).toBeDefined()
  })

  it('renders the source configuration entry inside the EdgeOne detail card', () => {
    const renderStory = meta.render as ((args: typeof stories.Default.args) => JSX.Element) | undefined

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(
          ThemeProvider,
          null,
          renderStory
            ? renderStory({
              ...(meta.args ?? {}),
              ...(stories.Default.args ?? {}),
            })
            : createElement((meta.component ?? (() => null)) as never, {
              ...(meta.args ?? {}),
              ...(stories.Default.args ?? {}),
            }),
        ),
      ),
    )

    expect(markup).toContain(translations.zh.admin.systemSettings.ha.nodeDetailEdgeoneTitle)
    expect(markup).toContain(translations.zh.admin.systemSettings.ha.configureSource)
    expect(markup).toContain('api.example.com')
  })
})
