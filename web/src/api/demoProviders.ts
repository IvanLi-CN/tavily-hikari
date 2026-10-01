/**
 * Browser demo provider surface (plan todo 25): REST route dispatch, the three
 * provider MCP endpoints (`/serpapi/mcp`, `/exa/mcp`, `/brave/mcp`), the
 * provider request-kind catalog, mixed-provider demo logs, and mixed-provider
 * demo keys.
 *
 * Fixture ground truth: `src/bin/mock_serpapi.rs`, `src/bin/mock_exa.rs`,
 * `src/bin/mock_brave.rs`; MCP catalogs mirror `src/server/provider_mcp_*`.
 * Everything here is answered locally in the browser — demo mode never calls a
 * network. The interception whitelist in `demo.ts` adds the three provider MCP
 * paths explicitly (M22) while unknown `/api/*` paths keep the pre-existing
 * demo fall-through shape.
 */
import { demoExaAnswerFixture, demoExaContentsFixture, demoExaMcpToolFixture, demoExaSearchFixture, EXA_MCP_TOOLS, EXA_STREAM_REJECT_MESSAGE, isDemoExaStreamTruthy } from './demoExa'
import { BRAVE_MCP_TOOLS, BRAVE_RATE_LIMIT_HEADERS, demoBraveLlmContextFixture, demoBraveMcpToolFixture, demoBraveNewsFixture, demoBraveWebFixture } from './demoBrave'
import { demoSerpapiMcpToolResult, demoSerpapiMcpTools, demoSerpapiSearchFixture } from './demoSerpapi'

type JsonObject = Record<string, unknown>

/** Shared MCP tool entry shape for the demo catalogs. */
export interface DemoMcpToolDefinition {
  name: string
  description: string
  inputSchema: Record<string, unknown>
}

const DEMO_PROVIDER_RESPONSE_HEADER = 'X-Tavily-Hikari-Demo'
const DEMO_MCP_SESSION_ID = 'demo-session-001'

function demoJsonResponse(payload: unknown, status = 200, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(payload), {
    status,
    headers: {
      'Content-Type': 'application/json; charset=utf-8',
      'Cache-Control': 'no-store',
      [DEMO_PROVIDER_RESPONSE_HEADER]: 'true',
      ...headers,
    },
  })
}

/** Same body-read contract as `demo.ts readJsonBody` (string fetch bodies). */
async function readDemoJsonBody(init?: RequestInit): Promise<JsonObject> {
  const body = init?.body
  if (typeof body === 'string') {
    try {
      const parsed = JSON.parse(body)
      return typeof parsed === 'object' && parsed ? (parsed as JsonObject) : {}
    } catch {
      return {}
    }
  }
  return {}
}

/** Exact provider MCP ingress paths (M22: explicit additions, not prefixes). */
export function isProviderMcpPath(path: string): boolean {
  return path === '/serpapi/mcp' || path === '/exa/mcp' || path === '/brave/mcp'
}

/**
 * Provider REST + MCP dispatch. Returns `null` for every path this module does
 * not own so `demo.ts` keeps its existing fall-through behavior (including the
 * unknown `/api/<other>/*` shape).
 */
export async function handleProviderDemoRoute(
  path: string,
  method: string,
  init?: RequestInit,
): Promise<Response | null> {
  if (path === '/api/serpapi/search' && (method === 'GET' || method === 'POST')) {
    return demoJsonResponse(demoSerpapiSearchFixture())
  }
  if (method === 'POST' && (path === '/api/exa/search' || path === '/api/exa/contents' || path === '/api/exa/answer')) {
    const body = await readDemoJsonBody(init)
    if (isDemoExaStreamTruthy(body.stream)) {
      return demoJsonResponse({ error: 'invalid_request', message: EXA_STREAM_REJECT_MESSAGE }, 400)
    }
    if (path === '/api/exa/search') return demoJsonResponse(demoExaSearchFixture())
    if (path === '/api/exa/contents') return demoJsonResponse(demoExaContentsFixture())
    return demoJsonResponse(demoExaAnswerFixture())
  }
  if (method === 'GET' && path === '/api/brave/res/v1/web/search') {
    return demoJsonResponse(demoBraveWebFixture(), 200, BRAVE_RATE_LIMIT_HEADERS)
  }
  if (method === 'GET' && path === '/api/brave/res/v1/news/search') {
    return demoJsonResponse(demoBraveNewsFixture())
  }
  if (method === 'GET' && path === '/api/brave/res/v1/llm/context') {
    return demoJsonResponse(demoBraveLlmContextFixture())
  }
  if (isProviderMcpPath(path)) return handleProviderMcpDemo(path, init)
  return null
}

