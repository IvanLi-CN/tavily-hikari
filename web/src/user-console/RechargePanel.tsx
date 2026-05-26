import type { RechargeConfig, RechargeOrder } from '../api'
import type { UserDashboard } from '../api'
import { Minus, Plus } from 'lucide-react'
import { Icon } from '../lib/icons'
import { Button } from '../components/ui/button'
import { StatusBadge, type StatusTone } from '../components/StatusBadge'
import { DEFAULT_RECHARGE_UNIT_CREDITS, clampRechargeStep } from './rechargeControls'

const DEFAULT_RECHARGE_MAX_CREDITS = 20_000
const DEFAULT_RECHARGE_MAX_MONTHS = 12

interface RechargePanelText {
  title: string
  description: string
  enabled: string
  disabled: string
  currentEntitlement: string
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
  busy: boolean
  error: string | null
  onCreditsChange: (value: number) => void
  onMonthsChange: (value: number) => void
  onCreateOrder: () => void
}

function formatRechargeMoney(value: number): string {
  return value.toLocaleString('en-US', {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  })
}

function rechargeStatusTone(status: string): StatusTone {
  if (status === 'paid') return 'success'
  if (status === 'failed') return 'error'
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
  busy,
  error,
  onCreditsChange,
  onMonthsChange,
  onCreateOrder,
}: RechargePanelProps): JSX.Element {
  const unitCredits = config?.unitCredits ?? DEFAULT_RECHARGE_UNIT_CREDITS
  const minCredits = config?.minCredits ?? unitCredits
  const maxCredits = config?.maxCredits ?? DEFAULT_RECHARGE_MAX_CREDITS
  const creditsStep = config?.creditsStep ?? unitCredits
  const minMonths = config?.minMonths ?? 1
  const maxMonths = config?.maxMonths ?? DEFAULT_RECHARGE_MAX_MONTHS
  const normalizedCredits = clampRechargeStep(credits, minCredits, maxCredits, creditsStep)
  const normalizedMonths = Math.min(maxMonths, Math.max(minMonths, months))
  const amount = config
    ? (normalizedCredits / config.unitCredits) * normalizedMonths * config.unitPriceLdc
    : 0
  const quotaBaseCredits = config?.quotaDeltaBaseCredits && config.quotaDeltaBaseCredits > 0
    ? config.quotaDeltaBaseCredits
    : DEFAULT_RECHARGE_UNIT_CREDITS
  const quotaDelta = config
    ? {
        hourly: Math.ceil(normalizedCredits * config.hourlyDeltaPerQuotaUnit / quotaBaseCredits),
        daily: Math.ceil(normalizedCredits * config.dailyDeltaPerQuotaUnit / quotaBaseCredits),
        monthly: Math.round(normalizedCredits * config.monthlyDeltaPerQuotaUnit / quotaBaseCredits),
      }
    : { hourly: 0, daily: 0, monthly: normalizedCredits }
  const effectiveUntil = dashboard?.recharge.effectiveUntilMonthStart
    ?? config?.effectiveUntilMonthStart
    ?? null
  const currentEntitlement = dashboard?.recharge.currentEntitlementCredits
    ?? config?.currentEntitlementCredits
    ?? 0

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
              <span>{text.effectiveUntil}</span>
              <strong>{effectiveUntil ? formatTimestamp(effectiveUntil) : text.noEntitlement}</strong>
            </div>
            {config?.testPriceEnabled ? (
              <p className="user-console-recharge-test-price">{text.testPrice}</p>
            ) : null}
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
                      onClick={() => onCreditsChange(clampRechargeStep(normalizedCredits - creditsStep, minCredits, maxCredits, creditsStep))}
                      disabled={normalizedCredits <= minCredits}
                      aria-label={`Decrease ${text.credits}`}
                    >
                      <Minus size={16} strokeWidth={2.2} aria-hidden="true" />
                    </button>
                    <input
                      className="input input-bordered user-console-recharge-readonly"
                      type="text"
                      readOnly
                      value={formatNumber(normalizedCredits)}
                      aria-label={text.credits}
                    />
                    <button
                      type="button"
                      className="btn btn-outline btn-sm"
                      onClick={() => onCreditsChange(clampRechargeStep(normalizedCredits + creditsStep, minCredits, maxCredits, creditsStep))}
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
                      onClick={() => onMonthsChange(Math.max(minMonths, normalizedMonths - 1))}
                      disabled={normalizedMonths <= minMonths}
                      aria-label={`Decrease ${text.months}`}
                    >
                      <Minus size={16} strokeWidth={2.2} aria-hidden="true" />
                    </button>
                    <input
                      className="input input-bordered user-console-recharge-readonly"
                      type="text"
                      readOnly
                      value={formatNumber(normalizedMonths)}
                      aria-label={text.months}
                    />
                    <button
                      type="button"
                      className="btn btn-outline btn-sm"
                      onClick={() => onMonthsChange(Math.min(maxMonths, normalizedMonths + 1))}
                      disabled={normalizedMonths >= maxMonths}
                      aria-label={`Increase ${text.months}`}
                    >
                      <Plus size={16} strokeWidth={2.2} aria-hidden="true" />
                    </button>
                  </div>
                </div>
              </div>
              <div className="user-console-recharge-delta" aria-label={text.quotaDelta}>
                {[
                  [text.hourlyDelta, quotaDelta.hourly],
                  [text.dailyDelta, quotaDelta.daily],
                  [text.monthlyDelta, quotaDelta.monthly],
                ].map(([label, value]) => (
                  <div
                    key={label}
                    className="user-console-recharge-delta-pill"
                  >
                    <span>{label}</span>
                    <strong>+{formatNumber(Number(value))}</strong>
                  </div>
                ))}
              </div>
              <div className="user-console-recharge-checkout">
                <div>
                  <span>{text.amount}</span>
                  <strong>{formatRechargeMoney(amount)} LDC</strong>
                </div>
                <Button type="button" disabled={busy} aria-busy={busy} onClick={onCreateOrder}>
                  <Icon icon={busy ? 'mdi:loading' : 'mdi:credit-card-outline'} width={16} height={16} aria-hidden="true" />
                  {busy ? text.creating : text.create}
                </Button>
              </div>
              {error ? (
                <p className="user-console-recharge-error" role="status" aria-live="polite">{error}</p>
              ) : null}
            </div>
          ) : (
            <p className="empty-state user-console-recharge-disabled">{text.unavailable}</p>
          )}
        </div>

        <div className="user-console-recharge-orders">
          <h3>{text.orders}</h3>
          {orders.length === 0 ? (
            <p className="empty-state">{text.noOrders}</p>
          ) : (
            <ul>
              {orders.slice(0, 3).map((order) => (
                <li key={order.outTradeNo}>
                  <div>
                    <strong>{formatNumber(order.credits)} × {order.months}</strong>
                    <span>{order.money} LDC · {formatTimestamp(order.createdAt)}</span>
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
    </section>
  )
}

const numberFormatter = new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 })

function formatNumber(value: number): string {
  return numberFormatter.format(value)
}

function formatTimestamp(ts: number): string {
  try {
    return new Date(ts * 1000).toLocaleString()
  } catch {
    return String(ts)
  }
}
