import {
  getProbeEnvelopeError,
  requestMcpProbeEnvelopeWithToken,
  requestMcpProbeNotificationWithToken,
} from '../lib/mcpProbe'
import type { TokenLogRequestKindOption } from '../tokenLogRequestKinds'

export interface Summary {
  total_requests: number
  success_count: number
  error_count: number
  quota_exhausted_count: number
  active_keys: number
  exhausted_keys: number
  quarantined_keys: number
  temporary_isolated_keys: number
  last_activity: number | null
  total_quota_limit: number
  total_quota_remaining: number
}

export interface SummaryQuotaCharge {
  local_estimated_credits: number
  upstream_actual_credits: number
  sampled_key_count: number
  stale_key_count: number
  latest_sync_at: number | null
}

export interface SummaryWindowMetrics {
  total_requests: number
  success_count: number
  error_count: number
  quota_exhausted_count: number
  valuable_success_count: number
  valuable_failure_count: number
  other_success_count: number
  other_failure_count: number
  unknown_count: number
  upstream_exhausted_key_count: number
  new_keys: number
  new_quarantines: number
  quota_charge?: SummaryQuotaCharge
}

export interface SummaryWindowsResponse {
  today: SummaryWindowMetrics
  yesterday: SummaryWindowMetrics
  month: SummaryWindowMetrics
}

export interface DashboardSiteStatusSnapshot {
  remainingQuota: number
  totalQuotaLimit: number
  activeKeys: number
  quarantinedKeys: number
  temporaryIsolatedKeys: number
  exhaustedKeys: number
  availableProxyNodes: number | null
  totalProxyNodes: number | null
}

export interface DashboardForwardProxySnapshot {
  availableNodes: number | null
  totalNodes: number | null
}

export interface ForwardProxyDashboardSummaryResponse {
  availableNodes: number
  totalNodes: number
}

export type DashboardTokenCoverage = 'ok' | 'truncated' | 'error'

export interface DashboardTrendBuckets {
  request: number[]
  error: number[]
}

export interface DashboardHourlyRequestBucket {
  bucketStart: number
  secondarySuccess: number
  primarySuccess: number
  secondaryFailure: number
  primaryFailure429: number
  primaryFailureOther: number
  unknown: number
  mcpNonBillable: number
  mcpBillable: number
  apiNonBillable: number
  apiBillable: number
}

export interface DashboardHourlyRequestWindow {
  bucketSeconds: number
  visibleBuckets: number
  retainedBuckets: number
  buckets: DashboardHourlyRequestBucket[]
}

export interface DashboardOverviewResponse {
  summary: Summary
  summaryWindows: SummaryWindowsResponse
  hourlyRequestWindow: DashboardHourlyRequestWindow
  siteStatus: DashboardSiteStatusSnapshot
  forwardProxy: DashboardForwardProxySnapshot
  trend: DashboardTrendBuckets
  exhaustedKeys: ApiKeyStats[]
  recentLogs: RequestLog[]
  recentJobs: JobLogView[]
  disabledTokens: AuthToken[]
  tokenCoverage: DashboardTokenCoverage
  recentAlerts: RecentAlertsSummary
}

export interface DashboardSnapshotEvent extends DashboardOverviewResponse {
  keys: ApiKeyStats[]
  logs: RequestLog[]
}

export type AlertType =
  | 'upstream_rate_limited_429'
  | 'upstream_usage_limit_432'
  | 'upstream_key_blocked'
  | 'user_request_rate_limited'
  | 'user_quota_exhausted'

export interface AlertFacetOption {
  value: string
  label: string
  count: number
}

export interface AlertEntityRef {
  id: string
  label: string
}

export interface AlertUserRef {
  userId: string
  displayName: string | null
  username: string | null
}

export interface AlertRequestRef {
  id: number
  method: string
  path: string
  query: string | null
}

export interface AlertRequestKind {
  key: string
  label: string
  detail: string | null
}

export interface AlertSourceRef {
  kind: string
  id: string
}

export interface AlertEvent {
  id: string
  type: AlertType
  title: string
  summary: string
  occurredAt: number
  subjectKind: 'user' | 'token' | 'key'
  subjectId: string
  subjectLabel: string
  user: AlertUserRef | null
  token: AlertEntityRef | null
  key: AlertEntityRef | null
  request: AlertRequestRef | null
  requestKind: AlertRequestKind | null
  failureKind: string | null
  resultStatus: string | null
  errorMessage: string | null
  reasonCode: string | null
  reasonSummary: string | null
  reasonDetail: string | null
  source: AlertSourceRef
}

export interface AlertGroup {
  id: string
  type: AlertType
  subjectKind: 'user' | 'token' | 'key'
  subjectId: string
  subjectLabel: string
  user: AlertUserRef | null
  token: AlertEntityRef | null
  key: AlertEntityRef | null
  requestKind: AlertRequestKind | null
  count: number
  firstSeen: number
  lastSeen: number
  latestEvent: AlertEvent
}

export interface AlertTypeCount {
  type: AlertType
  count: number
}

export interface AlertCatalog {
  retentionDays: number
  types: LogFacetOption[]
  requestKindOptions: TokenLogRequestKindOption[]
  users: AlertFacetOption[]
  tokens: AlertFacetOption[]
  keys: AlertFacetOption[]
}

export interface RecentAlertsSummary {
  windowHours: number
  totalEvents: number
  groupedCount: number
  countsByType: AlertTypeCount[]
  topGroups: AlertGroup[]
}

export interface AlertsPage<T> {
  items: T[]
  total: number
  page: number
  perPage: number
}

export interface PublicMetrics {
  monthlySuccess: number
  dailySuccess: number
}

