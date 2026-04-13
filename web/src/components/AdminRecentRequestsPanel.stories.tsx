import type { Meta, StoryObj } from '@storybook/react-vite'
import { useCallback, useLayoutEffect, useMemo, useState } from 'react'

import type { RequestLog, RequestLogBodies } from '../api'
import { useLanguage, useTranslate } from '../i18n'
import type { TokenLogRequestKindOption } from '../tokenLogRequestKinds'
import AdminRecentRequestsPanel from './AdminRecentRequestsPanel'

const storyLogs: RequestLog[] = [
  {
    id: 7001,
    key_id: 'K001',
    auth_token_id: 'T001',
    method: 'POST',
    path: '/api/tavily/search',
    query: null,
    http_status: 200,
    mcp_status: 200,
    business_credits: 2,
    request_kind_key: 'api:search',
    request_kind_label: 'API | search',
    request_kind_detail: null,
    result_status: 'success',
    created_at: 1_774_693_640,
    error_message: null,
    key_effect_code: 'none',
    key_effect_summary: null,
    binding_effect_code: 'http_project_affinity_bound',
    binding_effect_summary: 'The system created a new upstream key binding for this project',
    selection_effect_code: 'http_project_affinity_pressure_avoided',
    selection_effect_summary: 'Project affinity routing avoided a key under higher recent pressure',
    request_body: null,
    response_body: null,
    forwarded_headers: ['x-request-id', 'x-forwarded-for'],
    dropped_headers: ['authorization'],
    operationalClass: 'success',
    requestKindProtocolGroup: 'api',
    requestKindBillingGroup: 'billable',
  },
  {
    id: 7002,
    key_id: 'K002',
    auth_token_id: 'T002',
    method: 'POST',
    path: '/mcp',
    query: null,
    http_status: 200,
    mcp_status: 202,
    business_credits: null,
    request_kind_key: 'mcp:notifications/initialized',
    request_kind_label: 'MCP | notifications/initialized',
    request_kind_detail: null,
    result_status: 'success',
    created_at: 1_774_693_580,
    error_message: null,
    key_effect_code: 'mcp_session_init_backoff_set',
    key_effect_summary: 'The system armed a temporary backoff for MCP session initialization',
    binding_effect_code: 'none',
    binding_effect_summary: null,
    selection_effect_code: 'mcp_session_init_pressure_avoided',
    selection_effect_summary: 'Initialization avoided a key under higher recent pressure',
    gateway_mode: 'rebalance_http',
    experiment_variant: 'rebalance',
    proxy_session_id: 'rebalance-session-7002',
    routing_subject_hash: '9f2ab31c0d44e912',
    upstream_operation: 'mcp',
    fallback_reason: null,
    request_body: null,
    response_body: null,
    forwarded_headers: ['x-request-id'],
    dropped_headers: [],
    operationalClass: 'neutral',
    requestKindProtocolGroup: 'mcp',
    requestKindBillingGroup: 'non_billable',
  },
  {
    id: 7003,
    key_id: 'K003',
    auth_token_id: 'T003',
    method: 'POST',
    path: '/api/tavily/extract',
    query: null,
    http_status: 200,
    mcp_status: 200,
    business_credits: 3,
    request_kind_key: 'api:extract',
    request_kind_label: 'API | extract',
    request_kind_detail: null,
    result_status: 'success',
    created_at: 1_774_693_520,
    error_message: null,
    key_effect_code: 'restored_active',
    key_effect_summary: 'The system automatically restored this key to active',
    binding_effect_code: 'none',
    binding_effect_summary: null,
    selection_effect_code: 'none',
    selection_effect_summary: null,
    gateway_mode: 'rebalance_http',
    experiment_variant: 'rebalance',
    proxy_session_id: 'rebalance-session-7004',
    routing_subject_hash: '9f2ab31c0d44e912',
    upstream_operation: 'http_search',
    fallback_reason: 'upstream_http_error',
    request_body: null,
    response_body: null,
    forwarded_headers: ['x-request-id'],
    dropped_headers: [],
    operationalClass: 'success',
    requestKindProtocolGroup: 'api',
    requestKindBillingGroup: 'billable',
  },
  {
    id: 7004,
    key_id: 'K004',
    auth_token_id: 'T004',
    method: 'POST',
    path: '/mcp',
    query: null,
    http_status: 200,
    mcp_status: 401,
    business_credits: null,
    request_kind_key: 'mcp:crawl',
    request_kind_label: 'MCP | crawl',
    request_kind_detail: null,
    result_status: 'error',
    created_at: 1_774_693_460,
    error_message: 'The account associated with this API key has been deactivated.',
    failure_kind: 'upstream_account_deactivated_401',
    key_effect_code: 'quarantined',
    key_effect_summary: 'Automatically quarantined this key',
    binding_effect_code: 'none',
    binding_effect_summary: null,
    selection_effect_code: 'none',
    selection_effect_summary: null,
    request_body: null,
    response_body: null,
    forwarded_headers: ['x-request-id'],
    dropped_headers: [],
    operationalClass: 'upstream_error',
    requestKindProtocolGroup: 'mcp',
    requestKindBillingGroup: 'billable',
  },
  {
    id: 7005,
    key_id: 'K005',
    auth_token_id: 'T005',
    method: 'POST',
    path: '/api/tavily/map',
    query: null,
    http_status: 200,
    mcp_status: 200,
    business_credits: 2,
    request_kind_key: 'api:map',
    request_kind_label: 'API | map',
    request_kind_detail: null,
    result_status: 'success',
    created_at: 1_774_693_400,
    error_message: null,
    key_effect_code: 'none',
    key_effect_summary: null,
    binding_effect_code: 'http_project_affinity_reused',
    binding_effect_summary: 'The system reused the current upstream key binding for this project',
    selection_effect_code: 'none',
    selection_effect_summary: null,
    request_body: null,
    response_body: null,
    forwarded_headers: ['x-request-id'],
    dropped_headers: [],
    operationalClass: 'success',
    requestKindProtocolGroup: 'api',
    requestKindBillingGroup: 'billable',
  },
]

