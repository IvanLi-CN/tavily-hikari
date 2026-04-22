import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import type {
  AlertCatalog,
  AlertEvent,
  AlertGroup,
  AlertType,
  AlertsQuery,
  AlertsPage,
  RequestLogBodies,
} from '../api'
import { fetchAlertCatalog, fetchAlertEvents, fetchAlertGroups, fetchRequestLogDetails } from '../api'
import type { Language } from '../i18n'
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
import { Button } from '../components/ui/button'
import { Drawer, DrawerContent } from '../components/ui/drawer'
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

function totalPages(total: number, perPage: number): number {
  return Math.max(1, Math.ceil(total / Math.max(1, perPage)))
}

function requestSummary(request: AlertEvent['request']): string {
  if (!request) return '—'
  const query = request.query ? `?${request.query}` : ''
  return `${request.method} ${request.path}${query}`
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
            time: '最新命中',
            type: '类型',
            subject: '主体',
            requestKind: '请求类型',
            count: '次数',
            firstSeen: '首次',
            latest: '最新事件',
          },
        },
        emptyEvents: '当前筛选下没有告警事件。',
        emptyGroups: '当前筛选下没有告警分组。',
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
            time: 'Last seen',
            type: 'Type',
            subject: 'Subject',
            requestKind: 'Request kind',
            count: 'Count',
            firstSeen: 'First seen',
            latest: 'Latest event',
          },
        },
        emptyEvents: 'No alert events match the current filters.',
        emptyGroups: 'No alert groups match the current filters.',
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
  initialCatalog?: AlertCatalog | null
  initialEventsPage?: AlertsPage<AlertEvent> | null
  initialGroupsPage?: AlertsPage<AlertGroup> | null
  disableAutoLoad?: boolean
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
  initialCatalog = null,
  initialEventsPage = null,
  initialGroupsPage = null,
  disableAutoLoad = false,
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
  const [selectedRequest, setSelectedRequest] = useState<AlertEvent['request'] | null>(null)
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

  return (
    <div className="alerts-center-stack">
      <section className="surface panel alerts-center-hero">
        <div className="panel-header">
          <div>
            <h2>{copy.title}</h2>
            <p className="panel-description">{copy.description}</p>
          </div>
          <Button type="button" variant="outline" onClick={() => onNavigate(alertsPath({ view }))}>
            {copy.filters.clear}
          </Button>
        </div>
      </section>

      <section className="surface panel alerts-center-panel">
        <div className="alerts-center-toolbar">
          <SegmentedTabs<AlertsCenterView>
            className="alerts-center-tabs"
            value={view}
            onChange={(nextView) => onNavigate(alertsPath({ view: nextView, type, since, until, userId, tokenId, keyId, requestKinds }))}
            options={[
              { value: 'events', label: copy.tabs.events },
              { value: 'groups', label: copy.tabs.groups },
            ]}
            ariaLabel={copy.title}
          />

          <div className="alerts-center-filters">
            <div className="alerts-center-filter-field">
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

            <div className="alerts-center-filter-field">
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

            <div className="alerts-center-filter-field">
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

            <div className="alerts-center-filter-field">
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

            <div className="alerts-center-filter-field alerts-center-filter-field--request-kinds">
              <span className="alerts-center-filter-label">{copy.filters.requestKinds}</span>
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button type="button" variant="outline" className="alerts-center-request-kinds-trigger">
                    {requestKindsSummary}
                  </Button>
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

            <div className="alerts-center-filter-field alerts-center-filter-field--time">
              <span className="alerts-center-filter-label">{copy.filters.since}</span>
              <Input type="datetime-local" value={draftSince} onChange={(event) => setDraftSince(event.target.value)} />
            </div>
            <div className="alerts-center-filter-field alerts-center-filter-field--time">
              <span className="alerts-center-filter-label">{copy.filters.until}</span>
              <Input type="datetime-local" value={draftUntil} onChange={(event) => setDraftUntil(event.target.value)} />
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
            </div>
          </div>
        </div>

        <AdminLoadingRegion loadState={catalogLoadState} loadingLabel={copy.title} errorLabel={catalogError}>
          {view === 'events' ? (
            <AdminTableShell
              className="alerts-center-table-shell"
              loadState={listLoadState}
              loadingLabel={copy.title}
              errorLabel={listError}
            >
              <TableHeader>
                <TableRow>
                  <TableHead>{copy.table.events.time}</TableHead>
                  <TableHead>{copy.table.events.type}</TableHead>
                  <TableHead>{copy.table.events.subject}</TableHead>
                  <TableHead>{copy.table.events.requestKind}</TableHead>
                  <TableHead>{copy.table.events.related}</TableHead>
                  <TableHead>{copy.table.events.request}</TableHead>
                  <TableHead>{copy.table.events.summary}</TableHead>
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
                      <TableCell>
                        <div className="alerts-center-time-cell">
                          <strong>{formatTime(event.occurredAt)}</strong>
                          <span>{formatTimeDetail(event.occurredAt)}</span>
                        </div>
                      </TableCell>
                      <TableCell>
                        <StatusBadge tone={alertTypeTone(event.type)}>{copy.types[event.type]}</StatusBadge>
                      </TableCell>
                      <TableCell>
                        <div className="alerts-center-subject-cell">
                          <strong>{event.subjectLabel}</strong>
                          <span>{event.subjectKind}</span>
                        </div>
                      </TableCell>
                      <TableCell>
                        {event.requestKind ? (
                          <RequestKindBadge requestKindKey={event.requestKind.key} requestKindLabel={event.requestKind.label} size="sm" />
                        ) : '—'}
                      </TableCell>
                      <TableCell>
                        <div className="alerts-center-related-actions">
                          {event.user ? (
                            <Button type="button" variant="ghost" size="sm" onClick={() => onOpenUser(event.user!.userId)}>
                              {copy.openUser}
                            </Button>
                          ) : null}
                          {event.token ? (
                            <Button type="button" variant="ghost" size="sm" onClick={() => onOpenToken(event.token!.id)}>
                              {copy.openToken}
                            </Button>
                          ) : null}
                          {event.key ? (
                            <Button type="button" variant="ghost" size="sm" onClick={() => onOpenKey(event.key!.id)}>
                              {copy.openKey}
                            </Button>
                          ) : null}
                        </div>
                      </TableCell>
                      <TableCell>
                        {event.request ? (
                          <Button type="button" variant="ghost" size="sm" onClick={() => setSelectedRequest(event.request)}>
                            {copy.requestOpen}
                          </Button>
                        ) : (
                          '—'
                        )}
                      </TableCell>
                      <TableCell>
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
              loadState={listLoadState}
              loadingLabel={copy.title}
              errorLabel={listError}
            >
              <TableHeader>
                <TableRow>
                  <TableHead>{copy.table.groups.time}</TableHead>
                  <TableHead>{copy.table.groups.type}</TableHead>
                  <TableHead>{copy.table.groups.subject}</TableHead>
                  <TableHead>{copy.table.groups.requestKind}</TableHead>
                  <TableHead>{copy.table.groups.count}</TableHead>
                  <TableHead>{copy.table.groups.firstSeen}</TableHead>
                  <TableHead>{copy.table.groups.latest}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {groupsPage.items.length === 0 ? (
                  <TableRow>
                    <TableCell colSpan={7}>
                      <div className="empty-state alert">{copy.emptyGroups}</div>
                    </TableCell>
                  </TableRow>
                ) : (
                  groupsPage.items.map((group) => (
                    <TableRow key={group.id}>
                      <TableCell>
                        <div className="alerts-center-time-cell">
                          <strong>{formatTime(group.lastSeen)}</strong>
                          <span>{formatTimeDetail(group.lastSeen)}</span>
                        </div>
                      </TableCell>
                      <TableCell>
                        <StatusBadge tone={alertTypeTone(group.type)}>{copy.types[group.type]}</StatusBadge>
                      </TableCell>
                      <TableCell>
                        <div className="alerts-center-subject-cell">
                          <strong>{group.subjectLabel}</strong>
                          <div className="alerts-center-related-actions">
                            {group.user ? (
                              <Button type="button" variant="ghost" size="sm" onClick={() => onOpenUser(group.user!.userId)}>
                                {copy.openUser}
                              </Button>
                            ) : null}
                            {group.token ? (
                              <Button type="button" variant="ghost" size="sm" onClick={() => onOpenToken(group.token!.id)}>
                                {copy.openToken}
                              </Button>
                            ) : null}
                            {group.key ? (
                              <Button type="button" variant="ghost" size="sm" onClick={() => onOpenKey(group.key!.id)}>
                                {copy.openKey}
                              </Button>
                            ) : null}
                          </div>
                        </div>
                      </TableCell>
                      <TableCell>
                        {group.requestKind ? (
                          <RequestKindBadge requestKindKey={group.requestKind.key} requestKindLabel={group.requestKind.label} size="sm" />
                        ) : '—'}
                      </TableCell>
                      <TableCell>
                        <strong>{group.count}</strong>
                      </TableCell>
                      <TableCell>{formatTime(group.firstSeen)}</TableCell>
                      <TableCell>
                        <div className="alerts-center-summary-cell">
                          <strong>{group.latestEvent.title}</strong>
                          <span>{group.latestEvent.summary}</span>
                        </div>
                      </TableCell>
                    </TableRow>
                  ))
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
              <div>
                <h3>{copy.requestDrawer.title}</h3>
                <p className="panel-description">{requestSummary(selectedRequest)}</p>
              </div>
            </header>

            <AdminLoadingRegion
              loadState={requestLoadState}
              loadingLabel={copy.requestDrawer.loading}
              errorLabel={requestLoadError ?? copy.requestDrawer.error}
            >
              <div className="alerts-center-request-drawer__grid">
                <div>
                  <h4>{copy.requestDrawer.requestBody}</h4>
                  <pre>{requestBodies?.request_body ?? copy.requestDrawer.noBody}</pre>
                </div>
                <div>
                  <h4>{copy.requestDrawer.responseBody}</h4>
                  <pre>{requestBodies?.response_body ?? copy.requestDrawer.noBody}</pre>
                </div>
              </div>
            </AdminLoadingRegion>
          </section>
        </DrawerContent>
      </Drawer>
    </div>
  )
}