export interface TokenMetrics {
  monthlySuccess: number
  dailySuccess: number
  dailyFailure: number
  quotaHourlyUsed: number
  quotaHourlyLimit: number
  quotaDailyUsed: number
  quotaDailyLimit: number
  quotaMonthlyUsed: number
  quotaMonthlyLimit: number
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

interface ServerUserTokenEventSnapshot {
  token: UserTokenSummary
  logs: ServerPublicTokenLog[]
}

export interface UserTokenEventSnapshot {
  token: UserTokenSummary
  logs: PublicTokenLog[]
}

export interface ApiKeyQuarantine {
  source: string
  reasonCode: string
  reasonSummary: string
  reasonDetail?: string | null
  createdAt: number
}

export interface ApiKeyStats {
  id: string
  status: string
  group: string | null
  registration_ip: string | null
  registration_region: string | null
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
  quarantine: ApiKeyQuarantine | null
  transient_backoff: ApiKeyTransientBackoff | null
}

export interface ApiKeyTransientBackoff {
  reasonCode: string
  cooldownUntil: number
  retryAfterSecs: number
  scopes: string[]
}

export interface RequestLog {
  id: number
  key_id: string | null
  auth_token_id: string | null
  method: string
  path: string
  query: string | null
  http_status: number | null
  mcp_status: number | null
  business_credits?: number | null
  request_kind_key?: string
  request_kind_label?: string
  request_kind_detail?: string | null
  result_status: string
  created_at: number
  error_message: string | null
  failure_kind?: string | null
  key_effect_code?: string
  key_effect_summary?: string | null
  binding_effect_code?: string
  binding_effect_summary?: string | null
  selection_effect_code?: string
  selection_effect_summary?: string | null
  gateway_mode?: string | null
  experiment_variant?: string | null
  proxy_session_id?: string | null
  routing_subject_hash?: string | null
  upstream_operation?: string | null
  fallback_reason?: string | null
  request_body: string | null
  response_body: string | null
  forwarded_headers: string[]
  dropped_headers: string[]
  operationalClass:
    | 'success'
    | 'neutral'
    | 'client_error'
    | 'upstream_error'
    | 'system_error'
    | 'quota_exhausted'
  requestKindProtocolGroup: 'api' | 'mcp'
  requestKindBillingGroup: 'billable' | 'non_billable'
}

export interface RequestLogBodies {
  request_body: string | null
  response_body: string | null
}

export interface LogFacetOption {
  value: string
  count: number
}

export interface RequestLogFacets {
  results: LogFacetOption[]
  keyEffects: LogFacetOption[]
  bindingEffects: LogFacetOption[]
  selectionEffects: LogFacetOption[]
  tokens: LogFacetOption[]
  keys: LogFacetOption[]
}

export interface RequestLogsPage {
  items: RequestLog[]
  total: number
  page: number
  perPage: number
  requestKindOptions: TokenLogRequestKindOption[]
  facets: RequestLogFacets
}

export interface RequestLogsListPage {
  items: RequestLog[]
  pageSize: number
  nextCursor: string | null
  prevCursor: string | null
  hasOlder: boolean
  hasNewer: boolean
}

export interface RequestLogsCatalog {
  retentionDays: number
  requestKindOptions: TokenLogRequestKindOption[]
  facets: RequestLogFacets
}

interface ServerAlertFacetOption {
  value: string
  label: string
  count: number
}

interface ServerAlertEntityRef {
  id: string
  label: string
}

interface ServerAlertUserRef {
  userId: string
  displayName: string | null
  username: string | null
}

interface ServerAlertRequestRef {
  id: number
  method: string
  path: string
  query?: string | null
}

interface ServerAlertRequestKind {
  key: string
  label: string
  detail?: string | null
}

interface ServerAlertSourceRef {
  kind: string
  id: string
}

interface ServerAlertEvent {
  id: string
  type: AlertType
  title: string
  summary: string
  occurredAt: number
  subjectKind: 'user' | 'token' | 'key'
  subjectId: string
  subjectLabel: string
  user?: ServerAlertUserRef | null
  token?: ServerAlertEntityRef | null
  key?: ServerAlertEntityRef | null
  request?: ServerAlertRequestRef | null
  requestKind?: ServerAlertRequestKind | null
  failureKind?: string | null
  resultStatus?: string | null
  errorMessage?: string | null
  reasonCode?: string | null
  reasonSummary?: string | null
  reasonDetail?: string | null
  source: ServerAlertSourceRef
}

interface ServerAlertGroup {
  id: string
  type: AlertType
  subjectKind: 'user' | 'token' | 'key'
  subjectId: string
  subjectLabel: string
  user?: ServerAlertUserRef | null
  token?: ServerAlertEntityRef | null
  key?: ServerAlertEntityRef | null
  requestKind?: ServerAlertRequestKind | null
  count: number
  firstSeen: number
  lastSeen: number
  latestEvent: ServerAlertEvent
}

interface ServerAlertsPage<T> {
  items: T[]
  total: number
  page: number
  perPage?: number
  per_page?: number
}

interface ServerAlertCatalog {
  retentionDays?: number
  retention_days?: number
  types?: LogFacetOption[]
  requestKindOptions?: TokenLogRequestKindOption[]
  request_kind_options?: TokenLogRequestKindOption[]
  users?: ServerAlertFacetOption[]
  tokens?: ServerAlertFacetOption[]
  keys?: ServerAlertFacetOption[]
}

interface ServerAlertTypeCount {
  type: AlertType
  count: number
}

interface ServerRecentAlertsSummary {
  windowHours?: number
  window_hours?: number
  totalEvents?: number
  total_events?: number
  groupedCount?: number
  grouped_count?: number
  countsByType?: ServerAlertTypeCount[]
  counts_by_type?: ServerAlertTypeCount[]
  topGroups?: ServerAlertGroup[]
  top_groups?: ServerAlertGroup[]
}

interface ServerLogFacetOption {
  value: string
  count: number
}

interface ServerRequestLogFacets {
  results?: ServerLogFacetOption[]
  keyEffects?: ServerLogFacetOption[]
  key_effects?: ServerLogFacetOption[]
  bindingEffects?: ServerLogFacetOption[]
  binding_effects?: ServerLogFacetOption[]
  selectionEffects?: ServerLogFacetOption[]
  selection_effects?: ServerLogFacetOption[]
  tokens?: ServerLogFacetOption[]
  keys?: ServerLogFacetOption[]
}

interface ServerRequestLogsPage {
  items: RequestLog[]
  total: number
  page: number
  perPage?: number
  per_page?: number
  requestKindOptions?: TokenLogRequestKindOption[]
  request_kind_options?: TokenLogRequestKindOption[]
  facets?: ServerRequestLogFacets
}

interface ServerRequestLogsListPage {
  items?: RequestLog[]
  pageSize?: number
  page_size?: number
  nextCursor?: string | null
  next_cursor?: string | null
  prevCursor?: string | null
  prev_cursor?: string | null
  hasOlder?: boolean
  has_older?: boolean
  hasNewer?: boolean
  has_newer?: boolean
}

interface ServerRequestLogsCatalog {
  retentionDays?: number
  retention_days?: number
  requestKindOptions?: TokenLogRequestKindOption[]
  request_kind_options?: TokenLogRequestKindOption[]
  facets?: ServerRequestLogFacets
}

export interface RequestLogsPageQuery {
  page?: number
  perPage?: number
  requestKinds?: string[]
  result?: LogResultFilter
  keyEffect?: string
  bindingEffect?: string
  selectionEffect?: string
  operationalClass?: LogOperationalClass | 'all'
  includeBodies?: boolean
  tokenId?: string
  keyId?: string
  since?: number
  sinceIso?: string
  untilIso?: string
}

export interface RequestLogsListQuery
  extends Omit<RequestLogsPageQuery, 'page' | 'perPage' | 'includeBodies'> {
  limit?: number
  cursor?: string | null
  direction?: 'older' | 'newer'
}

export type RequestLogsCatalogQuery = Omit<RequestLogsListQuery, 'limit' | 'cursor' | 'direction'>

function normalizeRequestLogFacets(value?: ServerRequestLogFacets): RequestLogFacets {
  return {
    results: value?.results ?? [],
    keyEffects: value?.keyEffects ?? value?.key_effects ?? [],
    bindingEffects: value?.bindingEffects ?? value?.binding_effects ?? [],
    selectionEffects: value?.selectionEffects ?? value?.selection_effects ?? [],
    tokens: value?.tokens ?? [],
    keys: value?.keys ?? [],
  }
}

function normalizeRequestLogsPage(value: ServerRequestLogsPage): RequestLogsPage {
  return {
    items: value.items ?? [],
    total: value.total ?? 0,
    page: value.page ?? 1,
    perPage: value.perPage ?? value.per_page ?? 20,
    requestKindOptions: value.requestKindOptions ?? value.request_kind_options ?? [],
    facets: normalizeRequestLogFacets(value.facets),
  }
}

function normalizeRequestLogsListPage(value: ServerRequestLogsListPage): RequestLogsListPage {
  return {
    items: value.items ?? [],
    pageSize: value.pageSize ?? value.page_size ?? 20,
    nextCursor: value.nextCursor ?? value.next_cursor ?? null,
    prevCursor: value.prevCursor ?? value.prev_cursor ?? null,
    hasOlder: value.hasOlder ?? value.has_older ?? false,
    hasNewer: value.hasNewer ?? value.has_newer ?? false,
  }
}

function normalizeRequestLogsCatalog(value: ServerRequestLogsCatalog): RequestLogsCatalog {
  return {
    retentionDays: value.retentionDays ?? value.retention_days ?? 0,
    requestKindOptions: value.requestKindOptions ?? value.request_kind_options ?? [],
    facets: normalizeRequestLogFacets(value.facets),
  }
}

function normalizeAlertFacetOption(value: ServerAlertFacetOption): AlertFacetOption {
  return {
    value: value.value,
    label: value.label,
    count: value.count,
  }
}

function normalizeAlertEntityRef(value?: ServerAlertEntityRef | null): AlertEntityRef | null {
  if (!value) return null
  return {
    id: value.id,
    label: value.label,
  }
}

function normalizeAlertUserRef(value?: ServerAlertUserRef | null): AlertUserRef | null {
  if (!value) return null
  return {
    userId: value.userId,
    displayName: value.displayName ?? null,
    username: value.username ?? null,
  }
}

function normalizeAlertRequestRef(value?: ServerAlertRequestRef | null): AlertRequestRef | null {
  if (!value) return null
  return {
    id: value.id,
    method: value.method,
    path: value.path,
    query: value.query ?? null,
  }
}

function normalizeAlertRequestKind(value?: ServerAlertRequestKind | null): AlertRequestKind | null {
  if (!value) return null
  return {
    key: value.key,
    label: value.label,
    detail: value.detail ?? null,
  }
}

function normalizeAlertSource(value: ServerAlertSourceRef): AlertSourceRef {
  return {
    kind: value.kind,
    id: value.id,
  }
}

function normalizeAlertEvent(value: ServerAlertEvent): AlertEvent {
  return {
    id: value.id,
    type: value.type,
    title: value.title,
    summary: value.summary,
    occurredAt: value.occurredAt,
    subjectKind: value.subjectKind,
    subjectId: value.subjectId,
    subjectLabel: value.subjectLabel,
    user: normalizeAlertUserRef(value.user),
    token: normalizeAlertEntityRef(value.token),
    key: normalizeAlertEntityRef(value.key),
    request: normalizeAlertRequestRef(value.request),
    requestKind: normalizeAlertRequestKind(value.requestKind),
    failureKind: value.failureKind ?? null,
    resultStatus: value.resultStatus ?? null,
    errorMessage: value.errorMessage ?? null,
    reasonCode: value.reasonCode ?? null,
    reasonSummary: value.reasonSummary ?? null,
    reasonDetail: value.reasonDetail ?? null,
    source: normalizeAlertSource(value.source),
  }
}

function normalizeAlertGroup(value: ServerAlertGroup): AlertGroup {
  return {
    id: value.id,
    type: value.type,
    subjectKind: value.subjectKind,
    subjectId: value.subjectId,
    subjectLabel: value.subjectLabel,
    user: normalizeAlertUserRef(value.user),
    token: normalizeAlertEntityRef(value.token),
    key: normalizeAlertEntityRef(value.key),
    requestKind: normalizeAlertRequestKind(value.requestKind),
    count: value.count,
    firstSeen: value.firstSeen,
    lastSeen: value.lastSeen,
    latestEvent: normalizeAlertEvent(value.latestEvent),
  }
}

function normalizeAlertsPage<TSource, TTarget>(
  value: ServerAlertsPage<TSource>,
  mapItem: (item: TSource) => TTarget,
): AlertsPage<TTarget> {
  return {
    items: (value.items ?? []).map(mapItem),
    total: value.total ?? 0,
    page: value.page ?? 1,
    perPage: value.perPage ?? value.per_page ?? 20,
  }
}

function normalizeAlertCatalog(value: ServerAlertCatalog): AlertCatalog {
  return {
    retentionDays: value.retentionDays ?? value.retention_days ?? 0,
    types: value.types ?? [],
    requestKindOptions: value.requestKindOptions ?? value.request_kind_options ?? [],
    users: (value.users ?? []).map(normalizeAlertFacetOption),
    tokens: (value.tokens ?? []).map(normalizeAlertFacetOption),
    keys: (value.keys ?? []).map(normalizeAlertFacetOption),
  }
}

function normalizeRecentAlertsSummary(value: ServerRecentAlertsSummary): RecentAlertsSummary {
  return {
    windowHours: value.windowHours ?? value.window_hours ?? 24,
    totalEvents: value.totalEvents ?? value.total_events ?? 0,
    groupedCount: value.groupedCount ?? value.grouped_count ?? 0,
    countsByType: (value.countsByType ?? value.counts_by_type ?? []).map((item) => ({
      type: item.type,
      count: item.count,
    })),
    topGroups: (value.topGroups ?? value.top_groups ?? []).map(normalizeAlertGroup),
  }
}

export interface AlertsQuery {
  page?: number
  perPage?: number
  type?: AlertType | null
  since?: string | null
  until?: string | null
  userId?: string | null
  tokenId?: string | null
  keyId?: string | null
  requestKinds?: string[]
}

function appendAlertsQueryParams(params: URLSearchParams, query: AlertsQuery): void {
  params.set('page', String(query.page ?? 1))
  params.set('per_page', String(query.perPage ?? 20))
  if (query.type) params.set('type', query.type)
  if (query.since?.trim()) params.set('since', query.since.trim())
  if (query.until?.trim()) params.set('until', query.until.trim())
  if (query.userId?.trim()) params.set('user_id', query.userId.trim())
  if (query.tokenId?.trim()) params.set('token_id', query.tokenId.trim())
  if (query.keyId?.trim()) params.set('key_id', query.keyId.trim())
  for (const requestKind of query.requestKinds ?? []) {
    const trimmed = requestKind.trim()
    if (trimmed) params.append('request_kind', trimmed)
  }
}

function appendRequestLogsPageFilters(
  params: URLSearchParams,
  {
    requestKinds,
    result,
    keyEffect,
    bindingEffect,
    selectionEffect,
    operationalClass,
    includeBodies,
    tokenId,
    keyId,
    since,
    sinceIso,
    untilIso,
  }: Pick<
    RequestLogsPageQuery,
    | 'requestKinds'
    | 'result'
    | 'keyEffect'
    | 'bindingEffect'
    | 'selectionEffect'
    | 'operationalClass'
    | 'includeBodies'
    | 'tokenId'
    | 'keyId'
    | 'since'
    | 'sinceIso'
    | 'untilIso'
  >,
) {
  for (const requestKind of requestKinds ?? []) {
    const trimmed = requestKind.trim()
    if (trimmed) params.append('request_kind', trimmed)
  }
  if (result) params.set('result', result)
  if (keyEffect?.trim()) params.set('key_effect', keyEffect.trim())
  if (bindingEffect?.trim()) params.set('binding_effect', bindingEffect.trim())
  if (selectionEffect?.trim()) params.set('selection_effect', selectionEffect.trim())
  if (operationalClass && operationalClass !== 'all') params.set('operational_class', operationalClass)
  if (includeBodies) params.set('include_bodies', 'true')
  if (tokenId?.trim()) params.set('auth_token_id', tokenId.trim())
  if (keyId?.trim()) params.set('key_id', keyId.trim())
  if (typeof since === 'number' && Number.isFinite(since)) params.set('since', String(since))
  if (sinceIso?.trim()) params.set('since', sinceIso.trim())
  if (untilIso?.trim()) params.set('until', untilIso.trim())
}

function appendRequestLogsListParams(params: URLSearchParams, query: RequestLogsListQuery) {
  params.set('limit', String(query.limit ?? 20))
  if (query.cursor?.trim()) params.set('cursor', query.cursor.trim())
  if (query.direction) params.set('direction', query.direction)
  appendRequestLogsPageFilters(params, query)
}

function appendRequestLogsCatalogParams(params: URLSearchParams, query: RequestLogsCatalogQuery) {
  appendRequestLogsPageFilters(params, query)
}

export interface ApiKeySecret {
  api_key: string
}

export interface ApiKeyFacetOption {
  value: string
  count: number
}

export interface ApiKeyListFacets {
  groups: ApiKeyFacetOption[]
  statuses: ApiKeyFacetOption[]
  regions: ApiKeyFacetOption[]
}

// ---- Access Tokens (for /mcp auth) ----
export interface TokenOwnerSummary {
  userId: string
  displayName: string | null
  username: string | null
}

export interface AuthToken {
  id: string // 4-char code
  enabled: boolean
  note: string | null
  group: string | null
  owner?: TokenOwnerSummary | null
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

export interface TodayWindowRange {
  todayStart: string
  todayEnd: string
}

function formatIso8601WithOffset(date: Date): string {
  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  const hours = String(date.getHours()).padStart(2, '0')
  const minutes = String(date.getMinutes()).padStart(2, '0')
  const seconds = String(date.getSeconds()).padStart(2, '0')
  const offsetMinutes = -date.getTimezoneOffset()
  const sign = offsetMinutes >= 0 ? '+' : '-'
  const absoluteOffsetMinutes = Math.abs(offsetMinutes)
  const offsetHours = String(Math.floor(absoluteOffsetMinutes / 60)).padStart(2, '0')
  const offsetRemainderMinutes = String(absoluteOffsetMinutes % 60).padStart(2, '0')
  return `${year}-${month}-${day}T${hours}:${minutes}:${seconds}${sign}${offsetHours}:${offsetRemainderMinutes}`
}

export function createBrowserTodayWindow(now = new Date()): TodayWindowRange {
  const start = new Date(now)
  start.setHours(0, 0, 0, 0)
  const end = new Date(start)
  end.setDate(end.getDate() + 1)
  return {
    todayStart: formatIso8601WithOffset(start),
    todayEnd: formatIso8601WithOffset(end),
  }
}

export function millisecondsUntilNextBrowserDayBoundary(now = new Date()): number {
  const next = new Date(now)
  next.setHours(24, 0, 0, 0)
  return Math.max(1, next.getTime() - now.getTime())
}

function appendTodayWindowRange(params: URLSearchParams, todayWindow?: TodayWindowRange): void {
  if (!todayWindow) return
  params.set('today_start', todayWindow.todayStart)
  params.set('today_end', todayWindow.todayEnd)
}

export function buildPublicEventsUrl(token?: string, todayWindow?: TodayWindowRange): string {
  const params = new URLSearchParams()
  if (token) params.set('token', token)
  appendTodayWindowRange(params, todayWindow)
  return `/api/public/events${params.toString() ? `?${params.toString()}` : ''}`
}

async function requestJson<T>(input: RequestInfo, init?: RequestInit): Promise<T> {
  const response = await fetch(input, init)
  if (!response.ok) {
    const message = await response.text().catch(() => response.statusText)
    const err = new Error(message || `Request failed with status ${response.status}`) as Error & {
      status?: number
    }
    err.status = response.status
    throw err
  }
  return (await response.json()) as T
}

async function requestNoContent(input: RequestInfo, init?: RequestInit): Promise<void> {
  const response = await fetch(input, init)
  if (!response.ok) {
    const message = await response.text().catch(() => response.statusText)
    const err = new Error(message || `Request failed with status ${response.status}`) as Error & {
      status?: number
    }
    err.status = response.status
    throw err
  }
}

async function requestJsonWithToken<T>(
  input: RequestInfo,
  token: string,
  init?: RequestInit,
): Promise<T> {
  const headers = new Headers(init?.headers ?? {})
  headers.set('Authorization', `Bearer ${token}`)
  if (init?.body != null && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json')
  }
  return requestJson<T>(input, { ...init, headers })
}

export type ForwardProxyProgressOperation = 'save' | 'validate' | 'revalidate'
export type ForwardProxyProgressPhaseKey =
  | 'save_settings'
  | 'validate_egress_socks5'
  | 'apply_egress_socks5'
  | 'refresh_subscription'
  | 'bootstrap_probe'
  | 'normalize_input'
  | 'parse_input'
  | 'fetch_subscription'
  | 'probe_nodes'
  | 'generate_result'
  | 'refresh_ui'

export type ForwardProxyProgressNodeStatus = 'pending' | 'probing' | 'ok' | 'failed'

export interface ForwardProxyProgressNodeState {
  nodeKey: string
  displayName: string
  protocol: string
  status: ForwardProxyProgressNodeStatus
  ok?: boolean | null
  latencyMs?: number | null
  ip?: string | null
  location?: string | null
  message?: string | null
}

export type ForwardProxyProgressEvent =
  | {
      type: 'phase'
      operation: ForwardProxyProgressOperation
      phaseKey: ForwardProxyProgressPhaseKey
      label: string
      current?: number | null
      total?: number | null
      detail?: string | null
    }
  | {
      type: 'complete'
      operation: ForwardProxyProgressOperation
      payload: unknown
    }
  | {
      type: 'nodes'
      operation: ForwardProxyProgressOperation
      nodes: ForwardProxyProgressNodeState[]
    }
  | {
      type: 'node'
      operation: ForwardProxyProgressOperation
      node: ForwardProxyProgressNodeState
    }
  | {
      type: 'error'
      operation: ForwardProxyProgressOperation
      message: string
      phaseKey?: ForwardProxyProgressPhaseKey | null
      label?: string | null
      current?: number | null
      total?: number | null
      detail?: string | null
    }

function extractErrorMessage(response: Response, fallbackBody?: string): Error & { status?: number } {
  const err = new Error(
    (fallbackBody != null && fallbackBody.trim().length > 0 ? fallbackBody : response.statusText)
      || `Request failed with status ${response.status}`,
  ) as Error & { status?: number }
  err.status = response.status
  return err
}

function parseJsonSseChunk<T>(chunk: string): T | null {
  const trimmed = chunk.trim()
  if (!trimmed) return null
  const data = trimmed
    .split(/\r?\n/)
    .filter((line) => line.startsWith('data:'))
    .map((line) => line.slice('data:'.length).trim())
    .join('\n')
  if (!data) return null
  return JSON.parse(data) as T
}

function parseForwardProxySseChunk(chunk: string): ForwardProxyProgressEvent | null {
  return parseJsonSseChunk<ForwardProxyProgressEvent>(chunk)
}

async function requestForwardProxyProgress<T>(
  input: RequestInfo,
  init: RequestInit,
  fallbackOperation: ForwardProxyProgressOperation,
  onEvent?: (event: ForwardProxyProgressEvent) => void,
): Promise<T> {
  const headers = new Headers(init.headers ?? {})
  headers.set('Accept', 'text/event-stream, application/json')
  if (init.body != null && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json')
  }

  const response = await fetch(input, { ...init, headers })
  const contentType = response.headers.get('Content-Type') ?? ''

  if (!response.ok && !contentType.includes('text/event-stream')) {
    const message = await response.text().catch(() => response.statusText)
    throw extractErrorMessage(response, message)
  }

  if (!contentType.includes('text/event-stream')) {
    const payload = (await response.json()) as T
    onEvent?.({ type: 'complete', operation: fallbackOperation, payload })
    return payload
  }

  if (!response.ok) {
    const message = await response.text().catch(() => response.statusText)
    throw extractErrorMessage(response, message)
  }

  const reader = response.body?.getReader()
  if (!reader) {
    throw new Error('Progress stream body is unavailable')
  }

  const decoder = new TextDecoder()
  let buffer = ''
  let completePayload: T | null = null

  while (true) {
    const { done, value } = await reader.read()
    buffer += decoder.decode(value ?? new Uint8Array(), { stream: !done })

    let boundaryIndex = buffer.search(/\r?\n\r?\n/)
    while (boundaryIndex >= 0) {
      const chunk = buffer.slice(0, boundaryIndex)
      buffer = buffer.slice(boundaryIndex + (buffer[boundaryIndex] === '\r' ? 4 : 2))
      const event = parseForwardProxySseChunk(chunk)
      if (event) {
        onEvent?.(event)
        if (event.type === 'complete') {
          completePayload = event.payload as T
        }
        if (event.type === 'error') {
          throw new Error(event.message || event.detail || 'Forward proxy progress stream failed')
        }
      }
      boundaryIndex = buffer.search(/\r?\n\r?\n/)
    }

    if (done) {
      const trailingEvent = parseForwardProxySseChunk(buffer)
      if (trailingEvent) {
        onEvent?.(trailingEvent)
        if (trailingEvent.type === 'complete') {
          completePayload = trailingEvent.payload as T
        }
        if (trailingEvent.type === 'error') {
          throw new Error(
            trailingEvent.message || trailingEvent.detail || 'Forward proxy progress stream failed',
          )
        }
      }
      break
    }
  }

  if (completePayload == null) {
    throw new Error('Forward proxy progress stream ended before completion')
  }

  return completePayload
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

export function fetchSummaryWindows(signal?: AbortSignal): Promise<SummaryWindowsResponse> {
  return requestJson('/api/summary/windows', { signal })
}

export function fetchDashboardOverview(signal?: AbortSignal): Promise<DashboardOverviewResponse> {
  return requestJson('/api/dashboard/overview', { signal })
}

export function fetchPublicMetrics(todayWindow?: TodayWindowRange, signal?: AbortSignal): Promise<PublicMetrics> {
  const params = new URLSearchParams()
  appendTodayWindowRange(params, todayWindow)
  const url = `/api/public/metrics${params.toString() ? `?${params.toString()}` : ''}`
  return requestJson(url, { signal })
}

export function fetchTokenMetrics(
  token: string,
  todayWindow?: TodayWindowRange,
  signal?: AbortSignal,
): Promise<TokenMetrics> {
  const params = new URLSearchParams({ token })
  appendTodayWindowRange(params, todayWindow)
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

export interface PaginatedApiKeys extends Paginated<ApiKeyStats> {
  facets: ApiKeyListFacets
}

export type ApiKeyBulkAction = 'delete' | 'clear_quarantine' | 'sync_usage'

export interface ApiKeyBulkActionSummary {
  requested: number
  succeeded: number
  skipped: number
  failed: number
}

export interface ApiKeyBulkActionResult {
  key_id: string
  status: 'success' | 'skipped' | 'failed'
  detail?: string | null
}

export interface ApiKeyBulkActionResponse {
  summary: ApiKeyBulkActionSummary
  results: ApiKeyBulkActionResult[]
}

export type ApiKeyBulkSyncProgressPhaseKey = 'prepare_request' | 'sync_usage' | 'refresh_ui'

export type ApiKeyBulkSyncProgressEvent =
  | {
      type: 'phase'
      phaseKey: ApiKeyBulkSyncProgressPhaseKey
      label: string
      current?: number | null
      total?: number | null
      detail?: string | null
    }
  | {
      type: 'item'
      keyId: string
      status: ApiKeyBulkActionResult['status']
      current: number
      total: number
      summary: ApiKeyBulkActionSummary
      detail?: string | null
    }
  | {
      type: 'complete'
      payload: ApiKeyBulkActionResponse
    }
  | {
      type: 'error'
      message: string
      phaseKey?: ApiKeyBulkSyncProgressPhaseKey | null
      detail?: string | null
    }

export function fetchApiKeys(
  page = 1,
  perPage = 20,
  options?: { groups?: string[]; statuses?: string[]; registrationIp?: string | null; regions?: string[] },
  signal?: AbortSignal,
): Promise<PaginatedApiKeys> {
  const params = new URLSearchParams({
    page: String(page),
    per_page: String(perPage),
  })
  for (const group of options?.groups ?? []) {
    const normalized = group.trim()
    params.append('group', normalized)
  }
  for (const status of options?.statuses ?? []) {
    const normalized = status.trim().toLowerCase()
    if (!normalized) continue
    params.append('status', normalized)
  }
  const normalizedRegistrationIp = options?.registrationIp?.trim()
  if (normalizedRegistrationIp) {
    params.set('registration_ip', normalizedRegistrationIp)
  }
  for (const region of options?.regions ?? []) {
    const normalized = region.trim()
    if (!normalized) continue
    params.append('region', normalized)
  }
  return requestJson(`/api/keys?${params.toString()}`, { signal })
}

export function fetchApiKeyDetail(id: string, signal?: AbortSignal): Promise<ApiKeyStats> {
  const encoded = encodeURIComponent(id)
  return requestJson(`/api/keys/${encoded}`, { signal })
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

export async function applyApiKeyBulkAction(
  action: ApiKeyBulkAction,
  keyIds: string[],
): Promise<ApiKeyBulkActionResponse> {
  const res = await fetch('/api/keys/bulk-actions', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      action,
      key_ids: keyIds,
    }),
  })
  if (!res.ok) {
    let message = ''
    try {
      const data = await res.json()
      message = (data?.detail as string) ?? (data?.error as string) ?? ''
    } catch {
      message = await res.text().catch(() => '')
    }
    const statusPart = ` (HTTP ${res.status})`
    throw new Error((message ? `${message}` : 'Failed to apply bulk key action') + statusPart)
  }
  return (await res.json()) as ApiKeyBulkActionResponse
}

export async function syncApiKeyBulkUsageWithProgress(
  keyIds: string[],
  onEvent?: (event: ApiKeyBulkSyncProgressEvent) => void,
): Promise<ApiKeyBulkActionResponse> {
  const headers = new Headers({ 'Content-Type': 'application/json' })
  headers.set('Accept', 'text/event-stream, application/json')

  const response = await fetch('/api/keys/bulk-actions', {
    method: 'POST',
    headers,
    body: JSON.stringify({
      action: 'sync_usage',
      key_ids: keyIds,
    }),
  })

  const contentType = response.headers.get('Content-Type') ?? ''
  if (!response.ok && !contentType.includes('text/event-stream')) {
    let message = ''
    try {
      const data = await response.json()
      message = (data?.detail as string) ?? (data?.error as string) ?? ''
    } catch {
      message = await response.text().catch(() => '')
    }
    const statusPart = ` (HTTP ${response.status})`
    throw new Error((message ? `${message}` : 'Failed to sync key usage') + statusPart)
  }

  if (!contentType.includes('text/event-stream')) {
    const payload = (await response.json()) as ApiKeyBulkActionResponse
    onEvent?.({ type: 'complete', payload })
    return payload
  }

  if (!response.ok) {
    const message = await response.text().catch(() => response.statusText)
    throw extractErrorMessage(response, message)
  }

  const reader = response.body?.getReader()
  if (!reader) {
    throw new Error('Bulk sync progress stream body is unavailable')
  }

  const decoder = new TextDecoder()
  let buffer = ''
  let completePayload: ApiKeyBulkActionResponse | null = null

  while (true) {
    const { done, value } = await reader.read()
    buffer += decoder.decode(value ?? new Uint8Array(), { stream: !done })

    let boundaryIndex = buffer.search(/\r?\n\r?\n/)
    while (boundaryIndex >= 0) {
      const chunk = buffer.slice(0, boundaryIndex)
      buffer = buffer.slice(boundaryIndex + (buffer[boundaryIndex] === '\r' ? 4 : 2))
      const event = parseJsonSseChunk<ApiKeyBulkSyncProgressEvent>(chunk)
      if (event) {
        onEvent?.(event)
        if (event.type === 'complete') {
          completePayload = event.payload
        }
        if (event.type === 'error') {
          throw new Error(event.detail || event.message || 'Bulk sync progress stream failed')
        }
      }
      boundaryIndex = buffer.search(/\r?\n\r?\n/)
    }

    if (done) {
      const trailingEvent = parseJsonSseChunk<ApiKeyBulkSyncProgressEvent>(buffer)
      if (trailingEvent) {
        onEvent?.(trailingEvent)
        if (trailingEvent.type === 'complete') {
          completePayload = trailingEvent.payload
        }
        if (trailingEvent.type === 'error') {
          throw new Error(
            trailingEvent.detail || trailingEvent.message || 'Bulk sync progress stream failed',
          )
        }
      }
      break
    }
  }

  if (completePayload == null) {
    throw new Error('Bulk sync progress stream ended before completion')
  }

  return completePayload
}

export interface JobLogView {
  id: number
  job_type: string
  key_id: string | null
  key_group: string | null
  status: string
  attempt: number
  message: string | null
  started_at: number
  finished_at: number | null
}

interface ServerJobLogView {
  id: number
  jobType: string
  keyId: string | null
  keyGroup: string | null
  status: string
  attempt: number
  message: string | null
  startedAt: number
  finishedAt: number | null
}

export type JobGroup = 'all' | 'quota' | 'usage' | 'logs' | 'geo' | 'linuxdo'

export interface JobGroupCounts {
  all: number
  quota: number
  usage: number
  logs: number
  geo: number
  linuxdo: number
}

interface ServerJobGroupCounts {
  all: number
  quota: number
  usage: number
  logs: number
  geo: number
  linuxdo: number
}

export interface PaginatedJobs extends Paginated<JobLogView> {
  groupCounts: JobGroupCounts
}

export interface Profile {
  displayName: string | null
  isAdmin: boolean
  forwardAuthEnabled: boolean
  builtinAuthEnabled: boolean
  allowRegistration: boolean
  userLoggedIn?: boolean
  userProvider?: 'linuxdo' | null
  userDisplayName?: string | null
  userAvatarUrl?: string | null
}

export function fetchProfile(signal?: AbortSignal): Promise<Profile> {
  return requestJson('/api/profile', { signal })
}

export interface AdminRegistrationSettings {
  allowRegistration: boolean
}

export function fetchAdminRegistrationSettings(
  signal?: AbortSignal,
): Promise<AdminRegistrationSettings> {
  return requestJson('/api/admin/registration', { signal })
}

export function updateAdminRegistrationSettings(
  allowRegistration: boolean,
): Promise<AdminRegistrationSettings> {
  return requestJson('/api/admin/registration', {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ allowRegistration }),
  })
}