const storyBodiesById = new Map<number, RequestLogBodies>([
  [7001, { request_body: '{"query":"site reliability"}', response_body: '{"answer":"ok"}' }],
  [7002, { request_body: null, response_body: null }],
  [7003, { request_body: '{"urls":["https://example.com"]}', response_body: '{"status":"queued"}' }],
])

const requestKindOptions: TokenLogRequestKindOption[] = [
  { key: 'api:extract', label: 'API | extract', protocol_group: 'api', billing_group: 'billable', count: 1 },
  { key: 'api:map', label: 'API | map', protocol_group: 'api', billing_group: 'billable', count: 1 },
  { key: 'api:search', label: 'API | search', protocol_group: 'api', billing_group: 'billable', count: 1 },
  {
    key: 'mcp:notifications/initialized',
    label: 'MCP | notifications/initialized',
    protocol_group: 'mcp',
    billing_group: 'non_billable',
    count: 1,
  },
  {
    key: 'mcp:crawl',
    label: 'MCP | crawl',
    protocol_group: 'mcp',
    billing_group: 'billable',
    count: 1,
  },
]

function buildFacetOptions(values: Array<string | null | undefined>) {
  const counts = new Map<string, number>()
  for (const raw of values) {
    const value = raw?.trim()
    if (!value) continue
    counts.set(value, (counts.get(value) ?? 0) + 1)
  }
  return Array.from(counts.entries())
    .sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))
    .map(([value, count]) => ({ value, count }))
}

