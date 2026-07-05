import type { StoryObj } from '@storybook/react-vite'
import { useState } from 'react'

import type { AlertCatalog, AlertEvent, AlertGroup, AlertsPage } from '../../api'
import AlertsCenter from '../AlertsCenter'
import { alertsPath } from '../routes'
import { useLanguage } from '../../i18n'
import SegmentedTabs from '../../components/ui/SegmentedTabs'
import { AdminPageFrame } from './AdminPagesStoryRuntime'

const STORY_ALERTS_CATALOG: AlertCatalog = {
  retentionDays: 30,
  types: [
    { value: 'upstream_usage_limit_432', count: 2 },
    { value: 'user_request_rate_limited', count: 2 },
  ],
  requestKindOptions: [
    { key: 'tavily_search', label: 'Tavily Search', protocol_group: 'api', billing_group: 'billable', count: 2 },
    { key: 'mcp_initialize', label: 'MCP initialize', protocol_group: 'mcp', billing_group: 'non_billable', count: 1 },
    { key: 'mcp_resources_list', label: 'MCP resources/list', protocol_group: 'mcp', billing_group: 'non_billable', count: 1 },
  ],
  users: [{ value: 'usr_alice', label: 'Alice Wang', count: 2 }],
  tokens: [{ value: 'tok_ops_01', label: 'tok_ops_01', count: 2 }],
  keys: [{ value: 'key_001', label: 'key_001', count: 1 }],
}

function storyTime(timestamp: number | null): string {
  if (timestamp == null) return '—'
  const date = new Date(timestamp * 1000)
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  const hours = String(date.getHours()).padStart(2, '0')
  const minutes = String(date.getMinutes()).padStart(2, '0')
  return `${month}/${day} ${hours}:${minutes}`
}

