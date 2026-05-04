import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'

import { LanguageProvider } from '../i18n'
import { ThemeProvider } from '../theme'
import { TooltipProvider } from '../components/ui/tooltip'
import { renderToStaticMarkup } from 'react-dom/server'

import meta, * as adminPageStories from './AdminPages.stories'

describe('AdminPages Storybook proofs', () => {
  it('keeps the keys selected, sync-progress, and request stories available', () => {
    expect(meta).toMatchObject({
      title: 'Admin/Pages',
    })

    expect(adminPageStories.KeysSelected).toMatchObject({})
    expect(adminPageStories.KeysSyncUsageInProgress).toMatchObject({})
    expect(adminPageStories.KeysSelectionRetainedAfterSync).toMatchObject({})
    expect(adminPageStories.KeysTemporaryIsolationFilter).toMatchObject({})
    expect(adminPageStories.Requests).toMatchObject({})
    expect(adminPageStories.RequestsResultFilterOpen).toMatchObject({})
    expect(adminPageStories.KeyDetailRecentRequests).toMatchObject({})
    expect(adminPageStories.TokenDetailRecentRequests).toMatchObject({})
    expect(adminPageStories.UserDetailSharedUsageTooltip).toMatchObject({})
    expect(adminPageStories.UserDetailCompact).toMatchObject({})
  })

  it('renders the sync-progress story with the progress bubble copy', () => {
    const renderStory = adminPageStories.KeysSyncUsageInProgress.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
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
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )

    expect(markup).toContain('同步额度完成：列表已刷新，仍在当前页中的 2 个密钥继续保持勾选。')
    expect(markup).toContain('已选 2 项')
  })

  it('renders the temporary isolation filter story with the filtered badge and count', () => {
    const renderStory = adminPageStories.KeysTemporaryIsolationFilter.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )

    expect(markup).toContain('临时隔离')
    expect(markup).toContain('状态: 临时隔离')
    expect(markup).toContain('U2vK')
    expect(markup).not.toContain('MZli')
  })

  it('renders the requests page story with retention-based copy instead of page-count copy', () => {
    const renderStory = adminPageStories.Requests.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )
    expect(markup).toContain('按时间倒序浏览近期请求。日志保留 32 天。')
    expect(markup).toContain('使用较新 / 较旧翻页浏览近 32 天内保留的请求。')
    expect(markup).toContain('限额')
    expect(markup).not.toContain('额度耗尽')
    expect(markup).not.toContain('200 条')
    expect(markup).not.toContain('10 页')
  })

  it('renders the system settings page story with a bundled navigation icon', () => {
    const renderStory = adminPageStories.SystemSettings.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )

    expect(markup).toContain('系统设置')
    expect(markup).toContain('admin-nav-item-active')
    expect(markup).toContain('admin-nav-item-icon')
    expect(markup).toContain('<svg')
  })

  it('renders the user detail story with compact card fallbacks for tokens and quota breakdown', () => {
    const renderStory = adminPageStories.UserDetailCompact.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )

    expect(markup).toContain('admin-user-token-card')
    expect(markup).toContain('admin-user-breakdown-card')
    expect(markup).toContain('admin-user-mobile-metric-grid')
    expect(markup).toContain('admin-user-mobile-chip')
    expect(markup).toContain('累计请求')
    expect(markup).toContain('最终有效额度')
  })
})
