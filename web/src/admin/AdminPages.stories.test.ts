import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'

import { LanguageProvider, translations } from '../i18n'
import { ThemeProvider } from '../theme'
import { TooltipProvider } from '../components/ui/tooltip'
import { renderToStaticMarkup } from 'react-dom/server'

import meta, * as adminPageStories from './AdminPages.stories'

describe('AdminPages Storybook proofs', () => {
  it('keeps the keys selected, sync-progress, and request stories available', () => {
    expect(meta).toMatchObject({
      title: 'Admin/Pages',
    })
    expect((meta as { decorators?: unknown }).decorators).toBeUndefined()

    expect(adminPageStories.KeysSelected).toMatchObject({})
    expect(adminPageStories.KeysSyncUsageInProgress).toMatchObject({})
    expect(adminPageStories.KeysSelectionRetainedAfterSync).toMatchObject({})
    expect(adminPageStories.KeysTemporaryIsolationFilter).toMatchObject({})
    expect(adminPageStories.Requests).toMatchObject({})
    expect(adminPageStories.Rankings).toMatchObject({})
    expect(adminPageStories.RankingsEmpty).toMatchObject({})
    expect(adminPageStories.RankingsLoading).toMatchObject({})
    expect(adminPageStories.RankingsMobile).toMatchObject({})
    expect(adminPageStories.Pressure).toMatchObject({})
    expect(adminPageStories.PressureMobile).toMatchObject({})
    expect(adminPageStories.RequestsResultFilterOpen).toMatchObject({})
    expect(adminPageStories.KeyDetailRecentRequests).toMatchObject({})
    expect(adminPageStories.TokenDetailRecentRequests).toMatchObject({})
    expect(adminPageStories.UserDetailSharedUsageTooltip).toMatchObject({})
    expect(adminPageStories.UserDetailCompact).toMatchObject({})
    expect(adminPageStories.UserDetailSingleTokenGuard).toMatchObject({})
    expect(adminPageStories.UserDetailBusinessCalls1h).toMatchObject({})
    expect(adminPageStories.Alerts).toMatchObject({})
    expect(adminPageStories.AlertsMobile).toMatchObject({})
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

  it('renders the rankings route story with the active nav icon and rankings shell', () => {
    const renderStory = adminPageStories.Rankings.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )

    expect(markup).toContain('用户排行')
    expect(markup).toContain('admin-nav-item-parent-active')
    expect(markup).toContain('admin-nav-subitem-active')
    expect(markup).toContain('admin-ranking-chart-shell')
  })

  it('renders the rankings dimension route story inside the same three-metric rankings shell', () => {
    const renderStory = adminPageStories.RankingsDimension.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )

    expect(markup).toContain('用户排行')
    expect(markup).toContain('admin-nav-item-parent-active')
    expect(markup).toContain('admin-nav-subitem-active')
    expect(markup).toContain('admin-ranking-chart-shell')
  })

  it('renders the rankings empty route story with the redesigned empty stage', () => {
    const renderStory = adminPageStories.RankingsEmpty.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )

    expect(markup).toContain('admin-ranking-empty-state')
    expect(markup).toContain('当前分组暂无可展示的用户数据。')
  })

  it('renders the rankings loading route story with live header copy and card-only skeletons', () => {
    const renderStory = adminPageStories.RankingsLoading.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )

    expect(markup).toContain('admin-ranking-skeleton-stage')
    expect(markup).toContain('admin-nav-item-parent-active')
    expect(markup).toContain('admin-nav-subitem-active')
  })

  it('renders the pressure route story with analysis nav active state and chart shells', () => {
    const renderStory = adminPageStories.Pressure.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )

    expect(markup).toContain('最近 24 小时服务器 1 小时窗口压力')
    expect(markup).toContain('当前 1 小时用户压力分布')
    expect(markup).toContain('最近 7 天服务器小时压力')
    expect(markup).toContain('admin-nav-subitem-active')
    expect(markup).toContain('pressure-analysis-page')
  })

  it('renders the jobs story with manual trigger controls and source labels', () => {
    const renderStory = adminPageStories.Jobs.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'en' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )

    expect(markup).toContain('Run')
    expect(markup).toContain('DB compaction is already running; using the active job.')
    expect(markup).toContain('DB compaction')
    expect(markup).toContain('Auto')
    expect(markup).toContain('Manual')
    expect(markup).toContain('Scheduled')
  })

  it('renders the jobs story in Chinese with the dedicated jobs action spacing hook', () => {
    const renderStory = adminPageStories.Jobs.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )

    expect(markup).toContain('admin-jobs-actions')
    expect(markup).toContain('访问令牌日志清理')
    expect(markup).not.toContain('mcp_sessions_gc')
    expect(markup).not.toContain('mcp_session_init_backoffs_gc')
  })

  it('keeps the tokens story shell chrome available', () => {
    const renderStory = adminPageStories.Tokens.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'en' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )

    const accessTokenHeadings = markup.match(/<h1[^>]*>Access Tokens<\/h1>/g) ?? []
    const panelAccessTokenHeadings = markup.match(/<h2[^>]*>Access Tokens<\/h2>/g) ?? []

    expect(accessTokenHeadings).toHaveLength(1)
    expect(panelAccessTokenHeadings).toHaveLength(0)
    expect(markup).toContain('View unbound token usage')
  })

  it('renders user tables with one sortable 7-day IP count column', () => {
    const renderUsersStory = adminPageStories.Users.render as (() => JSX.Element) | undefined
    const renderUsageStory = adminPageStories.UsersUsage.render as (() => JSX.Element) | undefined
    expect(renderUsersStory).toBeDefined()
    expect(renderUsageStory).toBeDefined()

    const renderMarkup = (renderStory: () => JSX.Element) =>
      renderToStaticMarkup(
        createElement(
          LanguageProvider,
          { initialLanguage: 'zh' },
          createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory))),
        ),
      )

    const usersMarkup = renderMarkup(renderUsersStory!)
    const usageMarkup = renderMarkup(renderUsageStory!)

    expect(usersMarkup).toContain('IP 数')
    expect(usersMarkup).toContain('data-sort-field="recentIpCount7d"')
    expect(usersMarkup).not.toContain('7天IP')
    expect(usageMarkup).toContain('IP 数')
    expect(usageMarkup).toContain('data-sort-field="recentIpCount7d"')
  })

  it('renders active-only user stories with the default filter hint and search fallback hint', () => {
    const renderUsersActiveOnly = adminPageStories.UsersActiveOnlyDefault.render as
      | (() => JSX.Element)
      | undefined
    const renderUsersSearchAll = adminPageStories.UsersActiveOnlySearchAll.render as
      | (() => JSX.Element)
      | undefined
    const renderUsageActiveOnly = adminPageStories.UsersUsageActiveOnlyDefault.render as
      | (() => JSX.Element)
      | undefined
    const renderUsageSearchAll = adminPageStories.UsersUsageActiveOnlySearchAll.render as
      | (() => JSX.Element)
      | undefined
    expect(renderUsersActiveOnly).toBeDefined()
    expect(renderUsersSearchAll).toBeDefined()
    expect(renderUsageActiveOnly).toBeDefined()
    expect(renderUsageSearchAll).toBeDefined()

    const renderMarkup = (renderStory: () => JSX.Element) =>
      renderToStaticMarkup(
        createElement(
          LanguageProvider,
          { initialLanguage: 'zh' },
          createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory))),
        ),
      )

    const usersActiveOnlyMarkup = renderMarkup(renderUsersActiveOnly!)
    const usersSearchAllMarkup = renderMarkup(renderUsersSearchAll!)
    const usageActiveOnlyMarkup = renderMarkup(renderUsageActiveOnly!)
    const usageSearchAllMarkup = renderMarkup(renderUsageSearchAll!)

    expect(usersActiveOnlyMarkup).toContain('默认仅展示近 90 天内调用过接口的活跃用户。')
    expect(usersActiveOnlyMarkup).not.toContain('Charlie Li')
    expect(usersSearchAllMarkup).toContain('搜索已扩展到全部用户集合，避免遗漏非活跃用户。')
    expect(usersSearchAllMarkup).toContain('Charlie Li')

    expect(usageActiveOnlyMarkup).toContain('默认仅展示近 90 天内调用过接口的活跃用户。')
    expect(usageActiveOnlyMarkup).not.toContain('Charlie Li')
    expect(usageActiveOnlyMarkup.indexOf('data-testid="users-filter-status"')).toBeGreaterThan(-1)
    expect(usageActiveOnlyMarkup.indexOf('<section class="surface panel">')).toBeGreaterThan(-1)
    expect(usageActiveOnlyMarkup.indexOf('data-testid="users-filter-status"')).toBeLessThan(
      usageActiveOnlyMarkup.indexOf('<section class="surface panel">'),
    )
    expect(usageSearchAllMarkup).toContain('搜索已扩展到全部用户集合，避免遗漏非活跃用户。')
    expect(usageSearchAllMarkup).toContain('Charlie Li')
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
    expect(markup).toContain('常规设置')
    expect(markup).toContain('高可用')
    expect(markup).toContain('admin-nav-item-active')
    expect(markup).toContain('admin-nav-item-icon')
    expect(markup).toContain('<svg')
    expect(markup).toContain('活跃用户 12 / 总用户 30')
    expect(markup).not.toContain(translations.zh.admin.systemSettings.ha.panelTitle)
  })

  it('renders the system settings HA page with node inventory and active child nav', () => {
    const renderStory = adminPageStories.SystemSettingsHa.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )

    expect(markup).toContain('高可用')
    expect(markup).toContain('admin-nav-subitem-active')
    expect(markup).toContain(translations.zh.admin.systemSettings.ha.panelTitle)
    expect(markup).toContain(translations.zh.admin.systemSettings.ha.nodeInventoryTitle)
    expect(markup).toContain(translations.zh.admin.systemSettings.ha.actionPlannedCutover)
    expect(markup).toContain(translations.zh.admin.systemSettings.ha.timelineTitle)
  })

  it('renders abnormal HA attention on dashboard without the full node panel', () => {
    const renderStory = adminPageStories.DashboardHaAttention.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )

    expect(markup).toContain('高可用状态需要关注')
    expect(markup).toContain('查看 HA 设置')
    expect(markup).not.toContain(translations.zh.admin.systemSettings.ha.nodeInventoryTitle)
    expect(markup).not.toContain(translations.zh.admin.systemSettings.ha.promoteToMaster)
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

  it('renders the user detail stories with business 1h summary and tab affordances', () => {
    const renderStory = adminPageStories.UserDetailBusinessCalls1h.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )

    expect(markup).toContain('业务 1h')
    expect(markup).toContain('36')
    expect(markup).toContain('34')
    expect(markup).toContain('2')
  })

  it('renders the user detail stories with add and delete token controls', () => {
    const renderStory = adminPageStories.UserDetail.render as (() => JSX.Element) | undefined
    const renderSingleStory = adminPageStories.UserDetailSingleTokenGuard.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()
    expect(renderSingleStory).toBeDefined()

    const renderMarkup = (renderFn: () => JSX.Element) =>
      renderToStaticMarkup(
        createElement(
          LanguageProvider,
          { initialLanguage: 'en' },
          createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderFn))),
        ),
      )

    const multiMarkup = renderMarkup(renderStory!)
    const singleMarkup = renderMarkup(renderSingleStory!)

    expect(multiMarkup).toContain('Add token')
    expect(multiMarkup).toContain('Delete token')
    expect(singleMarkup).toContain('Add token')
    expect(singleMarkup).toContain('At least one token must remain.')
  })
})
