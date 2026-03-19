import { useMemo } from 'react'

import type {
  StickyNode,
  StickyUserDailyBucket,
  StickyUserRow,
} from '../api'
import { useTranslate } from '../i18n'
import AdminLoadingRegion from '../components/AdminLoadingRegion'
import AdminTablePagination from '../components/AdminTablePagination'
import { StatusBadge } from '../components/StatusBadge'
import { Table } from '../components/ui/table'
import type { QueryLoadState } from './queryLoadState'
import { isBlockingLoadState, isRefreshingLoadState } from './queryLoadState'

const numberFormatter = new Intl.NumberFormat('en-US')
const timestampFormatter = new Intl.DateTimeFormat('zh-CN', {
  month: '2-digit',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit',
  hour12: false,
})
const dateFormatter = new Intl.DateTimeFormat('zh-CN', {
  month: '2-digit',
  day: '2-digit',
})

type StickyUserIdentityLike = StickyUserRow['user']

export interface KeyStickyPanelsProps {
  stickyUsers: StickyUserRow[]
  stickyUsersLoadState: QueryLoadState
  stickyUsersError?: string | null
  stickyUsersPage: number
  stickyUsersTotal: number
  stickyUsersPerPage: number
  onStickyUsersPrevious?: () => void
  onStickyUsersNext?: () => void
  stickyNodes: StickyNode[]
  stickyNodesLoadState: QueryLoadState
  stickyNodesError?: string | null
  onOpenUser?: (userId: string) => void
}

function formatNumber(value: number): string {
  return numberFormatter.format(value)
}

function formatTimestamp(value: number | null): string {
  if (!value) return '—'
  return timestampFormatter.format(new Date(value * 1000))
}

function formatDateOnly(value: number): string {
  return dateFormatter.format(new Date(value * 1000))
}

function stickyUserPrimary(user: StickyUserIdentityLike): string {
  return user.displayName || user.userId
}

function stickyUserSecondary(user: StickyUserIdentityLike): string | null {
  return user.username ? `@${user.username}` : null
}

function buildVisibleBarHeights(successCount: number, failureCount: number, scaleMax: number, totalHeightPx: number) {
  if (scaleMax <= 0 || totalHeightPx <= 0) {
    return { empty: totalHeightPx, failure: 0, success: 0 }
  }

  let success = successCount > 0 ? Math.max((successCount / scaleMax) * totalHeightPx, 1) : 0
  let failure = failureCount > 0 ? Math.max((failureCount / scaleMax) * totalHeightPx, 1) : 0
  const maxVisible = Math.max(totalHeightPx, 0)
  let overflow = success + failure - maxVisible

  const shrink = (value: number, minVisible: number, amount: number) => {
    if (amount <= 0 || value <= minVisible) return { nextValue: value, remaining: amount }
    const delta = Math.min(value - minVisible, amount)
    return { nextValue: value - delta, remaining: amount - delta }
  }

  if (overflow > 0) {
    const first = success >= failure ? 'success' : 'failure'
    const second = first === 'success' ? 'failure' : 'success'
    for (const key of [first, second] as const) {
      const minVisible = key === 'success' ? (successCount > 0 ? 1 : 0) : failureCount > 0 ? 1 : 0
      const current = key === 'success' ? success : failure
      const result = shrink(current, minVisible, overflow)
      if (key === 'success') {
        success = result.nextValue
      } else {
        failure = result.nextValue
      }
      overflow = result.remaining
    }
  }

  const used = Math.min(success + failure, maxVisible)
  return {
    empty: Math.max(maxVisible - used, 0),
    failure,
    success,
  }
}

function StickyCreditsTrendCell({
  buckets,
  scaleMax,
}: {
  buckets: StickyUserDailyBucket[]
  scaleMax: number
}): JSX.Element {
  if (buckets.length === 0) return <span className="token-owner-empty">—</span>

  return (
    <div className="flex h-10 items-end gap-px">
      {buckets.map((bucket) => {
        const total = bucket.successCredits + bucket.failureCredits
        const heights = buildVisibleBarHeights(bucket.successCredits, bucket.failureCredits, scaleMax, 40)
        return (
          <div
            key={bucket.bucketStart}
            className="relative flex h-10 min-w-0 flex-1 flex-col overflow-hidden rounded-[3px] border border-border/40 bg-muted/35"
            title={`${formatDateOnly(bucket.bucketStart)} · ${bucket.successCredits}/${bucket.failureCredits}`}
          >
            <div style={{ height: `${heights.empty}px` }} />
            <div className={total > 0 ? 'bg-destructive/80' : 'bg-transparent'} style={{ height: `${heights.failure}px` }} />
            <div className={total > 0 ? 'bg-success/85' : 'bg-transparent'} style={{ height: `${heights.success}px` }} />
          </div>
        )
      })}
    </div>
  )
}

