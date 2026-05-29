import {
  createDemoAnnouncements,
  demoUserActiveAnnouncements,
  demoUserAnnouncementHistory,
  handleAnnouncementsRoute,
} from './demoAnnouncements'
import { createDemoRechargeOrders, demoAdminUserRechargeAudit, handleDemoAdminRechargeAction, handleDemoAdminRecharges, type DemoRechargeOrder } from './demoAdminRecharge'
type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue }
type DemoListener = EventListenerOrEventListenerObject

declare global {
  interface Window {
    __tavilyHikariDemoInstalled?: boolean
    __tavilyHikariDemoFetch?: typeof fetch
    __tavilyHikariDemoEventSource?: typeof EventSource
  }
}

const DEMO_STORAGE_KEY = 'tavily-hikari-demo-mode'
const DEMO_TOKEN = 'th-dm01-demoaccesssecret'
const DEMO_TOKEN_ID = 'dm01'
const DEMO_KEY_ID = 'Hk01'
const DEMO_BACKUP_KEY_ID = 'Sf02'
const DEMO_EU_KEY_ID = 'Fr03'
const DEMO_QUOTA_KEY_ID = 'Qt04'
const DEMO_QUARANTINE_KEY_ID = 'Qr05'
const DEMO_TOKEN_OWNER = { userId: 'user-demo-admin', displayName: 'Hikari Demo Admin', username: 'hikari-demo' }
const DEMO_RECHARGE_UNIT_CREDITS = 1000
const DEMO_RECHARGE_UNIT_PRICE_LDC = 50
const DEMO_TEST_RECHARGE_CREDITS = 1
const DEMO_TEST_RECHARGE_MONTHS = 1
const DEMO_TEST_RECHARGE_AMOUNT_LDC = 1

function truthy(value: string | boolean | undefined | null): boolean {
  if (value === true) return true
  if (typeof value !== 'string') return false
  return ['1', 'true', 'yes', 'on', 'demo'].includes(value.trim().toLowerCase())
}

export function isDemoMode(): boolean {
  const env = (import.meta as unknown as { env?: { VITE_DEMO_MODE?: string } }).env
  if (truthy(env?.VITE_DEMO_MODE)) return true
  if (typeof window === 'undefined') return false
  const params = new URLSearchParams(window.location.search)
  if (truthy(params.get('demo')) || params.get('mode') === 'demo') return true
  try {
    return truthy(window.localStorage.getItem(DEMO_STORAGE_KEY))
  } catch {
    return false
  }
}

function nowSeconds(offset = 0): number {
  return Math.floor(Date.now() / 1000) + offset
}

function isoSeconds(offset = 0): string {
  return new Date((nowSeconds(offset)) * 1000).toISOString()
}

function monthStartSeconds(monthOffset = 0): number {
  const now = new Date()
  return Math.floor(new Date(now.getFullYear(), now.getMonth() + monthOffset, 1).getTime() / 1000)
}

function demoPulse(now = Date.now()): number {
  return Math.floor(now / 6000)
}

function range(count: number): number[] {
  return Array.from({ length: count }, (_, index) => index)
}

function createRequestKindOptions() {
  return [
    { key: 'api:search', label: 'API | search', protocol_group: 'api', billing_group: 'billable', count: 34 },
    { key: 'api:extract', label: 'API | extract', protocol_group: 'api', billing_group: 'billable', count: 18 },
    { key: 'mcp:search', label: 'MCP | search', protocol_group: 'mcp', billing_group: 'billable', count: 26 },
    { key: 'mcp:tools/list', label: 'MCP | tools/list', protocol_group: 'mcp', billing_group: 'non_billable', count: 12 },
    { key: 'mcp:ping', label: 'MCP | ping', protocol_group: 'mcp', billing_group: 'non_billable', count: 8 },
  ]
}

const requestKindOptions = createRequestKindOptions()

function createDemoLog(index: number) {
  const success = index % 7 !== 0
  const quota = index % 19 === 0
  const rebalance = index % 11 === 0
  const mcp = !rebalance && index % 3 === 0
  const kind = mcp ? (index % 2 === 0 ? 'mcp:search' : 'mcp:tools/list') : (index % 2 === 0 ? 'api:search' : 'api:extract')
  const option = requestKindOptions.find((item) => item.key === kind) ?? requestKindOptions[0]
  return {
    id: 9000 + index,
    key_id: rebalance || index % 5 === 0 ? DEMO_BACKUP_KEY_ID : DEMO_KEY_ID,
    auth_token_id: index % 4 === 0 ? 'ops2' : DEMO_TOKEN_ID,
    method: mcp ? 'POST' : (kind === 'api:extract' ? 'POST' : 'GET'),
    path: mcp ? '/mcp' : `/api/tavily/${kind.endsWith('extract') ? 'extract' : 'search'}`,
    query: kind === 'api:search' ? 'query=demo+mode&topic=general' : null,
    http_status: success ? 200 : quota ? 432 : 502,
    mcp_status: mcp ? (success ? 0 : -32603) : null,
    business_credits: success && option.billing_group === 'billable' ? 1 : 0,
    request_kind_key: option.key,
    request_kind_label: option.label,
    request_kind_detail: option.billing_group === 'billable' ? 'Billable request' : 'Control-plane request',
    result_status: success ? 'success' : quota ? 'quota_exhausted' : 'error',
    created_at: nowSeconds(-index * 900),
    error_message: success ? null : quota ? 'Demo upstream monthly quota exhausted' : 'Demo upstream timeout',
    failure_kind: success ? null : quota ? 'upstream_usage_limit_432' : 'upstream_error',
    key_effect_code: success ? 'none' : quota ? 'marked_exhausted' : 'transient_backoff_set',
    key_effect_summary: success ? null : quota ? 'Marked quota exhausted' : 'Applied transient upstream backoff',
    binding_effect_code: rebalance ? (quota ? 'api_rebalance_route_rebound' : 'api_rebalance_route_reused') : (index % 6 === 0 ? 'http_project_affinity_reused' : 'none'),
    binding_effect_summary: rebalance ? (quota ? 'API rebalance rebound this route to a backup key' : 'API rebalance reused the current route binding') : (index % 6 === 0 ? 'Sticky user binding preserved' : null),
    selection_effect_code: rebalance ? (quota ? 'api_rebalance_rate_limit_avoided' : 'api_rebalance_pressure_avoided') : (success ? 'none' : 'http_project_affinity_pressure_avoided'),
    selection_effect_summary: rebalance ? (quota ? 'API rebalance avoided a rate-limited key' : 'API rebalance skipped a hotter key') : (success ? null : 'Fallback path used'),
    gateway_mode: rebalance ? 'rebalance_http' : mcp ? 'mcp' : 'http',
    experiment_variant: rebalance ? 'rebalance' : index % 2 === 0 ? 'primary' : 'secondary',
    proxy_session_id: rebalance ? `rebalance-demo-${index % 4}` : `demo-proxy-${index % 4}`,
    routing_subject_hash: `demo-${index % 9}`,
    upstream_operation: option.label,
    fallback_reason: success ? null : rebalance ? 'api-rebalance' : 'demo-backoff',
    request_body: JSON.stringify({ query: 'demo mode', max_results: 5 }, null, 2),
    response_body: success
      ? JSON.stringify({ answer: 'Mocked Tavily result for demo mode', results: [{ title: 'Demo result', url: 'https://example.test/demo' }] }, null, 2)
      : JSON.stringify({ error: quota ? 'quota_exhausted' : 'upstream_timeout' }, null, 2),
    forwarded_headers: ['authorization', 'content-type'],
    dropped_headers: ['cookie', 'x-real-ip'],
    remote_addr: `203.0.113.${10 + (index % 40)}`,
    client_ip: `198.51.100.${20 + (index % 20)}`,
    client_ip_source: index % 3 === 0 ? 'cf-connecting-ip' : 'x-forwarded-for',
    client_ip_trusted: true,
    ip_headers: [{ name: 'cf-connecting-ip', value: `198.51.100.${20 + (index % 20)}`, trusted: true }],
    operationalClass: success ? 'success' : quota ? 'quota_exhausted' : 'upstream_error',
    requestKindProtocolGroup: option.protocol_group,
    requestKindBillingGroup: option.billing_group,
  }
}

