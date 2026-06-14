import type { Meta, StoryObj } from '@storybook/react-vite'

import type { HaStatus } from '../api'
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
    for (const expected of ['节点清单', '提升为主节点', '完成主切换', '请使用该节点管理端']) {
      if (!text.includes(expected)) {
        throw new Error(`Expected HA node list gallery to contain: ${expected}`)
      }
    }
  },
}

export const StandbyAdmin: Story = {
  play: async ({ canvasElement }) => {
    const text = canvasElement.textContent ?? ''
    for (const expected of ['HA 服务节点', 'node-b', '已配置的对端', '提升为主节点']) {
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

export const OriginGroupSourceDialog: Story = {
  render: () => (
    <StoryFrame>
      <>
        <HaStatusBanner
          status={originGroupMasterStatus}
          audience="admin"
          strings={translations.zh.admin.systemSettings.ha}
          language="zh"
          onConfigureSource={() => undefined}
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
            onConfigureSource={() => undefined}
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
          onConfigureSource={() => undefined}
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

export const RecoveryAdmin: Story = {
  args: {
    status: recoveryStatus,
    onPromote: undefined,
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
