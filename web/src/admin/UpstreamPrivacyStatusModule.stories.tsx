import type { Meta, StoryObj } from '@storybook/react-vite'
import { useState, type ComponentProps } from 'react'
import { expect, userEvent, within } from 'storybook/test'

import UpstreamPrivacyStatusModule from './UpstreamPrivacyStatusModule'
import type { UpstreamPrivacyStatus } from '../api'
import { translations } from '../i18n'

type StoryArgs = ComponentProps<typeof UpstreamPrivacyStatusModule>

const pendingStatus: UpstreamPrivacyStatus = {
  phase: 'pending',
  configuredProjectIdMode: 'accessToken',
  effectiveProjectIdMode: 'accessToken',
  fixedProjectIdConfigured: false,
  configuredMcpUserAgent: '',
  effectiveMcpUserAgent: null,
  upstreamPreciseReconciliationEnabled: true,
  httpAllowedHeaders: ['accept', 'accept-encoding', 'content-type', 'x-project-id (policy injected)'],
  controlMcpAllowedHeaders: ['accept', 'cache-control', 'mcp-protocol-version', 'mcp-session-id', 'user-agent (configured only)'],
  gates: [
    { key: 'accessTokenMode', ready: true, detail: 'accessToken' },
    { key: 'apiRebalance', ready: true, detail: '100%' },
    { key: 'mcpRebalance', ready: true, detail: '100%' },
    { key: 'controlSessionsDrained', ready: false, detail: '2' },
  ],
  completedGates: 3,
  totalGates: 4,
  activeControlSessions: 2,
  currentPeriodCode: '2026-07-14/S2',
  currentPeriodEndsAt: 1_783_994_400,
  nextEpochAt: 1_783_994_400,
  pendingResearch: 1,
  queuedSettlements: 2,
  degradedSettlements: 0,
  recentAdjustments: [
    {
      settlementKey: 'v1:tok_demo:2026-07-14/S1',
      tokenIdHint: 'tok_demo',
      billingSubjectKind: 'token',
      periodCode: '2026-07-14/S1',
      deltaCredits: -3,
      degradedReason: null,
      createdAt: 1_783_958_100,
    },
  ],
  generatedAt: 1_783_958_400,
}

const activeStatus: UpstreamPrivacyStatus = {
  ...pendingStatus,
  phase: 'active',
  completedGates: 4,
  activeControlSessions: 0,
  pendingResearch: 0,
  queuedSettlements: 0,
  gates: pendingStatus.gates.map((gate) => ({
    ...gate,
    ready: true,
    detail: gate.key === 'controlSessionsDrained' ? '0' : gate.detail,
  })),
  recentAdjustments: [],
}

const drainingStatus: UpstreamPrivacyStatus = {
  ...pendingStatus,
  phase: 'draining',
  completedGates: 3,
  activeControlSessions: 5,
  pendingResearch: 0,
  queuedSettlements: 0,
  gates: pendingStatus.gates.map((gate) => ({
    ...gate,
    ready: gate.key !== 'controlSessionsDrained',
    detail: gate.key === 'controlSessionsDrained' ? '5' : gate.detail,
  })),
}

const degradedStatus: UpstreamPrivacyStatus = {
  ...activeStatus,
  phase: 'degraded',
  degradedSettlements: 1,
  recentAdjustments: [
    {
      settlementKey: 'v1:tok_demo:2026-07-13/S3',
      tokenIdHint: 'tok_demo',
      billingSubjectKind: 'token',
      periodCode: '2026-07-13/S3',
      deltaCredits: 2,
      degradedReason: 'research_timeout_24h',
      createdAt: 1_783_958_800,
    },
  ],
}

const compareStatus: UpstreamPrivacyStatus = {
  ...activeStatus,
  phase: 'compare',
  upstreamPreciseReconciliationEnabled: false,
  queuedSettlements: 1,
  recentAdjustments: [
    {
      settlementKey: 'shadow:v1:tok_demo:2026-07-14/S2',
      tokenIdHint: 'tok_demo',
      billingSubjectKind: 'account',
      periodCode: '2026-07-14/S2',
      deltaCredits: 4,
      degradedReason: null,
      createdAt: 1_783_959_000,
    },
  ],
}

const meta = {
  title: 'Admin/Modules/SystemStatusModule',
  component: UpstreamPrivacyStatusModule,
  tags: ['autodocs'],
  parameters: {
    layout: 'padded',
    docs: {
      description: {
        component:
          'Route content for the admin system status page. Keeps the page header separate while foregrounding only the live gates, counters, and disclosure-backed technical details.',
      },
    },
  },
  decorators: [
    (Story) => (
      <div style={{ maxWidth: 1280, margin: '0 auto', padding: 24, overflowX: 'clip' }}>
        <Story />
      </div>
    ),
  ],
  args: {
    strings: translations.zh.admin.systemSettings.privacy,
    formStrings: translations.zh.admin.systemSettings.form,
    language: 'zh',
    status: pendingStatus,
    loadState: 'ready',
    error: null,
    refreshing: false,
    autoRefreshEnabled: true,
    onAutoRefreshChange: () => undefined,
    onRefresh: () => undefined,
  },
} satisfies Meta<StoryArgs>

