import type { Meta, StoryObj } from '@storybook/react-vite'

import DashboardOverview from './DashboardOverview'
import { createDashboardTodayMetrics } from './dashboardTodayMetrics'

const storyNumberFormatter = new Intl.NumberFormat('en-US', {
  maximumFractionDigits: 0,
})

const storyPercentageFormatter = new Intl.NumberFormat('en-US', {
  style: 'percent',
  minimumFractionDigits: 0,
  maximumFractionDigits: 1,
})

const meta = {
  title: 'Admin/Components/DashboardOverview',
  component: DashboardOverview,
  tags: ['autodocs'],
  decorators: [
    (Story) => (
      <div style={{ padding: 24, background: 'hsl(var(--background))' }}>
        <Story />
      </div>
    ),
  ],
  parameters: {
    docs: {
      description: {
        component:
          'Dashboard overview shell with today/month/status summary cards. Success and error deltas use rate shifts instead of raw count changes.',
      },
    },
  },
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
  percentagePointUnit: 'pp',
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

const todayMetrics = createDashboardTodayMetrics({
  today: {
    total_requests: 4_812,
    success_count: 4_192,
    error_count: 451,
    quota_exhausted_count: 169,
    new_keys: 0,
    new_quarantines: 0,
  },
  yesterday: {
    total_requests: 4_386,
    success_count: 3_694,
    error_count: 527,
    quota_exhausted_count: 19,
    new_keys: 0,
    new_quarantines: 0,
  },
  labels: {
    total: 'Total Requests',
    success: 'Successful',
    errors: 'Errors',
    quota: 'Quota Exhausted',
  },
  strings,
  formatters: {
    formatNumber: (value) => storyNumberFormatter.format(value),
    formatPercent: (numerator, denominator) =>
      denominator === 0 ? '—' : storyPercentageFormatter.format(numerator / denominator),
  },
})

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

const zhStrings = {
  title: '管理总览',
  description: '把全站运行、风险信号和可执行动作收在同一个面板里。',
  loading: '正在加载仪表盘数据…',
  summaryUnavailable: '暂时无法加载期间摘要。',
  statusUnavailable: '暂时无法加载站点当前状态。',
  todayTitle: '今日',
  todayDescription: '截至当前的核心请求指标，并与昨日同一时刻直接对比。',
  monthTitle: '本月',
  monthDescription: '按月累计的请求表现，方便快速判断整体趋势。',
  currentStatusTitle: '站点当前状态',
  currentStatusDescription: '当前额度、活跃密钥和代理池健康度快照。',
  deltaFromYesterday: '较昨日同刻',
  deltaNoBaseline: '昨日无基线',
  percentagePointUnit: '个百分点',
  asOfNow: '截至当前',
  todayShare: '今日占比',
  monthToDate: '本月累计',
  monthShare: '本月占比',
  trendsTitle: '流量趋势',
  trendsDescription: '根据近期请求观察流量和错误变化。',
  requestTrend: '请求量',
  errorTrend: '错误量',
  riskTitle: '风险观察',
  riskDescription: '优先查看需要运维动作的项目。',
  riskEmpty: '当前没有需要处理的风险信号。',
  actionsTitle: '快捷入口',
  actionsDescription: '最近事件可直接跳转处理。',
  recentRequests: '近期请求',
  recentJobs: '近期任务',
  openModule: '打开',
  openToken: '打开令牌',
  openKey: '打开密钥',
  disabledTokenRisk: '令牌 {id} 已停用',
  exhaustedKeyRisk: '密钥 {id} 已耗尽',
  failedJobRisk: '任务 #{id} 状态：{status}',
  tokenCoverageTruncated: '令牌范围数据被截断。',
  tokenCoverageError: '令牌范围数据加载失败。',
}

const zhDarkEvidenceTodayMetrics = [
  {
    id: 'today-total',
    label: '总请求数',
    value: '10,683',
    subtitle: '截至当前',
    comparison: {
      label: '较昨日同刻',
      value: '+226 (2.2%)',
      direction: 'up' as const,
      tone: 'positive' as const,
    },
  },
  {
    id: 'today-success',
    label: '成功',
    value: '8,762',
    subtitle: '今日占比 · 82%',
    comparison: {
      label: '较昨日同刻',
      value: '-2.0 个百分点',
      direction: 'down' as const,
      tone: 'negative' as const,
    },
  },
  {
    id: 'today-errors',
    label: '错误',
    value: '681',
    subtitle: '今日占比 · 6.4%',
    comparison: {
      label: '较昨日同刻',
      value: '昨日无基线',
      direction: 'flat' as const,
      tone: 'neutral' as const,
    },
  },
  {
    id: 'today-quota',
    label: '额度耗尽',
    value: '42',
    subtitle: '今日占比 · 0.4%',
    comparison: {
      label: '较昨日同刻',
      value: '+38 (950%)',
      direction: 'up' as const,
      tone: 'negative' as const,
    },
  },
]

const zhDarkEvidenceMonthMetrics = [
  { id: 'month-total', label: '总请求数', value: '237,587', subtitle: '本月累计' },
  { id: 'month-success', label: '成功', value: '204,203', subtitle: '本月占比 · 85.9%' },
  { id: 'month-errors', label: '错误', value: '4,399', subtitle: '本月占比 · 1.9%' },
  { id: 'month-quota', label: '额度耗尽', value: '73', subtitle: '本月占比 · 0%' },
  { id: 'month-new-keys', label: '新增密钥', value: '256', subtitle: '本月新增' },
  { id: 'month-new-quarantines', label: '新增隔离密钥', value: '66', subtitle: '本月新增' },
]

const zhDarkEvidenceStatusMetrics = [
  { id: 'remaining', label: '剩余可用', value: '150,801', subtitle: '当前快照 · 79.4%' },
  { id: 'keys', label: '活跃密钥', value: '173', subtitle: '当前快照' },
  { id: 'quarantined', label: '隔离中', value: '66', subtitle: '隔离中' },
  { id: 'exhausted', label: '已耗尽', value: '17', subtitle: '17 个耗尽' },
  { id: 'proxy-available', label: '可用代理节点', value: '74', subtitle: '当前快照 · 98.7%' },
  { id: 'proxy-total', label: '代理节点总数', value: '75', subtitle: '当前快照' },
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
      { id: 1, key_id: 'MZli', auth_token_id: '9vsN', method: 'POST', path: '/mcp', query: null, http_status: 200, mcp_status: 0, result_status: 'success', created_at: 1, error_message: null, request_body: null, response_body: null, forwarded_headers: [], dropped_headers: [], operationalClass: 'success', requestKindProtocolGroup: 'mcp', requestKindBillingGroup: 'billable' },
      { id: 2, key_id: 'MZli', auth_token_id: '9vsN', method: 'POST', path: '/mcp', query: null, http_status: 429, mcp_status: -1, result_status: 'quota_exhausted', created_at: 2, error_message: 'quota', request_body: null, response_body: null, forwarded_headers: [], dropped_headers: [], operationalClass: 'quota_exhausted', requestKindProtocolGroup: 'mcp', requestKindBillingGroup: 'billable' },
    ],
    jobs: [
      { id: 3, job_type: 'forward_proxy_geo_refresh', key_id: null, key_group: null, status: 'success', attempt: 1, message: 'refreshed_candidates=9', started_at: 5, finished_at: 6 },
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
      { id: 1, key_id: 'Qn8R', auth_token_id: '9vsN', method: 'POST', path: '/mcp', query: null, http_status: 401, mcp_status: -1, result_status: 'error', created_at: 1, error_message: 'account deactivated', request_body: null, response_body: null, forwarded_headers: [], dropped_headers: [], operationalClass: 'upstream_error', requestKindProtocolGroup: 'mcp', requestKindBillingGroup: 'billable' },
    ],
    jobs: [
      { id: 2, job_type: 'forward_proxy_geo_refresh', key_id: null, key_group: null, status: 'success', attempt: 1, message: 'refreshed_candidates=4', started_at: 3, finished_at: 4 },
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

export const ZeroBaseline: Story = {
  args: {
    ...Default.args,
    todayMetrics: createDashboardTodayMetrics({
      today: {
        total_requests: 24,
        success_count: 18,
        error_count: 6,
        quota_exhausted_count: 0,
        new_keys: 0,
        new_quarantines: 0,
      },
      yesterday: {
        total_requests: 0,
        success_count: 0,
        error_count: 0,
        quota_exhausted_count: 0,
        new_keys: 0,
        new_quarantines: 0,
      },
      labels: {
        total: 'Total Requests',
        success: 'Successful',
        errors: 'Errors',
        quota: 'Quota Exhausted',
      },
      strings,
      formatters: {
        formatNumber: (value) => storyNumberFormatter.format(value),
        formatPercent: (numerator, denominator) =>
          denominator === 0 ? '—' : storyPercentageFormatter.format(numerator / denominator),
      },
    }),
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 50))
    const text = canvasElement.ownerDocument.body.textContent ?? ''
    for (const expected of ['No yesterday baseline', '75%', '25%']) {
      if (!text.includes(expected)) {
        throw new Error(`Expected dashboard overview zero-baseline story to contain: ${expected}`)
      }
    }
  },
}