function createDemoState() {
  const logs = range(64).map(createDemoLog)
  const tokens = [
    {
      id: DEMO_TOKEN_ID,
      enabled: true,
      note: 'Demo operator token',
      group: 'demo',
      owner: { userId: 'user-demo-admin', displayName: 'Hikari Demo Admin', username: 'hikari-demo' },
      total_requests: 1842,
      created_at: nowSeconds(-86400 * 22),
      last_used_at: nowSeconds(-120),
      quota_state: 'normal',
      quota_hourly_used: 42,
      quota_hourly_limit: 180,
      quota_daily_used: 388,
      quota_daily_limit: 1600,
      quota_monthly_used: 8400,
      quota_monthly_limit: 24000,
      quota_hourly_reset_at: nowSeconds(2500),
      quota_daily_reset_at: nowSeconds(3600 * 8),
      quota_monthly_reset_at: nowSeconds(86400 * 13),
    },
    {
      id: 'ops2',
      enabled: true,
      note: 'Ops automation',
      group: 'internal',
      owner: { userId: 'user-ops', displayName: 'Ops Runner', username: 'ops-runner' },
      total_requests: 954,
      created_at: nowSeconds(-86400 * 44),
      last_used_at: nowSeconds(-920),
      quota_state: 'day',
      quota_hourly_used: 12,
      quota_hourly_limit: 80,
      quota_daily_used: 760,
      quota_daily_limit: 800,
      quota_monthly_used: 7800,
      quota_monthly_limit: 12000,
      quota_hourly_reset_at: nowSeconds(2200),
      quota_daily_reset_at: nowSeconds(3600 * 8),
      quota_monthly_reset_at: nowSeconds(86400 * 13),
    },
  ]
  const keys = [
    createDemoKey(DEMO_KEY_ID, 'active', 'primary', 49000, 38640, 1230, 1122, 88, null),
    createDemoKey(DEMO_BACKUP_KEY_ID, 'active', 'backup', 25000, 16120, 612, 574, 27, null),
    createDemoKey(DEMO_EU_KEY_ID, 'temporarily_isolated', 'eu', 12000, 8500, 226, 206, 16, {
      reasonCode: 'temporary_backoff',
      cooldownUntil: nowSeconds(780),
      retryAfterSecs: 780,
      scopes: ['api', 'mcp'],
    }),
    createDemoKey(DEMO_QUOTA_KEY_ID, 'exhausted', 'primary', 10000, 0, 901, 816, 85, null),
    createDemoKey(DEMO_QUARANTINE_KEY_ID, 'quarantined', 'legacy', 8000, 3100, 144, 105, 39, null, {
      source: 'upstream',
      reasonCode: 'upstream_key_blocked',
      reasonSummary: 'Blocked by upstream during demo validation',
      reasonDetail: 'This is mock data used by the standalone web demo.',
      createdAt: nowSeconds(-3600 * 9),
    }),
  ]
  return {
    profile: {
      displayName: 'Hikari Demo Admin',
      isAdmin: true,
      forwardAuthEnabled: false,
      builtinAuthEnabled: true,
      allowRegistration: false,
      userLoggedIn: true,
      userProvider: 'linuxdo',
      userDisplayName: 'Hikari Demo Admin',
      userAvatarUrl: null,
    },
    version: { backend: 'demo-web', frontend: '0.1.0-demo' },
    tokens,
    tokenSecrets: new Map(tokens.map((token) => [token.id, token.id === DEMO_TOKEN_ID ? DEMO_TOKEN : `th-${token.id}-demoaccesssecret`])),
    keys,
    logs,
    announcements: createDemoAnnouncements(nowSeconds),
    users: createDemoUsers(),
    jobs: createDemoJobs(),
    forwardProxy: createDemoForwardProxy(),
    systemSettings: createDemoSystemSettings(),
    rechargeOrders: createDemoRechargeOrders(nowSeconds, window.location.origin),
    userTags: [createDemoUserTag('tag-demo', {
      name: 'demo',
      displayName: 'Demo',
      icon: 'sparkles',
      effectKind: 'quota_delta',
      hourlyAnyDelta: 20,
      hourlyDelta: 20,
      dailyDelta: 200,
      monthlyDelta: 2000,
    }, 3)],
    registration: { allowRegistration: false },
  }
}

function demoRechargeSummary() {
  return {
    currentMonthStart: monthStartSeconds(),
    currentEntitlementCredits: DEMO_TEST_RECHARGE_CREDITS,
    effectiveUntilMonthStart: monthStartSeconds(1),
  }
}

function demoRechargeConfig() {
  return {
    visible: true,
    enabled: true,
    unitCredits: DEMO_RECHARGE_UNIT_CREDITS,
    unitPriceLdc: DEMO_RECHARGE_UNIT_PRICE_LDC,
    minCredits: DEMO_RECHARGE_UNIT_CREDITS,
    maxCredits: 20_000,
    creditsStep: DEMO_RECHARGE_UNIT_CREDITS,
    defaultCredits: DEMO_TEST_RECHARGE_CREDITS,
    minMonths: 1,
    maxMonths: 12,
    quotaDeltaBaseCredits: DEMO_RECHARGE_UNIT_CREDITS,
    hourlyDeltaPerQuotaUnit: 20,
    dailyDeltaPerQuotaUnit: 100,
    monthlyDeltaPerQuotaUnit: 1000,
    testPriceEnabled: true,
    ...demoRechargeSummary(),
  }
}

function createDemoAuthToken(id: string, note: string | null, createdAt = nowSeconds()) {
  return {
    id,
    enabled: true,
    note: note ?? 'Story token',
    group: 'demo',
    owner: DEMO_TOKEN_OWNER,
    total_requests: 0,
    created_at: createdAt,
    last_used_at: createdAt,
    quota_state: 'normal',
    quota_hourly_used: 0,
    quota_hourly_limit: 180,
    quota_daily_used: 0,
    quota_daily_limit: 1600,
    quota_monthly_used: 0,
    quota_monthly_limit: 24000,
    quota_hourly_reset_at: nowSeconds(2500),
    quota_daily_reset_at: nowSeconds(3600 * 8),
    quota_monthly_reset_at: nowSeconds(86400 * 13),
  }
}

async function createDemoToken(init?: RequestInit): Promise<Response> {
  const body = await readJsonBody(init)
  const note = typeof body.note === 'string' && body.note.trim().length > 0 ? body.note.trim() : null
  const id = `dm${String(demoState.tokens.length + 1).padStart(2, '0')}`
  const secret = `th-${id}-demoaccesssecret`
  demoState.tokens.unshift(createDemoAuthToken(id, note))
  demoState.tokenSecrets.set(id, secret)
  return jsonResponse({ token: secret })
}

function createDemoUserTag(
  id: string,
  payload: {
    name: string
    displayName: string
    icon: string | null
    effectKind: string
    hourlyAnyDelta: number
    hourlyDelta: number
    dailyDelta: number
    monthlyDelta: number
  },
  userCount = 0,
) {
  return {
    id,
    name: payload.name,
    displayName: payload.displayName,
    icon: payload.icon,
    systemKey: payload.name === 'demo' ? 'demo' : null,
    effectKind: payload.effectKind,
    hourlyAnyDelta: payload.hourlyAnyDelta,
    hourlyDelta: payload.hourlyDelta,
    dailyDelta: payload.dailyDelta,
    monthlyDelta: payload.monthlyDelta,
    userCount,
  }
}

function createDemoKey(
  id: string,
  status: string,
  group: string,
  quotaLimit: number,
  quotaRemaining: number,
  total: number,
  success: number,
  error: number,
  transient_backoff: JsonValue,
  quarantine: JsonValue = null,
) {
  return {
    id,
    status,
    group,
    registration_ip: id.includes('sfo') ? '203.0.113.45' : '198.51.100.24',
    registration_region: id.includes('fra') ? 'DE' : id.includes('sfo') ? 'US' : 'HK',
    status_changed_at: nowSeconds(-3600 * 6),
    last_used_at: status === 'exhausted' ? nowSeconds(-86400) : nowSeconds(-180),
    deleted_at: null,
    quota_limit: quotaLimit,
    quota_remaining: quotaRemaining,
    quota_synced_at: nowSeconds(-900),
    total_requests: total,
    success_count: success,
    error_count: error,
    quota_exhausted_count: status === 'exhausted' ? 22 : 0,
    quarantine,
    transient_backoff,
  }
}

function createDemoUsers() {
  return [
    createDemoUser('user-demo-admin', 'Hikari Demo Admin', 'hikari-demo', 2, 3, 42, 180, 388, 1600, 8400, 24000),
    createDemoUser('user-research', 'Research Team', 'research-team', 1, 1, 28, 100, 312, 900, 4200, 10000),
    createDemoUser('user-ops', 'Ops Runner', 'ops-runner', 1, 1, 12, 80, 760, 800, 7800, 12000),
  ]
}

function createDemoUser(
  userId: string,
  displayName: string,
  username: string,
  tokenCount: number,
  apiKeyCount: number,
  hourlyUsed: number,
  hourlyLimit: number,
  dailyUsed: number,
  dailyLimit: number,
  monthlyUsed: number,
  monthlyLimit: number,
) {
  return {
    userId,
    displayName,
    username,
    active: true,
    lastLoginAt: nowSeconds(-1800),
    tokenCount,
    apiKeyCount,
    tags: [{
      tagId: 'tag-demo',
      name: 'demo',
      displayName: 'Demo',
      icon: 'sparkles',
      systemKey: 'demo',
      effectKind: 'quota_delta',
      hourlyAnyDelta: 20,
      hourlyDelta: 20,
      dailyDelta: 200,
      monthlyDelta: 2000,
      source: 'system',
    }],
    requestRate: { used: hourlyUsed, limit: hourlyLimit, windowMinutes: 60, scope: 'user' },
    hourlyAnyUsed: hourlyUsed,
    hourlyAnyLimit: hourlyLimit,
    quotaHourlyUsed: hourlyUsed,
    quotaHourlyLimit: hourlyLimit,
    quotaDailyUsed: dailyUsed,
    quotaDailyLimit: dailyLimit,
    quotaMonthlyUsed: monthlyUsed,
    quotaMonthlyLimit: monthlyLimit,
    dailySuccess: Math.max(1, dailyUsed - 18),
    dailyFailure: 18,
    monthlySuccess: Math.max(1, monthlyUsed - 140),
    monthlyFailure: 140,
    monthlyBrokenCount: userId === 'user-ops' ? 3 : 1,
    monthlyBrokenLimit: 5,
    recentIpCount24h: 3,
    recentIpCount7d: 7,
    lastActivity: nowSeconds(-240),
  }
}