export default meta

type Story = StoryObj<typeof meta>

function renderWithStatus(status: UpstreamPrivacyStatus | null, overrides?: Partial<StoryArgs>): JSX.Element {
  return (
    <UpstreamPrivacyStatusModule
      strings={translations.zh.admin.systemSettings.privacy}
      formStrings={translations.zh.admin.systemSettings.form}
      language="zh"
      status={status}
      loadState={overrides?.loadState ?? 'ready'}
      error={overrides?.error ?? null}
      refreshing={overrides?.refreshing ?? false}
      autoRefreshEnabled={overrides?.autoRefreshEnabled ?? true}
      onAutoRefreshChange={overrides?.onAutoRefreshChange ?? (() => undefined)}
      onRefresh={overrides?.onRefresh ?? (() => undefined)}
    />
  )
}

function InteractionCanvas(args: StoryArgs): JSX.Element {
  const [autoRefreshEnabled, setAutoRefreshEnabled] = useState(args.autoRefreshEnabled)
  const [refreshCount, setRefreshCount] = useState(0)

  return (
    <div style={{ display: 'grid', gap: 12 }}>
      <UpstreamPrivacyStatusModule
        {...args}
        autoRefreshEnabled={autoRefreshEnabled}
        onAutoRefreshChange={setAutoRefreshEnabled}
        onRefresh={() => setRefreshCount((current) => current + 1)}
      />
      <p data-testid="system-status-refresh-count" style={{ margin: 0, color: 'hsl(var(--muted-foreground))', fontSize: 13 }}>
        刷新次数：{refreshCount}
      </p>
    </div>
  )
}

export const Pending: Story = {}

export const Draining: Story = {
  args: {
    status: drainingStatus,
  },
}

export const Active: Story = {
  args: {
    status: activeStatus,
  },
}

export const CompareOnly: Story = {
  args: {
    status: compareStatus,
  },
}

export const Degraded: Story = {
  args: {
    status: degradedStatus,
  },
}

export const EmptyState: Story = {
  render: () => renderWithStatus(null),
}

export const ErrorState: Story = {
  render: () => renderWithStatus(null, {
    loadState: 'error',
    error: translations.zh.admin.systemSettings.privacy.loadFailed,
  }),
}

export const LoadingState: Story = {
  render: () => renderWithStatus(null, {
    loadState: 'initial_loading',
  }),
}

export const Mobile: Story = {
  parameters: {
    viewport: { defaultViewport: '0390-device-iphone-14' },
  },
}

export const Gallery: Story = {
  render: () => (
    <div style={{ display: 'grid', gap: 24 }}>
      {[
        { title: 'Pending', status: pendingStatus },
        { title: 'Draining', status: drainingStatus },
        { title: 'Compare', status: compareStatus },
        { title: 'Active', status: activeStatus },
        { title: 'Degraded', status: degradedStatus },
      ].map((scenario) => (
        <section key={scenario.title} style={{ display: 'grid', gap: 12 }}>
          <h3 style={{ margin: 0, fontSize: 18, fontWeight: 700 }}>{scenario.title}</h3>
          {renderWithStatus(scenario.status)}
        </section>
      ))}
      <section style={{ display: 'grid', gap: 12 }}>
        <h3 style={{ margin: 0, fontSize: 18, fontWeight: 700 }}>Empty</h3>
        {renderWithStatus(null)}
      </section>
      <section style={{ display: 'grid', gap: 12 }}>
        <h3 style={{ margin: 0, fontSize: 18, fontWeight: 700 }}>Error</h3>
        {renderWithStatus(null, {
          loadState: 'error',
          error: translations.zh.admin.systemSettings.privacy.loadFailed,
        })}
      </section>
    </div>
  ),
}

export const InteractionContract: Story = {
  render: (args) => <InteractionCanvas {...args} />,
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 120))
    const canvas = within(canvasElement)
    const autoRefreshSwitch = canvas.getByRole('switch', { name: '自动刷新' })
    await expect(autoRefreshSwitch).toHaveAttribute('aria-checked', 'true')

    await userEvent.click(autoRefreshSwitch)
    await expect(autoRefreshSwitch).toHaveAttribute('aria-checked', 'false')

    const details = canvasElement.querySelector<HTMLDetailsElement>('[data-testid="system-status-technical-details"]')
    if (!details) {
      throw new Error('Expected the system status module to expose a technical-details disclosure.')
    }
    if (details.open) {
      throw new Error('Expected the technical-details disclosure to stay collapsed by default.')
    }

    await userEvent.click(canvas.getByRole('button', { name: '立即刷新' }))
    await expect(canvas.getByTestId('system-status-refresh-count')).toHaveTextContent('刷新次数：1')
  },
}
