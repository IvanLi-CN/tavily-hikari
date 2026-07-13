import { useEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent } from 'react'
import { ChevronLeft, ChevronRight } from 'lucide-react'

import type {
  RechargeConfig,
  RechargeOrder,
  RechargeQuote,
  UserBillingSummary,
} from '../api'
import AdminTablePagination from '../components/AdminTablePagination'
import { StatusBadge, type StatusTone } from '../components/StatusBadge'
import { useViewportMode } from '../lib/responsive'
import RechargePanel from './RechargePanel'

interface BillingText {
  title: string
  description: string
  summaryTitle: string
  summaryDescription: string
  currentTotal: string
  baseEntitlements: string
  monthlyAdjustments: string
  rechargePackage: string
  rechargeCredits: string
  emptyDelta: string
  blockAllNotice: string
  pricingTitle: string
  pricingDescription: string
  unitPrice: string
  creditStep: string
  monthsRange: string
  testPriceEnabled: string
  unavailableNotice: string
  timelineTitle: string
  timelineDescription: string
  timelinePrevious: string
  timelineCurrent: string
  timelineFuture: string
  timelineBack: string
  timelineForward: string
  timelineSelectMonth: string
  timelinePersistent: string
  timelineAdjustments: string
  timelineRecharge: string
  timelineEffective: string
  timelineRechargeCredits: string
  timelineNoFuture: string
  timelineNoScheduledChanges: string
  ordersTitle: string
  ordersDescription: string
  orderCreatedAt: string
  orderImpact: string
  orderClampApplied: string
  ordersPageSummary: string
  ordersPreviousPage: string
  ordersNextPage: string
}

interface RechargePanelText {
  title: string
  description: string
  enabled: string
  disabled: string
  currentEntitlement: string
  currentMonthFinal: string
  effectiveUntil: string
  noEntitlement: string
  credits: string
  months: string
  quotaDelta: string
  hourlyDelta: string
  dailyDelta: string
  monthlyDelta: string
  testPrice: string
  amount: string
  discountedAmount: string
  discountNotice: string
  clampNotice: string
  preview: string
  previewTitle: string
  previewDescription: string
  previewScopeNote: string
  previewMonth: string
  previewCurrentQuota: string
  previewDelta: string
  previewExpectedQuota: string
  previewAfterExpiry: string
  closePreview: string
  create: string
  creating: string
  unavailable: string
  orders: string
  noOrders: string
  status: Record<string, string>
  orderStatusDetail: Record<string, string>
}

interface BillingPageProps {
  text: BillingText
  rechargeText: RechargePanelText
  summary: UserBillingSummary | null
  config: RechargeConfig | null
  orders: RechargeOrder[]
  loading: boolean
  credits: number
  months: number
  quote: RechargeQuote | null
  busy: boolean
  error: string | null
  language: 'en' | 'zh'
  onCreditsChange: (value: number) => void
  onMonthsChange: (value: number) => void
  onCreateOrder: () => void
}

interface BillingQuota {
  hourly: number
  daily: number
  monthly: number
}

type BillingTimelineMonth = NonNullable<BillingPageProps['summary']>['timeline'][number]

const ORDERS_PER_PAGE = 10

function formatTemplate(template: string, values: Record<string, string | number>): string {
  return template.replace(/\{(\w+)\}/g, (_, key: string) => String(values[key] ?? ''))
}

function formatNumber(value: number): string {
  return value.toLocaleString('en-US', { maximumFractionDigits: 0 })
}

function formatMoneyLdc(value: number): string {
  return value.toLocaleString('en-US', {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  })
}

function formatMonthLabel(monthStart: number, language: 'en' | 'zh'): string {
  return new Intl.DateTimeFormat(language === 'zh' ? 'zh-CN' : 'en-US', {
    year: 'numeric',
    month: 'short',
  }).format(new Date(monthStart * 1000))
}

