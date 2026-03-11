import type { Meta, StoryObj } from '@storybook/react-vite'

import AdminNavButton from './AdminNavButton'

const meta = {
  title: 'Admin/Wrappers/AdminNavButton',
  component: AdminNavButton,
  parameters: {
    layout: 'padded',
  },
  args: {
    icon: 'mdi:account-group-outline',
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
        <AdminNavButton icon="mdi:view-dashboard-outline">Dashboard</AdminNavButton>
        <AdminNavButton icon="mdi:key-chain-variant">Tokens</AdminNavButton>
        <AdminNavButton icon="mdi:key-outline">API Keys</AdminNavButton>
        <AdminNavButton icon="mdi:file-document-outline">Requests</AdminNavButton>
        <AdminNavButton icon="mdi:calendar-clock-outline">Jobs</AdminNavButton>
        <AdminNavButton icon="mdi:account-group-outline" active>
          Users
        </AdminNavButton>
      </nav>
    </div>
  ),
}
