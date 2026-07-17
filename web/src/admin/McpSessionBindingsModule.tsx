import { useEffect, useId, useMemo, useState } from 'react'

import type {
  AdminMcpSessionBindingsPage,
  AdminMcpSessionBindingsQuery,
  AdminMcpSessionBindingListItem,
} from '../api'
import type { Language } from '../i18n'
import type { QueryLoadState } from './queryLoadState'
import type {
  AdminMcpSessionBindingsPathContext,
  AdminMcpSessionBindingsStatusView,
} from './routes'
import AdminLoadingRegion from '../components/AdminLoadingRegion'
import AdminTablePagination from '../components/AdminTablePagination'
import { StatusBadge, type StatusTone } from '../components/StatusBadge'
import { Button } from '../components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '../components/ui/dialog'
import { Input } from '../components/ui/input'
import SegmentedTabs from '../components/ui/SegmentedTabs'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '../components/ui/table'
import { Icon } from '../lib/icons'

interface McpSessionBindingsModuleProps {
  language: Language
  query: AdminMcpSessionBindingsPathContext
  data: AdminMcpSessionBindingsPage | null
  loadState: QueryLoadState
  error: string | null
  refreshing: boolean
  busy: boolean
  onRefresh: () => Promise<void> | void
  onNavigate: (next: AdminMcpSessionBindingsPathContext) => void
  onRevokeSelected: (proxySessionIds: string[]) => Promise<void> | void
  onRevokeFiltered: (query: AdminMcpSessionBindingsQuery) => Promise<void> | void
  onOpenUser: (userId: string) => void
  onOpenToken: (tokenId: string) => void
  onOpenKey: (keyId: string) => void
}

