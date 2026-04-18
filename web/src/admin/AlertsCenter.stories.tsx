import { useState } from 'react'

import type { Meta, StoryObj } from '@storybook/react-vite'

import type { AlertCatalog, AlertEvent, AlertGroup, AlertsPage, RequestLogBodies } from '../api'
import AlertsCenter from './AlertsCenter'
import { alertsPath } from './routes'

const now = 1_776_220_800

const catalog: AlertCatalog = {
  retentionDays: 30,
  types: [
    { value: 'upstream_rate_limited_429', count: 2 },
    { value: 'upstream_key_blocked', count: 1 },
    { value: 'user_request_rate_limited', count: 1 },
    { value: 'user_quota_exhausted', count: 2 },
  ],
  requestKindOptions: [
    { key: 'tavily_search', label: 'Tavily Search', protocol_group: 'api', billing_group: 'billable', count: 4 },
    { key: 'mcp_search', label: 'MCP Search', protocol_group: 'mcp', billing_group: 'billable', count: 2 },
  ],
  users: [
    { value: 'usr_alice', label: 'Alice Wang', count: 4 },
    { value: 'usr_bob', label: 'Bob Chen', count: 2 },
  ],
  tokens: [
    { value: 'tok_ops_01', label: 'tok_ops_01', count: 4 },
    { value: 'tok_ops_02', label: 'tok_ops_02', count: 2 },
  ],
  keys: [
    { value: 'key_001', label: 'key_001', count: 3 },
  ],
}

