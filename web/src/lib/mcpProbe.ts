export type ProbeQuotaWindow = 'hour' | 'day' | 'month'
export type McpProbeStepState = 'success' | 'failed' | 'blocked'

export const MCP_PROBE_ACCEPT_HEADER = 'application/json, text/event-stream'

interface QuotaSnapshotLike {
  quotaHourlyUsed: number
  quotaHourlyLimit: number
  quotaDailyUsed: number
  quotaDailyLimit: number
  quotaMonthlyUsed: number
  quotaMonthlyLimit: number
}

export class McpProbeRequestError extends Error {
  status: number
  payload: unknown
  rawBody: string

  constructor(message: string, status: number, payload: unknown, rawBody: string) {
    super(message)
    this.name = 'McpProbeRequestError'
    this.status = status
    this.payload = payload
    this.rawBody = rawBody
  }
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' ? (value as Record<string, unknown>) : null
}

function extractSseDataPayloads(raw: string): string[] {
  const payloads: string[] = []
  const chunks = raw.split(/\r?\n\r?\n/)
  for (const part of chunks) {
    const chunk = part.trim()
    if (!chunk) continue
    const dataLines = chunk
      .split(/\r?\n/)
      .filter((line) => line.startsWith('data:'))
      .map((line) => line.slice('data:'.length).trim())
      .filter((line) => line.length > 0)
    if (dataLines.length > 0) {
      payloads.push(dataLines.join('\n'))
    }
  }
  return payloads
}

function isMcpProbeEnvelope(value: unknown): value is Record<string, unknown> {
  const map = asRecord(value)
  return !!map && ('result' in map || 'error' in map)
}

export function parseMcpProbePayload(raw: string): unknown {
  const trimmed = raw.trim()
  if (trimmed.length === 0) {
    throw new Error('Empty MCP probe payload')
  }

  try {
    return JSON.parse(trimmed) as unknown
  } catch {
    const ssePayloads = extractSseDataPayloads(trimmed)
    if (ssePayloads.length === 0) {
      throw new Error('Invalid MCP probe payload')
    }

    let lastPayload: unknown | undefined
    for (const payload of ssePayloads) {
      try {
        const parsedPayload = parseMcpProbePayload(payload)
        lastPayload = parsedPayload
        if (isMcpProbeEnvelope(parsedPayload)) {
          return parsedPayload
        }
      } catch {
        continue
      }
    }

    if (lastPayload === undefined) {
      throw new Error('Invalid MCP probe payload')
    }
    return lastPayload
  }
}

export function getProbeEnvelopeError(payload: unknown): string | null {
  const map = asRecord(payload)
  if (!map) return null

  const topError = map.error
  if (typeof topError === 'string' && topError.trim().length > 0) {
    return topError
  }
  if (topError != null) {
    const topErrorObj = asRecord(topError)
    const topMessage = topErrorObj?.message
    if (typeof topMessage === 'string' && topMessage.trim().length > 0) {
      return topMessage
    }
    return 'Request failed'
  }

  const detail = asRecord(map.detail)
  const detailError = detail?.error
  if (typeof detailError === 'string' && detailError.trim().length > 0) {
    return detailError
  }
  if (detailError != null) {
    const detailErrorObj = asRecord(detailError)
    const detailMessage = detailErrorObj?.message
    if (typeof detailMessage === 'string' && detailMessage.trim().length > 0) {
      return detailMessage
    }
    return 'Request failed'
  }

  return null
}

export function getQuotaExceededWindow(payload: unknown): ProbeQuotaWindow | null {
  const map = asRecord(payload)
  if (!map) return null
  if (map.error !== 'quota_exceeded') return null
  const window = map.window
  return window === 'hour' || window === 'day' || window === 'month' ? window : null
}

export function getTokenBusinessQuotaWindow(token: QuotaSnapshotLike | null | undefined): ProbeQuotaWindow | null {
  if (!token) return null
  if (token.quotaHourlyLimit > 0 && token.quotaHourlyUsed >= token.quotaHourlyLimit) {
    return 'hour'
  }
  if (token.quotaDailyLimit > 0 && token.quotaDailyUsed >= token.quotaDailyLimit) {
    return 'day'
  }
  if (token.quotaMonthlyLimit > 0 && token.quotaMonthlyUsed >= token.quotaMonthlyLimit) {
    return 'month'
  }
  return null
}

export async function revalidateBlockedQuotaWindow<T extends QuotaSnapshotLike | null | undefined>(
  token: T,
  loadLatest: () => Promise<T>,
): Promise<{ token: T; window: ProbeQuotaWindow | null }> {
  const currentWindow = getTokenBusinessQuotaWindow(token)
  if (!currentWindow) {
    return { token, window: null }
  }

  const refreshedToken = await loadLatest()
  const nextToken = refreshedToken ?? token
  return {
    token: nextToken,
    window: getTokenBusinessQuotaWindow(nextToken),
  }
}

export function resolveMcpProbeButtonState(stepStates: readonly McpProbeStepState[]): 'success' | 'partial' | 'failed' {
  const passed = stepStates.filter((state) => state === 'success').length
  if (passed === stepStates.length) return 'success'
  if (passed === 0) return 'failed'
  return 'partial'
}

export async function requestMcpProbeWithToken<T>(
  input: RequestInfo,
  token: string,
  init?: RequestInit,
): Promise<T> {
  const headers = new Headers(init?.headers ?? {})
  headers.set('Authorization', `Bearer ${token}`)
  headers.set('Accept', MCP_PROBE_ACCEPT_HEADER)
  if (init?.body != null && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json')
  }

  const response = await fetch(input, { ...init, headers })
  const rawBody = await response.text().catch(() => '')

  let payload: unknown = {}
  let parseError: Error | null = null
  if (rawBody.trim().length > 0) {
    try {
      payload = parseMcpProbePayload(rawBody)
    } catch (err) {
      parseError = err instanceof Error ? err : new Error('Invalid MCP probe payload')
      payload = rawBody
    }
  } else if (response.ok) {
    parseError = new Error('Empty MCP probe payload')
  }

  if (!response.ok) {
    const message = getProbeEnvelopeError(payload)
      ?? (typeof payload === 'string' && payload.trim().length > 0 ? payload : null)
      ?? `Request failed with status ${response.status}`
    throw new McpProbeRequestError(message, response.status, payload, rawBody)
  }

  if (parseError) {
    throw new McpProbeRequestError(parseError.message, response.status, payload, rawBody)
  }

  if (!isMcpProbeEnvelope(payload)) {
    throw new McpProbeRequestError('Invalid MCP probe payload', response.status, payload, rawBody)
  }

  return payload as T
}
