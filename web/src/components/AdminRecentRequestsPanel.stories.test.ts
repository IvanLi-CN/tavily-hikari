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
    expect(panelStories.RequestKindDesktopExpanded).toMatchObject({})
    expect(panelStories.RequestKindMobileDrawer).toMatchObject({})
    expect(panelStories.ProviderDimension).toMatchObject({})
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

  it('renders the desktop request-kind proof with the 2x2 filter structure copy', () => {
    const renderStory = panelStories.RequestKindDesktopExpanded.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )

    expect(markup).toContain('Request Type Desktop 2x2')
    expect(markup).toContain('请求类型')
    expect(markup).toContain('Desktop proof for the shared request-type grid')
    expect(markup).toContain('API | search')
    expect(markup).toContain('MCP | notifications/initialized')
  })

  it('keeps the mobile drawer proof available for button-style quick filters', () => {
    const renderStory = panelStories.RequestKindMobileDrawer.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()
  })

  it('renders the provider facet filter proof with the coalesced legacy Tavily row', () => {
    const renderStory = panelStories.ProviderDimension.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )

    expect(markup).toContain('提供方')
    expect(markup).toContain('全部提供方')
    expect(markup).toContain('Exa')
    expect(markup).toContain('Brave')
    // The legacy fixture has no provider field; it must render as Tavily.
    expect(markup).toContain('LegacyK')
    expect(markup).toContain('Tavily')
  })
})
