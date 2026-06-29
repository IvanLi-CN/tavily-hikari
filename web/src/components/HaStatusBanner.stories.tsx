import type { Meta, StoryObj } from '@storybook/react-vite'

import type { HaPeerNode, HaSourceSettingsApiError, HaStatus, HaTimelineEvent } from '../api'
import HaSourceSettingsDialog from '../admin/HaSourceSettingsDialog'
import HaStatusBanner from './HaStatusBanner'
import { translations } from '../i18n'

const baseStatus: HaStatus = {
  mode: 'active_standby',
  nodeId: 'node-b',
  nodePublicOrigin: '203.0.113.10:58087',
  role: 'standby',
  degraded: true,
  allowsBasicBusiness: false,
  allowsFullWrites: false,
  edgeoneDomain: 'api.example.com',
  edgeoneOrigin: '203.0.113.9:58087',
  edgeoneExpectedOrigin: '203.0.113.9:58087',
  edgeoneCurrentTarget: '203.0.113.9:58087',
  edgeoneExpectedTarget: '203.0.113.9:58087',
  edgeoneCurrentSourceKind: 'direct',
  edgeoneExpectedSourceKind: 'direct',
  edgeoneCurrentOriginGroupId: null,
  edgeoneExpectedOriginGroupId: null,
  haSourceDefaults: {
    sourceKind: 'direct',
    directOriginScheme: 'https',
    directOriginHost: '203.0.113.9',
    directOriginPort: 58087,
    originGroupId: null,
    target: '203.0.113.9:58087',
  },
  haSourceOverride: null,
  haSourceEffective: {
    sourceKind: 'direct',
    directOriginScheme: 'https',
    directOriginHost: '203.0.113.9',
    directOriginPort: 58087,
    originGroupId: null,
    target: '203.0.113.9:58087',
  },
  edgeoneApiConfigured: true,
  lastEdgeoneCheckAt: 1_700_000_000,
  lastSyncAt: 1_700_000_002,
  syncLagSeconds: 8,
  recoveryStatus: null,
  message: 'standby is synchronized and ready for manual promotion',
  peerNodes: [
    {
      nodeId: 'node-a',
      publicOrigin: '203.0.113.9:58087',
      sourceConfigTarget: '203.0.113.9:58087',
      role: 'full_master',
      allowsBasicBusiness: true,
      allowsFullWrites: true,
      lastSyncAt: 1_700_000_000,
      syncLagSeconds: 2,
      recoveryStatus: null,
      message: 'active node is serving traffic',
      lastSeenAt: 1_700_000_001,
      stale: false,
      roleHint: 'standby_candidate',
      plannedCutoverEligible: false,
    },
  ],
  plannedCutoverEligible: false,
}

const provisionalStatus: HaStatus = {
  ...baseStatus,
  role: 'provisional_master',
  allowsBasicBusiness: true,
  edgeoneOrigin: '203.0.113.10:58087',
  message: 'promoted by EdgeOne origin switch; finalize required',
}

const fullMasterStatus: HaStatus = {
  ...provisionalStatus,
  role: 'full_master',
  degraded: false,
  allowsFullWrites: true,
  edgeoneExpectedOrigin: null,
  message: 'node is serving as active master',
}

const originGroupMasterStatus: HaStatus = {
  ...fullMasterStatus,
  edgeoneCurrentTarget: 'eo-origin-group-ha-demo',
  edgeoneExpectedTarget: 'eo-origin-group-ha-demo',
  edgeoneCurrentSourceKind: 'origin_group',
  edgeoneExpectedSourceKind: 'origin_group',
  edgeoneCurrentOriginGroupId: 'eo-origin-group-ha-demo',
  edgeoneExpectedOriginGroupId: 'eo-origin-group-ha-demo',
  edgeoneOrigin: 'eo-origin-group-ha-demo',
  edgeoneExpectedOrigin: null,
  haSourceOverride: {
    sourceKind: 'origin_group',
    directOriginScheme: null,
    directOriginHost: null,
    directOriginPort: null,
    originGroupId: 'eo-origin-group-ha-demo',
    target: 'eo-origin-group-ha-demo',
  },
  haSourceEffective: {
    sourceKind: 'origin_group',
    directOriginScheme: null,
    directOriginHost: null,
    directOriginPort: null,
    originGroupId: 'eo-origin-group-ha-demo',
    target: 'eo-origin-group-ha-demo',
  },
  message: 'node is serving through an EdgeOne origin group',
}