function formatDateTime(value: number, language: 'en' | 'zh'): string {
  return new Intl.DateTimeFormat(language === 'zh' ? 'zh-CN' : 'en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(value * 1000))
}

function formatMonthsRange(minMonths: number, maxMonths: number, language: 'en' | 'zh'): string {
  const range = minMonths === maxMonths ? `${minMonths}` : `${minMonths} - ${maxMonths}`
  return language === 'zh' ? `${range} 个月` : `${range} months`
}

function formatCreditStepValue(value: number, language: 'en' | 'zh'): string {
  const formatted = formatNumber(value)
  return language === 'zh' ? `${formatted} 月积分` : `${formatted} monthly credits`
}

function orderStatusTone(status: string): StatusTone {
  if (status === 'paid') return 'success'
  if (status === 'pending' || status === 'expired' || status === 'refunding') return 'warning'
  if (status === 'failed') return 'error'
  return 'neutral'
}

function orderStatusDetail(
  order: RechargeOrder,
  text: RechargePanelText,
  language: 'en' | 'zh',
): string | null {
  if (order.status === 'pending') {
    return formatTemplate(text.orderStatusDetail.pending, {
      time: formatDateTime(order.payExpiresAt, language),
    })
  }
  if (order.status === 'paid' && order.paidAt != null) {
    return formatTemplate(text.orderStatusDetail.paid, {
      time: formatDateTime(order.paidAt, language),
    })
  }
  if (order.status === 'failed') return text.orderStatusDetail.failed
  if (order.status === 'expired') {
    return formatTemplate(text.orderStatusDetail.expired, {
      time: formatDateTime(order.payExpiresAt, language),
    })
  }
  if (order.status === 'cancelled') {
    return formatTemplate(text.orderStatusDetail.cancelled, {
      time: formatDateTime(order.cancelledAt ?? order.cancelAfterAt, language),
    })
  }
  if (order.status === 'refunding') {
    if (order.refundRetryAfterAt != null) {
      return `${text.orderStatusDetail.refunding} ${formatTemplate(text.orderStatusDetail.refundRetryAt, {
        time: formatDateTime(order.refundRetryAfterAt, language),
      })}`
    }
    return text.orderStatusDetail.refunding
  }
  if (order.status === 'refunded') return text.orderStatusDetail.refunded
  if (order.status === 'refundOnly') return text.orderStatusDetail.refundOnly
  return null
}

function isZeroQuota(value: BillingQuota): boolean {
  return value.hourly === 0 && value.daily === 0 && value.monthly === 0
}

function sumQuotas(...quotas: BillingQuota[]): BillingQuota {
  return quotas.reduce<BillingQuota>((total, quota) => ({
    hourly: total.hourly + quota.hourly,
    daily: total.daily + quota.daily,
    monthly: total.monthly + quota.monthly,
  }), {
    hourly: 0,
    daily: 0,
    monthly: 0,
  })
}

function clampIndex(value: number, max: number): number {
  return Math.min(Math.max(value, 0), Math.max(max, 0))
}

function resolveTimelineVisibleCount(width: number, viewportMode: 'small' | 'normal'): 1 | 2 | 3 {
  if (viewportMode === 'small') return 1
  if (width >= 980) return 3
  if (width >= 680) return 2
  return 1
}

function scrollTimelineToIndex(
  viewport: HTMLDivElement | null,
  index: number,
  behavior: ScrollBehavior,
): void {
  if (!viewport) return

  const cards = Array.from(viewport.querySelectorAll<HTMLElement>('[data-timeline-index]'))
  const target = cards[index]
  if (!target) return

  viewport.scrollTo({
    left: target.offsetLeft - viewport.offsetLeft,
    behavior,
  })
}

function QuotaStrip({
  quota,
  tone = 'table',
  muted = false,
}: {
  quota: BillingQuota
  tone?: 'hero' | 'table' | 'micro'
  muted?: boolean
}): JSX.Element {
  return (
    <div className={`user-console-billing-quota-strip is-${tone}${muted ? ' is-muted' : ''}`}>
      <div>
        <span>1H</span>
        <strong>{formatNumber(quota.hourly)}</strong>
      </div>
      <div>
        <span>1D</span>
        <strong>{formatNumber(quota.daily)}</strong>
      </div>
      <div>
        <span>1M</span>
        <strong>{formatNumber(quota.monthly)}</strong>
      </div>
    </div>
  )
}

function SummaryRow({
  title,
  description,
  quota,
  badge,
}: {
  title: string
  description?: string | null
  quota: BillingQuota
  badge?: string | null
}): JSX.Element {
  return (
    <li className="user-console-billing-summary-row">
      <div className="user-console-billing-summary-row-copy">
        <div className="user-console-billing-summary-row-title">
          <h3>{title}</h3>
          {description ? <span className="user-console-billing-summary-row-note">{description}</span> : null}
          {badge ? <span className="user-console-billing-inline-badge">{badge}</span> : null}
        </div>
      </div>
      <QuotaStrip quota={quota} muted={isZeroQuota(quota)} />
    </li>
  )
}

function TimelineQuotaRow({
  label,
  quota,
}: {
  label: string
  quota: BillingQuota
}): JSX.Element {
  return (
    <div className="user-console-billing-timeline-row">
      <span className="user-console-billing-timeline-row-label">{label}</span>
      <QuotaStrip quota={quota} tone="micro" muted={isZeroQuota(quota)} />
    </div>
  )
}

function TimelineNavButton({
  direction,
  label,
  disabled,
  onClick,
}: {
  direction: 'prev' | 'next'
  label: string
  disabled: boolean
  onClick: () => void
}): JSX.Element {
  return (
    <button
      type="button"
      className={`user-console-billing-timeline-nav-button is-${direction}`}
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={onClick}
    >
      {direction === 'prev' ? (
        <ChevronLeft aria-hidden="true" size={20} strokeWidth={2.25} />
      ) : (
        <ChevronRight aria-hidden="true" size={20} strokeWidth={2.25} />
      )}
      <span className="sr-only">{label}</span>
    </button>
  )
}

function resolveTimelinePhaseLabel(
  month: BillingTimelineMonth,
  currentMonthStart: number,
  text: BillingText,
): string {
  if (month.isCurrentMonth) return text.timelineCurrent
  return month.monthStart < currentMonthStart ? text.timelinePrevious : text.timelineFuture
}

function TimelineCard({
  language,
  text,
  currentMonthStart,
  month,
  index,
  selected,
  onSelect,
}: {
  language: 'en' | 'zh'
  text: BillingText
  currentMonthStart: number
  month: BillingTimelineMonth
  index: number
  selected: boolean
  onSelect: (index: number) => void
}): JSX.Element {
  const monthLabel = formatMonthLabel(month.monthStart, language)
  const phaseLabel = resolveTimelinePhaseLabel(month, currentMonthStart, text)
  const rechargeBadge = month.recharge.credits > 0
    ? formatTemplate(text.timelineRechargeCredits, { credits: formatNumber(month.recharge.credits) })
    : null
  const selectionLabel = formatTemplate(text.timelineSelectMonth, { month: monthLabel })
  const handleKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key !== 'Enter' && event.key !== ' ') return
    event.preventDefault()
    onSelect(index)
  }

  return (
    <article
      className={`user-console-billing-timeline-card${selected ? ' is-selected' : ''}`}
      data-timeline-index={index}
      role="button"
      tabIndex={0}
      aria-pressed={selected}
      aria-label={selectionLabel}
      onClick={() => onSelect(index)}
      onKeyDown={handleKeyDown}
    >
      <div className="user-console-billing-timeline-card-head">
        <div className="user-console-billing-timeline-card-aside">
          <p>{phaseLabel}</p>
          <h3>{monthLabel}</h3>
        </div>
        {rechargeBadge ? <span className="user-console-billing-inline-badge">{rechargeBadge}</span> : null}
      </div>
      <div className="user-console-billing-timeline-total">
        <span>{text.timelineEffective}</span>
        <QuotaStrip quota={month.effectiveTotal} tone="micro" />
      </div>
      <div className="user-console-billing-timeline-card-body">
        <TimelineQuotaRow label={text.timelinePersistent} quota={month.persistentTotal} />
        <TimelineQuotaRow label={text.timelineAdjustments} quota={month.monthlyAdjustments} />
        <TimelineQuotaRow label={text.timelineRecharge} quota={month.recharge.quota} />
      </div>
    </article>
  )
}