export const ZhDarkEvidence: Story = {
  globals: {
    language: 'zh',
    themeMode: 'dark',
  },
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
    docs: {
      description: {
        story:
          '用于验收“去掉外层卡片壳 + 去渐变 + 高对比度胶囊”的稳定中文暗色画布。',
      },
    },
  },
  args: {
    ...Default.args,
    strings: zhStrings,
    todayMetrics: zhDarkEvidenceTodayMetrics,
    monthMetrics: zhDarkEvidenceMonthMetrics,
    statusMetrics: zhDarkEvidenceStatusMetrics,
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 50))

    const summaryPanel = canvasElement.querySelector<HTMLElement>('.dashboard-summary-panel')
    if (summaryPanel == null) {
      throw new Error('Expected dashboard summary panel to render')
    }
    if (summaryPanel.classList.contains('surface') || summaryPanel.classList.contains('panel')) {
      throw new Error('Expected dashboard summary panel to render without the legacy outer shell')
    }
    for (const selector of ['.metric-delta-positive', '.metric-delta-negative', '.metric-delta-neutral']) {
      if (canvasElement.querySelector(selector) == null) {
        throw new Error(`Expected dashboard evidence story to render ${selector}`)
      }
    }

    const text = canvasElement.ownerDocument.body.textContent ?? ''
    for (const expected of ['今日', '本月', '站点当前状态', '较昨日同刻', '昨日无基线']) {
      if (!text.includes(expected)) {
        throw new Error(`Expected dashboard overview evidence story to contain: ${expected}`)
      }
    }
  },
}
