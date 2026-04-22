import type { Meta, StoryObj } from '@storybook/react-vite'

import type { RecentAlertsSummary } from '../api'
import DashboardOverview, { type DashboardMetricCard, type DashboardQuotaChargeCardData } from './DashboardOverview'
import {
  createDashboardMonthMetrics,
  createDashboardTodayMetrics,
} from './dashboardTodayMetrics'
import { buildDashboardHourlyRequestWindowFixture } from './dashboardHourlyCharts'

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
          'Dashboard overview shell with fixed summary rows. Today and month both render the total card on the first row, a dedicated quota-charge card on the second row, then the remaining taxonomy cards in the grid.',
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
  todayDescription: 'Request-value signals up to now, compared with the same time yesterday.',
  monthTitle: 'This Month',
  monthDescription: 'Month-to-date request taxonomy and lifecycle totals in one compact view.',
  currentStatusTitle: 'Current Site Status',
  currentStatusDescription: 'Live quota, active keys, and pool health right now.',
  deltaFromYesterday: 'vs same time yesterday',
  deltaNoBaseline: 'No yesterday baseline',
  percentagePointUnit: 'pp',
  asOfNow: 'Up to now',
  todayShare: 'Today share',
  todayAdded: 'Added today',
  monthToDate: 'Month to date',
  monthAdded: 'Added this month',
  monthShare: 'Month share',
  valuableTag: 'Valuable',
  otherTag: 'Other',
  unknownTag: 'Unknown',
  trendsTitle: 'Traffic Trends',
  trendsDescription: 'The chart covers the last 24 hours as 25 server-time hour buckets, including the current hour in progress, while the x-axis is shown in local time.',
  requestTrend: 'Request volume',
  errorTrend: 'Error volume',
  chartModeResults: 'Results',
  chartModeTypes: 'Types',
  chartModeResultsDelta: 'Δ Results',
  chartModeTypesDelta: 'Δ Types',
  chartVisibleSeries: 'Visible series',
  chartDeltaSeries: 'Compared series',
  chartSelectionAll: 'All',
  chartEmpty: 'No visible chart series for the current selection.',
  chartUtcWindow: 'Local time axis · Last 24 hours ({count} server-time hour buckets, current hour included)',
  chartResultSecondarySuccess: 'Secondary success',
  chartResultPrimarySuccess: 'Primary success',
  chartResultSecondaryFailure: 'Secondary failure',
  chartResultPrimaryFailure429: 'Primary failure · 429',
  chartResultPrimaryFailureOther: 'Primary failure · other',
  chartResultUnknown: 'Unknown',
  chartTypeMcpNonBillable: 'MCP non-billable',
  chartTypeMcpBillable: 'MCP billable',
  chartTypeApiNonBillable: 'API non-billable',
  chartTypeApiBillable: 'API billable',
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
  recentAlertsTitle: 'Alerts · Last 24 hours',
  recentAlertsDescription: 'A compact summary of alert events and grouped subjects for the same default 24-hour window.',
  recentAlertsEvents: 'Events',
  recentAlertsGroups: 'Groups',
  recentAlertsEmpty: 'No alert events were recorded in the current 24-hour window.',
  recentAlertsOpen: 'Open alerts',
  recentAlertsTypeLabels: {
    upstream_rate_limited_429: 'Upstream 429',
    upstream_usage_limit_432: 'Upstream usage limit 432',
    upstream_key_blocked: 'Upstream key blocked',
    user_request_rate_limited: 'User request rate limited',
    user_quota_exhausted: 'User quota exhausted',
  },
}

