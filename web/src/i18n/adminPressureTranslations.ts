import type { Language } from './types'
import type { AdminPressureTranslations } from './adminPressureTranslationTypes'

export const adminPressureTranslations: Record<Language, AdminPressureTranslations> = {
  zh: {
    title: '压力',
    description: '观察服务器业务 1h 压力与当前用户压力分布，辅助判断峰值时段与公平策略。',
    loading: '正在加载压力快照…',
    retry: '立即重试',
    errorTitle: '压力数据加载失败',
    emptyTitle: '暂无压力数据',
    emptyDescription: '当前窗口内还没有可用于分析的服务器压力样本。',
    userFallback: '未命名用户',
    summary: {
      currentPressure: '当前 1h 压力',
      currentPressureHint: '最近 1 小时内已实际上游的业务请求总数',
      currentPeak: '24h 峰值',
      yesterdayDelta: '较昨日同期',
      yesterdayDeltaHint: '以当前 1h pressure 对比昨日同刻窗口',
      activeUsers: '活跃压力用户',
      activeUsersHint: '零压力用户 {count}',
      distribution: '中位数 / P90',
      distributionHint: '峰值用户 {peak}',
    },
    charts: {
      last24h: {
        title: '最近 24 小时服务器 1 小时窗口压力',
        description: '当前压力走势与昨日同期对齐对比，用于观察峰值时段变化。',
        currentLabel: '当前',
        previousLabel: '昨日同期',
      },
      userDistribution: {
        title: '当前 1 小时用户压力分布',
        description: '按压力区间分桶，展示当前 1 小时各压力段内有多少用户。',
        seriesLabel: '用户数',
        empty: '当前 1 小时内还没有产生压力的用户。',
      },
      last7d: {
        title: '最近 7 天服务器小时压力',
        description: '按小时比较最近 7 天服务器压力走势。',
        seriesLabel: '小时压力',
      },
    },
  },
  en: {
    title: 'Pressure',
    description: 'Review server business 1h pressure and current user pressure distribution to understand peak hours and fairness pressure.',
    loading: 'Loading pressure snapshot…',
    retry: 'Retry now',
    errorTitle: 'Failed to load pressure data',
    emptyTitle: 'No pressure data yet',
    emptyDescription: 'No server pressure samples are available for the current analysis window.',
    userFallback: 'Unknown user',
    summary: {
      currentPressure: 'Current 1h pressure',
      currentPressureHint: 'Total upstream business requests completed in the last hour',
      currentPeak: '24h peak',
      yesterdayDelta: 'vs yesterday',
      yesterdayDeltaHint: 'Current 1h pressure compared with the same local clock window yesterday',
      activeUsers: 'Active pressure users',
      activeUsersHint: '{count} zero-pressure users',
      distribution: 'Median / P90',
      distributionHint: 'Peak user {peak}',
    },
    charts: {
      last24h: {
        title: 'Last 24 hours server rolling 1h pressure',
        description: 'Compare the current pressure curve with the aligned same-time-yesterday baseline.',
        currentLabel: 'Current',
        previousLabel: 'Yesterday',
      },
      userDistribution: {
        title: 'Current 1h user pressure distribution',
        description: 'Users grouped into pressure buckets for the current 1h window.',
        seriesLabel: 'Users',
        empty: 'No users have produced pressure in the current 1h window.',
      },
      last7d: {
        title: 'Last 7 days server hourly pressure',
        description: 'Compare hourly server pressure trends across the last seven days.',
        seriesLabel: 'Hourly pressure',
      },
    },
  },
}