function LazyDetailsStateGallery(): JSX.Element {
  const admin = useTranslate().admin
  const { language } = useLanguage()
  const [selectedRequestKinds, setSelectedRequestKinds] = useState<string[]>([])

  const loadLogBodies = useCallback((log: RequestLog, signal: AbortSignal) => {
    if (log.id === 7003) {
      return new Promise<RequestLogBodies>((resolve, reject) => {
        const timer = window.setTimeout(() => resolve(storyBodiesById.get(log.id) ?? { request_body: null, response_body: null }), 20_000)
        signal.addEventListener(
          'abort',
          () => {
            window.clearTimeout(timer)
            reject(new DOMException('The operation was aborted.', 'AbortError'))
          },
          { once: true },
        )
      })
    }
    if (log.id === 7004) {
      return Promise.reject(new Error('Upstream detail fetch timed out.'))
    }
    return Promise.resolve(storyBodiesById.get(log.id) ?? { request_body: null, response_body: null })
  }, [])

  const facets = useMemo(
    () => ({
      results: buildFacetOptions(storyLogs.map((log) => log.result_status)),
      keyEffects: buildFacetOptions(storyLogs.map((log) => log.key_effect_code ?? 'none')),
      bindingEffects: buildFacetOptions(storyLogs.map((log) => log.binding_effect_code ?? 'none')),
      selectionEffects: buildFacetOptions(storyLogs.map((log) => log.selection_effect_code ?? 'none')),
      tokens: buildFacetOptions(storyLogs.map((log) => log.auth_token_id)),
      keys: buildFacetOptions(storyLogs.map((log) => log.key_id)),
    }),
    [],
  )

  useLayoutEffect(() => {
    const timer = window.setTimeout(() => {
      for (const id of [7001, 7002, 7003, 7004]) {
        const trigger = document.querySelector<HTMLButtonElement>(`[aria-controls="recent-request-details-${id}"]`)
        trigger?.click()
      }
    }, 80)
    return () => window.clearTimeout(timer)
  }, [])

  return (
    <div style={{ maxWidth: 1480, margin: '0 auto', padding: 24 }}>
      <AdminRecentRequestsPanel
        variant="admin"
        language={language}
        strings={admin}
        title="Request Details Lazy Loading"
        description="Collapsed, loaded, no-body, loading, and retryable error states shown together."
        emptyLabel="No logs."
        loadState="ready"
        loadingLabel="Loading…"
        logs={storyLogs}
        requestKindOptions={requestKindOptions}
        requestKindQuickBilling="all"
        requestKindQuickProtocol="all"
        selectedRequestKinds={selectedRequestKinds}
        onRequestKindQuickFiltersChange={() => undefined}
        onToggleRequestKind={(key) =>
          setSelectedRequestKinds((current) =>
            current.includes(key) ? current.filter((value) => value !== key) : [...current, key],
          )
        }
        onClearRequestKinds={() => setSelectedRequestKinds([])}
        outcomeFilter={null}
        resultOptions={facets.results}
        keyEffectOptions={facets.keyEffects}
        bindingEffectOptions={facets.bindingEffects}
        selectionEffectOptions={facets.selectionEffects}
        onOutcomeFilterChange={() => undefined}
        keyOptions={facets.keys}
        selectedKeyId={null}
        onKeyFilterChange={() => undefined}
        showKeyColumn
        showTokenColumn
        perPage={20}
        hasOlder
        hasNewer={false}
        paginationSummary={admin.logs.pagination.summaryWithRetention.replace('{days}', '32')}
        onNewerPage={() => undefined}
        onOlderPage={() => undefined}
        onPerPageChange={() => undefined}
        formatTime={(ts) => new Date((ts ?? 0) * 1000).toLocaleString(language === 'zh' ? 'zh-CN' : 'en-US')}
        formatTimeDetail={(ts) => new Date((ts ?? 0) * 1000).toISOString()}
        onOpenKey={() => undefined}
        onOpenToken={() => undefined}
        loadLogBodies={loadLogBodies}
      />
    </div>
  )
}