const recentAlerts: RecentAlertsSummary = {
  windowHours: 24,
  totalEvents: 19,
  groupedCount: 6,
  countsByType: [
    { type: 'upstream_rate_limited_429', count: 7 },
    { type: 'upstream_usage_limit_432', count: 4 },
    { type: 'upstream_key_blocked', count: 2 },
    { type: 'user_request_rate_limited', count: 5 },
    { type: 'user_quota_exhausted', count: 1 },
  ],
  topGroups: [
    {
      id: 'group:upstream_usage_limit_432:user:usr_001:tavily_search',
      type: 'upstream_usage_limit_432',
      subjectKind: 'user',
      subjectId: 'usr_001',
      subjectLabel: 'Alice Wang',
      user: { userId: 'usr_001', displayName: 'Alice Wang', username: 'alice' },
      token: { id: 'tok_ops_01', label: 'tok_ops_01' },
      key: { id: 'key_001', label: 'key_001' },
      requestKind: { key: 'tavily_search', label: 'Tavily Search', detail: 'POST /api/tavily/search' },
      count: 4,
      firstSeen: 1_762_373_400,
      lastSeen: 1_762_379_200,
      latestEvent: {
        id: 'alert_evt_001',
        type: 'upstream_usage_limit_432',
        title: 'Tavily usage limit 432',
        summary: 'Alice Wang hit the upstream Tavily usage limit for Tavily Search.',
        occurredAt: 1_762_379_200,
        subjectKind: 'user',
        subjectId: 'usr_001',
        subjectLabel: 'Alice Wang',
        user: { userId: 'usr_001', displayName: 'Alice Wang', username: 'alice' },
        token: { id: 'tok_ops_01', label: 'tok_ops_01' },
        key: { id: 'key_001', label: 'key_001' },
        request: { id: 401, method: 'POST', path: '/api/tavily/search', query: null },
        requestKind: { key: 'tavily_search', label: 'Tavily Search', detail: 'POST /api/tavily/search' },
        failureKind: null,
        resultStatus: 'quota_exhausted',
        errorMessage: 'This request exceeds your plan\'s set usage limit.',
        reasonCode: null,
        reasonSummary: null,
        reasonDetail: null,
        source: { kind: 'auth_token_log', id: 'log_401' },
      },
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
      count: 2,
      firstSeen: 1_762_375_000,
      lastSeen: 1_762_378_100,
      latestEvent: {
        id: 'alert_evt_002',
        type: 'upstream_key_blocked',
        title: 'Upstream key blocked',
        summary: 'key_001 was disabled upstream and quarantined locally.',
        occurredAt: 1_762_378_100,
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
        reasonDetail: 'Tavily rejected the key with a deactivated account response.',
        source: { kind: 'api_key_maintenance_record', id: 'maint_002' },
      },
    },
  ],
}

const todayMetrics = createDashboardTodayMetrics({
  today: {
    total_requests: 4_812,
    success_count: 0,
    error_count: 0,
    quota_exhausted_count: 0,
    valuable_success_count: 3_442,
    valuable_failure_count: 604,
    other_success_count: 498,
    other_failure_count: 176,
    unknown_count: 92,
    upstream_exhausted_key_count: 7,
    new_keys: 0,
    new_quarantines: 0,
  },
  yesterday: {
    total_requests: 4_386,
    success_count: 0,
    error_count: 0,
    quota_exhausted_count: 0,
    valuable_success_count: 3_118,
    valuable_failure_count: 582,
    other_success_count: 454,
    other_failure_count: 161,
    unknown_count: 71,
    upstream_exhausted_key_count: 3,
    new_keys: 0,
    new_quarantines: 0,
  },
  labels: {
    total: 'Total Requests',
    success: 'Success',
    failure: 'Failure',
    unknownCalls: 'Unknown Calls',
    upstreamExhausted: 'Upstream Keys Exhausted',
    valuableTag: 'Primary',
    otherTag: 'Secondary',
    unknownTag: 'Unknown',
  },
  strings,
  formatters: {
    formatNumber: (value) => storyNumberFormatter.format(value),
    formatPercent: (numerator, denominator) =>
      denominator === 0 ? '—' : storyPercentageFormatter.format(numerator / denominator),
  },
})

const monthMetrics = createDashboardMonthMetrics({
  month: {
    total_requests: 105_041,
    success_count: 0,
    error_count: 0,
    quota_exhausted_count: 0,
    valuable_success_count: 70_211,
    valuable_failure_count: 12_440,
    other_success_count: 10_062,
    other_failure_count: 4_083,
    unknown_count: 1_844,
    upstream_exhausted_key_count: 12,
    new_keys: 3,
    new_quarantines: 0,
  },
  labels: {
    total: 'Total Requests',
    success: 'Success',
    failure: 'Failure',
    unknownCalls: 'Unknown Calls',
    upstreamExhausted: 'Upstream Keys Exhausted',
    valuableTag: 'Primary',
    otherTag: 'Secondary',
    unknownTag: 'Unknown',
    newKeys: 'New Keys',
    newQuarantines: 'New Quarantines',
  },
  strings: {
    monthToDate: 'Month to date',
    monthShare: 'Month share',
    monthAdded: 'Added this month',
  },
  formatters: {
    formatNumber: (value) => storyNumberFormatter.format(value),
    formatPercent: (numerator, denominator) =>
      denominator === 0 ? '—' : storyPercentageFormatter.format(numerator / denominator),
  },
})