const recoveryStatus: HaStatus = {
  ...baseStatus,
  nodeId: 'node-a',
  nodePublicOrigin: '203.0.113.9:58087',
  role: 'recovery',
  edgeoneOrigin: '203.0.113.10:58087',
  edgeoneExpectedOrigin: '203.0.113.9:58087',
  recoveryStatus: 'importing old-master-batch-1',
  message: 'EdgeOne origin moved to 203.0.113.10:58087; recovery import required',
}

const eligiblePeer: HaPeerNode = {
  nodeId: 'node-b',
  publicOrigin: '203.0.113.10:58087',
  sourceConfigTarget: '203.0.113.10:58087',
  role: 'standby',
  allowsBasicBusiness: false,
  allowsFullWrites: false,
  lastSyncAt: 1_700_000_018,
  syncLagSeconds: 4,
  recoveryStatus: null,
  message: 'standby is synchronized and ready',
  lastSeenAt: 1_700_000_020,
  stale: false,
  roleHint: 'standby_candidate',
  plannedCutoverEligible: true,
}

const stalePeer: HaPeerNode = {
  ...eligiblePeer,
  nodeId: 'node-c',
  publicOrigin: '203.0.113.11:58087',
  stale: true,
  plannedCutoverEligible: false,
  message: 'peer status probe is older than 30 seconds',
}

const unreachablePeer: HaPeerNode = {
  ...eligiblePeer,
  nodeId: 'node-d',
  publicOrigin: '203.0.113.12:58087',
  role: 'recovery',
  recoveryStatus: 'peer unreachable',
  stale: true,
  plannedCutoverEligible: false,
  message: 'peer internal status endpoint is unreachable',
}

const lagBlockedPeer: HaPeerNode = {
  ...eligiblePeer,
  nodeId: 'node-e',
  publicOrigin: '203.0.113.13:58087',
  syncLagSeconds: 61,
  plannedCutoverEligible: false,
  message: 'sync lag exceeds 30 seconds threshold',
}

const runningTimeline: HaTimelineEvent[] = [
  {
    id: 401,
    eventKind: 'planned_cutover_started',
    category: 'planned_cutover',
    status: 'running',
    nodeId: 'node-a',
    operationId: 'ha-op-running',
    summary: 'planned cutover to node-b is running',
    detail: 'EdgeOne route already points to node-b; waiting for finalize.',
    technicalDetails: { phase: 'await_peer_finalize', targetNodeId: 'node-b' },
    createdAt: 1_700_000_050,
  },
]

const completedTimeline: HaTimelineEvent[] = [
  {
    id: 501,
    eventKind: 'planned_cutover_succeeded',
    category: 'planned_cutover',
    status: 'success',
    nodeId: 'node-a',
    operationId: 'ha-op-success',
    summary: 'planned cutover completed to node-b',
    detail: 'Target peer finalized and this node moved into recovery.',
    technicalDetails: { targetNodeId: 'node-b', oldRole: 'full_master', newRole: 'recovery' },
    createdAt: 1_700_000_060,
  },
  {
    id: 500,
    eventKind: 'edgeone_modify_acceleration_domain',
    category: 'edgeone',
    status: 'success',
    nodeId: 'node-a',
    operationId: 'ha-op-success',
    summary: 'EdgeOne ModifyAccelerationDomain succeeded',
    detail: 'Route switched to node-b.',
    technicalDetails: { requestId: 'edgeone-demo-request' },
    createdAt: 1_700_000_058,
  },
]

