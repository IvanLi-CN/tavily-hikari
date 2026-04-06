import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'

import { LanguageProvider } from '../i18n'
import { ThemeProvider } from '../theme'
import { renderToStaticMarkup } from 'react-dom/server'

import meta, * as adminPageStories from './AdminPages.stories'

describe('AdminPages Storybook proofs', () => {
  it('keeps the keys selected and sync-progress stories available', () => {
    expect(meta).toMatchObject({
      title: 'Admin/Pages',
    })

    expect(adminPageStories.KeysSelected).toMatchObject({})
    expect(adminPageStories.KeysSyncUsageInProgress).toMatchObject({})
    expect(adminPageStories.KeysSelectionRetainedAfterSync).toMatchObject({})
  })

  it('renders the sync-progress story with the progress bubble copy', () => {
    const renderStory = adminPageStories.KeysSyncUsageInProgress.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(renderStory!)),
      ),
    )
    expect(markup).toContain('同步额度进度')
    expect(markup).toContain('已处理 5/6')
    expect(markup).toContain('最近结果')
  })

  it('renders the retained-selection story with completion feedback', () => {
    const renderStory = adminPageStories.KeysSelectionRetainedAfterSync.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(renderStory!)),
      ),
    )

    expect(markup).toContain('同步额度完成：列表已刷新，仍在当前页中的 2 个密钥继续保持勾选。')
    expect(markup).toContain('已选 2 项')
  })
})
