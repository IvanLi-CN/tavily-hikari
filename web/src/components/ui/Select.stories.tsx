import { useState } from 'react'
import type { Meta, StoryObj } from '@storybook/react-vite'

import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './select'

const meta = {
  title: 'UI/Select',
  component: Select,
  subcomponents: {
    SelectContent,
    SelectItem,
    SelectValue,
  },
  tags: ['autodocs'],
  parameters: {
    layout: 'centered',
    docs: {
      description: {
        component:
          'Composed select primitive built from a root, trigger, floating content, and items. The trigger owns the visible field, while `SelectContent` and `SelectItem` define the popover menu.',
      },
    },
  },
} satisfies Meta<typeof Select>

export default meta

type Story = StoryObj<typeof meta>

function SelectStory(props: { width?: number }): JSX.Element {
  const [value, setValue] = useState('all')
  return (
    <div style={{ width: props.width ?? 220 }}>
      <Select value={value} onValueChange={setValue}>
        <SelectTrigger aria-label="Request filter">
          <SelectValue />
        </SelectTrigger>
        <SelectContent align="start">
          <SelectItem value="all">All</SelectItem>
          <SelectItem value="success">Success</SelectItem>
          <SelectItem value="error">Errors</SelectItem>
          <SelectItem value="quota_exhausted">Quota exhausted</SelectItem>
        </SelectContent>
      </Select>
    </div>
  )
}

export const Default: Story = {
  parameters: {
    docs: {
      description: {
        story:
          'Baseline desktop composition showing the trigger and popover content in their default width relationship.',
      },
    },
  },
  render: () => <SelectStory />,
}

export const MobileWidth: Story = {
  parameters: {
    viewport: { defaultViewport: '0390-device-iphone-14' },
    docs: {
      description: {
        story:
          'Use the viewport toolbar to validate narrow trigger widths while keeping the popover aligned to the trigger edge.',
      },
    },
  },
  render: () => <SelectStory width={176} />,
}
