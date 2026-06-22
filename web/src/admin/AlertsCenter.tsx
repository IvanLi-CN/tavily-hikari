import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from 'react'

import type {
  AlertCatalog,
  AlertEvent,
  AlertGroup,
  AlertType,
  AlertsQuery,
  AlertsPage,
  RequestLog,
  RequestLogsListPage,
  RequestLogsListQuery,
  RequestLogBodies,
} from '../api'
import {
  fetchAlertCatalog,
  fetchAlertEvents,
  fetchAlertGroups,
  fetchRequestLogDetails,
  fetchRequestLogsList,
} from '../api'
import type { Language } from '../i18n'
import { Icon } from '../lib/icons'
import { getBlockingLoadState, getRefreshingLoadState, type QueryLoadState } from './queryLoadState'
import {
  alertsPath,
  getAlertKeyIdFromSearch,
  getAlertPageFromSearch,
  getAlertRequestKindsFromSearch,
  getAlertSinceFromSearch,
  getAlertTokenIdFromSearch,
  getAlertTypeFromSearch,
  getAlertUntilFromSearch,
  getAlertUserIdFromSearch,
  getAlertsViewFromSearch,
  type AlertsCenterView,
} from './routes'
import AdminLoadingRegion from '../components/AdminLoadingRegion'
import AdminTablePagination from '../components/AdminTablePagination'
import AdminTableShell from '../components/AdminTableShell'
import SearchableFacetSelect from '../components/SearchableFacetSelect'
import RequestKindBadge from '../components/RequestKindBadge'
import { StatusBadge, type StatusTone } from '../components/StatusBadge'
import { cleanedRequestLogBodySummary } from '../requestLogBodySummary'
import { Button } from '../components/ui/button'
import { Drawer, DrawerContent, DrawerDescription, DrawerTitle } from '../components/ui/drawer'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '../components/ui/dropdown-menu'
import { Input } from '../components/ui/input'
import SegmentedTabs from '../components/ui/SegmentedTabs'
import { TableBody, TableCell, TableHead, TableHeader, TableRow } from '../components/ui/table'

const EMPTY_ALERT_EVENTS_PAGE: AlertsPage<AlertEvent> = {
  items: [],
  total: 0,
  page: 1,
  perPage: 20,
}

const EMPTY_ALERT_GROUPS_PAGE: AlertsPage<AlertGroup> = {
  items: [],
  total: 0,
  page: 1,
  perPage: 20,
}

function alertTypeTone(type: AlertType): StatusTone {
  switch (type) {
    case 'upstream_key_blocked':
    case 'user_quota_exhausted':
      return 'error'
    case 'upstream_usage_limit_432':
    case 'upstream_rate_limited_429':
    case 'user_request_rate_limited':
      return 'warning'
    default:
      return 'neutral'
  }
}

function formatIso8601WithOffset(date: Date): string {
  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  const hours = String(date.getHours()).padStart(2, '0')
  const minutes = String(date.getMinutes()).padStart(2, '0')
  const seconds = String(date.getSeconds()).padStart(2, '0')
  const offsetMinutes = -date.getTimezoneOffset()
  const sign = offsetMinutes >= 0 ? '+' : '-'
  const absoluteOffsetMinutes = Math.abs(offsetMinutes)
  const offsetHours = String(Math.floor(absoluteOffsetMinutes / 60)).padStart(2, '0')
  const offsetRemainderMinutes = String(absoluteOffsetMinutes % 60).padStart(2, '0')
  return `${year}-${month}-${day}T${hours}:${minutes}:${seconds}${sign}${offsetHours}:${offsetRemainderMinutes}`
}

function isoToDateTimeLocal(iso: string | null): string {
  if (!iso) return ''
  const parsed = new Date(iso)
  if (Number.isNaN(parsed.getTime())) return ''
  const year = parsed.getFullYear()
  const month = String(parsed.getMonth() + 1).padStart(2, '0')
  const day = String(parsed.getDate()).padStart(2, '0')
  const hours = String(parsed.getHours()).padStart(2, '0')
  const minutes = String(parsed.getMinutes()).padStart(2, '0')
  return `${year}-${month}-${day}T${hours}:${minutes}`
}

function dateTimeLocalToIso(value: string): string | null {
  const trimmed = value.trim()
  if (!trimmed) return null
  const parsed = new Date(trimmed)
  if (Number.isNaN(parsed.getTime())) return null
  return formatIso8601WithOffset(parsed)
}

function formatMonthDayTimeWithSeconds(timestamp: number | null, language: Language): string {
  if (timestamp == null) return '—'
  const parsed = new Date(timestamp * 1000)
  if (Number.isNaN(parsed.getTime())) return '—'
  const month = String(parsed.getMonth() + 1).padStart(2, '0')
  const day = String(parsed.getDate()).padStart(2, '0')
  const hours = String(parsed.getHours()).padStart(2, '0')
  const minutes = String(parsed.getMinutes()).padStart(2, '0')
  const seconds = String(parsed.getSeconds()).padStart(2, '0')
  return language === 'zh'
    ? `${month}月${day}日 ${hours}:${minutes}:${seconds}`
    : `${month}/${day} ${hours}:${minutes}:${seconds}`
}

function totalPages(total: number, perPage: number): number {
  return Math.max(1, Math.ceil(total / Math.max(1, perPage)))
}

function requestSummary(request: AlertEvent['request']): string {
  if (!request) return '—'
  const query = request.query ? `?${request.query}` : ''
  return `${request.method} ${request.path}${query}`
}

function semanticWindowLabel(group: Pick<AlertGroup, 'semanticWindowKind' | 'semanticWindowMinutes'>, language: Language): string {
  switch (group.semanticWindowKind) {
    case 'request_rate':
      return language === 'zh'
        ? `滚动 ${group.semanticWindowMinutes ?? 5} 分钟`
        : `Rolling ${group.semanticWindowMinutes ?? 5}m`
    case 'rolling_hour':
      return language === 'zh' ? '滚动 60 分钟' : 'Rolling 60m'
    case 'day':
      return language === 'zh' ? '自然日窗口' : 'Day window'
    case 'month':
      return language === 'zh' ? '自然月窗口' : 'Month window'
    default:
      return language === 'zh' ? '兼容分组' : 'Compatibility group'
  }
}

function subjectDisplayLabel(subject: Pick<AlertEvent, 'subjectLabel'> | Pick<AlertGroup, 'subjectLabel'>): string {
  return subject.subjectLabel.trim() || '—'
}

function hasClickableGroupSubject(group: AlertGroup): boolean {
  return group.subjectKind === 'user'
    ? Boolean(group.user?.userId)
    : group.subjectKind === 'token'
      ? Boolean(group.token?.id)
      : Boolean(group.key?.id)
}

function isCompatibilityGroup(group: Pick<AlertGroup, 'groupingKind'>): boolean {
  return (group.groupingKind ?? 'compat') === 'compat'
}

function isSemanticMother(group: AlertGroup): boolean {
  return (group.groupingKind ?? 'compat') === 'mother' && (group.children?.length ?? 0) > 0
}