function providerMcpTools(path: string): DemoMcpToolDefinition[] {
  if (path === '/serpapi/mcp') return demoSerpapiMcpTools()
  if (path === '/exa/mcp') return EXA_MCP_TOOLS
  return BRAVE_MCP_TOOLS
}

function providerMcpServerName(path: string): string {
  if (path === '/serpapi/mcp') return 'serpapi-hikari'
  if (path === '/exa/mcp') return 'exa-search-hikari'
  return 'brave-search-hikari'
}

function demoMcpResponse(payload: JsonObject, status = 200): Response {
  return new Response(JSON.stringify(payload), {
    status,
    headers: {
      'Content-Type': 'application/json; charset=utf-8',
      'Mcp-Session-Id': DEMO_MCP_SESSION_ID,
      [DEMO_PROVIDER_RESPONSE_HEADER]: 'true',
    },
  })
}

/**
 * Local-facade tool result envelope (mirrors
 * `local_mcp_facade.rs build_local_mcp_tool_result_body`): the fixture rides as
 * text content and as `structuredContent` with the upstream status projected.
 * The SerpApi passthrough keeps its own mock shape instead.
 */
function demoLocalFacadeToolResult(fixture: JsonObject): JsonObject {
  const text = JSON.stringify(fixture)
  return {
    content: [{ type: 'text', text }],
    structuredContent: { ...fixture, status: 200 },
  }
}

async function handleProviderMcpDemo(path: string, init?: RequestInit): Promise<Response> {
  const body = await readDemoJsonBody(init)
  const method = typeof body.method === 'string' ? body.method : ''
  const id = body.id ?? 'demo'
  if (method === 'notifications/initialized') {
    return demoMcpResponse({ jsonrpc: '2.0', result: null }, 202)
  }
  if (method === 'initialize') {
    return demoMcpResponse({
      jsonrpc: '2.0',
      id,
      result: {
        protocolVersion: '2025-03-26',
        capabilities: { tools: {} },
        serverInfo: { name: providerMcpServerName(path), version: 'demo' },
      },
    })
  }
  if (method === 'tools/list') {
    return demoMcpResponse({ jsonrpc: '2.0', id, result: { tools: providerMcpTools(path) } })
  }
  if (method === 'tools/call') {
    const params = (body.params ?? {}) as { name?: unknown }
    const toolName = typeof params.name === 'string' ? params.name : ''
    if (path === '/serpapi/mcp') {
      return demoMcpResponse({ jsonrpc: '2.0', id, result: demoSerpapiMcpToolResult() })
    }
    const fixture = path === '/exa/mcp' ? demoExaMcpToolFixture(toolName) : demoBraveMcpToolFixture(toolName)
    if (!fixture) {
      return demoMcpResponse({ jsonrpc: '2.0', id, error: { code: -32602, message: `Unknown tool: ${toolName}` } })
    }
    return demoMcpResponse({ jsonrpc: '2.0', id, result: demoLocalFacadeToolResult(fixture) })
  }
  if (method === 'ping') return demoMcpResponse({ jsonrpc: '2.0', id, result: {} })
  return demoMcpResponse({ jsonrpc: '2.0', id, result: {} })
}

interface DemoRequestKindOption {
  key: string
  label: string
  protocol_group: 'api' | 'mcp'
  billing_group: 'billable' | 'non_billable'
  count: number
}

/**
 * The 16 committed provider request kinds (plan todo 23 / M9, mirrored from
 * `tokenLogRequestKinds.ts` canonical labels and billing groups) so the demo
 * request-kind catalog stays consistent with the shipped filter vocabulary.
 */
