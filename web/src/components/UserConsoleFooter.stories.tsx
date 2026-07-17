import type { Meta, StoryObj } from '@storybook/react-vite'

import UserConsoleFooter from './UserConsoleFooter'

const strings = {
  title: 'Tavily Hikari User Console',
  githubAria: 'Open GitHub repository',
  githubLabel: 'GitHub',
  loadingVersion: '· Loading version…',
  errorVersion: '· Version unavailable',
  tagPrefix: '· ',
}

const meta = {
  title: 'Console/UserConsoleFooter',
  component: UserConsoleFooter,
  parameters: {
    layout: 'centered',
    docs: {
      description: {
        component: 'Footer proof for backend-version release links shared with the public home and admin footer.',
      },
    },
  },
} satisfies Meta<typeof UserConsoleFooter>

export default meta

type Story = StoryObj<typeof meta>

export const StableRelease: Story = {
  args: {
    strings,
    versionState: { status: 'ready', value: { backend: '0.81.1', frontend: '0.81.1' } },
  },
}

export const Prerelease: Story = {
  args: {
    strings,
    versionState: { status: 'ready', value: { backend: '0.81.1-rc.1', frontend: '0.81.1-rc.1' } },
  },
}

export const NonReleaseFallback: Story = {
  args: {
    strings,
    versionState: { status: 'ready', value: { backend: '0.81.1-dev', frontend: '0.81.1-dev' } },
  },
}
