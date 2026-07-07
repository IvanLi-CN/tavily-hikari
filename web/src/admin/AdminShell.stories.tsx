import type { Meta, StoryObj } from '@storybook/react-vite'
import { ChartColumnIncreasing } from 'lucide-react'
import { useState } from 'react'

import AdminCompactIntro from '../components/AdminCompactIntro'
import AdminPanelHeader from '../components/AdminPanelHeader'
import AdminReturnToConsoleLink from '../components/AdminReturnToConsoleLink'
import { AdminSidebarUtilityCard, AdminSidebarUtilityStack } from '../components/AdminSidebarUtility'
import LanguageSwitcher from '../components/LanguageSwitcher'
import ThemeToggle from '../components/ThemeToggle'
import TokenUsageHeader from '../components/TokenUsageHeader'
import SegmentedTabs from '../components/ui/SegmentedTabs'
import { Button } from '../components/ui/button'
import { translations, useLanguage, useTranslate, type AdminTranslations } from '../i18n'
import { Icon } from '../lib/icons'
import AdminShell, { AdminShellSidebarUtility, type AdminNavItem, type AdminNavTarget } from './AdminShell'

function navIcon(name: string): JSX.Element {
  return <Icon icon={name} width={18} height={18} />
}

function buildNavItems(admin: AdminTranslations): AdminNavItem[] {
  return [
    { target: 'dashboard', label: admin.nav.dashboard, icon: navIcon('mdi:view-dashboard-outline') },
    {
      target: 'analysis',
      label: admin.nav.analysis,
      icon: <ChartColumnIncreasing size={18} strokeWidth={2.2} />,
      children: [
        { target: 'analysis-rankings', label: admin.nav.rankings },
        { target: 'analysis-usage', label: admin.nav.usage },
        { target: 'analysis-pressure', label: admin.nav.pressure },
      ],
    },
    { target: 'tokens', label: admin.nav.tokens, icon: navIcon('mdi:key-chain-variant') },
    { target: 'keys', label: admin.nav.keys, icon: navIcon('mdi:key-outline') },
    { target: 'requests', label: admin.nav.requests, icon: navIcon('mdi:file-document-outline') },
    { target: 'jobs', label: admin.nav.jobs, icon: navIcon('mdi:calendar-clock-outline') },
    {
      target: 'users',
      label: admin.nav.users,
      icon: navIcon('mdi:account-group-outline'),
      children: [
        { target: 'users-list', label: admin.users.table.user },
        { target: 'user-tags', label: admin.users.table.tags },
      ],
    },
    { target: 'alerts', label: admin.nav.alerts, icon: navIcon('mdi:bell-ring-outline') },
    {
      target: 'system-settings',
      label: admin.nav.systemSettings,
      icon: navIcon('mdi:cog-outline'),
      children: [
        { target: 'system-settings', label: admin.systemSettings.subnav.general },
        { target: 'system-settings-admin', label: admin.systemSettings.subnav.admin },
        { target: 'system-settings-ha', label: admin.systemSettings.subnav.highAvailability },
        { target: 'proxy-settings', label: admin.nav.proxySettings },
      ],
    },
  ]
}

const DEFAULT_NAV_ITEMS = buildNavItems(translations.en.admin)

function LayoutBody(props: { title: string; description: string }): JSX.Element {
  return (
    <>
      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{props.title}</h2>
            <p className="panel-description">{props.description}</p>
          </div>
        </div>
        <div className="table-wrapper admin-responsive-up">
          <table className="jobs-table">
            <thead>
              <tr>
                <th>ID</th>
                <th>Type</th>
                <th>Status</th>
                <th>Updated</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>610</td>
                <td>Sync quota</td>
                <td>Success</td>
                <td>11:42:10</td>
              </tr>
              <tr>
                <td>609</td>
                <td>Usage rollups</td>
                <td>Running</td>
                <td>11:41:37</td>
              </tr>
            </tbody>
          </table>
        </div>
        <div className="admin-mobile-list admin-responsive-down">
          <article className="admin-mobile-card">
            <div className="admin-mobile-kv">
              <span>ID</span>
              <strong>610</strong>
            </div>
            <div className="admin-mobile-kv">
              <span>Type</span>
              <strong>Sync quota</strong>
            </div>
            <div className="admin-mobile-kv">
              <span>Status</span>
              <strong>Success</strong>
            </div>
          </article>
          <article className="admin-mobile-card">
            <div className="admin-mobile-kv">
              <span>ID</span>
              <strong>609</strong>
            </div>
            <div className="admin-mobile-kv">
              <span>Type</span>
              <strong>Usage rollups</strong>
            </div>
            <div className="admin-mobile-kv">
              <span>Status</span>
              <strong>Running</strong>
            </div>
          </article>
        </div>
      </section>
    </>
  )
}

