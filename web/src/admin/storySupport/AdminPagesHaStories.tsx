import type { StoryObj } from '@storybook/react-vite'

import type { HaStatus } from '../../api'
import HaStatusBanner from '../../components/HaStatusBanner'
import { useLanguage, useTranslate } from '../../i18n'
import { AdminPageFrame, DashboardPageCanvas } from './AdminPagesStoryRuntime'

type Story = StoryObj

const storyHaStandbyStatus: HaStatus = {
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
}

function DashboardHaAttentionPageCanvas(): JSX.Element {
  const { language } = useLanguage()
  const admin = useTranslate().admin
  return (
    <DashboardPageCanvas
      beforeIntro={(
        <HaStatusBanner
          status={storyHaStandbyStatus}
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
          status={storyHaStandbyStatus}
          audience="admin"
          strings={admin.systemSettings.ha}
          language={language}
          adminVariant="panel"
          onPromote={() => undefined}
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
    if (!text.includes('HA 服务节点') || !text.includes('节点清单')) {
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