function StickyWindowValue({
  successValue,
  failureValue,
  successLabel,
  failureLabel,
}: {
  successValue: number
  failureValue: number
  successLabel: string
  failureLabel: string
}): JSX.Element {
  return (
    <span className="sticky-window-values">
      <span
        className="sticky-window-value sticky-window-value-success"
        aria-label={`${successLabel} ${formatNumber(successValue)}`}
      >
        {formatNumber(successValue)}
      </span>
      <span className="sticky-window-value-divider" aria-hidden="true">|</span>
      <span
        className="sticky-window-value sticky-window-value-failure"
        aria-label={`${failureLabel} ${formatNumber(failureValue)}`}
      >
        {formatNumber(failureValue)}
      </span>
    </span>
  )
}

export default function KeyStickyPanels({
  stickyUsers,
  stickyUsersLoadState,
  stickyUsersError,
  stickyUsersPage,
  stickyUsersTotal,
  stickyUsersPerPage,
  onStickyUsersPrevious,
  onStickyUsersNext,
  stickyNodes,
  stickyNodesLoadState,
  stickyNodesError,
  onOpenUser = () => undefined,
}: KeyStickyPanelsProps): JSX.Element {
  const translations = useTranslate()
  const adminStrings = translations.admin
  const keyStrings = adminStrings.keys
  const keyDetailsStrings = adminStrings.keyDetails
  const loadingStateStrings = adminStrings.loadingStates
  const tokenStrings = adminStrings.tokens

  const stickyUserScaleMax = useMemo(
    () => Math.max(...stickyUsers.flatMap((item) => item.dailyBuckets.map((bucket) => bucket.successCredits + bucket.failureCredits)), 0),
    [stickyUsers],
  )
  const stickyUsersBlocking = isBlockingLoadState(stickyUsersLoadState)
  const stickyUsersRefreshing = isRefreshingLoadState(stickyUsersLoadState)
  const stickyUsersLoadingLabel = stickyUsersRefreshing ? loadingStateStrings.refreshing : loadingStateStrings.switching
  const stickyUsersTotalPages = Math.max(1, Math.ceil(stickyUsersTotal / stickyUsersPerPage))
  const stickyNodesRefreshing = isRefreshingLoadState(stickyNodesLoadState)
  const stickyNodesLoadingLabel = stickyNodesRefreshing ? loadingStateStrings.refreshing : loadingStateStrings.switching

  return (
    <>
      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{keyDetailsStrings.stickyUsers.title}</h2>
            <p className="panel-description">{keyDetailsStrings.stickyUsers.description}</p>
          </div>
        </div>
        <AdminLoadingRegion
          className="table-wrapper admin-responsive-up"
          loadState={stickyUsersLoadState}
          loadingLabel={stickyUsersLoadingLabel}
          errorLabel={stickyUsersError ?? adminStrings.errors.loadKeyDetails}
          minHeight={220}
        >
          {stickyUsers.length === 0 ? (
            <div className="empty-state alert">{keyDetailsStrings.stickyUsers.empty}</div>
          ) : (
            <Table>
              <thead>
                <tr>
                  <th>{keyDetailsStrings.stickyUsers.user}</th>
                  <th>{keyDetailsStrings.stickyUsers.yesterday}</th>
                  <th>{keyDetailsStrings.stickyUsers.today}</th>
                  <th>{keyDetailsStrings.stickyUsers.month}</th>
                  <th>{keyDetailsStrings.stickyUsers.lastSuccess}</th>
                  <th>{keyDetailsStrings.stickyUsers.trend}</th>
                </tr>
              </thead>
              <tbody>
                {stickyUsers.map((item) => {
                  const secondary = stickyUserSecondary(item.user)
                  return (
                    <tr key={item.user.userId}>
                      <td>
                        <div className="token-owner-block">
                          <button type="button" className="link-button token-owner-trigger" onClick={() => onOpenUser(item.user.userId)}>
                            <span className="token-owner-link">{stickyUserPrimary(item.user)}</span>
                            {secondary ? <span className="token-owner-secondary">{secondary}</span> : null}
                          </button>
                          {!item.user.active ? <span className="token-owner-empty">{keyDetailsStrings.stickyUsers.inactive}</span> : null}
                        </div>
                      </td>
                      <td>
                        <StickyWindowValue
                          successValue={item.windows.yesterday.successCredits}
                          failureValue={item.windows.yesterday.failureCredits}
                          successLabel={keyDetailsStrings.stickyUsers.success}
                          failureLabel={keyDetailsStrings.stickyUsers.failure}
                        />
                      </td>
                      <td>
                        <StickyWindowValue
                          successValue={item.windows.today.successCredits}
                          failureValue={item.windows.today.failureCredits}
                          successLabel={keyDetailsStrings.stickyUsers.success}
                          failureLabel={keyDetailsStrings.stickyUsers.failure}
                        />
                      </td>
                      <td>
                        <StickyWindowValue
                          successValue={item.windows.month.successCredits}
                          failureValue={item.windows.month.failureCredits}
                          successLabel={keyDetailsStrings.stickyUsers.success}
                          failureLabel={keyDetailsStrings.stickyUsers.failure}
                        />
                      </td>
                      <td>{formatTimestamp(item.lastSuccessAt)}</td>
                      <td style={{ minWidth: 180 }}>
                        <StickyCreditsTrendCell buckets={item.dailyBuckets} scaleMax={stickyUserScaleMax} />
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </Table>
          )}
        </AdminLoadingRegion>
        <AdminLoadingRegion
          className="admin-mobile-list admin-responsive-down"
          loadState={stickyUsersLoadState}
          loadingLabel={stickyUsersLoadingLabel}
          errorLabel={stickyUsersError ?? adminStrings.errors.loadKeyDetails}
          minHeight={220}
        >
          {stickyUsers.length === 0 ? (
            <div className="empty-state alert">{keyDetailsStrings.stickyUsers.empty}</div>
          ) : (
            stickyUsers.map((item) => {
              const secondary = stickyUserSecondary(item.user)
              return (
                <article key={item.user.userId} className="admin-mobile-card">
                  <div className="admin-mobile-kv">
                    <span>{keyDetailsStrings.stickyUsers.user}</span>
                    <strong>
                      <button type="button" className="link-button token-owner-trigger" onClick={() => onOpenUser(item.user.userId)}>
                        <span className="token-owner-link">{stickyUserPrimary(item.user)}</span>
                        {secondary ? <span className="token-owner-secondary">{secondary}</span> : null}
                      </button>
                    </strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{keyDetailsStrings.stickyUsers.yesterday}</span>
                    <strong>
                      <StickyWindowValue
                        successValue={item.windows.yesterday.successCredits}
                        failureValue={item.windows.yesterday.failureCredits}
                        successLabel={keyDetailsStrings.stickyUsers.success}
                        failureLabel={keyDetailsStrings.stickyUsers.failure}
                      />
                    </strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{keyDetailsStrings.stickyUsers.today}</span>
                    <strong>
                      <StickyWindowValue
                        successValue={item.windows.today.successCredits}
                        failureValue={item.windows.today.failureCredits}
                        successLabel={keyDetailsStrings.stickyUsers.success}
                        failureLabel={keyDetailsStrings.stickyUsers.failure}
                      />
                    </strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{keyDetailsStrings.stickyUsers.month}</span>
                    <strong>
                      <StickyWindowValue
                        successValue={item.windows.month.successCredits}
                        failureValue={item.windows.month.failureCredits}
                        successLabel={keyDetailsStrings.stickyUsers.success}
                        failureLabel={keyDetailsStrings.stickyUsers.failure}
                      />
                    </strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{keyDetailsStrings.stickyUsers.lastSuccess}</span>
                    <strong>{formatTimestamp(item.lastSuccessAt)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{keyDetailsStrings.stickyUsers.trend}</span>
                    <div style={{ width: '100%' }}>
                      <StickyCreditsTrendCell buckets={item.dailyBuckets} scaleMax={stickyUserScaleMax} />
                    </div>
                  </div>
                </article>
              )
            })
          )}
        </AdminLoadingRegion>
        {stickyUsersTotal > stickyUsersPerPage ? (
          <AdminTablePagination
            page={stickyUsersPage}
            totalPages={stickyUsersTotalPages}
            pageSummary={
              <span className="panel-description">
                {keyStrings.pagination.page
                  .replace('{page}', String(stickyUsersPage))
                  .replace('{total}', String(stickyUsersTotalPages))}
              </span>
            }
            previousLabel={tokenStrings.pagination.prev}
            nextLabel={tokenStrings.pagination.next}
            previousDisabled={stickyUsersPage <= 1}
            nextDisabled={stickyUsersPage >= stickyUsersTotalPages}
            disabled={stickyUsersBlocking}
            onPrevious={onStickyUsersPrevious ?? (() => undefined)}
            onNext={onStickyUsersNext ?? (() => undefined)}
          />
        ) : null}
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{keyDetailsStrings.stickyNodes.title}</h2>
            <p className="panel-description">{keyDetailsStrings.stickyNodes.description}</p>
          </div>
        </div>
        <AdminLoadingRegion
          className="table-wrapper admin-responsive-up"
          loadState={stickyNodesLoadState}
          loadingLabel={stickyNodesLoadingLabel}
          errorLabel={stickyNodesError ?? adminStrings.errors.loadKeyDetails}
          minHeight={220}
        >
          {stickyNodes.length === 0 ? (
            <div className="empty-state alert">{keyDetailsStrings.stickyNodes.empty}</div>
          ) : (
            <Table>
              <thead>
                <tr>
                  <th>{keyDetailsStrings.stickyNodes.node}</th>
                  <th>{keyDetailsStrings.stickyNodes.primaryAssignmentCount}</th>
                  <th>{keyDetailsStrings.stickyNodes.secondaryAssignmentCount}</th>
                  <th>{keyDetailsStrings.stickyNodes.role}</th>
                </tr>
              </thead>
              <tbody>
                {stickyNodes.map((node) => (
                  <tr key={`${node.role}:${node.key}`}>
                    <td>
                      <div style={{ display: 'grid', gap: 4 }}>
                        <strong>{node.displayName}</strong>
                        <span className="token-owner-empty">{node.key}</span>
                      </div>
                    </td>
                    <td>{formatNumber(node.primaryAssignmentCount)}</td>
                    <td>{formatNumber(node.secondaryAssignmentCount)}</td>
                    <td>
                      <StatusBadge tone={node.role === 'primary' ? 'success' : 'info'}>
                        {node.role === 'primary' ? keyDetailsStrings.stickyNodes.primary : keyDetailsStrings.stickyNodes.secondary}
                      </StatusBadge>
                    </td>
                  </tr>
                ))}
              </tbody>
            </Table>
          )}
        </AdminLoadingRegion>
        <AdminLoadingRegion
          className="admin-mobile-list admin-responsive-down"
          loadState={stickyNodesLoadState}
          loadingLabel={stickyNodesLoadingLabel}
          errorLabel={stickyNodesError ?? adminStrings.errors.loadKeyDetails}
          minHeight={220}
        >
          {stickyNodes.length === 0 ? (
            <div className="empty-state alert">{keyDetailsStrings.stickyNodes.empty}</div>
          ) : (
            stickyNodes.map((node) => (
              <article key={`${node.role}:${node.key}`} className="admin-mobile-card">
                <div className="admin-mobile-kv">
                  <span>{keyDetailsStrings.stickyNodes.node}</span>
                  <strong>{node.displayName}</strong>
                </div>
                <div className="admin-mobile-kv">
                  <span>{keyDetailsStrings.stickyNodes.primaryAssignmentCount}</span>
                  <strong>{formatNumber(node.primaryAssignmentCount)}</strong>
                </div>
                <div className="admin-mobile-kv">
                  <span>{keyDetailsStrings.stickyNodes.secondaryAssignmentCount}</span>
                  <strong>{formatNumber(node.secondaryAssignmentCount)}</strong>
                </div>
                <div className="admin-mobile-kv">
                  <span>{keyDetailsStrings.stickyNodes.role}</span>
                  <StatusBadge tone={node.role === 'primary' ? 'success' : 'info'}>
                    {node.role === 'primary' ? keyDetailsStrings.stickyNodes.primary : keyDetailsStrings.stickyNodes.secondary}
                  </StatusBadge>
                </div>
              </article>
            ))
          )}
        </AdminLoadingRegion>
      </section>
    </>
  )
}
