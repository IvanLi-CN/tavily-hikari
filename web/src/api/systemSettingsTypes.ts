import type { RequestLogRetentionSettings } from './requestLogRetention'

export type UpstreamProjectIdMode = 'passthrough' | 'fixed' | 'accessToken'

export interface SystemSettings {
  requestRateLimit: number
  authTokenLogRetentionDays: number
  mcpSessionAffinityKeyCount: number
  rebalanceMcpEnabled: boolean
  rebalanceMcpSessionPercent: number
  apiRebalanceEnabled: boolean
  apiRebalancePercent: number
  upstreamProjectIdMode: UpstreamProjectIdMode
  upstreamProjectIdFixedValue: string
  upstreamMcpUserAgent: string
  upstreamPreciseReconciliationEnabled: boolean
  rechargeFeatureEnabled: boolean
  rechargeUserEnabled: boolean
  adminDefaultActiveUsersOnly: boolean
  userBlockedKeyBaseLimit: number
  globalIpLimit: number
  trustedProxyCidrs: string[]
  trustedClientIpHeaders: string[]
  requestLogRetention: RequestLogRetentionSettings
}

export interface AdminUserListStats {
  activeUsers90d: number
  totalUsers: number
  windowDays: number
}

export interface UpstreamPrivacyGate {
  key: string
  ready: boolean
  detail: string
}

export interface UpstreamReconciliationAdjustment {
  settlementKey: string
  tokenIdHint: string
  billingSubjectKind: string
  periodCode: string
  deltaCredits: number
  degradedReason: string | null
  createdAt: number
}

export interface UpstreamReconciliationRetryBuckets {
  upstream429: number
  localUsageRateLimit: number
  missingEligibleUpstreamKey?: number
  other: number
}

export interface UpstreamKeyActivityPoint {
  keyIdHint: string
  count: number
}

export interface DailyReconciliationProgress {
  observedAccounts: number
  accountsWithSettledPeriod: number
  fullyTerminalAccounts: number
  observedPeriods: number
  settledPeriods: number
  degradedPeriods: number
  pendingPeriods: number
  researchTotal: number
  researchTerminal: number
  researchPending: number
  researchUnavailable?: number
  researchPollablePending?: number
}

export interface DailyReconciliationKeyProgress {
  keyIdHint: string
  terminalResearch: number
  pendingResearch: number
  pendingProjectIds: number
  cooldownUntil: number | null
  cooldownReason: string | null
}

export interface ReconciliationObservation {
  observedAt: number | null
  coverage: 'bounded' | 'unknown' | string
  queueEstimate: number | null
  hasEligible: boolean
  oldestCandidateAgeSecs: number | null
}

export interface ReconciliationLocalBackoff {
  pressureStreak: number
  level: number
  availableAt: number | null
  lastRecoveredAt: number | null
}

export interface ReconciliationRunObservation {
  mode: 'disabled' | 'compare' | 'active' | string
  projectionState: 'unknown' | 'projecting' | 'complete' | 'deferred' | string
  projectionScannedRows: number
  projectionBatchSize: number
  projectionTransactionP95Ms: number
  cursorAdvanced: boolean
  hydrateMs: number
  firstRemoteMs: number | null
  remoteMs: number
  finalizationMs: number
  researchMs: number
  settled: number
  noAdjustment: number
  observed: number
  upstream429: number
  transportFailure: number
  semanticFailure: number
  localPressure: number
  /** Count-only diagnostics for bounded multi-key reconciliation progress. */
  partialKeyObservations: number
  multiKeyPending: number
  remoteAttemptBudgetDefers: number
  resumedRuns: number
  terminalRuns: number
  lastTransportKind?: 'connect' | 'timeout' | 'response_body' | 'invalid_endpoint' | 'credentials_or_database' | 'unknown' | string | null
  lastTransportKindAt?: number | null
  lastRetryableOutcome?: string | null
  continuationReason: string | null
  nextRetryAt: number | null
  observedAt: number | null
}

export interface ReconciliationResearchProgressWindow {
  windowStartedAt: number | null
  windowEndedAt: number | null
  windowSeconds: number
  terminalDelta: number
  pendingDelta: number
  unavailableDelta?: number
  pollablePendingDelta?: number
  terminalRatePositive: boolean
  pollResolutionRatePositive?: boolean
  pendingNonGrowing: boolean
  complete: boolean
}

export interface ReconciliationResearchPollDiagnostics {
  unavailable: number
  pollablePending: number
  credentialsCoolingKeys: number
  earliestCredentialsRetryAt: number | null
  lastPollOutcome: string | null
  lastPollObservedAt: number | null
}

export interface ReconciliationControllerStatus {
  mode: 'compare' | 'active' | 'active_paused' | string
  activationPeriodCode: string | null
  activationPeriodStart: number | null
  legacyActive: boolean
  pausedReason: string | null
  transitionedAt: number | null
}

