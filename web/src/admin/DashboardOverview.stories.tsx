import type { Meta, StoryObj } from '@storybook/react-vite'

import DashboardOverview from './DashboardOverview'

const meta = {
  title: 'Admin/Components/DashboardOverview',
  component: DashboardOverview,
  decorators: [
    (Story) => (
      <div style={{ padding: 24, background: '#eef3fb' }}>
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof DashboardOverview>

export default meta

type Story = StoryObj<typeof meta>

const strings = {
  title: 'Operations Dashboard',
  description: 'Global health, risk signals, and actionable activity in one place.',
  loading: 'Loading dashboard data…',
  summaryUnavailable: 'Unable to load the summary windows right now.',
  statusUnavailable: 'Unable to load the current site status right now.',
  todayTitle: 'Today',
  todayDescription: 'Core request signals up to now, directly compared with yesterday.',
  monthTitle: 'This Month',
  monthDescription: 'Month-to-date request totals in one compact view.',
  currentStatusTitle: 'Current Site Status',
  currentStatusDescription: 'Live quota, active keys, and pool health right now.',
  deltaFromYesterday: 'vs yesterday',
  deltaNoBaseline: 'No yesterday baseline',
  asOfNow: 'Up to now',
  todayShare: 'Today share',
  monthToDate: 'Month to date',
  monthShare: 'Month share',
  trendsTitle: 'Traffic Trends',
  trendsDescription: 'Recent request and error changes from latest logs.',
  requestTrend: 'Request volume',
  errorTrend: 'Error volume',
  riskTitle: 'Risk Watchlist',
  riskDescription: 'Items that may require operator action soon.',
  riskEmpty: 'No active risk signals detected.',
  actionsTitle: 'Action Center',
  actionsDescription: 'Recent events you can jump into quickly.',
  recentRequests: 'Recent requests',
  recentJobs: 'Recent jobs',
  openModule: 'Open',
  openToken: 'Open token',
  openKey: 'Open key',
  disabledTokenRisk: 'Token {id} is disabled',
  exhaustedKeyRisk: 'API key {id} is exhausted',
  failedJobRisk: 'Job #{id} status: {status}',
  tokenCoverageTruncated: 'Token scope is truncated.',
  tokenCoverageError: 'Token scope failed to load.',
}

const todayMetrics = [
  {
    id: 'today-total',
    label: 'Total Requests',
    value: '4,812',
    subtitle: 'Up to now',
    comparison: { label: 'vs yesterday', value: '+426 (+9.7%)', direction: 'up' as const },
  },
  {
    id: 'today-success',
    label: 'Successful',
    value: '4,192',
    subtitle: 'Today share · 87.1%',
    comparison: { label: 'vs yesterday', value: '+318 (+8.2%)', direction: 'up' as const },
  },
  {
    id: 'today-errors',
    label: 'Errors',
    value: '451',
    subtitle: 'Today share · 9.4%',
    comparison: { label: 'vs yesterday', value: '-42 (-8.5%)', direction: 'down' as const },
  },
  {
    id: 'today-quota',
    label: 'Quota Exhausted',
    value: '169',
    subtitle: 'Today share · 3.5%',
    comparison: { label: 'vs yesterday', value: '+150 · No yesterday baseline', direction: 'up' as const },
  },
]

const monthMetrics = [
  { id: 'month-total', label: 'Total Requests', value: '105,041', subtitle: 'Month to date' },
  { id: 'month-success', label: 'Successful', value: '86,279', subtitle: 'Month share · 82.1%' },
  { id: 'month-errors', label: 'Errors', value: '2,368', subtitle: 'Month share · 2.3%' },
  { id: 'month-quota', label: 'Quota Exhausted', value: '0', subtitle: 'Month share · 0%' },
  { id: 'month-new-keys', label: 'New Keys', value: '3', subtitle: 'Added this month' },
  { id: 'month-new-quarantines', label: 'New Quarantines', value: '0', subtitle: 'Added this month' },
]

const statusMetrics = [
  { id: 'remaining', label: 'Remaining', value: '49,482', subtitle: 'Current snapshot · 88.4%' },
  { id: 'keys', label: 'Active Keys', value: '57', subtitle: 'Current snapshot' },
  { id: 'quarantined', label: 'Quarantined', value: '59', subtitle: 'Needs manual review' },
  { id: 'exhausted', label: 'Exhausted', value: '0', subtitle: '0 exhausted' },
  { id: 'proxy-available', label: 'Available Proxy Nodes', value: '12', subtitle: 'Current snapshot · 85.7%' },
  { id: 'proxy-total', label: 'Proxy Nodes Total', value: '14', subtitle: 'Current snapshot' },
]

export const Default: Story = {
  args: {
    strings,
    overviewReady: true,
    statusLoading: false,
    todayMetrics,
    monthMetrics,
    statusMetrics,
    trend: {
      request: [4, 7, 8, 5, 9, 11, 12, 10],
      error: [1, 1, 2, 1, 2, 3, 2, 1],
    },
    tokenCoverage: 'ok',
    tokens: [
      {
        id: '9vsN',
        enabled: false,
        note: 'ops',
        group: 'ops',
        total_requests: 42,
        created_at: 0,
        last_used_at: 0,
        quota_state: 'normal',
        quota_hourly_used: 1,
        quota_hourly_limit: 100,
        quota_daily_used: 5,
        quota_daily_limit: 1000,
        quota_monthly_used: 20,
        quota_monthly_limit: 5000,
        quota_hourly_reset_at: null,
        quota_daily_reset_at: null,
        quota_monthly_reset_at: null,
      },
    ],
    keys: [
      {
        id: 'MZli',
        status: 'exhausted',
        group: 'ops',
        registration_ip: '8.8.8.8',
        registration_region: 'US',
        status_changed_at: 0,
        last_used_at: 0,
        deleted_at: null,
        quota_limit: 1000,
        quota_remaining: 0,
        quota_synced_at: 0,
        total_requests: 111,
        success_count: 88,
        error_count: 23,
        quota_exhausted_count: 11,
        quarantine: null,
      },
    ],
    logs: [
      { id: 1, key_id: 'MZli', auth_token_id: '9vsN', method: 'POST', path: '/mcp', query: null, http_status: 200, mcp_status: 0, result_status: 'success', created_at: 1, error_message: null, request_body: null, response_body: null, forwarded_headers: [], dropped_headers: [] },
      { id: 2, key_id: 'MZli', auth_token_id: '9vsN', method: 'POST', path: '/mcp', query: null, http_status: 429, mcp_status: -1, result_status: 'quota_exhausted', created_at: 2, error_message: 'quota', request_body: null, response_body: null, forwarded_headers: [], dropped_headers: [] },
    ],
    jobs: [
      { id: 1, job_type: 'quota_sync', key_id: 'MZli', key_group: 'ops', status: 'error', attempt: 2, message: 'rate limit', started_at: 1, finished_at: 2 },
      { id: 2, job_type: 'quota_sync', key_id: 'MZli', key_group: 'ops', status: 'success', attempt: 1, message: null, started_at: 3, finished_at: 4 },
    ],
    onOpenModule: () => {},
    onOpenToken: () => {},
    onOpenKey: () => {},
  },
}

export const QuarantineState: Story = {
  args: {
    ...Default.args,
    statusMetrics: [
      { id: 'remaining', label: 'Remaining', value: '3,120', subtitle: 'Current snapshot · 78.0%' },
      { id: 'keys', label: 'Active Keys', value: '5', subtitle: 'Current snapshot' },
      { id: 'quarantined', label: 'Quarantined', value: '1', subtitle: 'Needs manual review' },
      { id: 'exhausted', label: 'Exhausted', value: '1', subtitle: '1 exhausted' },
      { id: 'proxy-available', label: 'Available Proxy Nodes', value: '2', subtitle: 'Current snapshot · 50.0%' },
      { id: 'proxy-total', label: 'Proxy Nodes Total', value: '4', subtitle: 'Current snapshot' },
    ],
    keys: [
      {
        id: 'Qn8R',
        status: 'active',
        group: 'ops',
        registration_ip: '1.1.1.1',
        registration_region: null,
        status_changed_at: 0,
        last_used_at: 0,
        deleted_at: null,
        quota_limit: 1000,
        quota_remaining: 0,
        quota_synced_at: 0,
        total_requests: 111,
        success_count: 88,
        error_count: 23,
        quota_exhausted_count: 11,
        quarantine: {
          source: '/api/tavily/search',
          reasonCode: 'account_deactivated',
          reasonSummary: 'Tavily account deactivated (HTTP 401)',
          reasonDetail: 'The account associated with this API key has been deactivated.',
          createdAt: 0,
        },
      },
    ],
    logs: [
      { id: 1, key_id: 'Qn8R', auth_token_id: '9vsN', method: 'POST', path: '/mcp', query: null, http_status: 401, mcp_status: -1, result_status: 'error', created_at: 1, error_message: 'account deactivated', request_body: null, response_body: null, forwarded_headers: [], dropped_headers: [] },
    ],
    jobs: [
      { id: 1, job_type: 'quota_sync', key_id: 'Qn8R', key_group: 'ops', status: 'error', attempt: 1, message: 'account deactivated', started_at: 1, finished_at: 2 },
    ],
  },
}

export const LargeNumbers: Story = {
  args: {
    ...Default.args,
    monthMetrics: [
      { id: 'month-total', label: 'Total Requests', value: '1,205,420', subtitle: 'Month to date' },
      { id: 'month-success', label: 'Successful', value: '1,084,031', subtitle: 'Month share · 89.9%' },
      { id: 'month-errors', label: 'Errors', value: '88,247', subtitle: 'Month share · 7.3%' },
      { id: 'month-quota', label: 'Quota Exhausted', value: '33,142', subtitle: 'Month share · 2.8%' },
      { id: 'month-new-keys', label: 'New Keys', value: '1,248', subtitle: 'Added this month' },
      { id: 'month-new-quarantines', label: 'New Quarantines', value: '108', subtitle: 'Added this month' },
    ],
    statusMetrics: [
      { id: 'remaining', label: 'Remaining', value: '149,482', subtitle: 'Current snapshot · 12.5%' },
      { id: 'keys', label: 'Active Keys', value: '1,231', subtitle: 'Current snapshot' },
      { id: 'quarantined', label: 'Quarantined', value: '29', subtitle: 'Needs manual review' },
      { id: 'exhausted', label: 'Exhausted', value: '402', subtitle: '402 exhausted' },
      { id: 'proxy-available', label: 'Available Proxy Nodes', value: '128', subtitle: 'Current snapshot · 84.8%' },
      { id: 'proxy-total', label: 'Proxy Nodes Total', value: '151', subtitle: 'Current snapshot' },
    ],
  },
}