export interface AdminQuotaLimitSet {
  hourlyAnyLimit: number
  hourlyLimit: number
  dailyLimit: number
  monthlyLimit: number
  inheritsDefaults: boolean
}

export type RequestRateScope = 'user' | 'token'

export interface RequestRate {
  used: number
  limit: number
  windowMinutes: number
  scope: RequestRateScope
}

export interface AdminUserTag {
  id: string
  name: string
  displayName: string
  icon: string | null
  systemKey: string | null
  effectKind: string
  hourlyAnyDelta: number
  hourlyDelta: number
  dailyDelta: number
  monthlyDelta: number
  userCount: number
}

export interface AdminUserTagBinding {
  tagId: string
  name: string
  displayName: string
  icon: string | null
  systemKey: string | null
  effectKind: string
  hourlyAnyDelta: number
  hourlyDelta: number
  dailyDelta: number
  monthlyDelta: number
  source: string
}

export interface AdminUserQuotaBreakdownEntry {
  kind: string
  label: string
  tagId: string | null
  tagName: string | null
  source: string | null
  effectKind: string
  hourlyAnyDelta: number
  hourlyDelta: number
  dailyDelta: number
  monthlyDelta: number
}

export interface AdminUserSummary {
  userId: string
  displayName: string | null
  username: string | null
  active: boolean
  lastLoginAt: number | null
  tokenCount: number
  apiKeyCount: number
  tags: AdminUserTagBinding[]
  requestRate: RequestRate
  hourlyAnyUsed: number
  hourlyAnyLimit: number
  quotaHourlyUsed: number
  quotaHourlyLimit: number
  quotaDailyUsed: number
  quotaDailyLimit: number
  quotaMonthlyUsed: number
  quotaMonthlyLimit: number
  dailySuccess: number
  dailyFailure: number
  monthlySuccess: number
  monthlyFailure: number
  monthlyBrokenCount: number
  monthlyBrokenLimit: number
  lastActivity: number | null
}

