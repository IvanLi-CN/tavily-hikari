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
import { Icon } from '../lib/icons'
import AdminShell, { AdminShellSidebarUtility, type AdminNavItem, type AdminNavTarget } from './AdminShell'

function navIcon(name: string): JSX.Element {
  return <Icon icon={name} width={18} height={18} />
}

const NAV_ITEMS: AdminNavItem[] = [
  { target: 'dashboard', label: 'Dashboard', icon: navIcon('mdi:view-dashboard-outline') },
  { target: 'user-usage', label: 'Usage', icon: <ChartColumnIncreasing size={18} strokeWidth={2.2} /> },
  { target: 'tokens', label: 'Tokens', icon: navIcon('mdi:key-chain-variant') },
  { target: 'keys', label: 'API Keys', icon: navIcon('mdi:key-outline') },
  { target: 'requests', label: 'Requests', icon: navIcon('mdi:file-document-outline') },
  { target: 'jobs', label: 'Jobs', icon: navIcon('mdi:calendar-clock-outline') },
  { target: 'users', label: 'Users', icon: navIcon('mdi:account-group-outline') },
  { target: 'alerts', label: 'Alerts', icon: navIcon('mdi:bell-ring-outline') },
  { target: 'proxy-settings', label: 'Proxy Settings', icon: navIcon('mdi:tune-variant') },
]

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
  const [activeModule, setActiveModule] = useState<AdminNavTarget>('jobs')

  return (
    <AdminShell
      activeItem={activeModule}
      navItems={NAV_ITEMS}
      skipToContentLabel="Skip to main content"
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
                <span>Ops Admin</span>
              </div>
              <span className="admin-panel-header-time" aria-live="polite">
                <Icon icon="mdi:clock-time-four-outline" width={14} height={14} className="admin-panel-header-time-icon" aria-hidden="true" />
                <span className="admin-panel-header-time-label">Updated</span>
                <span className="admin-panel-header-time-value">11:42:10</span>
              </span>
            </div>
          </AdminSidebarUtilityCard>
          <AdminSidebarUtilityCard>
            <div className="admin-sidebar-utility-actions">
              <AdminReturnToConsoleLink
                label="Back to User Console"
                href="/console"
                className="admin-sidebar-utility-action"
              />
              <Button type="button" variant="outline" size="sm" className="admin-panel-refresh-button admin-sidebar-utility-action">
                <Icon icon="mdi:refresh" width={16} height={16} aria-hidden="true" />
                <span>Refresh Now</span>
              </Button>
            </div>
          </AdminSidebarUtilityCard>
        </AdminSidebarUtilityStack>
      </AdminShellSidebarUtility>

      <div className="admin-stacked-only">
        <AdminPanelHeader
          title="Tavily Hikari Overview"
          subtitle="Monitor API key allocation, quota health, and recent proxy activity."
          displayName="Ops Admin"
          isAdmin
          updatedPrefix="Updated"
          updatedTime="11:42:10"
          isRefreshing={false}
          refreshLabel="Refresh Now"
          refreshingLabel="Refreshing"
          userConsoleLabel="Back to User Console"
          userConsoleHref="/console"
          onRefresh={() => undefined}
        />
      </div>
      <div className="admin-desktop-only">
        <AdminCompactIntro
          title="Tavily Hikari Overview"
          description="Monitor API key allocation, quota health, and recent proxy activity."
        />
      </div>
      <LayoutBody title="Scheduled Jobs" description="Responsive layout fixture for shell and header verification." />
    </AdminShell>
  )
}

function TokenUsageLayoutStory(): JSX.Element {
  const [activeModule, setActiveModule] = useState<AdminNavTarget>('tokens')
  const [period, setPeriod] = useState<'day' | 'month' | 'all'>('day')
  const [focus, setFocus] = useState<'usage' | 'errors' | 'other'>('usage')

  return (
    <AdminShell
      activeItem={activeModule}
      navItems={NAV_ITEMS}
      skipToContentLabel="Skip to main content"
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
                label="Back to User Console"
                href="/console"
                className="admin-sidebar-utility-action"
              />
              <Button type="button" variant="ghost" size="sm" className="token-usage-back-button admin-sidebar-utility-action" onClick={() => setActiveModule('tokens')}>
                <Icon icon="mdi:arrow-left" width={16} height={16} aria-hidden="true" />
                <span>Back</span>
              </Button>
              <Button type="button" variant="outline" size="sm" className="token-usage-refresh-button admin-sidebar-utility-action">
                <Icon icon="mdi:refresh" width={16} height={16} aria-hidden="true" />
                <span>Refresh Now</span>
              </Button>
            </div>
          </AdminSidebarUtilityCard>
        </AdminSidebarUtilityStack>
      </AdminShellSidebarUtility>

      <div className="admin-stacked-only">
        <TokenUsageHeader
          title="Token Usage Leaderboard"
          subtitle="Compare usage, errors, and anomaly signals by period."
          visualPreset="accent"
          backLabel="Back"
          refreshLabel="Refresh Now"
          refreshingLabel="Refreshing"
          userConsoleLabel="Back to User Console"
          userConsoleHref="/console"
          isRefreshing={false}
          period={period}
          focus={focus}
          periodOptions={[
            { value: 'day', label: 'Today' },
            { value: 'month', label: 'Month' },
            { value: 'all', label: 'All time' },
          ]}
          focusOptions={[
            { value: 'usage', label: 'Usage' },
            { value: 'errors', label: 'Errors' },
            { value: 'other', label: 'Other' },
          ]}
          onBack={() => setActiveModule('tokens')}
          onRefresh={() => undefined}
          onPeriodChange={setPeriod}
          onFocusChange={setFocus}
        />
      </div>
      <div className="admin-desktop-only" style={{ display: 'grid', gap: 14 }}>
        <AdminCompactIntro
          title="Token Usage Leaderboard"
          description="Compare usage, errors, and anomaly signals by period."
        />
        <div className="surface panel" style={{ padding: 14 }}>
          <div className="token-usage-header-filters">
            <SegmentedTabs<'day' | 'month' | 'all'>
              className="token-usage-segmented"
              value={period}
              onChange={setPeriod}
              options={[
                { value: 'day', label: 'Today' },
                { value: 'month', label: 'Month' },
                { value: 'all', label: 'All time' },
              ]}
              ariaLabel="Token leaderboard period"
            />
            <SegmentedTabs<'usage' | 'errors' | 'other'>
              className="token-usage-segmented"
              value={focus}
              onChange={setFocus}
              options={[
                { value: 'usage', label: 'Usage' },
                { value: 'errors', label: 'Errors' },
                { value: 'other', label: 'Other' },
              ]}
              ariaLabel="Token leaderboard focus"
            />
          </div>
        </div>
      </div>
      <LayoutBody title="Top Tokens" description="Use viewport toolbar to verify the mobile top layout behavior." />
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
    navItems: NAV_ITEMS,
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