function copyFor(language: Language) {
  if (language === 'zh') {
    return {
      generatedAt: '按最近续约时间倒序排列',
      loading: '正在加载 session 绑定记录…',
      loadFailed: '读取 session 绑定记录失败。',
      empty: '当前筛选下没有 session 绑定记录。',
      statusTabs: {
        active: '活跃项',
        revoked: '已释放',
        all: '全部',
      },
      filters: {
        createdFrom: '创建时间起',
        createdTo: '创建时间止',
        updatedFrom: '续约时间起',
        updatedTo: '续约时间止',
        apply: '应用筛选',
        reset: '重置',
      },
      summary: {
        total: '命中总数',
        activeMatching: '当前筛选内活跃',
        currentPageActionable: '当前页可释放',
      },
      selection: {
        page: '选择当前页可操作会话',
        row: '选择会话 {id}',
        selectedCount: '已选 {count} 项',
        clear: '清空选择',
      },
      actions: {
        refresh: '刷新',
        release: '释放',
        releaseSelected: '释放已选',
        releaseFiltered: '释放当前筛选结果全部活跃会话',
        releaseFilteredWithCount: '释放当前筛选结果全部活跃会话（{count}）',
      },
      table: {
        proxySessionId: '代理会话',
        authTokenId: '访问令牌',
        userId: '用户',
        upstreamKeyId: '上游 Key',
        createdAt: '创建时间',
        updatedAt: '续约时间',
        expiresAt: '过期时间',
        status: '状态',
        history: '释放记录',
        action: '操作',
      },
      status: {
        active: '活跃',
        expired: '已过期',
        revoked: '已释放',
      },
      revokeHistoryEmpty: '—',
      revokeReasonPrefix: '原因：',
      confirmTitle: '确认释放当前筛选结果中的全部活跃会话',
      confirmDescription:
        '该操作会释放当前筛选结果中的全部活跃 upstream_mcp session。已有过期或已释放记录不会被重复处理。',
      confirmCount: '本次将释放 {count} 个活跃会话。',
      confirmCancel: '取消',
      confirmRelease: '确认释放',
      pagination: '第 {page} / {total} 页',
      notAvailable: '—',
    }
  }

  return {
    generatedAt: 'Sorted by most recent renewal',
    loading: 'Loading session binding records…',
    loadFailed: 'Failed to load session binding records.',
    empty: 'No session binding records match the current filters.',
    statusTabs: {
      active: 'Active',
      revoked: 'Revoked',
      all: 'All',
    },
    filters: {
      createdFrom: 'Created from',
      createdTo: 'Created to',
      updatedFrom: 'Renewed from',
      updatedTo: 'Renewed to',
      apply: 'Apply filters',
      reset: 'Reset',
    },
    summary: {
      total: 'Matches',
      activeMatching: 'Active in filter',
      currentPageActionable: 'Actionable on page',
    },
    selection: {
      page: 'Select actionable sessions on this page',
      row: 'Select session {id}',
      selectedCount: '{count} selected',
      clear: 'Clear',
    },
    actions: {
      refresh: 'Refresh',
      release: 'Release',
      releaseSelected: 'Release selected',
      releaseFiltered: 'Release all active sessions in current filter',
      releaseFilteredWithCount: 'Release all active sessions in current filter ({count})',
    },
    table: {
      proxySessionId: 'Proxy Session',
      authTokenId: 'Access token',
      userId: 'User',
      upstreamKeyId: 'Upstream key',
      createdAt: 'Created',
      updatedAt: 'Renewed',
      expiresAt: 'Expires',
      status: 'Status',
      history: 'Release history',
      action: 'Action',
    },
    status: {
      active: 'Active',
      expired: 'Expired',
      revoked: 'Revoked',
    },
    revokeHistoryEmpty: '—',
    revokeReasonPrefix: 'Reason:',
    confirmTitle: 'Release all active sessions in the current filter',
    confirmDescription:
      'This releases every active upstream_mcp session in the current filter result. Expired or revoked records are ignored.',
    confirmCount: 'This action will release {count} active sessions.',
    confirmCancel: 'Cancel',
    confirmRelease: 'Release sessions',
    pagination: 'Page {page} / {total}',
    notAvailable: '—',
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

function isoToDateTimeLocal(iso: string | null | undefined): string {
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

function formatTimestamp(timestamp: number | null, formatter: Intl.DateTimeFormat, emptyLabel: string): string {
  if (timestamp == null) return emptyLabel
  const parsed = new Date(timestamp * 1000)
  return Number.isNaN(parsed.getTime()) ? emptyLabel : formatter.format(parsed)
}

function rowTone(item: AdminMcpSessionBindingListItem): StatusTone {
  switch (item.status) {
    case 'active':
      return 'warning'
    case 'expired':
      return 'neutral'
    case 'revoked':
      return 'success'
    default:
      return 'neutral'
  }
}

const selectionCheckboxLabelStyle = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 8,
  cursor: 'pointer',
  whiteSpace: 'nowrap',
} as const

export default function McpSessionBindingsModule({
  language,
  query,
  data,
  loadState,
  error,
  refreshing,
  busy,
  onRefresh,
  onNavigate,
  onRevokeSelected,
  onRevokeFiltered,
  onOpenUser,
  onOpenToken,
  onOpenKey,
}: McpSessionBindingsModuleProps): JSX.Element {
  const copy = useMemo(() => copyFor(language), [language])
  const formatter = useMemo(
    () =>
      new Intl.DateTimeFormat(language === 'zh' ? 'zh-CN' : 'en-US', {
        dateStyle: 'medium',
        timeStyle: 'short',
      }),
    [language],
  )
  const numberFormatter = useMemo(
    () => new Intl.NumberFormat(language === 'zh' ? 'zh-CN' : 'en-US'),
    [language],
  )
  const [draftCreatedFrom, setDraftCreatedFrom] = useState(() => isoToDateTimeLocal(query.createdFrom))
  const [draftCreatedTo, setDraftCreatedTo] = useState(() => isoToDateTimeLocal(query.createdTo))
  const [draftUpdatedFrom, setDraftUpdatedFrom] = useState(() => isoToDateTimeLocal(query.updatedFrom))
  const [draftUpdatedTo, setDraftUpdatedTo] = useState(() => isoToDateTimeLocal(query.updatedTo))
  const [selectedIds, setSelectedIds] = useState<Set<string>>(() => new Set())
  const [confirmReleaseAllOpen, setConfirmReleaseAllOpen] = useState(false)
  const statusTabsLabelId = useId()

  useEffect(() => {
    setDraftCreatedFrom(isoToDateTimeLocal(query.createdFrom))
    setDraftCreatedTo(isoToDateTimeLocal(query.createdTo))
    setDraftUpdatedFrom(isoToDateTimeLocal(query.updatedFrom))
    setDraftUpdatedTo(isoToDateTimeLocal(query.updatedTo))
  }, [query.createdFrom, query.createdTo, query.updatedFrom, query.updatedTo])

  useEffect(() => {
    setSelectedIds((current) => {
      if (!data) return new Set()
      const next = new Set<string>()
      data.items.forEach((item) => {
        if (item.status === 'active' && current.has(item.proxySessionId)) {
          next.add(item.proxySessionId)
        }
      })
      return next
    })
  }, [data])

  const actionableItems = useMemo(
    () => data?.items.filter((item) => item.status === 'active') ?? [],
    [data],
  )
  const actionableIds = useMemo(
    () => actionableItems.map((item) => item.proxySessionId),
    [actionableItems],
  )
  const allActionableSelected = actionableIds.length > 0 && actionableIds.every((id) => selectedIds.has(id))
  const selectedCount = selectedIds.size
  const totalPages = Math.max(1, Math.ceil((data?.total ?? 0) / Math.max(1, data?.perPage ?? 20)))

  const applyFilters = () => {
    onNavigate({
      status: query.status ?? 'active',
      createdFrom: dateTimeLocalToIso(draftCreatedFrom),
      createdTo: dateTimeLocalToIso(draftCreatedTo),
      updatedFrom: dateTimeLocalToIso(draftUpdatedFrom),
      updatedTo: dateTimeLocalToIso(draftUpdatedTo),
      page: 1,
    })
  }

  const resetFilters = () => {
    setDraftCreatedFrom('')
    setDraftCreatedTo('')
    setDraftUpdatedFrom('')
    setDraftUpdatedTo('')
    onNavigate({
      status: query.status ?? 'active',
      createdFrom: null,
      createdTo: null,
      updatedFrom: null,
      updatedTo: null,
      page: 1,
    })
  }

  const currentApiQuery: AdminMcpSessionBindingsQuery = {
    status: query.status ?? 'active',
    createdFrom: query.createdFrom ?? null,
    createdTo: query.createdTo ?? null,
    updatedFrom: query.updatedFrom ?? null,
    updatedTo: query.updatedTo ?? null,
    page: query.page ?? 1,
    perPage: data?.perPage ?? 20,
  }

  return (
    <section className="surface panel" style={{ display: 'grid', gap: 16 }}>
      <div className="panel-header" style={{ alignItems: 'flex-start', gap: 12 }}>
        <div>
          <p className="panel-description">{copy.generatedAt}</p>
          <div
            role="group"
            aria-labelledby={statusTabsLabelId}
            style={{ display: 'grid', gap: 10, marginTop: 12 }}
          >
            <span id={statusTabsLabelId} className="sr-only">
              status
            </span>
            <SegmentedTabs
              value={query.status ?? 'active'}
              onChange={(value) => {
                onNavigate({
                  ...query,
                  status: value,
                  page: 1,
                })
              }}
              ariaLabel={language === 'zh' ? 'session 状态' : 'Session status'}
              options={[
                { value: 'active', label: copy.statusTabs.active },
                { value: 'revoked', label: copy.statusTabs.revoked },
                { value: 'all', label: copy.statusTabs.all },
              ]}
            />
          </div>
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => void onRefresh()}
          disabled={refreshing || busy}
        >
          <Icon icon={refreshing ? 'mdi:loading' : 'mdi:refresh'} width={16} height={16} className={refreshing ? 'icon-spin' : undefined} />
          <span>{copy.actions.refresh}</span>
        </Button>
      </div>

      <div className="surface" style={{ display: 'grid', gap: 12, padding: 16 }}>
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
          <div>
            <label className="text-xs font-medium text-muted-foreground" htmlFor="mcp-session-bindings-created-from">
              {copy.filters.createdFrom}
            </label>
            <Input
              id="mcp-session-bindings-created-from"
              type="datetime-local"
              value={draftCreatedFrom}
              onChange={(event) => setDraftCreatedFrom(event.target.value)}
              disabled={busy}
            />
          </div>
          <div>
            <label className="text-xs font-medium text-muted-foreground" htmlFor="mcp-session-bindings-created-to">
              {copy.filters.createdTo}
            </label>
            <Input
              id="mcp-session-bindings-created-to"
              type="datetime-local"
              value={draftCreatedTo}
              onChange={(event) => setDraftCreatedTo(event.target.value)}
              disabled={busy}
            />
          </div>
          <div>
            <label className="text-xs font-medium text-muted-foreground" htmlFor="mcp-session-bindings-updated-from">
              {copy.filters.updatedFrom}
            </label>
            <Input
              id="mcp-session-bindings-updated-from"
              type="datetime-local"
              value={draftUpdatedFrom}
              onChange={(event) => setDraftUpdatedFrom(event.target.value)}
              disabled={busy}
            />
          </div>
          <div>
            <label className="text-xs font-medium text-muted-foreground" htmlFor="mcp-session-bindings-updated-to">
              {copy.filters.updatedTo}
            </label>
            <Input
              id="mcp-session-bindings-updated-to"
              type="datetime-local"
              value={draftUpdatedTo}
              onChange={(event) => setDraftUpdatedTo(event.target.value)}
              disabled={busy}
            />
          </div>
        </div>
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
          <Button type="button" size="sm" onClick={applyFilters} disabled={busy}>
            {copy.filters.apply}
          </Button>
          <Button type="button" variant="outline" size="sm" onClick={resetFilters} disabled={busy}>
            {copy.filters.reset}
          </Button>
        </div>
      </div>

      <div className="grid gap-3 md:grid-cols-3">
        <SummaryCard label={copy.summary.total} value={numberFormatter.format(data?.total ?? 0)} />
        <SummaryCard
          label={copy.summary.activeMatching}
          value={numberFormatter.format(data?.activeMatchingCount ?? 0)}
        />
        <SummaryCard
          label={copy.summary.currentPageActionable}
          value={numberFormatter.format(actionableItems.length)}
        />
      </div>

      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          gap: 8,
          flexWrap: 'wrap',
          paddingBottom: 8,
          borderBottom: '1px solid hsl(var(--border) / 0.46)',
        }}
      >
        <div style={{ display: 'inline-flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
          <strong>{copy.selection.selectedCount.replace('{count}', numberFormatter.format(selectedCount))}</strong>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => setSelectedIds(new Set())}
            disabled={selectedCount === 0 || busy}
          >
            {copy.selection.clear}
          </Button>
        </div>
        <div style={{ display: 'inline-flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void onRevokeSelected(Array.from(selectedIds))}
            disabled={selectedCount === 0 || busy}
          >
            {copy.actions.releaseSelected}
          </Button>
          <Button
            type="button"
            variant="warning"
            size="sm"
            onClick={() => setConfirmReleaseAllOpen(true)}
            disabled={(data?.activeMatchingCount ?? 0) === 0 || busy}
          >
            {(data?.activeMatchingCount ?? 0) > 0
              ? copy.actions.releaseFilteredWithCount.replace(
                  '{count}',
                  numberFormatter.format(data?.activeMatchingCount ?? 0),
                )
              : copy.actions.releaseFiltered}
          </Button>
        </div>
      </div>

      <AdminLoadingRegion
        loadState={loadState}
        loadingLabel={copy.loading}
        errorLabel={error ?? copy.loadFailed}
        minHeight={260}
      >
        {!data || data.items.length === 0 ? (
          <div className="empty-state alert">{copy.empty}</div>
        ) : (
          <>
            <div className="table-wrapper jobs-table-wrapper">
              <Table className="jobs-table admin-users-table mcp-session-bindings-table">
                <TableHeader>
                  <TableRow>
                    <TableHead>
                      <label style={selectionCheckboxLabelStyle}>
                        <input
                          type="checkbox"
                          aria-label={copy.selection.page}
                          checked={allActionableSelected}
                          disabled={actionableIds.length === 0 || busy}
                          onChange={(event) => {
                            if (event.currentTarget.checked) {
                              setSelectedIds(new Set(actionableIds))
                            } else {
                              setSelectedIds(new Set())
                            }
                          }}
                        />
                      </label>
                    </TableHead>
                    <TableHead>{copy.table.proxySessionId}</TableHead>
                    <TableHead>{copy.table.authTokenId}</TableHead>
                    <TableHead>{copy.table.userId}</TableHead>
                    <TableHead>{copy.table.upstreamKeyId}</TableHead>
                    <TableHead>{copy.table.createdAt}</TableHead>
                    <TableHead>{copy.table.updatedAt}</TableHead>
                    <TableHead>{copy.table.expiresAt}</TableHead>
                    <TableHead>{copy.table.status}</TableHead>
                    <TableHead>{copy.table.history}</TableHead>
                    <TableHead>{copy.table.action}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {data.items.map((item) => {
                    const selectable = item.status === 'active'
                    return (
                      <TableRow key={item.proxySessionId}>
                        <TableCell>
                          <label style={selectionCheckboxLabelStyle}>
                            <input
                              type="checkbox"
                              aria-label={copy.selection.row.replace('{id}', item.proxySessionId)}
                              checked={selectedIds.has(item.proxySessionId)}
                              disabled={!selectable || busy}
                              onChange={(event) => {
                                setSelectedIds((current) => {
                                  const next = new Set(current)
                                  if (event.currentTarget.checked) next.add(item.proxySessionId)
                                  else next.delete(item.proxySessionId)
                                  return next
                                })
                              }}
                            />
                          </label>
                        </TableCell>
                        <TableCell className="font-mono text-xs">{item.proxySessionId}</TableCell>
                        <TableCell>{renderRelated(item.authTokenId, copy.notAvailable, onOpenToken)}</TableCell>
                        <TableCell>{renderRelated(item.userId, copy.notAvailable, onOpenUser)}</TableCell>
                        <TableCell>{renderRelated(item.upstreamKeyId, copy.notAvailable, onOpenKey)}</TableCell>
                        <TableCell>{formatTimestamp(item.createdAt, formatter, copy.notAvailable)}</TableCell>
                        <TableCell>{formatTimestamp(item.updatedAt, formatter, copy.notAvailable)}</TableCell>
                        <TableCell>{formatTimestamp(item.expiresAt, formatter, copy.notAvailable)}</TableCell>
                        <TableCell>
                          <StatusBadge tone={rowTone(item)}>{copy.status[item.status]}</StatusBadge>
                        </TableCell>
                        <TableCell>
                          {item.revokedAt == null ? (
                            <span>{copy.revokeHistoryEmpty}</span>
                          ) : (
                            <div style={{ display: 'grid', gap: 4 }}>
                              <span>{formatTimestamp(item.revokedAt, formatter, copy.notAvailable)}</span>
                              {item.revokeReason ? (
                                <small className="text-muted-foreground">
                                  {copy.revokeReasonPrefix} {item.revokeReason}
                                </small>
                              ) : null}
                            </div>
                          )}
                        </TableCell>
                        <TableCell>
                          {selectable ? (
                            <Button
                              type="button"
                              variant="ghost"
                              size="sm"
                              onClick={() => void onRevokeSelected([item.proxySessionId])}
                              disabled={busy}
                            >
                              {copy.actions.release}
                            </Button>
                          ) : (
                            <span>{copy.revokeHistoryEmpty}</span>
                          )}
                        </TableCell>
                      </TableRow>
                    )
                  })}
                </TableBody>
              </Table>
            </div>

            <AdminTablePagination
              page={data.page}
              totalPages={totalPages}
              pageSummary={
                <span className="panel-description">
                  {copy.pagination
                    .replace('{page}', String(data.page))
                    .replace('{total}', String(totalPages))}
                </span>
              }
              previousLabel={language === 'zh' ? '上一页' : 'Previous'}
              nextLabel={language === 'zh' ? '下一页' : 'Next'}
              previousDisabled={data.page <= 1 || busy}
              nextDisabled={data.page >= totalPages || busy}
              disabled={busy}
              onPrevious={() => onNavigate({ ...query, page: Math.max(1, (query.page ?? 1) - 1) })}
              onNext={() => onNavigate({ ...query, page: Math.min(totalPages, (query.page ?? 1) + 1) })}
            />
          </>
        )}
      </AdminLoadingRegion>

      <Dialog open={confirmReleaseAllOpen} onOpenChange={setConfirmReleaseAllOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{copy.confirmTitle}</DialogTitle>
            <DialogDescription>{copy.confirmDescription}</DialogDescription>
          </DialogHeader>
          <p className="text-sm font-medium">
            {copy.confirmCount.replace('{count}', numberFormatter.format(data?.activeMatchingCount ?? 0))}
          </p>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setConfirmReleaseAllOpen(false)} disabled={busy}>
              {copy.confirmCancel}
            </Button>
            <Button
              type="button"
              variant="warning"
              onClick={async () => {
                await onRevokeFiltered(currentApiQuery)
                setConfirmReleaseAllOpen(false)
              }}
              disabled={(data?.activeMatchingCount ?? 0) === 0 || busy}
            >
              {copy.confirmRelease}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  )
}

function SummaryCard({ label, value }: { label: string; value: string }): JSX.Element {
  return (
    <article className="upstream-privacy-stat">
      <span>{label}</span>
      <strong>{value}</strong>
    </article>
  )
}

function renderRelated(
  value: string | null,
  emptyLabel: string,
  onOpen: (id: string) => void,
): JSX.Element {
  if (!value) return <span>{emptyLabel}</span>
  return (
    <button
      type="button"
      className="text-left font-mono text-xs text-primary underline-offset-2 hover:underline"
      onClick={() => onOpen(value)}
    >
      {value}
    </button>
  )
}