function PanelHeaderLayoutStory(): JSX.Element {
  const { language } = useLanguage()
  const admin = useTranslate().admin
  const [activeModule, setActiveModule] = useState<AdminNavTarget>('jobs')
  const navItems = buildNavItems(admin)
  const displayName = language === 'zh' ? 'Ivan Li' : 'Ops Admin'
  const layoutBody = language === 'zh'
    ? { title: '任务作业', description: '用于验证 shell 与页头响应式布局的固定数据。' }
    : { title: 'Scheduled Jobs', description: 'Responsive layout fixture for shell and header verification.' }

  return (
    <AdminShell
      activeItem={activeModule}
      navItems={navItems}
      skipToContentLabel={language === 'zh' ? '跳到主内容' : 'Skip to main content'}
      onSelectItem={setActiveModule}
    >
      <AdminShellSidebarUtility>
        <AdminSidebarUtilityStack>
          <AdminSidebarUtilityCard>
            <div className="admin-sidebar-utility-toolbar">
              <ThemeToggle />
              <LanguageSwitcher />
            </div>
            <div className="admin-sidebar-utility-meta">
              <div className="user-badge user-badge-admin">
                <Icon icon="mdi:crown-outline" className="user-badge-icon" aria-hidden="true" />
                <span>{displayName}</span>
              </div>
              <span className="admin-panel-header-time" aria-live="polite">
                <Icon icon="mdi:clock-time-four-outline" width={14} height={14} className="admin-panel-header-time-icon" aria-hidden="true" />
                <span className="admin-panel-header-time-label">{admin.header.updatedPrefix}</span>
                <span className="admin-panel-header-time-value">11:42:10</span>
              </span>
            </div>
          </AdminSidebarUtilityCard>
          <AdminSidebarUtilityCard>
            <div className="admin-sidebar-utility-actions">
              <AdminReturnToConsoleLink
                label={admin.header.returnToConsole}
                href="/console"
                className="admin-sidebar-utility-action"
              />
              <Button type="button" variant="outline" size="sm" className="admin-panel-refresh-button admin-sidebar-utility-action">
                <Icon icon="mdi:refresh" width={16} height={16} aria-hidden="true" />
                <span>{admin.header.refreshNow}</span>
              </Button>
            </div>
          </AdminSidebarUtilityCard>
        </AdminSidebarUtilityStack>
      </AdminShellSidebarUtility>

      <div className="admin-stacked-only">
        <AdminPanelHeader
          title={admin.header.title}
          subtitle={admin.header.subtitle}
          displayName={displayName}
          isAdmin
          updatedPrefix={admin.header.updatedPrefix}
          updatedTime="11:42:10"
          isRefreshing={false}
          refreshLabel={admin.header.refreshNow}
          refreshingLabel={admin.header.refreshing}
          userConsoleLabel={admin.header.returnToConsole}
          userConsoleHref="/console"
          onRefresh={() => undefined}
        />
      </div>
      <div className="admin-desktop-only">
        <AdminCompactIntro
          title={admin.header.title}
          description={admin.header.subtitle}
        />
      </div>
      <LayoutBody title={layoutBody.title} description={layoutBody.description} />
    </AdminShell>
  )
}

