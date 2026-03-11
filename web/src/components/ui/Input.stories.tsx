import type { Meta, StoryObj } from '@storybook/react-vite'

import { Input } from './input'

const meta = {
  title: 'UI/Input',
  component: Input,
  tags: ['autodocs'],
  parameters: {
    layout: 'centered',
    docs: {
      description: {
        component:
          'Shared single-line text input with the project focus ring and muted placeholder styling. Pair it with labels, helper text, or inline buttons in parent layout code.',
      },
    },
  },
  args: {
    placeholder: 'Search request logs',
    type: 'text',
  },
  render: (args) => (
    <div className="w-[320px]">
      <Input {...args} />
    </div>
  ),
} satisfies Meta<typeof Input>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {}

export const Disabled: Story = {
  args: {
    disabled: true,
    defaultValue: 'Readonly snapshot',
  },
}

export const WithSupportingCopy: Story = {
  render: () => (
    <label className="grid w-[320px] gap-2 text-sm font-medium">
      Token label
      <Input placeholder="batch-enrichment" />
      <span className="text-xs font-normal text-muted-foreground">
        Supporting copy lives outside the primitive so forms can choose their own layout rhythm.
      </span>
    </label>
  ),
}
