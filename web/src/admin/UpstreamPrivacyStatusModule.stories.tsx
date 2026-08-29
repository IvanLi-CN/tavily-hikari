import type { Meta, StoryObj } from '@storybook/react-vite'
import { useState, type ComponentProps } from 'react'
import { expect, userEvent, within } from 'storybook/test'

import UpstreamPrivacyStatusModule from './UpstreamPrivacyStatusModule'
import type { UpstreamPrivacyStatus } from '../api'
import { translations } from '../i18n'

type StoryArgs = ComponentProps<typeof UpstreamPrivacyStatusModule>

const desktopViewport = { viewport: { defaultViewport: '1440-device-desktop' } } as const
const mobileViewport = { viewport: { defaultViewport: '0393-admin-mobile' } } as const

const congestedBoundUsers = Array.from({ length: 14 }, (_, index) => ({
  keyIdHint: `key-${String(index + 1).padStart(2, '0')}`,
  count: Math.max(1, 28 - index * 2),
}))

const congestedPendingProjects = Array.from({ length: 15 }, (_, index) => ({
  keyIdHint: `key-${String(index + 1).padStart(2, '0')}`,
  count: Math.max(2, 72 - index * 4),
}))

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
    { key: 'apiRebalance', ready: true, detail: 'enabled' },
    { key: 'mcpRebalance', ready: true, detail: 'enabled' },
    { key: 'controlSessionsDrained', ready: false, detail: '2' },
  ],
  completedGates: 3,
  totalGates: 4,
  activeUpstreamMcpSessions: 2,
  currentPeriodCode: '2026-07-14/S2',
  currentPeriodEndsAt: 1_783_994_400,
  nextEpochAt: 1_783_994_400,
  pendingResearch: 1,
  queuedSettlements: 2,
  degradedSettlements: 0,
  degradedSettlementsCapped: false,
  lastReconciliationRunAt: 1_783_958_250,
  lastShadowAdjustmentAt: 1_783_958_100,
  lastReconciliationEnqueueErrorAt: 1_783_957_900,
  lastResearchSweepAt: 1_783_958_320,
  lastResearchTerminalAt: 1_783_958_300,
  reconciliationLastNoAdjustment: 0,
  reconciliationPressureStreak: 0,
  reconciliationBackoffLevel: 0,
  reconciliationBackoffUntil: null,
  reconciliationObservation: {
    observedAt: 1_783_958_320,
    coverage: 'bounded',
    queueEstimate: null,
    hasEligible: true,
    oldestCandidateAgeSecs: 3_600,
  },
  reconciliationLocalBackoff: {
    pressureStreak: 0,
    level: 0,
    availableAt: null,
    lastRecoveredAt: null,
  },
  reconciliationController: {
    mode: 'compare',
    activationPeriodCode: null,
    activationPeriodStart: null,
    legacyActive: false,
    pausedReason: null,
    transitionedAt: 1_783_958_320,
  },
  dashboardAlertProjection: {
    coverage: 'ok',
    observedAt: 1_783_958_320,
    staleReason: null,
  },
  coverage: 'ok',
  observedAt: 1_783_958_320,
  staleReason: null,
  retryBuckets: {
    upstream429: 3,
    localUsageRateLimit: 1,
    other: 0,
  },
  currentPeriodBoundUsersByKey: [
    { keyIdHint: 'key-primary', count: 12 },
    { keyIdHint: 'key-backup', count: 5 },
    { keyIdHint: 'key-eu-west', count: 3 },
  ],
  currentPeriodPendingProjectIdsByKey: [
    { keyIdHint: 'key-primary', count: 28 },
    { keyIdHint: 'key-backup', count: 9 },
    { keyIdHint: 'key-eu-west', count: 4 },
  ],
  dailyReconciliationProgress: {
    observedAccounts: 18,
    accountsWithSettledPeriod: 7,
    fullyTerminalAccounts: 10,
    observedPeriods: 42,
    settledPeriods: 16,
    degradedPeriods: 2,
    pendingPeriods: 24,
    researchTotal: 31,
    researchTerminal: 12,
    researchPending: 19,
  },
  dailyReconciliationByKey: [
    {
      keyIdHint: 'key-primary',
      terminalResearch: 7,
      pendingResearch: 14,
      pendingProjectIds: 28,
      cooldownUntil: 1_783_960_000,
      cooldownReason: 'upstream429',
    },
    {
      keyIdHint: 'key-backup',
      terminalResearch: 5,
      pendingResearch: 5,
      pendingProjectIds: 9,
      cooldownUntil: null,
      cooldownReason: null,
    },
  ],
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
  activeUpstreamMcpSessions: 0,
  pendingResearch: 0,
  queuedSettlements: 0,
  gates: pendingStatus.gates.map((gate) => ({
    ...gate,
    ready: true,
    detail: gate.key === 'controlSessionsDrained' ? '0' : gate.detail,
  })),
  recentAdjustments: [],
  lastReconciliationRunAt: 1_783_958_500,
  lastShadowAdjustmentAt: 1_783_958_100,
  lastReconciliationEnqueueErrorAt: null,
  lastResearchSweepAt: 1_783_958_500,
  lastResearchTerminalAt: 1_783_958_450,
  retryBuckets: {
    upstream429: 0,
    localUsageRateLimit: 0,
    other: 0,
  },
  currentPeriodBoundUsersByKey: [],
  currentPeriodPendingProjectIdsByKey: [],
  dailyReconciliationProgress: {
    observedAccounts: 9,
    accountsWithSettledPeriod: 9,
    fullyTerminalAccounts: 9,
    observedPeriods: 13,
    settledPeriods: 13,
    degradedPeriods: 0,
    pendingPeriods: 0,
    researchTotal: 4,
    researchTerminal: 4,
    researchPending: 0,
  },
  dailyReconciliationByKey: [],
}