function TokenUsageLayoutStory(): JSX.Element {
  const { language } = useLanguage()
  const admin = useTranslate().admin
  const [activeModule, setActiveModule] = useState<AdminNavTarget>('tokens')
  const [period, setPeriod] = useState<'day' | 'month' | 'all'>('day')
  const [focus, setFocus] = useState<'usage' | 'errors' | 'other'>('usage')
  const navItems = buildNavItems(admin)
  const copy = language === 'zh'
    ? {
        title: '访问令牌用量排行',
        subtitle: '按周期比较用量、错误与异常信号。',
        back: '返回',
        bodyTitle: '访问令牌排行',
        bodyDescription: '使用视口工具验证移动端顶部布局行为。',
        periods: ['今日', '本月', '全部'],
        focuses: ['用量', '错误', '其他'],
      }
    : {
        title: 'Token Usage Leaderboard',
        subtitle: 'Compare usage, errors, and anomaly signals by period.',
        back: 'Back',
        bodyTitle: 'Top Tokens',
        bodyDescription: 'Use viewport toolbar to verify the mobile top layout behavior.',
        periods: ['Today', 'Month', 'All time'],
        focuses: ['Usage', 'Errors', 'Other'],
      }

  return (
    <AdminShell
      activeItem={activeModule}
      navItems={navItems}
      skipToContentLabel={language === 'zh' ? '跳到主内容' : 'Skip to main content'}
      onSelectItem={setActiveModule}
    >
      <AdminShellSidebarUtility>
        <AdminSidebarUtilityStack>
          <AdminSidebarUtilityCard>
            <div className="admin-sidebar-utility-toolbar">
              <ThemeToggle />
            </div>
            <div className="admin-sidebar-utility-actions">
              <AdminReturnToConsoleLink
                label={admin.header.returnToConsole}
                href="/console"
                className="admin-sidebar-utility-action"
              />
              <Button type="button" variant="ghost" size="sm" className="token-usage-back-button admin-sidebar-utility-action" onClick={() => setActiveModule('tokens')}>
                <Icon icon="mdi:arrow-left" width={16} height={16} aria-hidden="true" />
                <span>{copy.back}</span>
              </Button>
              <Button type="button" variant="outline" size="sm" className="token-usage-refresh-button admin-sidebar-utility-action">
                <Icon icon="mdi:refresh" width={16} height={16} aria-hidden="true" />
                <span>{admin.header.refreshNow}</span>
              </Button>
            </div>
          </AdminSidebarUtilityCard>
        </AdminSidebarUtilityStack>
      </AdminShellSidebarUtility>

      <div className="admin-stacked-only">
        <TokenUsageHeader
          title={copy.title}
          subtitle={copy.subtitle}
          visualPreset="accent"
          backLabel={copy.back}
          refreshLabel={admin.header.refreshNow}
          refreshingLabel={admin.header.refreshing}
          userConsoleLabel={admin.header.returnToConsole}
          userConsoleHref="/console"
          isRefreshing={false}
          period={period}
          focus={focus}
          periodOptions={[
            { value: 'day', label: copy.periods[0] },
            { value: 'month', label: copy.periods[1] },
            { value: 'all', label: copy.periods[2] },
          ]}
          focusOptions={[
            { value: 'usage', label: copy.focuses[0] },
            { value: 'errors', label: copy.focuses[1] },
            { value: 'other', label: copy.focuses[2] },
          ]}
          onBack={() => setActiveModule('tokens')}
          onRefresh={() => undefined}
          onPeriodChange={setPeriod}
          onFocusChange={setFocus}
        />
      </div>
      <div className="admin-desktop-only" style={{ display: 'grid', gap: 14 }}>
        <AdminCompactIntro
          title={copy.title}
          description={copy.subtitle}
        />
        <div className="surface panel" style={{ padding: 14 }}>
          <div className="token-usage-header-filters">
            <SegmentedTabs<'day' | 'month' | 'all'>
              className="token-usage-segmented"
              value={period}
              onChange={setPeriod}
              options={[
                { value: 'day', label: copy.periods[0] },
                { value: 'month', label: copy.periods[1] },
                { value: 'all', label: copy.periods[2] },
              ]}
              ariaLabel="Token leaderboard period"
            />
            <SegmentedTabs<'usage' | 'errors' | 'other'>
              className="token-usage-segmented"
              value={focus}
              onChange={setFocus}
              options={[
                { value: 'usage', label: copy.focuses[0] },
                { value: 'errors', label: copy.focuses[1] },
                { value: 'other', label: copy.focuses[2] },
              ]}
              ariaLabel="Token leaderboard focus"
            />
          </div>
        </div>
      </div>
      <LayoutBody title={copy.bodyTitle} description={copy.bodyDescription} />
    </AdminShell>
  )
}

const meta = {
  title: 'Admin/AdminShell',
  parameters: {
    layout: 'fullscreen',
    docs: {
      description: {
        component:
          'Admin shell primitive that owns skip-link, responsive sidebar, stacked mobile menu, and the main content frame used by admin pages.',
      },
    },
  },
  component: AdminShell,
  tags: ['autodocs'],
  args: {
    activeItem: 'dashboard',
    navItems: DEFAULT_NAV_ITEMS,
    skipToContentLabel: 'Skip to main content',
    onSelectItem: () => undefined,
  },
} satisfies Meta<typeof AdminShell>

export default meta

type Story = StoryObj<typeof meta>

export const PanelHeaderShell: Story = {
  render: () => <PanelHeaderLayoutStory />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 50))
    const root = canvasElement.ownerDocument
    const utility = root.querySelector<HTMLElement>('.admin-sidebar-utility')
    const intro = root.querySelector<HTMLElement>('.admin-compact-intro')
    const stackedHeader = root.querySelector<HTMLElement>('.admin-panel-header')

    if (!utility || !intro || !stackedHeader) {
      throw new Error('Expected sidebar utility, compact intro, and stacked header fixtures to render.')
    }

    const toolbar = utility.querySelector<HTMLElement>('.admin-sidebar-utility-toolbar')
    const themeTrigger = toolbar?.querySelector<HTMLElement>('.theme-toggle-trigger')
    const languageTrigger = toolbar?.querySelector<HTMLElement>('.language-switcher-trigger')

    if (!toolbar || !themeTrigger || !languageTrigger) {
      throw new Error('Expected sidebar utility theme and language controls to render.')
    }

    const themeRect = themeTrigger.getBoundingClientRect()
    const languageRect = languageTrigger.getBoundingClientRect()
    if (Math.abs(themeRect.top - languageRect.top) > 1 || Math.abs(themeRect.height - languageRect.height) > 1) {
      throw new Error('Expected sidebar utility theme and language controls to share one row.')
    }
  },
}

export const TokenUsageShell: Story = {
  render: () => <TokenUsageLayoutStory />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const PanelHeaderShellStacked: Story = {
  render: () => <PanelHeaderLayoutStory />,
  parameters: {
    viewport: { defaultViewport: '1100-breakpoint-admin-stack-max' },
  },
}
