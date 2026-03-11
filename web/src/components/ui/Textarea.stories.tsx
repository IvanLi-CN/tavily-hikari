import type { Meta, StoryObj } from '@storybook/react-vite'

import { Textarea } from './textarea'

const meta = {
  title: 'UI/Textarea',
  component: Textarea,
  tags: ['autodocs'],
  parameters: {
    layout: 'centered',
    docs: {
      description: {
        component:
          'Shared multiline text primitive for notes, pasted batches, and freeform explanations. Autosizing and parsing behaviors belong in the caller, not inside the primitive.',
      },
    },
  },
  args: {
    placeholder: 'Paste one token per line',
  },
  render: (args) => (
    <div className="w-[360px]">
      <Textarea {...args} />
    </div>
  ),
} satisfies Meta<typeof Textarea>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {}

export const Prefilled: Story = {
  args: {
    defaultValue: 'token_a\ntoken_b\ntoken_c',
  },
}
