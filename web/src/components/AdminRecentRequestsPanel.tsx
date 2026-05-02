import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from 'react'

import type { QueryLoadState } from '../admin/queryLoadState'
import type { LogFacetOption, RequestLog, RequestLogBodies } from '../api'
import type { AdminTranslations } from '../i18n'
import { Icon } from '../lib/icons'
import {
  buildRequestKindQuickFilterSelection,
  buildVisibleRequestKindOptions,
  hasActiveRequestKindQuickFilters,
  mergeRequestKindCatalog,
  summarizeRequestKindQuickFilters,
  summarizeSelectedRequestKinds,
  type TokenLogRequestKindOption,
  type TokenLogRequestKindQuickBilling,
  type TokenLogRequestKindQuickProtocol,
} from '../tokenLogRequestKinds'
import RequestKindBadge from './RequestKindBadge'
import AdminLoadingRegion from './AdminLoadingRegion'
import AdminTablePagination from './AdminTablePagination'
import AdminTableShell from './AdminTableShell'
import SearchableFacetSelect from './SearchableFacetSelect'
import { StatusBadge, type StatusTone } from './StatusBadge'
import { Button } from './ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from './ui/dropdown-menu'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectSeparator,
  SelectTrigger,
} from './ui/select'
import SegmentedTabs from './ui/SegmentedTabs'
import { TableBody, TableCell, TableHead, TableHeader, TableRow } from './ui/table'
import { Tooltip, TooltipContent, TooltipTrigger } from './ui/tooltip'

type Language = 'en' | 'zh'
type RecentRequestsVariant = 'admin' | 'token'
type RecentRequestsOutcomeFilterKind = 'result' | 'keyEffect' | 'bindingEffect' | 'selectionEffect'

export interface RecentRequestsOutcomeFilter {
  kind: RecentRequestsOutcomeFilterKind
  value: string
}

export interface AdminRecentRequestsPanelProps {
  variant: RecentRequestsVariant
  language: Language
  strings: AdminTranslations
  title: string
  description: string
  emptyLabel: string
  loadState: QueryLoadState
  loadingLabel: string
  errorLabel?: string | null
  logs: RequestLog[]
  requestKindOptions: TokenLogRequestKindOption[]
  requestKindQuickBilling: TokenLogRequestKindQuickBilling
  requestKindQuickProtocol: TokenLogRequestKindQuickProtocol
  selectedRequestKinds: string[]
  onRequestKindQuickFiltersChange: (
    billing: TokenLogRequestKindQuickBilling,
    protocol: TokenLogRequestKindQuickProtocol,
  ) => void
  onToggleRequestKind: (key: string) => void
  onClearRequestKinds: () => void
  outcomeFilter: RecentRequestsOutcomeFilter | null
  resultOptions: LogFacetOption[]
  keyEffectOptions: LogFacetOption[]
  bindingEffectOptions?: LogFacetOption[]
  selectionEffectOptions?: LogFacetOption[]
  onOutcomeFilterChange: (value: RecentRequestsOutcomeFilter | null) => void
  keyOptions?: LogFacetOption[]
  selectedKeyId?: string | null
  onKeyFilterChange?: (value: string | null) => void
  showKeyColumn: boolean
  showTokenColumn: boolean
  perPage: number
  hasOlder: boolean
  hasNewer: boolean
  paginationSummary?: string
  paginationDisabled?: boolean
  onOlderPage: () => void | Promise<void>
  onNewerPage: () => void | Promise<void>
  onPerPageChange: (value: number) => void | Promise<void>
  formatTime: (ts: number | null) => string
  formatTimeDetail?: (ts: number | null) => string
  onOpenKey?: (id: string) => void
  onOpenToken?: (id: string) => void
  loadLogBodies: (log: RequestLog, signal: AbortSignal) => Promise<RequestLogBodies>
}

type LogBodiesLoadState =
  | { status: 'loading' }
  | { status: 'ready'; value: RequestLogBodies }
  | { status: 'error'; message: string }

const requestKindBillingQuickFilterOptions = [
  { value: 'all', label: 'Any' },
  { value: 'billable', label: 'Paid' },
  { value: 'non_billable', label: 'Free' },
] as const

const requestKindProtocolQuickFilterOptions = [
  { value: 'all', label: 'Any' },
  { value: 'mcp', label: 'MCP' },
  { value: 'api', label: 'API' },
] as const

const recentRequestsAllFilterValue = '__all__'
const recentRequestsCompactAllLabel = 'All'

function statusTone(status: string): StatusTone {
  const normalized = status.trim().toLowerCase()
  if (normalized === 'success') return 'success'
  if (normalized === 'quota_exhausted') return 'warning'
  if (normalized === 'error') return 'error'
  return 'neutral'
}

function statusLabel(status: string, strings: AdminTranslations): string {
  const normalized = status.trim().toLowerCase()
  if (normalized === 'success') return strings.statuses.success
  if (normalized === 'error') return strings.statuses.error
  if (normalized === 'neutral') return strings.statuses.neutral
  if (normalized === 'quota_exhausted') return strings.statuses.quota_exhausted
  return status || strings.logs.errors.none
}

function keyEffectTone(code: string | null | undefined): StatusTone {
  switch ((code ?? '').trim()) {
    case 'quarantined':
      return 'error'
    case 'marked_exhausted':
      return 'warning'
    case 'restored_active':
    case 'cleared_quarantine':
      return 'success'
    case 'transient_backoff_set':
      return 'warning'
    case 'transient_backoff_cleared':
      return 'success'
    case 'mcp_session_init_backoff_set':
    case 'mcp_session_retry_waited':
    case 'mcp_session_retry_scheduled':
      return 'warning'
    default:
      return 'neutral'
  }
}

function keyEffectLabel(code: string | null | undefined, strings: AdminTranslations): string {
  switch ((code ?? '').trim()) {
    case 'quarantined':
      return strings.logs.keyEffects.quarantined
    case 'marked_exhausted':
      return strings.logs.keyEffects.markedExhausted
    case 'restored_active':
      return strings.logs.keyEffects.restoredActive
    case 'cleared_quarantine':
      return strings.logs.keyEffects.clearedQuarantine
    case 'transient_backoff_set':
      return strings.logs.keyEffects.transientBackoffSet
    case 'transient_backoff_cleared':
      return strings.logs.keyEffects.transientBackoffCleared
    case 'mcp_session_init_backoff_set':
      return strings.logs.keyEffects.mcpSessionInitBackoffSet
    case 'mcp_session_retry_waited':
      return strings.logs.keyEffects.mcpSessionRetryWaited
    case 'mcp_session_retry_scheduled':
      return strings.logs.keyEffects.mcpSessionRetryScheduled
    case 'none':
    case '':
      return strings.logs.keyEffects.none
    default:
      return strings.logs.keyEffects.unknown
  }
}

function keyEffectBadgeLabel(log: RequestLog, strings: AdminTranslations): string {
  return keyEffectLabel(log.key_effect_code, strings)
}

function isRebalanceGatewayLog(log: RequestLog): boolean {
  return (log.gateway_mode ?? '').trim() === 'rebalance_http'
}