const compareBlockedStatus: UpstreamPrivacyStatus = {
  ...pendingStatus,
  phase: 'compare',
  completedGates: 3,
  activeUpstreamMcpSessions: 5,
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

const activePausedStatus: UpstreamPrivacyStatus = {
  ...activeStatus,
  phase: 'active_paused',
  upstreamPreciseReconciliationEnabled: false,
  reconciliationController: {
    mode: 'active_paused',
    activationPeriodCode: '2026-07-15/S1',
    activationPeriodStart: 1_783_994_400,
    legacyActive: false,
    pausedReason: 'durable_integrity_failed',
    transitionedAt: 1_783_958_800,
  },
  dashboardAlertProjection: {
    coverage: 'stale',
    observedAt: 1_783_958_100,
    staleReason: 'tail_replay_deferred',
  },
}

const compareStatus: UpstreamPrivacyStatus = {
  ...activeStatus,
  phase: 'compare',
  upstreamPreciseReconciliationEnabled: false,
  queuedSettlements: 1,
  lastReconciliationEnqueueErrorAt: 1_783_957_900,
  retryBuckets: {
    upstream429: 7,
    localUsageRateLimit: 2,
    other: 1,
  },
  currentPeriodBoundUsersByKey: congestedBoundUsers,
  currentPeriodPendingProjectIdsByKey: congestedPendingProjects,
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

const runObservationBase: NonNullable<UpstreamPrivacyStatus['reconciliationRunObservation']> = {
  mode: 'compare',
  projectionState: 'complete',
  projectionScannedRows: 12_450,
  projectionBatchSize: 75,
  projectionTransactionP95Ms: 47,
  cursorAdvanced: false,
  hydrateMs: 91,
  firstRemoteMs: 418,
  remoteMs: 622,
  finalizationMs: 44,
  researchMs: 0,
  settled: 0,
  noAdjustment: 0,
  observed: 0,
  upstream429: 0,
  transportFailure: 0,
  semanticFailure: 0,
  localPressure: 0,
  lastTransportKind: null,
  continuationReason: null,
  nextRetryAt: null,
  observedAt: 1_783_959_000,
}

const localBackoffStatus: UpstreamPrivacyStatus = {
  ...compareStatus,
  reconciliationBackoffLevel: 0,
  reconciliationBackoffUntil: null,
  reconciliationLocalBackoff: {
    pressureStreak: 3,
    level: 1,
    availableAt: 1_783_959_120,
    lastRecoveredAt: null,
  },
  reconciliationRunObservation: {
    ...runObservationBase,
    projectionState: 'deferred',
    localPressure: 1,
    continuationReason: 'local_pressure',
    nextRetryAt: 1_783_959_120,
  },
}

const upstreamBackoffStatus: UpstreamPrivacyStatus = {
  ...compareStatus,
  reconciliationPressureStreak: 3,
  reconciliationBackoffLevel: 1,
  reconciliationBackoffUntil: 1_783_959_120,
  reconciliationLocalBackoff: {
    pressureStreak: 0,
    level: 0,
    availableAt: null,
    lastRecoveredAt: null,
  },
  reconciliationRunObservation: {
    ...runObservationBase,
    upstream429: 1,
    continuationReason: 'upstream_429',
    nextRetryAt: 1_783_959_120,
  },
}

const transportFailureStatus: UpstreamPrivacyStatus = {
  ...compareStatus,
  coverage: 'ok',
  observedAt: 1_783_959_010,
  staleReason: null,
  reconciliationRunObservation: {
    ...runObservationBase,
    transportFailure: 1,
    lastTransportKind: 'timeout',
    lastTransportKindAt: 1_783_959_010,
    lastRetryableOutcome: 'transport_failure',
    continuationReason: 'transport_failure',
    nextRetryAt: 1_783_959_030,
  },
}

const stalePrivacyStatus: UpstreamPrivacyStatus = {
  ...transportFailureStatus,
  coverage: 'stale',
  observedAt: 1_783_958_320,
  staleReason: 'sqlite_pressure',
  dashboardAlertProjection: {
    coverage: 'stale',
    observedAt: 1_783_958_320,
    staleReason: 'tail_replay_deferred',
  },
}

const semanticFailureStatus: UpstreamPrivacyStatus = {
  ...compareStatus,
  reconciliationRunObservation: {
    ...runObservationBase,
    semanticFailure: 1,
    continuationReason: 'semantic_failure',
    nextRetryAt: 1_783_959_300,
  },
}

const missingEligibleUpstreamKeyStatus: UpstreamPrivacyStatus = {
  ...pendingStatus,
  retryBuckets: {
    ...pendingStatus.retryBuckets,
    missingEligibleUpstreamKey: 7,
  },
}

const budgetExhaustedStatus: UpstreamPrivacyStatus = {
  ...compareStatus,
  reconciliationLastDurationMs: 20_000,
  reconciliationLastAttempted: 20,
  reconciliationLastSettled: 0,
  reconciliationLastNoAdjustment: 0,
  reconciliationLastUpstream429: 16,
  reconciliationLastBudgetExhausted: true,
}

const noAdjustmentStatus: UpstreamPrivacyStatus = {
  ...activeStatus,
  reconciliationLastAttempted: 6,
  reconciliationLastSettled: 6,
  reconciliationLastNoAdjustment: 6,
  reconciliationLastUpstream429: 0,
  reconciliationLastBudgetExhausted: false,
}

const projectingStatus: UpstreamPrivacyStatus = {
  ...compareStatus,
  dashboardAlertProjection: {
    coverage: 'projecting',
    observedAt: 1_783_958_120,
    staleReason: null,
  },
  reconciliationRunObservation: {
    ...runObservationBase,
    projectionState: 'projecting',
    cursorAdvanced: true,
    firstRemoteMs: null,
    remoteMs: 0,
    continuationReason: 'projection_progress',
    nextRetryAt: 1_783_959_005,
  },
}

const observedStatus: UpstreamPrivacyStatus = {
  ...compareStatus,
  reconciliationRunObservation: {
    ...projectingStatus.reconciliationRunObservation!,
    projectionState: 'complete',
    firstRemoteMs: 418,
    remoteMs: 622,
    finalizationMs: 44,
    observed: 1,
    continuationReason: 'observed',
    nextRetryAt: null,
  },
}

const meta = {
  title: 'Admin/Modules/SystemStatusModule',
  component: UpstreamPrivacyStatusModule,
  tags: ['autodocs'],
  parameters: {
    layout: 'padded',
    ...desktopViewport,
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
    autoRefreshEnabled: true,
    onAutoRefreshChange: () => undefined,
    onOpenMcpSessionBindings: () => undefined,
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
      autoRefreshEnabled={overrides?.autoRefreshEnabled ?? true}
      onAutoRefreshChange={overrides?.onAutoRefreshChange ?? (() => undefined)}
      onOpenMcpSessionBindings={overrides?.onOpenMcpSessionBindings ?? (() => undefined)}
    />
  )
}

function renderEvidenceSurface(child: JSX.Element): JSX.Element {
  return (
    <div
      data-testid="upstream-privacy-evidence-surface"
      style={{
        background: '#453754',
        boxSizing: 'border-box',
        padding: 48,
      }}
    >
      <div style={{ background: '#ffffff', boxSizing: 'border-box', padding: 24 }}>{child}</div>
    </div>
  )
}

function InteractionCanvas(args: StoryArgs): JSX.Element {
  const [autoRefreshEnabled, setAutoRefreshEnabled] = useState(args.autoRefreshEnabled)

  return (
    <UpstreamPrivacyStatusModule
      {...args}
      autoRefreshEnabled={autoRefreshEnabled}
      onAutoRefreshChange={setAutoRefreshEnabled}
    />
  )
}

export const Pending: Story = {}

export const BlockedBySessions: Story = {
  args: {
    status: compareBlockedStatus,
  },
}

export const Active: Story = {
  args: {
    status: activeStatus,
  },
}

export const ActivePaused: Story = {
  args: {
    status: activePausedStatus,
  },
}

export const Healthy: Story = Active

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

export const LocalBackoff: Story = {
  args: {
    status: localBackoffStatus,
  },
}

export const EvidenceLocalBackoff: Story = {
  render: () => renderEvidenceSurface(renderWithStatus(localBackoffStatus)),
}

export const UpstreamBackoff: Story = {
  args: {
    status: upstreamBackoffStatus,
  },
}

export const GlobalBackoff: Story = UpstreamBackoff

export const UnknownObservation: Story = {
  args: {
    status: {
      ...activeStatus,
      queuedSettlements: null,
      reconciliationObservation: {
        observedAt: null,
        coverage: 'unknown',
        queueEstimate: null,
        hasEligible: false,
        oldestCandidateAgeSecs: null,
      },
      reconciliationLocalBackoff: {
        pressureStreak: 0,
        level: 0,
        availableAt: null,
        lastRecoveredAt: null,
      },
    },
  },
}

export const BudgetExhausted: Story = {
  args: {
    status: budgetExhaustedStatus,
  },
}

export const NoAdjustment: Story = {
  args: {
    status: noAdjustmentStatus,
  },
}

export const Projecting: Story = {
  args: { status: projectingStatus },
}

export const Observed: Story = {
  args: { status: observedStatus },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await expect(canvas.getByText('结算 / 无调整 / 仅观测')).toBeInTheDocument()
    await expect(canvas.getByText('阶段耗时（准备 / 首次远端 / 远端）')).toBeInTheDocument()
    await expect(canvas.getByText('0 / 0 / 1')).toBeInTheDocument()
  },
}

export const TransportFailure: Story = {
  args: { status: transportFailureStatus },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await expect(canvas.getByText('最近传输失败类别')).toBeInTheDocument()
    await expect(canvas.getByText('timeout')).toBeInTheDocument()
    await expect(canvas.getByText('transport_failure')).toBeInTheDocument()
  },
}

