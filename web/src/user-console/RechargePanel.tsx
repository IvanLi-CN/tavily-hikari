import type { RechargeConfig, RechargeOrder, RechargeQuote } from '../api'
import type { UserDashboard } from '../api'
import { Eye, Minus, Plus } from 'lucide-react'
import { useMemo, useState } from 'react'
import { Icon } from '../lib/icons'
import { Button } from '../components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '../components/ui/dialog'
import {
  Drawer,
  DrawerClose,
  DrawerContent,
  DrawerDescription,
  DrawerFooter,
  DrawerHeader,
  DrawerTitle,
} from '../components/ui/drawer'
import { StatusBadge, type StatusTone } from '../components/StatusBadge'
import { useViewportMode } from '../lib/responsive'
import {
  DEFAULT_RECHARGE_UNIT_CREDITS,
  TEST_RECHARGE_CREDITS,
  TEST_RECHARGE_MONTHS,
  isTestRechargeSelection,
  nextRechargeCredits,
  normalizeRechargeMonths,
  normalizeRechargeSelection,
} from './rechargeControls'

const DEFAULT_RECHARGE_MAX_CREDITS = 20_000
const DEFAULT_RECHARGE_MAX_MONTHS = 12

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
}

interface RechargePanelProps {
  text: RechargePanelText
  dashboard: UserDashboard | null
  config: RechargeConfig | null
  orders: RechargeOrder[]
  credits: number
  months: number
  quote: RechargeQuote | null
  busy: boolean
  error: string | null
  onCreditsChange: (value: number) => void
  onMonthsChange: (value: number) => void
  onCreateOrder: () => void
}

interface RechargePreviewMonth {
  monthStart: number
  currentQuota: number
  delta: number
  expectedQuota: number
  afterExpiry: boolean
  clampApplied: boolean
}

function formatRechargeMoney(value: number): string {
  return value.toLocaleString('en-US', {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  })
}

function rechargeStatusTone(status: string): StatusTone {
  if (status === 'paid') return 'success'
  if (status === 'failed' || status === 'expired') return 'error'
  if (status === 'pending') return 'warning'
  return 'neutral'
}

