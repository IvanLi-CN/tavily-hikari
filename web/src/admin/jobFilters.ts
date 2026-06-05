import type { JobGroup, JobGroupCounts } from '../api'
import type { AdminTranslations } from '../i18n'

export interface AdminJobFilterOption {
  value: JobGroup
  label: string
  count: number
}

const JOB_GROUP_VALUES = ['all', 'quota', 'usage', 'logs', 'db', 'geo', 'linuxdo'] as const
export const MANUAL_JOB_ACTIONS = [
  'token_usage_rollup',
  'auth_token_logs_gc',
  'request_logs_gc',
  'mcp_sessions_gc',
  'mcp_session_init_backoffs_gc',
  'linuxdo_user_status_sync',
  'linuxdo_user_tag_binding_refresh',
  'forward_proxy_geo_refresh',
  'db_compaction',
] as const

const QUOTA_JOB_TYPES = new Set(['quota_sync', 'quota_sync/manual', 'quota_sync/hot'])
const USAGE_JOB_TYPES = new Set(['token_usage_rollup', 'usage_aggregation'])
const LOG_JOB_TYPES = new Set([
  'auth_token_logs_gc',
  'request_logs_gc',
  'mcp_sessions_gc',
  'mcp_session_init_backoffs_gc',
  'log_cleanup',
])
const DB_JOB_TYPES = new Set(['db_compaction'])
const GEO_JOB_TYPES = new Set(['forward_proxy_geo_refresh'])
const LINUXDO_JOB_TYPES = new Set(['linuxdo_user_status_sync', 'linuxdo_user_tag_binding_refresh'])
const JOB_TYPE_ALIASES: Record<string, string> = {
  usage_aggregation: 'token_usage_rollup',
  log_cleanup: 'auth_token_logs_gc',
}

export function emptyAdminJobGroupCounts(): JobGroupCounts {
  return {
    all: 0,
    quota: 0,
    usage: 0,
    logs: 0,
    db: 0,
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
    case 'db':
      return DB_JOB_TYPES.has(normalized)
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
    case 'db':
      return strings.filters.db
    case 'geo':
      return strings.filters.geo
    case 'linuxdo':
      return strings.filters.linuxdo ?? strings.types?.linuxdo_user_status_sync ?? 'LinuxDo user sync'
    case 'all':
    default:
      return strings.filters.all
  }
}

export function jobSourceLabel(source: string | null | undefined, strings: AdminTranslations['jobs']): string {
  const normalized = String(source ?? '').trim().toLowerCase()
  return normalized ? strings.sources?.[normalized] ?? normalized : '—'
}

export function adminJobTypeLabel(jobType: string, strings: AdminTranslations['jobs']): string {
  const normalized = jobType.trim()
  if (!normalized) return '—'

  const direct = strings.types?.[normalized]
  if (direct) return direct

  const aliasTarget = JOB_TYPE_ALIASES[normalized]
  if (aliasTarget && strings.types?.[aliasTarget]) {
    return strings.types[aliasTarget]
  }

  return normalized
    .replace(/[\/_]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
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