export const EvidenceTransportFailure: Story = {
  render: () => renderEvidenceSurface(renderWithStatus(transportFailureStatus)),
}

export const TransportFailureMobile393x852: Story = {
  args: { status: transportFailureStatus },
  parameters: mobileViewport,
}

export const EvidenceTransportFailureMobile393x852: Story = {
  parameters: mobileViewport,
  render: EvidenceTransportFailure.render,
}

export const PrivacyStatusStale: Story = {
  args: { status: stalePrivacyStatus },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await expect(canvas.getByText('隐私状态新鲜度')).toBeInTheDocument()
    await expect(canvas.getByText('stale')).toBeInTheDocument()
    await expect(canvas.getByText(/sqlite_pressure/)).toBeInTheDocument()
  },
}

export const EvidencePrivacyStatusStale: Story = {
  render: () => renderEvidenceSurface(renderWithStatus(stalePrivacyStatus)),
}

export const PrivacyStatusStaleMobile393x852: Story = {
  args: { status: stalePrivacyStatus },
  parameters: mobileViewport,
}

export const EvidencePrivacyStatusStaleMobile393x852: Story = {
  parameters: mobileViewport,
  render: EvidencePrivacyStatusStale.render,
}

export const SemanticFailure: Story = {
  args: { status: semanticFailureStatus },
}