function RebalanceGatewayMarker(): JSX.Element {
  return (
    <span className="log-key-pill__marker" aria-hidden="true">
      <svg viewBox="0 0 16 16" focusable="false">
        <path
          d="M5 2.75a1.75 1.75 0 1 1-1 3.18V11a1 1 0 0 0 1 1h4.18a1.75 1.75 0 1 1 0 1.5H5A2.5 2.5 0 0 1 2.5 11V5.93A1.75 1.75 0 0 1 5 2.75m6 0a1.75 1.75 0 1 1-1 3.18V8a1 1 0 0 1-1 1H6.5v-1.5H8.5V5.93A1.75 1.75 0 0 1 11 2.75"
          fill="currentColor"
        />
      </svg>
    </span>
  )
}

function keyEffectBadgeCompactLabel(log: RequestLog, strings: AdminTranslations, language: Language): string {
  switch ((log.key_effect_code ?? '').trim()) {
    case 'quarantined':
      return language === 'zh' ? '隔离' : 'Quarantined'
    case 'marked_exhausted':
      return language === 'zh' ? '耗尽' : 'Exhausted'
    case 'restored_active':
      return language === 'zh' ? '恢复' : 'Restored'
    case 'cleared_quarantine':
      return language === 'zh' ? '解隔离' : 'Cleared'
    case 'mcp_session_init_backoff_set':
      return language === 'zh' ? 'MCP回退' : 'MCP backoff'
    case 'mcp_session_retry_waited':
      return language === 'zh' ? 'MCP已等待' : 'MCP waited'
    case 'mcp_session_retry_scheduled':
      return language === 'zh' ? 'MCP重试' : 'MCP retry'
    default:
      return keyEffectBadgeLabel(log, strings)
  }
}

function bindingEffectTone(code: string | null | undefined): StatusTone {
  switch ((code ?? '').trim()) {
    case 'http_project_affinity_bound':
      return 'success'
    case 'http_project_affinity_reused':
      return 'neutral'
    case 'http_project_affinity_rebound':
      return 'warning'
    default:
      return 'neutral'
  }
}

function bindingEffectLabel(code: string | null | undefined, strings: AdminTranslations): string {
  switch ((code ?? '').trim()) {
    case 'http_project_affinity_bound':
      return strings.logs.bindingEffects.bound
    case 'http_project_affinity_reused':
      return strings.logs.bindingEffects.reused
    case 'http_project_affinity_rebound':
      return strings.logs.bindingEffects.rebound
    case 'none':
    case '':
      return strings.logs.bindingEffects.none
    default:
      return strings.logs.bindingEffects.unknown
  }
}

function bindingEffectBadgeLabel(log: RequestLog, strings: AdminTranslations, language: Language): string {
  switch ((log.binding_effect_code ?? '').trim()) {
    case 'http_project_affinity_bound':
      return language === 'zh' ? '绑定' : 'Bound'
    case 'http_project_affinity_reused':
      return language === 'zh' ? '复用' : 'Reused'
    case 'http_project_affinity_rebound':
      return language === 'zh' ? '重绑' : 'Rebound'
    default:
      return bindingEffectLabel(log.binding_effect_code, strings)
  }
}

function selectionEffectTone(code: string | null | undefined): StatusTone {
  switch ((code ?? '').trim()) {
    case 'mcp_session_init_cooldown_avoided':
    case 'http_project_affinity_cooldown_avoided':
      return 'warning'
    case 'mcp_session_init_rate_limit_avoided':
    case 'http_project_affinity_rate_limit_avoided':
      return 'error'
    case 'mcp_session_init_pressure_avoided':
    case 'http_project_affinity_pressure_avoided':
      return 'success'
    default:
      return 'neutral'
  }
}

function selectionEffectLabel(code: string | null | undefined, strings: AdminTranslations): string {
  switch ((code ?? '').trim()) {
    case 'mcp_session_init_cooldown_avoided':
      return strings.logs.selectionEffects.mcpSessionInitCooldownAvoided
    case 'mcp_session_init_rate_limit_avoided':
      return strings.logs.selectionEffects.mcpSessionInitRateLimitAvoided
    case 'mcp_session_init_pressure_avoided':
      return strings.logs.selectionEffects.mcpSessionInitPressureAvoided
    case 'http_project_affinity_cooldown_avoided':
      return strings.logs.selectionEffects.httpProjectAffinityCooldownAvoided
    case 'http_project_affinity_rate_limit_avoided':
      return strings.logs.selectionEffects.httpProjectAffinityRateLimitAvoided
    case 'http_project_affinity_pressure_avoided':
      return strings.logs.selectionEffects.httpProjectAffinityPressureAvoided
    case 'none':
    case '':
      return strings.logs.selectionEffects.none
    default:
      return strings.logs.selectionEffects.unknown
  }
}

function selectionEffectBadgeLabel(log: RequestLog, strings: AdminTranslations, language: Language): string {
  switch ((log.selection_effect_code ?? '').trim()) {
    case 'mcp_session_init_cooldown_avoided':
      return language === 'zh' ? 'MCP避冷却' : 'MCP cooldown'
    case 'mcp_session_init_rate_limit_avoided':
      return language === 'zh' ? 'MCP避429' : 'MCP 429'
    case 'mcp_session_init_pressure_avoided':
      return language === 'zh' ? 'MCP避高压' : 'MCP pressure'
    case 'http_project_affinity_cooldown_avoided':
      return language === 'zh' ? '避冷却' : 'Cooldown'
    case 'http_project_affinity_rate_limit_avoided':
      return language === 'zh' ? '避429' : '429 avoided'
    case 'http_project_affinity_pressure_avoided':
      return language === 'zh' ? '避高压' : 'Pressure'
    default:
      return selectionEffectLabel(log.selection_effect_code, strings)
  }
}

function formatKeyEffectSummary(
  log: RequestLog,
  strings: AdminTranslations,
  language: Language,
): string {
  const summary = log.key_effect_summary?.trim()
  switch ((log.key_effect_code ?? '').trim()) {
    case 'quarantined':
      return language === 'zh' ? '系统已自动隔离该 Key' : 'The system automatically quarantined this key'
    case 'marked_exhausted':
      return language === 'zh' ? '系统已自动将该 Key 标记为耗尽' : 'The system automatically marked this key as exhausted'
    case 'restored_active':
      return language === 'zh'
        ? '系统已自动将 exhausted Key 恢复为 active'
        : 'The system automatically restored this exhausted key to active'
    case 'cleared_quarantine':
      return language === 'zh' ? '管理员已解除该 Key 的隔离' : 'An admin cleared the quarantine on this key'
    case 'mcp_session_init_backoff_set':
      return language === 'zh'
        ? '系统已为 MCP 初始化请求设置临时退避'
        : 'The system armed a temporary backoff for MCP session initialization'
    case 'mcp_session_retry_waited':
      return language === 'zh'
        ? '该 MCP 会话请求在发送上游前等待了 Retry-After'
        : 'This MCP session request waited for Retry-After before sending upstream'
    case 'mcp_session_retry_scheduled':
      return language === 'zh'
        ? '该 MCP 会话请求命中上游 429 后按 Retry-After 安排了一次重试'
        : 'This MCP session request hit upstream 429 and retried once after Retry-After'
    case 'none':
      return strings.logDetails.noKeyEffect
    default:
      return summary && summary.length > 0 ? summary : strings.logDetails.noKeyEffect
  }
}