const failedTimeline: HaTimelineEvent[] = [
  {
    id: 601,
    eventKind: 'planned_cutover_rejected_precheck',
    category: 'planned_cutover',
    status: 'error',
    nodeId: 'node-a',
    operationId: 'ha-op-failed',
    summary: 'planned cutover precheck rejected node-e',
    detail: 'The standby candidate exceeded the sync lag threshold.',
    technicalDetails: { targetNodeId: 'node-e', syncLagSeconds: 61 },
    createdAt: 1_700_000_070,
  },
]

const cutoverReadyStatus: HaStatus = {
  ...fullMasterStatus,
  nodeId: 'node-a',
  nodePublicOrigin: '203.0.113.9:58087',
  edgeoneOrigin: '203.0.113.9:58087',
  edgeoneCurrentTarget: '203.0.113.9:58087',
  edgeoneExpectedOrigin: null,
  edgeoneExpectedTarget: null,
  lastSyncAt: 1_700_000_020,
  syncLagSeconds: 0,
  message: 'full master is ready to drain traffic for planned maintenance',
  peerNodes: [eligiblePeer, stalePeer, unreachablePeer, lagBlockedPeer],
}

const cutoverRunningStatus: HaStatus = {
  ...cutoverReadyStatus,
  message: 'planned cutover is in progress',
  peerNodes: [
    {
      ...eligiblePeer,
      plannedCutoverEligible: false,
      role: 'provisional_master',
      message: 'peer is waiting for finalize',
    },
  ],
}

const cutoverSuccessStatus: HaStatus = {
  ...recoveryStatus,
  peerNodes: [
    {
      ...eligiblePeer,
      role: 'full_master',
      allowsBasicBusiness: true,
      allowsFullWrites: true,
      plannedCutoverEligible: false,
      message: 'planned cutover completed; node-b now serves traffic',
    },
  ],
}

function StoryFrame({ children }: { children: JSX.Element }): JSX.Element {
  return (
    <div style={{ maxWidth: 1280, margin: '0 auto', display: 'grid', gap: 18 }}>
      {children}
    </div>
  )
}

function StateGallery(): JSX.Element {
  return (
    <StoryFrame>
      <>
        <HaStatusBanner
          status={baseStatus}
          audience="admin"
          strings={translations.zh.admin.systemSettings.ha}
          language="zh"
          onPromote={() => undefined}
        />
        <HaStatusBanner
          status={provisionalStatus}
          audience="admin"
          strings={translations.zh.admin.systemSettings.ha}
          language="zh"
          onFinalize={() => undefined}
        />
        <HaStatusBanner status={fullMasterStatus} audience="admin" strings={translations.zh.admin.systemSettings.ha} language="zh" />
        <HaStatusBanner status={recoveryStatus} audience="admin" strings={translations.zh.admin.systemSettings.ha} language="zh" />
        <HaStatusBanner
          status={cutoverReadyStatus}
          audience="admin"
          strings={translations.zh.admin.systemSettings.ha}
          language="zh"
          onPlannedCutover={() => undefined}
          timeline={completedTimeline}
        />
      </>
    </StoryFrame>
  )
}

const meta = {
  title: 'Components/HaStatusBanner',
  component: HaStatusBanner,
  tags: ['autodocs'],
  parameters: {
    layout: 'padded',
    docs: {
      description: {
        component:
          'HA admin management panel with service-node rows, EdgeOne origin state, and the local promote/finalize entry point. The user audience still receives only the degraded-service notice.',
      },
    },
  },
  args: {
    status: baseStatus,
    audience: 'admin',
    strings: translations.zh.admin.systemSettings.ha,
    language: 'zh',
    onPromote: () => undefined,
  },
  render: (args) => (
    <StoryFrame>
      <HaStatusBanner {...args} />
    </StoryFrame>
  ),
} satisfies Meta<typeof HaStatusBanner>

export default meta
type Story = StoryObj<typeof meta>