const DEMO_PROVIDER_REQUEST_KINDS: DemoRequestKindOption[] = [
  { key: 'api:serpapi/search', label: 'API | serpapi/search', protocol_group: 'api', billing_group: 'billable', count: 9 },
  { key: 'api:exa/search', label: 'API | exa/search', protocol_group: 'api', billing_group: 'billable', count: 7 },
  { key: 'api:exa/contents', label: 'API | exa/contents', protocol_group: 'api', billing_group: 'billable', count: 5 },
  { key: 'api:exa/answer', label: 'API | exa/answer', protocol_group: 'api', billing_group: 'billable', count: 3 },
  { key: 'api:brave/web', label: 'API | brave/web', protocol_group: 'api', billing_group: 'billable', count: 8 },
  { key: 'api:brave/news', label: 'API | brave/news', protocol_group: 'api', billing_group: 'billable', count: 4 },
  { key: 'api:brave/llm-context', label: 'API | brave/llm-context', protocol_group: 'api', billing_group: 'billable', count: 2 },
  { key: 'mcp:serpapi/search', label: 'MCP | serpapi/search', protocol_group: 'mcp', billing_group: 'billable', count: 6 },
  { key: 'mcp:serpapi/search_table', label: 'MCP | serpapi/search_table', protocol_group: 'mcp', billing_group: 'billable', count: 3 },
  { key: 'mcp:serpapi/search_dashboard', label: 'MCP | serpapi/search_dashboard', protocol_group: 'mcp', billing_group: 'billable', count: 2 },
  { key: 'mcp:brave/web_search', label: 'MCP | brave/web_search', protocol_group: 'mcp', billing_group: 'billable', count: 5 },
  { key: 'mcp:brave/news_search', label: 'MCP | brave/news_search', protocol_group: 'mcp', billing_group: 'billable', count: 2 },
  { key: 'mcp:brave/llm_context', label: 'MCP | brave/llm_context', protocol_group: 'mcp', billing_group: 'billable', count: 2 },
  { key: 'mcp:exa/web_search', label: 'MCP | exa/web_search', protocol_group: 'mcp', billing_group: 'billable', count: 4 },
  { key: 'mcp:exa/web_fetch', label: 'MCP | exa/web_fetch', protocol_group: 'mcp', billing_group: 'billable', count: 2 },
  { key: 'mcp:exa/answer', label: 'MCP | exa/answer', protocol_group: 'mcp', billing_group: 'billable', count: 2 },
]

/** Fresh provider request-kind options for the demo catalog. */
export function createDemoProviderRequestKindOptions(): DemoRequestKindOption[] {
  return DEMO_PROVIDER_REQUEST_KINDS.map((option) => ({ ...option }))
}

/** One demo log seed per committed provider kind (mixed providers/statuses). */
interface DemoProviderLogSeed {
  kind: string
  provider: 'serpapi' | 'exa' | 'brave'
  method: string
  path: string
  query: string | null
  status: number
}

