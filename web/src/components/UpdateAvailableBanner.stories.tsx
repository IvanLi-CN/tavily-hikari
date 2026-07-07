import type { Meta, StoryObj } from '@storybook/react-vite'

import { EN } from '../i18n/translations/en'
import { ZH } from '../i18n/translations/zh'
import UpdateAvailableBanner from './UpdateAvailableBanner'

const meta = {
  title: 'Support/Status/UpdateAvailableBanner',
  component: UpdateAvailableBanner,
  tags: ['autodocs'],
  parameters: {
    layout: 'padded',
    docs: {
      description: {
        component:
          'Shared PWA update prompt used by public, console, login, registration-paused, and admin surfaces. It appears once the service worker starts caching a new app shell and switches to a reload action after the update is ready.',
      },
    },
  },
  args: {
    strings: EN.public.updateBanner,
    currentVersion: '0.2.0',
    availableVersion: '0.2.1',
    status: 'ready',
    loading: false,
    onUpdate: () => undefined,
    onDismiss: () => undefined,
  },
  render: (args) => (
    <div className="app-shell public-home" style={{ maxWidth: 1040, margin: '0 auto' }}>
      <UpdateAvailableBanner {...args} />
    </div>
  ),
} satisfies Meta<typeof UpdateAvailableBanner>

export default meta

type Story = StoryObj<typeof meta>

export const Ready: Story = {}

export const Installing: Story = {
  args: {
    status: 'installing',
    loading: true,
  },
}

export const Activating: Story = {
  args: {
    status: 'activating',
    loading: true,
  },
}

export const ChineseReady: Story = {
  args: {
    strings: ZH.public.updateBanner,
    currentVersion: '0.2.0',
    availableVersion: '0.2.1',
  },
}

export const DarkReady: Story = {
  args: {
    strings: ZH.public.updateBanner,
    currentVersion: '0.2.0',
    availableVersion: '0.2.1',
  },
  parameters: {
    backgrounds: { default: 'dark' },
  },
  decorators: [
    (StoryComponent) => (
      <div className="dark" style={{ minHeight: 180, padding: 24, background: 'hsl(var(--background))' }}>
        <StoryComponent />
      </div>
    ),
  ],
}
