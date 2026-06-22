import type { Meta, StoryObj } from '@storybook/react-vite'
import type { ComponentProps } from 'react'

import { LanguageProvider, translations } from '../i18n'
import AdminUserRankingsPage, { type RankingTabKey } from './AdminUserRankingsPage'
import { rankingsStoryEmptySnapshot, rankingsStorySnapshot } from './rankingsStoryData'

type StoryArgs = ComponentProps<typeof AdminUserRankingsPage> & {
  initialTab?: RankingTabKey
}

const meta = {
  title: 'Admin/Modules/UserRankingsContent',
  component: AdminUserRankingsPage,
  tags: ['autodocs'],
  parameters: {
    layout: 'fullscreen',
    docs: {
      description: {
        component:
          'Inner rankings content module showing six single-select tabs and one stable three-metric card layout inside the current time window.',
      },
    },
  },
  decorators: [
    (Story) => (
      <LanguageProvider initialLanguage="zh">
        <div style={{ maxWidth: 1440, margin: '0 auto', padding: '24px', overflowX: 'clip' }}>
          <Story />
        </div>
      </LanguageProvider>
    ),
  ],
  args: {
    strings: translations.zh.admin.rankings,
    language: 'zh',
    snapshot: rankingsStorySnapshot,
    loading: false,
    error: null,
    connectionState: 'live',
    onRetry: () => undefined,
    initialTab: 'last24h',
    showHeader: false,
  },
} satisfies Meta<StoryArgs>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {
  render: (args) => <AdminUserRankingsPage {...args} />,
}

export const DimensionView: Story = {
  args: {
    initialTab: 'uniqueIp',
  },
  render: (args) => <AdminUserRankingsPage {...args} />,
}

export const EmptyState: Story = {
  args: {
    snapshot: rankingsStoryEmptySnapshot,
  },
  render: (args) => <AdminUserRankingsPage {...args} />,
}

export const ErrorState: Story = {
  args: {
    error: translations.zh.admin.rankings.error,
    connectionState: 'degraded',
  },
  render: (args) => <AdminUserRankingsPage {...args} />,
}

export const ConnectingState: Story = {
  args: {
    connectionState: 'connecting',
  },
  render: (args) => <AdminUserRankingsPage {...args} />,
}

export const LoadingState: Story = {
  args: {
    snapshot: null,
    loading: true,
    connectionState: 'connecting',
  },
  render: (args) => <AdminUserRankingsPage {...args} />,
}

export const Mobile: Story = {
  parameters: {
    viewport: { defaultViewport: '0390-device-iphone-14' },
  },
  render: (args) => <AdminUserRankingsPage {...args} />,
}