const baseEvents: AlertEvent[] = [
  {
    id: 'alert_evt_001',
    type: 'user_quota_exhausted',
    title: '用户额度耗尽',
    summary: 'Alice Wang 的 Tavily Search 请求触发本地额度上限。',
    occurredAt: now - 120,
    subjectKind: 'user',
    subjectId: 'usr_alice',
    subjectLabel: 'Alice Wang',
    user: { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
    token: { id: 'tok_ops_01', label: 'tok_ops_01' },
    key: null,
    request: { id: 501, method: 'POST', path: '/api/tavily/search', query: null },
    requestKind: { key: 'tavily_search', label: 'Tavily Search', detail: 'POST /api/tavily/search' },
    failureKind: null,
    resultStatus: 'quota_exhausted',
    errorMessage: 'quota exhausted',
    reasonCode: null,
    reasonSummary: null,
    reasonDetail: null,
    source: { kind: 'auth_token_log', id: 'log_501' },
  },
  {
    id: 'alert_evt_002',
    type: 'upstream_rate_limited_429',
    title: '上游返回 429',
    summary: 'key_001 对 tok_ops_01 的 Tavily Search 请求返回 429。',
    occurredAt: now - 360,
    subjectKind: 'user',
    subjectId: 'usr_alice',
    subjectLabel: 'Alice Wang',
    user: { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
    token: { id: 'tok_ops_01', label: 'tok_ops_01' },
    key: { id: 'key_001', label: 'key_001' },
    request: { id: 502, method: 'POST', path: '/api/tavily/search', query: 'max_results=5' },
    requestKind: { key: 'tavily_search', label: 'Tavily Search', detail: 'POST /api/tavily/search' },
    failureKind: 'upstream_rate_limited_429',
    resultStatus: 'error',
    errorMessage: 'HTTP 429 from upstream',
    reasonCode: null,
    reasonSummary: null,
    reasonDetail: null,
    source: { kind: 'auth_token_log', id: 'log_502' },
  },
  {
    id: 'alert_evt_003',
    type: 'upstream_key_blocked',
    title: '上游 Key 封禁',
    summary: 'key_001 因上游账号停用被隔离。',
    occurredAt: now - 720,
    subjectKind: 'key',
    subjectId: 'key_001',
    subjectLabel: 'key_001',
    user: null,
    token: null,
    key: { id: 'key_001', label: 'key_001' },
    request: null,
    requestKind: { key: 'mcp_search', label: 'MCP Search', detail: 'POST /mcp' },
    failureKind: null,
    resultStatus: null,
    errorMessage: null,
    reasonCode: 'account_deactivated',
    reasonSummary: 'Upstream account deactivated',
    reasonDetail: 'The upstream disabled this key.',
    source: { kind: 'api_key_maintenance_record', id: 'maint_503' },
  },
]

const groupsPage: AlertsPage<AlertGroup> = {
  page: 1,
  perPage: 20,
  total: 2,
  items: [
    {
      id: 'group:user_quota_exhausted:user:usr_alice:tavily_search',
      type: 'user_quota_exhausted',
      subjectKind: 'user',
      subjectId: 'usr_alice',
      subjectLabel: 'Alice Wang',
      user: { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
      token: { id: 'tok_ops_01', label: 'tok_ops_01' },
      key: null,
      requestKind: { key: 'tavily_search', label: 'Tavily Search', detail: 'POST /api/tavily/search' },
      count: 2,
      firstSeen: now - 1800,
      lastSeen: now - 120,
      latestEvent: baseEvents[0],
    },
    {
      id: 'group:upstream_key_blocked:key:key_001:mcp_search',
      type: 'upstream_key_blocked',
      subjectKind: 'key',
      subjectId: 'key_001',
      subjectLabel: 'key_001',
      user: null,
      token: null,
      key: { id: 'key_001', label: 'key_001' },
      requestKind: { key: 'mcp_search', label: 'MCP Search', detail: 'POST /mcp' },
      count: 1,
      firstSeen: now - 720,
      lastSeen: now - 720,
      latestEvent: baseEvents[2],
    },
  ],
}

const requestBodies: Record<number, RequestLogBodies> = {
  501: {
    request_body: JSON.stringify({ query: 'quota exhausted', max_results: 5 }, null, 2),
    response_body: JSON.stringify({ error: 'quota exhausted' }, null, 2),
  },
  502: {
    request_body: JSON.stringify({ query: '429', max_results: 5 }, null, 2),
    response_body: JSON.stringify({ status: 429, detail: 'rate limit' }, null, 2),
  },
}

function formatTs(value: number | null): string {
  if (!value) return '—'
  return new Intl.DateTimeFormat('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(value * 1000))
}

export function AlertsCenterStoryShell({ initialSearch = alertsPath() }: { initialSearch?: string }): JSX.Element {
  const [search, setSearch] = useState(initialSearch.replace('/admin/alerts', ''))

  return (
    <div style={{ padding: 24, background: 'hsl(var(--background))' }}>
      <AlertsCenter
        language="zh"
        search={search}
        refreshToken={0}
        onNavigate={setSearch}
        onOpenUser={() => {}}
        onOpenToken={() => {}}
      onOpenKey={() => {}}
      formatTime={formatTs}
      formatTimeDetail={formatTs}
      initialCatalog={catalog}
      initialEventsPage={{ page: 1, perPage: 20, total: baseEvents.length, items: baseEvents }}
      initialGroupsPage={groupsPage}
      disableAutoLoad
      catalogLoader={async () => catalog}
      eventsLoader={async () => ({ page: 1, perPage: 20, total: baseEvents.length, items: baseEvents })}
      groupsLoader={async () => groupsPage}
        requestLoader={async (requestId) => requestBodies[requestId] ?? { request_body: null, response_body: null }}
      />
    </div>
  )
}

const meta = {
  title: 'Admin/Components/AlertsCenter',
  component: AlertsCenterStoryShell,
  tags: ['autodocs'],
  parameters: {
    docs: {
      description: {
        component:
          'Admin alerts center with shared filters, event/group tabs, and inline request detail drawer backed by stable Storybook fixtures.',
      },
    },
  },
} satisfies Meta<typeof AlertsCenterStoryShell>

export default meta

type Story = StoryObj<typeof meta>

export const EventsDefault: Story = {
  args: {
    initialSearch: alertsPath({ view: 'events' }),
  },
}

export const GroupsView: Story = {
  args: {
    initialSearch: alertsPath({ view: 'groups', type: 'upstream_key_blocked', requestKinds: ['mcp_search'] }),
  },
}
