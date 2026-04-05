import type { Meta, StoryObj } from '@storybook/react-vite'

import SystemSettingsModule from './SystemSettingsModule'
import { translations } from '../i18n'

function SystemSettingsCanvas(props: {
  count?: number
  loadState?: 'initial_loading' | 'switch_loading' | 'refreshing' | 'ready' | 'error'
  error?: string | null
  saving?: boolean
}): JSX.Element {
  return (
    <div style={{ maxWidth: 960, margin: '0 auto' }}>
      <SystemSettingsModule
        strings={translations.zh.admin.systemSettings}
        settings={{ mcpSessionAffinityKeyCount: props.count ?? 5 }}
        loadState={props.loadState ?? 'ready'}
        error={props.error ?? null}
        saving={props.saving ?? false}
        onApply={() => {}}
      />
    </div>
  )
}

const meta = {
  title: 'Admin/SystemSettingsModule',
  component: SystemSettingsModule,
  parameters: {
    layout: 'padded',
    docs: {
      description: {
        component: 'Admin-only MCP session affinity controls with immediate apply feedback.',
      },
    },
  },
  args: {
    strings: translations.zh.admin.systemSettings,
    settings: { mcpSessionAffinityKeyCount: 5 },
    loadState: 'ready',
    error: null,
    saving: false,
    onApply: () => {},
  },
  render: () => <SystemSettingsCanvas />,
} satisfies Meta<typeof SystemSettingsModule>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {}

export const Applying: Story = {
  render: () => <SystemSettingsCanvas saving />,
}

export const ErrorState: Story = {
  render: () => <SystemSettingsCanvas error="Failed to save system settings." />,
}
