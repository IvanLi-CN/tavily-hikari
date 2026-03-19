import { useMemo } from 'react'

import type {
  ForwardProxyActivityBucket,
  ForwardProxyStatsNode,
  ForwardProxyWeightBucket,
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
const percentageFormatter = new Intl.NumberFormat('en-US', {
  style: 'percent',
  maximumFractionDigits: 0,
})
const timestampFormatter = new Intl.DateTimeFormat('zh-CN', {
  month: '2-digit',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit',
  hour12: false,
})
const timeRangeFormatter = new Intl.DateTimeFormat('zh-CN', {
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

type WeightTrendScale = {
  minValue: number
  maxValue: number
}

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

function formatTrendTimeRange(startIso: string, endIso: string): string {
  return `${timeRangeFormatter.format(new Date(startIso))} - ${timeRangeFormatter.format(new Date(endIso))}`
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

function ProxyActivityTrendCell({
  buckets,
  scaleMax,
}: {
  buckets: ForwardProxyActivityBucket[]
  scaleMax: number
}): JSX.Element {
  if (buckets.length === 0) return <span className="token-owner-empty">—</span>

  return (
    <div className="flex h-10 items-end gap-px">
      {buckets.map((bucket) => {
        const total = bucket.successCount + bucket.failureCount
        const heights = buildVisibleBarHeights(bucket.successCount, bucket.failureCount, scaleMax, 40)
        return (
          <div
            key={bucket.bucketStart}
            className="relative flex h-10 min-w-0 flex-1 flex-col overflow-hidden rounded-[3px] border border-border/40 bg-muted/35"
            title={`${formatTrendTimeRange(bucket.bucketStart, bucket.bucketEnd)} · ${bucket.successCount}/${bucket.failureCount}`}
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

function buildWeightTrendGeometry(buckets: ForwardProxyWeightBucket[], scale: WeightTrendScale) {
  if (buckets.length === 0) return null

  const chartWidth = 216
  const chartHeight = 40
  const span = Math.max(scale.maxValue - scale.minValue, Number.EPSILON)
  const bucketWidth = chartWidth / buckets.length
  const points = buckets.map((bucket, index) => {
    const ratio = Math.max(0, Math.min(1, (bucket.lastWeight - scale.minValue) / span))
    const x = bucketWidth * index + bucketWidth / 2
    const y = chartHeight - ratio * chartHeight
    return { x, y }
  })
  const firstPoint = points[0]
  const lastPoint = points[points.length - 1]
  if (!firstPoint || !lastPoint) return null

  const zeroRatio = (0 - scale.minValue) / span
  const zeroY = chartHeight - Math.max(0, Math.min(1, zeroRatio)) * chartHeight
  const linePath = points
    .map((point, index) => `${index === 0 ? 'M' : 'L'} ${point.x.toFixed(2)} ${point.y.toFixed(2)}`)
    .join(' ')
  const areaPath = `${linePath} L ${lastPoint.x.toFixed(2)} ${zeroY.toFixed(2)} L ${firstPoint.x.toFixed(2)} ${zeroY.toFixed(2)} Z`

  return { chartWidth, chartHeight, linePath, areaPath, zeroY }
}

function ProxyWeightTrendCell({ buckets, scale }: { buckets: ForwardProxyWeightBucket[]; scale: WeightTrendScale }): JSX.Element {
  const geometry = buildWeightTrendGeometry(buckets, scale)
  if (!geometry) return <span className="token-owner-empty">—</span>

  return (
    <svg
      viewBox={`0 0 ${geometry.chartWidth} ${geometry.chartHeight}`}
      className="block h-10 w-full rounded-md border border-border/55 bg-background/45"
      aria-hidden="true"
    >
      <line x1={0} y1={geometry.zeroY} x2={geometry.chartWidth} y2={geometry.zeroY} stroke="hsl(var(--foreground) / 0.14)" strokeWidth="1" />
      <path d={geometry.areaPath} fill="hsl(var(--success) / 0.18)" />
      <path d={geometry.linePath} fill="none" stroke="hsl(var(--success))" strokeWidth="1.8" strokeLinejoin="round" strokeLinecap="round" />
    </svg>
  )
}

function resolveStickyNodeWeightBuckets(node: ForwardProxyStatsNode): ForwardProxyWeightBucket[] {
  if (node.weight24h.length > 0) return node.weight24h
  if (node.last24h.length === 0) return []
  return node.last24h.map((bucket) => ({
    bucketStart: bucket.bucketStart,
    bucketEnd: bucket.bucketEnd,
    sampleCount: 0,
    minWeight: node.weight,
    maxWeight: node.weight,
    avgWeight: node.weight,
    lastWeight: node.weight,
  }))
}

function stickyNodeWindowSummary(node: StickyNode): string {
  const attempts = node.stats.oneDay.attempts
  const successRate = node.stats.oneDay.successRate
  const latency = node.stats.oneDay.avgLatencyMs
  const rateLabel = successRate == null ? '—' : percentageFormatter.format(successRate)
  const latencyLabel = latency == null ? '—' : `${Math.round(latency)} ms`
  return `${formatNumber(attempts)} · ${rateLabel} · ${latencyLabel}`
}

function stickyNodeAssignmentSummary(
  node: StickyNode,
  strings: {
    assignmentSummary: string
    primaryAssignments: string
    secondaryAssignments: string
  },
): { compact: string; detail: string } {
  const primaryCount = formatNumber(node.primaryAssignmentCount)
  const secondaryCount = formatNumber(node.secondaryAssignmentCount)
  const detail = [
    strings.primaryAssignments.replace('{count}', primaryCount),
    strings.secondaryAssignments.replace('{count}', secondaryCount),
  ].join(' · ')
  return {
    compact: strings.assignmentSummary
      .replace('{primary}', primaryCount)
      .replace('{secondary}', secondaryCount),
    detail,
  }
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
  const stickyNodeScaleMax = useMemo(
    () => Math.max(...stickyNodes.flatMap((node) => node.last24h.map((bucket) => bucket.successCount + bucket.failureCount)), 0),
    [stickyNodes],
  )
  const stickyNodeWeightScale = useMemo(() => {
    const weightValues = stickyNodes.flatMap((node) =>
      resolveStickyNodeWeightBuckets(node).flatMap((bucket) => [bucket.minWeight, bucket.maxWeight, bucket.lastWeight]),
    )
    const minWeightValue = Math.min(...weightValues, 0)
    const maxWeightValue = Math.max(...weightValues, 0)
    const weightPadding = Math.max((maxWeightValue - minWeightValue) * 0.08, 0.2)
    return {
      minValue: minWeightValue - weightPadding,
      maxValue: maxWeightValue + weightPadding,
    }
  }, [stickyNodes])
  const stickyUsersBlocking = isBlockingLoadState(stickyUsersLoadState)
  const stickyUsersRefreshing = isRefreshingLoadState(stickyUsersLoadState)
  const stickyUsersLoadingLabel = stickyUsersRefreshing ? loadingStateStrings.refreshing : loadingStateStrings.switching
  const stickyUsersTotalPages = Math.max(1, Math.ceil(stickyUsersTotal / stickyUsersPerPage))
  const stickyNodesRefreshing = isRefreshingLoadState(stickyNodesLoadState)
  const stickyNodesLoadingLabel = stickyNodesRefreshing ? loadingStateStrings.refreshing : loadingStateStrings.switching

  return (
    <div className="key-sticky-panels-stack">
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
            <Table className="key-sticky-nodes-table">
              <thead>
                <tr>
                  <th>{keyDetailsStrings.stickyNodes.role}</th>
                  <th>{keyDetailsStrings.stickyNodes.node}</th>
                  <th>{keyDetailsStrings.stickyNodes.window}</th>
                  <th>{keyDetailsStrings.stickyNodes.activity}</th>
                  <th>{keyDetailsStrings.stickyNodes.weight}</th>
                </tr>
              </thead>
              <tbody>
                {stickyNodes.map((node) => {
                  const assignmentSummary = stickyNodeAssignmentSummary(node, keyDetailsStrings.stickyNodes)
                  return (
                    <tr key={`${node.role}:${node.key}`}>
                      <td className="key-sticky-nodes-role-cell">
                        <StatusBadge tone={node.role === 'primary' ? 'success' : 'info'}>
                          {node.role === 'primary' ? keyDetailsStrings.stickyNodes.primary : keyDetailsStrings.stickyNodes.secondary}
                        </StatusBadge>
                      </td>
                      <td className="key-sticky-nodes-node-cell">
                        <div className="sticky-node-summary" title={`${node.displayName} · ${assignmentSummary.detail}`}>
                          <strong className="sticky-node-summary-title">{node.displayName}</strong>
                          <div className="sticky-node-summary-meta">
                            <span className="sticky-node-summary-chip" aria-label={assignmentSummary.detail}>
                              {assignmentSummary.compact}
                            </span>
                          </div>
                        </div>
                      </td>
                      <td>{stickyNodeWindowSummary(node)}</td>
                      <td style={{ minWidth: 180 }}>
                        <ProxyActivityTrendCell buckets={node.last24h} scaleMax={stickyNodeScaleMax} />
                      </td>
                      <td style={{ minWidth: 180 }}>
                        <ProxyWeightTrendCell buckets={resolveStickyNodeWeightBuckets(node)} scale={stickyNodeWeightScale} />
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
          loadState={stickyNodesLoadState}
          loadingLabel={stickyNodesLoadingLabel}
          errorLabel={stickyNodesError ?? adminStrings.errors.loadKeyDetails}
          minHeight={220}
        >
          {stickyNodes.length === 0 ? (
            <div className="empty-state alert">{keyDetailsStrings.stickyNodes.empty}</div>
          ) : (
            stickyNodes.map((node) => {
              const assignmentSummary = stickyNodeAssignmentSummary(node, keyDetailsStrings.stickyNodes)
              return (
                <article key={`${node.role}:${node.key}`} className="admin-mobile-card">
                  <div className="admin-mobile-kv">
                    <span>{keyDetailsStrings.stickyNodes.role}</span>
                    <StatusBadge tone={node.role === 'primary' ? 'success' : 'info'}>
                      {node.role === 'primary' ? keyDetailsStrings.stickyNodes.primary : keyDetailsStrings.stickyNodes.secondary}
                    </StatusBadge>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{keyDetailsStrings.stickyNodes.node}</span>
                    <div className="sticky-node-summary" title={`${node.displayName} · ${assignmentSummary.detail}`}>
                      <strong className="sticky-node-summary-title">{node.displayName}</strong>
                      <div className="sticky-node-summary-meta">
                        <span className="sticky-node-summary-chip" aria-label={assignmentSummary.detail}>
                          {assignmentSummary.compact}
                        </span>
                      </div>
                    </div>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{keyDetailsStrings.stickyNodes.activity}</span>
                    <div style={{ width: '100%' }}>
                      <ProxyActivityTrendCell buckets={node.last24h} scaleMax={stickyNodeScaleMax} />
                    </div>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{keyDetailsStrings.stickyNodes.weight}</span>
                    <div style={{ width: '100%' }}>
                      <ProxyWeightTrendCell buckets={resolveStickyNodeWeightBuckets(node)} scale={stickyNodeWeightScale} />
                    </div>
                  </div>
                </article>
              )
            })
          )}
        </AdminLoadingRegion>
      </section>
    </div>
  )
}