const todayQuotaCharge: DashboardQuotaChargeCardData = {
  title: 'Quota Charges',
  localLabel: 'Local estimate',
  localValue: '4,768',
  upstreamLabel: 'Upstream actual',
  upstreamValue: '4,721',
  deltaLabel: 'Delta',
  deltaValue: '+47',
  deltaTone: 'negative',
  coverage: 'Sampled 24 · Stale 3',
  freshness: 'Latest sync · 2 minutes ago · 14:28',
}

const monthQuotaCharge: DashboardQuotaChargeCardData = {
  title: 'Quota Charges',
  localLabel: 'Local estimate',
  localValue: '104,881',
  upstreamLabel: 'Upstream actual',
  upstreamValue: '104,744',
  deltaLabel: 'Delta',
  deltaValue: '+137',
  deltaTone: 'negative',
  coverage: 'Sampled 68 · Stale 5',
  freshness: 'Latest sync · 2 minutes ago · 14:28',
}

const defaultHourlyRequestWindow = buildDashboardHourlyRequestWindowFixture({
  mapBucket: ({ index, bucket }) => ({
    secondarySuccess: (index % 5) + 2,
    primarySuccess: bucket.primarySuccess + (index % 3),
    secondaryFailure: index % 4,
    primaryFailure429: index % 8 === 0 ? 2 : bucket.primaryFailure429,
    primaryFailureOther: index % 6 === 0 ? 2 : bucket.primaryFailureOther,
    unknown: index % 11 === 0 ? 1 : 0,
    mcpNonBillable: index % 3,
    mcpBillable: (index % 5) + 3,
    apiNonBillable: index % 2,
    apiBillable: (index % 6) + 4,
  }),
})

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
  todayDescription: '按调用价值查看截至当前的请求表现，并直接对比昨日同刻。',
  monthTitle: '本月',
  monthDescription: '把本月累计的请求价值分类与生命周期指标压缩到同一组卡片里。',
  currentStatusTitle: '站点当前状态',
  currentStatusDescription: '当前额度、活跃密钥和代理池健康度快照。',
  deltaFromYesterday: '较昨日同刻',
  deltaNoBaseline: '昨日无基线',
  percentagePointUnit: '个百分点',
  asOfNow: '截至当前',
  todayShare: '今日占比',
  todayAdded: '今日新增',
  monthToDate: '本月累计',
  monthAdded: '本月新增',
  monthShare: '本月占比',
  valuableTag: '主要',
  otherTag: '次要',
  unknownTag: '未知',
  trendsTitle: '流量趋势',
  trendsDescription: '统计窗口覆盖近 24 小时（共 25 组服务器时区小时数据，含当前小时进行中），横轴按本地时间显示。',
  requestTrend: '请求量',
  errorTrend: '错误量',
  chartModeResults: '调用结果',
  chartModeTypes: '调用类型',
  chartModeResultsDelta: '较昨日 · 调用结果',
  chartModeTypesDelta: '较昨日 · 调用类型',
  chartVisibleSeries: '显示系列',
  chartDeltaSeries: '对比系列',
  chartSelectionAll: '全部',
  chartEmpty: '当前选择下没有可显示的图表系列。',
  chartUtcWindow: '本地时间横轴 · 近 24 小时（共 {count} 组服务器时区小时数据，含当前小时）',
  chartResultSecondarySuccess: '次要成功',
  chartResultPrimarySuccess: '主要成功',
  chartResultSecondaryFailure: '次要失败',
  chartResultPrimaryFailure429: '主要失败 · 429',
  chartResultPrimaryFailureOther: '主要失败 · 其他',
  chartResultUnknown: '未知',
  chartTypeMcpNonBillable: 'MCP 非计费',
  chartTypeMcpBillable: 'MCP 计费',
  chartTypeApiNonBillable: 'API 非计费',
  chartTypeApiBillable: 'API 计费',
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
  recentAlertsTitle: '告警 · 最近 24 小时',
  recentAlertsDescription: '按默认 24 小时窗口展示事件总数、分组数与高频告警主体。',
  recentAlertsEvents: '事件数',
  recentAlertsGroups: '分组数',
  recentAlertsEmpty: '当前 24 小时窗口内没有记录到告警事件。',
  recentAlertsOpen: '打开告警中心',
  recentAlertsTypeLabels: {
    upstream_rate_limited_429: '上游 429',
    upstream_usage_limit_432: '上游用量限制 432',
    upstream_key_blocked: '上游 Key 封禁',
    user_request_rate_limited: '用户请求限流',
    user_quota_exhausted: '用户额度耗尽',
  },
}

