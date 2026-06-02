export interface RequestLogRetentionProfile {
  businessBodyDays: number
  nonBusinessBodyDays: number
  nonSuccessBodyDays: number
}

export interface RequestLogRetentionSettings {
  maxLogRetentionDays: number
  heavyUsageThresholdPercent: number
  global: RequestLogRetentionProfile
  heavyUsage: RequestLogRetentionProfile
  debugShared: RequestLogRetentionProfile
}
