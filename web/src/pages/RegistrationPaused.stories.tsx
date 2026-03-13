import type { Meta, StoryObj } from '@storybook/react-vite'

import RegistrationPaused from './RegistrationPaused'

const meta = {
  title: 'Public/Pages/RegistrationPaused',
  component: RegistrationPaused,
  parameters: {
    layout: 'fullscreen',
  },
} satisfies Meta<typeof RegistrationPaused>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {}