const zhDarkEvidenceTodayMetrics: DashboardMetricCard[] = [
  {
    id: 'today-total',
    label: '总请求数',
    value: '10,683',
    subtitle: '截至当前',
    fullWidth: true,
    comparison: {
      label: '较昨日同刻',
      value: '+226 (2.2%)',
      direction: 'up',
      tone: 'positive',
    },
  },
  {
    id: 'today-valuable-success',
    label: '成功',
    marker: '主要',
    markerTone: 'primary',
    value: '6,831',
    valueMeta: '今日占比 · 63.9%',
    comparison: {
      label: '较昨日同刻',
      value: '+542 (8.6%)',
      direction: 'up',
      tone: 'positive',
    },
  },
  {
    id: 'today-valuable-failure',
    label: '失败',
    marker: '主要',
    markerTone: 'primary',
    value: '1,144',
    valueMeta: '今日占比 · 10.7%',
    comparison: {
      label: '较昨日同刻',
      value: '-126 (-9.9%)',
      direction: 'down',
      tone: 'positive',
    },
  },
  {
    id: 'today-other-success',
    label: '成功',
    marker: '次要',
    markerTone: 'secondary',
    value: '1,882',
    valueMeta: '今日占比 · 17.6%',
    comparison: {
      label: '较昨日同刻',
      value: '+94 (5.3%)',
      direction: 'up',
      tone: 'positive',
    },
  },
  {
    id: 'today-other-failure',
    label: '失败',
    marker: '次要',
    markerTone: 'secondary',
    value: '552',
    valueMeta: '今日占比 · 5.2%',
    comparison: {
      label: '较昨日同刻',
      value: '+41 (8%)',
      direction: 'up',
      tone: 'negative',
    },
  },
  {
    id: 'today-unknown',
    label: '未知调用',
    value: '274',
    valueMeta: '今日占比 · 2.6%',
    comparison: {
      label: '较昨日同刻',
      value: '+18 · 昨日无基线',
      direction: 'up',
      tone: 'negative',
    },
  },
  {
    id: 'today-upstream-exhausted',
    label: '上游 Key 耗尽',
    value: '42',
    subtitle: '今日新增',
    comparison: {
      label: '较昨日同刻',
      value: '+38 (950%)',
      direction: 'up',
      tone: 'negative',
    },
  },
]

