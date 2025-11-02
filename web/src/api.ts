export interface Summary {
  total_requests: number
  success_count: number
  error_count: number
  quota_exhausted_count: number
  active_keys: number
  exhausted_keys: number
  last_activity: number | null
}

export interface ApiKeyStats {
  id: string
  status: string
  status_changed_at: number | null
  last_used_at: number | null
  total_requests: number
  success_count: number
  error_count: number
  quota_exhausted_count: number
}

export interface RequestLog {
  id: number
  key_id: string
  http_status: number | null
  mcp_status: number | null
  result_status: string
  created_at: number
  error_message: string | null
}

export interface ApiKeySecret {
  api_key: string
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

export function fetchApiKeys(signal?: AbortSignal): Promise<ApiKeyStats[]> {
  return requestJson('/api/keys', { signal })
}

export function fetchRequestLogs(limit = 50, signal?: AbortSignal): Promise<RequestLog[]> {
  const params = new URLSearchParams({ limit: limit.toString() })
  return requestJson(`/api/logs?${params.toString()}`, { signal })
}

export function fetchApiKeySecret(id: string, signal?: AbortSignal): Promise<ApiKeySecret> {
  const encoded = encodeURIComponent(id)
  return requestJson(`/api/keys/${encoded}/secret`, { signal })
}

export interface Profile {
  displayName: string | null
  isAdmin: boolean
}

export function fetchProfile(signal?: AbortSignal): Promise<Profile> {
  return requestJson('/api/profile', { signal })
}
