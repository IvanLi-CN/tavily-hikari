import type { JobGroup, JobGroupCounts } from '../api'
import type { AdminTranslations } from '../i18n'

export interface AdminJobFilterOption {
  value: JobGroup
  label: string
  count: number
}

const JOB_GROUP_VALUES = ['all', 'quota', 'usage', 'logs', 'geo', 'linuxdo'] as const

const QUOTA_JOB_TYPES = new Set(['quota_sync', 'quota_sync/manual', 'quota_sync/hot'])
const USAGE_JOB_TYPES = new Set(['token_usage_rollup', 'usage_aggregation'])
const LOG_JOB_TYPES = new Set(['auth_token_logs_gc', 'request_logs_gc', 'log_cleanup'])
const GEO_JOB_TYPES = new Set(['forward_proxy_geo_refresh'])
const LINUXDO_JOB_TYPES = new Set(['linuxdo_user_status_sync'])

export function emptyAdminJobGroupCounts(): JobGroupCounts {
  return {
    all: 0,
    quota: 0,
    usage: 0,
    logs: 0,
    geo: 0,
    linuxdo: 0,
  }
}

export function jobMatchesGroup(jobType: string, group: JobGroup): boolean {
  const normalized = jobType.trim()
  switch (group) {
    case 'quota':
      return QUOTA_JOB_TYPES.has(normalized)
    case 'usage':
      return USAGE_JOB_TYPES.has(normalized)
    case 'logs':
      return LOG_JOB_TYPES.has(normalized)
    case 'geo':
      return GEO_JOB_TYPES.has(normalized)
    case 'linuxdo':
      return LINUXDO_JOB_TYPES.has(normalized)
    case 'all':
    default:
      return true
  }
}

export function jobFilterLabel(group: JobGroup, strings: AdminTranslations['jobs']): string {
  switch (group) {
    case 'quota':
      return strings.filters.quota
    case 'usage':
      return strings.filters.usage
    case 'logs':
      return strings.filters.logs
    case 'geo':
      return strings.filters.geo
    case 'linuxdo':
      return strings.filters.linuxdo ?? strings.types?.linuxdo_user_status_sync ?? 'LinuxDo user sync'
    case 'all':
    default:
      return strings.filters.all
  }
}

export function buildAdminJobFilterOptions(
  strings: AdminTranslations['jobs'],
  counts: JobGroupCounts = emptyAdminJobGroupCounts(),
): ReadonlyArray<AdminJobFilterOption> {
  return JOB_GROUP_VALUES.map((value) => ({
    value,
    label: jobFilterLabel(value, strings),
    count: counts[value],
  }))
}

export function countAdminJobGroups(jobs: ReadonlyArray<{ job_type: string }>): JobGroupCounts {
  const counts = emptyAdminJobGroupCounts()
  counts.all = jobs.length
  for (const job of jobs) {
    for (const group of JOB_GROUP_VALUES) {
      if (group !== 'all' && jobMatchesGroup(job.job_type, group)) {
        counts[group] += 1
      }
    }
  }
  return counts
}

export function summarizeAdminJobFilter(
  group: JobGroup,
  strings: AdminTranslations['jobs'],
): string {
  return `${strings.table.type}: ${jobFilterLabel(group, strings)}`
}