const zhDarkEvidenceMonthMetrics: DashboardMetricCard[] = [
  { id: 'month-total', label: '总请求数', value: '237,587', subtitle: '本月累计' },
  { id: 'month-valuable-success', label: '成功', marker: '主要', markerTone: 'primary', value: '152,204', subtitle: '本月占比 · 64%' },
  { id: 'month-valuable-failure', label: '失败', marker: '主要', markerTone: 'primary', value: '25,881', subtitle: '本月占比 · 10.9%' },
  { id: 'month-other-success', label: '成功', marker: '次要', markerTone: 'secondary', value: '39,118', subtitle: '本月占比 · 16.5%' },
  { id: 'month-other-failure', label: '失败', marker: '次要', markerTone: 'secondary', value: '8,960', subtitle: '本月占比 · 3.8%' },
  { id: 'month-unknown', label: '未知调用', value: '3,654', subtitle: '本月占比 · 1.5%' },
  { id: 'month-upstream-exhausted', label: '上游 Key 耗尽', value: '73', subtitle: '本月新增' },
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

const zhDarkTodayQuotaCharge: DashboardQuotaChargeCardData = {
  title: '额度扣减',
  localLabel: '本地估算',
  localValue: '10,622',
  upstreamLabel: '上游 Key 实扣',
  upstreamValue: '10,587',
  deltaLabel: '差值',
  deltaValue: '+35',
  deltaTone: 'negative',
  coverage: '已采样 31 · 滞后 4',
  freshness: '最新同步 · 2 分钟前 · 14:28',
}

const zhDarkMonthQuotaCharge: DashboardQuotaChargeCardData = {
  title: '额度扣减',
  localLabel: '本地估算',
  localValue: '236,901',
  upstreamLabel: '上游 Key 实扣',
  upstreamValue: '236,744',
  deltaLabel: '差值',
  deltaValue: '+157',
  deltaTone: 'negative',
  coverage: '已采样 73 · 滞后 6',
  freshness: '最新同步 · 2 分钟前 · 14:28',
}

export const Default: Story = {
  args: {
    strings,
    overviewReady: true,
    statusLoading: false,
    todayMetrics,
    todayQuotaCharge,
    monthMetrics,
    monthQuotaCharge,
    statusMetrics,
    hourlyRequestWindow: defaultHourlyRequestWindow,
    chartLabelTimeZone: 'Asia/Shanghai',
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
      { id: 4, job_type: 'linuxdo_user_status_sync', key_id: null, key_group: null, status: 'error', attempt: 1, message: 'attempted=18 success=17 skipped=0 failure=1 first_failure=hhf0517: token upstream status 400: {\"error\":\"invalid_grant\"}', started_at: 7, finished_at: 8 },
      { id: 3, job_type: 'forward_proxy_geo_refresh', key_id: null, key_group: null, status: 'success', attempt: 1, message: 'refreshed_candidates=9', started_at: 5, finished_at: 6 },
      { id: 1, job_type: 'quota_sync', key_id: 'MZli', key_group: 'ops', status: 'error', attempt: 2, message: 'rate limit', started_at: 1, finished_at: 2 },
      { id: 2, job_type: 'quota_sync', key_id: 'MZli', key_group: 'ops', status: 'success', attempt: 1, message: null, started_at: 3, finished_at: 4 },
    ],
    recentAlerts,
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
      { id: 3, job_type: 'linuxdo_user_status_sync', key_id: null, key_group: null, status: 'success', attempt: 1, message: 'attempted=4 success=4 skipped=0 failure=0', started_at: 5, finished_at: 6 },
      { id: 2, job_type: 'forward_proxy_geo_refresh', key_id: null, key_group: null, status: 'success', attempt: 1, message: 'refreshed_candidates=4', started_at: 3, finished_at: 4 },
      { id: 1, job_type: 'quota_sync', key_id: 'Qn8R', key_group: 'ops', status: 'error', attempt: 1, message: 'account deactivated', started_at: 1, finished_at: 2 },
    ],
  },
}

