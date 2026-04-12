import type {
  LogOperationalClass,
  LogResultFilter,
  RequestLogsCatalogQuery,
  RequestLogsListPage,
  RequestLogsListQuery,
} from '../api'
import type { AdminTranslations } from '../i18n'

type AdminLogsStrings = AdminTranslations['logs']

export interface RequestLogsFilterQueryInput {
  requestKinds?: string[]
  result?: LogResultFilter
  keyEffect?: string
  bindingEffect?: string
  selectionEffect?: string
  tokenId?: string | null
  keyId?: string | null
  operationalClass?: LogOperationalClass | 'all'
  since?: number
  sinceIso?: string
  untilIso?: string
}

export interface RequestLogsListPlanInput extends RequestLogsFilterQueryInput {
  limit: number
  cursor?: string | null
  direction?: 'older' | 'newer'
  hasEmptyMatch?: boolean
}

export interface RequestLogsCatalogPlanInput extends RequestLogsFilterQueryInput {
}

export type RequestLogsQueryPlan<TQuery> =
  | { kind: 'empty' }
  | {
      kind: 'fetch'
      query: TQuery
    }

export interface RequestLogsFetchPlan<TQuery> {
  kind: 'fetch'
  query: TQuery
}

export function createEmptyRequestLogsListPage(pageSize: number): RequestLogsListPage {
  return {
    items: [],
    pageSize,
    nextCursor: null,
    prevCursor: null,
    hasOlder: false,
    hasNewer: false,
  }
}

function buildRequestLogsFilterQuery(input: RequestLogsFilterQueryInput): RequestLogsCatalogQuery {
  return {
    requestKinds: input.requestKinds,
    result: input.result,
    keyEffect: input.keyEffect,
    bindingEffect: input.bindingEffect,
    selectionEffect: input.selectionEffect,
    tokenId: input.tokenId ?? undefined,
    keyId: input.keyId ?? undefined,
    operationalClass: input.operationalClass,
    since: input.since,
    sinceIso: input.sinceIso,
    untilIso: input.untilIso,
  }
}

export function buildRequestLogsListPlan(input: RequestLogsListPlanInput): RequestLogsQueryPlan<RequestLogsListQuery> {
  if (input.hasEmptyMatch) {
    return { kind: 'empty' }
  }
  return {
    kind: 'fetch',
    query: {
      limit: input.limit,
      cursor: input.cursor,
      direction: input.direction,
      ...buildRequestLogsFilterQuery(input),
    },
  }
}

export function buildRequestLogsCatalogPlan(
  input: RequestLogsCatalogPlanInput,
): RequestLogsFetchPlan<RequestLogsCatalogQuery> {
  return {
    kind: 'fetch',
    query: {
      since: input.since,
      sinceIso: input.sinceIso,
      untilIso: input.untilIso,
    },
  }
}

export function formatRequestLogsDescription(strings: AdminLogsStrings, retentionDays?: number | null): string {
  if (typeof retentionDays === 'number' && retentionDays > 0) {
    return strings.descriptionWithRetention.replace('{days}', String(retentionDays))
  }
  return strings.descriptionFallback || strings.description
}

export function formatRequestLogsPaginationSummary(strings: AdminLogsStrings, retentionDays?: number | null): string {
  if (typeof retentionDays === 'number' && retentionDays > 0) {
    return strings.pagination.summaryWithRetention.replace('{days}', String(retentionDays))
  }
  return strings.pagination.summary
}
