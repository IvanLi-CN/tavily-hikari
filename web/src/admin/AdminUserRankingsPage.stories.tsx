import type { Meta, StoryObj } from '@storybook/react-vite'
import { useState, type ComponentProps } from 'react'
import { expect, userEvent, within } from 'storybook/test'

import { LanguageProvider, translations } from '../i18n'
import AdminUserRankingsPage, { type RankingTabKey } from './AdminUserRankingsPage'
import { rankingsStoryEmptySnapshot, rankingsStorySnapshot } from './rankingsStoryData'

type StoryArgs = ComponentProps<typeof AdminUserRankingsPage> & {
  activeTab?: RankingTabKey
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
          'Inner rankings content module showing six single-select tabs with three cards per active tab grouping.',
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
    activeTab: 'last24h',
    showHeader: false,
  },
} satisfies Meta<StoryArgs>

export default meta

type Story = StoryObj<typeof meta>

function InteractiveRender(args: StoryArgs): JSX.Element {
  const [activeTab, setActiveTab] = useState<RankingTabKey>(args.activeTab ?? 'last24h')
  return <AdminUserRankingsPage {...args} activeTab={activeTab} onTabChange={setActiveTab} />
}

export const Default: Story = {
  render: (args) => <InteractiveRender {...args} />,
}

export const DimensionView: Story = {
  args: {
    activeTab: 'uniqueIp',
  },
  render: (args) => <InteractiveRender {...args} />,
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 180))
    const canvas = within(canvasElement)

    await expect(canvas.getByRole('radio', { name: 'IP' })).toHaveAttribute('aria-checked', 'true')
    await expect(canvas.getByRole('heading', { name: '最近 24 小时' })).toBeInTheDocument()
    await expect(canvas.getByRole('heading', { name: '最近 7 天' })).toBeInTheDocument()
    await expect(canvas.getByRole('heading', { name: '最近 30 天' })).toBeInTheDocument()
    await expect(canvas.queryByRole('heading', { name: '主要调用' })).not.toBeInTheDocument()

    const primaryTab = canvas.getByRole('radio', { name: '主要调用' })
    await userEvent.click(primaryTab)
    await expect(primaryTab).toHaveAttribute('aria-checked', 'true')
    await expect(canvas.getByRole('heading', { name: '最近 24 小时' })).toBeInTheDocument()
    await expect(canvas.getByRole('heading', { name: '最近 7 天' })).toBeInTheDocument()
    await expect(canvas.getByRole('heading', { name: '最近 30 天' })).toBeInTheDocument()
  },
}

export const EmptyState: Story = {
  args: {
    snapshot: rankingsStoryEmptySnapshot,
  },
  render: (args) => <InteractiveRender {...args} />,
}

export const ErrorState: Story = {
  args: {
    error: translations.zh.admin.rankings.error,
    connectionState: 'degraded',
  },
  render: (args) => <InteractiveRender {...args} />,
}

export const ConnectingState: Story = {
  args: {
    connectionState: 'connecting',
  },
  render: (args) => <InteractiveRender {...args} />,
}

export const LoadingState: Story = {
  args: {
    snapshot: null,
    loading: true,
    connectionState: 'connecting',
  },
  render: (args) => <InteractiveRender {...args} />,
}

export const Mobile: Story = {
  parameters: {
    viewport: { defaultViewport: '0390-device-iphone-14' },
  },
  render: (args) => <InteractiveRender {...args} />,
}

export const InteractionContract: Story = {
  args: {
    activeTab: 'last24h',
  },
  render: (args) => <InteractiveRender {...args} />,
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 240))
    const canvas = within(canvasElement)
    const firstRow = canvas.getByRole('button', { name: /^1\./ })

    await userEvent.hover(firstRow)
    await expect(canvasElement.querySelectorAll('.admin-ranking-chart-hit-target.is-interactive').length).toBeGreaterThanOrEqual(2)

    await userEvent.unhover(firstRow)
    await expect(canvasElement.querySelectorAll('.admin-ranking-chart-hit-target.is-interactive').length).toBe(0)

    firstRow.focus()
    await new Promise((resolve) => window.setTimeout(resolve, 50))
    await expect(canvasElement.querySelectorAll('.admin-ranking-chart-hit-target.is-interactive').length).toBeGreaterThanOrEqual(2)

    firstRow.blur()
    await new Promise((resolve) => window.setTimeout(resolve, 50))
    await expect(canvasElement.querySelectorAll('.admin-ranking-chart-hit-target.is-interactive').length).toBe(0)
  },
}
