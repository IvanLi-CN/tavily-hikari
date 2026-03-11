import { useMemo, useState } from 'react'
import type { Meta, StoryObj } from '@storybook/react-vite'

import QuotaRangeField from './QuotaRangeField'

function QuotaRangeFieldStory(): JSX.Element {
  const [sliderValue, setSliderValue] = useState(3)
  const [inputValue, setInputValue] = useState('1000')

  const helperText = useMemo(() => {
    return `Stage ${sliderValue} maps to a quota bucket around ${inputValue || '0'} requests.`
  }, [inputValue, sliderValue])

  return (
    <div style={{ maxWidth: 760, margin: '0 auto' }}>
      <QuotaRangeField
        label="Hourly quota"
        sliderName="hourly-quota-stage"
        sliderMin={0}
        sliderMax={6}
        sliderValue={sliderValue}
        sliderAriaLabel="Hourly quota stage"
        helperText={helperText}
        onSliderChange={(value) => setSliderValue(value)}
        inputName="hourly-quota-input"
        inputValue={inputValue}
        inputAriaLabel="Hourly quota input"
        onInputChange={setInputValue}
      />
    </div>
  )
}

const meta = {
  title: 'Admin/Wrappers/QuotaRangeField',
  component: QuotaRangeField,
  parameters: {
    layout: 'padded',
  },
  args: {
    label: 'Hourly quota',
    sliderName: 'hourly-quota-stage',
    sliderMin: 0,
    sliderMax: 6,
    sliderValue: 3,
    sliderAriaLabel: 'Hourly quota stage',
    helperText: 'Stage 3 maps to a quota bucket around 1000 requests.',
    onSliderChange: () => undefined,
    inputName: 'hourly-quota-input',
    inputValue: '1000',
    inputAriaLabel: 'Hourly quota input',
    onInputChange: () => undefined,
  },
  render: () => <QuotaRangeFieldStory />,
} satisfies Meta<typeof QuotaRangeField>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {}