export default function BillingPage({
  text,
  rechargeText,
  summary,
  config,
  orders,
  loading,
  credits,
  months,
  quote,
  busy,
  error,
  language,
  onCreditsChange,
  onMonthsChange,
  onCreateOrder,
}: BillingPageProps): JSX.Element {
  const viewportMode = useViewportMode()
  const timelineViewportRef = useRef<HTMLDivElement | null>(null)
  const [timelineVisibleCount, setTimelineVisibleCount] = useState<1 | 2 | 3>(1)
  const [timelineWindowIndex, setTimelineWindowIndex] = useState(0)
  const [selectedTimelineIndex, setSelectedTimelineIndex] = useState(0)
  const [ordersPage, setOrdersPage] = useState(1)
  const timeline = summary?.timeline ?? []
  const rechargeVisible = config?.visible ?? false
  const purchaseConfig = rechargeVisible
    ? config
    : config
      ? { ...config, visible: false, enabled: false }
      : null
  const unitPriceText = config
    ? language === 'zh'
      ? `${formatMoneyLdc(config.unitPriceLdc)} LDC 可换 ${formatNumber(config.unitCredits)} 月积分`
      : `${formatMoneyLdc(config.unitPriceLdc)} LDC exchanges for ${formatNumber(config.unitCredits)} monthly credits`
    : text.unavailableNotice
  const effectiveUntilLabel = summary?.effectiveUntilMonthStart
    ? formatMonthLabel(summary.effectiveUntilMonthStart, language)
    : null
  const currentTimelineIndex = Math.max(0, timeline.findIndex((item) => item.isCurrentMonth))
  const visibleTimelineCount = Math.max(1, Math.min(timelineVisibleCount, timeline.length || 1))
  const maxTimelineIndex = Math.max(0, timeline.length - visibleTimelineCount)
  const defaultTimelineIndex = useMemo(() => {
    if (timeline.length === 0) return 0
    if (visibleTimelineCount >= 3) {
      return clampIndex(currentTimelineIndex - 1, maxTimelineIndex)
    }
    return clampIndex(currentTimelineIndex, maxTimelineIndex)
  }, [currentTimelineIndex, maxTimelineIndex, timeline.length, visibleTimelineCount])
  const safeTimelineWindowIndex = clampIndex(timelineWindowIndex, maxTimelineIndex)
  const safeSelectedTimelineIndex = clampIndex(selectedTimelineIndex, timeline.length - 1)
  const visibleTimelineEndIndex = Math.min(
    timeline.length - 1,
    safeTimelineWindowIndex + visibleTimelineCount - 1,
  )
  const selectedTimelineMonth = timeline[safeSelectedTimelineIndex] ?? null
  const selectedTimelinePhaseLabel = selectedTimelineMonth
    ? resolveTimelinePhaseLabel(selectedTimelineMonth, summary?.currentMonthStart ?? selectedTimelineMonth.monthStart, text)
    : null
  const selectedTimelineRechargeBadge = selectedTimelineMonth && selectedTimelineMonth.recharge.credits > 0
    ? formatTemplate(text.timelineRechargeCredits, {
      credits: formatNumber(selectedTimelineMonth.recharge.credits),
    })
    : null
  const visibleTimelineRangeLabel = timeline.length === 0
    ? null
    : visibleTimelineCount === 1
      ? formatMonthLabel(timeline[safeTimelineWindowIndex].monthStart, language)
      : `${formatMonthLabel(timeline[safeTimelineWindowIndex].monthStart, language)} - ${formatMonthLabel(
        timeline[visibleTimelineEndIndex].monthStart,
        language,
      )}`
  const visibleTimelineProgress = timeline.length === 0
    ? null
    : visibleTimelineCount === 1
      ? `${safeTimelineWindowIndex + 1} / ${timeline.length}`
      : `${safeTimelineWindowIndex + 1}-${visibleTimelineEndIndex + 1} / ${timeline.length}`
  const hasFutureScheduledEntitlement = summary
    ? timeline.some((item) => (
      item.monthStart > summary.currentMonthStart
      && (
        item.recharge.credits > 0
        || !isZeroQuota(item.recharge.quota)
        || !isZeroQuota(item.monthlyAdjustments)
      )
    ))
    : false
  const timelineViewportStyle = {
    '--billing-timeline-visible': String(visibleTimelineCount),
  } as CSSProperties
  const baselineEntitlements = summary
    ? sumQuotas(
      summary.composition.baseAccess,
      summary.composition.permanentEntitlements,
      summary.composition.tagAdjustments,
    )
    : null
  const summaryRows = summary && selectedTimelineMonth ? [
    {
      title: text.baseEntitlements,
      description: baselineEntitlements && isZeroQuota(baselineEntitlements) ? text.emptyDelta : null,
      quota: baselineEntitlements ?? selectedTimelineMonth.persistentTotal,
      badge: null,
    },
    {
      title: text.monthlyAdjustments,
      description: isZeroQuota(selectedTimelineMonth.monthlyAdjustments) ? text.emptyDelta : null,
      quota: selectedTimelineMonth.monthlyAdjustments,
      badge: null,
    },
    {
      title: text.rechargePackage,
      description: isZeroQuota(selectedTimelineMonth.recharge.quota) ? text.emptyDelta : null,
      quota: selectedTimelineMonth.recharge.quota,
      badge: selectedTimelineMonth.recharge.credits > 0
        ? formatTemplate(text.rechargeCredits, {
          credits: formatNumber(selectedTimelineMonth.recharge.credits),
        })
        : null,
    },
  ] : []
  const ordersTotalPages = Math.max(1, Math.ceil(orders.length / ORDERS_PER_PAGE))
  const safeOrdersPage = Math.min(Math.max(ordersPage, 1), ordersTotalPages)
  const visibleOrders = useMemo(() => {
    if (orders.length === 0) return []
    const start = (safeOrdersPage - 1) * ORDERS_PER_PAGE
    return orders.slice(start, start + ORDERS_PER_PAGE)
  }, [orders, safeOrdersPage])
  const ordersPageSummary = formatTemplate(text.ordersPageSummary, {
    page: safeOrdersPage,
    totalPages: ordersTotalPages,
    total: formatNumber(orders.length),
  })

  useEffect(() => {
    const node = timelineViewportRef.current
    if (!node) {
      setTimelineVisibleCount(viewportMode === 'small' ? 1 : 3)
      return
    }

    const update = () => {
      const width = node.getBoundingClientRect().width
      setTimelineVisibleCount(resolveTimelineVisibleCount(width, viewportMode))
    }

    update()

    const resizeObserver = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(update) : null
    resizeObserver?.observe(node)
    window.addEventListener('resize', update)

    return () => {
      resizeObserver?.disconnect()
      window.removeEventListener('resize', update)
    }
  }, [timeline.length, viewportMode])

  useEffect(() => {
    setTimelineWindowIndex(defaultTimelineIndex)
    const handle = window.requestAnimationFrame(() => {
      scrollTimelineToIndex(timelineViewportRef.current, defaultTimelineIndex, 'auto')
    })
    return () => window.cancelAnimationFrame(handle)
  }, [defaultTimelineIndex])

  useEffect(() => {
    if (timeline.length === 0) {
      setSelectedTimelineIndex(0)
      return
    }

    setSelectedTimelineIndex(currentTimelineIndex)
  }, [currentTimelineIndex, timeline.length])

  useEffect(() => {
    const viewport = timelineViewportRef.current
    if (!viewport) return

    let frame = 0
    let isInitialSync = true
    const syncWindowIndex = () => {
      const cards = Array.from(viewport.querySelectorAll<HTMLElement>('[data-timeline-index]'))
      if (cards.length === 0) {
        setTimelineWindowIndex(0)
        return
      }

      const scrollLeft = viewport.scrollLeft
      let nextIndex = 0
      let closestOffset = Number.POSITIVE_INFINITY

      cards.forEach((card, index) => {
        const offset = Math.abs(card.offsetLeft - viewport.offsetLeft - scrollLeft)
        if (offset < closestOffset) {
          closestOffset = offset
          nextIndex = index
        }
      })

      setTimelineWindowIndex((current) => current === nextIndex ? current : nextIndex)
      if (visibleTimelineCount === 1 && !isInitialSync) {
        setSelectedTimelineIndex((current) => current === nextIndex ? current : nextIndex)
      }
    }

    const handleScroll = () => {
      window.cancelAnimationFrame(frame)
      frame = window.requestAnimationFrame(syncWindowIndex)
    }

    syncWindowIndex()
    isInitialSync = false
    viewport.addEventListener('scroll', handleScroll, { passive: true })

    return () => {
      window.cancelAnimationFrame(frame)
      viewport.removeEventListener('scroll', handleScroll)
    }
  }, [timeline.length, visibleTimelineCount])

  useEffect(() => {
    setOrdersPage((current) => Math.min(Math.max(current, 1), ordersTotalPages))
  }, [ordersTotalPages])

  useEffect(() => {
    if (timeline.length === 0) return

    setSelectedTimelineIndex((current) => {
      const clampedCurrent = clampIndex(current, timeline.length - 1)
      if (clampedCurrent < safeTimelineWindowIndex) return safeTimelineWindowIndex
      if (clampedCurrent > visibleTimelineEndIndex) return visibleTimelineEndIndex
      return current
    })
  }, [safeTimelineWindowIndex, timeline.length, visibleTimelineEndIndex])

  const moveTimeline = (step: number) => {
    const nextIndex = clampIndex(safeTimelineWindowIndex + step, maxTimelineIndex)
    setTimelineWindowIndex(nextIndex)
    if (visibleTimelineCount === 1) setSelectedTimelineIndex(nextIndex)
    scrollTimelineToIndex(timelineViewportRef.current, nextIndex, 'smooth')
  }

  return (
    <div className="user-console-billing-stack">
      <section className="surface panel user-console-section user-console-billing-section">
        <header className="user-console-billing-stage-head">
          <div className="user-console-billing-stage-intro">
            <h2>{text.timelineTitle}</h2>
            <p>{text.timelineDescription}</p>
          </div>
          {summary ? (
            <div className="user-console-billing-stage-meta">
              <span className="user-console-billing-meta-pill">
                {formatMonthLabel(summary.currentMonthStart, language)}
              </span>
              {effectiveUntilLabel ? (
                <span className="user-console-billing-meta-pill">{effectiveUntilLabel}</span>
              ) : null}
              {summary.blockAll ? (
                <span className="user-console-billing-meta-pill is-warning">block_all</span>
              ) : null}
            </div>
          ) : null}
        </header>
        {loading && timeline.length === 0 ? (
          <div className="empty-state">Loading timeline...</div>
        ) : timeline.length > 0 ? (
          <>
            <div className="user-console-billing-timeline-stage">
              {viewportMode === 'small' ? null : (
                <TimelineNavButton
                  direction="prev"
                  label={text.timelineBack}
                  disabled={safeTimelineWindowIndex <= 0}
                  onClick={() => moveTimeline(-1)}
                />
              )}
              <div
                ref={timelineViewportRef}
                className="user-console-billing-timeline-viewport"
                style={timelineViewportStyle}
              >
                <div className="user-console-billing-timeline-track">
                  {timeline.map((month, index) => (
                    <TimelineCard
                      key={month.monthStart}
                      index={index}
                      language={language}
                      text={text}
                      currentMonthStart={summary?.currentMonthStart ?? month.monthStart}
                      month={month}
                      selected={index === safeSelectedTimelineIndex}
                      onSelect={(nextIndex) => setSelectedTimelineIndex(nextIndex)}
                    />
                  ))}
                </div>
              </div>
              {viewportMode === 'small' ? null : (
                <TimelineNavButton
                  direction="next"
                  label={text.timelineForward}
                  disabled={safeTimelineWindowIndex >= maxTimelineIndex}
                  onClick={() => moveTimeline(1)}
                />
              )}
            </div>

            {viewportMode === 'small' ? (
              <div className="user-console-billing-timeline-mobile-controls">
                <TimelineNavButton
                  direction="prev"
                  label={text.timelineBack}
                  disabled={safeTimelineWindowIndex <= 0}
                  onClick={() => moveTimeline(-1)}
                />
                <div className="user-console-billing-timeline-status">
                  {visibleTimelineRangeLabel ? <strong>{visibleTimelineRangeLabel}</strong> : null}
                  {visibleTimelineProgress ? <span>{visibleTimelineProgress}</span> : null}
                </div>
                <TimelineNavButton
                  direction="next"
                  label={text.timelineForward}
                  disabled={safeTimelineWindowIndex >= maxTimelineIndex}
                  onClick={() => moveTimeline(1)}
                />
              </div>
            ) : (
              <div className="user-console-billing-timeline-status-bar">
                {visibleTimelineRangeLabel ? <strong>{visibleTimelineRangeLabel}</strong> : null}
                {visibleTimelineProgress ? <span>{visibleTimelineProgress}</span> : null}
              </div>
            )}
          </>
        ) : (
          <div className="empty-state">{text.timelineNoFuture}</div>
        )}
        {timeline.length > 0 && !hasFutureScheduledEntitlement ? (
          <p className="user-console-billing-inline-note">{text.timelineNoScheduledChanges}</p>
        ) : null}
      </section>

      <div className="user-console-billing-workbench">
        <div className="user-console-billing-main-column">
          <section className="surface panel user-console-section user-console-billing-section user-console-billing-summary-section">
            <header className="panel-header user-console-section-header user-console-billing-summary-head">
              <div>
                <h2>{text.summaryTitle}</h2>
                <p className="panel-description">{text.summaryDescription}</p>
              </div>
              {selectedTimelineMonth ? (
                <div className="user-console-billing-summary-meta">
                  <span className="user-console-billing-meta-pill">
                    {formatMonthLabel(selectedTimelineMonth.monthStart, language)}
                  </span>
                  {selectedTimelinePhaseLabel ? (
                    <span className="user-console-billing-meta-pill">{selectedTimelinePhaseLabel}</span>
                  ) : null}
                  {selectedTimelineRechargeBadge ? (
                    <span className="user-console-billing-meta-pill">{selectedTimelineRechargeBadge}</span>
                  ) : null}
                </div>
              ) : null}
            </header>
            {loading && !summary ? (
              <div className="empty-state">Loading billing summary...</div>
            ) : summary && selectedTimelineMonth ? (
              <>
                <div className="user-console-billing-current-total-row">
                  <span>{text.timelineEffective}</span>
                  <QuotaStrip quota={selectedTimelineMonth.effectiveTotal} tone="table" />
                </div>
                <ul className="user-console-billing-summary-list">
                  {summaryRows.map((row) => (
                    <SummaryRow
                      key={row.title}
                      title={row.title}
                      description={row.description}
                      quota={row.quota}
                      badge={row.badge}
                    />
                  ))}
                </ul>
                {summary.blockAll ? (
                  <p className="user-console-billing-notice">{text.blockAllNotice}</p>
                ) : null}
              </>
            ) : (
              <div className="empty-state">{text.emptyDelta}</div>
            )}
          </section>
        </div>

        <aside className="user-console-landing-rail user-console-billing-side-column">
          <section className="user-console-billing-pricing-inline" aria-label={text.pricingTitle}>
            <div className="user-console-billing-pricing-inline-head">
              <div>
                <h3>{text.pricingTitle}</h3>
                {text.pricingDescription ? <p>{text.pricingDescription}</p> : null}
              </div>
              {rechargeVisible ? (
                <StatusBadge tone={config?.enabled ? 'success' : 'neutral'}>
                  {config?.enabled ? rechargeText.enabled : rechargeText.disabled}
                </StatusBadge>
              ) : (
                <StatusBadge tone="neutral">{rechargeText.disabled}</StatusBadge>
              )}
            </div>
            <div className="user-console-billing-pricing-inline-metrics">
              <div>
                <span>{text.unitPrice}</span>
                <strong>{unitPriceText}</strong>
              </div>
              <div>
                <span>{text.creditStep}</span>
                <strong>{config ? formatCreditStepValue(config.creditsStep, language) : '0'}</strong>
              </div>
              <div>
                <span>{text.monthsRange}</span>
                <strong>{config ? formatMonthsRange(config.minMonths, config.maxMonths, language) : '0'}</strong>
              </div>
            </div>
            {config?.testPriceEnabled && text.testPriceEnabled ? (
              <p className="user-console-billing-inline-note">{text.testPriceEnabled}</p>
            ) : null}
            {!config?.enabled || !rechargeVisible ? (
              <p className="user-console-billing-inline-note">{text.unavailableNotice}</p>
            ) : null}
          </section>
          <RechargePanel
            text={rechargeText}
            language={language}
            dashboard={null}
            config={purchaseConfig}
            orders={[]}
            credits={credits}
            months={months}
            quote={quote}
            busy={busy}
            error={error}
            onCreditsChange={onCreditsChange}
            onMonthsChange={onMonthsChange}
            onCreateOrder={onCreateOrder}
            showSummary={false}
            showOrders={false}
          />
        </aside>

        <section className="surface panel user-console-section user-console-billing-section user-console-billing-orders-section">
          <header className="panel-header user-console-section-header">
            <div>
              <h2>{text.ordersTitle}</h2>
              <p className="panel-description">{text.ordersDescription}</p>
            </div>
          </header>
          {orders.length === 0 ? (
            <div className="empty-state">{rechargeText.noOrders}</div>
          ) : (
            <>
              <div className="user-console-billing-orders-table" role="list">
                {visibleOrders.map((order) => {
                  const detail = orderStatusDetail(order, rechargeText, language)
                  return (
                    <article key={order.outTradeNo} className="user-console-billing-order-row" role="listitem">
                      <div className="user-console-billing-order-primary">
                        <div className="user-console-billing-order-title-row">
                          <h3>{formatNumber(order.credits)} × {order.months}</h3>
                          {order.monthEndClampApplied ? (
                            <span className="user-console-billing-inline-badge is-warm">{text.orderClampApplied}</span>
                          ) : null}
                        </div>
                        <p>{formatTemplate(text.orderCreatedAt, {
                          time: formatDateTime(order.createdAt, language),
                        })}</p>
                      </div>
                      <div className="user-console-billing-order-facts">
                        <strong>{formatNumber(order.credits)} / {order.months}</strong>
                        <strong>{order.money} LDC</strong>
                        <strong>{formatMonthLabel(order.quoteMonthStart, language)}</strong>
                      </div>
                      <div className="user-console-billing-order-impact">
                        <span>{text.timelineEffective}</span>
                        <strong>{formatTemplate(text.orderImpact, {
                          hourly: formatNumber(order.finalHourlyDelta),
                          daily: formatNumber(order.finalDailyDelta),
                          monthly: formatNumber(order.finalMonthlyDelta),
                        })}</strong>
                        {detail ? <p className="user-console-billing-order-detail">{detail}</p> : null}
                      </div>
                      <div className="user-console-billing-order-status">
                        <StatusBadge tone={orderStatusTone(order.status)}>
                          {rechargeText.status[order.status] ?? order.status}
                        </StatusBadge>
                      </div>
                    </article>
                  )
                })}
              </div>
              {ordersTotalPages > 1 ? (
                <AdminTablePagination
                  page={safeOrdersPage}
                  totalPages={ordersTotalPages}
                  pageSummary={ordersPageSummary}
                  previousLabel={text.ordersPreviousPage}
                  nextLabel={text.ordersNextPage}
                  previousDisabled={safeOrdersPage <= 1}
                  nextDisabled={safeOrdersPage >= ordersTotalPages}
                  onPrevious={() => setOrdersPage((current) => Math.max(1, current - 1))}
                  onNext={() => setOrdersPage((current) => Math.min(ordersTotalPages, current + 1))}
                />
              ) : null}
            </>
          )}
        </section>
      </div>
    </div>
  )
}