function createDemoJobs() {
  return range(12).map((index) => ({
    id: 3000 + index,
    jobType: index % 3 === 0 ? 'quota_sync' : index % 3 === 1 ? 'usage_rollup' : 'geo_lookup',
    keyId: index % 2 === 0 ? DEMO_KEY_ID : DEMO_BACKUP_KEY_ID,
    keyGroup: index % 2 === 0 ? 'primary' : 'backup',
    status: index % 5 === 0 ? 'failed' : 'success',
    attempt: 1 + (index % 2),
    message: index % 5 === 0 ? 'Demo retry scheduled' : 'Demo job completed',
    startedAt: nowSeconds(-index * 1800),
    finishedAt: nowSeconds(-index * 1800 + 12),
  }))
}

function createDemoForwardProxy() {
  const nodes = ['HK edge', 'Tokyo relay', 'SFO relay'].map((displayName, index) => ({
    key: `demo-proxy-${index + 1}`,
    source: 'demo',
    displayName,
    endpointUrl: `socks5://demo-${index + 1}.internal:1080`,
    resolvedIps: [`198.51.100.${30 + index}`],
    resolvedRegions: [index === 1 ? 'JP' : index === 2 ? 'US' : 'HK'],
    weight: 100 - index * 12,
    available: index !== 2,
    disabled: index === 2,
    disabledAt: index === 2 ? nowSeconds(-4200) : null,
    lastError: index === 2 ? 'Demo node disabled for presentation' : null,
    penalized: index === 2,
    primaryAssignmentCount: 40 - index * 5,
    secondaryAssignmentCount: 18 + index * 2,
    stats: {
      oneMinute: { attempts: 8, successCount: 8, failureCount: 0, successRate: 1, avgLatencyMs: 86 + index * 12 },
      fifteenMinutes: { attempts: 124, successCount: 119, failureCount: 5, successRate: 0.96, avgLatencyMs: 96 + index * 14 },
      oneHour: { attempts: 420, successCount: 402, failureCount: 18, successRate: 0.957, avgLatencyMs: 104 + index * 18 },
      oneDay: { attempts: 4420, successCount: 4260, failureCount: 160, successRate: 0.964, avgLatencyMs: 110 + index * 20 },
      sevenDays: { attempts: 26800, successCount: 26020, failureCount: 780, successRate: 0.971, avgLatencyMs: 112 + index * 21 },
    },
  }))
  return {
    proxyUrls: ['socks5://hk-demo.internal:1080', 'socks5://tokyo-demo.internal:1080'],
    subscriptionUrls: ['https://demo.example.test/proxy/subscription'],
    subscriptionUpdateIntervalSecs: 3600,
    insertDirect: true,
    egressSocks5Enabled: true,
    egressSocks5Url: 'socks5://hk-demo.internal:1080',
    nodes,
  }
}

function createDemoSystemSettings() {
  return {
    requestRateLimit: 180,
    mcpSessionAffinityKeyCount: 4,
    rebalanceMcpEnabled: true,
    rebalanceMcpSessionPercent: 35,
    apiRebalanceEnabled: true,
    apiRebalancePercent: 25,
    rechargeFeatureEnabled: true,
    rechargeUserEnabled: true,
    userBlockedKeyBaseLimit: 5,
    globalIpLimit: 8,
    trustedProxyCidrs: ['127.0.0.0/8', '10.0.0.0/8'],
    trustedClientIpHeaders: ['cf-connecting-ip', 'x-real-ip', 'x-forwarded-for'],
  }
}

const demoState = createDemoState()

function demoSummary(now = Date.now()) {
  const pulse = demoPulse(now)
  const total = demoState.logs.length
  const success = demoState.logs.filter((log) => log.result_status === 'success').length
  const quota = demoState.logs.filter((log) => log.result_status === 'quota_exhausted').length
  const requestDrift = pulse % 6
  const quotaDrift = pulse % 4
  return {
    total_requests: 24882 + requestDrift * 48,
    success_count: 23710 + requestDrift * 42,
    error_count: 992 + quotaDrift * 8,
    quota_exhausted_count: quota + quotaDrift,
    active_keys: demoState.keys.filter((key) => key.status === 'active').length,
    exhausted_keys: demoState.keys.filter((key) => key.status === 'exhausted').length,
    quarantined_keys: demoState.keys.filter((key) => key.status === 'quarantined').length,
    temporary_isolated_keys: demoState.keys.filter((key) => key.status === 'temporarily_isolated').length,
    last_activity: (demoState.logs[0]?.created_at ?? nowSeconds(-120)) + requestDrift * 45,
    total_quota_limit: 104000,
    total_quota_remaining: 66360 - requestDrift * 64,
    _demo_window_total: total,
    _demo_window_success: success,
  }
}

function demoSummaryWindows(
  currentHourStart = Math.floor(Date.now() / 3_600_000) * 3_600,
  now = Date.now(),
) {
  const pulse = demoPulse(now)
  const todayDrift = pulse % 5
  const monthDrift = pulse % 12
  const todayStart = currentHourStart - 23 * 3_600
  const quotaCharge = {
    local_estimated_credits: 1260 + todayDrift * 6,
    upstream_actual_credits: 1218 + todayDrift * 5,
    sampled_key_count: 4,
    stale_key_count: 1,
    latest_sync_at: nowSeconds(-900 + todayDrift * 30),
  }
  return {
    today: {
      total_requests: 760 + todayDrift * 4,
      success_count: 714 + todayDrift * 3,
      error_count: 36 + (todayDrift % 3),
      quota_exhausted_count: 10 + (todayDrift % 2),
      valuable_success_count: 482 + todayDrift * 2,
      valuable_failure_count: 18 + (todayDrift % 3),
      other_success_count: 232 + todayDrift,
      other_failure_count: 18 + (todayDrift % 2),
      unknown_count: todayDrift % 2,
      upstream_exhausted_key_count: 1 + (todayDrift % 3 === 0 ? 1 : 0),
      new_keys: 1 + (todayDrift % 4 === 0 ? 1 : 0),
      new_quarantines: 1 + (todayDrift % 5 === 0 ? 1 : 0),
      quota_charge: quotaCharge,
    },
    yesterday: {
      total_requests: 690,
      success_count: 654,
      error_count: 28,
      quota_exhausted_count: 8,
      valuable_success_count: 430,
      valuable_failure_count: 12,
      other_success_count: 224,
      other_failure_count: 16,
      unknown_count: 0,
      upstream_exhausted_key_count: 1,
      new_keys: 0,
      new_quarantines: 0,
      quota_charge: quotaCharge,
    },
    month: {
      total_requests: 24882 + monthDrift * 60,
      success_count: 23710 + monthDrift * 55,
      error_count: 992 + monthDrift * 4,
      quota_exhausted_count: 180 + monthDrift * 2,
      valuable_success_count: 17880 + monthDrift * 38,
      valuable_failure_count: 420 + (monthDrift % 4),
      other_success_count: 5830 + monthDrift * 14,
      other_failure_count: 572 + (monthDrift % 5),
      unknown_count: monthDrift % 3,
      upstream_exhausted_key_count: 1 + (monthDrift % 6 === 0 ? 1 : 0),
      new_keys: 4 + (monthDrift % 4 === 0 ? 1 : 0),
      new_quarantines: 2 + (monthDrift % 5 === 0 ? 1 : 0),
      quota_charge: quotaCharge,
    },
    today_start: todayStart,
    today_end: currentHourStart + 1,
    yesterday_start: todayStart - 24 * 3_600,
    yesterday_end: todayStart,
    month_start: todayStart - 14 * 24 * 3_600,
    month_end: currentHourStart + 1,
    previous_month_start: todayStart - 45 * 24 * 3_600,
    previous_month_end: todayStart - 14 * 24 * 3_600,
  }
}

function demoDashboardOverview(now = Date.now()) {
  const pulse = demoPulse(now)
  const currentHourStart = Math.floor(Date.now() / 3_600_000) * 3_600
  return {
    summary: demoSummary(now),
    summaryWindows: demoSummaryWindows(currentHourStart, now),
    hourlyRequestWindow: {
      bucketSeconds: 3600,
      visibleBuckets: 25,
      retainedBuckets: 49,
      buckets: range(49).map((index) => ({
        bucketStart: currentHourStart - (48 - index) * 3_600,
        secondarySuccess: 8 + index + (index >= 42 ? pulse % 4 : 0),
        primarySuccess: 24 + index * 2 + (index >= 40 ? (pulse % 5) * 2 : 0),
        secondaryFailure: (index % 5) + (index >= 44 ? pulse % 2 : 0),
        primaryFailure429: index % 7 === 0 ? 2 + (index >= 45 ? pulse % 2 : 0) : 0,
        primaryFailureOther: (index % 4) + (index >= 41 ? pulse % 3 : 0),
        unknown: index === 48 && pulse % 4 === 0 ? 1 : 0,
        mcpNonBillable: 4 + (index % 6) + (index >= 43 ? pulse % 2 : 0),
        mcpBillable: 10 + (index % 8) + (index >= 44 ? pulse % 3 : 0),
        apiNonBillable: 3 + (index % 5) + (index >= 42 ? pulse % 2 : 0),
        apiBillable: 18 + index + (index >= 45 ? pulse % 4 : 0),
      })),
    },
    siteStatus: {
      remainingQuota: 66360 - (pulse % 7) * 24,
      totalQuotaLimit: 104000,
      activeKeys: 2,
      quarantinedKeys: 1,
      temporaryIsolatedKeys: 1,
      exhaustedKeys: 1,
      availableProxyNodes: 2,
      totalProxyNodes: 3,
    },
    forwardProxy: { availableNodes: 2, totalNodes: 3 },
    trend: {
      request: range(24).map((index) => 28 + index * 2 + (index >= 20 ? pulse % 4 : 0)),
      error: range(24).map((index) => (index % 5) + (index >= 20 ? pulse % 2 : 0)),
    },
    exhaustedKeys: demoState.keys.filter((key) => key.status !== 'active'),
    recentLogs: demoState.logs.slice(0, 8),
    recentJobs: demoState.jobs.slice(0, 6).map(serverJobToView),
    disabledTokens: demoState.tokens.filter((token) => !token.enabled),
    tokenCoverage: 'ok',
    recentAlerts: demoRecentAlerts(),
  }
}