const alignmentStoryLogs: RequestLog[] = [
  {
    id: 8101,
    key_id: 'bab3',
    auth_token_id: 'cBtp',
    method: 'POST',
    path: '/mcp',
    query: null,
    http_status: 200,
    mcp_status: 429,
    business_credits: null,
    request_kind_key: 'mcp:extract',
    request_kind_label: 'MCP | extract',
    request_kind_detail: null,
    result_status: 'error',
    created_at: 1_775_438_268,
    error_message: 'Quota exhausted',
    key_effect_code: 'none',
    key_effect_summary: null,
    request_body: null,
    response_body: null,
    forwarded_headers: ['x-request-id'],
    dropped_headers: [],
    operationalClass: 'upstream_error',
    requestKindProtocolGroup: 'mcp',
    requestKindBillingGroup: 'billable',
  },
  {
    id: 8102,
    key_id: 'EGsl',
    auth_token_id: 'ZjvC',
    method: 'POST',
    path: '/mcp',
    query: null,
    http_status: 200,
    mcp_status: 429,
    business_credits: null,
    request_kind_key: 'mcp:search',
    request_kind_label: 'MCP | search',
    request_kind_detail: null,
    result_status: 'error',
    created_at: 1_775_438_146,
    error_message: 'Quota exhausted',
    key_effect_code: 'none',
    key_effect_summary: null,
    request_body: null,
    response_body: null,
    forwarded_headers: ['x-request-id'],
    dropped_headers: [],
    operationalClass: 'upstream_error',
    requestKindProtocolGroup: 'mcp',
    requestKindBillingGroup: 'billable',
  },
]

function IdentifierAlignmentShowcase(): JSX.Element {
  const admin = useTranslate().admin
  const { language } = useLanguage()

  const facets = useMemo(
    () => ({
      results: buildFacetOptions(alignmentStoryLogs.map((log) => log.result_status)),
      keyEffects: buildFacetOptions(alignmentStoryLogs.map((log) => log.key_effect_code ?? 'none')),
      bindingEffects: buildFacetOptions(alignmentStoryLogs.map((log) => log.binding_effect_code ?? 'none')),
      selectionEffects: buildFacetOptions(alignmentStoryLogs.map((log) => log.selection_effect_code ?? 'none')),
      tokens: buildFacetOptions(alignmentStoryLogs.map((log) => log.auth_token_id)),
      keys: buildFacetOptions(alignmentStoryLogs.map((log) => log.key_id)),
    }),
    [],
  )

  return (
    <div style={{ maxWidth: 1480, margin: '0 auto', padding: 24 }}>
      <AdminRecentRequestsPanel
        variant="admin"
        language={language}
        strings={admin}
        title="Identifier Alignment"
        description="Stable desktop canvas for validating token and key identifiers sit visually centered within the row."
        emptyLabel="No logs."
        loadState="ready"
        loadingLabel="Loading…"
        logs={alignmentStoryLogs}
        requestKindOptions={[
          { key: 'mcp:extract', label: 'MCP | extract', protocol_group: 'mcp', billing_group: 'billable', count: 1 },
          { key: 'mcp:search', label: 'MCP | search', protocol_group: 'mcp', billing_group: 'billable', count: 1 },
        ]}
        requestKindQuickBilling="all"
        requestKindQuickProtocol="all"
        selectedRequestKinds={[]}
        onRequestKindQuickFiltersChange={() => undefined}
        onToggleRequestKind={() => undefined}
        onClearRequestKinds={() => undefined}
        outcomeFilter={{ kind: 'result', value: 'error' }}
        resultOptions={facets.results}
        keyEffectOptions={facets.keyEffects}
        bindingEffectOptions={facets.bindingEffects}
        selectionEffectOptions={facets.selectionEffects}
        onOutcomeFilterChange={() => undefined}
        keyOptions={facets.keys}
        selectedKeyId={null}
        onKeyFilterChange={() => undefined}
        showKeyColumn
        showTokenColumn
        perPage={20}
        hasOlder
        hasNewer
        paginationSummary={admin.logs.pagination.summary}
        onNewerPage={() => undefined}
        onOlderPage={() => undefined}
        onPerPageChange={() => undefined}
        formatTime={(ts) => new Date((ts ?? 0) * 1000).toLocaleString(language === 'zh' ? 'zh-CN' : 'en-US')}
        formatTimeDetail={(ts) => new Date((ts ?? 0) * 1000).toISOString()}
        onOpenKey={() => undefined}
        onOpenToken={() => undefined}
        loadLogBodies={() => Promise.resolve({ request_body: null, response_body: null })}
      />
    </div>
  )
}