function formatBindingEffectSummary(
  log: RequestLog,
  strings: AdminTranslations,
  language: Language,
): string {
  const summary = log.binding_effect_summary?.trim()
  switch ((log.binding_effect_code ?? '').trim()) {
    case 'http_project_affinity_bound':
      return language === 'zh'
        ? '系统为该项目创建了新的上游 Key 绑定'
        : 'The system created a new upstream key binding for this project'
    case 'http_project_affinity_reused':
      return language === 'zh'
        ? '系统继续复用该项目当前的上游 Key 绑定'
        : 'The system reused the current upstream key binding for this project'
    case 'http_project_affinity_rebound':
      return language === 'zh'
        ? '系统将该项目重新绑定到新的上游 Key'
        : 'The system rebound this project to a different upstream key'
    case 'none':
      return strings.logDetails.noBindingEffect
    default:
      return summary && summary.length > 0 ? summary : strings.logDetails.noBindingEffect
  }
}

function formatSelectionEffectSummary(
  log: RequestLog,
  strings: AdminTranslations,
  language: Language,
): string {
  const summary = log.selection_effect_summary?.trim()
  switch ((log.selection_effect_code ?? '').trim()) {
    case 'mcp_session_init_cooldown_avoided':
      return language === 'zh'
        ? '初始化时避开了仍处于冷却中的 Key'
        : 'Initialization avoided a key that is still in cooldown'
    case 'mcp_session_init_rate_limit_avoided':
      return language === 'zh'
        ? '初始化时避开了最近更容易触发限流的 Key'
        : 'Initialization avoided a key that was recently more rate-limited'
    case 'mcp_session_init_pressure_avoided':
      return language === 'zh'
        ? '初始化时避开了近期压力更高的 Key'
        : 'Initialization avoided a key under higher recent pressure'
    case 'http_project_affinity_cooldown_avoided':
      return language === 'zh'
        ? '项目亲和选路避开了仍处于冷却中的 Key'
        : 'Project affinity routing avoided a key that is still in cooldown'
    case 'http_project_affinity_rate_limit_avoided':
      return language === 'zh'
        ? '项目亲和选路避开了最近更容易触发限流的 Key'
        : 'Project affinity routing avoided a key that was recently more rate-limited'
    case 'http_project_affinity_pressure_avoided':
      return language === 'zh'
        ? '项目亲和选路避开了近期压力更高的 Key'
        : 'Project affinity routing avoided a key under higher recent pressure'
    case 'none':
      return strings.logDetails.noSelectionEffect
    default:
      return summary && summary.length > 0 ? summary : strings.logDetails.noSelectionEffect
  }
}

function hasExplicitEffect(code: string | null | undefined): boolean {
  const normalized = (code ?? '').trim()
  return normalized.length > 0 && normalized !== 'none'
}

function renderEffectBadges(
  log: RequestLog,
  strings: AdminTranslations,
  language: Language,
  className?: string,
): JSX.Element {
  const badgeClassName = className ? `${className} recent-requests-effect-badge` : 'recent-requests-effect-badge'
  const badges: JSX.Element[] = []

  if (hasExplicitEffect(log.key_effect_code)) {
    badges.push(
      <StatusBadge
        key="key"
        className={badgeClassName}
        tone={keyEffectTone(log.key_effect_code)}
        title={formatKeyEffectSummary(log, strings, language)}
      >
        {keyEffectBadgeCompactLabel(log, strings, language)}
      </StatusBadge>,
    )
  }
  if (hasExplicitEffect(log.binding_effect_code)) {
    badges.push(
      <StatusBadge
        key="binding"
        className={badgeClassName}
        tone={bindingEffectTone(log.binding_effect_code)}
        title={formatBindingEffectSummary(log, strings, language)}
      >
        {bindingEffectBadgeLabel(log, strings, language)}
      </StatusBadge>,
    )
  }
  if (hasExplicitEffect(log.selection_effect_code)) {
    badges.push(
      <StatusBadge
        key="selection"
        className={badgeClassName}
        tone={selectionEffectTone(log.selection_effect_code)}
        title={formatSelectionEffectSummary(log, strings, language)}
      >
        {selectionEffectBadgeLabel(log, strings, language)}
      </StatusBadge>,
    )
  }

  if (badges.length === 0) {
    badges.push(
      <StatusBadge
        key="none"
        className={badgeClassName}
        tone={keyEffectTone(log.key_effect_code)}
        title={formatKeyEffectSummary(log, strings, language)}
      >
        {keyEffectBadgeCompactLabel(log, strings, language)}
      </StatusBadge>,
    )
  }

  return <div className="recent-requests-effect-badges">{badges}</div>
}

function formatRequestStatusPair(httpStatus: number | null, mcpStatus: number | null): string {
  return `${httpStatus ?? '—'} / ${mcpStatus ?? '—'}`
}

function formatRequestStatusTooltip(log: RequestLog, strings: AdminTranslations): string {
  return `${strings.logs.table.httpStatus}: ${log.http_status ?? '—'} · ${strings.logs.table.mcpStatus}: ${log.mcp_status ?? '—'}`
}

function formatChargedCredits(value: number | null | undefined): string {
  return value != null ? String(value) : '—'
}

function formatRequestLine(log: RequestLog): string {
  const query = log.query ? `?${log.query}` : ''
  return `${log.method} ${log.path}${query}`
}

function formatErrorMessage(log: RequestLog, strings: AdminTranslations['logs']['errors']): string {
  const message = log.error_message?.trim()
  if (message) return message

  const status = log.result_status.toLowerCase()
  if (status === 'quota_exhausted') {
    if (log.http_status != null) {
      return strings.quotaExhaustedHttp.replace('{http}', String(log.http_status))
    }
    return strings.quotaExhausted
  }

  if (status === 'error') {
    if (log.http_status != null && log.mcp_status != null) {
      return strings.requestFailedHttpMcp
        .replace('{http}', String(log.http_status))
        .replace('{mcp}', String(log.mcp_status))
    }
    if (log.http_status != null) return strings.requestFailedHttp.replace('{http}', String(log.http_status))
    if (log.mcp_status != null) return strings.requestFailedMcp.replace('{mcp}', String(log.mcp_status))
    return strings.requestFailedGeneric
  }

  if (status === 'success') return strings.none
  if (log.http_status != null) return strings.httpStatus.replace('{http}', String(log.http_status))
  return strings.none
}

function summarizeSingleFacet(
  selectedValue: string | null | undefined,
  options: LogFacetOption[] | undefined,
  fallbackLabel: string,
): string {
  const normalized = selectedValue?.trim()
  if (!normalized) return fallbackLabel
  return options?.find((option) => option.value === normalized)?.value ?? normalized
}

function summarizeOutcomeFilter(
  filter: RecentRequestsOutcomeFilter | null,
  strings: AdminTranslations,
  allLabel: string,
): string {
  if (!filter?.value) return allLabel
  if (filter.kind === 'result') {
    return statusLabel(filter.value, strings)
  }
  if (filter.kind === 'keyEffect') {
    return keyEffectLabel(filter.value, strings)
  }
  if (filter.kind === 'bindingEffect') {
    return bindingEffectLabel(filter.value, strings)
  }
  return selectionEffectLabel(filter.value, strings)
}

function renderOutcomeFacetLabel(
  kind: RecentRequestsOutcomeFilterKind,
  value: string,
  strings: AdminTranslations,
): JSX.Element {
  const tone =
    kind === 'result'
      ? statusTone(value)
      : kind === 'keyEffect'
        ? keyEffectTone(value)
        : kind === 'bindingEffect'
          ? bindingEffectTone(value)
          : selectionEffectTone(value)
  const label =
    kind === 'result'
      ? statusLabel(value, strings)
      : kind === 'keyEffect'
        ? keyEffectLabel(value, strings)
        : kind === 'bindingEffect'
          ? bindingEffectLabel(value, strings)
          : selectionEffectLabel(value, strings)
  return <StatusBadge tone={tone}>{label}</StatusBadge>
}