function demoRecentAlerts() {
  const latestEvent = {
    id: 'evt-demo-quota',
    type: 'upstream_usage_limit_432',
    title: 'Demo quota threshold reached',
    summary: 'A mock upstream key reached its monthly limit.',
    occurredAt: nowSeconds(-1200),
    subjectKind: 'key',
    subjectId: DEMO_QUOTA_KEY_ID,
    subjectLabel: DEMO_QUOTA_KEY_ID,
    user: null,
    token: { id: DEMO_TOKEN_ID, label: 'Demo operator token' },
    key: { id: DEMO_QUOTA_KEY_ID, label: DEMO_QUOTA_KEY_ID },
    request: { id: 9000, method: 'POST', path: '/mcp', query: null },
    requestKind: { key: 'mcp:search', label: 'MCP | search', detail: 'Billable request' },
    failureKind: 'upstream_usage_limit_432',
    resultStatus: 'quota_exhausted',
    errorMessage: 'Demo upstream monthly quota exhausted',
    reasonCode: 'upstream_usage_limit_432',
    reasonSummary: 'Monthly quota exhausted',
    reasonDetail: 'Mock alert generated by demo mode.',
    source: { kind: 'request_log', id: '9000' },
  }
  return {
    windowHours: 24,
    totalEvents: 6,
    groupedCount: 2,
    countsByType: [
      { type: 'upstream_usage_limit_432', count: 4 },
      { type: 'upstream_key_blocked', count: 2 },
    ],
    topGroups: [{
      id: 'grp-demo-quota',
      type: 'upstream_usage_limit_432',
      subjectKind: 'key',
      subjectId: DEMO_QUOTA_KEY_ID,
      subjectLabel: DEMO_QUOTA_KEY_ID,
      user: null,
      token: { id: DEMO_TOKEN_ID, label: 'Demo operator token' },
      key: { id: DEMO_QUOTA_KEY_ID, label: DEMO_QUOTA_KEY_ID },
      requestKind: { key: 'mcp:search', label: 'MCP | search', detail: 'Billable request' },
      count: 4,
      firstSeen: nowSeconds(-3600 * 5),
      lastSeen: nowSeconds(-1200),
      latestEvent,
    }],
  }
}

function buildAlertsPage<T>(items: T[], url: URL, defaultPerPage = 20) {
  const page = Math.max(1, Number(url.searchParams.get('page') ?? '1') || 1)
  const perPage = Math.max(1, Number(url.searchParams.get('per_page') ?? defaultPerPage) || defaultPerPage)
  const start = (page - 1) * perPage
  return {
    items: items.slice(start, start + perPage),
    total: items.length,
    page,
    perPage,
  }
}

function timestampMatchesAlertWindow(timestamp: number, url: URL): boolean {
  const since = url.searchParams.get('since')
  const until = url.searchParams.get('until')
  if (since) {
    const sinceSeconds = Date.parse(since) / 1000
    if (Number.isFinite(sinceSeconds) && timestamp < sinceSeconds) return false
  }
  if (until) {
    const untilSeconds = Date.parse(until) / 1000
    if (Number.isFinite(untilSeconds) && timestamp > untilSeconds) return false
  }
  return true
}

function requestKindMatchesAlertFilter(requestKindKey: string | undefined, url: URL): boolean {
  const filters = url.searchParams.getAll('request_kind').filter(Boolean)
  if (filters.length === 0) return true
  return Boolean(requestKindKey && filters.includes(requestKindKey))
}

function alertEventMatchesFilters(event: ReturnType<typeof demoRecentAlerts>['topGroups'][number]['latestEvent'], url: URL): boolean {
  const type = url.searchParams.get('type')
  const userId = url.searchParams.get('user_id')
  const tokenId = url.searchParams.get('token_id')
  const keyId = url.searchParams.get('key_id')
  const eventUser = event.user as { userId?: string } | null
  if (type && event.type !== type) return false
  if (userId && eventUser?.userId !== userId) return false
  if (tokenId && event.token?.id !== tokenId) return false
  if (keyId && event.key?.id !== keyId) return false
  if (!requestKindMatchesAlertFilter(event.requestKind?.key, url)) return false
  return timestampMatchesAlertWindow(event.occurredAt, url)
}

function alertGroupMatchesFilters(group: ReturnType<typeof demoRecentAlerts>['topGroups'][number], url: URL): boolean {
  const type = url.searchParams.get('type')
  const userId = url.searchParams.get('user_id')
  const tokenId = url.searchParams.get('token_id')
  const keyId = url.searchParams.get('key_id')
  const groupUser = group.user as { userId?: string } | null
  if (type && group.type !== type) return false
  if (userId && groupUser?.userId !== userId) return false
  if (tokenId && group.token?.id !== tokenId) return false
  if (keyId && group.key?.id !== keyId) return false
  if (!requestKindMatchesAlertFilter(group.requestKind?.key, url)) return false
  return timestampMatchesAlertWindow(group.lastSeen, url)
}

function demoAlertCatalogPayload() {
  const alerts = demoRecentAlerts()
  return {
    retentionDays: 30,
    types: alerts.countsByType.map((item) => ({ value: item.type, count: item.count })),
    requestKindOptions,
    users: demoState.users.map((user) => ({
      id: user.userId,
      label: user.displayName,
      username: user.username,
    })),
    tokens: demoState.tokens.map((token) => ({
      id: token.id,
      label: token.note ?? token.id,
    })),
    keys: demoState.keys.map((key) => ({
      id: key.id,
      label: key.id,
    })),
  }
}

function serverJobToView(job: ReturnType<typeof createDemoJobs>[number]) {
  return {
    id: job.id,
    job_type: job.jobType,
    key_id: job.keyId,
    key_group: job.keyGroup,
    status: job.status,
    attempt: job.attempt,
    message: job.message,
    started_at: job.startedAt,
    finished_at: job.finishedAt,
  }
}

function publicTokenLogs(items = demoState.logs) {
  return items.slice(0, 18).map((log) => ({
    id: log.id,
    method: log.method,
    path: log.path,
    query: log.query,
    httpStatus: log.http_status,
    mcpStatus: log.mcp_status,
    resultStatus: log.result_status,
    errorMessage: log.error_message,
    createdAt: log.created_at,
  }))
}

function demoLogDetailForPath(path: string, fallbackItems = demoState.logs) {
  const match = path.match(/\/logs\/(\d+)\/details$/)
  const logId = match ? Number(match[1]) : NaN
  const log = fallbackItems.find((item) => item.id === logId) ?? fallbackItems[0] ?? demoState.logs[0]
  return { request_body: log?.request_body ?? null, response_body: log?.response_body ?? null }
}

function tokenSummary(tokenId = DEMO_TOKEN_ID) {
  const logs = demoState.logs.filter((log) => log.auth_token_id === tokenId)
  return {
    total_requests: logs.length * 32,
    success_count: logs.filter((log) => log.result_status === 'success').length * 32,
    error_count: logs.filter((log) => log.result_status === 'error').length * 32,
    quota_exhausted_count: logs.filter((log) => log.result_status === 'quota_exhausted').length * 32,
    last_activity: logs[0]?.created_at ?? nowSeconds(-120),
  }
}

function buildListPage<T>(items: T[], url: URL, defaultPerPage = 20) {
  const page = Math.max(1, Number(url.searchParams.get('page') ?? '1') || 1)
  const perPage = Math.max(1, Number(url.searchParams.get('per_page') ?? url.searchParams.get('limit') ?? defaultPerPage) || defaultPerPage)
  const start = (page - 1) * perPage
  return {
    items: items.slice(start, start + perPage),
    total: items.length,
    page,
    perPage,
  }
}

function buildCursorListPage<T>(items: T[], url: URL, defaultPageSize = 20) {
  const pageSize = Math.max(1, Number(url.searchParams.get('limit') ?? defaultPageSize) || defaultPageSize)
  return {
    items: items.slice(0, pageSize),
    pageSize,
    nextCursor: items.length > pageSize ? String((items[pageSize] as { id?: unknown } | undefined)?.id ?? pageSize) : null,
    prevCursor: null,
    hasOlder: items.length > pageSize,
    hasNewer: false,
  }
}

