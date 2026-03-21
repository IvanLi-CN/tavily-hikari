import type { Meta, StoryObj } from '@storybook/react-vite'

import { Icon } from '../lib/icons'
import AdminNavButton from './AdminNavButton'

function navIcon(name: string): JSX.Element {
  return <Icon icon={name} width={18} height={18} />
}

const meta = {
  title: 'Admin/Wrappers/AdminNavButton',
  component: AdminNavButton,
  parameters: {
    layout: 'padded',
  },
  args: {
    icon: navIcon('mdi:account-group-outline'),
    children: 'Users',
  },
  render: (args) => (
    <div style={{ width: 260 }}>
      <AdminNavButton {...args} />
    </div>
  ),
} satisfies Meta<typeof AdminNavButton>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {}

export const Active: Story = {
  args: {
    active: true,
  },
}

export const SidebarStack: Story = {
  render: () => (
    <div className="admin-sidebar-menu is-open" style={{ width: 260, padding: 12, borderRadius: 18 }}>
      <nav className="admin-sidebar-nav" aria-label="Admin navigation preview">
        <AdminNavButton icon={navIcon('mdi:view-dashboard-outline')}>Dashboard</AdminNavButton>
        <AdminNavButton icon={navIcon('mdi:key-chain-variant')}>Tokens</AdminNavButton>
        <AdminNavButton icon={navIcon('mdi:key-outline')}>API Keys</AdminNavButton>
        <AdminNavButton icon={navIcon('mdi:file-document-outline')}>Requests</AdminNavButton>
        <AdminNavButton icon={navIcon('mdi:calendar-clock-outline')}>Jobs</AdminNavButton>
        <AdminNavButton icon={navIcon('mdi:account-group-outline')} active>
          Users
        </AdminNavButton>
      </nav>
    </div>
  ),
}