function defaultCopy(language: Language) {
  return language === 'zh'
    ? {
        title: '告警中心',
        description: '查看 429、上游用量限制 432、上游 Key 封禁、本地请求限流与额度耗尽事件，并按同一筛选口径聚合。',
        tabs: { events: '事件记录', groups: '聚合告警' },
        filters: {
          type: '告警类型',
          user: '用户',
          token: '令牌',
          key: 'Key',
          requestKinds: '请求类型',
          since: '开始时间',
          until: '结束时间',
          allTypes: '全部类型',
          allUsers: '全部用户',
          allTokens: '全部令牌',
          allKeys: '全部 Key',
          requestKindsAll: '全部请求类型',
          requestKindsEmpty: '没有可选请求类型',
          searchPlaceholder: '搜索…',
          applyTime: '应用时间',
          clear: '清空筛选',
        },
        table: {
          events: {
            time: '时间',
            type: '类型',
            subject: '主体',
            requestKind: '请求类型',
            related: '关联对象',
            request: '请求',
            summary: '摘要',
          },
          groups: {
            time: '连续区间',
            type: '类型',
            subject: '主体',
            requestKind: '受限窗口',
            count: '命中 / 子窗口',
            latest: '最新摘要',
          },
        },
        emptyEvents: '当前筛选下没有告警事件。',
        emptyGroups: '当前筛选下没有告警分组。',
        groupUi: {
          expand: '展开',
          collapse: '收起',
          children: '子窗口',
          rawEvents: '条原始告警',
          requestRecords: '条调用记录',
          hitRecords: '条命中调用',
          firstHit: '首次命中',
          lastHit: '末次命中',
          latestSummary: '最新摘要',
          noRawEvents: '当前子窗口下没有原始告警。',
          compatibility: '兼容分组',
        },
        childDrawer: {
          title: '调用记录',
          requestKind: '调用类型',
          allRequestKinds: '全部调用类型',
          noRequestKinds: '当前子窗口下没有调用类型',
          outcome: '结果',
          allOutcomes: '全部结果',
          quotaExhausted: '额度/限流',
          success: '成功',
          error: '错误',
          neutral: '中性',
          search: '搜索',
          searchPlaceholder: '搜索请求路径、摘要或主体',
          empty: '当前子窗口下没有关联调用记录。',
          emptyFiltered: '当前筛选下没有命中的调用记录。',
        },
        paginationPrevious: '上一页',
        paginationNext: '下一页',
        requestOpen: '查看请求',
        openUser: '查看用户',
        openToken: '查看令牌',
        openKey: '查看 Key',
        requestDrawer: {
          title: '请求详情',
          requestBody: '请求体',
          responseBody: '响应体',
          noBody: '无内容',
          retry: '重试加载',
          loading: '正在加载请求详情…',
          error: '加载请求详情失败。',
        },
        types: {
          upstream_rate_limited_429: '上游 429',
          upstream_usage_limit_432: '上游用量限制 432',
          upstream_key_blocked: '上游 Key 封禁',
          user_request_rate_limited: '用户请求限流',
          user_quota_exhausted: '用户额度耗尽',
        },
      }
    : {
        title: 'Alerts',
        description: 'Review upstream 429s, upstream usage-limit 432 events, upstream key blocks, local request-rate limits, and quota exhaustion with shared filters.',
        tabs: { events: 'Events', groups: 'Groups' },
        filters: {
          type: 'Alert type',
          user: 'User',
          token: 'Token',
          key: 'Key',
          requestKinds: 'Request kinds',
          since: 'Since',
          until: 'Until',
          allTypes: 'All types',
          allUsers: 'All users',
          allTokens: 'All tokens',
          allKeys: 'All keys',
          requestKindsAll: 'All request kinds',
          requestKindsEmpty: 'No request kinds',
          searchPlaceholder: 'Search…',
          applyTime: 'Apply time',
          clear: 'Clear filters',
        },
        table: {
          events: {
            time: 'Time',
            type: 'Type',
            subject: 'Subject',
            requestKind: 'Request kind',
            related: 'Related',
            request: 'Request',
            summary: 'Summary',
          },
          groups: {
            time: 'Range',
            type: 'Type',
            subject: 'Subject',
            requestKind: 'Window',
            count: 'Hits / children',
            latest: 'Latest summary',
          },
        },
        emptyEvents: 'No alert events match the current filters.',
        emptyGroups: 'No alert groups match the current filters.',
        groupUi: {
          expand: 'Expand',
          collapse: 'Collapse',
          children: 'Child windows',
          rawEvents: 'raw alerts',
          requestRecords: 'call records',
          hitRecords: 'matching calls',
          firstHit: 'First hit',
          lastHit: 'Last hit',
          latestSummary: 'Latest summary',
          noRawEvents: 'No raw alert events are available for this child window.',
          compatibility: 'Compatibility group',
        },
        childDrawer: {
          title: 'Call records',
          requestKind: 'Request kind',
          allRequestKinds: 'All request kinds',
          noRequestKinds: 'No request kinds in this child window',
          outcome: 'Outcome',
          allOutcomes: 'All outcomes',
          quotaExhausted: 'Quota / rate limit',
          success: 'Success',
          error: 'Error',
          neutral: 'Neutral',
          search: 'Search',
          searchPlaceholder: 'Search request path, summary, or subject',
          empty: 'No related call records are available for this child window.',
          emptyFiltered: 'No call records match the current filters.',
        },
        paginationPrevious: 'Previous',
        paginationNext: 'Next',
        requestOpen: 'Open request',
        openUser: 'Open user',
        openToken: 'Open token',
        openKey: 'Open key',
        requestDrawer: {
          title: 'Request details',
          requestBody: 'Request body',
          responseBody: 'Response body',
          noBody: 'No content',
          retry: 'Retry',
          loading: 'Loading request details…',
          error: 'Failed to load request details.',
        },
        types: {
          upstream_rate_limited_429: 'Upstream 429',
          upstream_usage_limit_432: 'Upstream usage limit 432',
          upstream_key_blocked: 'Upstream key blocked',
          user_request_rate_limited: 'User request rate limited',
          user_quota_exhausted: 'User quota exhausted',
        },
      }
}

function paginationSummary(copy: ReturnType<typeof defaultCopy>, total: number, page: number, perPage: number): string {
  const totalPageCount = totalPages(total, perPage)
  return `${total} · ${page}/${totalPageCount}`
}

interface AlertsSearchState {
  view: AlertsCenterView
  type: AlertType | null
  since: string | null
  until: string | null
  userId: string | null
  tokenId: string | null
  keyId: string | null
  requestKinds: string[]
  page: number
}

function listQueryKey(view: AlertsCenterView, query: AlertsQuery): string {
  return JSON.stringify({
    view,
    page: query.page ?? 1,
    perPage: query.perPage ?? 20,
    type: query.type ?? null,
    since: query.since ?? null,
    until: query.until ?? null,
    userId: query.userId ?? null,
    tokenId: query.tokenId ?? null,
    keyId: query.keyId ?? null,
    requestKinds: [...(query.requestKinds ?? [])],
  })
}