export type AdminUsersSortField =
  | 'hourlyAnyUsed'
  | 'quotaHourlyUsed'
  | 'quotaDailyUsed'
  | 'quotaMonthlyUsed'
  | 'dailySuccessRate'
  | 'monthlySuccessRate'
  | 'monthlyBrokenCount'
  | 'lastActivity'
  | 'lastLoginAt'

export type AdminUnboundTokenUsageSortField =
  | 'hourlyAnyUsed'
  | 'quotaHourlyUsed'
  | 'quotaDailyUsed'
  | 'quotaMonthlyUsed'
  | 'monthlyBrokenCount'
  | 'dailySuccessRate'
  | 'monthlySuccessRate'
  | 'lastUsedAt'

export type SortDirection = 'asc' | 'desc'

export interface AdminUserTokenSummary {
  tokenId: string
  enabled: boolean
  note: string | null
  createdAt: number
  lastUsedAt: number | null
  totalRequests: number
  dailySuccess: number
  dailyFailure: number
  monthlySuccess: number
}

export interface AdminUnboundTokenUsageSummary {
  tokenId: string
  enabled: boolean
  note: string | null
  group: string | null
  requestRate: RequestRate
  hourlyAnyUsed: number
  hourlyAnyLimit: number
  quotaHourlyUsed: number
  quotaHourlyLimit: number
  quotaDailyUsed: number
  quotaDailyLimit: number
  quotaMonthlyUsed: number
  quotaMonthlyLimit: number
  dailySuccess: number
  dailyFailure: number
  monthlySuccess: number
  monthlyFailure: number
  monthlyBrokenCount: number | null
  monthlyBrokenLimit: number | null
  lastUsedAt: number | null
}

