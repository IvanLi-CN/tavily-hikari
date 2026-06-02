import type { RequestLogRetentionSettings } from './requestLogRetention'

export interface SystemSettings {
  requestRateLimit: number
  mcpSessionAffinityKeyCount: number
  rebalanceMcpEnabled: boolean
  rebalanceMcpSessionPercent: number
  apiRebalanceEnabled: boolean
  apiRebalancePercent: number
  rechargeFeatureEnabled: boolean
  rechargeUserEnabled: boolean
  userBlockedKeyBaseLimit: number
  globalIpLimit: number
  trustedProxyCidrs: string[]
  trustedClientIpHeaders: string[]
  requestLogRetention: RequestLogRetentionSettings
}

export interface ForwardProxySettingsEnvelope {
  forwardProxy?: import('./runtime').ForwardProxySettings | null
  systemSettings?: SystemSettings | null
}

export interface UpdateSystemSettingsPayload {
  requestRateLimit: number
  mcpSessionAffinityKeyCount: number
  rebalanceMcpEnabled: boolean
  rebalanceMcpSessionPercent: number
  apiRebalanceEnabled: boolean
  apiRebalancePercent: number
  rechargeFeatureEnabled: boolean
  rechargeUserEnabled: boolean
  trustedProxyCidrs: string[]
  trustedClientIpHeaders: string[]
  userBlockedKeyBaseLimit: number
  globalIpLimit: number
  requestLogRetention: RequestLogRetentionSettings
}