interface AlertsCenterProps {
  language: Language
  search: string
  refreshToken: number
  onNavigate: (path: string) => void
  onOpenUser: (id: string) => void
  onOpenToken: (id: string) => void
  onOpenKey: (id: string) => void
  formatTime: (ts: number | null) => string
  formatTimeDetail: (ts: number | null) => string
  catalogLoader?: (signal?: AbortSignal) => Promise<AlertCatalog>
  eventsLoader?: (query: AlertsQuery, signal?: AbortSignal) => Promise<AlertsPage<AlertEvent>>
  groupsLoader?: (query: AlertsQuery, signal?: AbortSignal) => Promise<AlertsPage<AlertGroup>>
  requestLoader?: (requestId: number, signal?: AbortSignal) => Promise<RequestLogBodies>
  childRequestLoader?: (query: RequestLogsListQuery, signal?: AbortSignal) => Promise<RequestLogsListPage>
  initialCatalog?: AlertCatalog | null
  initialEventsPage?: AlertsPage<AlertEvent> | null
  initialGroupsPage?: AlertsPage<AlertGroup> | null
  disableAutoLoad?: boolean
  inlineTabsVariant?: 'all' | 'mobile'
}

interface SelectedChildDetails {
  child: AlertGroup
}

type ChildRequestOutcomeFilter = 'all' | 'success' | 'quota_exhausted' | 'error' | 'neutral'

interface ChildRequestFilterState {
  requestKind: string | null
  outcome: ChildRequestOutcomeFilter
  text: string
}

