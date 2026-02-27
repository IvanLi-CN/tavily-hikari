import { useState } from 'react'
import type { Meta, StoryObj } from '@storybook/react-vite'

import SegmentedTabs, { type SegmentedTabsOption } from './SegmentedTabs'

type DemoValue = 'all' | 'success' | 'error' | 'quota_exhausted' | 'quota' | 'usage' | 'logs'

const meta = {
  title: 'UI/SegmentedTabs',
  component: SegmentedTabs,
  parameters: {
    layout: 'centered',
  },
  args: {
    value: 'all',
    ariaLabel: 'filter',
    options: [
      { value: 'all', label: '全部' },
      { value: 'success', label: '成功' },
      { value: 'error', label: '错误' },
      { value: 'quota_exhausted', label: '额度耗尽' },
    ] as ReadonlyArray<SegmentedTabsOption<DemoValue>>,
    onChange: () => undefined,
  },
  render: (args) => {
    const [value, setValue] = useState(args.value as DemoValue)
    const options = args.options as ReadonlyArray<SegmentedTabsOption<DemoValue>>
    return (
      <div style={{ padding: 12, borderRadius: 14, background: 'hsl(var(--muted) / 0.3)' }}>
        <SegmentedTabs<DemoValue>
          value={value}
          onChange={setValue}
          ariaLabel={args.ariaLabel}
          options={options}
          className={args.className}
        />
      </div>
    )
  },
} satisfies Meta<typeof SegmentedTabs>

export default meta

type Story = StoryObj<typeof meta>

export const RequestResult: Story = {
  args: {
    ariaLabel: '近期请求筛选',
    value: 'all',
    options: [
      { value: 'all', label: '全部' },
      { value: 'success', label: '成功' },
      { value: 'error', label: '错误' },
      { value: 'quota_exhausted', label: '额度耗尽' },
    ],
  },
}

export const JobType: Story = {
  args: {
    ariaLabel: '计划任务筛选',
    value: 'all',
    options: [
      { value: 'all', label: '全部' },
      { value: 'quota', label: '同步额度' },
      { value: 'usage', label: '用量聚合' },
      { value: 'logs', label: '清理日志' },
    ],
  },
}
