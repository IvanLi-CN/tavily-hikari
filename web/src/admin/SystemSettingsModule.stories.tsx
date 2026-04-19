import type { Meta, StoryObj } from '@storybook/react-vite'

import SystemSettingsModule from './SystemSettingsModule'
import { translations } from '../i18n'

function SystemSettingsCanvas(props: {
  requestRateLimit?: number
  count?: number
  rebalanceEnabled?: boolean
  rebalancePercent?: number
  loadState?: 'initial_loading' | 'switch_loading' | 'refreshing' | 'ready' | 'error'
  error?: string | null
  saving?: boolean
  helpBubbleOpen?: boolean
}): JSX.Element {
  return (
    <div style={{ maxWidth: 960, margin: '0 auto' }}>
      <SystemSettingsModule
        strings={translations.zh.admin.systemSettings}
        settings={{
          requestRateLimit: props.requestRateLimit ?? 100,
          mcpSessionAffinityKeyCount: props.count ?? 5,
          rebalanceMcpEnabled: props.rebalanceEnabled ?? false,
          rebalanceMcpSessionPercent: props.rebalancePercent ?? 100,
        }}
        loadState={props.loadState ?? 'ready'}
        error={props.error ?? null}
        saving={props.saving ?? false}
        helpBubbleOpen={props.helpBubbleOpen}
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
        component: 'Admin-only MCP session affinity and Rebalance MCP controls with immediate apply feedback.',
      },
    },
  },
  args: {
    strings: translations.zh.admin.systemSettings,
    settings: {
      requestRateLimit: 100,
      mcpSessionAffinityKeyCount: 5,
      rebalanceMcpEnabled: false,
      rebalanceMcpSessionPercent: 100,
    },
    loadState: 'ready',
    error: null,
    saving: false,
    helpBubbleOpen: undefined,
    onApply: () => {},
  },
  render: () => <SystemSettingsCanvas />,
} satisfies Meta<typeof SystemSettingsModule>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {}

export const RebalanceEnabled: Story = {
  render: () => <SystemSettingsCanvas rebalanceEnabled rebalancePercent={35} />,
}

export const RebalanceDisabledSliderLocked: Story = {
  render: () => <SystemSettingsCanvas rebalanceEnabled={false} rebalancePercent={35} />,
}

export const Applying: Story = {
  render: () => <SystemSettingsCanvas rebalanceEnabled rebalancePercent={35} saving />,
}

export const ErrorState: Story = {
  render: () => <SystemSettingsCanvas error="Failed to save system settings." rebalanceEnabled />,
}

export const HelpBubbleOpen: Story = {
  render: () => <SystemSettingsCanvas helpBubbleOpen />,
}

export const RequestRateEdited: Story = {
  render: () => <SystemSettingsCanvas requestRateLimit={80} rebalanceEnabled rebalancePercent={35} />,
}
