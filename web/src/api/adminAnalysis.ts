import { requestJson } from './runtime'

export interface AnalysisPressurePoint {
  bucketStart: number
  displayBucketStart: number
  pressure: number
  successCount: number
  failureCount: number
}

export interface AnalysisPressurePeak {
  bucketStart: number
  displayBucketStart: number
  pressure: number
}

export interface AnalysisServerPressure24h {
  windowMinutes: number
  bucketSeconds: number
  current: AnalysisPressurePoint[]
  previous: AnalysisPressurePoint[]
  currentPeak: AnalysisPressurePeak | null
  previousPeak: AnalysisPressurePeak | null
}

export interface AnalysisCurrentUserPressureRow {
  userId: string
  displayName: string | null
  username: string | null
  avatarUrl: string | null
  pressure: number
  successCount: number
  failureCount: number
}

export interface AnalysisCurrentUserPressureSummary {
  activeUsers: number
  zeroPressureUsers: number
  median: number
  p90: number
  peak: number
  currentPressure: number
  vsYesterdayDelta: number
}

export interface AnalysisCurrentUserPressureDistribution {
  windowMinutes: number
  rows: AnalysisCurrentUserPressureRow[]
  summary: AnalysisCurrentUserPressureSummary
}

export interface AnalysisServerPressure7d {
  bucketSeconds: number
  points: AnalysisPressurePoint[]
  peak: AnalysisPressurePeak | null
}

export interface AnalysisPressureSnapshot {
  generatedAt: number
  server24h: AnalysisServerPressure24h
  currentUserDistribution: AnalysisCurrentUserPressureDistribution
  server7d: AnalysisServerPressure7d
}

export function fetchAdminAnalysisPressure(signal?: AbortSignal): Promise<AnalysisPressureSnapshot> {
  return requestJson('/api/analysis/pressure', { signal })
}
