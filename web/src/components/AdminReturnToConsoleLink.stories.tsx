import type { Meta, StoryObj } from '@storybook/react-vite'

import AdminReturnToConsoleLink from './AdminReturnToConsoleLink'

const meta = {
  title: 'Admin/AdminReturnToConsoleLink',
  component: AdminReturnToConsoleLink,
  parameters: {
    layout: 'centered',
  },
  args: {
    label: '返回用户控制台',
  },
} satisfies Meta<typeof AdminReturnToConsoleLink>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {}
