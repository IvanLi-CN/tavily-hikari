import { afterEach, beforeEach, describe, expect, it } from 'bun:test'

import {
  MCP_PROBE_ACCEPT_HEADER,
  McpProbeRequestError,
  getQuotaExceededWindow,
  getTokenBusinessQuotaWindow,
  parseMcpProbePayload,
  requestMcpProbeEnvelopeWithToken,
  requestMcpProbeNotificationWithToken,
  requestMcpProbeWithToken,
  revalidateBlockedQuotaWindow,
  resolveMcpProbeButtonState,
} from './mcpProbe'

describe('mcpProbe helpers', () => {
  const originalFetch = globalThis.fetch

  beforeEach(() => {
    globalThis.fetch = originalFetch
  })

  afterEach(() => {
    globalThis.fetch = originalFetch
  })

  it('parses SSE payloads into JSON envelopes', () => {
    const payload = parseMcpProbePayload(
      'event: message\n' +
      'data: {"jsonrpc":"2.0","id":"tools","result":{"tools":[{"name":"tavily_search"}]}}\n\n',
    )

    expect(payload.result.tools[0]?.name).toBe('tavily_search')
  })

  it('prefers the response envelope when SSE includes trailing notifications', () => {
    const payload = parseMcpProbePayload(
      'event: message\n' +
      'data: {"jsonrpc":"2.0","id":"tools","result":{"tools":[{"name":"tavily_search"}]}}\n\n' +
      'event: message\n' +
      'data: {"jsonrpc":"2.0","method":"notifications/message","params":{"level":"info","data":"done"}}\n\n',
    )

    expect(payload.result.tools[0]?.name).toBe('tavily_search')
  })

  it('ignores non-JSON SSE events before the response envelope', () => {
    const payload = parseMcpProbePayload(
      'event: endpoint\n' +
      'data: /mcp/messages?sessionId=abc\n\n' +
      'event: message\n' +
      'data: {"jsonrpc":"2.0","id":"tools","result":{"tools":[{"name":"tavily_search"}]}}\n\n',
    )

    expect(payload.result.tools[0]?.name).toBe('tavily_search')
  })

  it('detects quota-exhausted windows from payloads', () => {
    expect(getQuotaExceededWindow({ error: 'quota_exceeded', window: 'day' })).toBe('day')
    expect(getQuotaExceededWindow({ error: 'quota_exceeded', window: 'month' })).toBe('month')
    expect(getQuotaExceededWindow({ error: 'other' })).toBeNull()
  })

  it('detects business quota exhaustion from token snapshots', () => {
    expect(getTokenBusinessQuotaWindow({
      quotaHourlyUsed: 99,
      quotaHourlyLimit: 100,
      quotaDailyUsed: 500,
      quotaDailyLimit: 500,
      quotaMonthlyUsed: 4000,
      quotaMonthlyLimit: 5000,
    })).toBe('day')

    expect(getTokenBusinessQuotaWindow({
      quotaHourlyUsed: 100,
      quotaHourlyLimit: 100,
      quotaDailyUsed: 10,
      quotaDailyLimit: 500,
      quotaMonthlyUsed: 10,
      quotaMonthlyLimit: 5000,
    })).toBe('hour')
  })

  it('maps blocked plus success into partial button state', () => {
    expect(resolveMcpProbeButtonState(['blocked', 'success'])).toBe('partial')
    expect(resolveMcpProbeButtonState(['blocked', 'failed'])).toBe('failed')
    expect(resolveMcpProbeButtonState(['success', 'success'])).toBe('success')
    expect(resolveMcpProbeButtonState(['success', 'skipped'])).toBe('partial')
  })

  it('revalidates blocked snapshots against a fresh quota read', async () => {
    let calls = 0
    const blocked = {
      quotaHourlyUsed: 100,
      quotaHourlyLimit: 100,
      quotaDailyUsed: 100,
      quotaDailyLimit: 500,
      quotaMonthlyUsed: 100,
      quotaMonthlyLimit: 5000,
    }
    const fresh = {
      quotaHourlyUsed: 0,
      quotaHourlyLimit: 100,
      quotaDailyUsed: 100,
      quotaDailyLimit: 500,
      quotaMonthlyUsed: 100,
      quotaMonthlyLimit: 5000,
    }

    const result = await revalidateBlockedQuotaWindow(blocked, async () => {
      calls += 1
      return fresh
    })

    expect(calls).toBe(1)
    expect(result.token).toBe(fresh)
    expect(result.window).toBeNull()
  })

  it('skips quota revalidation when the cached snapshot is already available', async () => {
    let calls = 0
    const available = {
      quotaHourlyUsed: 0,
      quotaHourlyLimit: 100,
      quotaDailyUsed: 100,
      quotaDailyLimit: 500,
      quotaMonthlyUsed: 100,
      quotaMonthlyLimit: 5000,
    }

    const result = await revalidateBlockedQuotaWindow(available, async () => {
      calls += 1
      return available
    })

    expect(calls).toBe(0)
    expect(result.token).toBe(available)
    expect(result.window).toBeNull()
  })

  it('sends MCP Accept headers and parses SSE success responses', async () => {
    let headers = null
    globalThis.fetch = async (_input, init) => {
      headers = new Headers(init?.headers ?? {})
      return new Response(
        'event: message\n' +
          'data: {"jsonrpc":"2.0","id":"probe-tools-list","result":{"tools":[{"name":"tavily_search"}]}}\n\n',
        {
          status: 200,
          headers: { 'Content-Type': 'text/event-stream' },
        },
      )
    }

    const payload = await requestMcpProbeWithToken('/mcp', 'th-a1b2-secret', {
      method: 'POST',
      body: JSON.stringify({ method: 'tools/list' }),
    })

    expect(headers?.get('Authorization')).toBe('Bearer th-a1b2-secret')
    expect(headers?.get('Accept')).toBe(MCP_PROBE_ACCEPT_HEADER)
    expect(headers?.get('Content-Type')).toBe('application/json')
    expect(payload.result.tools[0]?.name).toBe('tavily_search')
  })

  it('returns response metadata for initialize envelopes and captures mcp-session-id', async () => {
    globalThis.fetch = async () => {
      return new Response(
        JSON.stringify({
          jsonrpc: '2.0',
          id: 'init-1',
          result: {
            protocolVersion: '2025-03-26',
            capabilities: {},
          },
        }),
        {
          status: 200,
          headers: {
            'Content-Type': 'application/json',
            'Mcp-Session-Id': 'session-123',
          },
        },
      )
    }

    const response = await requestMcpProbeEnvelopeWithToken('/mcp', 'th-a1b2-secret', {
      method: 'POST',
      headers: {
        'Mcp-Protocol-Version': '2025-03-26',
      },
      body: JSON.stringify({ method: 'initialize' }),
    })

    expect(response.metadata.status).toBe(200)
    expect(response.metadata.sessionId).toBe('session-123')
    expect(response.payload.result.protocolVersion).toBe('2025-03-26')
  })

  it('accepts notification-only 202 responses without requiring a JSON-RPC envelope', async () => {
    let headers = null
    globalThis.fetch = async (_input, init) => {
      headers = new Headers(init?.headers ?? {})
      return new Response(null, {
        status: 202,
      })
    }

    const metadata = await requestMcpProbeNotificationWithToken('/mcp', 'th-a1b2-secret', {
      method: 'POST',
      headers: {
        'Mcp-Protocol-Version': '2025-03-26',
        'Mcp-Session-Id': 'session-123',
      },
      body: JSON.stringify({ method: 'notifications/initialized' }),
    })

    expect(metadata.metadata.status).toBe(202)
    expect(metadata.payload).toBeNull()
    expect(headers?.get('Mcp-Protocol-Version')).toBe('2025-03-26')
    expect(headers?.get('Mcp-Session-Id')).toBe('session-123')
  })

  it('keeps notification error envelopes available to callers even on HTTP 2xx', async () => {
    globalThis.fetch = async () => {
      return new Response(
        JSON.stringify({
          jsonrpc: '2.0',
          error: {
            code: -32600,
            message: 'initialized rejected',
          },
        }),
        {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        },
      )
    }

    const response = await requestMcpProbeNotificationWithToken('/mcp', 'th-a1b2-secret', {
      method: 'POST',
      body: JSON.stringify({ method: 'notifications/initialized' }),
    })

    expect(response.metadata.status).toBe(200)
    expect(response.payload?.error?.message).toBe('initialized rejected')
  })

  it('rejects malformed 2xx probe payloads instead of treating them as success', async () => {
    globalThis.fetch = async () => {
      return new Response('<html>oops</html>', {
        status: 200,
        headers: { 'Content-Type': 'text/html' },
      })
    }

    try {
      await requestMcpProbeWithToken('/mcp', 'th-a1b2-secret', {
        method: 'POST',
        body: JSON.stringify({ method: 'tools/list' }),
      })
      throw new Error('expected requestMcpProbeWithToken to throw')
    } catch (err) {
      expect(err).toBeInstanceOf(McpProbeRequestError)
      expect(err.message).toContain('Invalid MCP probe payload')
    }
  })

  it('throws parsed quota errors for non-2xx MCP responses', async () => {
    globalThis.fetch = async () => {
      return new Response(
        JSON.stringify({
          error: 'quota_exceeded',
          window: 'day',
          daily: { limit: 500, used: 500 },
        }),
        {
          status: 429,
          headers: { 'Content-Type': 'application/json' },
        },
      )
    }

    await expect(requestMcpProbeWithToken('/mcp', 'th-a1b2-secret', {
      method: 'POST',
      body: JSON.stringify({ method: 'ping' }),
    })).rejects.toBeInstanceOf(McpProbeRequestError)

    try {
      await requestMcpProbeWithToken('/mcp', 'th-a1b2-secret', {
        method: 'POST',
        body: JSON.stringify({ method: 'ping' }),
      })
      throw new Error('expected requestMcpProbeWithToken to throw')
    } catch (err) {
      expect(err).toBeInstanceOf(McpProbeRequestError)
      const probeErr = err
      expect(probeErr.status).toBe(429)
      expect(getQuotaExceededWindow(probeErr.payload)).toBe('day')
    }
  })

  it('surfaces 406 accept negotiation failures as structured errors', async () => {
    globalThis.fetch = async () => {
      return new Response(
        JSON.stringify({
          jsonrpc: '2.0',
          id: 'server-error',
          error: {
            code: -32600,
            message: 'Not Acceptable: Client must accept both application/json and text/event-stream',
          },
        }),
        {
          status: 406,
          headers: { 'Content-Type': 'application/json' },
        },
      )
    }

    try {
      await requestMcpProbeWithToken('/mcp', 'th-a1b2-secret', {
        method: 'POST',
        body: JSON.stringify({ method: 'tools/list' }),
      })
      throw new Error('expected requestMcpProbeWithToken to throw')
    } catch (err) {
      expect(err).toBeInstanceOf(McpProbeRequestError)
      expect(err.message).toContain('Client must accept both application/json and text/event-stream')
    }
  })
})
