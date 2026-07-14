import '../../test/happydom'

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
    expect(adminPageStories.UserDetailAccountTab).toMatchObject({})
    expect(adminPageStories.UserDetailIdentityTab).toMatchObject({})
    expect(adminPageStories.UserDetailSingleTokenGuard).toMatchObject({})
    expect(adminPageStories.UserDetailTagsTab).toMatchObject({})
    expect(adminPageStories.UserDetailQuotaTab).toMatchObject({})
    expect(adminPageStories.UserDetailTokensTab).toMatchObject({})
    expect(adminPageStories.UserDetailBusinessCalls1h).toMatchObject({})
    expect(adminPageStories.Alerts).toMatchObject({})
    expect(adminPageStories.AlertsMobile).toMatchObject({})
    expect(adminPageStories.Recharges).toMatchObject({})
    expect(adminPageStories.SystemSettingsStatus).toMatchObject({})
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
    expect(markup).toContain('当前 1 小时活跃用户压力分布曲线')
    expect(markup).toContain('用户数')
    expect(markup).toContain('最近 7 天服务器小时压力')
    expect(markup).toContain('admin-nav-subitem-active')
    expect(markup).toContain('pressure-analysis-page')
  })

  it('renders the recharge page story with full-width filters and recharge navigation', () => {
    const renderStory = adminPageStories.Recharges.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )

    expect(markup).toContain('充值记录')
    expect(markup).toContain('admin-recharge-filter-row')
    expect(markup).toContain('admin-nav-item-active')
    expect(markup).toContain('admin-recharge-table')
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
    expect(usageMarkup).toContain('data-sort-field="businessCalls1hUsed"')
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
    expect(markup).toContain('管理员')
    expect(markup).toContain('高可用')
    expect(markup).toContain('admin-nav-item-active')
    expect(markup).toContain('admin-nav-item-icon')
    expect(markup).toContain('<svg')
    expect(markup).toContain('活跃用户 12 / 总用户 30')
    expect(markup).not.toContain(translations.zh.admin.systemSettings.ha.panelTitle)
  })

  it('renders the system settings admin child nav item as active', () => {
    const renderStory = adminPageStories.SystemSettingsAdmin.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )

    expect(markup).toContain('管理员')
    expect(markup).toContain('admin-nav-subitem-active')
    expect(markup).toContain('安全状态')
    expect(markup).toContain('当前至少保留两种管理员登录方式。')
    expect(markup).toContain('管理员密码')
    expect(markup).toContain('Passkey 管理')
    expect(markup).toContain('管理端 TOTP')
  })

  it('renders the system settings status child nav item as active', () => {
    const renderStory = adminPageStories.SystemSettingsStatus.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(ThemeProvider, null, createElement(TooltipProvider, null, createElement(renderStory!))),
      ),
    )

    expect(markup).toContain('系统状态')
    expect(markup).toContain('admin-nav-subitem-active')
    expect(markup).toContain('出站 Header 白名单')
    expect(markup).toContain('X-Project-ID')
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

  it('renders the user detail story with compact card fallbacks for tokens', () => {
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
    expect(markup).toContain('admin-user-mobile-metric-grid')
    expect(markup).toContain('累计请求')
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

    expect(markup).toContain('每小时')
    expect(markup).toContain('36')
    expect(markup).toContain('34')
    expect(markup).toContain('2')
    expect(markup).toContain('上限')
  })

  it('renders the user detail stories with add and delete token controls', () => {
    const renderStory = adminPageStories.UserDetailTokensTab.render as (() => JSX.Element) | undefined
    const renderQuotaStory = adminPageStories.UserDetailQuotaTab.render as (() => JSX.Element) | undefined
    const renderSingleStory = adminPageStories.UserDetailSingleTokenGuard.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()
    expect(renderQuotaStory).toBeDefined()
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
    const quotaMarkup = renderMarkup(renderQuotaStory!)
    const singleMarkup = renderMarkup(renderSingleStory!)

    expect(multiMarkup).toContain('Add token')
    expect(multiMarkup).toContain('Delete token')
    expect(quotaMarkup).toContain('Account entitlement ledger')
    expect(quotaMarkup).toContain('Add ledger row')
    expect(quotaMarkup).toContain('Story base quota migration row.')
    expect(quotaMarkup).toContain('Manual monthly quota correction for story coverage.')
    expect(quotaMarkup).toContain('Permanent entitlement trim for story coverage.')
    expect(singleMarkup).toContain('Add token')
    expect(singleMarkup).toContain('At least one token must remain.')
  })
})
