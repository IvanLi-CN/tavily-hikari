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

export interface UpstreamPrivacyStatus {
  phase: 'configured' | 'draining' | 'pending' | 'compare' | 'active' | 'degraded'
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
  activeControlSessions: number
  currentPeriodCode: string
  currentPeriodEndsAt: number
  nextEpochAt: number | null
  pendingResearch: number
  queuedSettlements: number
  degradedSettlements: number
  recentAdjustments: UpstreamReconciliationAdjustment[]
  generatedAt: number
}

export interface ForwardProxySettingsEnvelope {
  forwardProxy?: import('./runtime').ForwardProxySettings | null
  systemSettings?: SystemSettings | null
  adminUserListStats?: AdminUserListStats | null
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