function summarizeRequestKindTrigger(
  effectiveSelectedRequestKinds: string[],
  hasActiveQuickRequestKindFilters: boolean,
  requestKindQuickSummary: string,
  requestKindSummary: string,
  language: Language,
  allLabel: string,
): string {
  if (hasActiveQuickRequestKindFilters) return requestKindQuickSummary
  if (effectiveSelectedRequestKinds.length === 0) return allLabel
  if (effectiveSelectedRequestKinds.length <= 2) return requestKindSummary
  return language === 'zh' ? `已选 ${effectiveSelectedRequestKinds.length} 项` : `${effectiveSelectedRequestKinds.length} selected`
}

function RecentRequestDetails({
  log,
  logBodiesState,
  onRetryLoadBodies,
  strings,
  language,
  formatTime,
}: {
  log: RequestLog
  logBodiesState?: LogBodiesLoadState
  onRetryLoadBodies?: (() => void) | null
  strings: AdminTranslations
  language: Language
  formatTime: (ts: number | null) => string
}): JSX.Element {
  const forwarded = (log.forwarded_headers ?? []).filter((value) => value.trim().length > 0)
  const dropped = (log.dropped_headers ?? []).filter((value) => value.trim().length > 0)
  const requestBody =
    logBodiesState?.status === 'ready'
      ? logBodiesState.value.request_body ?? strings.logDetails.noBody
      : logBodiesState?.status === 'loading'
        ? strings.logDetails.loadingBody
        : logBodiesState?.status === 'error'
          ? strings.logDetails.loadBodyFailed
          : log.request_body ?? strings.logDetails.noBody
  const responseBody =
    logBodiesState?.status === 'ready'
      ? logBodiesState.value.response_body ?? strings.logDetails.noBody
      : logBodiesState?.status === 'loading'
        ? strings.logDetails.loadingBody
        : logBodiesState?.status === 'error'
          ? strings.logDetails.loadBodyFailed
          : log.response_body ?? strings.logDetails.noBody
  const requestKindLabel = log.request_kind_label ?? log.request_kind_key ?? strings.logs.errors.none
  const effectEntries = [
    hasExplicitEffect(log.key_effect_code)
      ? {
          label: strings.logDetails.keyEffect,
          value: formatKeyEffectSummary(log, strings, language),
        }
      : null,
    hasExplicitEffect(log.binding_effect_code)
      ? {
          label: strings.logDetails.bindingEffect,
          value: formatBindingEffectSummary(log, strings, language),
        }
      : null,
    hasExplicitEffect(log.selection_effect_code)
      ? {
          label: strings.logDetails.selectionEffect,
          value: formatSelectionEffectSummary(log, strings, language),
        }
      : null,
  ].filter((entry): entry is { label: string; value: string } => entry != null)
  const diagnosticEntries = [
    log.gateway_mode
      ? { label: strings.logDetails.gatewayMode, value: log.gateway_mode }
      : null,
    log.experiment_variant
      ? { label: strings.logDetails.experimentVariant, value: log.experiment_variant }
      : null,
    log.proxy_session_id
      ? { label: strings.logDetails.proxySessionId, value: log.proxy_session_id }
      : null,
    log.routing_subject_hash
      ? { label: strings.logDetails.routingSubjectHash, value: log.routing_subject_hash }
      : null,
    log.upstream_operation
      ? { label: strings.logDetails.upstreamOperation, value: log.upstream_operation }
      : null,
    log.fallback_reason
      ? { label: strings.logDetails.fallbackReason, value: log.fallback_reason }
      : null,
  ].filter((entry): entry is { label: string; value: string } => entry != null)

  return (
    <div className="log-details-panel">
      <div className="log-details-summary">
        <div>
          <span className="log-details-label">{strings.logs.table.time}</span>
          <span className="log-details-value">{formatTime(log.created_at)}</span>
        </div>
        <div>
          <span className="log-details-label">{strings.logDetails.request}</span>
          <span className="log-details-value">{formatRequestLine(log)}</span>
        </div>
        <div>
          <span className="log-details-label">{strings.logs.table.status}</span>
          <span className="log-details-value">{formatRequestStatusPair(log.http_status, log.mcp_status)}</span>
        </div>
        <div>
          <span className="log-details-label">{strings.logs.table.chargedCredits}</span>
          <span className="log-details-value">{formatChargedCredits(log.business_credits)}</span>
        </div>
        <div>
          <span className="log-details-label">{strings.logs.table.requestType}</span>
          <span className="log-details-value">
            <RequestKindBadge
              requestKindKey={log.request_kind_key ?? null}
              requestKindLabel={requestKindLabel}
              size="sm"
            />
          </span>
        </div>
        <div>
          <span className="log-details-label">{strings.logDetails.outcome}</span>
          <span className="log-details-value">{statusLabel(log.result_status, strings)}</span>
        </div>
        {effectEntries.length === 0 ? (
          <div>
            <span className="log-details-label">{strings.logDetails.keyEffect}</span>
            <span className="log-details-value">{strings.logDetails.noKeyEffect}</span>
          </div>
        ) : (
          effectEntries.map((entry) => (
            <div key={entry.label}>
              <span className="log-details-label">{entry.label}</span>
              <span className="log-details-value">{entry.value}</span>
            </div>
          ))
        )}
        {diagnosticEntries.map((entry) => (
          <div key={entry.label}>
            <span className="log-details-label">{entry.label}</span>
            <span className="log-details-value">
              <code>{entry.value}</code>
            </span>
          </div>
        ))}
      </div>
      <div className="log-details-body">
        <div className="log-details-section">
          <header>{strings.logs.table.error}</header>
          <pre>{formatErrorMessage(log, strings.logs.errors)}</pre>
        </div>
        <div className="log-details-section">
          <header>{strings.logDetails.requestBody}</header>
          <pre>{requestBody}</pre>
        </div>
        <div className="log-details-section">
          <header>{strings.logDetails.responseBody}</header>
          <pre>{responseBody}</pre>
        </div>
      </div>
      {logBodiesState?.status === 'error' ? (
        <div className="log-details-feedback" role="alert">
          <span className="log-details-feedback-message">{logBodiesState.message}</span>
          {onRetryLoadBodies ? (
            <Button type="button" variant="outline" size="sm" onClick={onRetryLoadBodies}>
              {strings.logDetails.retryLoadBody}
            </Button>
          ) : null}
        </div>
      ) : null}
      {(forwarded.length > 0 || dropped.length > 0) && (
        <div className="log-details-headers">
          {forwarded.length > 0 ? (
            <div className="log-details-section">
              <header>{strings.logDetails.forwardedHeaders}</header>
              <ul>
                {forwarded.map((header, index) => (
                  <li key={`forwarded-${index}-${header}`}>{header}</li>
                ))}
              </ul>
            </div>
          ) : null}
          {dropped.length > 0 ? (
            <div className="log-details-section">
              <header>{strings.logDetails.droppedHeaders}</header>
              <ul>
                {dropped.map((header, index) => (
                  <li key={`dropped-${index}-${header}`}>{header}</li>
                ))}
              </ul>
            </div>
          ) : null}
        </div>
      )}
    </div>
  )
}

