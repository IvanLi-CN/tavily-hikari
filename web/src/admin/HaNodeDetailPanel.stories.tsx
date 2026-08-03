import type { Meta, StoryObj } from '@storybook/react-vite'

import type { HaNodeDetail } from '../api'
import HaNodeDetailPanel from './HaNodeDetailPanel'
import { translations } from '../i18n'

const detail: HaNodeDetail = {
  currentNodeId: 'node-a',
  node: {
    nodeId: 'node-b',
    publicOrigin: '203.0.113.10:58087',
    sourceConfigTarget: '203.0.113.10:58087',
    role: 'standby',
    allowsBasicBusiness: true,
    allowsFullWrites: false,
    lastSyncAt: 1_700_000_018,
    syncLagSeconds: 4,
    recoveryStatus: null,
    message: 'standby is synchronized and ready for maintenance cutover',
    lastSeenAt: 1_700_000_020,
    stale: false,
    roleHint: 'standby_candidate',
    plannedCutoverEligible: true,
    channelHealth: [
      { channel: 'control', ackedSeq: 812, highWatermark: 812, ackLag: 0, cursorState: 'healthy', retentionSecs: 72 * 60 * 60, expiredBacklog: false, gcState: 'idle', oldestAgeSecs: 640, lastProgressAt: 1_700_000_010, lastDeferReason: null, nextRetryAt: null, batchSize: 250, gcDebtMode: 'normal', gcObservedAt: 1_700_000_020, gcDeletedRowsPerMinute: 0, gcRecoveryDeadlineAt: null, gcSloState: 'clear', gcForegroundRps: 2 },
      { channel: 'billing', ackedSeq: 490, highWatermark: 512, ackLag: 22, cursorState: 'catching_up', retentionSecs: 14 * 24 * 60 * 60, expiredBacklog: false, gcState: 'draining', oldestAgeSecs: 93_200, lastProgressAt: 1_700_000_016, lastDeferReason: null, nextRetryAt: 1_700_000_050, batchSize: 125, gcDebtMode: 'recovering', gcObservedAt: 1_700_000_020, gcDeletedRowsPerMinute: 132, gcRecoveryDeadlineAt: 1_700_086_420, gcSloState: 'on_track', gcForegroundRps: 1 },
      { channel: 'runtime', ackedSeq: null, highWatermark: 900, ackLag: null, cursorState: 'expired_backlog', retentionSecs: 14 * 24 * 60 * 60, expiredBacklog: true, gcState: 'deferred', oldestAgeSecs: 192_000, lastProgressAt: 1_699_999_800, lastDeferReason: 'foreground_activity', nextRetryAt: 1_700_000_050, batchSize: 62, gcDebtMode: 'foreground_pressure', gcObservedAt: 1_700_000_020, gcDeletedRowsPerMinute: 8, gcRecoveryDeadlineAt: null, gcSloState: 'breached', gcForegroundRps: 18 },
    ],
  },
  timeline: {
    events: [
      {
        id: 700,
        eventKind: 'planned_cutover_started',
        category: 'planned_cutover',
        status: 'running',
        nodeId: 'node-b',
        operationId: 'ha-op-700',
        summary: 'node-a started planned cutover to node-b',
        detail: 'EdgeOne already points to node-b and the control plane is waiting for finalize.',
        technicalDetails: { currentNodeId: 'node-a', targetNodeId: 'node-b' },
        createdAt: 1_700_000_070,
      },
      {
        id: 699,
        eventKind: 'edgeone_modifyaccelerationdomain',
        category: 'edgeone',
        status: 'success',
        nodeId: null,
        operationId: 'ha-op-700',
        summary: 'EdgeOne ModifyAccelerationDomain switched traffic',
        detail: 'The control plane updated the effective route to node-b.',
        technicalDetails: { domain: 'api.example.com' },
        createdAt: 1_700_000_068,
      },
    ],
    nextCursor: null,
  },
}

const meta = {
  title: 'Admin/HaNodeDetailPanel',
  component: HaNodeDetailPanel,
  tags: ['autodocs'],
  parameters: {
    layout: 'fullscreen',
    docs: {
      description: {
        component:
          'Peer-scoped HA node detail showing the selected node, channel health, and interaction history with the current management node.',
      },
    },
  },
  decorators: [
    (Story) => (
      <div
        style={{
          minHeight: '100vh',
          boxSizing: 'border-box',
          padding: 'clamp(16px, 3vw, 48px)',
          background: '#eef0f4',
        }}
      >
        <div
          style={{
            boxSizing: 'border-box',
            padding: 'clamp(16px, 2vw, 28px)',
            background: '#f7f5fb',
          }}
        >
          <Story />
        </div>
      </div>
    ),
  ],
  args: {
    detail,
    strings: translations.zh.admin.systemSettings.ha,
    language: 'zh',
    onBack: () => undefined,
    hasMoreTimeline: false,
  },
} satisfies Meta<typeof HaNodeDetailPanel>

export default meta
type Story = StoryObj<typeof meta>

export const Default: Story = {}

export const Recovering: Story = {
  args: {
    detail: {
      ...detail,
      node: {
        ...detail.node,
        channelHealth: detail.node.channelHealth?.map((health) => health.channel === 'billing'
          ? { ...health, gcDebtMode: 'recovering', gcState: 'draining', gcSloState: 'on_track', gcForegroundRps: 1 }
          : health),
      },
    },
  },
}

export const Deferred: Story = {
  args: {
    detail: {
      ...detail,
      node: {
        ...detail.node,
        channelHealth: detail.node.channelHealth?.map((health) => health.channel === 'runtime'
          ? { ...health, gcDebtMode: 'foreground_pressure', gcState: 'deferred', lastDeferReason: 'foreground_activity', gcForegroundRps: 18 }
          : health),
      },
    },
  },
}

export const Unknown: Story = {
  args: {
    detail: {
      ...detail,
      node: {
        ...detail.node,
        channelHealth: detail.node.channelHealth?.map((health) => ({
          ...health,
          gcDebtMode: 'unknown',
          gcObservedAt: null,
          gcSloState: 'unknown',
          gcRecoveryDeadlineAt: null,
        })),
      },
    },
  },
}

export const BaselineRequired: Story = {
  args: {
    detail: {
      ...detail,
      node: {
        ...detail.node,
        channelHealth: detail.node.channelHealth?.map((health) => health.channel === 'runtime'
          ? { ...health, cursorState: 'baseline_required', expiredBacklog: false }
          : health),
      },
    },
  },
}

export const Stalled: Story = {
  args: {
    detail: {
      ...detail,
      node: {
        ...detail.node,
        channelHealth: detail.node.channelHealth?.map((health) => health.channel === 'runtime'
          ? { ...health, gcState: 'stalled', lastDeferReason: 'consecutive_no_progress', nextRetryAt: 1_700_000_080 }
          : health),
      },
    },
  },
}

export const Mobile: Story = {
  parameters: {
    viewport: { defaultViewport: '0390-device-iphone-14' },
    docs: {
      description: {
        story: 'The peer-scoped detail remains readable at the mobile viewport without local EdgeOne configuration controls.',
      },
    },
  },
}
