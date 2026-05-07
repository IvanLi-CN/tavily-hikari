import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import { LanguageProvider } from '../i18n'
import { ThemeProvider } from '../theme'
import { TooltipProvider } from './ui/tooltip'
import meta, * as panelStories from './AdminRecentRequestsPanel.stories'

describe('AdminRecentRequestsPanel Storybook proofs', () => {
  it('keeps the catalog loading, empty, and error state stories available', () => {
    expect(meta).toMatchObject({
      title: 'Admin/Components/AdminRecentRequestsPanel',
    })

    expect(panelStories.CatalogLoading).toMatchObject({})
    expect(panelStories.EmptyState).toMatchObject({})
    expect(panelStories.ErrorState).toMatchObject({})
  })

  it('renders the catalog loading story with the retention-safe fallback copy', () => {
    const renderStory = panelStories.CatalogLoading.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )

    expect(markup).toContain('按时间倒序浏览近期请求。')
    expect(markup).toContain('使用较新 / 较旧翻页浏览近期请求。')
    expect(markup).not.toContain('日志保留 32 天')
  })

  it('renders API rebalance rows with explicit marker and effect labels', () => {
    const renderStory = panelStories.RebalanceMarkers.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )

    expect(markup).toContain('LKoZ')
    expect(markup).toContain('pK9x')
    expect(markup).toContain('API绑定')
    expect(markup).toContain('API避高压')
    expect(markup).toContain('API Rebalance 路由')
    expect(markup).not.toContain('绑定已更新')
    expect(markup).not.toContain('选路已更新')
  })
})