export const LargeNumbers: Story = {
  args: {
    ...Default.args,
    monthQuotaCharge: {
      title: 'Quota Charges',
      localLabel: 'Local estimate',
      localValue: '1,204,880',
      upstreamLabel: 'Upstream actual',
      upstreamValue: '1,204,441',
      deltaLabel: 'Delta',
      deltaValue: '+439',
      deltaTone: 'negative',
      coverage: 'Sampled 418 · Stale 9',
      freshness: 'Latest sync · 1 minute ago · 14:28',
    },
    monthMetrics: createDashboardMonthMetrics({
      month: {
        total_requests: 1_205_420,
        success_count: 0,
        error_count: 0,
        quota_exhausted_count: 0,
        valuable_success_count: 784_031,
        valuable_failure_count: 121_247,
        other_success_count: 214_500,
        other_failure_count: 58_420,
        unknown_count: 27_222,
        upstream_exhausted_key_count: 418,
        new_keys: 1_248,
        new_quarantines: 108,
      },
      labels: {
        total: 'Total Requests',
        success: 'Success',
        failure: 'Failure',
        unknownCalls: 'Unknown Calls',
        upstreamExhausted: 'Upstream Keys Exhausted',
        valuableTag: 'Valuable',
        otherTag: 'Other',
        unknownTag: 'Unknown',
        newKeys: 'New Keys',
        newQuarantines: 'New Quarantines',
      },
      strings: {
        monthToDate: 'Month to date',
        monthShare: 'Month share',
        monthAdded: 'Added this month',
      },
      formatters: {
        formatNumber: (value) => storyNumberFormatter.format(value),
        formatPercent: (numerator, denominator) =>
          denominator === 0 ? '—' : storyPercentageFormatter.format(numerator / denominator),
      },
    }),
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
        success_count: 0,
        error_count: 0,
        quota_exhausted_count: 0,
        valuable_success_count: 12,
        valuable_failure_count: 6,
        other_success_count: 4,
        other_failure_count: 2,
        unknown_count: 0,
        upstream_exhausted_key_count: 0,
        new_keys: 0,
        new_quarantines: 0,
      },
      yesterday: {
        total_requests: 0,
        success_count: 0,
        error_count: 0,
        quota_exhausted_count: 0,
        valuable_success_count: 0,
        valuable_failure_count: 0,
        other_success_count: 0,
        other_failure_count: 0,
        unknown_count: 0,
        upstream_exhausted_key_count: 0,
        new_keys: 0,
        new_quarantines: 0,
      },
      labels: {
        total: 'Total Requests',
        success: 'Success',
        failure: 'Failure',
        unknownCalls: 'Unknown Calls',
        upstreamExhausted: 'Upstream Keys Exhausted',
        valuableTag: 'Valuable',
        otherTag: 'Other',
        unknownTag: 'Unknown',
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
    for (const expected of ['No yesterday baseline', '50%', '25%', '17%']) {
      if (!text.includes(expected)) {
        throw new Error(`Expected dashboard overview zero-baseline story to contain: ${expected}`)
      }
    }
  },
}

export const TypesMode: Story = {
  args: {
    ...Default.args,
    initialChartMode: 'types',
  },
}

export const ResultsDeltaMode: Story = {
  args: {
    ...Default.args,
    initialChartMode: 'resultsDelta',
    initialResultDeltaSeries: 'all',
  },
}

export const TypesDeltaMode: Story = {
  args: {
    ...Default.args,
    initialChartMode: 'typesDelta',
    initialTypeDeltaSeries: 'all',
  },
}

export const HiddenSeriesEmpty: Story = {
  args: {
    ...Default.args,
    initialVisibleResultSeries: [],
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
          '用于验收“总请求数独占第一行 + 额度扣减卡独占第二行 + 其余卡片回到网格”的稳定中文暗色画布。',
      },
    },
  },
  args: {
    ...Default.args,
    strings: zhStrings,
    todayMetrics: zhDarkEvidenceTodayMetrics,
    todayQuotaCharge: zhDarkTodayQuotaCharge,
    monthMetrics: zhDarkEvidenceMonthMetrics,
    monthQuotaCharge: zhDarkMonthQuotaCharge,
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

    const todayCards = canvasElement.querySelectorAll('.dashboard-today-grid .dashboard-summary-card')
    if (todayCards.length !== 6) {
      throw new Error(`Expected 6 today grid cards, received ${todayCards.length}`)
    }
    const monthCards = canvasElement.querySelectorAll('.dashboard-summary-metrics-month .dashboard-summary-card')
    if (monthCards.length !== 8) {
      throw new Error(`Expected 8 month grid cards, received ${monthCards.length}`)
    }
    if (canvasElement.querySelector('.dashboard-today-comparisons') != null) {
      throw new Error('Expected legacy today comparison tray to be removed')
    }
    if (canvasElement.querySelector('.dashboard-summary-card-full-width') == null) {
      throw new Error('Expected the today total card to occupy its own row')
    }
    if (canvasElement.querySelectorAll('.dashboard-quota-charge-card').length < 2) {
      throw new Error('Expected both today and month quota charge cards to render')
    }
    for (const selector of ['.metric-delta-positive', '.metric-delta-negative']) {
      if (canvasElement.querySelector(selector) == null) {
        throw new Error(`Expected dashboard evidence story to render ${selector}`)
      }
    }

    const text = canvasElement.ownerDocument.body.textContent ?? ''
    for (const expected of ['今日', '本月', '站点当前状态', '较昨日同刻', '未知调用', '主要', '次要']) {
      if (!text.includes(expected)) {
        throw new Error(`Expected dashboard overview evidence story to contain: ${expected}`)
      }
    }
  },
}
