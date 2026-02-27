import type { Meta, StoryObj } from '@storybook/react-vite'

import AdminPanelHeader from './AdminPanelHeader'

const meta = {
  title: 'Admin/AdminPanelHeader',
  component: AdminPanelHeader,
  parameters: {
    layout: 'padded',
  },
  render: (args) => (
    <div style={{ maxWidth: 1240, margin: '0 auto' }}>
      <AdminPanelHeader {...args} />
    </div>
  ),
} satisfies Meta<typeof AdminPanelHeader>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {
  args: {
    title: 'Tavily Hikari 总览',
    subtitle: '监控 API Key 分配、额度健康与最新代理请求活动。',
    displayName: 'dev-mode',
    isAdmin: true,
    updatedPrefix: '更新于',
    updatedTime: '14:33:53',
    isRefreshing: false,
    refreshLabel: '立即刷新',
    refreshingLabel: '刷新中…',
    onRefresh: () => undefined,
  },
}

export const Refreshing: Story = {
  args: {
    ...Default.args,
    isRefreshing: true,
    refreshLabel: '立即刷新',
    refreshingLabel: '刷新中…',
  },
}