export interface AdminUserDetail extends AdminUserSummary {
  tokens: AdminUserTokenSummary[]
  quotaBase: AdminQuotaLimitSet
  effectiveQuota: AdminQuotaLimitSet
  quotaBreakdown: AdminUserQuotaBreakdownEntry[]
}

export type AdminUserUsageSeriesKey = 'rate5m' | 'quota1h' | 'quota24h' | 'quotaMonth'

export interface AdminUserUsageSeriesPoint {
  bucketStart: number
  displayBucketStart?: number | null
  value: number | null
  limitValue: number | null
}

export interface AdminUserUsageSeries {
  limit: number
  points: AdminUserUsageSeriesPoint[]
}

export interface UpdateUserQuotaPayload {
  hourlyAnyLimit?: number
  hourlyLimit: number
  dailyLimit: number
  monthlyLimit: number
}

export interface UpdateUserBrokenKeyLimitPayload {
  monthlyBrokenLimit: number
}

export interface MonthlyBrokenKeyRelatedUser {
  userId: string
  displayName: string | null
  username: string | null
}

export interface MonthlyBrokenKeyDetail {
  keyId: string
  currentStatus: string
  reasonCode: string | null
  reasonSummary: string | null
  latestBreakAt: number
  source: string
  breakerTokenId: string | null
  breakerUserId: string | null
  breakerUserDisplayName: string | null
  manualActorDisplayName: string | null
  relatedUsers: MonthlyBrokenKeyRelatedUser[]
}

export interface UpsertAdminUserTagPayload {
  name: string
  displayName: string
  icon: string | null
  effectKind: string
  hourlyAnyDelta: number
  hourlyDelta: number
  dailyDelta: number
  monthlyDelta: number
}

export interface UserTokenResponse {
  token: string
}

export function fetchUserToken(signal?: AbortSignal): Promise<UserTokenResponse> {
  return requestJson('/api/user/token', { signal })
}

export async function postUserLogout(signal?: AbortSignal): Promise<void> {
  const response = await fetch('/api/user/logout', {
    method: 'POST',
    signal,
  })
  if (response.status === 204 || response.status === 401) {
    return
  }
  if (!response.ok) {
    const message = await response.text().catch(() => response.statusText)
    const err = new Error(message || `Request failed with status ${response.status}`) as Error & {
      status?: number
    }
    err.status = response.status
    throw err
  }
}

export interface UserDashboard {
  requestRate: RequestRate
  hourlyAnyUsed: number
  hourlyAnyLimit: number
  quotaHourlyUsed: number
  quotaHourlyLimit: number
  quotaDailyUsed: number
  quotaDailyLimit: number
  quotaMonthlyUsed: number
  quotaMonthlyLimit: number
  dailySuccess: number
  dailyFailure: number
  monthlySuccess: number
  lastActivity: number | null
}

export interface UserTokenSummary {
  tokenId: string
  enabled: boolean
  note: string | null
  lastUsedAt: number | null
  requestRate: RequestRate
  hourlyAnyUsed: number
  hourlyAnyLimit: number
  quotaHourlyUsed: number
  quotaHourlyLimit: number
  quotaDailyUsed: number
  quotaDailyLimit: number
  quotaMonthlyUsed: number
  quotaMonthlyLimit: number
  dailySuccess: number
  dailyFailure: number
  monthlySuccess: number
}

export function fetchUserDashboard(todayWindow?: TodayWindowRange, signal?: AbortSignal): Promise<UserDashboard> {
  const params = new URLSearchParams()
  appendTodayWindowRange(params, todayWindow)
  const url = `/api/user/dashboard${params.toString() ? `?${params.toString()}` : ''}`
  return requestJson(url, { signal })
}

export function fetchUserTokens(todayWindow?: TodayWindowRange, signal?: AbortSignal): Promise<UserTokenSummary[]> {
  const params = new URLSearchParams()
  appendTodayWindowRange(params, todayWindow)
  const url = `/api/user/tokens${params.toString() ? `?${params.toString()}` : ''}`
  return requestJson(url, { signal })
}

export function fetchUserTokenDetail(
  id: string,
  todayWindow?: TodayWindowRange,
  signal?: AbortSignal,
): Promise<UserTokenSummary> {
  const encoded = encodeURIComponent(id)
  const params = new URLSearchParams()
  appendTodayWindowRange(params, todayWindow)
  const url = `/api/user/tokens/${encoded}${params.toString() ? `?${params.toString()}` : ''}`
  return requestJson(url, { signal })
}

export function fetchUserTokenSecret(id: string, signal?: AbortSignal): Promise<UserTokenResponse> {
  const encoded = encodeURIComponent(id)
  return requestJson(`/api/user/tokens/${encoded}/secret`, { signal })
}

export function buildUserTokenEventsUrl(id: string, todayWindow?: TodayWindowRange): string {
  const encoded = encodeURIComponent(id)
  const params = new URLSearchParams()
  appendTodayWindowRange(params, todayWindow)
  const query = params.toString()
  return `/api/user/tokens/${encoded}/events${query ? `?${query}` : ''}`
}