const DEMO_PROVIDER_LOG_SEEDS: DemoProviderLogSeed[] = [
  { kind: 'api:serpapi/search', provider: 'serpapi', method: 'GET', path: '/api/serpapi/search', query: 'engine=google&q=demo+mode', status: 200 },
  { kind: 'api:exa/search', provider: 'exa', method: 'POST', path: '/api/exa/search', query: null, status: 200 },
  { kind: 'api:exa/contents', provider: 'exa', method: 'POST', path: '/api/exa/contents', query: null, status: 200 },
  { kind: 'api:exa/answer', provider: 'exa', method: 'POST', path: '/api/exa/answer', query: null, status: 200 },
  { kind: 'api:brave/web', provider: 'brave', method: 'GET', path: '/api/brave/res/v1/web/search', query: 'q=demo+mode', status: 200 },
  { kind: 'api:brave/news', provider: 'brave', method: 'GET', path: '/api/brave/res/v1/news/search', query: 'q=demo+mode', status: 200 },
  { kind: 'api:brave/llm-context', provider: 'brave', method: 'GET', path: '/api/brave/res/v1/llm/context', query: 'q=demo+mode', status: 502 },
  { kind: 'mcp:serpapi/search', provider: 'serpapi', method: 'POST', path: '/serpapi/mcp', query: null, status: 200 },
  { kind: 'mcp:serpapi/search_table', provider: 'serpapi', method: 'POST', path: '/serpapi/mcp', query: null, status: 432 },
  { kind: 'mcp:serpapi/search_dashboard', provider: 'serpapi', method: 'POST', path: '/serpapi/mcp', query: null, status: 200 },
  { kind: 'mcp:brave/web_search', provider: 'brave', method: 'POST', path: '/brave/mcp', query: null, status: 200 },
  { kind: 'mcp:brave/news_search', provider: 'brave', method: 'POST', path: '/brave/mcp', query: null, status: 200 },
  { kind: 'mcp:brave/llm_context', provider: 'brave', method: 'POST', path: '/brave/mcp', query: null, status: 200 },
  { kind: 'mcp:exa/web_search', provider: 'exa', method: 'POST', path: '/exa/mcp', query: null, status: 200 },
  { kind: 'mcp:exa/web_fetch', provider: 'exa', method: 'POST', path: '/exa/mcp', query: null, status: 200 },
  { kind: 'mcp:exa/answer', provider: 'exa', method: 'POST', path: '/exa/mcp', query: null, status: 200 },
]

const DEMO_PROVIDER_KEY_IDS: Record<DemoProviderLogSeed['provider'], string> = {
  serpapi: 'Sp06',
  exa: 'Ex07',
  brave: 'Bv08',
}

/**
 * Demo log row shape consumed by the shared demo log surfaces; explicit
 * scalar fields keep the legacy `createDemoLog` union arithmetic typed.
 */
export interface DemoProviderLog extends Record<string, unknown> {
  id: number
  provider: string
  auth_token_id: string
  key_id: string
  method: string
  path: string
  query: string | null
  http_status: number
  mcp_status: number | null
  result_status: string
  created_at: number
  error_message: string | null
  request_body: string
  response_body: string
}

/**
 * Mixed-provider demo logs appended after the legacy (provider-less) Tavily
 * rows: one row per committed provider kind, all 16 kinds represented, with
 * success plus one quota-exhausted and one upstream-error row.
 */
export function createDemoProviderLogs(): DemoProviderLog[] {
  const now = Math.floor(Date.now() / 1000)
  return DEMO_PROVIDER_LOG_SEEDS.map((seed, index) => {
    const success = seed.status === 200
    const quota = seed.status === 432
    const option = DEMO_PROVIDER_REQUEST_KINDS.find((item) => item.key === seed.kind) ?? {
      key: seed.kind,
      label: seed.kind,
      protocol_group: 'api' as const,
      billing_group: 'non_billable' as const,
    }
    const mcp = seed.path.endsWith('/mcp')
    return {
      id: 9064 + index,
      provider: seed.provider,
      key_id: DEMO_PROVIDER_KEY_IDS[seed.provider],
      auth_token_id: index % 3 === 0 ? 'ops2' : 'dm01',
      method: seed.method,
      path: seed.path,
      query: seed.query,
      http_status: seed.status,
      mcp_status: mcp ? (success ? 0 : -32603) : null,
      business_credits: success && option.billing_group === 'billable' ? 1 : 0,
      request_kind_key: option.key,
      request_kind_label: option.label,
      request_kind_detail: option.billing_group === 'billable' ? 'Billable request' : 'Control-plane request',
      result_status: success ? 'success' : quota ? 'quota_exhausted' : 'error',
      created_at: now - (index + 1) * 600,
      error_message: success ? null : quota ? 'Demo provider quota exhausted' : 'Demo provider upstream error',
      failure_kind: success ? null : quota ? 'upstream_usage_limit_432' : 'upstream_error',
      key_effect_code: success ? 'none' : quota ? 'marked_exhausted' : 'transient_backoff_set',
      key_effect_summary: success ? null : quota ? 'Marked quota exhausted' : 'Applied transient upstream backoff',
      binding_effect_code: 'none',
      binding_effect_summary: null,
      selection_effect_code: success ? 'none' : 'http_project_affinity_pressure_avoided',
      selection_effect_summary: success ? null : 'Fallback path used',
      gateway_mode: mcp ? 'mcp' : 'http',
      experiment_variant: 'primary',
      proxy_session_id: `demo-provider-${index % 4}`,
      routing_subject_hash: `demo-provider-${index % 5}`,
      upstream_operation: option.label,
      fallback_reason: success ? null : 'demo-backoff',
      request_body: JSON.stringify({ query: 'demo mode' }, null, 2),
      response_body: success
        ? JSON.stringify({ provider: seed.provider, result: 'demo' }, null, 2)
        : JSON.stringify({ error: quota ? 'quota_exhausted' : 'upstream_error' }, null, 2),
      forwarded_headers: ['authorization', 'content-type'],
      dropped_headers: ['cookie', 'x-real-ip'],
      remote_addr: `203.0.113.${100 + (index % 40)}`,
      client_ip: `198.51.100.${60 + (index % 20)}`,
      client_ip_source: 'x-forwarded-for',
      client_ip_trusted: true,
      ip_headers: [{ name: 'x-forwarded-for', value: `198.51.100.${60 + (index % 20)}`, trusted: true }],
      operationalClass: success ? 'success' : quota ? 'quota_exhausted' : 'upstream_error',
      requestKindProtocolGroup: option.protocol_group,
      requestKindBillingGroup: option.billing_group,
    }
  })
}

