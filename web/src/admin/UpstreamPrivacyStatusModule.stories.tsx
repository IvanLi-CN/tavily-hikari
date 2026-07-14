import type { Meta, StoryObj } from '@storybook/react-vite'

import UpstreamPrivacyStatusModule from './UpstreamPrivacyStatusModule'
import type { UpstreamPrivacyStatus } from '../api'
import { translations } from '../i18n'

const readyStatus: UpstreamPrivacyStatus = {
  phase: 'pending',
  configuredProjectIdMode: 'accessToken',
  effectiveProjectIdMode: 'accessToken',
  fixedProjectIdConfigured: false,
  configuredMcpUserAgent: '',
  effectiveMcpUserAgent: null,
  httpAllowedHeaders: ['accept', 'accept-encoding', 'content-type', 'x-project-id (policy injected)'],
  controlMcpAllowedHeaders: ['accept', 'cache-control', 'mcp-protocol-version', 'mcp-session-id', 'user-agent (configured only)'],
  gates: [
    { key: 'accessTokenMode', ready: true, detail: 'AccessToken' },
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

const meta = {
  title: 'Admin/UpstreamPrivacyStatusModule',
  component: UpstreamPrivacyStatusModule,
  parameters: {
    layout: 'padded',
  },
  args: {
    strings: translations.zh.admin.systemSettings.privacy,
    formStrings: translations.zh.admin.systemSettings.form,
    language: 'zh',
    status: readyStatus,
    loadState: 'ready',
    error: null,
    refreshing: false,
    autoRefreshEnabled: true,
    onAutoRefreshChange: () => {},
    onRefresh: () => {},
  },
} satisfies Meta<typeof UpstreamPrivacyStatusModule>

export default meta

type Story = StoryObj<typeof meta>

export const Ready: Story = {}

export const Active: Story = {
  args: {
    status: {
      ...readyStatus,
      phase: 'active',
      completedGates: 4,
      activeControlSessions: 0,
      pendingResearch: 0,
      queuedSettlements: 0,
      gates: readyStatus.gates.map((gate) => ({
        ...gate,
        ready: true,
        detail: gate.key === 'controlSessionsDrained' ? '0' : gate.detail,
      })),
      recentAdjustments: [],
    },
  },
}

export const CustomControlUserAgentConfigured: Story = {
  args: {
    status: {
      ...readyStatus,
      phase: 'configured',
      configuredMcpUserAgent: 'custom-control-mcp',
      effectiveMcpUserAgent: 'custom-control-mcp',
    },
  },
}

export const Loading: Story = {
  args: {
    status: null,
    loadState: 'initial_loading',
  },
}

export const Degraded: Story = {
  args: {
    status: {
      ...readyStatus,
      phase: 'degraded',
      completedGates: 4,
      activeControlSessions: 0,
      pendingResearch: 0,
      degradedSettlements: 1,
      nextEpochAt: 1_783_994_400,
      gates: readyStatus.gates.map((gate) => ({ ...gate, ready: true, detail: gate.key === 'controlSessionsDrained' ? '0' : gate.detail })),
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
    },
  },
}