export const NodeListGallery: Story = {
  render: () => <StateGallery />,
  parameters: {
    docs: {
      description: {
        story: 'Curated admin states covering standby promotion, provisional finalize, full master, and recovery rows.',
      },
    },
  },
  play: async ({ canvasElement }) => {
    const text = canvasElement.textContent ?? ''
    for (const expected of ['节点清单', '提升为主节点', '完成主切换', '计划内切流', '7 天时间线']) {
      if (!text.includes(expected)) {
        throw new Error(`Expected HA node list gallery to contain: ${expected}`)
      }
    }
  },
}

export const StandbyAdmin: Story = {
  play: async ({ canvasElement }) => {
    const text = canvasElement.textContent ?? ''
    for (const expected of ['HA 服务节点', 'node-b', '当前管理节点', '提升为主节点']) {
      if (!text.includes(expected)) {
        throw new Error(`Expected standby admin story to contain: ${expected}`)
      }
    }
  },
}

export const ProvisionalAdmin: Story = {
  args: {
    status: provisionalStatus,
    onPromote: undefined,
    onFinalize: () => undefined,
  },
}

export const FullMasterAdmin: Story = {
  args: {
    status: fullMasterStatus,
    onPromote: undefined,
  },
}

export const PlannedCutoverReady: Story = {
  args: {
    status: cutoverReadyStatus,
    onPromote: undefined,
    onPlannedCutover: () => undefined,
    timeline: completedTimeline,
    onLoadMoreTimeline: () => undefined,
    hasMoreTimeline: true,
  },
  play: async ({ canvasElement }) => {
    const text = canvasElement.textContent ?? ''
    for (const expected of ['计划内切流', 'node-b', '备用节点探测正常', '状态过期', '需要恢复', '加载更多']) {
      if (!text.includes(expected)) {
        throw new Error(`Expected planned cutover ready story to contain: ${expected}`)
      }
    }
  },
}

export const PlannedCutoverRunning: Story = {
  args: {
    status: cutoverRunningStatus,
    onPromote: undefined,
    timeline: runningTimeline,
  },
}

export const NodeSourceConfigProof: Story = {
  args: {
    status: {
      ...fullMasterStatus,
      nodeId: 'gz-101',
      nodePublicOrigin: 'tavily-alt.ivanli.cc:443',
      edgeoneCurrentTarget: 'edgeone-live-route.example.com:443',
      edgeoneExpectedTarget: 'edgeone-live-route.example.com:443',
      edgeoneOrigin: 'edgeone-live-route.example.com:443',
      edgeoneExpectedOrigin: null,
      haSourceEffective: {
        sourceKind: 'direct',
        directOriginScheme: 'https',
        directOriginHost: 'gz.ivanli.cc',
        directOriginPort: 1443,
        originGroupId: null,
        target: 'gz.ivanli.cc:1443',
      },
      peerNodes: [
        {
          nodeId: 'hinet-lam-standby',
          publicOrigin: 'tavily-tw.ivanli.cc:443',
          sourceConfigTarget: 'hinet-ep.707979.xyz:53844',
          role: 'standby',
          allowsBasicBusiness: false,
          allowsFullWrites: false,
          lastSyncAt: 1_700_000_018,
          syncLagSeconds: 4,
          recoveryStatus: null,
          message: 'standby is synchronized and ready',
          lastSeenAt: 1_700_000_020,
          stale: false,
          roleHint: 'standby_candidate',
          plannedCutoverEligible: true,
        },
      ],
    },
    onPromote: undefined,
    onPlannedCutover: () => undefined,
    timeline: completedTimeline,
  },
  parameters: {
    docs: {
      description: {
        story:
          'Proof state showing the node inventory origin column bound to node source configuration rather than the live EdgeOne route or the peer direct-entry domain.',
      },
    },
  },
  play: async ({ canvasElement }) => {
    const text = canvasElement.textContent ?? ''
    for (const expected of [
      'gz.ivanli.cc:1443',
      'hinet-ep.707979.xyz:53844',
      'edgeone-live-route.example.com:443',
    ]) {
      if (!text.includes(expected)) {
        throw new Error(`Expected HA node source config proof story to contain: ${expected}`)
      }
    }
  },
}