/**
 * Mixed-provider demo keys (one per provider, sensible statuses): SerpApi
 * active with live `/account` quota + renewal date, Exa active with local
 * counting, Brave exhausted with passively harvested month-window quota. The
 * legacy Tavily demo keys intentionally stay provider-less so the T21
 * normalizer path remains exercised in demo mode.
 */
export function createDemoProviderKeys(): Array<Record<string, unknown>> {
  const now = Math.floor(Date.now() / 1000)
  const renewalAt = now + 86400 * 13
  return [
    {
      id: DEMO_PROVIDER_KEY_IDS.serpapi,
      provider: 'serpapi',
      status: 'active',
      group: 'serpapi',
      registration_ip: '203.0.113.61',
      registration_region: 'US',
      status_changed_at: now - 3600 * 6,
      last_used_at: now - 240,
      deleted_at: null,
      quota_limit: 30000,
      quota_remaining: 28740,
      quota_renews_at: renewalAt,
      quota_source: 'live',
      quota_synced_at: now - 600,
      total_requests: 1260,
      success_count: 1231,
      error_count: 29,
      quota_exhausted_count: 0,
      quarantine: null,
      transient_backoff: null,
    },
    {
      id: DEMO_PROVIDER_KEY_IDS.exa,
      provider: 'exa',
      status: 'active',
      group: 'exa',
      registration_ip: '198.51.100.32',
      registration_region: 'HK',
      status_changed_at: now - 3600 * 4,
      last_used_at: now - 420,
      deleted_at: null,
      quota_limit: 2000,
      quota_remaining: 1740,
      quota_synced_at: now - 1200,
      total_requests: 486,
      success_count: 470,
      error_count: 16,
      quota_exhausted_count: 0,
      quarantine: null,
      transient_backoff: null,
    },
    {
      id: DEMO_PROVIDER_KEY_IDS.brave,
      provider: 'brave',
      status: 'exhausted',
      group: 'brave',
      registration_ip: '198.51.100.44',
      registration_region: 'HK',
      status_changed_at: now - 3600 * 12,
      last_used_at: now - 86400,
      deleted_at: null,
      quota_limit: 1000,
      quota_remaining: 0,
      quota_renews_at: renewalAt,
      quota_source: 'passive',
      quota_synced_at: now - 300,
      total_requests: 812,
      success_count: 780,
      error_count: 32,
      quota_exhausted_count: 11,
      quarantine: null,
      transient_backoff: null,
    },
  ]
}