function CatalogLoadingShowcase(): JSX.Element {
  const admin = useTranslate().admin
  const { language } = useLanguage()

  return (
    <div style={{ maxWidth: 1480, margin: '0 auto', padding: 24 }}>
      <AdminRecentRequestsPanel
        variant="admin"
        language={language}
        strings={admin}
        title={admin.logs.title}
        description={admin.logs.descriptionFallback}
        emptyLabel={admin.logs.empty.none}
        loadState="ready"
        loadingLabel={admin.logs.empty.loading}
        logs={storyLogs.slice(0, 3)}
        requestKindOptions={[]}
        requestKindQuickBilling="all"
        requestKindQuickProtocol="all"
        selectedRequestKinds={[]}
        onRequestKindQuickFiltersChange={() => undefined}
        onToggleRequestKind={() => undefined}
        onClearRequestKinds={() => undefined}
        outcomeFilter={null}
        resultOptions={[]}
        keyEffectOptions={[]}
        onOutcomeFilterChange={() => undefined}
        keyOptions={[]}
        selectedKeyId={null}
        onKeyFilterChange={() => undefined}
        showKeyColumn
        showTokenColumn
        perPage={20}
        hasOlder
        hasNewer={false}
        paginationSummary={admin.logs.pagination.summary}
        onNewerPage={() => undefined}
        onOlderPage={() => undefined}
        onPerPageChange={() => undefined}
        formatTime={(ts) => new Date((ts ?? 0) * 1000).toLocaleString(language === 'zh' ? 'zh-CN' : 'en-US')}
        formatTimeDetail={(ts) => new Date((ts ?? 0) * 1000).toISOString()}
        onOpenKey={() => undefined}
        onOpenToken={() => undefined}
        loadLogBodies={() => Promise.resolve({ request_body: null, response_body: null })}
      />
    </div>
  )
}

function EmptyStateShowcase(): JSX.Element {
  const admin = useTranslate().admin
  const { language } = useLanguage()

  return (
    <div style={{ maxWidth: 1480, margin: '0 auto', padding: 24 }}>
      <AdminRecentRequestsPanel
        variant="admin"
        language={language}
        strings={admin}
        title={admin.logs.title}
        description={admin.logs.descriptionWithRetention.replace('{days}', '32')}
        emptyLabel={admin.logs.empty.none}
        loadState="ready"
        loadingLabel={admin.logs.empty.loading}
        logs={[]}
        requestKindOptions={requestKindOptions}
        requestKindQuickBilling="all"
        requestKindQuickProtocol="all"
        selectedRequestKinds={[]}
        onRequestKindQuickFiltersChange={() => undefined}
        onToggleRequestKind={() => undefined}
        onClearRequestKinds={() => undefined}
        outcomeFilter={null}
        resultOptions={[]}
        keyEffectOptions={[]}
        onOutcomeFilterChange={() => undefined}
        keyOptions={[]}
        selectedKeyId={null}
        onKeyFilterChange={() => undefined}
        showKeyColumn
        showTokenColumn
        perPage={20}
        hasOlder={false}
        hasNewer={false}
        paginationSummary={admin.logs.pagination.summaryWithRetention.replace('{days}', '32')}
        onNewerPage={() => undefined}
        onOlderPage={() => undefined}
        onPerPageChange={() => undefined}
        formatTime={(ts) => new Date((ts ?? 0) * 1000).toLocaleString(language === 'zh' ? 'zh-CN' : 'en-US')}
        formatTimeDetail={(ts) => new Date((ts ?? 0) * 1000).toISOString()}
        onOpenKey={() => undefined}
        onOpenToken={() => undefined}
        loadLogBodies={() => Promise.resolve({ request_body: null, response_body: null })}
      />
    </div>
  )
}