export async function fetchUserTokenLogs(id: string, limit = 20, signal?: AbortSignal): Promise<PublicTokenLog[]> {
  const encoded = encodeURIComponent(id)
  const params = new URLSearchParams({ limit: String(limit) })
  const url = `/api/user/tokens/${encoded}/logs?${params.toString()}`
  const data = await requestJson<ServerPublicTokenLog[]>(url, { signal })
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

export function parseUserTokenEventSnapshot(raw: string): UserTokenEventSnapshot {
  const snapshot = JSON.parse(raw) as ServerUserTokenEventSnapshot
  return {
    token: snapshot.token,
    logs: snapshot.logs.map((it) => ({
      id: it.id,
      method: it.method,
      path: it.path,
      query: it.query,
      http_status: it.httpStatus,
      mcp_status: it.mcpStatus,
      result_status: it.resultStatus,
      error_message: it.errorMessage,
      created_at: it.createdAt,
    })),
  }
}

export interface ProbeMcpResponse {
  result?: unknown
  error?: unknown
  [key: string]: unknown
}

export interface ProbeMcpRequestContext {
  protocolVersion?: string | null
  sessionId?: string | null
  requestId?: string
  signal?: AbortSignal
}

export interface ProbeMcpInitializeContext extends ProbeMcpRequestContext {
  clientVersion?: string | null
}

export interface ProbeMcpEnvelopeResult {
  payload: ProbeMcpResponse
  negotiatedProtocolVersion: string | null
  sessionId: string | null
  status: number
}

export interface ProbeMcpNotificationResult {
  sessionId: string | null
  status: number
}

function buildMcpProbeHeaders(context?: ProbeMcpRequestContext): HeadersInit | undefined {
  const headers = new Headers()
  if (context?.protocolVersion) {
    headers.set('Mcp-Protocol-Version', context.protocolVersion)
  }
  if (context?.sessionId) {
    headers.set('Mcp-Session-Id', context.sessionId)
  }
  return Array.from(headers.keys()).length > 0 ? headers : undefined
}

function resolveNegotiatedProtocolVersion(
  payload: ProbeMcpResponse,
  fallback?: string | null,
): string | null {
  const result = payload.result && typeof payload.result === 'object'
    ? payload.result as Record<string, unknown>
    : null
  const protocolVersion = result?.protocolVersion
  return typeof protocolVersion === 'string' && protocolVersion.trim().length > 0
    ? protocolVersion
    : fallback ?? null
}

function toProbeMcpEnvelopeResult(
  payload: ProbeMcpResponse,
  status: number,
  sessionId: string | null,
  fallbackProtocolVersion?: string | null,
): ProbeMcpEnvelopeResult {
  return {
    payload,
    negotiatedProtocolVersion: resolveNegotiatedProtocolVersion(payload, fallbackProtocolVersion),
    sessionId,
    status,
  }
}

export async function probeMcpInitialize(
  token: string,
  context: ProbeMcpInitializeContext,
): Promise<ProbeMcpEnvelopeResult> {
  const response = await requestMcpProbeEnvelopeWithToken<ProbeMcpResponse>('/mcp', token, {
    method: 'POST',
    signal: context.signal,
    headers: buildMcpProbeHeaders(context),
    body: JSON.stringify({
      jsonrpc: '2.0',
      id: context.requestId,
      method: 'initialize',
      params: {
        protocolVersion: context.protocolVersion ?? '2025-03-26',
        capabilities: {},
        clientInfo: {
          name: 'Tavily Hikari UserConsole Probe',
          version: context.clientVersion?.trim() || 'dev',
        },
      },
    }),
  })

  return toProbeMcpEnvelopeResult(
    response.payload,
    response.metadata.status,
    response.metadata.sessionId,
    context.protocolVersion ?? '2025-03-26',
  )
}

export async function probeMcpInitialized(
  token: string,
  context: ProbeMcpRequestContext,
): Promise<ProbeMcpNotificationResult> {
  const response = await requestMcpProbeNotificationWithToken<ProbeMcpResponse>('/mcp', token, {
    method: 'POST',
    signal: context.signal,
    headers: buildMcpProbeHeaders(context),
    body: JSON.stringify({
      jsonrpc: '2.0',
      method: 'notifications/initialized',
    }),
  })

  const error = getProbeEnvelopeError(response.payload)
  if (error) {
    throw new Error(error)
  }

  return {
    sessionId: response.metadata.sessionId,
    status: response.metadata.status,
  }
}

export async function probeMcpPing(
  token: string,
  context: ProbeMcpRequestContext,
): Promise<ProbeMcpEnvelopeResult> {
  const response = await requestMcpProbeEnvelopeWithToken<ProbeMcpResponse>('/mcp', token, {
    method: 'POST',
    signal: context.signal,
    headers: buildMcpProbeHeaders(context),
    body: JSON.stringify({
      jsonrpc: '2.0',
      id: context.requestId,
      method: 'ping',
    }),
  })

  return toProbeMcpEnvelopeResult(
    response.payload,
    response.metadata.status,
    response.metadata.sessionId,
    context.protocolVersion,
  )
}

export async function probeMcpToolsList(
  token: string,
  context: ProbeMcpRequestContext,
): Promise<ProbeMcpEnvelopeResult> {
  const response = await requestMcpProbeEnvelopeWithToken<ProbeMcpResponse>('/mcp', token, {
    method: 'POST',
    signal: context.signal,
    headers: buildMcpProbeHeaders(context),
    body: JSON.stringify({
      jsonrpc: '2.0',
      id: context.requestId,
      method: 'tools/list',
    }),
  })

  return toProbeMcpEnvelopeResult(
    response.payload,
    response.metadata.status,
    response.metadata.sessionId,
    context.protocolVersion,
  )
}

export async function probeMcpToolsCall(
  token: string,
  toolName: string,
  argumentsPayload: unknown,
  context: ProbeMcpRequestContext,
): Promise<ProbeMcpEnvelopeResult> {
  const response = await requestMcpProbeEnvelopeWithToken<ProbeMcpResponse>('/mcp', token, {
    method: 'POST',
    signal: context.signal,
    headers: buildMcpProbeHeaders(context),
    body: JSON.stringify({
      jsonrpc: '2.0',
      id: context.requestId,
      method: 'tools/call',
      params: {
        name: toolName,
        arguments: argumentsPayload,
      },
    }),
  })

  return toProbeMcpEnvelopeResult(
    response.payload,
    response.metadata.status,
    response.metadata.sessionId,
    context.protocolVersion,
  )
}

export interface TavilyResearchCreateResponse {
  request_id?: string
  requestId?: string
  status?: string
  [key: string]: unknown
}

export interface TavilyResearchResultResponse {
  request_id?: string
  requestId?: string
  status?: string
  [key: string]: unknown
}

export function probeApiTavilySearch(
  token: string,
  payload: Record<string, unknown>,
  signal?: AbortSignal,
): Promise<Record<string, unknown>> {
  return requestJsonWithToken('/api/tavily/search', token, {
    method: 'POST',
    signal,
    body: JSON.stringify(payload),
  })
}

export function probeApiTavilyExtract(
  token: string,
  payload: Record<string, unknown>,
  signal?: AbortSignal,
): Promise<Record<string, unknown>> {
  return requestJsonWithToken('/api/tavily/extract', token, {
    method: 'POST',
    signal,
    body: JSON.stringify(payload),
  })
}

export function probeApiTavilyCrawl(
  token: string,
  payload: Record<string, unknown>,
  signal?: AbortSignal,
): Promise<Record<string, unknown>> {
  return requestJsonWithToken('/api/tavily/crawl', token, {
    method: 'POST',
    signal,
    body: JSON.stringify(payload),
  })
}

export function probeApiTavilyMap(
  token: string,
  payload: Record<string, unknown>,
  signal?: AbortSignal,
): Promise<Record<string, unknown>> {
  return requestJsonWithToken('/api/tavily/map', token, {
    method: 'POST',
    signal,
    body: JSON.stringify(payload),
  })
}

export function probeApiTavilyResearch(
  token: string,
  payload: Record<string, unknown>,
  signal?: AbortSignal,
): Promise<TavilyResearchCreateResponse> {
  return requestJsonWithToken('/api/tavily/research', token, {
    method: 'POST',
    signal,
    body: JSON.stringify(payload),
  })
}

export function probeApiTavilyResearchResult(
  token: string,
  requestId: string,
  signal?: AbortSignal,
): Promise<TavilyResearchResultResponse> {
  return requestJsonWithToken(`/api/tavily/research/${encodeURIComponent(requestId)}`, token, {
    method: 'GET',
    signal,
  })
}

export interface CreateKeyResponse {
  id: string
}

export async function addApiKey(apiKey: string, group?: string): Promise<CreateKeyResponse> {
  const trimmedGroup = group?.trim()
  return await requestJson('/api/keys', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      api_key: apiKey,
      group: trimmedGroup && trimmedGroup.length > 0 ? trimmedGroup : undefined,
    }),
  })
}

export interface AddApiKeysBatchSummary {
  input_lines: number
  valid_lines: number
  unique_in_input: number
  created: number
  undeleted: number
  existed: number
  duplicate_in_input: number
  failed: number
}

export interface AddApiKeysBatchResult {
  api_key: string
  status: string
  id?: string
  error?: string
  marked_exhausted?: boolean
}

export interface AddApiKeysBatchResponse {
  summary: AddApiKeysBatchSummary
  results: AddApiKeysBatchResult[]
}

export interface AddApiKeysBatchItem {
  api_key: string
  registration_ip?: string | null
  assigned_proxy_key?: string | null
}

export async function addApiKeysBatch(
  items: AddApiKeysBatchItem[],
  group?: string,
  exhaustedApiKeys?: string[],
): Promise<AddApiKeysBatchResponse> {
  const trimmedGroup = group?.trim()
  return await requestJson('/api/keys/batch', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      items,
      group: trimmedGroup && trimmedGroup.length > 0 ? trimmedGroup : undefined,
      exhausted_api_keys: exhaustedApiKeys && exhaustedApiKeys.length > 0 ? exhaustedApiKeys : undefined,
    }),
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