export const MissingEligibleUpstreamKey: Story = {
  args: { status: missingEligibleUpstreamKeyStatus },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await expect(canvas.getByText('缺少可用上游 Key')).toBeInTheDocument()
    await expect(canvas.getByText('7')).toBeInTheDocument()
  },
}

export const Recovered: Story = {
  args: { status: observedStatus },
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

export const Mobile393x852: Story = {
  parameters: {
    ...mobileViewport,
  },
}

export const EvidenceLocalBackoffMobile393x852: Story = {
  parameters: mobileViewport,
  render: EvidenceLocalBackoff.render,
}

export const Gallery: Story = {
  render: () => (
    <div style={{ display: 'grid', gap: 24 }}>
      {[
        { title: 'Healthy', status: activeStatus },
        { title: 'Local backoff', status: localBackoffStatus },
        { title: 'Upstream backoff', status: upstreamBackoffStatus },
        { title: 'Budget exhausted', status: budgetExhaustedStatus },
        { title: 'No adjustment', status: noAdjustmentStatus },
        { title: 'Projecting', status: projectingStatus },
        { title: 'Observed', status: observedStatus },
        { title: 'Transport failure', status: transportFailureStatus },
        { title: 'Privacy status stale', status: stalePrivacyStatus },
        { title: 'Semantic failure', status: semanticFailureStatus },
        { title: 'Recovered', status: observedStatus },
        { title: 'Pending', status: pendingStatus },
        { title: 'Blocked by sessions', status: compareBlockedStatus },
        { title: 'Compare', status: compareStatus },
        { title: 'Active', status: activeStatus },
        { title: 'Active paused', status: activePausedStatus },
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

export const MobileStateGallery: Story = {
  parameters: mobileViewport,
  render: Gallery.render,
}

export const Mobile: Story = Mobile393x852

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

    await expect(canvas.queryByRole('button', { name: '立即刷新' })).not.toBeInTheDocument()
  },
}