function requestLogFacets() {
  return {
    results: [
      { value: 'success', count: 52 },
      { value: 'error', count: 8 },
      { value: 'quota_exhausted', count: 4 },
    ],
    keyEffects: [
      { value: 'used', count: 52 },
      { value: 'retry_next_key', count: 8 },
      { value: 'quarantine', count: 4 },
    ],
    bindingEffects: [
      { value: 'sticky_match', count: 11 },
      { value: 'none', count: 53 },
    ],
    selectionEffects: [
      { value: 'selected', count: 52 },
      { value: 'fallback', count: 12 },
    ],
    tokens: demoState.tokens.map((token) => ({ value: token.id, count: token.total_requests })),
    keys: demoState.keys.map((key) => ({ value: key.id, count: key.total_requests })),
  }
}

function requestLogsPage(url: URL, items = demoState.logs) {
  return {
    ...buildListPage(items, url),
    requestKindOptions,
    facets: requestLogFacets(),
  }
}

function requestLogsCatalog() {
  return {
    retentionDays: 30,
    requestKindOptions,
    facets: requestLogFacets(),
  }
}

function tokenUsageSeries(url: URL) {
  const bucketSecs = Math.max(3600, Number(url.searchParams.get('bucket_secs') ?? 3600) || 3600)
  const count = bucketSecs >= 86400 ? 31 : 25
  const currentBucket = Math.floor(nowSeconds() / bucketSecs) * bucketSecs
  return range(count).map((index) => ({
    bucket_start: currentBucket - (count - index - 1) * bucketSecs,
    success_count: 18 + (index % 9) * 2,
    system_failure_count: index % 5,
    external_failure_count: index % 7 === 0 ? 2 : 0,
  }))
}

function forwardProxyStats() {
  const bucketSeconds = 3600
  const rangeEnd = isoSeconds(0)
  const rangeStart = isoSeconds(-24 * 3600)
  return {
    rangeStart,
    rangeEnd,
    bucketSeconds,
    nodes: demoState.forwardProxy.nodes.map((node, nodeIndex) => ({
      ...node,
      last24h: range(24).map((index) => ({
        bucketStart: isoSeconds(-(24 - index) * bucketSeconds),
        bucketEnd: isoSeconds(-(23 - index) * bucketSeconds),
        successCount: 20 + index + nodeIndex,
        failureCount: index % (4 + nodeIndex),
      })),
      weight24h: range(24).map((index) => ({
        bucketStart: isoSeconds(-(24 - index) * bucketSeconds),
        bucketEnd: isoSeconds(-(23 - index) * bucketSeconds),
        sampleCount: 8,
        minWeight: node.weight - 8,
        maxWeight: node.weight + 4,
        avgWeight: node.weight - 2 + (index % 3),
        lastWeight: node.weight,
      })),
    })),
  }
}

function forwardProxyErrorStats() {
  const stats = forwardProxyStats()
  return {
    rangeStart: stats.rangeStart,
    rangeEnd: stats.rangeEnd,
    bucketSeconds: stats.bucketSeconds,
    nodes: stats.nodes.map((node, index) => ({
      key: node.key,
      source: node.source,
      displayName: node.displayName,
      endpointUrl: node.endpointUrl,
      resolvedIps: node.resolvedIps,
      resolvedRegions: node.resolvedRegions,
      available: node.available,
      disabled: node.disabled,
      disabledAt: node.disabledAt,
      windows: {
        oneMinute: { totalCount: 8, errorCount: index === 2 ? 2 : 0, errorRate: index === 2 ? 0.25 : 0 },
        fifteenMinutes: { totalCount: 124, errorCount: 5 + index, errorRate: 0.04 + index * 0.01 },
        oneHour: { totalCount: 420, errorCount: 18 + index, errorRate: 0.04 + index * 0.01 },
        oneDay: { totalCount: 4420, errorCount: 160 + index * 12, errorRate: 0.036 + index * 0.01 },
        sevenDays: { totalCount: 26800, errorCount: 780 + index * 40, errorRate: 0.029 + index * 0.01 },
      },
      last24h: range(24).map((bucket) => ({
        bucketStart: isoSeconds(-(24 - bucket) * 3600),
        bucketEnd: isoSeconds(-(23 - bucket) * 3600),
        totalCount: 32 + bucket,
        successCount: 30 + bucket,
        errorCount: bucket % 5,
        errors: [{ kind: 'upstream_timeout', count: bucket % 5 }],
      })),
      distribution24h: [
        { kind: 'upstream_timeout', count: 12 + index },
        { kind: 'proxy_disabled', count: index === 2 ? 7 : 1 },
      ],
      total24h: 4420,
      error24h: 160 + index * 12,
      errorRate24h: 0.036 + index * 0.01,
    })),
  }
}

function jsonResponse(payload: unknown, status = 200): Response {
  return new Response(JSON.stringify(payload), {
    status,
    headers: {
      'Content-Type': 'application/json; charset=utf-8',
      'Cache-Control': 'no-store',
      'X-Tavily-Hikari-Demo': 'true',
    },
  })
}

function textResponse(payload: string, status = 200): Response {
  return new Response(payload, {
    status,
    headers: { 'Content-Type': 'text/plain; charset=utf-8', 'X-Tavily-Hikari-Demo': 'true' },
  })
}

function noContentResponse(): Response {
  return new Response(null, { status: 204, headers: { 'X-Tavily-Hikari-Demo': 'true' } })
}

function parseUrl(input: RequestInfo | URL): URL | null {
  const raw = input instanceof Request ? input.url : String(input)
  try {
    return new URL(raw, window.location.origin)
  } catch {
    return null
  }
}

async function readJsonBody(init?: RequestInit): Promise<Record<string, unknown>> {
  const body = init?.body
  if (typeof body === 'string') {
    try {
      const parsed = JSON.parse(body)
      return typeof parsed === 'object' && parsed ? parsed as Record<string, unknown> : {}
    } catch {
      return {}
    }
  }
  return {}
}

async function demoFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response | null> {
  const url = parseUrl(input)
  if (!url) return null
  const path = url.pathname
  if (!(path === '/health' || path === '/mcp' || path.startsWith('/api/'))) return null
  if (init?.signal?.aborted) {
    throw new DOMException('The operation was aborted.', 'AbortError')
  }
  await new Promise((resolve) => window.setTimeout(resolve, 60))
  return handleDemoRoute(url, (init?.method ?? (input instanceof Request ? input.method : 'GET')).toUpperCase(), init)
}

