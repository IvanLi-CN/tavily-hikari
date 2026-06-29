import { useEffect, useMemo, useState } from 'react'

import {
  fetchAdminRecharges,
  fetchAdminTotpStatus,
  refundAdminRecharge,
  refundOnlyAdminRecharge,
  type AdminTotpStatus,
  type AdminRechargeListResponse,
  type AdminRechargeOrder,
  type AdminRechargeSort,
  type AdminRechargeStatus,
  type AdminRechargeViewMode,
} from '../api'
import AdminLoadingRegion from '../components/AdminLoadingRegion'
import { Button } from '../components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '../components/ui/dialog'
import { Input } from '../components/ui/input'
import { useTranslate, type AdminTranslations } from '../i18n'
import type { QueryLoadState } from './queryLoadState'

interface AdminRechargeRecordsModuleProps {
  initialData?: AdminRechargeListResponse
  initialTotpStatus?: AdminTotpStatus | null
  disableAutoLoad?: boolean
  onOpenUser?: (id: string) => void
  onOpenSystemSettings?: () => void
}

export type RefundKind = 'refund' | 'refundOnly'

const STATUS_OPTIONS: Array<AdminRechargeStatus | 'all'> = ['all', 'pending', 'paid', 'failed', 'expired', 'refunding', 'refunded', 'refundOnly']
const SORT_OPTIONS: AdminRechargeSort[] = ['createdAt', 'paidAt', 'refundedAt', 'status']

function formatDate(ts: number | null | undefined): string {
  if (!ts) return '-'
  return new Date(ts * 1000).toLocaleString()
}

function userLabel(user: AdminRechargeOrder['user']): string {
  return user.displayName || user.username || user.id
}

function dateStartSeconds(value: string): number | undefined {
  if (!value) return undefined
  const date = new Date(`${value}T00:00:00`)
  return Number.isNaN(date.getTime()) ? undefined : Math.floor(date.getTime() / 1000)
}

function dateEndSeconds(value: string): number | undefined {
  if (!value) return undefined
  const date = new Date(`${value}T23:59:59`)
  return Number.isNaN(date.getTime()) ? undefined : Math.floor(date.getTime() / 1000)
}

