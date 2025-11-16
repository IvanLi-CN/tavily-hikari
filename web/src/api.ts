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

export interface TokenHourlyBucket {
  bucket_start: number
  success_count: number
  system_failure_count: number
  external_failure_count: number
}

export interface TokenUsageBucket {
  bucket_start: number
  success_count: number
  system_failure_count: number
  external_failure_count: number
}

// Public token logs (per access token)
export interface PublicTokenLog {
  id: number
  method: string
  path: string
  query: string | null
  http_status: number | null
  mcp_status: number | null
  result_status: string
  error_message: string | null
  created_at: number
}

// Server returns camelCase. Define the server shape and map to snake_case used in UI.
interface ServerPublicTokenLog {
  id: number
  method: string
  path: string
  query: string | null
  httpStatus: number | null
  mcpStatus: number | null
  resultStatus: string
  errorMessage: string | null
  createdAt: number
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
  auth_token_id: string | null
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
  group: string | null
  total_requests: number
  created_at: number
  last_used_at: number | null
  quota_state: 'normal' | 'hour' | 'day' | 'month'
  quota_hourly_used: number
  quota_hourly_limit: number
  quota_daily_used: number
  quota_daily_limit: number
  quota_monthly_used: number
  quota_monthly_limit: number
  quota_hourly_reset_at: number | null
  quota_daily_reset_at: number | null
  quota_monthly_reset_at: number | null
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

export async function fetchPublicLogs(token: string, limit = 20, signal?: AbortSignal): Promise<PublicTokenLog[]> {
  const params = new URLSearchParams({ token, limit: String(limit) })
  const url = `/api/public/logs?${params.toString()}`
  const res = await fetch(url, { signal })
  if (!res.ok) {
    const message = await res.text().catch(() => res.statusText)
    const err = new Error(message || `Request failed with status ${res.status}`) as Error & { status?: number }
    err.status = res.status
    throw err
  }
  const data = (await res.json()) as ServerPublicTokenLog[]
  return data.map((it) => ({
    id: it.id,
    method: it.method,
    path: it.path,
    query: it.query,
    http_status: it.httpStatus,
    mcp_status: it.mcpStatus,
    result_status: it.resultStatus,
    error_message: it.errorMessage,
    created_at: it.createdAt,
  }))
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
  if (!res.ok) {
    let message = ''
    try {
      const data = await res.json()
      message = (data?.detail as string) ?? (data?.error as string) ?? ''
    } catch {
      message = await res.text().catch(() => '')
    }
    const statusPart = ` (HTTP ${res.status})`
    throw new Error((message ? `${message}` : 'Failed to sync key usage') + statusPart)
  }
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
export interface Paginated<T> {
  items: T[]
  total: number
  page: number
  perPage: number
}

export interface TokenGroup {
  name: string
  tokenCount: number
  latestCreatedAt: number
}

export function fetchTokens(
  page = 1,
  perPage = 10,
  options?: { group?: string | null; ungrouped?: boolean },
  signal?: AbortSignal,
): Promise<Paginated<AuthToken>> {
  const params = new URLSearchParams({ page: String(page), per_page: String(perPage) })
  if (options?.ungrouped) {
    params.set('no_group', 'true')
  } else if (options?.group && options.group.trim().length > 0) {
    params.set('group', options.group.trim())
  }
  return requestJson(`/api/tokens?${params.toString()}`, { signal })
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

export async function rotateTokenSecret(id: string): Promise<AuthTokenSecret> {
  const encoded = encodeURIComponent(id)
  return await requestJson(`/api/tokens/${encoded}/secret/rotate`, { method: 'POST' })
}

export interface BatchCreateTokensResponse {
  tokens: string[]
}

export async function createTokensBatch(group: string, count: number, note?: string): Promise<BatchCreateTokensResponse> {
  return await requestJson('/api/tokens/batch', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ group, count, note }),
  })
}

export function fetchTokenGroups(signal?: AbortSignal): Promise<TokenGroup[]> {
  return requestJson('/api/tokens/groups', { signal })
}

export function fetchTokenHourlyBuckets(id: string, hours = 25, signal?: AbortSignal): Promise<TokenHourlyBucket[]> {
  const encoded = encodeURIComponent(id)
  const params = new URLSearchParams({ hours: String(hours) })
  return requestJson(`/api/tokens/${encoded}/metrics/hourly?${params.toString()}`, { signal })
}

export function fetchTokenUsageSeries(
  id: string,
  params: { since: string; until: string; bucketSecs?: number },
  signal?: AbortSignal,
): Promise<TokenUsageBucket[]> {
  const encoded = encodeURIComponent(id)
  const search = new URLSearchParams({ since: params.since, until: params.until })
  if (params.bucketSecs != null) {
    search.set('bucket_secs', String(params.bucketSecs))
  }
  return requestJson(`/api/tokens/${encoded}/metrics/usage-series?${search.toString()}`, { signal })
}