export const PlannedCutoverFailed: Story = {
  args: {
    status: cutoverReadyStatus,
    onPromote: undefined,
    timeline: failedTimeline,
  },
}

export const TimelineEmpty: Story = {
  args: {
    status: cutoverReadyStatus,
    onPromote: undefined,
    onPlannedCutover: () => undefined,
    timeline: [],
  },
}

export const OriginGroupSourceDialog: Story = {
  render: () => (
    <StoryFrame>
      <>
        <HaStatusBanner
          status={originGroupMasterStatus}
          audience="admin"
          strings={translations.zh.admin.systemSettings.ha}
          language="zh"
        />
        <HaSourceSettingsDialog
          open
          status={originGroupMasterStatus}
          strings={translations.zh.admin.systemSettings.ha}
          onOpenChange={() => undefined}
          onSaved={() => undefined}
        />
      </>
    </StoryFrame>
  ),
  parameters: {
    docs: {
      description: {
        story: 'Source settings dialog opened on an active node using an EdgeOne origin group.',
      },
    },
  },
  play: async ({ canvasElement }) => {
    const text = canvasElement.textContent ?? ''
    for (const expected of ['配置源站', '源站组', '源站组 ID', '保存并切换 EdgeOne 到此源站']) {
      if (!text.includes(expected)) {
        throw new Error(`Expected HA source dialog story to contain: ${expected}`)
      }
    }
  },
}

export const DirectSourceDialog: Story = {
  render: () => {
    const directStatus: HaStatus = {
      ...originGroupMasterStatus,
      edgeoneCurrentTarget: '203.0.113.9:58087',
      edgeoneExpectedTarget: '203.0.113.9:58087',
      edgeoneCurrentSourceKind: 'direct',
      edgeoneExpectedSourceKind: 'direct',
      edgeoneCurrentOriginGroupId: null,
      edgeoneExpectedOriginGroupId: null,
      edgeoneOrigin: '203.0.113.9:58087',
      haSourceOverride: {
        sourceKind: 'direct',
        directOriginScheme: 'https',
        directOriginHost: '203.0.113.9',
        directOriginPort: 58087,
        originGroupId: null,
        target: '203.0.113.9:58087',
      },
      haSourceEffective: {
        sourceKind: 'direct',
        directOriginScheme: 'https',
        directOriginHost: '203.0.113.9',
        directOriginPort: 58087,
        originGroupId: null,
        target: '203.0.113.9:58087',
      },
      message: 'node is serving through a direct IP/domain origin',
    }

    return (
      <StoryFrame>
        <>
          <HaStatusBanner
            status={directStatus}
            audience="admin"
            strings={translations.zh.admin.systemSettings.ha}
            language="zh"
          />
          <HaSourceSettingsDialog
            open
            status={directStatus}
            strings={translations.zh.admin.systemSettings.ha}
            onOpenChange={() => undefined}
            onSaved={() => undefined}
          />
        </>
      </StoryFrame>
    )
  },
  parameters: {
    docs: {
      description: {
        story: 'Source settings dialog opened on an active node using a direct IP/domain origin.',
      },
    },
  },
  play: async ({ canvasElement }) => {
    const text = canvasElement.textContent ?? ''
    for (const expected of ['配置源站', 'IP/域名', '已选 IP/域名', 'HTTPS · 203.0.113.9:58087']) {
      if (!text.includes(expected)) {
        throw new Error(`Expected direct HA source dialog story to contain: ${expected}`)
      }
    }
  },
}