export default function AdminRechargeRecordsModule({
  initialData,
  initialTotpStatus,
  disableAutoLoad = false,
  onOpenUser,
  onOpenSystemSettings,
}: AdminRechargeRecordsModuleProps): JSX.Element {
  const strings = useTranslate().admin.recharges
  const [data, setData] = useState<AdminRechargeListResponse | null>(initialData ?? null)
  const [loadState, setLoadState] = useState<QueryLoadState>(initialData ? 'ready' : 'initial_loading')
  const [error, setError] = useState<string | null>(null)
  const [totpStatus, setTotpStatus] = useState<AdminTotpStatus | null>(initialTotpStatus ?? null)
  const [totpStatusError, setTotpStatusError] = useState<string | null>(null)
  const [totpStatusLoading, setTotpStatusLoading] = useState(!disableAutoLoad && initialTotpStatus === undefined)
  const [query, setQuery] = useState('')
  const [status, setStatus] = useState<AdminRechargeStatus | 'all'>('all')
  const [startDate, setStartDate] = useState('')
  const [endDate, setEndDate] = useState('')
  const [sort, setSort] = useState<AdminRechargeSort>('createdAt')
  const [view, setView] = useState<AdminRechargeViewMode>('flat')
  const [order, setOrder] = useState<'asc' | 'desc'>('desc')
  const [page, setPage] = useState(1)
  const [refundTarget, setRefundTarget] = useState<{ order: AdminRechargeOrder; kind: RefundKind } | null>(null)
  const [totpCode, setTotpCode] = useState('')
  const [refundBusy, setRefundBusy] = useState(false)
  const [refundError, setRefundError] = useState<string | null>(null)

  const load = () => {
    const controller = new AbortController()
    setLoadState((current) => (current === 'ready' ? 'refreshing' : 'initial_loading'))
    setError(null)
    fetchAdminRecharges(
      {
        user: query.trim() || undefined,
        status,
        startAt: dateStartSeconds(startDate),
        endAt: dateEndSeconds(endDate),
        sort,
        order,
        view,
        page,
        perPage: 25,
      },
      controller.signal,
    )
      .then((next) => {
        setData(next)
        setLoadState('ready')
      })
      .catch((err: unknown) => {
        if (!controller.signal.aborted) {
          setError(err instanceof Error ? err.message : String(err))
          setLoadState('error')
        }
      })
    return () => controller.abort()
  }

  const openUser = (id: string) => {
    if (onOpenUser) {
      onOpenUser(id)
      return
    }
    if (typeof window !== 'undefined') {
      window.location.assign(`/admin/users/${encodeURIComponent(id)}`)
    }
  }

  const openSystemSettings = () => {
    if (onOpenSystemSettings) {
      onOpenSystemSettings()
      return
    }
    if (typeof window !== 'undefined') {
      window.location.assign('/admin/system-settings')
    }
  }

  const openRefundDialog = (order: AdminRechargeOrder, kind: RefundKind) => {
    setRefundTarget({ order, kind })
    setTotpCode('')
    setRefundError(null)
  }

  const closeRefundDialog = () => {
    if (refundBusy) return
    setRefundTarget(null)
    setTotpCode('')
    setRefundError(null)
  }

  useEffect(() => {
    if (disableAutoLoad) return
    return load()
  }, [disableAutoLoad, query, status, startDate, endDate, sort, order, view, page])

  useEffect(() => {
    if (disableAutoLoad) return
    const controller = new AbortController()
    setTotpStatusLoading(true)
    setTotpStatusError(null)
    fetchAdminTotpStatus(controller.signal)
      .then((nextStatus) => {
        setTotpStatus(nextStatus)
      })
      .catch((err: unknown) => {
        if (!controller.signal.aborted) {
          setTotpStatusError(err instanceof Error ? err.message : String(err))
        }
      })
      .finally(() => {
        if (!controller.signal.aborted) setTotpStatusLoading(false)
      })
    return () => controller.abort()
  }, [disableAutoLoad])

  const summary = useMemo(() => {
    const items = data?.items ?? []
    return {
      actionable: items.filter((item) => item.status === 'paid').length,
      total: data?.total ?? 0,
    }
  }, [data])

  const executeRefund = async () => {
    if (!refundTarget) return
    setRefundBusy(true)
    setRefundError(null)
    try {
      await (refundTarget.kind === 'refund'
        ? refundAdminRecharge(refundTarget.order.outTradeNo, totpCode)
        : refundOnlyAdminRecharge(refundTarget.order.outTradeNo, totpCode))
      setRefundTarget(null)
      setTotpCode('')
      setRefundError(null)
      load()
    } catch (err) {
      setRefundError(refundErrorMessage(err instanceof Error ? err.message : String(err), strings))
    } finally {
      setRefundBusy(false)
    }
  }

  const totpSummaryText = totpStatus == null
    ? strings.summary.totpRequired
    : totpStatus.enabled
      ? strings.summary.totpRequired
      : totpStatus.available === false
        ? strings.summary.totpUnavailable
        : strings.summary.totpSetupRequired
  if (data && !data.hasRechargeOrders) {
    return (
      <section className="admin-recharge-module admin-recharge-module--empty" aria-label={strings.title}>
        <div className="empty-state">{strings.emptyHiddenDescription}</div>
      </section>
    )
  }

  return (
    <section className="admin-recharge-module" aria-label={strings.title}>
      <div className="admin-recharge-summary" aria-label={strings.title}>
        <span>{formatTemplate(strings.summary.orders, { total: summary.total })}</span>
        <span>{formatTemplate(strings.summary.actionable, { count: summary.actionable })}</span>
        <span>{totpSummaryText}</span>
      </div>
      {totpStatusError && (
        <p className="admin-recharge-inline-status" role="status" aria-live="polite">
          {formatTemplate(strings.totpStatusLoadFailed, { message: totpStatusError })}
        </p>
      )}

      <div className="admin-recharge-toolbar">
        <div className="admin-recharge-toolbar-primary">
          <label className="admin-recharge-search-field" htmlFor="admin-recharge-search">
            <span className="sr-only">{strings.searchLabel}</span>
            <Input
              id="admin-recharge-search"
              name="admin_recharge_search"
              value={query}
              onChange={(event) => { setPage(1); setQuery(event.target.value) }}
              placeholder={strings.searchPlaceholder}
            />
          </label>
          <div className="segmented-control" role="group" aria-label={strings.viewAriaLabel}>
            <Button type="button" variant={view === 'flat' ? 'default' : 'outline'} onClick={() => setView('flat')}>{strings.flatView}</Button>
            <Button type="button" variant={view === 'user' ? 'default' : 'outline'} onClick={() => setView('user')}>{strings.userView}</Button>
          </div>
        </div>
        <div className="admin-recharge-filter-row">
          <label className="admin-recharge-filter-field" htmlFor="admin-recharge-status">
            <span>{strings.statusFilterLabel}</span>
            <select id="admin-recharge-status" name="admin_recharge_status" value={status} onChange={(event) => { setPage(1); setStatus(event.target.value as AdminRechargeStatus | 'all') }}>
              {STATUS_OPTIONS.map((item) => <option key={item} value={item}>{item === 'all' ? strings.allStatuses : statusLabel(item, strings)}</option>)}
            </select>
          </label>
          <label className="admin-recharge-filter-field" htmlFor="admin-recharge-start-date">
            <span>{strings.startDateFilterLabel}</span>
            <Input
              id="admin-recharge-start-date"
              name="admin_recharge_start_date"
              type="date"
              value={startDate}
              max={endDate || undefined}
              onChange={(event) => { setPage(1); setStartDate(event.target.value) }}
            />
          </label>
          <label className="admin-recharge-filter-field" htmlFor="admin-recharge-end-date">
            <span>{strings.endDateFilterLabel}</span>
            <Input
              id="admin-recharge-end-date"
              name="admin_recharge_end_date"
              type="date"
              value={endDate}
              min={startDate || undefined}
              onChange={(event) => { setPage(1); setEndDate(event.target.value) }}
            />
          </label>
          <label className="admin-recharge-filter-field" htmlFor="admin-recharge-sort">
            <span>{strings.sortFilterLabel}</span>
            <select id="admin-recharge-sort" name="admin_recharge_sort" value={sort} onChange={(event) => setSort(event.target.value as AdminRechargeSort)}>
              {SORT_OPTIONS.map((item) => <option key={item} value={item}>{sortLabel(item, strings)}</option>)}
            </select>
          </label>
          <label className="admin-recharge-filter-field" htmlFor="admin-recharge-order">
            <span>{strings.orderFilterLabel}</span>
            <select id="admin-recharge-order" name="admin_recharge_order" value={order} onChange={(event) => setOrder(event.target.value as 'asc' | 'desc')}>
              <option value="desc">{strings.orderDesc}</option>
              <option value="asc">{strings.orderAsc}</option>
            </select>
          </label>
        </div>
      </div>

      <AdminLoadingRegion loadState={loadState} errorLabel={error} loadingLabel={strings.loading}>
        {view === 'user' ? (
          <div className="admin-recharge-group-grid">
            {(data?.groups ?? []).map((group) => (
              <button key={group.user.id} type="button" className="admin-recharge-user-card" onClick={() => openUser(group.user.id)}>
                <strong>{userLabel(group.user)}</strong>
                <span>{formatTemplate(strings.groupSummary, { orders: group.orderCount, paid: group.paidOrderCount, refunded: group.refundedOrderCount })}</span>
                <span>{formatTemplate(strings.groupCredits, { credits: group.totalCredits.toLocaleString(), amount: (group.totalMoneyCents / 100).toFixed(2) })}</span>
              </button>
            ))}
          </div>
        ) : (
          <div className="admin-recharge-table-scroll">
            <table className="admin-table admin-recharge-table">
              <thead>
                <tr>
                  <th>{strings.table.user}</th>
                  <th>{strings.table.order}</th>
                  <th>{strings.table.status}</th>
                  <th>{strings.table.amount}</th>
                  <th>{strings.table.finalAmount}</th>
                  <th>{strings.table.createdAt}</th>
                  <th>{strings.table.paidAt}</th>
                  <th>{strings.table.refundedAt}</th>
                  <th>{strings.table.actions}</th>
                </tr>
              </thead>
              <tbody>
                {(data?.items ?? []).map((item) => (
                  <tr key={item.outTradeNo}>
                    <td><button type="button" className="link-button admin-recharge-user-link" onClick={() => openUser(item.user.id)}>{userLabel(item.user)}</button></td>
                    <td>
                      <div className="admin-recharge-order-cell">
                        <span>{formatTemplate(strings.orderCredits, { credits: item.credits.toLocaleString(), months: item.months })}</span>
                        <code>{item.outTradeNo}</code>
                      </div>
                    </td>
                    <td><span className={`status-pill status-${item.status}`}>{statusLabel(item.status, strings)}</span></td>
                    <td>{formatTemplate(strings.amountLdc, { amount: item.money })}</td>
                    <td>
                      <div className="admin-recharge-order-cell">
                        <span>{formatTemplate(strings.finalAmountLdc, { amount: (item.finalMoneyCents / 100).toFixed(2) })}</span>
                        {item.monthEndClampApplied ? (
                          <span className="admin-recharge-order-meta">{strings.monthEndClampApplied}</span>
                        ) : (
                          <span className="admin-recharge-order-meta">{strings.monthEndClampInactive}</span>
                        )}
                      </div>
                    </td>
                    <td>{formatDate(item.createdAt)}</td>
                    <td>{formatDate(item.paidAt)}</td>
                    <td>{formatDate(item.refundedAt)}</td>
                    <td>
                      {item.status === 'paid' ? (
                        <div className="admin-recharge-actions">
                          <Button type="button" size="sm" variant="outline" className="admin-recharge-action-button" onClick={() => openRefundDialog(item, 'refund')}>{strings.actions.refund}</Button>
                          <Button type="button" size="sm" variant="outline" className="admin-recharge-action-button" onClick={() => openRefundDialog(item, 'refundOnly')}>{strings.actions.refundOnly}</Button>
                        </div>
                      ) : (
                        <span className="admin-recharge-action-state">{statusActionLabel(item.status, strings)}</span>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </AdminLoadingRegion>

      <div className="admin-recharge-pagination">
        <Button type="button" variant="outline" disabled={page <= 1} onClick={() => setPage((value) => Math.max(1, value - 1))}>{strings.actions.previousPage}</Button>
        <span>{formatTemplate(strings.paginationSummary, { page: data?.page ?? page, total: data?.total ?? 0 })}</span>
        <Button type="button" variant="outline" disabled={!data || page * data.perPage >= data.total} onClick={() => setPage((value) => value + 1)}>{strings.actions.nextPage}</Button>
      </div>

      <AdminRechargeRefundDialog
        refundTarget={refundTarget}
        totpStatus={totpStatus}
        totpCode={totpCode}
        refundBusy={refundBusy}
        refundError={refundError}
        totpStatusLoading={totpStatusLoading}
        totpStatusError={totpStatusError}
        onTotpCodeChange={(value) => {
          setTotpCode(value)
          setRefundError(null)
        }}
        onClose={closeRefundDialog}
        onExecuteRefund={() => void executeRefund()}
        onOpenSystemSettings={openSystemSettings}
      />
    </section>
  )
}

interface AdminRechargeRefundDialogProps {
  refundTarget: { order: AdminRechargeOrder; kind: RefundKind } | null
  totpStatus: AdminTotpStatus | null
  totpCode: string
  refundBusy: boolean
  refundError: string | null
  totpStatusLoading?: boolean
  totpStatusError?: string | null
  onTotpCodeChange: (value: string) => void
  onClose: () => void
  onExecuteRefund: () => void
  onOpenSystemSettings: () => void
  chrome?: 'dialog' | 'plain'
}

export function AdminRechargeRefundDialog({
  refundTarget,
  totpStatus,
  totpCode,
  refundBusy,
  refundError,
  totpStatusLoading,
  totpStatusError,
  onTotpCodeChange,
  onClose,
  onExecuteRefund,
  onOpenSystemSettings,
}: AdminRechargeRefundDialogProps): JSX.Element {
  return (
    <Dialog open={refundTarget != null} onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent>
        <AdminRechargeRefundDialogBody
          refundTarget={refundTarget}
          totpStatus={totpStatus}
          totpCode={totpCode}
          refundBusy={refundBusy}
          refundError={refundError}
          totpStatusLoading={totpStatusLoading}
          totpStatusError={totpStatusError}
          onTotpCodeChange={onTotpCodeChange}
          onClose={onClose}
          onExecuteRefund={onExecuteRefund}
          onOpenSystemSettings={onOpenSystemSettings}
        />
      </DialogContent>
    </Dialog>
  )
}

export function AdminRechargeRefundDialogBody({
  refundTarget,
  totpStatus,
  totpCode,
  refundBusy,
  refundError,
  totpStatusLoading = false,
  totpStatusError = null,
  onTotpCodeChange,
  onClose,
  onExecuteRefund,
  onOpenSystemSettings,
  chrome = 'dialog',
}: AdminRechargeRefundDialogProps): JSX.Element {
  const strings = useTranslate().admin.recharges
  const refundDialogNeedsStatus = totpStatus == null
  const refundDialogUnavailable = totpStatus?.available === false
  const refundDialogNeedsSetup = totpStatus?.enabled === false
  const refundDialogBlocked = refundDialogNeedsStatus || refundDialogUnavailable || refundDialogNeedsSetup
  const refundDialogStatus = refundError ?? (refundBusy ? strings.confirm.processing : null)
  const title = refundDialogBlocked
    ? refundDialogNeedsStatus
      ? strings.confirm.totpStatusTitle
      : refundDialogUnavailable
        ? strings.confirm.totpUnavailableTitle
      : strings.confirm.totpSetupTitle
    : refundTarget?.kind === 'refund' ? strings.confirm.refundTitle : strings.confirm.refundOnlyTitle
  const description = refundDialogBlocked
    ? refundDialogNeedsStatus
      ? strings.confirm.totpStatusDescription
      : refundDialogUnavailable
        ? strings.confirm.totpUnavailableDescription
      : strings.confirm.totpSetupDescription
    : strings.confirm.description
  const blockedCallout = refundDialogNeedsStatus
    ? totpStatusError
      ? formatTemplate(strings.confirm.totpStatusErrorCallout, { message: totpStatusError })
      : totpStatusLoading
        ? strings.confirm.totpStatusLoadingCallout
        : strings.confirm.totpStatusUnknownCallout
    : refundDialogUnavailable
      ? strings.confirm.totpUnavailableCallout
    : strings.confirm.totpSetupCallout
  const header = chrome === 'dialog' ? (
    <DialogHeader>
      <DialogTitle>{title}</DialogTitle>
      <DialogDescription>{description}</DialogDescription>
    </DialogHeader>
  ) : (
    <div>
      <h2>{title}</h2>
      <p>{description}</p>
    </div>
  )
  const footerContent = (
    <>
      <Button type="button" variant="outline" disabled={refundBusy} onClick={onClose}>{strings.actions.cancel}</Button>
      {refundDialogNeedsSetup && !refundDialogUnavailable ? (
        <Button type="button" onClick={onOpenSystemSettings}>{strings.actions.openTotpSettings}</Button>
      ) : refundDialogBlocked ? null : (
        <Button type="button" disabled={refundBusy || totpCode.length !== 6} onClick={onExecuteRefund}>
          {refundBusy ? strings.actions.processing : strings.actions.confirm}
        </Button>
      )}
    </>
  )
  return (
    <>
      {header}
      {refundDialogBlocked ? (
        <div className="admin-recharge-setup-callout" role="status" aria-live="polite">
          {blockedCallout}
        </div>
      ) : (
        <label className="admin-recharge-totp-field" htmlFor="admin-recharge-refund-totp">
          <span>{strings.confirm.totpLabel}</span>
          <Input
            id="admin-recharge-refund-totp"
            name="admin_recharge_refund_totp"
            type="text"
            className="admin-recharge-totp-input"
            value={totpCode}
            onChange={(event) => onTotpCodeChange(event.target.value.replace(/\D/g, '').slice(0, 6))}
            placeholder={strings.confirm.totpPlaceholder}
            autoComplete="one-time-code"
            inputMode="numeric"
            pattern="[0-9]*"
            maxLength={6}
            disabled={refundBusy}
          />
        </label>
      )}
      {refundDialogStatus && (
        <p
          className={refundError ? 'admin-recharge-dialog-error' : 'admin-recharge-dialog-status'}
          role="status"
          aria-live="polite"
        >
          {refundDialogStatus}
        </p>
      )}
      {chrome === 'dialog' ? <DialogFooter>{footerContent}</DialogFooter> : <div>{footerContent}</div>}
    </>
  )
}

function sortLabel(sort: AdminRechargeSort, strings: AdminTranslations['recharges']): string {
  return strings.sort[sort]
}

function statusLabel(status: AdminRechargeStatus, strings: AdminTranslations['recharges']): string {
  return strings.status[status]
}

function statusActionLabel(status: AdminRechargeStatus, strings: AdminTranslations['recharges']): string {
  if (status === 'paid') return strings.actions.refund
  return strings.statusAction[status]
}

export function refundErrorMessage(message: string, strings: AdminTranslations['recharges']): string {
  const normalized = message.trim()
  if (/admin TOTP is not bound/i.test(normalized)) return strings.errors.totpNotBound
  if (/invalid TOTP code/i.test(normalized)) return strings.errors.invalidTotp
  if (/TOTP is temporarily locked/i.test(normalized)) return strings.errors.totpLocked
  if (/DEV_OPEN_ADMIN/i.test(normalized)) return strings.errors.devOpenAdmin
  return normalized || strings.errors.refundFailed
}

function formatTemplate(template: string, values: Record<string, string | number>): string {
  return Object.entries(values).reduce(
    (current, [key, value]) => current.replaceAll(`{${key}}`, String(value)),
    template,
  )
}
