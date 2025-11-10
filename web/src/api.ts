export interface Summary {
  total_requests: number
  success_count: number
  error_count: number
  quota_exhausted_count: number
  active_keys: number
  exhausted_keys: number
  last_activity: number | null
  total_quota_limit: number
  total_quota_remaining: number
}

export interface PublicMetrics {
  monthlySuccess: number
  dailySuccess: number
}

export interface TokenMetrics {
  monthlySuccess: number
  dailySuccess: number
  dailyFailure: number
}

export interface ApiKeyStats {
  id: string
  status: string
  status_changed_at: number | null
  last_used_at: number | null
  deleted_at: number | null
  quota_limit: number | null
  quota_remaining: number | null
  quota_synced_at: number | null
  total_requests: number
  success_count: number
  error_count: number
  quota_exhausted_count: number
}

export interface RequestLog {
  id: number
  key_id: string
  method: string
  path: string
  query: string | null
  http_status: number | null
  mcp_status: number | null
  result_status: string
  created_at: number
  error_message: string | null
  request_body: string | null
  response_body: string | null
  forwarded_headers: string[]
  dropped_headers: string[]
}

export interface ApiKeySecret {
  api_key: string
}

// ---- Access Tokens (for /mcp auth) ----
export interface AuthToken {
  id: string // 4-char code
  enabled: boolean
  note: string | null
  total_requests: number
  created_at: number
  last_used_at: number | null
}

export interface AuthTokenSecret {
  token: string // th-<id>-<secret>
}

async function requestJson<T>(input: RequestInfo, init?: RequestInit): Promise<T> {
  const response = await fetch(input, init)
  if (!response.ok) {
    const message = await response.text().catch(() => response.statusText)
    throw new Error(message || `Request failed with status ${response.status}`)
  }
  return (await response.json()) as T
}

export interface VersionInfo {
  backend: string
  frontend: string
}

export function fetchVersion(signal?: AbortSignal): Promise<VersionInfo> {
  return requestJson('/api/version', { signal })
}

export function fetchSummary(signal?: AbortSignal): Promise<Summary> {
  return requestJson('/api/summary', { signal })
}

export function fetchPublicMetrics(signal?: AbortSignal): Promise<PublicMetrics> {
  return requestJson('/api/public/metrics', { signal })
}

export function fetchTokenMetrics(token: string, signal?: AbortSignal): Promise<TokenMetrics> {
  const params = new URLSearchParams({ token })
  return requestJson(`/api/token/metrics?${params.toString()}`, { signal })
}

export function fetchApiKeys(signal?: AbortSignal): Promise<ApiKeyStats[]> {
  return requestJson('/api/keys', { signal })
}

export function fetchApiKeyDetail(id: string, signal?: AbortSignal): Promise<ApiKeyStats> {
  const encoded = encodeURIComponent(id)
  return requestJson(`/api/keys/${encoded}`, { signal })
}

export function fetchRequestLogs(limit = 50, signal?: AbortSignal): Promise<RequestLog[]> {
  const params = new URLSearchParams({ limit: limit.toString() })
  return requestJson(`/api/logs?${params.toString()}`, { signal })
}

export function fetchApiKeySecret(id: string, signal?: AbortSignal): Promise<ApiKeySecret> {
  const encoded = encodeURIComponent(id)
  return requestJson(`/api/keys/${encoded}/secret`, { signal })
}

export async function syncApiKeyUsage(id: string): Promise<void> {
  const encoded = encodeURIComponent(id)
  const res = await fetch(`/api/keys/${encoded}/sync-usage`, { method: 'POST' })
  if (!res.ok) throw new Error(`Failed to sync key usage: ${res.status}`)
}

export interface JobLogView {
  id: number
  job_type: string
  key_id: string | null
  status: string
  attempt: number
  message: string | null
  started_at: number
  finished_at: number | null
}

export function fetchJobs(limit = 100, signal?: AbortSignal): Promise<JobLogView[]> {
  const params = new URLSearchParams({ limit: String(limit) })
  return requestJson(`/api/jobs?${params.toString()}`, { signal })
}

export interface Profile {
  displayName: string | null
  isAdmin: boolean
}

export function fetchProfile(signal?: AbortSignal): Promise<Profile> {
  return requestJson('/api/profile', { signal })
}

export interface CreateKeyResponse {
  id: string
}

export async function addApiKey(apiKey: string): Promise<CreateKeyResponse> {
  return await requestJson('/api/keys', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ api_key: apiKey }),
  })
}

export async function deleteApiKey(id: string): Promise<void> {
  const encoded = encodeURIComponent(id)
  await fetch(`/api/keys/${encoded}`, { method: 'DELETE' }).then((res) => {
    if (!res.ok) throw new Error(`Failed to delete key: ${res.status}`)
  })
}

export type KeyAdminStatus = 'active' | 'disabled'

export async function setKeyStatus(id: string, status: KeyAdminStatus): Promise<void> {
  const encoded = encodeURIComponent(id)
  const res = await fetch(`/api/keys/${encoded}/status`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ status }),
  })
  if (!res.ok) {
    throw new Error(`Failed to update key status: ${res.status}`)
  }
}

// ---- Key details ----
export interface KeySummary {
  total_requests: number
  success_count: number
  error_count: number
  quota_exhausted_count: number
  active_keys: number
  exhausted_keys: number
  last_activity: number | null
}

export function fetchKeyMetrics(id: string, period?: 'day' | 'week' | 'month', since?: number, signal?: AbortSignal): Promise<KeySummary> {
  const params = new URLSearchParams()
  if (period) params.set('period', period)
  if (since != null) params.set('since', String(since))
  const encoded = encodeURIComponent(id)
  return requestJson(`/api/keys/${encoded}/metrics?${params.toString()}`, { signal })
}

export function fetchKeyLogs(id: string, limit = 50, since?: number, signal?: AbortSignal): Promise<RequestLog[]> {
  const params = new URLSearchParams({ limit: String(limit) })
  if (since != null) params.set('since', String(since))
  const encoded = encodeURIComponent(id)
  return requestJson(`/api/keys/${encoded}/logs?${params.toString()}`, { signal })
}

// Tokens API
export function fetchTokens(signal?: AbortSignal): Promise<AuthToken[]> {
  return requestJson('/api/tokens', { signal })
}

export async function createToken(note?: string): Promise<AuthTokenSecret> {
  return await requestJson('/api/tokens', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ note }),
  })
}

export async function deleteToken(id: string): Promise<void> {
  const encoded = encodeURIComponent(id)
  const res = await fetch(`/api/tokens/${encoded}`, { method: 'DELETE' })
  if (!res.ok) throw new Error(`Failed to delete token: ${res.status}`)
}

export async function setTokenEnabled(id: string, enabled: boolean): Promise<void> {
  const encoded = encodeURIComponent(id)
  const res = await fetch(`/api/tokens/${encoded}/status`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ enabled }),
  })
  if (!res.ok) throw new Error(`Failed to update token status: ${res.status}`)
}

export async function updateTokenNote(id: string, note: string): Promise<void> {
  const encoded = encodeURIComponent(id)
  const res = await fetch(`/api/tokens/${encoded}/note`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ note }),
  })
  if (!res.ok) throw new Error(`Failed to update token note: ${res.status}`)
}

export function fetchTokenSecret(id: string, signal?: AbortSignal): Promise<AuthTokenSecret> {
  const encoded = encodeURIComponent(id)
  return requestJson(`/api/tokens/${encoded}/secret`, { signal })
}
