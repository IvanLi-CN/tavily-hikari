export type ProbeQuotaWindow = 'hour' | 'day' | 'month'
export type McpProbeStepState = 'success' | 'failed' | 'blocked' | 'skipped'

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

export interface McpProbeResponseMetadata {
  status: number
  headers: Headers
  sessionId: string | null
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

function numericStatus(value: unknown): number | null {
  if (typeof value === 'number' && Number.isFinite(value)) return value
  if (typeof value !== 'string') return null
  const parsed = Number(value)
  return Number.isFinite(parsed) ? parsed : null
}

function extractProbeErrorText(payload: Record<string, unknown>): string | null {
  const directError = payload.error
  if (typeof directError === 'string' && directError.trim().length > 0) {
    return directError
  }
  const directErrorObject = asRecord(directError)
  const directErrorMessage = directErrorObject?.message
  if (typeof directErrorMessage === 'string' && directErrorMessage.trim().length > 0) {
    return directErrorMessage
  }

  const message = payload.message
  if (typeof message === 'string' && message.trim().length > 0) {
    return message
  }

  const content = Array.isArray(payload.content) ? payload.content : []
  for (const item of content) {
    const contentItem = asRecord(item)
    if (!contentItem) continue
    if (contentItem.type === 'error') {
      const text = contentItem.text
      if (typeof text === 'string' && text.trim().length > 0) {
        return text
      }
      return 'Request failed'
    }
  }

  const detail = asRecord(payload.detail)
  return detail ? extractProbeErrorText(detail) : null
}

function extractProbeStatusCode(payload: Record<string, unknown>): number | null {
  const directStatus = numericStatus(payload.status)
  if (directStatus !== null) return directStatus

  const detail = asRecord(payload.detail)
  return detail ? extractProbeStatusCode(detail) : null
}

export function getMcpProbeResultError(payload: unknown): string | null {
  const result = asRecord(asRecord(payload)?.result)
  if (!result) return null

  const directError = extractProbeErrorText(result)
  if (directError) return directError

  const structuredContent = asRecord(result.structuredContent)
  if (structuredContent) {
    const structuredError = extractProbeErrorText(structuredContent)
    if (structuredError) return structuredError

    const structuredStatus = extractProbeStatusCode(structuredContent)
    if (structuredStatus !== null && (structuredStatus < 200 || structuredStatus >= 300)) {
      return `Request failed with status ${structuredStatus}`
    }
  }

  if (result.isError === true) {
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
  const failing = stepStates.filter((state) => state === 'failed' || state === 'blocked').length
  const skipped = stepStates.filter((state) => state === 'skipped').length
  const passed = stepStates.filter((state) => state === 'success').length
  if (failing === 0 && skipped === 0) return 'success'
  if (passed === 0) return 'failed'
  return 'partial'
}

async function requestMcpProbeWithTokenInternal<T>(
  input: RequestInfo,
  token: string,
  init?: RequestInit,
  options?: {
    allowEmptySuccess?: boolean
    requireEnvelope?: boolean
  },
): Promise<{ payload: T | null, metadata: McpProbeResponseMetadata }> {
  const headers = new Headers(init?.headers ?? {})
  headers.set('Authorization', `Bearer ${token}`)
  headers.set('Accept', MCP_PROBE_ACCEPT_HEADER)
  if (init?.body != null && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json')
  }

  const response = await fetch(input, { ...init, headers })
  const rawBody = await response.text().catch(() => '')
  const metadata: McpProbeResponseMetadata = {
    status: response.status,
    headers: new Headers(response.headers),
    sessionId: response.headers.get('mcp-session-id'),
  }

  let payload: unknown = null
  let parseError: Error | null = null
  if (rawBody.trim().length > 0) {
    try {
      payload = parseMcpProbePayload(rawBody)
    } catch (err) {
      parseError = err instanceof Error ? err : new Error('Invalid MCP probe payload')
      payload = rawBody
    }
  } else if (response.ok && !options?.allowEmptySuccess) {
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

  if (options?.requireEnvelope !== false && !isMcpProbeEnvelope(payload)) {
    throw new McpProbeRequestError('Invalid MCP probe payload', response.status, payload, rawBody)
  }

  return { payload: payload as T | null, metadata }
}

export async function requestMcpProbeEnvelopeWithToken<T>(
  input: RequestInfo,
  token: string,
  init?: RequestInit,
): Promise<{ payload: T, metadata: McpProbeResponseMetadata }> {
  const response = await requestMcpProbeWithTokenInternal<T>(input, token, init, {
    requireEnvelope: true,
  })

  if (response.payload == null) {
    throw new McpProbeRequestError('Empty MCP probe payload', response.metadata.status, null, '')
  }

  return {
    payload: response.payload,
    metadata: response.metadata,
  }
}

export async function requestMcpProbeNotificationWithToken<T = unknown>(
  input: RequestInfo,
  token: string,
  init?: RequestInit,
): Promise<{ payload: T | null, metadata: McpProbeResponseMetadata }> {
  const response = await requestMcpProbeWithTokenInternal(input, token, init, {
    allowEmptySuccess: true,
    requireEnvelope: false,
  })
  return {
    payload: response.payload as T | null,
    metadata: response.metadata,
  }
}

export async function requestMcpProbeWithToken<T>(
  input: RequestInfo,
  token: string,
  init?: RequestInit,
): Promise<T> {
  const response = await requestMcpProbeEnvelopeWithToken<T>(input, token, init)
  return response.payload
}