function ErrorStateShowcase(): JSX.Element {
  const admin = useTranslate().admin
  const { language } = useLanguage()

  return (
    <div style={{ maxWidth: 1480, margin: '0 auto', padding: 24 }}>
      <AdminRecentRequestsPanel
        variant="admin"
        language={language}
        strings={admin}
        title={admin.logs.title}
        description={admin.logs.descriptionWithRetention.replace('{days}', '32')}
        emptyLabel={admin.logs.empty.none}
        loadState="error"
        loadingLabel={admin.logs.empty.loading}
        errorLabel={language === 'zh' ? '加载近期请求失败。' : 'Failed to load recent requests.'}
        logs={[]}
        requestKindOptions={requestKindOptions}
        requestKindQuickBilling="all"
        requestKindQuickProtocol="all"
        selectedRequestKinds={[]}
        onRequestKindQuickFiltersChange={() => undefined}
        onToggleRequestKind={() => undefined}
        onClearRequestKinds={() => undefined}
        outcomeFilter={null}
        resultOptions={[]}
        keyEffectOptions={[]}
        onOutcomeFilterChange={() => undefined}
        keyOptions={[]}
        selectedKeyId={null}
        onKeyFilterChange={() => undefined}
        showKeyColumn
        showTokenColumn
        perPage={20}
        hasOlder={false}
        hasNewer={false}
        paginationSummary={admin.logs.pagination.summaryWithRetention.replace('{days}', '32')}
        onNewerPage={() => undefined}
        onOlderPage={() => undefined}
        onPerPageChange={() => undefined}
        formatTime={(ts) => new Date((ts ?? 0) * 1000).toLocaleString(language === 'zh' ? 'zh-CN' : 'en-US')}
        formatTimeDetail={(ts) => new Date((ts ?? 0) * 1000).toISOString()}
        onOpenKey={() => undefined}
        onOpenToken={() => undefined}
        loadLogBodies={() => Promise.resolve({ request_body: null, response_body: null })}
      />
    </div>
  )
}

const meta = {
  title: 'Admin/Components/AdminRecentRequestsPanel',
  component: LazyDetailsStateGallery,
  tags: ['autodocs'],
  parameters: {
    layout: 'fullscreen',
    docs: {
      description: {
        component:
          '共享日志面板的懒加载详情状态画廊，固定展示 collapsed / loading / loaded / no-body / error+retry 五种展开态。',
      },
    },
  },
} satisfies Meta<typeof LazyDetailsStateGallery>

export default meta

type Story = StoryObj<typeof meta>

export const LazyDetailsGallery: Story = {
  globals: {
    language: 'zh',
    themeMode: 'dark',
  },
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 250))
    const text = canvasElement.ownerDocument.body.textContent ?? ''
    for (const expected of [
      '未捕获内容。',
      '正在加载请求详情…',
      '加载请求详情失败。',
      '重试',
      'site reliability',
    ]) {
      if (!text.includes(expected)) {
        throw new Error(`Expected lazy detail gallery to contain: ${expected}`)
      }
    }
  },
}

export const IdentifierAlignment: Story = {
  render: () => <IdentifierAlignmentShowcase />,
  globals: {
    language: 'zh',
    themeMode: 'dark',
  },
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
    docs: {
      description: {
        story: '用于稳定验收桌面表格中 Token / Key 标识链接的视觉垂直居中效果。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 150))
    const text = canvasElement.ownerDocument.body.textContent ?? ''
    for (const expected of ['bab3', 'cBtp', 'EGsl', 'ZjvC']) {
      if (!text.includes(expected)) {
        throw new Error(`Expected identifier alignment canvas to contain: ${expected}`)
      }
    }
  },
}

export const CatalogLoading: Story = {
  render: () => <CatalogLoadingShowcase />,
  globals: {
    language: 'zh',
    themeMode: 'dark',
  },
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
    docs: {
      description: {
        story: '当 list 已经返回、catalog 尚未补齐时的安全兜底态，不展示保留天数数字。',
      },
    },
  },
}

export const EmptyState: Story = {
  render: () => <EmptyStateShowcase />,
  globals: {
    language: 'zh',
    themeMode: 'dark',
  },
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const ErrorState: Story = {
  render: () => <ErrorStateShowcase />,
  globals: {
    language: 'zh',
    themeMode: 'dark',
  },
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}