export default function RechargePanel({
  text,
  dashboard,
  config,
  orders,
  credits,
  months,
  quote,
  busy,
  error,
  onCreditsChange,
  onMonthsChange,
  onCreateOrder,
}: RechargePanelProps): JSX.Element {
  const [previewOpen, setPreviewOpen] = useState(false)
  const viewportMode = useViewportMode()
  const unitCredits = config?.unitCredits ?? DEFAULT_RECHARGE_UNIT_CREDITS
  const minCredits = config?.minCredits ?? unitCredits
  const maxCredits = config?.maxCredits ?? DEFAULT_RECHARGE_MAX_CREDITS
  const creditsStep = config?.creditsStep ?? unitCredits
  const minMonths = config?.minMonths ?? 1
  const maxMonths = config?.maxMonths ?? DEFAULT_RECHARGE_MAX_MONTHS
  const stepConfig = {
    minCredits,
    maxCredits,
    creditsStep,
    minMonths,
    maxMonths,
    testPriceEnabled: config?.testPriceEnabled ?? false,
  }
  const { credits: normalizedCredits, months: normalizedMonths } = normalizeRechargeSelection(
    credits,
    months,
    stepConfig,
  )
  const isTestOffer = config?.testPriceEnabled && isTestRechargeSelection(normalizedCredits, normalizedMonths)
  const currentEntitlement = dashboard?.recharge.currentEntitlementCredits
    ?? config?.currentEntitlementCredits
    ?? 0
  const currentMonthFinal = dashboard?.recharge.currentEntitlementMonthlyDelta
    ?? config?.currentEntitlementMonthlyDelta
    ?? currentEntitlement
  const effectiveUntil = dashboard?.recharge.effectiveUntilMonthStart
    ?? config?.effectiveUntilMonthStart
    ?? null
  const currentMonthStart = dashboard?.recharge.currentMonthStart
    ?? config?.currentMonthStart
    ?? currentBrowserMonthStartSeconds()
  const previewMonths = useMemo(() => buildRechargePreviewMonths({
    currentMonthStart,
    currentEntitlement,
    currentMonthFinal,
    effectiveUntil,
    quote,
  }), [currentEntitlement, currentMonthFinal, currentMonthStart, effectiveUntil, quote])
  const amountCents = quote?.finalOrderMoneyCents ?? 0

  const applyCreditsChange = (value: number) => {
    onCreditsChange(value)
    if (config?.testPriceEnabled && value === TEST_RECHARGE_CREDITS) {
      onMonthsChange(TEST_RECHARGE_MONTHS)
    }
  }

  return (
    <section className="surface panel user-console-section user-console-recharge-section">
      <header className="panel-header user-console-section-header user-console-recharge-header">
        <div>
          <h2>{text.title}</h2>
          <p className="panel-description">{text.description}</p>
        </div>
        {config?.enabled ? (
          <StatusBadge tone="success">{text.enabled}</StatusBadge>
        ) : (
          <StatusBadge tone="neutral">{text.disabled}</StatusBadge>
        )}
      </header>

      <div className="user-console-recharge-grid">
        <div className="user-console-recharge-main">
          <div className="user-console-recharge-summary">
            <div>
              <span>{text.currentEntitlement}</span>
              <strong>{formatNumber(currentEntitlement)}</strong>
            </div>
            <div>
              <span>{text.currentMonthFinal}</span>
              <strong>{formatNumber(currentMonthFinal)}</strong>
            </div>
            <div>
              <span>{text.effectiveUntil}</span>
              <strong>{effectiveUntil ? formatTimestamp(effectiveUntil) : text.noEntitlement}</strong>
            </div>
            {quote?.monthEndClampApplied ? <p className="user-console-recharge-test-price">{text.clampNotice}</p> : null}
            {config?.testPriceEnabled ? <p className="user-console-recharge-test-price">{text.testPrice}</p> : null}
          </div>

          {config?.enabled ? (
            <div className="user-console-recharge-form">
              <div className="user-console-recharge-controls">
                <div className="user-console-recharge-field">
                  <span>{text.credits}</span>
                  <div className="user-console-recharge-stepper">
                    <button
                      type="button"
                      className="btn btn-outline btn-sm"
                      onClick={() => applyCreditsChange(nextRechargeCredits(normalizedCredits, -1, stepConfig))}
                      disabled={normalizedCredits <= (config?.testPriceEnabled ? TEST_RECHARGE_CREDITS : minCredits)}
                      aria-label={`Decrease ${text.credits}`}
                    >
                      <Minus size={16} strokeWidth={2.2} aria-hidden="true" />
                    </button>
                    <input className="input input-bordered user-console-recharge-readonly" type="text" readOnly value={formatNumber(normalizedCredits)} aria-label={text.credits} />
                    <button
                      type="button"
                      className="btn btn-outline btn-sm"
                      onClick={() => applyCreditsChange(nextRechargeCredits(normalizedCredits, 1, stepConfig))}
                      disabled={normalizedCredits >= maxCredits}
                      aria-label={`Increase ${text.credits}`}
                    >
                      <Plus size={16} strokeWidth={2.2} aria-hidden="true" />
                    </button>
                  </div>
                </div>
                <div className="user-console-recharge-field">
                  <span>{text.months}</span>
                  <div className="user-console-recharge-stepper">
                    <button
                      type="button"
                      className="btn btn-outline btn-sm"
                      onClick={() => onMonthsChange(normalizeRechargeMonths(normalizedMonths - 1, normalizedCredits, stepConfig))}
                      disabled={isTestOffer || normalizedMonths <= minMonths}
                      aria-label={`Decrease ${text.months}`}
                    >
                      <Minus size={16} strokeWidth={2.2} aria-hidden="true" />
                    </button>
                    <input className="input input-bordered user-console-recharge-readonly" type="text" readOnly value={formatNumber(normalizedMonths)} aria-label={text.months} />
                    <button
                      type="button"
                      className="btn btn-outline btn-sm"
                      onClick={() => onMonthsChange(normalizeRechargeMonths(normalizedMonths + 1, normalizedCredits, stepConfig))}
                      disabled={isTestOffer || normalizedMonths >= maxMonths}
                      aria-label={`Increase ${text.months}`}
                    >
                      <Plus size={16} strokeWidth={2.2} aria-hidden="true" />
                    </button>
                  </div>
                </div>
              </div>

              <div className="user-console-recharge-delta" aria-label={text.quotaDelta}>
                {(quote
                  ? [
                      [text.hourlyDelta, quote.currentMonthFinalHourlyDelta],
                      [text.dailyDelta, quote.currentMonthFinalDailyDelta],
                      [text.monthlyDelta, quote.currentMonthFinalMonthlyDelta],
                    ]
                  : [
                      [text.hourlyDelta, 0],
                      [text.dailyDelta, 0],
                      [text.monthlyDelta, 0],
                    ]).map(([label, value]) => (
                  <div key={label} className="user-console-recharge-delta-pill">
                    <span>{label}</span>
                    <strong>+{formatNumber(Number(value))}</strong>
                  </div>
                ))}
              </div>

              <div className="user-console-recharge-checkout">
                <div className="user-console-recharge-amount">
                  <span>{quote?.monthEndClampApplied ? text.discountedAmount : text.amount}</span>
                  <strong>{formatRechargeMoney(amountCents / 100)} LDC</strong>
                </div>
                {quote?.monthEndClampApplied ? (
                  <p className="user-console-recharge-test-price">{text.discountNotice}</p>
                ) : null}
                <div className="user-console-recharge-actions">
                  <Button type="button" variant="outline" disabled={busy} onClick={() => setPreviewOpen(true)}>
                    <Eye size={16} strokeWidth={2.2} aria-hidden="true" />
                    {text.preview}
                  </Button>
                  <Button type="button" disabled={busy || !quote} aria-busy={busy} onClick={onCreateOrder}>
                    <Icon icon={busy ? 'mdi:loading' : 'mdi:credit-card-outline'} width={16} height={16} aria-hidden="true" />
                    {busy ? text.creating : text.create}
                  </Button>
                </div>
              </div>
              {error ? <p className="user-console-recharge-error" role="status" aria-live="polite">{error}</p> : null}
            </div>
          ) : (
            <p className="empty-state user-console-recharge-disabled">{text.unavailable}</p>
          )}
        </div>

        <div className="user-console-recharge-orders">
          <h3>{text.orders}</h3>
          <div className="user-console-recharge-orders-panel">
            {orders.length === 0 ? (
              <p className="empty-state">{text.noOrders}</p>
            ) : (
              <ul>
                {orders.slice(0, 3).map((order) => (
                  <li key={order.outTradeNo}>
                    <div>
                      <strong>{formatNumber(order.credits)} × {order.months}</strong>
                      <span>{order.money} LDC · {formatTimestamp(order.createdAt)}{order.monthEndClampApplied ? ` · ${text.discountedAmount}` : ''}</span>
                    </div>
                    <StatusBadge tone={rechargeStatusTone(order.status)}>
                      {text.status[order.status] ?? order.status}
                    </StatusBadge>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
      </div>

      {viewportMode === 'small' ? (
        <Drawer open={previewOpen} onOpenChange={setPreviewOpen} shouldScaleBackground={false}>
          <DrawerContent className="user-console-recharge-preview-drawer">
            <DrawerHeader>
              <DrawerTitle>{text.previewTitle}</DrawerTitle>
              <DrawerDescription>{text.previewDescription}</DrawerDescription>
            </DrawerHeader>
            <RechargePreviewBody
              text={text}
              quote={quote}
              credits={normalizedCredits}
              months={normalizedMonths}
              rows={previewMonths}
            />
            <DrawerFooter>
              <DrawerClose asChild>
                <Button type="button" variant="outline">{text.closePreview}</Button>
              </DrawerClose>
            </DrawerFooter>
          </DrawerContent>
        </Drawer>
      ) : (
        <Dialog open={previewOpen} onOpenChange={setPreviewOpen}>
          <DialogContent className="user-console-recharge-preview-modal max-w-3xl">
            <DialogHeader>
              <DialogTitle>{text.previewTitle}</DialogTitle>
              <DialogDescription>{text.previewDescription}</DialogDescription>
            </DialogHeader>
            <RechargePreviewBody
              text={text}
              quote={quote}
              credits={normalizedCredits}
              months={normalizedMonths}
              rows={previewMonths}
            />
          </DialogContent>
        </Dialog>
      )}
    </section>
  )
}

const numberFormatter = new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 })

function formatNumber(value: number): string {
  return numberFormatter.format(value)
}

function currentBrowserMonthStartSeconds(): number {
  const now = new Date()
  return Math.floor(new Date(now.getFullYear(), now.getMonth(), 1).getTime() / 1000)
}

function addMonthsToMonthStart(monthStart: number, offset: number): number {
  const month = new Date(monthStart * 1000)
  return Math.floor(new Date(month.getFullYear(), month.getMonth() + offset, 1).getTime() / 1000)
}

function buildRechargePreviewMonths(input: {
  currentMonthStart: number
  currentEntitlement: number
  currentMonthFinal: number
  effectiveUntil: number | null
  quote: RechargeQuote | null
}): RechargePreviewMonth[] {
  const rows: RechargePreviewMonth[] = []
  const currentEffectEnd = input.effectiveUntil ?? addMonthsToMonthStart(input.currentMonthStart, 1)
  const quote = input.quote
  const scheduleEnd = quote ? addMonthsToMonthStart(quote.quoteMonthStart, quote.requestedMonths - 1) : currentEffectEnd
  const previewEnd = Math.max(currentEffectEnd, scheduleEnd)
  for (let monthStart = input.currentMonthStart; monthStart <= previewEnd; monthStart = addMonthsToMonthStart(monthStart, 1)) {
    const scheduleRow = quote?.schedule.find((item) => item.monthStart === monthStart)
    const currentQuota = monthStart < currentEffectEnd ? input.currentEntitlement : 0
    const delta = scheduleRow?.monthlyDelta ?? 0
    rows.push({
      monthStart,
      currentQuota: monthStart === input.currentMonthStart ? input.currentMonthFinal : currentQuota,
      delta,
      expectedQuota: currentQuota + delta,
      afterExpiry: monthStart > currentEffectEnd,
      clampApplied: scheduleRow?.monthEndClampApplied ?? false,
    })
  }
  return rows
}

function formatMonthLabel(monthStart: number): string {
  try {
    return new Date(monthStart * 1000).toLocaleDateString(undefined, {
      year: 'numeric',
      month: 'long',
    })
  } catch {
    return String(monthStart)
  }
}

function RechargePreviewBody({
  text,
  quote,
  credits,
  months,
  rows,
}: {
  text: RechargePanelText
  quote: RechargeQuote | null
  credits: number
  months: number
  rows: RechargePreviewMonth[]
}): JSX.Element {
  return (
    <div className="user-console-recharge-preview">
      <div className="user-console-recharge-preview-summary">
        <div>
          <span>{text.credits}</span>
          <strong>{formatNumber(credits)}</strong>
        </div>
        <div>
          <span>{text.months}</span>
          <strong>{formatNumber(months)}</strong>
        </div>
        <div>
          <span>{quote?.monthEndClampApplied ? text.discountedAmount : text.amount}</span>
          <strong>{formatRechargeMoney((quote?.finalOrderMoneyCents ?? 0) / 100)} LDC</strong>
        </div>
      </div>
      <p className="user-console-recharge-preview-note">
        {quote?.monthEndClampApplied ? text.discountNotice : text.previewScopeNote}
      </p>

      <div className="user-console-recharge-preview-table" role="table">
        <div className="user-console-recharge-preview-row user-console-recharge-preview-head" role="row">
          <span role="columnheader">{text.previewMonth}</span>
          <span role="columnheader">{text.previewCurrentQuota}</span>
          <span role="columnheader">{text.previewDelta}</span>
          <span role="columnheader">{text.previewExpectedQuota}</span>
        </div>
        {rows.map((row) => (
          <div
            key={row.monthStart}
            className={row.afterExpiry
              ? 'user-console-recharge-preview-row is-after-expiry'
              : 'user-console-recharge-preview-row'}
            role="row"
          >
            <span role="cell">
              {formatMonthLabel(row.monthStart)}
              {row.afterExpiry ? <em>{text.previewAfterExpiry}</em> : null}
              {row.clampApplied ? <em>{text.clampNotice}</em> : null}
            </span>
            <strong role="cell" data-label={text.previewCurrentQuota}>
              <span className="user-console-recharge-preview-cell-label">{text.previewCurrentQuota}</span>
              {formatNumber(row.currentQuota)}
            </strong>
            <strong role="cell" data-label={text.previewDelta}>
              <span className="user-console-recharge-preview-cell-label">{text.previewDelta}</span>
              +{formatNumber(row.delta)}
            </strong>
            <strong role="cell" data-label={text.previewExpectedQuota}>
              <span className="user-console-recharge-preview-cell-label">{text.previewExpectedQuota}</span>
              {formatNumber(row.expectedQuota)}
            </strong>
          </div>
        ))}
      </div>
    </div>
  )
}

function formatTimestamp(ts: number): string {
  try {
    return new Date(ts * 1000).toLocaleString()
  } catch {
    return String(ts)
  }
}

function formatTemplate(template: string, values: Record<string, string | number>): string {
  return Object.entries(values).reduce(
    (current, [key, value]) => current.replaceAll(`{${key}}`, String(value)),
    template,
  )
}