export const StandbySourceDialog: Story = {
  render: () => (
    <StoryFrame>
      <>
        <HaStatusBanner
          status={baseStatus}
          audience="admin"
          strings={translations.zh.admin.systemSettings.ha}
          language="zh"
        />
        <HaSourceSettingsDialog
          open
          status={baseStatus}
          strings={translations.zh.admin.systemSettings.ha}
          onOpenChange={() => undefined}
          onSaved={() => undefined}
        />
      </>
    </StoryFrame>
  ),
  parameters: {
    docs: {
      description: {
        story: 'Source settings dialog opened on a standby node; it can save the local source only and cannot switch EdgeOne.',
      },
    },
  },
  play: async ({ canvasElement }) => {
    const text = canvasElement.textContent ?? ''
    for (const expected of ['配置源站', '备用节点', '仅保存']) {
      if (!text.includes(expected)) {
        throw new Error(`Expected standby HA source dialog story to contain: ${expected}`)
      }
    }
    if (text.includes('保存并切换 EdgeOne 到此源站')) {
      throw new Error('Expected standby HA source dialog to omit the EdgeOne switch action.')
    }
  },
}

export const SourceDialogSubmitFailure: Story = {
  render: () => {
    const submitSourceSettings = async (): Promise<HaStatus> => {
      const error = new Error('Failed to deserialize the JSON body into the target type') as HaSourceSettingsApiError
      error.status = 400
      error.rawDetail =
        'Failed to deserialize the JSON body into the target type: directOriginScheme: unknown variant `https`'
      throw error
    }

    return (
      <StoryFrame>
        <>
          <HaStatusBanner
            status={fullMasterStatus}
            audience="admin"
            strings={translations.zh.admin.systemSettings.ha}
            language="zh"
          />
          <HaSourceSettingsDialog
            open
            status={fullMasterStatus}
            strings={translations.zh.admin.systemSettings.ha}
            onOpenChange={() => undefined}
            onSaved={() => undefined}
            submitSourceSettings={submitSourceSettings}
          />
        </>
      </StoryFrame>
    )
  },
  parameters: {
    docs: {
      description: {
        story: 'Source settings dialog showing the destructive submit-failure alert with operator-friendly guidance and technical details.',
      },
    },
  },
  play: async ({ canvasElement }) => {
    const documentRoot = canvasElement.ownerDocument
    const applyButton = Array.from(documentRoot.querySelectorAll('button')).find(
      (button) => button.textContent?.trim() === translations.zh.admin.systemSettings.ha.sourceSaveAndApply,
    )
    if (!applyButton) {
      throw new Error('Expected submit-failure HA source dialog story to expose the EdgeOne switch action.')
    }

    applyButton.click()
    await new Promise<void>((resolve) => setTimeout(resolve, 32))
    await new Promise<void>((resolve) => setTimeout(resolve, 32))

    const text = documentRoot.body.textContent ?? ''
    for (const expected of [
      translations.zh.admin.systemSettings.ha.sourceApplyFailedTitle,
      translations.zh.admin.systemSettings.ha.sourceSubmitFailedDirectDescription,
      translations.zh.admin.systemSettings.ha.sourceTechnicalDetailsLabel,
    ]) {
      if (!text.includes(expected)) {
        throw new Error(`Expected submit-failure HA source dialog story to contain: ${expected}`)
      }
    }
  },
}

export const RecoveryAdmin: Story = {
  args: {
    status: cutoverSuccessStatus,
    onPromote: undefined,
    timeline: completedTimeline,
  },
}

export const CompactAdminAttention: Story = {
  args: {
    adminVariant: 'compact',
    compactHref: '/admin/system-settings/ha',
    compactTitle: translations.zh.admin.systemSettings.ha.compactTitle,
    compactDescription: translations.zh.admin.systemSettings.ha.compactDescription,
    compactActionLabel: translations.zh.admin.systemSettings.ha.viewSettings,
  },
  play: async ({ canvasElement }) => {
    const text = canvasElement.textContent ?? ''
    for (const expected of ['高可用状态需要关注', '查看 HA 设置']) {
      if (!text.includes(expected)) {
        throw new Error(`Expected compact HA alert to contain: ${expected}`)
      }
    }
    if (text.includes('节点清单') || text.includes('提升为主节点')) {
      throw new Error('Expected compact HA alert to omit node inventory and operations.')
    }
  },
}

export const UserDegraded: Story = {
  args: {
    audience: 'user',
    status: provisionalStatus,
    onPromote: undefined,
  },
}
