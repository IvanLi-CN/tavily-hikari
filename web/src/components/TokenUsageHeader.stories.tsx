import { useState } from 'react'
import type { Meta, StoryObj } from '@storybook/react-vite'

import TokenUsageHeader from './TokenUsageHeader'

type Period = 'day' | 'month' | 'all'
type Focus = 'usage' | 'errors' | 'other'

const periodOptions = [
  { value: 'day', label: '日维度' },
  { value: 'month', label: '月维度' },
  { value: 'all', label: '累计' },
] as const

const focusOptions = [
  { value: 'usage', label: '调用量' },
  { value: 'errors', label: '错误数' },
  { value: 'other', label: '其他结果' },
] as const

const meta = {
  title: 'Admin/TokenUsageHeader',
  component: TokenUsageHeader,
  parameters: {
    layout: 'padded',
  },
  render: (args) => {
    const [period, setPeriod] = useState<Period>('day')
    const [focus, setFocus] = useState<Focus>('usage')

    return (
      <div style={{ maxWidth: 1240, margin: '0 auto' }}>
        <TokenUsageHeader
          {...args}
          period={period}
          focus={focus}
          periodOptions={periodOptions}
          focusOptions={focusOptions}
          onPeriodChange={setPeriod}
          onFocusChange={setFocus}
        />
      </div>
    )
  },
} satisfies Meta<typeof TokenUsageHeader>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {
  args: {
    title: 'Token 使用排行榜',
    subtitle: '按调用量与异常类型快速筛选，定位高负载 Token 与异常热点。',
    backLabel: '返回总览',
    refreshLabel: '立即刷新',
    refreshingLabel: '刷新中…',
    isRefreshing: false,
    period: 'day',
    focus: 'usage',
    periodOptions,
    focusOptions,
    onBack: () => undefined,
    onRefresh: () => undefined,
    onPeriodChange: () => undefined,
    onFocusChange: () => undefined,
  },
}

export const Panel: Story = {
  args: {
    ...Default.args,
    visualPreset: 'panel',
  },
}

export const Inline: Story = {
  args: {
    ...Default.args,
    visualPreset: 'inline',
  },
}

export const Accent: Story = {
  args: {
    ...Default.args,
    visualPreset: 'accent',
  },
}

export const Refreshing: Story = {
  args: {
    ...Default.args,
    isRefreshing: true,
  },
}