export default function AlertsCenter({
  language,
  search,
  refreshToken,
  onNavigate,
  onOpenUser,
  onOpenToken,
  onOpenKey,
  formatTime,
  formatTimeDetail,
  catalogLoader = fetchAlertCatalog,
  eventsLoader = fetchAlertEvents,
  groupsLoader = fetchAlertGroups,
  requestLoader = fetchRequestLogDetails,
  childRequestLoader = fetchRequestLogsList,
  initialCatalog = null,
  initialEventsPage = null,
  initialGroupsPage = null,
  disableAutoLoad = false,
  inlineTabsVariant = 'all',
}: AlertsCenterProps): JSX.Element {
  const copy = useMemo(() => defaultCopy(language), [language])
  const searchState = useMemo<AlertsSearchState>(
    () => ({
      view: getAlertsViewFromSearch(search),
      type: getAlertTypeFromSearch(search) as AlertType | null,
      since: getAlertSinceFromSearch(search),
      until: getAlertUntilFromSearch(search),
      userId: getAlertUserIdFromSearch(search),
      tokenId: getAlertTokenIdFromSearch(search),
      keyId: getAlertKeyIdFromSearch(search),
      requestKinds: getAlertRequestKindsFromSearch(search),
      page: getAlertPageFromSearch(search),
    }),
    [search],
  )
  const { view, type, since, until, userId, tokenId, keyId, requestKinds, page } = searchState

  const [draftSince, setDraftSince] = useState(() => isoToDateTimeLocal(since))
  const [draftUntil, setDraftUntil] = useState(() => isoToDateTimeLocal(until))
  const [catalog, setCatalog] = useState<AlertCatalog | null>(initialCatalog)
  const [catalogLoadState, setCatalogLoadState] = useState<QueryLoadState>(() =>
    initialCatalog ? 'ready' : 'initial_loading',
  )
  const [catalogError, setCatalogError] = useState<string | null>(null)
  const [eventsPage, setEventsPage] = useState<AlertsPage<AlertEvent>>(initialEventsPage ?? EMPTY_ALERT_EVENTS_PAGE)
  const [groupsPage, setGroupsPage] = useState<AlertsPage<AlertGroup>>(initialGroupsPage ?? EMPTY_ALERT_GROUPS_PAGE)
  const [listLoadState, setListLoadState] = useState<QueryLoadState>(() =>
    initialEventsPage || initialGroupsPage ? 'ready' : 'initial_loading',
  )
  const [listError, setListError] = useState<string | null>(null)
  const [expandedGroupIds, setExpandedGroupIds] = useState<string[]>([])
  const [selectedChildDetails, setSelectedChildDetails] = useState<SelectedChildDetails | null>(null)
  const [selectedRequest, setSelectedRequest] = useState<AlertEvent['request'] | null>(null)
  const [childRequestFilters, setChildRequestFilters] = useState<ChildRequestFilterState>({
    requestKind: null,
    outcome: 'all',
    text: '',
  })
  const [childRequestPage, setChildRequestPage] = useState<RequestLogsListPage>({
    items: [],
    pageSize: 50,
    nextCursor: null,
    prevCursor: null,
    hasOlder: false,
    hasNewer: false,
  })
  const [childRequestLoadState, setChildRequestLoadState] = useState<QueryLoadState>('initial_loading')
  const [childRequestLoadError, setChildRequestLoadError] = useState<string | null>(null)
  const [requestBodies, setRequestBodies] = useState<RequestLogBodies | null>(null)
  const [requestLoadState, setRequestLoadState] = useState<QueryLoadState>('initial_loading')
  const [requestLoadError, setRequestLoadError] = useState<string | null>(null)
  const hasLoadedCatalogRef = useRef(Boolean(initialCatalog))
  const currentPerPage = view === 'events' ? eventsPage.perPage : groupsPage.perPage
  const currentListQuery = useMemo<AlertsQuery>(
    () => ({
      page,
      perPage: currentPerPage,
      type,
      since,
      until,
      userId,
      tokenId,
      keyId,
      requestKinds,
    }),
    [currentPerPage, keyId, page, requestKinds, since, tokenId, type, until, userId],
  )
  const currentListQueryKey = useMemo(() => listQueryKey(view, currentListQuery), [currentListQuery, view])
  const hasInitialListPage = Boolean(view === 'events' ? initialEventsPage : initialGroupsPage)
  const hasLoadedListRef = useRef(hasInitialListPage)
  const lastListQueryKeyRef = useRef<string | null>(
    hasInitialListPage ? currentListQueryKey : null,
  )

  useEffect(() => {
    setDraftSince(isoToDateTimeLocal(since))
    setDraftUntil(isoToDateTimeLocal(until))
  }, [since, until])

  const navigateWith = useCallback(
    (patch: Partial<Parameters<typeof alertsPath>[0]>) => {
      onNavigate(
        alertsPath({
          view,
          type,
          since,
          until,
          userId,
          tokenId,
          keyId,
          requestKinds,
          page,
          ...patch,
        }),
      )
    },
    [keyId, onNavigate, page, requestKinds, since, tokenId, type, until, userId, view],
  )

  useEffect(() => {
    if (disableAutoLoad) return
    const controller = new AbortController()
    setCatalogLoadState(hasLoadedCatalogRef.current ? 'refreshing' : 'initial_loading')
    setCatalogError(null)
    catalogLoader(controller.signal)
      .then((value) => {
        if (controller.signal.aborted) return
        hasLoadedCatalogRef.current = true
        setCatalog(value)
        setCatalogLoadState('ready')
      })
      .catch((error) => {
        if (controller.signal.aborted) return
        if (!hasLoadedCatalogRef.current) {
          setCatalog(null)
        }
        setCatalogError(error instanceof Error ? error.message : 'Failed to load alert catalog')
        setCatalogLoadState('error')
      })
    return () => controller.abort()
  }, [catalogLoader, disableAutoLoad, refreshToken])

  useEffect(() => {
    if (disableAutoLoad) return
    const controller = new AbortController()
    const queryChanged = lastListQueryKeyRef.current !== currentListQueryKey
    setListLoadState(
      queryChanged
        ? getBlockingLoadState(hasLoadedListRef.current)
        : getRefreshingLoadState(hasLoadedListRef.current),
    )
    setListError(null)
    lastListQueryKeyRef.current = currentListQueryKey
    const loader = view === 'events'
      ? eventsLoader(currentListQuery, controller.signal)
      : groupsLoader(currentListQuery, controller.signal)
    loader
      .then((value) => {
        if (controller.signal.aborted) return
        hasLoadedListRef.current = true
        if (view === 'events') {
          setEventsPage(value as AlertsPage<AlertEvent>)
        } else {
          setGroupsPage(value as AlertsPage<AlertGroup>)
        }
        setListLoadState('ready')
      })
      .catch((error) => {
        if (controller.signal.aborted) return
        if (view === 'events') {
          setEventsPage({ ...EMPTY_ALERT_EVENTS_PAGE, page, perPage: currentListQuery.perPage ?? 20 })
        } else {
          setGroupsPage({ ...EMPTY_ALERT_GROUPS_PAGE, page, perPage: currentListQuery.perPage ?? 20 })
        }
        setListError(error instanceof Error ? error.message : 'Failed to load alerts')
        setListLoadState('error')
      })
    return () => controller.abort()
  }, [
    currentListQuery,
    currentListQueryKey,
    disableAutoLoad,
    eventsLoader,
    groupsLoader,
    refreshToken,
    view,
  ])

  useEffect(() => {
    if (!selectedRequest?.id) {
      setRequestBodies(null)
      setRequestLoadState('initial_loading')
      setRequestLoadError(null)
      return
    }
    const controller = new AbortController()
    setRequestLoadState('initial_loading')
    setRequestLoadError(null)
    requestLoader(selectedRequest.id, controller.signal)
      .then((value) => {
        if (controller.signal.aborted) return
        setRequestBodies(value)
        setRequestLoadState('ready')
      })
      .catch((error) => {
        if (controller.signal.aborted) return
        setRequestBodies(null)
        setRequestLoadError(error instanceof Error ? error.message : copy.requestDrawer.error)
        setRequestLoadState('error')
      })
    return () => controller.abort()
  }, [copy.requestDrawer.error, requestLoader, selectedRequest])

  useEffect(() => {
    if (!selectedChildDetails) {
      setChildRequestPage({
        items: [],
        pageSize: 50,
        nextCursor: null,
        prevCursor: null,
        hasOlder: false,
        hasNewer: false,
      })
      setChildRequestLoadState('initial_loading')
      setChildRequestLoadError(null)
      return
    }
    const controller = new AbortController()
    const child = selectedChildDetails.child
    setChildRequestLoadState('initial_loading')
    setChildRequestLoadError(null)
    childRequestLoader(
      {
        limit: 50,
        userId: child.user?.userId ?? (child.subjectKind === 'user' ? child.subjectId : undefined),
        tokenId: child.token?.id ?? (child.subjectKind === 'token' ? child.subjectId : undefined),
        keyId: child.key?.id ?? (child.subjectKind === 'key' ? child.subjectId : undefined),
        since: child.semanticWindowStart ?? child.firstSeen,
        untilIso:
          child.semanticWindowEnd != null
            ? formatIso8601WithOffset(new Date(child.semanticWindowEnd * 1000))
            : undefined,
      },
      controller.signal,
    )
      .then((value) => {
        if (controller.signal.aborted) return
        setChildRequestPage(value)
        setChildRequestLoadState('ready')
      })
      .catch((error) => {
        if (controller.signal.aborted) return
        setChildRequestPage({
          items: [],
          pageSize: 50,
          nextCursor: null,
          prevCursor: null,
          hasOlder: false,
          hasNewer: false,
        })
        setChildRequestLoadError(error instanceof Error ? error.message : copy.childDrawer.empty)
        setChildRequestLoadState('error')
      })
    return () => controller.abort()
  }, [childRequestLoader, copy.childDrawer.empty, selectedChildDetails])

  const currentPage = view === 'events' ? eventsPage : groupsPage
  const totalPageCount = totalPages(currentPage.total, currentPage.perPage)
  const typeOptions = useMemo(
    () =>
      (catalog?.types ?? []).map((option) => ({
        value: option.value,
        label: copy.types[option.value as AlertType] ?? option.value,
        count: option.count,
      })),
    [catalog?.types, copy.types],
  )
  const requestKindsSummary =
    requestKinds.length === 0
      ? copy.filters.requestKindsAll
      : requestKinds.length === 1
        ? (catalog?.requestKindOptions.find((option) => option.key === requestKinds[0])?.label ?? requestKinds[0])
        : language === 'zh'
          ? `已选 ${requestKinds.length} 项`
          : `${requestKinds.length} selected`
  const cleanedBodySummary = useCallback(
    (source: RequestLogBodies | null, kind: 'request' | 'response') =>
      source
        ? cleanedRequestLogBodySummary({
            source,
            kind,
            language,
            noBodyLabel: copy.requestDrawer.noBody,
            emptyValueLabel: '—',
            formatTime: (ts) => formatTime(ts),
          })
        : copy.requestDrawer.noBody,
    [copy.requestDrawer.noBody, formatTime, language],
  )
  const requestBody = requestBodies?.request_body ?? cleanedBodySummary(requestBodies, 'request')
  const responseBody = requestBodies?.response_body ?? cleanedBodySummary(requestBodies, 'response')
  const childRequestRecords = childRequestPage.items
  const childRequestKindOptions = useMemo(() => {
    const options = new Map<string, { value: string; label: string; count: number }>()
    for (const record of childRequestRecords) {
      const key = record.request_kind_key?.trim()
      if (!key) continue
      const current = options.get(key)
      if (current) {
        current.count += 1
      } else {
        options.set(key, {
          value: key,
          label: record.request_kind_label ?? key,
          count: 1,
        })
      }
    }
    return [...options.values()]
  }, [childRequestRecords])
  const filteredChildRequestRecords = useMemo(() => {
    const normalizedText = childRequestFilters.text.trim().toLowerCase()
    return childRequestRecords.filter((record) => {
      if (childRequestFilters.requestKind && record.request_kind_key !== childRequestFilters.requestKind) {
        return false
      }
      if (childRequestFilters.outcome !== 'all' && record.result_status !== childRequestFilters.outcome) {
        return false
      }
      if (!normalizedText) return true
      const haystacks = [
        record.method,
        record.path,
        record.query ?? '',
        record.request_kind_label ?? '',
        record.request_kind_detail ?? '',
        record.error_message ?? '',
        record.auth_token_id ?? '',
        record.key_id ?? '',
      ]
      return haystacks.some((candidate) => candidate.toLowerCase().includes(normalizedText))
    })
  }, [childRequestFilters, childRequestRecords])
  const toggleExpandedGroup = useCallback((groupId: string) => {
    setExpandedGroupIds((current) =>
      current.includes(groupId)
        ? current.filter((value) => value !== groupId)
        : [...current, groupId],
    )
  }, [])
  return (
    <div className="alerts-center-stack">
      <section className="surface panel alerts-center-panel">
        <div className="alerts-center-toolbar">
          <div className={`alerts-center-tabs-mobile${inlineTabsVariant === 'mobile' ? ' admin-stacked-only' : ''}`}>
            <SegmentedTabs<AlertsCenterView>
              className="alerts-center-tabs"
              value={view}
              onChange={(nextView) => onNavigate(alertsPath({ view: nextView, type, since, until, userId, tokenId, keyId, requestKinds }))}
              options={[
                { value: 'events', label: copy.tabs.events },
                { value: 'groups', label: copy.tabs.groups },
              ]}
              ariaLabel={copy.title}
              collapseMode="never"
            />
          </div>

          <div className="alerts-center-filters alerts-center-filters--primary">
            <div className="alerts-center-filter-field alerts-center-filter-field--type">
              <span className="alerts-center-filter-label">{copy.filters.type}</span>
              <SearchableFacetSelect
                value={type}
                options={typeOptions}
                summary={type ? copy.types[type] ?? type : copy.filters.allTypes}
                allLabel={copy.filters.allTypes}
                emptyLabel={copy.filters.allTypes}
                searchPlaceholder={copy.filters.searchPlaceholder}
                searchAriaLabel={copy.filters.type}
                triggerAriaLabel={copy.filters.type}
                listAriaLabel={copy.filters.type}
                onChange={(nextType) => navigateWith({ type: nextType, page: 1 })}
              />
            </div>

            <div className="alerts-center-filter-field alerts-center-filter-field--request-kinds">
              <span className="alerts-center-filter-label">{copy.filters.requestKinds}</span>
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <button
                    type="button"
                    className="searchable-facet-select__trigger alerts-center-request-kinds-trigger"
                    aria-label={copy.filters.requestKinds}
                  >
                    <span className="searchable-facet-select__summary">{requestKindsSummary}</span>
                    <Icon icon="mdi:chevron-down" width={16} height={16} aria-hidden="true" />
                  </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="start" className="alerts-center-request-kinds-menu">
                  {(catalog?.requestKindOptions ?? []).length === 0 ? (
                    <div className="alerts-center-request-kinds-empty">{copy.filters.requestKindsEmpty}</div>
                  ) : (
                    catalog?.requestKindOptions.map((option) => {
                      const checked = requestKinds.includes(option.key)
                      return (
                        <DropdownMenuCheckboxItem
                          key={option.key}
                          checked={checked}
                          onCheckedChange={() => {
                            const nextRequestKinds = checked
                              ? requestKinds.filter((value) => value !== option.key)
                              : [...requestKinds, option.key]
                            navigateWith({ requestKinds: nextRequestKinds, page: 1 })
                          }}
                        >
                          <span className="alerts-center-request-kinds-option">
                            <span>{option.label}</span>
                            <span className="alerts-center-request-kinds-count">x{option.count}</span>
                          </span>
                        </DropdownMenuCheckboxItem>
                      )
                    })
                  )}
                </DropdownMenuContent>
              </DropdownMenu>
            </div>

            <div className="alerts-center-filter-field alerts-center-filter-field--since">
              <span className="alerts-center-filter-label">{copy.filters.since}</span>
              <Input
                type="datetime-local"
                className="alerts-center-time-input"
                value={draftSince}
                onChange={(event) => setDraftSince(event.target.value)}
              />
            </div>
            <div className="alerts-center-filter-field alerts-center-filter-field--until">
              <span className="alerts-center-filter-label">{copy.filters.until}</span>
              <Input
                type="datetime-local"
                className="alerts-center-time-input"
                value={draftUntil}
                onChange={(event) => setDraftUntil(event.target.value)}
              />
            </div>
            <div className="alerts-center-filter-field alerts-center-filter-field--user">
              <span className="alerts-center-filter-label">{copy.filters.user}</span>
              <SearchableFacetSelect
                value={userId}
                options={catalog?.users ?? []}
                summary={catalog?.users.find((option) => option.value === userId)?.label ?? copy.filters.allUsers}
                allLabel={copy.filters.allUsers}
                emptyLabel={copy.filters.allUsers}
                searchPlaceholder={copy.filters.searchPlaceholder}
                searchAriaLabel={copy.filters.user}
                triggerAriaLabel={copy.filters.user}
                listAriaLabel={copy.filters.user}
                onChange={(nextUserId) => navigateWith({ userId: nextUserId, page: 1 })}
              />
            </div>
            <div className="alerts-center-filter-field alerts-center-filter-field--token">
              <span className="alerts-center-filter-label">{copy.filters.token}</span>
              <SearchableFacetSelect
                value={tokenId}
                options={catalog?.tokens ?? []}
                summary={catalog?.tokens.find((option) => option.value === tokenId)?.label ?? copy.filters.allTokens}
                allLabel={copy.filters.allTokens}
                emptyLabel={copy.filters.allTokens}
                searchPlaceholder={copy.filters.searchPlaceholder}
                searchAriaLabel={copy.filters.token}
                triggerAriaLabel={copy.filters.token}
                listAriaLabel={copy.filters.token}
                onChange={(nextTokenId) => navigateWith({ tokenId: nextTokenId, page: 1 })}
                labelVariant="mono"
              />
            </div>
            <div className="alerts-center-filter-field alerts-center-filter-field--key">
              <span className="alerts-center-filter-label">{copy.filters.key}</span>
              <SearchableFacetSelect
                value={keyId}
                options={catalog?.keys ?? []}
                summary={catalog?.keys.find((option) => option.value === keyId)?.label ?? copy.filters.allKeys}
                allLabel={copy.filters.allKeys}
                emptyLabel={copy.filters.allKeys}
                searchPlaceholder={copy.filters.searchPlaceholder}
                searchAriaLabel={copy.filters.key}
                triggerAriaLabel={copy.filters.key}
                listAriaLabel={copy.filters.key}
                onChange={(nextKeyId) => navigateWith({ keyId: nextKeyId, page: 1 })}
                labelVariant="mono"
              />
            </div>
            <div className="alerts-center-filter-actions">
              <Button
                type="button"
                variant="outline"
                onClick={() =>
                  navigateWith({
                    since: dateTimeLocalToIso(draftSince),
                    until: dateTimeLocalToIso(draftUntil),
                    page: 1,
                  })
                }
              >
                {copy.filters.applyTime}
              </Button>
              <Button type="button" variant="outline" className="alerts-center-clear-button" onClick={() => onNavigate(alertsPath({ view: 'groups' }))}>
                {copy.filters.clear}
              </Button>
            </div>
          </div>
        </div>

        <AdminLoadingRegion loadState={catalogLoadState} loadingLabel={copy.title} errorLabel={catalogError}>
          {view === 'events' ? (
            <AdminTableShell
              className="alerts-center-table-shell"
              tableClassName="alerts-center-table alerts-center-table--events"
              loadState={listLoadState}
              loadingLabel={copy.title}
              errorLabel={listError}
            >
              <TableHeader>
                <TableRow>
                  <TableHead className="alerts-center-col alerts-center-col--time">{copy.table.events.time}</TableHead>
                  <TableHead className="alerts-center-col alerts-center-col--type">{copy.table.events.type}</TableHead>
                  <TableHead className="alerts-center-col alerts-center-col--subject">{copy.table.events.subject}</TableHead>
                  <TableHead className="alerts-center-col alerts-center-col--request-kind">{copy.table.events.requestKind}</TableHead>
                  <TableHead className="alerts-center-col alerts-center-col--related">{copy.table.events.related}</TableHead>
                  <TableHead className="alerts-center-col alerts-center-col--request">{copy.table.events.request}</TableHead>
                  <TableHead className="alerts-center-col alerts-center-col--summary">{copy.table.events.summary}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {eventsPage.items.length === 0 ? (
                  <TableRow>
                    <TableCell colSpan={7}>
                      <div className="empty-state alert">{copy.emptyEvents}</div>
                    </TableCell>
                  </TableRow>
                ) : (
                  eventsPage.items.map((event) => (
                    <TableRow key={event.id}>
                      <TableCell className="alerts-center-col alerts-center-col--time">
                        <div className="alerts-center-time-cell">
                          <strong>{formatTime(event.occurredAt)}</strong>
                          <span>{formatTimeDetail(event.occurredAt)}</span>
                        </div>
                      </TableCell>
                      <TableCell className="alerts-center-col alerts-center-col--type">
                        <StatusBadge tone={alertTypeTone(event.type)}>{copy.types[event.type]}</StatusBadge>
                      </TableCell>
                      <TableCell className="alerts-center-col alerts-center-col--subject">
                        <div className="alerts-center-subject-cell">
                          <strong>{subjectDisplayLabel(event)}</strong>
                        </div>
                      </TableCell>
                      <TableCell className="alerts-center-col alerts-center-col--request-kind">
                        {event.requestKind ? (
                          <RequestKindBadge requestKindKey={event.requestKind.key} requestKindLabel={event.requestKind.label} size="sm" />
                        ) : '—'}
                      </TableCell>
                      <TableCell className="alerts-center-col alerts-center-col--related">
                        <div className="alerts-center-related-actions">
                          {event.user ? (
                            <button type="button" className="alerts-center-related-link" onClick={() => onOpenUser(event.user!.userId)}>
                              {event.user.displayName ?? event.user.username ?? event.user.userId}
                            </button>
                          ) : null}
                          {event.token ? (
                            <button type="button" className="alerts-center-related-link alerts-center-related-link--mono" onClick={() => onOpenToken(event.token!.id)}>
                              {event.token.label ?? event.token.id}
                            </button>
                          ) : null}
                          {event.key ? (
                            <button type="button" className="alerts-center-related-link alerts-center-related-link--mono" onClick={() => onOpenKey(event.key!.id)}>
                              {event.key.label ?? event.key.id}
                            </button>
                          ) : null}
                        </div>
                      </TableCell>
                      <TableCell className="alerts-center-col alerts-center-col--request">
                        {event.request ? (
                          <button type="button" className="alerts-center-request-link" onClick={() => setSelectedRequest(event.request)}>
                            {requestSummary(event.request)}
                          </button>
                        ) : (
                          '—'
                        )}
                      </TableCell>
                      <TableCell className="alerts-center-col alerts-center-col--summary">
                        <div className="alerts-center-summary-cell">
                          <strong>{event.title}</strong>
                          <span>{event.summary}</span>
                        </div>
                      </TableCell>
                    </TableRow>
                  ))
                )}
              </TableBody>
            </AdminTableShell>
          ) : (
            <AdminTableShell
              className="alerts-center-table-shell"
              tableClassName="alerts-center-table alerts-center-table--groups"
              loadState={listLoadState}
              loadingLabel={copy.title}
              errorLabel={listError}
            >
              <TableHeader>
                <TableRow>
                  <TableHead className="alerts-center-col alerts-center-col--expander" />
                  <TableHead className="alerts-center-col alerts-center-col--time">{copy.table.groups.time}</TableHead>
                  <TableHead className="alerts-center-col alerts-center-col--type">{copy.table.groups.type}</TableHead>
                  <TableHead className="alerts-center-col alerts-center-col--subject">{copy.table.groups.subject}</TableHead>
                  <TableHead className="alerts-center-col alerts-center-col--request-kind">{copy.table.groups.requestKind}</TableHead>
                  <TableHead className="alerts-center-col alerts-center-col--summary">{copy.table.groups.latest}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {groupsPage.items.length === 0 ? (
                  <TableRow>
                    <TableCell colSpan={6}>
                      <div className="empty-state alert">{copy.emptyGroups}</div>
                    </TableCell>
                  </TableRow>
                ) : (
                  groupsPage.items.map((group) => {
                    const expanded = expandedGroupIds.includes(group.id)
                    const canExpand = isSemanticMother(group)
                    const compatibilityGroup = isCompatibilityGroup(group)
                    return (
                      <Fragment key={group.id}>
                        <TableRow key={group.id}>
                          <TableCell className="alerts-center-col alerts-center-col--expander">
                            {canExpand ? (
                              <button
                                type="button"
                                className="alerts-center-row-expander"
                                onClick={() => toggleExpandedGroup(group.id)}
                                aria-label={expanded ? copy.groupUi.collapse : copy.groupUi.expand}
                                aria-expanded={expanded}
                              >
                                <Icon icon={expanded ? 'mdi:chevron-up' : 'mdi:chevron-down'} width={18} height={18} aria-hidden="true" />
                              </button>
                            ) : null}
                          </TableCell>
                          <TableCell className="alerts-center-col alerts-center-col--time">
                            <div className="alerts-center-time-cell alerts-center-time-cell--range">
                              <strong>{formatMonthDayTimeWithSeconds(group.firstSeen, language)}</strong>
                              <span>{formatMonthDayTimeWithSeconds(group.lastSeen, language)}</span>
                            </div>
                          </TableCell>
                          <TableCell className="alerts-center-col alerts-center-col--type">
                            <StatusBadge tone={alertTypeTone(group.type)}>{copy.types[group.type]}</StatusBadge>
                          </TableCell>
                          <TableCell className="alerts-center-col alerts-center-col--subject">
                            <div className="alerts-center-subject-cell">
                              {hasClickableGroupSubject(group) ? (
                                <button
                                  type="button"
                                  className={`alerts-center-related-link${group.subjectKind !== 'user' ? ' alerts-center-related-link--mono' : ''}`}
                                  onClick={() => {
                                    if (group.subjectKind === 'user' && group.user?.userId) {
                                      onOpenUser(group.user.userId)
                                      return
                                    }
                                    if (group.subjectKind === 'token' && group.token?.id) {
                                      onOpenToken(group.token.id)
                                      return
                                    }
                                    if (group.subjectKind === 'key' && group.key?.id) {
                                      onOpenKey(group.key.id)
                                    }
                                  }}
                                >
                                  {subjectDisplayLabel(group)}
                                </button>
                              ) : (
                                <strong>{subjectDisplayLabel(group)}</strong>
                              )}
                            </div>
                          </TableCell>
                          <TableCell className="alerts-center-col alerts-center-col--request-kind">
                            <div className="alerts-center-summary-cell">
                              <strong>{compatibilityGroup ? '—' : semanticWindowLabel(group, language)}</strong>
                              {(group.groupingKind ?? 'compat') === 'mother' ? (
                                <span>{`x${group.eventCount ?? group.count} · ${group.childCount ?? group.children?.length ?? 0} ${copy.groupUi.children}`}</span>
                              ) : (
                                <span>{`x${group.eventCount ?? group.count}`}</span>
                              )}
                            </div>
                          </TableCell>
                          <TableCell className="alerts-center-col alerts-center-col--summary">
                            <div className="alerts-center-summary-cell">
                              <strong>{group.latestEvent.title}</strong>
                              <span>{group.latestEvent.summary}</span>
                            </div>
                          </TableCell>
                        </TableRow>
                        {canExpand && expanded
                          ? (group.children ?? []).map((child) => {
                              return (
                                <Fragment key={child.id}>
                                  <TableRow key={`${child.id}:summary`} className="alerts-center-child-row">
                                    <TableCell className="alerts-center-col alerts-center-col--expander alerts-center-child-row__expander" />
                                    <TableCell className="alerts-center-col alerts-center-col--time">
                                      <div className="alerts-center-time-cell alerts-center-child-window">
                                        <strong>{semanticWindowLabel(child, language)}</strong>
                                        {child.semanticWindowStart != null && child.semanticWindowEnd != null ? (
                                          <span>
                                            {`${formatMonthDayTimeWithSeconds(child.semanticWindowStart, language)} → ${formatMonthDayTimeWithSeconds(child.semanticWindowEnd, language)}`}
                                          </span>
                                        ) : null}
                                      </div>
                                    </TableCell>
                                    <TableCell className="alerts-center-col alerts-center-col--type">
                                      <div className="alerts-center-child-stat">
                                        <strong>x{child.eventCount ?? child.count}</strong>
                                        <span>{copy.groupUi.children}</span>
                                      </div>
                                    </TableCell>
                                    <TableCell className="alerts-center-col alerts-center-col--subject">
                                      <div className="alerts-center-summary-cell">
                                        <strong>{`${formatTime(child.firstSeen)} · ${formatTimeDetail(child.firstSeen)}`}</strong>
                                        <span>{copy.groupUi.firstHit}</span>
                                      </div>
                                    </TableCell>
                                    <TableCell className="alerts-center-col alerts-center-col--request-kind">
                                      <div className="alerts-center-summary-cell">
                                        <strong>{`${formatTime(child.lastSeen)} · ${formatTimeDetail(child.lastSeen)}`}</strong>
                                        <span>{copy.groupUi.lastHit}</span>
                                      </div>
                                    </TableCell>
                                    <TableCell className="alerts-center-col alerts-center-col--summary">
                                      <div className="alerts-center-child-summary-row">
                                        <div className="alerts-center-summary-cell">
                                          <strong>{child.latestEvent.title}</strong>
                                          <span>{child.latestEvent.summary}</span>
                                        </div>
                                        <button
                                          type="button"
                                          className="alerts-center-inline-toggle"
                                          onClick={() => {
                                            setChildRequestFilters({
                                              requestKind: null,
                                              outcome: 'all',
                                              text: '',
                                            })
                                            setSelectedChildDetails({
                                              child,
                                            })
                                          }}
                                        >
                                          <Icon icon="mdi:format-list-bulleted-square" width={16} height={16} aria-hidden="true" />
                                          <span>{`${copy.groupUi.expand} ${child.eventCount ?? child.count} ${copy.groupUi.requestRecords}`}</span>
                                        </button>
                                      </div>
                                    </TableCell>
                                  </TableRow>
                                </Fragment>
                              )
                            })
                          : null}
                      </Fragment>
                    )
                  })
                )}
              </TableBody>
            </AdminTableShell>
          )}

          <AdminTablePagination
            page={currentPage.page}
            totalPages={totalPageCount}
            pageSummary={paginationSummary(copy, currentPage.total, currentPage.page, currentPage.perPage)}
            perPage={currentPage.perPage}
            previousLabel={copy.paginationPrevious}
            nextLabel={copy.paginationNext}
            previousDisabled={currentPage.page <= 1}
            nextDisabled={currentPage.page >= totalPageCount}
            onPrevious={() => navigateWith({ page: Math.max(1, currentPage.page - 1) })}
            onNext={() => navigateWith({ page: Math.min(totalPageCount, currentPage.page + 1) })}
            onPerPageChange={(nextPerPage) => {
              if (view === 'events') {
                setEventsPage((current) => ({ ...current, perPage: nextPerPage, page: 1 }))
              } else {
                setGroupsPage((current) => ({ ...current, perPage: nextPerPage, page: 1 }))
              }
              navigateWith({ page: 1 })
            }}
          />
        </AdminLoadingRegion>
      </section>

      <Drawer open={selectedRequest != null} onOpenChange={(open) => !open && setSelectedRequest(null)} shouldScaleBackground={false}>
        <DrawerContent className="request-entity-drawer-content-fit">
          <section className="alerts-center-request-drawer">
            <header className="alerts-center-request-drawer__header">
              <DrawerTitle asChild>
                <h3>{copy.requestDrawer.title}</h3>
              </DrawerTitle>
              <DrawerDescription asChild>
                <p className="panel-description">{requestSummary(selectedRequest)}</p>
              </DrawerDescription>
            </header>

            <AdminLoadingRegion
              loadState={requestLoadState}
              loadingLabel={copy.requestDrawer.loading}
              errorLabel={requestLoadError ?? copy.requestDrawer.error}
            >
              <div className="alerts-center-request-drawer__grid">
                <div>
                  <h4>{copy.requestDrawer.requestBody}</h4>
                  <pre>{requestBody}</pre>
                </div>
                <div>
                  <h4>{copy.requestDrawer.responseBody}</h4>
                  <pre>{responseBody}</pre>
                </div>
              </div>
            </AdminLoadingRegion>
          </section>
        </DrawerContent>
      </Drawer>

      <Drawer
        open={selectedChildDetails != null}
        onOpenChange={(open) => !open && setSelectedChildDetails(null)}
        shouldScaleBackground={false}
        direction="right"
      >
        <DrawerContent direction="right" className="alerts-center-child-drawer">
          <section className="alerts-center-request-drawer alerts-center-child-drawer__content">
            <header className="alerts-center-request-drawer__header alerts-center-child-drawer__header">
              <DrawerTitle asChild>
                <h3>{copy.childDrawer.title}</h3>
              </DrawerTitle>
              <DrawerDescription asChild>
                <p className="panel-description">
                  {selectedChildDetails
                    ? `${semanticWindowLabel(selectedChildDetails.child, language)} · ${childRequestRecords.length} ${copy.groupUi.requestRecords}`
                    : '—'}
                </p>
              </DrawerDescription>
            </header>

            <div className="alerts-center-child-request-filters">
              <div className="alerts-center-filter-field">
                <span className="alerts-center-filter-label">{copy.childDrawer.requestKind}</span>
                <SearchableFacetSelect
                  value={childRequestFilters.requestKind}
                  options={childRequestKindOptions}
                  summary={
                    childRequestFilters.requestKind == null
                      ? copy.childDrawer.allRequestKinds
                      : childRequestKindOptions.find((option) => option.value === childRequestFilters.requestKind)?.label ??
                        childRequestFilters.requestKind
                  }
                  allLabel={copy.childDrawer.allRequestKinds}
                  emptyLabel={copy.childDrawer.noRequestKinds}
                  searchPlaceholder={copy.filters.searchPlaceholder}
                  searchAriaLabel={copy.childDrawer.requestKind}
                  triggerAriaLabel={copy.childDrawer.requestKind}
                  listAriaLabel={copy.childDrawer.requestKind}
                  onChange={(nextValue) =>
                    setChildRequestFilters((current) => ({
                      ...current,
                      requestKind: nextValue,
                    }))}
                />
              </div>
              <div className="alerts-center-filter-field">
                <span className="alerts-center-filter-label">{copy.childDrawer.outcome}</span>
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <button type="button" className="searchable-facet-select__trigger alerts-center-request-kinds-trigger">
                      <span className="searchable-facet-select__summary">
                        {childRequestFilters.outcome === 'all'
                          ? copy.childDrawer.allOutcomes
                          : childRequestFilters.outcome === 'quota_exhausted'
                            ? copy.childDrawer.quotaExhausted
                            : childRequestFilters.outcome === 'success'
                              ? copy.childDrawer.success
                              : childRequestFilters.outcome === 'error'
                                ? copy.childDrawer.error
                                : copy.childDrawer.neutral}
                      </span>
                      <Icon icon="mdi:chevron-down" width={16} height={16} aria-hidden="true" />
                    </button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="start" className="alerts-center-request-kinds-menu">
                    {[
                      ['all', copy.childDrawer.allOutcomes],
                      ['quota_exhausted', copy.childDrawer.quotaExhausted],
                      ['success', copy.childDrawer.success],
                      ['error', copy.childDrawer.error],
                      ['neutral', copy.childDrawer.neutral],
                    ].map(([value, label]) => (
                      <button
                        key={value}
                        type="button"
                        className={`searchable-facet-select__option${childRequestFilters.outcome === value ? ' searchable-facet-select__option--active' : ''}`}
                        onClick={() =>
                          setChildRequestFilters((current) => ({
                            ...current,
                            outcome: value as ChildRequestOutcomeFilter,
                          }))}
                      >
                        <span className="searchable-facet-select__mark" aria-hidden="true">
                          {childRequestFilters.outcome === value ? <Icon icon="mdi:check" width={16} height={16} /> : null}
                        </span>
                        <span className="searchable-facet-select__option-body">
                          <span className="searchable-facet-select__label">{label}</span>
                        </span>
                      </button>
                    ))}
                  </DropdownMenuContent>
                </DropdownMenu>
              </div>
              <div className="alerts-center-filter-field alerts-center-child-request-filters__search">
                <span className="alerts-center-filter-label">{copy.childDrawer.search}</span>
                <Input
                  value={childRequestFilters.text}
                  onChange={(event) =>
                    setChildRequestFilters((current) => ({
                      ...current,
                      text: event.target.value,
                    }))}
                  placeholder={copy.childDrawer.searchPlaceholder}
                  className="alerts-center-time-input"
                />
              </div>
            </div>

            <div className="alerts-center-child-events alerts-center-child-request-list">
              {selectedChildDetails == null ? (
                <div className="alerts-center-inline-muted">{copy.childDrawer.empty}</div>
              ) : childRequestLoadState === 'error' ? (
                <div className="alerts-center-inline-muted">{childRequestLoadError ?? copy.childDrawer.empty}</div>
              ) : childRequestLoadState !== 'ready' ? (
                <div className="alerts-center-inline-muted">{copy.requestDrawer.loading}</div>
              ) : childRequestRecords.length === 0 ? (
                <div className="alerts-center-inline-muted">{copy.childDrawer.empty}</div>
              ) : filteredChildRequestRecords.length === 0 ? (
                <div className="alerts-center-inline-muted">{copy.childDrawer.emptyFiltered}</div>
              ) : (
                filteredChildRequestRecords.map((log) => (
                  <div key={log.id} className="alerts-center-child-event alerts-center-child-request-item">
                    <div className="alerts-center-child-event__meta">
                      <StatusBadge tone={alertTypeTone(log.result_status === 'quota_exhausted' ? 'user_quota_exhausted' : 'user_request_rate_limited')}>
                        {log.result_status === 'quota_exhausted'
                          ? copy.types.user_quota_exhausted
                          : copy.types.user_request_rate_limited}
                      </StatusBadge>
                      <span>{formatMonthDayTimeWithSeconds(log.created_at, language)}</span>
                      {log.request_kind_key ? (
                        <RequestKindBadge requestKindKey={log.request_kind_key} requestKindLabel={log.request_kind_label ?? log.request_kind_key} size="sm" />
                      ) : null}
                    </div>
                    <div className="alerts-center-summary-cell">
                      <strong>{`${log.method} ${log.path}${log.query ? `?${log.query}` : ''}`}</strong>
                      <span>{log.error_message?.trim() || requestSummary({ id: log.id, method: log.method, path: log.path, query: log.query })}</span>
                    </div>
                    <div className="alerts-center-related-actions">
                      {selectedChildDetails?.child.user?.userId ? (
                        <button type="button" className="alerts-center-related-link" onClick={() => onOpenUser(selectedChildDetails.child.user!.userId)}>
                          {selectedChildDetails.child.user.displayName ?? selectedChildDetails.child.user.username ?? selectedChildDetails.child.user.userId}
                        </button>
                      ) : null}
                      {log.auth_token_id ? (
                        <button type="button" className="alerts-center-related-link alerts-center-related-link--mono" onClick={() => onOpenToken(log.auth_token_id!)}>
                          {log.auth_token_id}
                        </button>
                      ) : null}
                      {log.key_id ? (
                        <button type="button" className="alerts-center-related-link alerts-center-related-link--mono" onClick={() => onOpenKey(log.key_id!)}>
                          {log.key_id}
                        </button>
                      ) : null}
                      <button
                        type="button"
                        className="alerts-center-request-link"
                        onClick={() =>
                          setSelectedRequest({
                            id: log.id,
                            method: log.method,
                            path: log.path,
                            query: log.query,
                          })}
                      >
                        {requestSummary({ id: log.id, method: log.method, path: log.path, query: log.query })}
                      </button>
                    </div>
                  </div>
                ))
              )}
            </div>
          </section>
        </DrawerContent>
      </Drawer>
    </div>
  )
}
