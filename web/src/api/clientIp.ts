import { requestJson } from './runtime'

export interface ClientIpHeaderValue {
  name: string
  value: string
}

export interface ObservedClientIpHeaderValue extends ClientIpHeaderValue {
  count: number
  lastSeenAt: number
}

export interface ObservedClientIpRequest {
  id: number
  createdAt: number
  remoteAddr?: string | null
  clientIp?: string | null
  clientIpSource?: string | null
  clientIpTrusted: boolean
  ipHeaders: ClientIpHeaderValue[]
}

export function fetchObservedClientIpRequests(signal?: AbortSignal): Promise<ObservedClientIpRequest[]> {
  return requestJson<{ items: ObservedClientIpRequest[] }>('/api/settings/client-ip/observed-headers', {
    signal,
  }).then((response) => response.items ?? [])
}
