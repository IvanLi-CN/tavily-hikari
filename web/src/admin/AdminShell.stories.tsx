import type { Meta, StoryObj } from '@storybook/react-vite'
import { useState } from 'react'

import AdminPanelHeader from '../components/AdminPanelHeader'
import TokenUsageHeader from '../components/TokenUsageHeader'
import AdminShell, { type AdminNavItem } from './AdminShell'
import type { AdminModuleId } from './routes'

const NAV_ITEMS: AdminNavItem[] = [
  { module: 'dashboard', label: 'Dashboard', icon: 'mdi:view-dashboard-outline' },
  { module: 'tokens', label: 'Tokens', icon: 'mdi:key-chain-variant' },
  { module: 'keys', label: 'API Keys', icon: 'mdi:key-outline' },
  { module: 'requests', label: 'Requests', icon: 'mdi:file-document-outline' },
  { module: 'jobs', label: 'Jobs', icon: 'mdi:calendar-clock-outline' },
  { module: 'users', label: 'Users', icon: 'mdi:account-group-outline' },
  { module: 'alerts', label: 'Alerts', icon: 'mdi:bell-ring-outline' },
  { module: 'proxy-settings', label: 'Proxy Settings', icon: 'mdi:tune-variant' },
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
  const [activeModule, setActiveModule] = useState<AdminModuleId>('jobs')

  return (
    <AdminShell
      activeModule={activeModule}
      navItems={NAV_ITEMS}
      skipToContentLabel="Skip to main content"
      onSelectModule={setActiveModule}
    >
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
      <LayoutBody title="Scheduled Jobs" description="Responsive layout fixture for shell and header verification." />
    </AdminShell>
  )
}

function TokenUsageLayoutStory(): JSX.Element {
  const [activeModule, setActiveModule] = useState<AdminModuleId>('tokens')
  const [period, setPeriod] = useState<'day' | 'month' | 'all'>('day')
  const [focus, setFocus] = useState<'usage' | 'errors' | 'other'>('usage')

  return (
    <AdminShell
      activeModule={activeModule}
      navItems={NAV_ITEMS}
      skipToContentLabel="Skip to main content"
      onSelectModule={setActiveModule}
    >
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
    activeModule: 'dashboard',
    navItems: NAV_ITEMS,
    skipToContentLabel: 'Skip to main content',
    onSelectModule: () => undefined,
  },
} satisfies Meta<typeof AdminShell>

export default meta

type Story = StoryObj<typeof meta>

export const PanelHeaderShell: Story = {
  render: () => <PanelHeaderLayoutStory />,
}

export const TokenUsageShell: Story = {
  render: () => <TokenUsageLayoutStory />,
}