async function handleDemoRoute(url: URL, method: string, init?: RequestInit): Promise<Response> {
  const path = url.pathname
  if (path === '/health') return jsonResponse({ ok: true, mode: 'demo' })
  if (path === '/mcp') return handleMcpDemo(init)

  if (path === '/api/version') return jsonResponse(demoState.version)
  if (path === '/api/profile') return jsonResponse(demoState.profile)
  if (path === '/api/summary') return jsonResponse(demoSummary())
  if (path === '/api/summary/windows') return jsonResponse(demoSummaryWindows())
  if (path === '/api/dashboard/overview') return jsonResponse(demoDashboardOverview())
  if (path === '/api/public/metrics') return jsonResponse({ monthlySuccess: 23710, dailySuccess: 714 })
  if (path === '/api/token/metrics') return jsonResponse({
    monthlySuccess: 8400,
    dailySuccess: 388,
    dailyFailure: 18,
    quotaHourlyUsed: 42,
    quotaHourlyLimit: 180,
    quotaDailyUsed: 388,
    quotaDailyLimit: 1600,
    quotaMonthlyUsed: 8400,
    quotaMonthlyLimit: 24000,
  })
  if (path === '/api/public/logs') return jsonResponse(publicTokenLogs())
  if (path === '/api/public/events') return textResponse('demo event stream is provided by the browser demo runtime\n')
  if (path === '/api/admin/login' && method === 'POST') return noContentResponse()
  if (path === '/api/admin/registration') {
    if (method === 'PATCH') {
      const body = await readJsonBody(init)
      demoState.registration.allowRegistration = Boolean(body.allowRegistration)
    }
    return jsonResponse(demoState.registration)
  }
  if (path === '/api/user/logout') return noContentResponse()
  if (path === '/api/user/token') return jsonResponse({ token: DEMO_TOKEN })
  if (path === '/api/user/dashboard') return jsonResponse({
    requestRate: { used: 42, limit: 180, windowMinutes: 5, scope: 'user' },
    hourlyAnyUsed: 42,
    hourlyAnyLimit: 180,
    quotaHourlyUsed: 42,
    quotaHourlyLimit: 180,
    quotaDailyUsed: 388,
    quotaDailyLimit: 1600,
    quotaMonthlyUsed: 8400,
    quotaMonthlyLimit: 24000,
    dailySuccess: 388,
    dailyFailure: 18,
    monthlySuccess: 8400,
    lastActivity: nowSeconds(-120),
    recharge: demoRechargeSummary(),
  })
  if (path === '/api/user/recharge/config') return jsonResponse(demoRechargeConfig())
  if (path === '/api/user/recharge/orders') {
    if (method === 'POST') return handleCreateDemoRechargeOrder(init)
    return jsonResponse({ items: demoState.rechargeOrders })
  }
  if (path === '/api/admin/recharges') {
    return handleDemoAdminRecharges(demoState.rechargeOrders, url, jsonResponse)
  }
  const adminRechargeAction = handleDemoAdminRechargeAction(demoState.rechargeOrders, path, method, jsonResponse, nowSeconds)
  if (adminRechargeAction) return adminRechargeAction
  if (path === '/api/admin/totp') {
    return jsonResponse({ enabled: false, available: true, rechargeFeatureEnabled: true, missingCryptoKey: false, lockedUntil: null, issuer: 'Tavily Hikari', accountName: 'admin-recharge' })
  }
  if (path === '/api/admin/totp/setup' && method === 'POST') {
    return jsonResponse({ secret: 'JBSWY3DPEHPK3PXP', otpAuthUrl: 'otpauth://totp/Tavily%20Hikari:admin-recharge?secret=JBSWY3DPEHPK3PXP&issuer=Tavily%20Hikari', qrPngBase64: '' })
  }
  const rechargeOrderRoute = path.match(/^\/api\/user\/recharge\/orders\/([^/]+)$/)
  if (rechargeOrderRoute) {
    const outTradeNo = decodeURIComponent(rechargeOrderRoute[1])
    const order = demoState.rechargeOrders.find((item) => item.outTradeNo === outTradeNo)
    return order ? jsonResponse(order) : jsonResponse({ message: 'Demo recharge order not found' }, 404)
  }
  if (path === '/api/user/tokens') return jsonResponse(demoState.tokens)
  if (path.startsWith('/api/user/tokens/')) return handleUserTokenRoute(path, url)
  if (path === '/api/user/announcements') return jsonResponse({ items: demoUserActiveAnnouncements(demoState.announcements) })
  if (path === '/api/user/announcements/history') return jsonResponse({ items: demoUserAnnouncementHistory(demoState.announcements) })

  if (path === '/api/keys/validate' && method === 'POST') return jsonResponse({
    summary: { input_lines: 2, valid_lines: 2, unique_in_input: 2, duplicate_in_input: 0, already_exists: 1, ok: 1, exhausted: 1, invalid: 0, error: 0 },
    results: [
      { api_key: 'tvly-dev-demo-one', status: 'ok', registration_ip: '198.51.100.24', registration_region: 'HK', assigned_proxy_key: 'demo-proxy-1', assigned_proxy_label: 'HK edge', assigned_proxy_match_kind: 'registration_ip', quota_limit: 1000, quota_remaining: 820 },
      { api_key: 'tvly-dev-demo-two', status: 'exhausted', detail: 'Demo exhausted key', quota_limit: 1000, quota_remaining: 0 },
    ],
  })
  if (path === '/api/keys/bulk-actions') return jsonResponse({
    summary: { requested: 2, succeeded: 2, skipped: 0, failed: 0 },
    results: [{ key_id: DEMO_KEY_ID, status: 'success', detail: 'Demo action applied' }],
  })
  if (path === '/api/keys/batch') return jsonResponse({
    summary: { input_lines: 2, valid_lines: 2, unique_in_input: 2, created: 1, undeleted: 0, existed: 1, duplicate_in_input: 0, failed: 0 },
    results: [{ api_key: 'tvly-dev-demo-one', status: 'created', id: 'key-new-demo' }],
  })
  if (path === '/api/keys' && method === 'GET') return jsonResponse({ ...buildListPage(demoState.keys, url), facets: {
    groups: [{ value: 'primary', count: 2 }, { value: 'backup', count: 1 }, { value: 'eu', count: 1 }],
    statuses: [{ value: 'active', count: 2 }, { value: 'quarantined', count: 1 }, { value: 'exhausted', count: 1 }],
    regions: [{ value: 'HK', count: 2 }, { value: 'US', count: 1 }, { value: 'DE', count: 1 }],
  } })
  if (path === '/api/keys' && method === 'POST') return jsonResponse({ id: 'key-new-demo' })
  if (path.startsWith('/api/keys/')) return handleKeyRoute(path, url, method)

  if (path === '/api/logs/list') return jsonResponse(buildCursorListPage(demoState.logs, url))
  if (path === '/api/logs/catalog') return jsonResponse(requestLogsCatalog())
  if (path === '/api/logs') return jsonResponse(requestLogsPage(url))
  if (/^\/api\/logs\/\d+\/details$/.test(path)) return jsonResponse(demoLogDetailForPath(path))
  if (path === '/api/jobs') return jsonResponse({ ...buildListPage(demoState.jobs, url, 10), groupCounts: { all: demoState.jobs.length, quota: 4, usage: 4, logs: 2, geo: 2, linuxdo: 0 } })

  if (path === '/api/announcements') return handleAnnouncementsRoute({ announcements: demoState.announcements, path, method, init, nowSeconds, readJsonBody, jsonResponse })
  if (path.startsWith('/api/announcements/')) return handleAnnouncementsRoute({ announcements: demoState.announcements, path, method, init, nowSeconds, readJsonBody, jsonResponse })

  if (path === '/api/users') return jsonResponse(buildListPage(demoState.users, url))
  if (path.startsWith('/api/users/')) return handleUserRoute(path, url, method, init)
  if (path === '/api/user-tags') return handleUserTags(path, method, init)
  if (path.startsWith('/api/user-tags/')) return handleUserTags(path, method, init)

  if (path === '/api/tokens/groups') return jsonResponse([{ name: 'demo', tokenCount: 1, latestCreatedAt: nowSeconds(-86400) }, { name: 'internal', tokenCount: 1, latestCreatedAt: nowSeconds(-86400 * 2) }])
  if (path === '/api/tokens/unbound-usage') return jsonResponse({ items: [], total: 0, page: 1, perPage: 20 })
  if (path === '/api/tokens/batch' && method === 'POST') return jsonResponse({ tokens: ['th-b001-demoaccesssecret', 'th-b002-demoaccesssecret'] })
  if (path === '/api/tokens' && method === 'GET') return jsonResponse(buildListPage(demoState.tokens, url, 10))
  if (path === '/api/tokens' && method === 'POST') return createDemoToken(init)
  if (path.startsWith('/api/tokens/')) return handleTokenRoute(path, url, method)

  if (path === '/api/settings') return jsonResponse({ forwardProxy: demoState.forwardProxy, systemSettings: demoState.systemSettings })
  if (path === '/api/settings/forward-proxy') return jsonResponse(demoState.forwardProxy)
  if (path === '/api/settings/system') return jsonResponse(demoState.systemSettings)
  if (path === '/api/settings/forward-proxy/validate') return jsonResponse({ ok: true, message: 'Demo proxy candidate is reachable', normalizedValue: 'socks5://demo.internal:1080', discoveredNodes: 3, latencyMs: 94, nodes: [{ displayName: 'HK edge', protocol: 'socks5', ok: true, latencyMs: 94, ip: '198.51.100.30', location: 'HK' }] })
  if (path === '/api/settings/forward-proxy/revalidate') return jsonResponse(demoState.forwardProxy)
  if (path === '/api/settings/forward-proxy/nodes/state') return jsonResponse({ results: demoState.forwardProxy.nodes.map((node) => ({ proxyKey: node.key, disabled: Boolean(node.disabled), disabledAt: node.disabledAt ?? null })) })
  if (path === '/api/stats/forward-proxy') return jsonResponse(forwardProxyStats())
  if (path === '/api/stats/forward-proxy/errors') return jsonResponse(forwardProxyErrorStats())
  if (path === '/api/stats/forward-proxy/summary') return jsonResponse({ availableNodes: 2, totalNodes: 3 })

  if (path === '/api/alerts/catalog') return jsonResponse(demoAlertCatalogPayload())
  if (path === '/api/alerts/events') {
    const events = demoRecentAlerts().topGroups.map((group) => group.latestEvent).filter((event) => alertEventMatchesFilters(event, url))
    return jsonResponse(buildAlertsPage(events, url))
  }
  if (path === '/api/alerts/groups') {
    const groups = demoRecentAlerts().topGroups.filter((group) => alertGroupMatchesFilters(group, url))
    return jsonResponse(buildAlertsPage(groups, url))
  }

  if (path.startsWith('/api/tavily/')) return jsonResponse(handleTavilyProbe(path))
  return method === 'GET' ? jsonResponse({ demo: true, path }) : noContentResponse()
}

function handleUserTokenRoute(path: string, url: URL): Response {
  const parts = path.split('/')
  const id = decodeURIComponent(parts[4] ?? DEMO_TOKEN_ID)
  const token = demoState.tokens.find((item) => item.id === id) ?? demoState.tokens[0]
  if (path.endsWith('/secret')) return jsonResponse({ token: demoState.tokenSecrets.get(token.id) ?? DEMO_TOKEN })
  if (path.endsWith('/logs')) return jsonResponse(publicTokenLogs(demoState.logs.filter((log) => log.auth_token_id === token.id)))
  if (path.endsWith('/events')) return textResponse('demo event stream is provided by the browser demo runtime\n')
  return jsonResponse(token)
}

