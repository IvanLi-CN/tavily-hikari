import { useState } from 'react'
import type { Meta, StoryObj } from '@storybook/react-vite'

import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './select'

const meta = {
  title: 'UI/Select',
  parameters: {
    layout: 'centered',
  },
} satisfies Meta

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
  render: () => <SelectStory />,
}

export const MobileWidth: Story = {
  parameters: {
    viewport: { defaultViewport: '0390-device-iphone-14' },
  },
  render: () => <SelectStory width={176} />,
}