export default function AdminRecentRequestsPanel({
  variant,
  language,
  strings,
  title,
  description,
  emptyLabel,
  loadState,
  loadingLabel,
  errorLabel,
  logs,
  requestKindOptions,
  requestKindQuickBilling,
  requestKindQuickProtocol,
  selectedRequestKinds,
  onRequestKindQuickFiltersChange,
  onToggleRequestKind,
  onClearRequestKinds,
  outcomeFilter,
  resultOptions,
  keyEffectOptions,
  bindingEffectOptions = [],
  selectionEffectOptions = [],
  onOutcomeFilterChange,
  keyOptions = [],
  selectedKeyId,
  onKeyFilterChange,
  showKeyColumn,
  showTokenColumn,
  perPage,
  hasOlder,
  hasNewer,
  paginationSummary,
  paginationDisabled = false,
  onOlderPage,
  onNewerPage,
  onPerPageChange,
  formatTime,
  formatTimeDetail,
  onOpenKey,
  onOpenToken,
  loadLogBodies,
}: AdminRecentRequestsPanelProps): JSX.Element {
  const [expandedLogs, setExpandedLogs] = useState<Set<number>>(() => new Set())
  const [logBodiesById, setLogBodiesById] = useState<Record<number, LogBodiesLoadState>>({})
  const logBodyControllersRef = useRef<Map<number, AbortController>>(new Map())

  useEffect(() => {
    setExpandedLogs(new Set())
    for (const controller of logBodyControllersRef.current.values()) {
      controller.abort()
    }
    logBodyControllersRef.current.clear()
    setLogBodiesById({})
  }, [loadLogBodies, logs])

  const triggerLoadLogBodies = useCallback(
    (log: RequestLog, force = false) => {
      const currentState = logBodiesById[log.id]
      if (!force && (currentState?.status === 'loading' || currentState?.status === 'ready')) {
        return
      }

      logBodyControllersRef.current.get(log.id)?.abort()
      const controller = new AbortController()
      logBodyControllersRef.current.set(log.id, controller)
      setLogBodiesById((current) => ({ ...current, [log.id]: { status: 'loading' } }))

      loadLogBodies(log, controller.signal)
        .then((value) => {
          if (controller.signal.aborted) return
          setLogBodiesById((current) => ({
            ...current,
            [log.id]: { status: 'ready', value },
          }))
        })
        .catch((error) => {
          if ((error as Error | undefined)?.name === 'AbortError' || controller.signal.aborted) {
            return
          }
          const message =
            error instanceof Error && error.message.trim().length > 0
              ? error.message
              : strings.logDetails.loadBodyFailed
          setLogBodiesById((current) => ({
            ...current,
            [log.id]: { status: 'error', message },
          }))
        })
        .finally(() => {
          if (logBodyControllersRef.current.get(log.id) === controller) {
            logBodyControllersRef.current.delete(log.id)
          }
        })
    },
    [loadLogBodies, logBodiesById, strings.logDetails.loadBodyFailed],
  )

  const toggleExpandedLog = useCallback(
    (log: RequestLog) => {
      const expanded = expandedLogs.has(log.id)
      if (!expanded) {
        triggerLoadLogBodies(log)
      }
      setExpandedLogs((current) => {
        const next = new Set(current)
        if (next.has(log.id)) {
          next.delete(log.id)
        } else {
          next.add(log.id)
        }
        return next
      })
    },
    [expandedLogs, triggerLoadLogBodies],
  )

  const retryLoadBodies = useCallback(
    (log: RequestLog) => {
      triggerLoadLogBodies(log, true)
    },
    [triggerLoadLogBodies],
  )

  const normalizedSelectedRequestKinds = useMemo(
    () => Array.from(new Set(selectedRequestKinds.map((value) => value.trim()).filter(Boolean))),
    [selectedRequestKinds],
  )
  const requestKindCatalog = useMemo(
    () => mergeRequestKindCatalog(requestKindOptions),
    [requestKindOptions],
  )
  const requestKindQuickFilters = useMemo(
    () => ({
      billing: requestKindQuickBilling,
      protocol: requestKindQuickProtocol,
    }),
    [requestKindQuickBilling, requestKindQuickProtocol],
  )
  const hasActiveQuickRequestKindFilters = useMemo(
    () => hasActiveRequestKindQuickFilters(requestKindQuickFilters),
    [requestKindQuickFilters],
  )
  const quickSelection = useMemo(
    () => buildRequestKindQuickFilterSelection(requestKindOptions, requestKindQuickFilters),
    [requestKindOptions, requestKindQuickFilters],
  )
  const effectiveSelectedRequestKinds = useMemo(
    () => (hasActiveQuickRequestKindFilters ? quickSelection : normalizedSelectedRequestKinds),
    [hasActiveQuickRequestKindFilters, normalizedSelectedRequestKinds, quickSelection],
  )
  const visibleRequestKindOptions = useMemo(
    () =>
      buildVisibleRequestKindOptions(
        effectiveSelectedRequestKinds,
        requestKindCatalog,
        Object.fromEntries(requestKindCatalog.map((option) => [option.key, option])),
      ),
    [effectiveSelectedRequestKinds, requestKindCatalog],
  )
  const requestKindSummary = useMemo(
    () =>
      summarizeSelectedRequestKinds(
        effectiveSelectedRequestKinds,
        visibleRequestKindOptions,
        strings.logs.filters.requestTypeAll,
      ),
    [effectiveSelectedRequestKinds, strings.logs.filters.requestTypeAll, visibleRequestKindOptions],
  )
  const requestKindQuickSummary = useMemo(
    () => summarizeRequestKindQuickFilters(requestKindQuickFilters),
    [requestKindQuickFilters],
  )
  const requestKindTriggerSummary = useMemo(() => {
    return summarizeRequestKindTrigger(
      effectiveSelectedRequestKinds,
      hasActiveQuickRequestKindFilters,
      requestKindQuickSummary,
      requestKindSummary,
      language,
      recentRequestsCompactAllLabel,
    )
  }, [
    effectiveSelectedRequestKinds.length,
    hasActiveQuickRequestKindFilters,
    language,
    requestKindQuickSummary,
    requestKindSummary,
  ])
  const outcomeValue = outcomeFilter
    ? `${outcomeFilter.kind}:${outcomeFilter.value}`
    : recentRequestsAllFilterValue
  const outcomeSummary = useMemo(
    () =>
      summarizeOutcomeFilter(
        outcomeFilter,
        strings,
        recentRequestsCompactAllLabel,
      ),
    [outcomeFilter, strings],
  )
  const keyFilterSummary = useMemo(
    () => summarizeSingleFacet(selectedKeyId, keyOptions, recentRequestsCompactAllLabel),
    [keyOptions, selectedKeyId],
  )
  const summaryColumnCount = 6 + Number(showKeyColumn) + Number(showTokenColumn)
  const desktopClassName = `recent-requests-desktop recent-requests-desktop--${variant}`
  const mobileClassName = `recent-requests-mobile-list recent-requests-mobile-list--${variant}`
  const mobileCardClassName =
    variant === 'token' ? 'user-console-mobile-card' : 'admin-mobile-card'
  const mobileKvClassName =
    variant === 'token' ? 'user-console-mobile-kv' : 'admin-mobile-kv'
  const mobileStackedClassName =
    variant === 'token'
      ? 'user-console-mobile-kv user-console-mobile-kv--stacked'
      : 'admin-mobile-kv admin-mobile-kv--stacked'

  return (
    <section className="surface panel">
      <div className="panel-header recent-requests-header">
        <div>
          <h2>{title}</h2>
          <p className="panel-description">{description}</p>
        </div>
        <div className="panel-actions recent-requests-filters">
          <div className="recent-requests-filter-field recent-requests-filter-field--request-kind">
            <span className="recent-requests-filter-label">{strings.logs.filters.requestType}</span>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  className="recent-requests-filter-select-trigger recent-requests-filter-select-trigger--menu"
                  aria-label={`${strings.logs.filters.requestType}: ${requestKindTriggerSummary}`}
                >
                  <span className="recent-requests-filter-select-text">{requestKindTriggerSummary}</span>
                  <Icon icon="mdi:chevron-down" width={16} height={16} aria-hidden="true" />
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent
                align="end"
                className="token-request-kind-menu recent-requests-filter-menu recent-requests-filter-menu--request-kind"
              >
                <DropdownMenuLabel>{strings.logs.filters.requestType}</DropdownMenuLabel>
                <div className="token-request-quick-filters">
                  <div className="token-request-quick-filter-row">
                    <span className="token-request-quick-filter-label">{strings.logs.filters.billingGroup}</span>
                    <SegmentedTabs<TokenLogRequestKindQuickBilling>
                      value={requestKindQuickBilling}
                      onChange={(next) => onRequestKindQuickFiltersChange(next, requestKindQuickProtocol)}
                      options={requestKindBillingQuickFilterOptions}
                      ariaLabel={strings.logs.filters.billingGroup}
                      className="token-request-quick-segmented"
                    />
                  </div>
                  <div className="token-request-quick-filter-row">
                    <span className="token-request-quick-filter-label">{strings.logs.filters.protocolGroup}</span>
                    <SegmentedTabs<TokenLogRequestKindQuickProtocol>
                      value={requestKindQuickProtocol}
                      onChange={(next) => onRequestKindQuickFiltersChange(requestKindQuickBilling, next)}
                      options={requestKindProtocolQuickFilterOptions}
                      ariaLabel={strings.logs.filters.protocolGroup}
                      className="token-request-quick-segmented"
                    />
                  </div>
                </div>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  className="cursor-pointer"
                  disabled={effectiveSelectedRequestKinds.length === 0 && !hasActiveQuickRequestKindFilters}
                  onSelect={(event) => {
                    event.preventDefault()
                    onClearRequestKinds()
                  }}
                >
                  {strings.logs.filters.requestTypeAll}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                {visibleRequestKindOptions.length === 0 ? (
                  <DropdownMenuItem disabled>{strings.logs.filters.requestTypeEmpty}</DropdownMenuItem>
                ) : (
                  visibleRequestKindOptions.map((option) => (
                    <DropdownMenuCheckboxItem
                      key={option.key}
                      className="cursor-pointer"
                      checked={effectiveSelectedRequestKinds.includes(option.key)}
                      onSelect={(event) => event.preventDefault()}
                      onCheckedChange={() => onToggleRequestKind(option.key)}
                    >
                      <span className="recent-requests-request-kind-option">
                        <RequestKindBadge requestKindKey={option.key} requestKindLabel={option.label} size="sm" />
                        <span className="recent-requests-request-kind-count">
                          {`x${option.count ?? 0}`}
                        </span>
                      </span>
                    </DropdownMenuCheckboxItem>
                  ))
                )}
              </DropdownMenuContent>
            </DropdownMenu>
          </div>

          <div className="recent-requests-filter-field">
            <span className="recent-requests-filter-label">{strings.logs.filters.resultOrEffect}</span>
            <Select
              value={outcomeValue}
              onValueChange={(value) => {
                if (!value || value === recentRequestsAllFilterValue) {
                  onOutcomeFilterChange(null)
                  return
                }
                const [kind, nextValue] = value.split(':', 2)
                if (
                  !nextValue ||
                  (kind !== 'result'
                    && kind !== 'keyEffect'
                    && kind !== 'bindingEffect'
                    && kind !== 'selectionEffect')
                ) {
                  onOutcomeFilterChange(null)
                  return
                }
                onOutcomeFilterChange({
                  kind: kind as RecentRequestsOutcomeFilterKind,
                  value: nextValue,
                })
              }}
            >
              <SelectTrigger
                className="recent-requests-filter-select-trigger"
                aria-label={`${strings.logs.filters.resultOrEffect}: ${outcomeSummary}`}
              >
                <span className="recent-requests-filter-select-text">{outcomeSummary}</span>
              </SelectTrigger>
              <SelectContent className="recent-requests-filter-content">
                <SelectItem value={recentRequestsAllFilterValue}>{strings.logs.filters.resultOrEffectAll}</SelectItem>
                <SelectSeparator />
                <SelectGroup>
                  <SelectLabel>{strings.logs.filters.resultGroup}</SelectLabel>
                  {resultOptions.length === 0 ? (
                    <SelectItem value="__recent-requests-no-results__" disabled>
                      {strings.logs.filters.noFacetOptions}
                    </SelectItem>
                  ) : (
                    resultOptions.map((option) => (
                      <SelectItem key={`result-${option.value}`} value={`result:${option.value}`}>
                        <span className="recent-requests-facet-option recent-requests-facet-option--status">
                          <span className="recent-requests-facet-option-main">
                            {renderOutcomeFacetLabel('result', option.value, strings)}
                          </span>
                          <span className="recent-requests-facet-option-spacer" aria-hidden="true" />
                          <span className="recent-requests-facet-count">{`x${option.count ?? 0}`}</span>
                        </span>
                      </SelectItem>
                    ))
                  )}
                </SelectGroup>
                <SelectSeparator />
                <SelectGroup>
                  <SelectLabel>{strings.logs.filters.keyEffectGroup}</SelectLabel>
                  {keyEffectOptions.length === 0 ? (
                    <SelectItem value="__recent-requests-no-effects__" disabled>
                      {strings.logs.filters.noFacetOptions}
                    </SelectItem>
                  ) : (
                    keyEffectOptions.map((option) => (
                      <SelectItem key={`effect-${option.value}`} value={`keyEffect:${option.value}`}>
                        <span className="recent-requests-facet-option recent-requests-facet-option--status">
                          <span className="recent-requests-facet-option-main">
                            {renderOutcomeFacetLabel('keyEffect', option.value, strings)}
                          </span>
                          <span className="recent-requests-facet-option-spacer" aria-hidden="true" />
                          <span className="recent-requests-facet-count">{`x${option.count ?? 0}`}</span>
                        </span>
                      </SelectItem>
                    ))
                  )}
                </SelectGroup>
                <SelectSeparator />
                <SelectGroup>
                  <SelectLabel>{strings.logs.filters.bindingEffectGroup}</SelectLabel>
                  {bindingEffectOptions.length === 0 ? (
                    <SelectItem value="__recent-requests-no-binding-effects__" disabled>
                      {strings.logs.filters.noFacetOptions}
                    </SelectItem>
                  ) : (
                    bindingEffectOptions.map((option) => (
                      <SelectItem key={`binding-effect-${option.value}`} value={`bindingEffect:${option.value}`}>
                        <span className="recent-requests-facet-option recent-requests-facet-option--status">
                          <span className="recent-requests-facet-option-main">
                            {renderOutcomeFacetLabel('bindingEffect', option.value, strings)}
                          </span>
                          <span className="recent-requests-facet-option-spacer" aria-hidden="true" />
                          <span className="recent-requests-facet-count">{`x${option.count ?? 0}`}</span>
                        </span>
                      </SelectItem>
                    ))
                  )}
                </SelectGroup>
                <SelectSeparator />
                <SelectGroup>
                  <SelectLabel>{strings.logs.filters.selectionEffectGroup}</SelectLabel>
                  {selectionEffectOptions.length === 0 ? (
                    <SelectItem value="__recent-requests-no-selection-effects__" disabled>
                      {strings.logs.filters.noFacetOptions}
                    </SelectItem>
                  ) : (
                    selectionEffectOptions.map((option) => (
                      <SelectItem key={`selection-effect-${option.value}`} value={`selectionEffect:${option.value}`}>
                        <span className="recent-requests-facet-option recent-requests-facet-option--status">
                          <span className="recent-requests-facet-option-main">
                            {renderOutcomeFacetLabel('selectionEffect', option.value, strings)}
                          </span>
                          <span className="recent-requests-facet-option-spacer" aria-hidden="true" />
                          <span className="recent-requests-facet-count">{`x${option.count ?? 0}`}</span>
                        </span>
                      </SelectItem>
                    ))
                  )}
                </SelectGroup>
              </SelectContent>
            </Select>
          </div>

          {showKeyColumn && onKeyFilterChange ? (
            <div className="recent-requests-filter-field">
              <span className="recent-requests-filter-label">{strings.logs.table.key}</span>
              <SearchableFacetSelect
                value={selectedKeyId ?? null}
                options={keyOptions}
                summary={keyFilterSummary}
                allLabel={strings.logs.filters.keyAll}
                emptyLabel={strings.logs.filters.noFacetOptions}
                searchPlaceholder={language === 'zh' ? '输入 Key 片段筛选' : 'Filter keys'}
                searchAriaLabel={language === 'zh' ? '筛选 Key' : 'Filter keys'}
                triggerAriaLabel={`${strings.logs.table.key}: ${keyFilterSummary}`}
                listAriaLabel={strings.logs.table.key}
                onChange={onKeyFilterChange}
                disabled={keyOptions.length === 0 && !selectedKeyId}
                triggerClassName="recent-requests-filter-select-trigger recent-requests-filter-select-trigger--menu"
                contentClassName="recent-requests-filter-menu"
              />
            </div>
          ) : null}
        </div>
      </div>

      <AdminTableShell
        className={desktopClassName}
        tableClassName={`recent-requests-table recent-requests-table--${variant}`}
        loadState={loadState}
        loadingLabel={loadingLabel}
        errorLabel={errorLabel ?? undefined}
        minHeight={320}
      >
        <TableHeader>
          <TableRow>
            <TableHead className="recent-requests-col recent-requests-col--time">{strings.logs.table.time}</TableHead>
            {showTokenColumn ? (
              <TableHead className="recent-requests-col recent-requests-col--token">{strings.logs.table.token}</TableHead>
            ) : null}
            {showKeyColumn ? (
              <TableHead className="recent-requests-col recent-requests-col--key">{strings.logs.table.key}</TableHead>
            ) : null}
            <TableHead className="recent-requests-col recent-requests-col--request-type">{strings.logs.table.requestType}</TableHead>
            <TableHead className="recent-requests-col recent-requests-col--status">{strings.logs.table.status}</TableHead>
            <TableHead className="recent-requests-col recent-requests-col--credits">{strings.logs.table.chargedCredits}</TableHead>
            <TableHead className="recent-requests-col recent-requests-col--result">{strings.logs.table.result}</TableHead>
            <TableHead className="recent-requests-col recent-requests-col--key-effect">{strings.logs.table.effects}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {logs.length === 0 ? (
            <TableRow>
              <TableCell colSpan={summaryColumnCount}>
                <div className="empty-state alert">{emptyLabel}</div>
              </TableCell>
            </TableRow>
          ) : (
            logs.map((log) => {
              const expanded = expandedLogs.has(log.id)
              const resolvedLogBodiesState =
                logBodiesById[log.id]
                ?? (expanded
                  ? {
                      status: 'loading' as const,
                    }
                  : undefined)
              const requestKindLabel = log.request_kind_label ?? log.request_kind_key ?? strings.logs.errors.none
              const keyId = log.key_id?.trim() || null
              const tokenId = log.auth_token_id?.trim() || null
              const isRebalanceGateway = isRebalanceGatewayLog(log)
              const timeLabel = formatTime(log.created_at)
              const timeDetailLabel = formatTimeDetail?.(log.created_at) ?? null
              const hasTimeBubble = Boolean(timeDetailLabel && timeDetailLabel !== timeLabel)
              return (
                <Fragment key={log.id}>
                  <TableRow>
                    <TableCell className="recent-requests-col recent-requests-col--time">
                      <div className="log-time-cell">
                        {hasTimeBubble ? (
                          <>
                            <button type="button" className="log-time-trigger" aria-label={timeDetailLabel ?? timeLabel}>
                              <span className="log-time-main">{timeLabel}</span>
                            </button>
                            <div className="log-time-bubble">{timeDetailLabel}</div>
                          </>
                        ) : (
                          <span className="log-time-main">{timeLabel}</span>
                        )}
                      </div>
                    </TableCell>
                    {showTokenColumn ? (
                      <TableCell className="recent-requests-col recent-requests-col--token">
                        {tokenId ? (
                          <button
                            type="button"
                            className="link-button log-token-link recent-requests-entity-link request-entity-button"
                            onClick={() => onOpenToken?.(tokenId)}
                          >
                            <code>{tokenId}</code>
                          </button>
                        ) : (
                          strings.logs.errors.none
                        )}
                      </TableCell>
                    ) : null}
                    {showKeyColumn ? (
                      <TableCell className="recent-requests-col recent-requests-col--key">
                        {keyId ? (
                          <button
                            type="button"
                            className={[
                              'link-button',
                              'log-token-link',
                              'log-key-link',
                              'recent-requests-entity-link',
                              'request-entity-button',
                              'log-key-pill',
                              isRebalanceGateway ? 'log-key-pill--rebalance' : '',
                            ].filter(Boolean).join(' ')}
                            onClick={() => onOpenKey?.(keyId)}
                          >
                            {isRebalanceGateway ? (
                              <>
                                <RebalanceGatewayMarker />
                                <span className="sr-only">{strings.logDetails.gatewayMode}: {log.gateway_mode}</span>
                              </>
                            ) : null}
                            <code>{keyId}</code>
                          </button>
                        ) : (
                          strings.logs.errors.none
                        )}
                      </TableCell>
                    ) : null}
                    <TableCell className="recent-requests-col recent-requests-col--request-type">
                      <RequestKindBadge requestKindKey={log.request_kind_key ?? null} requestKindLabel={requestKindLabel} size="sm" />
                    </TableCell>
                    <TableCell className="recent-requests-col recent-requests-col--status">
                      <Tooltip>
                        <TooltipTrigger asChild>
                        <button
                          type="button"
                          className="status-pair-trigger"
                          aria-label={formatRequestStatusTooltip(log, strings)}
                        >
                          {formatRequestStatusPair(log.http_status, log.mcp_status)}
                        </button>
                        </TooltipTrigger>
                        <TooltipContent side="top">
                          {formatRequestStatusTooltip(log, strings)}
                        </TooltipContent>
                      </Tooltip>
                    </TableCell>
                    <TableCell className="recent-requests-col recent-requests-col--credits">
                      {formatChargedCredits(log.business_credits)}
                    </TableCell>
                    <TableCell className="recent-requests-col recent-requests-col--result">
                      <Button
                        type="button"
                        variant="ghost"
                        className={`log-result-button${expanded ? ' log-result-button-active' : ''}`}
                        onClick={() => toggleExpandedLog(log)}
                        aria-expanded={expanded}
                        aria-controls={`recent-request-details-${log.id}`}
                      >
                        <StatusBadge tone={statusTone(log.result_status)}>
                          {statusLabel(log.result_status, strings)}
                        </StatusBadge>
                        <Icon
                          icon={expanded ? 'mdi:chevron-up' : 'mdi:chevron-down'}
                          width={18}
                          height={18}
                          className="log-result-icon"
                          aria-hidden="true"
                        />
                      </Button>
                    </TableCell>
                    <TableCell className="recent-requests-col recent-requests-col--key-effect">
                      {renderEffectBadges(log, strings, language)}
                    </TableCell>
                  </TableRow>
                  {expanded ? (
                    <TableRow className="log-details-row">
                      <TableCell
                        colSpan={summaryColumnCount}
                        id={`recent-request-details-${log.id}`}
                      >
                        <RecentRequestDetails
                          log={log}
                          logBodiesState={resolvedLogBodiesState}
                          onRetryLoadBodies={() => retryLoadBodies(log)}
                          strings={strings}
                          language={language}
                          formatTime={formatTime}
                        />
                      </TableCell>
                    </TableRow>
                  ) : null}
                </Fragment>
              )
            })
          )}
        </TableBody>
      </AdminTableShell>

      <AdminLoadingRegion
        className={mobileClassName}
        loadState={loadState}
        loadingLabel={loadingLabel}
        errorLabel={errorLabel ?? undefined}
        minHeight={240}
      >
        {logs.length === 0 ? (
          <div className="empty-state alert">{emptyLabel}</div>
        ) : (
          logs.map((log) => {
            const expanded = expandedLogs.has(log.id)
            const resolvedLogBodiesState =
              logBodiesById[log.id]
              ?? (expanded
                ? {
                    status: 'loading' as const,
                  }
                : undefined)
            const keyId = log.key_id?.trim() || null
            const tokenId = log.auth_token_id?.trim() || null
            const isRebalanceGateway = isRebalanceGatewayLog(log)
            return (
              <article key={log.id} className={mobileCardClassName}>
                <div className={mobileKvClassName}>
                  <span>{strings.logs.table.time}</span>
                  <strong>{formatTime(log.created_at)}</strong>
                </div>
                {showTokenColumn ? (
                  <div className={mobileKvClassName}>
                    <span>{strings.logs.table.token}</span>
                    {tokenId ? (
                      <button type="button" className="request-entity-button admin-mobile-request-entity-button" onClick={() => onOpenToken?.(tokenId)}>
                        <strong><code>{tokenId}</code></strong>
                      </button>
                    ) : (
                      <strong><code>{strings.logs.errors.none}</code></strong>
                    )}
                  </div>
                ) : null}
                {showKeyColumn ? (
                  <div className={mobileKvClassName}>
                    <span>{strings.logs.table.key}</span>
                    {keyId ? (
                      <button
                        type="button"
                        className={[
                          'request-entity-button',
                          'admin-mobile-request-entity-button',
                          'log-key-link',
                          'log-key-pill',
                          isRebalanceGateway ? 'log-key-pill--rebalance' : '',
                        ].filter(Boolean).join(' ')}
                        onClick={() => onOpenKey?.(keyId)}
                      >
                        {isRebalanceGateway ? (
                          <>
                            <RebalanceGatewayMarker />
                            <span className="sr-only">{strings.logDetails.gatewayMode}: {log.gateway_mode}</span>
                          </>
                        ) : null}
                        <strong><code>{keyId}</code></strong>
                      </button>
                    ) : (
                      <strong><code>{strings.logs.errors.none}</code></strong>
                    )}
                  </div>
                ) : null}
                <div className={mobileKvClassName}>
                  <span>{strings.logDetails.request}</span>
                  <strong>{formatRequestLine(log)}</strong>
                </div>
                <div className={mobileKvClassName}>
                  <span>{strings.logs.table.requestType}</span>
                  <RequestKindBadge
                    requestKindKey={log.request_kind_key ?? null}
                    requestKindLabel={log.request_kind_label ?? log.request_kind_key ?? strings.logs.errors.none}
                    size="sm"
                    className={variant === 'token' ? 'user-console-mobile-request-kind' : undefined}
                  />
                </div>
                <div className={mobileKvClassName}>
                  <span>{strings.logs.table.status}</span>
                  <Tooltip>
                    <TooltipTrigger asChild>
                    <button type="button" className="status-pair-trigger" aria-label={formatRequestStatusTooltip(log, strings)}>
                      <strong>{formatRequestStatusPair(log.http_status, log.mcp_status)}</strong>
                    </button>
                    </TooltipTrigger>
                    <TooltipContent side="top">
                      {formatRequestStatusTooltip(log, strings)}
                    </TooltipContent>
                  </Tooltip>
                </div>
                <div className={mobileKvClassName}>
                  <span>{strings.logs.table.chargedCredits}</span>
                  <strong>{formatChargedCredits(log.business_credits)}</strong>
                </div>
                <div className={mobileKvClassName}>
                  <span>{strings.logs.table.result}</span>
                  <button
                    type="button"
                    className={`log-result-button recent-requests-mobile-result-button${expanded ? ' log-result-button-active' : ''}`}
                    onClick={() => toggleExpandedLog(log)}
                    aria-expanded={expanded}
                    aria-controls={`recent-request-mobile-details-${log.id}`}
                  >
                    <StatusBadge className={variant === 'token' ? 'user-console-mobile-status' : undefined} tone={statusTone(log.result_status)}>
                      {statusLabel(log.result_status, strings)}
                    </StatusBadge>
                    <Icon
                      icon={expanded ? 'mdi:chevron-up' : 'mdi:chevron-down'}
                      width={18}
                      height={18}
                      className="log-result-icon"
                      aria-hidden="true"
                    />
                  </button>
                </div>
                <div className={mobileStackedClassName}>
                  <span>{strings.logs.table.effects}</span>
                  {renderEffectBadges(
                    log,
                    strings,
                    language,
                    variant === 'token' ? 'user-console-mobile-status' : undefined,
                  )}
                </div>
                {expanded ? (
                  <div className="recent-requests-mobile-details" id={`recent-request-mobile-details-${log.id}`}>
                    <RecentRequestDetails
                      log={log}
                      logBodiesState={resolvedLogBodiesState}
                      onRetryLoadBodies={() => retryLoadBodies(log)}
                      strings={strings}
                      language={language}
                      formatTime={formatTime}
                    />
                  </div>
                ) : null}
              </article>
            )
          })
        )}
      </AdminLoadingRegion>

      <AdminTablePagination
        page={1}
        totalPages={1}
        pageSummary={paginationSummary}
        perPage={perPage}
        previousLabel={strings.logs.pagination.newer}
        nextLabel={strings.logs.pagination.older}
        previousDisabled={!hasNewer}
        nextDisabled={!hasOlder}
        disabled={paginationDisabled}
        onPrevious={onNewerPage}
        onNext={onOlderPage}
        onPerPageChange={onPerPageChange}
      />
    </section>
  )
}