export async function clearApiKeyQuarantine(id: string): Promise<void> {
  const encoded = encodeURIComponent(id)
  const res = await fetch(`/api/keys/${encoded}/quarantine`, {
    method: 'DELETE',
  })
  if (!res.ok) {
    throw new Error(`Failed to clear key quarantine: ${res.status}`)
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

export interface StickyUserIdentity {
  userId: string
  displayName: string | null
  username: string | null
  active: boolean
  lastLoginAt: number | null
  tokenCount: number
}

export interface StickyCreditsWindow {
  successCredits: number
  failureCredits: number
}

export interface StickyUserDailyBucket {
  bucketStart: number
  bucketEnd: number
  successCredits: number
  failureCredits: number
}

export interface StickyUserRow {
  user: StickyUserIdentity
  lastSuccessAt: number
  windows: {
    yesterday: StickyCreditsWindow
    today: StickyCreditsWindow
    month: StickyCreditsWindow
  }
  dailyBuckets: StickyUserDailyBucket[]
}

export interface StickyUsersResponse extends Paginated<StickyUserRow> {}

export interface StickyNode extends ForwardProxyStatsNode {
  role: 'primary' | 'secondary'
}

export interface StickyNodesResponse {
  rangeStart: string
  rangeEnd: string
  bucketSeconds: number
  nodes: StickyNode[]
}

export function fetchKeyMetrics(id: string, period?: 'day' | 'week' | 'month', since?: number, signal?: AbortSignal): Promise<KeySummary> {
  const params = new URLSearchParams()
  if (period) params.set('period', period)
  if (since != null) params.set('since', String(since))
  const encoded = encodeURIComponent(id)
  return requestJson(`/api/keys/${encoded}/metrics?${params.toString()}`, { signal })
}

// ---- Key validation (admin only) ----
export interface ValidateKeysSummary {
  input_lines: number
  valid_lines: number
  unique_in_input: number
  duplicate_in_input: number
  already_exists: number
  ok: number
  exhausted: number
  invalid: number
  error: number
}

export type ValidateAssignedProxyMatchKind = 'registration_ip' | 'same_region' | 'other'

export interface ValidateKeyResult {
  api_key: string
  status: string
  registration_ip?: string | null
  registration_region?: string | null
  assigned_proxy_key?: string | null
  assigned_proxy_label?: string | null
  assigned_proxy_match_kind?: ValidateAssignedProxyMatchKind | null
  quota_limit?: number
  quota_remaining?: number
  detail?: string
}

export interface ValidateKeyInput {
  api_key: string
  registration_ip?: string | null
}

export interface ValidateKeysResponse {
  summary: ValidateKeysSummary
  results: ValidateKeyResult[]
}

export async function validateApiKeys(items: ValidateKeyInput[], signal?: AbortSignal): Promise<ValidateKeysResponse> {
  return await requestJson('/api/keys/validate', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    signal,
    body: JSON.stringify({ items }),
  })
}

export function fetchKeyLogs(id: string, limit = 50, since?: number, signal?: AbortSignal): Promise<RequestLog[]> {
  const params = new URLSearchParams({ limit: String(limit) })
  if (since != null) params.set('since', String(since))
  const encoded = encodeURIComponent(id)
  return requestJson(`/api/keys/${encoded}/logs?${params.toString()}`, { signal })
}

export function fetchKeyLogsPage(
  id: string,
  query: RequestLogsPageQuery = {},
  signal?: AbortSignal,
): Promise<RequestLogsPage> {
  const params = new URLSearchParams({
    page: String(query.page ?? 1),
    per_page: String(query.perPage ?? 20),
  })
  appendRequestLogsPageFilters(params, query)
  const encoded = encodeURIComponent(id)
  return requestJson<ServerRequestLogsPage>(`/api/keys/${encoded}/logs/page?${params.toString()}`, { signal }).then(
    normalizeRequestLogsPage,
  )
}

export function fetchKeyLogsList(
  id: string,
  query: RequestLogsListQuery = {},
  signal?: AbortSignal,
): Promise<RequestLogsListPage> {
  const params = new URLSearchParams()
  appendRequestLogsListParams(params, query)
  const encoded = encodeURIComponent(id)
  return requestJson<ServerRequestLogsListPage>(`/api/keys/${encoded}/logs/list?${params.toString()}`, { signal }).then(
    normalizeRequestLogsListPage,
  )
}

export function fetchKeyLogsCatalog(
  id: string,
  options: RequestLogsCatalogQuery = {},
  signal?: AbortSignal,
): Promise<RequestLogsCatalog> {
  const params = new URLSearchParams()
  appendRequestLogsCatalogParams(params, options)
  const encoded = encodeURIComponent(id)
  const suffix = params.toString() ? `?${params.toString()}` : ''
  return requestJson<ServerRequestLogsCatalog>(`/api/keys/${encoded}/logs/catalog${suffix}`, { signal }).then(
    normalizeRequestLogsCatalog,
  )
}

export function fetchKeyLogDetails(id: string, logId: number, signal?: AbortSignal): Promise<RequestLogBodies> {
  const encoded = encodeURIComponent(id)
  return requestJson(`/api/keys/${encoded}/logs/${encodeURIComponent(String(logId))}/details`, { signal })
}

export function fetchKeyStickyUsers(
  id: string,
  page = 1,
  perPage = 20,
  signal?: AbortSignal,
): Promise<StickyUsersResponse> {
  const params = new URLSearchParams({
    page: String(page),
    per_page: String(perPage),
  })
  const encoded = encodeURIComponent(id)
  return requestJson(`/api/keys/${encoded}/sticky-users?${params.toString()}`, { signal })
}

export function fetchKeyStickyNodes(id: string, signal?: AbortSignal): Promise<StickyNodesResponse> {
  const encoded = encodeURIComponent(id)
  return requestJson(`/api/keys/${encoded}/sticky-nodes`, { signal })
}

// Tokens API
export interface Paginated<T> {
  items: T[]
  total: number
  page: number
  perPage: number
}

export type LogResultFilter = 'success' | 'error' | 'neutral' | 'quota_exhausted'
export type LogOperationalClass =
  | 'success'
  | 'neutral'
  | 'client_error'
  | 'upstream_error'
  | 'system_error'
  | 'quota_exhausted'

export function fetchRequestLogsPage(
  query: RequestLogsPageQuery = {},
  signal?: AbortSignal,
): Promise<RequestLogsPage> {
  const params = new URLSearchParams({
    page: String(query.page ?? 1),
    per_page: String(query.perPage ?? 20),
  })
  appendRequestLogsPageFilters(params, query)
  return requestJson<ServerRequestLogsPage>(`/api/logs?${params.toString()}`, { signal }).then(
    normalizeRequestLogsPage,
  )
}

export function fetchRequestLogsList(
  query: RequestLogsListQuery = {},
  signal?: AbortSignal,
): Promise<RequestLogsListPage> {
  const params = new URLSearchParams()
  appendRequestLogsListParams(params, query)
  return requestJson<ServerRequestLogsListPage>(`/api/logs/list?${params.toString()}`, { signal }).then(
    normalizeRequestLogsListPage,
  )
}

export function fetchRequestLogsCatalog(
  query: RequestLogsCatalogQuery = {},
  signal?: AbortSignal,
): Promise<RequestLogsCatalog> {
  const params = new URLSearchParams()
  appendRequestLogsCatalogParams(params, query)
  const suffix = params.toString() ? `?${params.toString()}` : ''
  return requestJson<ServerRequestLogsCatalog>(`/api/logs/catalog${suffix}`, { signal }).then(
    normalizeRequestLogsCatalog,
  )
}

export function fetchAlertCatalog(signal?: AbortSignal): Promise<AlertCatalog> {
  return requestJson<ServerAlertCatalog>('/api/alerts/catalog', { signal }).then(normalizeAlertCatalog)
}

export function fetchAlertEvents(
  query: AlertsQuery = {},
  signal?: AbortSignal,
): Promise<AlertsPage<AlertEvent>> {
  const params = new URLSearchParams()
  appendAlertsQueryParams(params, query)
  return requestJson<ServerAlertsPage<ServerAlertEvent>>(`/api/alerts/events?${params.toString()}`, {
    signal,
  }).then((value) => normalizeAlertsPage(value, normalizeAlertEvent))
}

export function fetchAlertGroups(
  query: AlertsQuery = {},
  signal?: AbortSignal,
): Promise<AlertsPage<AlertGroup>> {
  const params = new URLSearchParams()
  appendAlertsQueryParams(params, query)
  return requestJson<ServerAlertsPage<ServerAlertGroup>>(`/api/alerts/groups?${params.toString()}`, {
    signal,
  }).then((value) => normalizeAlertsPage(value, normalizeAlertGroup))
}

export function fetchRequestLogs(
  page = 1,
  perPage = 20,
  result?: LogResultFilter,
  signal?: AbortSignal,
  operationalClass?: LogOperationalClass | 'all',
): Promise<RequestLogsPage> {
  return fetchRequestLogsPage({ page, perPage, result, operationalClass, includeBodies: true }, signal)
}

export function fetchRequestLogDetails(logId: number, signal?: AbortSignal): Promise<RequestLogBodies> {
  return requestJson(`/api/logs/${encodeURIComponent(String(logId))}/details`, { signal })
}

export function fetchJobs(
  page = 1,
  perPage = 10,
  group: JobGroup = 'all',
  signal?: AbortSignal,
): Promise<PaginatedJobs> {
  const params = new URLSearchParams({
    page: String(page),
    per_page: String(perPage),
  })
  if (group !== 'all') {
    params.set('group', group)
  }
  return requestJson<Paginated<ServerJobLogView> & { groupCounts: ServerJobGroupCounts }>(
    `/api/jobs?${params.toString()}`,
    { signal },
  ).then((data) => ({
    total: data.total,
    page: data.page,
    perPage: data.perPage,
    groupCounts: {
      all: data.groupCounts.all,
      quota: data.groupCounts.quota,
      usage: data.groupCounts.usage,
      logs: data.groupCounts.logs,
      geo: data.groupCounts.geo,
      linuxdo: data.groupCounts.linuxdo,
    },
    items: data.items.map((item) => ({
      id: item.id,
      job_type: item.jobType,
      key_id: item.keyId,
      key_group: item.keyGroup,
      status: item.status,
      attempt: item.attempt,
      message: item.message,
      started_at: item.startedAt,
      finished_at: item.finishedAt,
    })),
  }))
}

export function fetchAdminUsers(
  page = 1,
  perPage = 20,
  query?: string,
  tagId?: string | null,
  sort?: AdminUsersSortField | null,
  order?: SortDirection | null,
  signal?: AbortSignal,
): Promise<Paginated<AdminUserSummary>> {
  const params = new URLSearchParams({
    page: String(page),
    per_page: String(perPage),
  })
  if (query && query.trim().length > 0) {
    params.set('q', query.trim())
  }
  if (tagId && tagId.trim().length > 0) {
    params.set('tagId', tagId.trim())
  }
  if (sort) {
    params.set('sort', sort)
    params.set('order', order ?? 'desc')
  }
  return requestJson(`/api/users?${params.toString()}`, { signal })
}

export function fetchAdminUnboundTokenUsage(
  page = 1,
  perPage = 20,
  query?: string,
  sort?: AdminUnboundTokenUsageSortField | null,
  order?: SortDirection | null,
  signal?: AbortSignal,
): Promise<Paginated<AdminUnboundTokenUsageSummary>> {
  const params = new URLSearchParams({
    page: String(page),
    per_page: String(perPage),
  })
  if (query && query.trim().length > 0) {
    params.set('q', query.trim())
  }
  if (sort) {
    params.set('sort', sort)
    params.set('order', order ?? 'desc')
  }
  return requestJson(`/api/tokens/unbound-usage?${params.toString()}`, { signal })
}

export function fetchAdminUserBrokenKeys(
  id: string,
  page = 1,
  perPage = 20,
  signal?: AbortSignal,
): Promise<Paginated<MonthlyBrokenKeyDetail>> {
  const encoded = encodeURIComponent(id)
  const params = new URLSearchParams({
    page: String(page),
    per_page: String(perPage),
  })
  return requestJson(`/api/users/${encoded}/broken-keys?${params.toString()}`, { signal })
}

export function fetchTokenBrokenKeys(
  id: string,
  page = 1,
  perPage = 20,
  signal?: AbortSignal,
): Promise<Paginated<MonthlyBrokenKeyDetail>> {
  const encoded = encodeURIComponent(id)
  const params = new URLSearchParams({
    page: String(page),
    per_page: String(perPage),
  })
  return requestJson(`/api/tokens/${encoded}/broken-keys?${params.toString()}`, { signal })
}

export function fetchAdminUserDetail(id: string, signal?: AbortSignal): Promise<AdminUserDetail> {
  const encoded = encodeURIComponent(id)
  return requestJson(`/api/users/${encoded}`, { signal })
}

export function fetchAdminUserUsageSeries(
  id: string,
  series: AdminUserUsageSeriesKey,
  signal?: AbortSignal,
): Promise<AdminUserUsageSeries> {
  const encoded = encodeURIComponent(id)
  const params = new URLSearchParams({ series })
  return requestJson(`/api/users/${encoded}/usage-series?${params.toString()}`, { signal })
}

export async function updateAdminUserQuota(id: string, payload: UpdateUserQuotaPayload): Promise<void> {
  const encoded = encodeURIComponent(id)
  await requestNoContent(`/api/users/${encoded}/quota`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  })
}

