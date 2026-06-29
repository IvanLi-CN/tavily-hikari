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
  },
  edgeoneDomain: 'api.example.com',
  edgeoneCurrentTarget: '203.0.113.9:58087',
  edgeoneCurrentSourceKind: 'direct',
  haSourceEffective: {
    sourceKind: 'direct',
    directOriginScheme: 'https',
    directOriginHost: '203.0.113.9',
    directOriginPort: 58087,
    originGroupId: null,
    target: '203.0.113.9:58087',
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
  args: {
    detail,
    strings: translations.zh.admin.systemSettings.ha,
    language: 'zh',
    onBack: () => undefined,
    onConfigureSource: () => undefined,
    hasMoreTimeline: false,
  },
} satisfies Meta<typeof HaNodeDetailPanel>

export default meta
type Story = StoryObj<typeof meta>

export const Default: Story = {}
