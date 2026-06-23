import type { StoryObj } from '@storybook/react-vite'

import type { HaStatus } from '../../api'
import HaStatusBanner from '../../components/HaStatusBanner'
import { useLanguage, useTranslate } from '../../i18n'
import { AdminPageFrame, DashboardPageCanvas } from './AdminPagesStoryRuntime'

type Story = StoryObj

const dashboardHaAttentionStatus: HaStatus = {
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

const systemSettingsHaStatus: HaStatus = {
  mode: 'active_standby',
  nodeId: 'node-a',
  nodePublicOrigin: '203.0.113.9:58087',
  role: 'full_master',
  degraded: false,
  allowsBasicBusiness: true,
  allowsFullWrites: true,
  edgeoneDomain: 'api.example.com',
  edgeoneOrigin: '203.0.113.9:58087',
  edgeoneExpectedOrigin: null,
  edgeoneCurrentTarget: '203.0.113.9:58087',
  edgeoneExpectedTarget: null,
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
  syncLagSeconds: 0,
  recoveryStatus: null,
  message: 'full master is ready to drain traffic for planned maintenance',
  peerNodes: [
    {
      nodeId: 'node-b',
      publicOrigin: '203.0.113.10:58087',
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
    {
      nodeId: 'node-c',
      publicOrigin: '203.0.113.11:58087',
      role: 'standby',
      allowsBasicBusiness: false,
      allowsFullWrites: false,
      lastSyncAt: 1_700_000_010,
      syncLagSeconds: 14,
      recoveryStatus: null,
      message: 'peer status probe is older than 30 seconds',
      lastSeenAt: 1_699_999_920,
      stale: true,
      roleHint: 'observer',
      plannedCutoverEligible: false,
    },
  ],
  plannedCutoverEligible: false,
}

function DashboardHaAttentionPageCanvas(): JSX.Element {
  const { language } = useLanguage()
  const admin = useTranslate().admin
  return (
    <DashboardPageCanvas
      beforeIntro={(
        <HaStatusBanner
          status={dashboardHaAttentionStatus}
          audience="admin"
          adminVariant="compact"
          compactHref="/admin/system-settings/ha"
          compactTitle={admin.systemSettings.ha.compactTitle}
          compactDescription={admin.systemSettings.ha.compactDescription}
          compactActionLabel={admin.systemSettings.ha.viewSettings}
        />
      )}
    />
  )
}

function SystemSettingsHaPageCanvas(): JSX.Element {
  const { language } = useLanguage()
  const admin = useTranslate().admin
  return (
    <AdminPageFrame activeModule="system-settings-ha">
      <section className="admin-settings-ha-page">
        <HaStatusBanner
          status={systemSettingsHaStatus}
          audience="admin"
          strings={admin.systemSettings.ha}
          language={language}
          adminVariant="panel"
          onPlannedCutover={() => undefined}
          timeline={[
            {
              id: 501,
              eventKind: 'planned_cutover_succeeded',
              category: 'planned_cutover',
              status: 'success',
              nodeId: 'node-a',
              operationId: 'ha-op-success',
              summary: 'planned cutover completed to node-b',
              detail: 'Target peer finalized and this node moved into recovery.',
              technicalDetails: { targetNodeId: 'node-b' },
              createdAt: 1_700_000_060,
            },
          ]}
          hasMoreTimeline
        />
      </section>
    </AdminPageFrame>
  )
}

export const SystemSettingsHa: Story = {
  render: () => <SystemSettingsHaPageCanvas />,
  parameters: { viewport: { defaultViewport: '1440-device-desktop' } },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 80))
    const text = canvasElement.textContent ?? ''
    if (!text.includes('HA 服务节点') || !text.includes('节点清单') || !text.includes('计划内切流') || !text.includes('7 天时间线')) {
      throw new Error('Expected the HA settings page to render the full service-node panel.')
    }
    const activeSubitem = canvasElement.ownerDocument.querySelector<HTMLElement>('.admin-nav-subitem-active')
    if (!activeSubitem?.textContent?.includes('高可用')) {
      throw new Error('Expected the HA settings sidebar child item to be active.')
    }
  },
}

export const SystemSettingsHaMobile: Story = {
  ...SystemSettingsHa,
  parameters: { viewport: { defaultViewport: '0390-device-iphone-14' } },
}

export const DashboardHaAttention: Story = {
  render: () => <DashboardHaAttentionPageCanvas />,
  parameters: { viewport: { defaultViewport: '1440-device-desktop' } },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 80))
    const text = canvasElement.textContent ?? ''
    if (!text.includes('高可用状态需要关注') || !text.includes('查看 HA 设置')) {
      throw new Error('Expected abnormal HA state to render the compact settings link.')
    }
    if (text.includes('节点清单') || text.includes('提升为主节点')) {
      throw new Error('Expected the compact HA alert to avoid full node inventory and operations.')
    }
  },
}

export const DashboardHaAttentionMobile: Story = {
  ...DashboardHaAttention,
  parameters: { viewport: { defaultViewport: '0390-device-iphone-14' } },
}