export async function updateAdminUserBrokenKeyLimit(
  id: string,
  payload: UpdateUserBrokenKeyLimitPayload,
): Promise<void> {
  const encoded = encodeURIComponent(id)
  await requestNoContent(`/api/users/${encoded}/broken-key-limit`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  })
}

export function fetchAdminUserTags(signal?: AbortSignal): Promise<AdminUserTag[]> {
  return requestJson<{ items: AdminUserTag[] }>('/api/user-tags', { signal }).then((response) => response.items)
}

export function createAdminUserTag(payload: UpsertAdminUserTagPayload): Promise<AdminUserTag> {
  return requestJson('/api/user-tags', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  })
}

export function updateAdminUserTag(id: string, payload: UpsertAdminUserTagPayload): Promise<AdminUserTag> {
  const encoded = encodeURIComponent(id)
  return requestJson(`/api/user-tags/${encoded}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  })
}

export async function deleteAdminUserTag(id: string): Promise<void> {
  const encoded = encodeURIComponent(id)
  await requestNoContent(`/api/user-tags/${encoded}`, { method: 'DELETE' })
}

export async function bindAdminUserTag(userId: string, tagId: string): Promise<void> {
  const encoded = encodeURIComponent(userId)
  await requestNoContent(`/api/users/${encoded}/tags`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ tagId }),
  })
}

export async function unbindAdminUserTag(userId: string, tagId: string): Promise<void> {
  const encodedUserId = encodeURIComponent(userId)
  const encodedTagId = encodeURIComponent(tagId)
  await requestNoContent(`/api/users/${encodedUserId}/tags/${encodedTagId}`, { method: 'DELETE' })
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

export function fetchTokenLogsPage(
  id: string,
  query: RequestLogsPageQuery = {},
  signal?: AbortSignal,
): Promise<RequestLogsPage> {
  const params = new URLSearchParams({
    page: String(query.page ?? 1),
    per_page: String(query.perPage ?? 20),
  })
  appendRequestLogsPageFilters(params, query)
  const encoded = encodeURIComponent(id)
  return requestJson<ServerRequestLogsPage>(`/api/tokens/${encoded}/logs/page?${params.toString()}`, { signal }).then(
    normalizeRequestLogsPage,
  )
}

export function fetchTokenLogsList(
  id: string,
  query: RequestLogsListQuery = {},
  signal?: AbortSignal,
): Promise<RequestLogsListPage> {
  const params = new URLSearchParams()
  appendRequestLogsListParams(params, query)
  const encoded = encodeURIComponent(id)
  return requestJson<ServerRequestLogsListPage>(`/api/tokens/${encoded}/logs/list?${params.toString()}`, {
    signal,
  }).then(normalizeRequestLogsListPage)
}

export function fetchTokenLogsCatalog(
  id: string,
  options: RequestLogsCatalogQuery = {},
  signal?: AbortSignal,
): Promise<RequestLogsCatalog> {
  const params = new URLSearchParams()
  appendRequestLogsCatalogParams(params, options)
  const encoded = encodeURIComponent(id)
  const suffix = params.toString() ? `?${params.toString()}` : ''
  return requestJson<ServerRequestLogsCatalog>(`/api/tokens/${encoded}/logs/catalog${suffix}`, { signal }).then(
    normalizeRequestLogsCatalog,
  )
}

export function fetchTokenLogDetails(id: string, logId: number, signal?: AbortSignal): Promise<RequestLogBodies> {
  const encoded = encodeURIComponent(id)
  return requestJson(`/api/tokens/${encoded}/logs/${encodeURIComponent(String(logId))}/details`, { signal })
}

export interface ForwardProxyWindowStats {
  attempts: number
  successCount?: number | null
  failureCount?: number | null
  successRate?: number | null
  avgLatencyMs?: number | null
}

export interface ForwardProxyNode {
  key: string
  source: string
  displayName: string
  endpointUrl: string | null
  resolvedIps: string[]
  resolvedRegions: string[]
  weight: number
  available: boolean
  lastError?: string | null
  penalized: boolean
  primaryAssignmentCount: number
  secondaryAssignmentCount: number
  stats: {
    oneMinute: ForwardProxyWindowStats
    fifteenMinutes: ForwardProxyWindowStats
    oneHour: ForwardProxyWindowStats
    oneDay: ForwardProxyWindowStats
    sevenDays: ForwardProxyWindowStats
  }
}

export interface ForwardProxySettings {
  proxyUrls: string[]
  subscriptionUrls: string[]
  subscriptionUpdateIntervalSecs: number
  insertDirect: boolean
  egressSocks5Enabled: boolean
  egressSocks5Url: string
  nodes: ForwardProxyNode[]
}

export interface SystemSettings {
  requestRateLimit: number
  mcpSessionAffinityKeyCount: number
  rebalanceMcpEnabled: boolean
  rebalanceMcpSessionPercent: number
  userBlockedKeyBaseLimit: number
}

export interface ForwardProxySettingsEnvelope {
  forwardProxy?: ForwardProxySettings | null
  systemSettings?: SystemSettings | null
}

export interface UpdateForwardProxySettingsPayload {
  proxyUrls: string[]
  subscriptionUrls: string[]
  subscriptionUpdateIntervalSecs: number
  insertDirect: boolean
  egressSocks5Enabled?: boolean
  egressSocks5Url?: string
  skipBootstrapProbe?: boolean
}

export interface UpdateSystemSettingsPayload {
  requestRateLimit: number
  mcpSessionAffinityKeyCount: number
  rebalanceMcpEnabled: boolean
  rebalanceMcpSessionPercent: number
  userBlockedKeyBaseLimit: number
}

export type ForwardProxyValidationKind = 'proxyUrl' | 'subscriptionUrl'

export interface ForwardProxyValidationRequest {
  kind: ForwardProxyValidationKind
  value: string
}

export interface ForwardProxyValidationNode {
  displayName: string
  protocol: string
  ok: boolean
  latencyMs?: number | null
  ip?: string | null
  location?: string | null
  message?: string | null
}

export interface ForwardProxyValidationResponse {
  ok: boolean
  message: string
  normalizedValue?: string | null
  discoveredNodes?: number | null
  latencyMs?: number | null
  errorCode?: string | null
  nodes?: ForwardProxyValidationNode[]
}

export interface ForwardProxyActivityBucket {
  bucketStart: string
  bucketEnd: string
  successCount: number
  failureCount: number
}

export interface ForwardProxyWeightBucket {
  bucketStart: string
  bucketEnd: string
  sampleCount: number
  minWeight: number
  maxWeight: number
  avgWeight: number
  lastWeight: number
}

export interface ForwardProxyStatsNode extends ForwardProxyNode {
  last24h: ForwardProxyActivityBucket[]
  weight24h: ForwardProxyWeightBucket[]
}

export interface ForwardProxyStatsResponse {
  rangeStart: string
  rangeEnd: string
  bucketSeconds: number
  nodes: ForwardProxyStatsNode[]
}

function createEmptyForwardProxySettings(): ForwardProxySettings {
  return {
    proxyUrls: [],
    subscriptionUrls: [],
    subscriptionUpdateIntervalSecs: 3600,
    insertDirect: true,
    egressSocks5Enabled: false,
    egressSocks5Url: '',
    nodes: [],
  }
}

function createEmptySystemSettings(): SystemSettings {
  return {
    requestRateLimit: 100,
    mcpSessionAffinityKeyCount: 5,
    rebalanceMcpEnabled: false,
    rebalanceMcpSessionPercent: 100,
    userBlockedKeyBaseLimit: 5,
  }
}

export async function fetchForwardProxySettings(signal?: AbortSignal): Promise<ForwardProxySettings> {
  const response = await requestJson<ForwardProxySettingsEnvelope>('/api/settings', { signal })
  return response.forwardProxy ?? createEmptyForwardProxySettings()
}

export async function fetchSystemSettings(signal?: AbortSignal): Promise<SystemSettings> {
  const response = await requestJson<ForwardProxySettingsEnvelope>('/api/settings', { signal })
  return response.systemSettings ?? createEmptySystemSettings()
}

export function updateForwardProxySettings(
  payload: UpdateForwardProxySettingsPayload,
): Promise<ForwardProxySettings> {
  return requestJson('/api/settings/forward-proxy', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  })
}

export function updateForwardProxySettingsWithProgress(
  payload: UpdateForwardProxySettingsPayload,
  onEvent?: (event: ForwardProxyProgressEvent) => void,
): Promise<ForwardProxySettings> {
  return requestForwardProxyProgress<ForwardProxySettings>(
    '/api/settings/forward-proxy',
    {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
    },
    'save',
    onEvent,
  )
}

export function updateSystemSettings(
  payload: UpdateSystemSettingsPayload,
): Promise<SystemSettings> {
  return requestJson('/api/settings/system', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  })
}

export function validateForwardProxyCandidate(
  payload: ForwardProxyValidationRequest,
): Promise<ForwardProxyValidationResponse> {
  return requestJson('/api/settings/forward-proxy/validate', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  })
}

export function validateForwardProxyCandidateWithProgress(
  payload: ForwardProxyValidationRequest,
  onEvent?: (event: ForwardProxyProgressEvent) => void,
  signal?: AbortSignal,
): Promise<ForwardProxyValidationResponse> {
  return requestForwardProxyProgress<ForwardProxyValidationResponse>(
    '/api/settings/forward-proxy/validate',
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
      signal,
    },
    'validate',
    onEvent,
  )
}

export function revalidateForwardProxyWithProgress(
  onEvent?: (event: ForwardProxyProgressEvent) => void,
): Promise<ForwardProxySettings> {
  return requestForwardProxyProgress<ForwardProxySettings>(
    '/api/settings/forward-proxy/revalidate',
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({}),
    },
    'revalidate',
    onEvent,
  )
}

export function fetchForwardProxyStats(signal?: AbortSignal): Promise<ForwardProxyStatsResponse> {
  return requestJson('/api/stats/forward-proxy', { signal })
}

export function fetchForwardProxyDashboardSummary(
  signal?: AbortSignal,
): Promise<ForwardProxyDashboardSummaryResponse> {
  return requestJson('/api/stats/forward-proxy/summary', { signal })
}