function handleKeyRoute(path: string, url: URL, method: string): Response {
  const match = path.match(/^\/api\/keys\/([^/]+)/)
  const id = decodeURIComponent(match?.[1] ?? DEMO_KEY_ID)
  const key = demoState.keys.find((item) => item.id === id) ?? demoState.keys[0]
  if (method === 'DELETE') return noContentResponse()
  if (path.endsWith('/sync-usage') || path.endsWith('/status') || path.endsWith('/quarantine')) return noContentResponse()
  if (path.endsWith('/secret')) return jsonResponse({ api_key: `tvly-dev-${id.replace(/[^a-z0-9]/gi, '').slice(0, 18)}demo` })
  if (path.includes('/metrics')) return jsonResponse({
    total_requests: key.total_requests,
    success_count: key.success_count,
    error_count: key.error_count,
    quota_exhausted_count: key.quota_exhausted_count,
    active_keys: 1,
    exhausted_keys: key.status === 'exhausted' ? 1 : 0,
    last_activity: key.last_used_at,
  })
  if (path.endsWith('/logs/list')) return jsonResponse(buildCursorListPage(demoState.logs.filter((log) => log.key_id === id), url))
  if (path.endsWith('/logs/catalog')) return jsonResponse(requestLogsCatalog())
  if (path.endsWith('/logs/page')) return jsonResponse(requestLogsPage(url, demoState.logs.filter((log) => log.key_id === id)))
  if (path.includes('/logs/') && path.endsWith('/details')) return jsonResponse(demoLogDetailForPath(path, demoState.logs.filter((log) => log.key_id === id)))
  if (path.endsWith('/logs')) return jsonResponse(demoState.logs.filter((log) => log.key_id === id))
  if (path.endsWith('/sticky-users')) return jsonResponse({ ...buildListPage(demoState.users.map((user) => ({
    user: { userId: user.userId, displayName: user.displayName, username: user.username, active: user.active, lastLoginAt: user.lastLoginAt, tokenCount: user.tokenCount },
    lastSuccessAt: nowSeconds(-360),
    windows: { yesterday: { successCredits: 120, failureCredits: 4 }, today: { successCredits: 144, failureCredits: 6 }, month: { successCredits: 3200, failureCredits: 88 } },
    dailyBuckets: range(7).map((index) => ({ bucketStart: nowSeconds(-(7 - index) * 86400), bucketEnd: nowSeconds(-(6 - index) * 86400), successCredits: 80 + index * 4, failureCredits: index % 3 })),
  })), url) })
  if (path.endsWith('/sticky-nodes')) return jsonResponse({ rangeStart: isoSeconds(-86400), rangeEnd: isoSeconds(0), bucketSeconds: 3600, nodes: forwardProxyStats().nodes.slice(0, 2).map((node, index) => ({ ...node, role: index === 0 ? 'primary' : 'secondary' })) })
  return jsonResponse(key)
}

function handleUserRoute(path: string, url: URL, method: string, init?: RequestInit): Response {
  const match = path.match(/^\/api\/users\/([^/]+)/)
  const id = decodeURIComponent(match?.[1] ?? 'user-demo-admin')
  const user = demoState.users.find((item) => item.userId === id) ?? demoState.users[0]
  if (method !== 'GET') return noContentResponse()
  if (path.endsWith('/usage-series')) return jsonResponse({ limit: user.quotaHourlyLimit, points: range(24).map((index) => ({ bucketStart: nowSeconds(-(23 - index) * 3600), displayBucketStart: nowSeconds(-(23 - index) * 3600), value: 20 + index, limitValue: user.quotaHourlyLimit })) })
  if (path.endsWith('/broken-keys')) return jsonResponse({ ...buildListPage([{ keyId: DEMO_QUOTA_KEY_ID, currentStatus: 'exhausted', reasonCode: 'upstream_usage_limit_432', reasonSummary: 'Demo quota exhausted', latestBreakAt: nowSeconds(-4200), source: 'request_log', breakerTokenId: DEMO_TOKEN_ID, breakerUserId: user.userId, breakerUserDisplayName: user.displayName, manualActorDisplayName: null, relatedUsers: [] }], url) })
  return jsonResponse({
    ...user,
    tokenCount: demoState.tokens.length,
    tokens: demoState.tokens.map((token) => ({ tokenId: token.id, enabled: token.enabled, note: token.note, createdAt: token.created_at, lastUsedAt: token.last_used_at, totalRequests: token.total_requests, dailySuccess: token.quota_daily_used, dailyFailure: 18, monthlySuccess: token.quota_monthly_used })),
    quotaBase: { hourlyAnyLimit: 100, hourlyLimit: 100, dailyLimit: 1000, monthlyLimit: 10000, inheritsDefaults: true },
    effectiveQuota: { hourlyAnyLimit: user.hourlyAnyLimit, hourlyLimit: user.quotaHourlyLimit, dailyLimit: user.quotaDailyLimit, monthlyLimit: user.quotaMonthlyLimit, inheritsDefaults: false },
    quotaBreakdown: [],
    recentIpAddresses24h: ['198.51.100.24', '203.0.113.45'],
    recentIpAddresses7d: ['198.51.100.24', '203.0.113.45', '192.0.2.14'],
    recentIpTimeline7d: range(3).map((index) => ({ ipAddress: `198.51.100.${20 + index}`, firstSeenAt: nowSeconds(-(index + 1) * 86400), lastSeenAt: nowSeconds(-index * 2400), requestCount: 20 + index })),
    recharge: demoAdminUserRechargeAudit(demoState.rechargeOrders, user.userId),
  })
}

async function handleUserTags(path: string, method: string, init?: RequestInit): Promise<Response> {
  const id = decodeURIComponent(path.match(/^\/api\/user-tags\/([^/]+)/)?.[1] ?? '')
  if (method === 'GET') return jsonResponse({ items: demoState.userTags })
  if (method === 'DELETE') {
    demoState.userTags = demoState.userTags.filter((tag) => tag.id !== id)
    return noContentResponse()
  }

  const body = await readJsonBody(init)
  const payload = {
    name: typeof body.name === 'string' && body.name.trim() ? body.name.trim() : 'demo',
    displayName: typeof body.displayName === 'string' && body.displayName.trim() ? body.displayName.trim() : 'Demo',
    icon: typeof body.icon === 'string' ? body.icon : null,
    effectKind: typeof body.effectKind === 'string' ? body.effectKind : 'quota_delta',
    hourlyAnyDelta: typeof body.hourlyAnyDelta === 'number' ? body.hourlyAnyDelta : 0,
    hourlyDelta: typeof body.hourlyDelta === 'number' ? body.hourlyDelta : 0,
    dailyDelta: typeof body.dailyDelta === 'number' ? body.dailyDelta : 0,
    monthlyDelta: typeof body.monthlyDelta === 'number' ? body.monthlyDelta : 0,
  }

  if (method === 'PATCH') {
    const index = demoState.userTags.findIndex((tag) => tag.id === id)
    const nextTag = createDemoUserTag(id || `tag-${payload.name}`, payload, index >= 0 ? demoState.userTags[index].userCount : 0)
    if (index >= 0) {
      demoState.userTags[index] = nextTag
    } else {
      demoState.userTags.push(nextTag)
    }
    return jsonResponse(nextTag)
  }

  const nextId = `tag-${payload.name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || Date.now()}`
  const nextTag = createDemoUserTag(nextId, payload)
  demoState.userTags.push(nextTag)
  return jsonResponse(nextTag)
}

function handleTokenRoute(path: string, url: URL, method: string): Response {
  const match = path.match(/^\/api\/tokens\/([^/]+)/)
  const id = decodeURIComponent(match?.[1] ?? DEMO_TOKEN_ID)
  const token = demoState.tokens.find((item) => item.id === id) ?? demoState.tokens[0]
  if (method === 'DELETE') {
    if (demoState.tokens.length > 1) {
      demoState.tokens = demoState.tokens.filter((item) => item.id !== id)
      demoState.tokenSecrets.delete(id)
    }
    return noContentResponse()
  }
  if (path.endsWith('/status') || path.endsWith('/note')) return noContentResponse()
  if (path.endsWith('/secret') || path.endsWith('/secret/rotate')) return jsonResponse({ token: demoState.tokenSecrets.get(token.id) ?? DEMO_TOKEN })
  if (path.endsWith('/metrics/hourly')) return jsonResponse(range(25).map((index) => ({ bucket_start: nowSeconds(-(24 - index) * 3600), success_count: 12 + index, system_failure_count: index % 4, external_failure_count: index % 6 === 0 ? 2 : 0 })))
  if (path.endsWith('/metrics/usage-series')) return jsonResponse(tokenUsageSeries(url))
  if (path.includes('/metrics')) return jsonResponse(tokenSummary(id))
  if (path.endsWith('/logs/list')) return jsonResponse(buildCursorListPage(demoState.logs.filter((log) => log.auth_token_id === id), url))
  if (path.endsWith('/logs/catalog')) return jsonResponse(requestLogsCatalog())
  if (path.endsWith('/logs/page')) return jsonResponse(requestLogsPage(url, demoState.logs.filter((log) => log.auth_token_id === id)))
  if (path.includes('/logs/') && path.endsWith('/details')) return jsonResponse(demoLogDetailForPath(path, demoState.logs.filter((log) => log.auth_token_id === id)))
  if (path.endsWith('/broken-keys')) return jsonResponse({ ...buildListPage([{ keyId: DEMO_QUOTA_KEY_ID, currentStatus: 'exhausted', reasonCode: 'upstream_usage_limit_432', reasonSummary: 'Demo quota exhausted', latestBreakAt: nowSeconds(-4200), source: 'request_log', breakerTokenId: id, breakerUserId: 'user-demo-admin', breakerUserDisplayName: 'Hikari Demo Admin', manualActorDisplayName: null, relatedUsers: [] }], url) })
  if (path.endsWith('/events')) return textResponse('demo event stream is provided by the browser demo runtime\n')
  return jsonResponse(token)
}

