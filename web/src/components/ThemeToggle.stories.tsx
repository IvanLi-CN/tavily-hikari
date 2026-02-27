import type { Meta, StoryObj } from '@storybook/react-vite'

import ThemeToggle from './ThemeToggle'

const meta = {
  title: 'Components/ThemeToggle',
  component: ThemeToggle,
  render: () => (
    <div style={{ padding: 24, display: 'flex', justifyContent: 'flex-end' }}>
      <ThemeToggle />
    </div>
  ),
} satisfies Meta<typeof ThemeToggle>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {}