const STORY_RATE_EVENT_OLD: AlertEvent = {
  id: 'alert_evt_rate_old',
  type: 'user_request_rate_limited',
  title: '用户请求限流',
  summary: 'Token tok_ops_01 was rate limited by the local rolling 5m request-rate window for MCP initialize.',
  occurredAt: 1_762_379_280,
  subjectKind: 'user',
  subjectId: 'usr_alice',
  subjectLabel: 'Alice Wang',
  user: { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
  token: { id: 'tok_ops_01', label: 'tok_ops_01' },
  key: null,
  request: { id: 801, method: 'POST', path: '/mcp', query: null },
  requestKind: { key: 'mcp_initialize', label: 'MCP initialize', detail: 'initialize' },
  failureKind: null,
  resultStatus: 'quota_exhausted',
  errorMessage: 'user request rate limit exceeded on rolling 5m window (limit 25, used 25)',
  reasonCode: null,
  reasonSummary: null,
  reasonDetail: null,
  source: { kind: 'auth_token_log', id: 'alert-rate-old' },
  semanticWindow: {
    kind: 'request_rate',
    windowMinutes: 5,
    windowStart: 1_762_379_100,
    windowEnd: 1_762_379_280,
    windowKey: 'request_rate:story:0',
  },
}

const STORY_RATE_EVENT_NEW: AlertEvent = {
  ...STORY_RATE_EVENT_OLD,
  id: 'alert_evt_rate_new',
  summary: 'Token tok_ops_01 was rate limited by the local rolling 5m request-rate window for MCP resources/list.',
  occurredAt: 1_762_379_340,
  request: { id: 802, method: 'POST', path: '/mcp', query: null },
  requestKind: { key: 'mcp_resources_list', label: 'MCP resources/list', detail: 'resources/list' },
  source: { kind: 'auth_token_log', id: 'alert-rate-new' },
  semanticWindow: {
    kind: 'request_rate',
    windowMinutes: 5,
    windowStart: 1_762_379_160,
    windowEnd: 1_762_379_340,
    windowKey: 'request_rate:story:0',
  },
}

const STORY_USAGE_EVENT_OLD: AlertEvent = {
  id: 'alert_evt_usage_old',
  type: 'upstream_usage_limit_432',
  title: '上游用量限制 432',
  summary: 'Alice Wang 的 Tavily Search 请求命中了上游 Tavily 用量限制。',
  occurredAt: 1_762_379_460,
  subjectKind: 'key',
  subjectId: 'key_001',
  subjectLabel: 'key_001',
  user: { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
  token: { id: 'tok_ops_01', label: 'tok_ops_01' },
  key: { id: 'key_001', label: 'key_001' },
  request: { id: 901, method: 'POST', path: '/api/tavily/search', query: 'max_results=5' },
  requestKind: { key: 'tavily_search', label: 'Tavily Search', detail: 'POST /api/tavily/search' },
  failureKind: null,
  resultStatus: 'quota_exhausted',
  errorMessage: "This request exceeds your plan's set usage limit.",
  reasonCode: null,
  reasonSummary: null,
  reasonDetail: null,
  source: { kind: 'auth_token_log', id: 'alert-usage-old' },
  semanticWindow: null,
}

const STORY_USAGE_EVENT_NEW: AlertEvent = {
  ...STORY_USAGE_EVENT_OLD,
  id: 'alert_evt_usage_new',
  summary: 'Alice Wang 的 Tavily Search 请求继续命中上游 Tavily 用量限制。',
  occurredAt: 1_762_379_580,
  request: { id: 902, method: 'POST', path: '/api/tavily/search', query: 'max_results=5' },
  source: { kind: 'auth_token_log', id: 'alert-usage-new' },
}

const STORY_ALERTS_EVENTS_PAGE: AlertsPage<AlertEvent> = {
  page: 1,
  perPage: 20,
  total: 4,
  items: [STORY_USAGE_EVENT_NEW, STORY_USAGE_EVENT_OLD, STORY_RATE_EVENT_NEW, STORY_RATE_EVENT_OLD],
}

const STORY_ALERTS_GROUPS_PAGE: AlertsPage<AlertGroup> = {
  page: 1,
  perPage: 20,
  total: 2,
  items: [
    {
      id: 'group:user_request_rate_limited:user:usr_alice:request_rate:mother:0',
      type: 'user_request_rate_limited',
      subjectKind: 'user',
      subjectId: 'usr_alice',
      subjectLabel: 'Alice Wang',
      user: { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
      token: { id: 'tok_ops_01', label: 'tok_ops_01' },
      key: null,
      requestKind: null,
      count: 2,
      firstSeen: STORY_RATE_EVENT_OLD.occurredAt,
      lastSeen: STORY_RATE_EVENT_NEW.occurredAt,
      latestEvent: STORY_RATE_EVENT_NEW,
      groupingKind: 'mother',
      semanticWindowKind: 'request_rate',
      semanticWindowMinutes: 5,
      semanticWindowStart: STORY_RATE_EVENT_OLD.semanticWindow?.windowStart ?? null,
      semanticWindowEnd: STORY_RATE_EVENT_NEW.semanticWindow?.windowEnd ?? null,
      semanticWindowKey: STORY_RATE_EVENT_NEW.semanticWindow?.windowKey ?? null,
      childCount: 1,
      eventCount: 2,
      children: [
        {
          id: 'group:user_request_rate_limited:user:usr_alice:request_rate:child:0',
          type: 'user_request_rate_limited',
          subjectKind: 'user',
          subjectId: 'usr_alice',
          subjectLabel: 'Alice Wang',
          user: { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
          token: { id: 'tok_ops_01', label: 'tok_ops_01' },
          key: null,
          requestKind: null,
          count: 2,
          firstSeen: STORY_RATE_EVENT_OLD.occurredAt,
          lastSeen: STORY_RATE_EVENT_NEW.occurredAt,
          latestEvent: STORY_RATE_EVENT_NEW,
          groupingKind: 'child',
          semanticWindowKind: 'request_rate',
          semanticWindowMinutes: 5,
          semanticWindowStart: STORY_RATE_EVENT_OLD.semanticWindow?.windowStart ?? null,
          semanticWindowEnd: STORY_RATE_EVENT_NEW.semanticWindow?.windowEnd ?? null,
          semanticWindowKey: STORY_RATE_EVENT_NEW.semanticWindow?.windowKey ?? null,
          childCount: 0,
          eventCount: 2,
          children: [],
          childEvents: [STORY_RATE_EVENT_NEW, STORY_RATE_EVENT_OLD],
        },
      ],
      childEvents: [],
    },
    {
      id: 'group:upstream_usage_limit_432:key:key_001:tavily_search',
      type: 'upstream_usage_limit_432',
      subjectKind: 'key',
      subjectId: 'key_001',
      subjectLabel: 'key_001',
      user: { userId: 'usr_alice', displayName: 'Alice Wang', username: 'alice' },
      token: { id: 'tok_ops_01', label: 'tok_ops_01' },
      key: { id: 'key_001', label: 'key_001' },
      requestKind: { key: 'tavily_search', label: 'Tavily Search', detail: 'POST /api/tavily/search' },
      count: 2,
      firstSeen: STORY_USAGE_EVENT_OLD.occurredAt,
      lastSeen: STORY_USAGE_EVENT_NEW.occurredAt,
      latestEvent: STORY_USAGE_EVENT_NEW,
      groupingKind: 'compat',
      semanticWindowKind: null,
      semanticWindowMinutes: null,
      semanticWindowStart: null,
      semanticWindowEnd: null,
      semanticWindowKey: null,
      childCount: 0,
      eventCount: 2,
      children: [],
      childEvents: [],
    },
  ],
}

function AlertsPageCanvas({ inlineTabsVariant = 'all' }: { inlineTabsVariant?: 'all' | 'mobile' } = {}): JSX.Element {
  const { language } = useLanguage()
  const [search, setSearch] = useState(alertsPath({ view: 'groups' }).replace('/admin/alerts', ''))
  const currentView = new URLSearchParams(search.startsWith('?') ? search.slice(1) : search).get('view') === 'events'
    ? 'events'
    : 'groups'
  const headerTabs = inlineTabsVariant === 'all' ? (
    <SegmentedTabs<'events' | 'groups'>
      className="alerts-center-tabs alerts-center-tabs--header"
      value={currentView}
      onChange={(nextView) => setSearch(alertsPath({ view: nextView }).replace('/admin/alerts', ''))}
      options={[
        { value: 'groups', label: language === 'zh' ? '聚合告警' : 'Groups' },
        { value: 'events', label: language === 'zh' ? '事件记录' : 'Events' },
      ]}
      ariaLabel="告警视图"
    />
  ) : null

  return (
    <AdminPageFrame activeModule="alerts" actions={headerTabs ?? undefined}>
      <AlertsCenter
        language={language}
        search={search}
        refreshToken={0}
        onNavigate={(path) => setSearch(path.replace('/admin/alerts', ''))}
        onOpenUser={() => {}}
        onOpenToken={() => {}}
        onOpenKey={() => {}}
        formatTime={storyTime}
        formatTimeDetail={storyTime}
        initialCatalog={STORY_ALERTS_CATALOG}
        initialEventsPage={STORY_ALERTS_EVENTS_PAGE}
        initialGroupsPage={STORY_ALERTS_GROUPS_PAGE}
        disableAutoLoad
        inlineTabsVariant={inlineTabsVariant}
      />
    </AdminPageFrame>
  )
}

type Story = StoryObj

export const Alerts: Story = {
  render: () => <AlertsPageCanvas />,
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
}

export const AlertsMobile: Story = {
  render: () => <AlertsPageCanvas inlineTabsVariant="mobile" />,
  parameters: {
    viewport: { defaultViewport: '375-mobile' },
  },
}
