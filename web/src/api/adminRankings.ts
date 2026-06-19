import { requestJson } from './runtime'

export interface AdminUserRankingIdentity {
  userId: string
  displayName: string | null
  username: string | null
  avatarUrl: string | null
}

export interface AdminUserRankingRow {
  rank: number
  value: number
  user: AdminUserRankingIdentity
}

export interface AdminUserRankingWindow {
  primarySuccessTop: AdminUserRankingRow[]
  businessCreditsTop: AdminUserRankingRow[]
}

export interface AdminUserRankingsSnapshot {
  generatedAt: number
  refreshIntervalSecs: number
  last24h: AdminUserRankingWindow
  last7d: AdminUserRankingWindow
  last30d: AdminUserRankingWindow
}

export function fetchAdminUserRankings(signal?: AbortSignal): Promise<AdminUserRankingsSnapshot> {
  return requestJson('/api/users/rankings', { signal })
}