async function handleCreateDemoRechargeOrder(init?: RequestInit): Promise<Response> {
  const body = await readJsonBody(init)
  const credits = typeof body.credits === 'number' ? body.credits : 0
  const months = typeof body.months === 'number' ? body.months : 0
  const isTestOffer = credits === DEMO_TEST_RECHARGE_CREDITS && months === DEMO_TEST_RECHARGE_MONTHS
  const isRegularOffer = credits >= DEMO_RECHARGE_UNIT_CREDITS
    && credits <= 20_000
    && credits % DEMO_RECHARGE_UNIT_CREDITS === 0
    && months >= 1
    && months <= 12

  if (!isTestOffer && !isRegularOffer) {
    return jsonResponse({ message: 'Demo recharge only supports 1x1 test offer or regular 1000-credit steps.' }, 400)
  }

  const amountLdc = isTestOffer
    ? DEMO_TEST_RECHARGE_AMOUNT_LDC
    : (credits / DEMO_RECHARGE_UNIT_CREDITS) * DEMO_RECHARGE_UNIT_PRICE_LDC * months
  const outTradeNo = `ldc_demo_${Date.now()}`
  const paymentUrl = `${window.location.origin}/console/dashboard?demo_checkout=${encodeURIComponent(outTradeNo)}`
  const order: DemoRechargeOrder = {
    outTradeNo,
    userId: DEMO_TOKEN_OWNER.userId,
    userDisplayName: DEMO_TOKEN_OWNER.displayName,
    username: DEMO_TOKEN_OWNER.username,
    status: 'pending',
    credits,
    months,
    money: amountLdc.toFixed(2),
    tradeNo: null,
    paymentUrl,
    createdAt: nowSeconds(),
    updatedAt: nowSeconds(),
    paidAt: null,
    refundedAt: null,
    refundActor: null,
    lastNotifyAt: null,
    lastError: null,
  }
  demoState.rechargeOrders.unshift(order)
  return jsonResponse({ order, paymentUrl })
}

function handleTavilyProbe(path: string): JsonValue {
  if (path.includes('/research/')) return { request_id: 'demo-research-001', status: 'completed', answer: 'Demo research result is ready.' }
  if (path.endsWith('/research')) return { request_id: 'demo-research-001', status: 'queued' }
  if (path.endsWith('/extract')) return { results: [{ url: 'https://example.test/demo', raw_content: 'Demo extracted content.' }] }
  if (path.endsWith('/crawl')) return { base_url: 'https://example.test', results: [{ url: 'https://example.test/demo', title: 'Demo crawl page' }] }
  if (path.endsWith('/map')) return { base_url: 'https://example.test', links: ['https://example.test/demo', 'https://example.test/docs'] }
  return { query: 'demo mode', answer: 'Mocked Tavily search result', results: [{ title: 'Demo result', url: 'https://example.test/demo', content: 'This response is generated entirely in the browser demo mode.' }] }
}

async function handleMcpDemo(init?: RequestInit): Promise<Response> {
  const body = await readJsonBody(init)
  const method = typeof body.method === 'string' ? body.method : ''
  const id = body.id ?? 'demo'
  const headers = new Headers({ 'Content-Type': 'application/json; charset=utf-8', 'Mcp-Session-Id': 'demo-session-001', 'X-Tavily-Hikari-Demo': 'true' })
  if (method === 'notifications/initialized') return new Response(JSON.stringify({ jsonrpc: '2.0', result: null }), { status: 202, headers })
  if (method === 'initialize') return new Response(JSON.stringify({ jsonrpc: '2.0', id, result: { protocolVersion: '2025-03-26', capabilities: { tools: {} }, serverInfo: { name: 'tavily-hikari-demo', version: 'demo' } } }), { headers })
  if (method === 'tools/list') return new Response(JSON.stringify({ jsonrpc: '2.0', id, result: { tools: [{ name: 'tavily_search', description: 'Browser-only demo search tool', inputSchema: { type: 'object' } }] } }), { headers })
  if (method === 'tools/call') return new Response(JSON.stringify({ jsonrpc: '2.0', id, result: { content: [{ type: 'text', text: 'Demo MCP tool call completed without contacting an upstream service.' }] } }), { headers })
  return new Response(JSON.stringify({ jsonrpc: '2.0', id, result: {} }), { headers })
}

class DemoEventSource {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSED = 2

  readonly url: string
  readonly withCredentials = false
  readyState = DemoEventSource.CONNECTING
  onopen: ((this: EventSource, ev: Event) => unknown) | null = null
  onmessage: ((this: EventSource, ev: MessageEvent) => unknown) | null = null
  onerror: ((this: EventSource, ev: Event) => unknown) | null = null
  private listeners = new Map<string, Set<DemoListener>>()
  private refreshTimer: number | null = null

  constructor(url: string | URL) {
    this.url = String(url)
    activeEventSources.add(this)
    window.setTimeout(() => {
      if (this.readyState === DemoEventSource.CLOSED) return
      this.readyState = DemoEventSource.OPEN
      this.dispatch('open', new Event('open'))
      this.emitInitialDemoEvent()
    }, 40)
  }

  addEventListener(type: string, listener: DemoListener | null): void {
    if (!listener) return
    const set = this.listeners.get(type) ?? new Set<DemoListener>()
    set.add(listener)
    this.listeners.set(type, set)
  }

  removeEventListener(type: string, listener: DemoListener | null): void {
    if (!listener) return
    this.listeners.get(type)?.delete(listener)
  }

  dispatchEvent(event: Event): boolean {
    this.dispatch(event.type, event)
    return true
  }

  close(): void {
    this.readyState = DemoEventSource.CLOSED
    if (this.refreshTimer != null) {
      window.clearInterval(this.refreshTimer)
      this.refreshTimer = null
    }
    activeEventSources.delete(this)
    this.listeners.clear()
  }

  emit(type: string, payload: unknown): void {
    if (this.readyState === DemoEventSource.CLOSED) return
    const event = new MessageEvent(type, { data: JSON.stringify(payload) })
    this.dispatch(type, event)
    if (type === 'message') this.onmessage?.call(this as unknown as EventSource, event)
  }

  private dispatch(type: string, event: Event): void {
    if (type === 'open') this.onopen?.call(this as unknown as EventSource, event)
    if (type === 'error') this.onerror?.call(this as unknown as EventSource, event)
    for (const listener of this.listeners.get(type) ?? []) {
      if (typeof listener === 'function') {
        listener.call(this as unknown as EventSource, event)
      } else {
        listener.handleEvent(event)
      }
    }
  }

  private emitInitialDemoEvent(): void {
    const url = parseUrl(this.url)
    const path = url?.pathname ?? ''
    if (path === '/api/events') {
      this.emit('snapshot', demoDashboardOverview())
      this.refreshTimer = window.setInterval(() => {
        if (this.readyState !== DemoEventSource.OPEN) return
        this.emit('snapshot', demoDashboardOverview())
      }, 6000)
      return
    }
    if (path === '/api/public/events') {
      this.emit('metrics', {
        public: { monthlySuccess: 23710, dailySuccess: 714 },
        token: {
          monthlySuccess: 8400,
          dailySuccess: 388,
          dailyFailure: 18,
          quotaHourlyUsed: 42,
          quotaHourlyLimit: 180,
          quotaDailyUsed: 388,
          quotaDailyLimit: 1600,
          quotaMonthlyUsed: 8400,
          quotaMonthlyLimit: 24000,
        },
      })
      this.refreshTimer = window.setInterval(() => {
        if (this.readyState !== DemoEventSource.OPEN) return
        const pulse = demoPulse()
        this.emit('metrics', {
          public: { monthlySuccess: 23710 + (pulse % 6) * 20, dailySuccess: 714 + (pulse % 5) * 3 },
          token: {
            monthlySuccess: 8400 + (pulse % 6) * 12,
            dailySuccess: 388 + (pulse % 5) * 2,
            dailyFailure: 18 + (pulse % 3),
            quotaHourlyUsed: 42 + (pulse % 4),
            quotaHourlyLimit: 180,
            quotaDailyUsed: 388 + (pulse % 5) * 2,
            quotaDailyLimit: 1600,
            quotaMonthlyUsed: 8400 + (pulse % 6) * 12,
            quotaMonthlyLimit: 24000,
          },
        })
      }, 6000)
      return
    }
    if (path.startsWith('/api/user/tokens/')) {
      const tokenId = decodeURIComponent(path.split('/')[4] ?? DEMO_TOKEN_ID)
      const token = demoState.tokens.find((item) => item.id === tokenId) ?? demoState.tokens[0]
      const logs = demoState.logs.filter((log) => log.auth_token_id === token.id)
      this.emit('snapshot', { token, logs: publicTokenLogs(logs) })
      return
    }
    if (path.startsWith('/api/tokens/')) {
      const tokenId = decodeURIComponent(path.split('/')[3] ?? DEMO_TOKEN_ID)
      const logs = demoState.logs.filter((log) => log.auth_token_id === tokenId)
      this.emit('snapshot', { summary: tokenSummary(tokenId), logs: logs.slice(0, 12) })
    }
  }
}

const activeEventSources = new Set<DemoEventSource>()

export function installDemoRuntime(): void {
  if (!isDemoMode() || typeof window === 'undefined' || window.__tavilyHikariDemoInstalled) return
  window.__tavilyHikariDemoInstalled = true
  document.documentElement.dataset.demoMode = 'true'

  const originalFetch = window.fetch.bind(window)
  window.__tavilyHikariDemoFetch = originalFetch
  window.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
    const response = await demoFetch(input, init)
    if (response) return response
    return originalFetch(input, init)
  }

  window.__tavilyHikariDemoEventSource = window.EventSource
  window.EventSource = DemoEventSource as unknown as typeof EventSource
}
