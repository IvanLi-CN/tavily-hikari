import { useState } from 'react'
import type { Meta, StoryObj } from '@storybook/react-vite'

import DateTimeRangeField from './DateTimeRangeField'

function DateTimeRangeFieldStory(): JSX.Element {
  const [createdFrom, setCreatedFrom] = useState('2026-07-13')
  const [createdTo, setCreatedTo] = useState('2026-07-14')

  return (
    <div style={{ maxWidth: 760, margin: '0 auto' }}>
      <DateTimeRangeField
        label="Created range"
        startId="storybook-datetime-range-created-from"
        endId="storybook-datetime-range-created-to"
        startLabel="Created from"
        endLabel="Created to"
        startValue={createdFrom}
        endValue={createdTo}
        startSeparator="to"
        startMax={createdTo}
        endMin={createdFrom}
        onStartChange={setCreatedFrom}
        onEndChange={setCreatedTo}
      />
    </div>
  )
}

const meta = {
  title: 'Admin/Wrappers/DateTimeRangeField',
  component: DateTimeRangeField,
  parameters: {
    layout: 'padded',
  },
  args: {
    label: 'Created range',
    startId: 'storybook-datetime-range-created-from',
    endId: 'storybook-datetime-range-created-to',
    startLabel: 'Created from',
    endLabel: 'Created to',
    startValue: '2026-07-13',
    endValue: '2026-07-14',
    startSeparator: 'to',
    startMax: '2026-07-14',
    endMin: '2026-07-13',
    onStartChange: () => undefined,
    onEndChange: () => undefined,
  },
  render: () => <DateTimeRangeFieldStory />,
} satisfies Meta<typeof DateTimeRangeField>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {}