export interface DashboardAlertProjectionStatus {
  coverage: string
  observedAt: number | null
  staleReason: string | null
}

export interface UpstreamPrivacyStatus {
  phase: 'configured' | 'draining' | 'pending' | 'compare' | 'active' | 'active_paused' | 'degraded'
  configuredProjectIdMode: UpstreamProjectIdMode
  effectiveProjectIdMode: UpstreamProjectIdMode
  fixedProjectIdConfigured: boolean
  configuredMcpUserAgent: string
  effectiveMcpUserAgent: string | null
  upstreamPreciseReconciliationEnabled: boolean
  httpAllowedHeaders: string[]
  controlMcpAllowedHeaders: string[]
  gates: UpstreamPrivacyGate[]
  completedGates: number
  totalGates: number
  activeUpstreamMcpSessions: number
  currentPeriodCode: string
  currentPeriodEndsAt: number
  nextEpochAt: number | null
  pendingResearch: number | null
  queuedSettlements: number | null
  degradedSettlements: number
  degradedSettlementsCapped: boolean
  lastReconciliationRunAt: number | null
  lastShadowAdjustmentAt: number | null
  lastReconciliationEnqueueErrorAt: number | null
  lastResearchSweepAt: number | null
  lastResearchTerminalAt: number | null
  reconciliationPressureStreak?: number
  reconciliationBackoffLevel?: number
  reconciliationBackoffUntil?: number | null
  reconciliationLastDurationMs?: number | null
  reconciliationLastAttempted?: number
  reconciliationLastSettled?: number
  reconciliationLastNoAdjustment?: number
  reconciliationLastUpstream429?: number
  reconciliationLastBudgetExhausted?: boolean
  reconciliationObservation: ReconciliationObservation
  reconciliationLocalBackoff: ReconciliationLocalBackoff
  reconciliationRunObservation?: ReconciliationRunObservation
  reconciliationResearchProgressWindow?: ReconciliationResearchProgressWindow
  reconciliationResearchPollDiagnostics?: ReconciliationResearchPollDiagnostics
  reconciliationController?: ReconciliationControllerStatus
  dashboardAlertProjection?: DashboardAlertProjectionStatus
  coverage?: string
  observedAt?: number | null
  staleReason?: string | null
  retryBuckets: UpstreamReconciliationRetryBuckets
  currentPeriodBoundUsersByKey: UpstreamKeyActivityPoint[]
  currentPeriodPendingProjectIdsByKey: UpstreamKeyActivityPoint[]
  dailyReconciliationProgress: DailyReconciliationProgress
  dailyReconciliationByKey: DailyReconciliationKeyProgress[]
  recentAdjustments: UpstreamReconciliationAdjustment[]
  generatedAt: number
}

export interface ForwardProxySettingsEnvelope {
  forwardProxy?: import('./runtime').ForwardProxySettings | null
  systemSettings?: SystemSettings | null
  adminUserListStats?: AdminUserListStats | null
  activeUpstreamMcpSessions?: number | null
}

export type AdminMcpSessionBindingsFilterStatus = 'active' | 'revoked' | 'all'
export type AdminMcpSessionBindingStatus = 'active' | 'expired' | 'revoked'

export interface AdminMcpSessionBindingListItem {
  proxySessionId: string
  authTokenId: string | null
  userId: string | null
  upstreamKeyId: string | null
  createdAt: number
  updatedAt: number
  expiresAt: number
  status: AdminMcpSessionBindingStatus
  revokedAt: number | null
  revokeReason: string | null
}

export interface AdminMcpSessionBindingsPage {
  items: AdminMcpSessionBindingListItem[]
  total: number
  page: number
  perPage: number
  activeMatchingCount: number
}

export interface AdminMcpSessionBindingsQuery {
  status?: AdminMcpSessionBindingsFilterStatus
  createdFrom?: string | null
  createdTo?: string | null
  updatedFrom?: string | null
  updatedTo?: string | null
  page?: number | null
  perPage?: number | null
}

export interface AdminMcpSessionBindingsRevokeResult {
  revokedCount: number
}

export interface UpdateSystemSettingsPayload {
  requestRateLimit: number
  authTokenLogRetentionDays: number
  mcpSessionAffinityKeyCount: number
  rebalanceMcpEnabled: boolean
  rebalanceMcpSessionPercent: number
  apiRebalanceEnabled: boolean
  apiRebalancePercent: number
  upstreamProjectIdMode: UpstreamProjectIdMode
  upstreamProjectIdFixedValue: string
  upstreamMcpUserAgent: string
  upstreamPreciseReconciliationEnabled: boolean
  rechargeFeatureEnabled: boolean
  rechargeUserEnabled: boolean
  adminDefaultActiveUsersOnly: boolean
  trustedProxyCidrs: string[]
  trustedClientIpHeaders: string[]
  userBlockedKeyBaseLimit: number
  globalIpLimit: number
  requestLogRetention: RequestLogRetentionSettings
}
