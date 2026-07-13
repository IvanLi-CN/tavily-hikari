import type { Meta, StoryObj } from '@storybook/react-vite'
import { expect, userEvent, within } from 'storybook/test'

import type {
  RechargeConfig,
  RechargeOrder,
  RechargeQuote,
  UserBillingSummary,
} from '../api'
import { LanguageProvider } from '../i18n'
import { ThemeProvider } from '../theme'
import BillingPage from './BillingPage'
import { EN } from './text'

interface BillingPageStoryProps {
  summary: UserBillingSummary | null
  config: RechargeConfig | null
  orders: RechargeOrder[]
  loading?: boolean
  credits?: number
  months?: number
  quote?: RechargeQuote | null
  busy?: boolean
  error?: string | null
}

function formatMoney(value: number): string {
  return value.toLocaleString('en-US', {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  })
}

const storyNow = Math.floor(Date.now() / 1000)

const billingSummaryWithFuture: UserBillingSummary = {
  currentMonthStart: 1_783_843_200,
  effectiveUntilMonthStart: 1_789_200_000,
  blockAll: false,
  currentTotal: {
    hourly: 110,
    daily: 625,
    monthly: 6250,
  },
  composition: {
    baseAccess: {
      hourly: 20,
      daily: 120,
      monthly: 1200,
    },
    tagAdjustments: {
      hourly: 0,
      daily: 0,
      monthly: 0,
    },
    permanentEntitlements: {
      hourly: 5,
      daily: 25,
      monthly: 250,
    },
    monthlyAdjustments: {
      hourly: 25,
      daily: 180,
      monthly: 1800,
    },
    recharge: {
      credits: 3000,
      quota: {
        hourly: 60,
        daily: 300,
        monthly: 3000,
      },
    },
  },
  timeline: [
    {
      monthStart: 1_781_164_800,
      isCurrentMonth: false,
      persistentTotal: {
        hourly: 25,
        daily: 145,
        monthly: 1450,
      },
      monthlyAdjustments: {
        hourly: 0,
        daily: 0,
        monthly: 0,
      },
      recharge: {
        credits: 0,
        quota: {
          hourly: 0,
          daily: 0,
          monthly: 0,
        },
      },
      effectiveTotal: {
        hourly: 25,
        daily: 145,
        monthly: 1450,
      },
    },
    {
      monthStart: 1_783_843_200,
      isCurrentMonth: true,
      persistentTotal: {
        hourly: 25,
        daily: 145,
        monthly: 1450,
      },
      monthlyAdjustments: {
        hourly: 25,
        daily: 180,
        monthly: 1800,
      },
      recharge: {
        credits: 3000,
        quota: {
          hourly: 60,
          daily: 300,
          monthly: 3000,
        },
      },
      effectiveTotal: {
        hourly: 110,
        daily: 625,
        monthly: 6250,
      },
    },
    {
      monthStart: 1_786_521_600,
      isCurrentMonth: false,
      persistentTotal: {
        hourly: 25,
        daily: 145,
        monthly: 1450,
      },
      monthlyAdjustments: {
        hourly: 0,
        daily: 0,
        monthly: 0,
      },
      recharge: {
        credits: 3000,
        quota: {
          hourly: 60,
          daily: 300,
          monthly: 3000,
        },
      },
      effectiveTotal: {
        hourly: 85,
        daily: 445,
        monthly: 4450,
      },
    },
    {
      monthStart: 1_789_200_000,
      isCurrentMonth: false,
      persistentTotal: {
        hourly: 25,
        daily: 145,
        monthly: 1450,
      },
      monthlyAdjustments: {
        hourly: 0,
        daily: 0,
        monthly: 0,
      },
      recharge: {
        credits: 3000,
        quota: {
          hourly: 60,
          daily: 300,
          monthly: 3000,
        },
      },
      effectiveTotal: {
        hourly: 85,
        daily: 445,
        monthly: 4450,
      },
    },
    {
      monthStart: 1_791_878_400,
      isCurrentMonth: false,
      persistentTotal: {
        hourly: 25,
        daily: 145,
        monthly: 1450,
      },
      monthlyAdjustments: {
        hourly: 0,
        daily: 0,
        monthly: 0,
      },
      recharge: {
        credits: 0,
        quota: {
          hourly: 0,
          daily: 0,
          monthly: 0,
        },
      },
      effectiveTotal: {
        hourly: 25,
        daily: 145,
        monthly: 1450,
      },
    },
  ],
}

const billingSummaryCurrentOnly: UserBillingSummary = {
  ...billingSummaryWithFuture,
  effectiveUntilMonthStart: 1_783_843_200,
  composition: {
    ...billingSummaryWithFuture.composition,
    monthlyAdjustments: {
      hourly: 0,
      daily: 0,
      monthly: 0,
    },
    recharge: {
      credits: 0,
      quota: {
        hourly: 0,
        daily: 0,
        monthly: 0,
      },
    },
  },
  timeline: [
    {
      monthStart: 1_781_164_800,
      isCurrentMonth: false,
      persistentTotal: {
        hourly: 25,
        daily: 145,
        monthly: 1450,
      },
      monthlyAdjustments: {
        hourly: 0,
        daily: 0,
        monthly: 0,
      },
      recharge: {
        credits: 0,
        quota: {
          hourly: 0,
          daily: 0,
          monthly: 0,
        },
      },
      effectiveTotal: {
        hourly: 25,
        daily: 145,
        monthly: 1450,
      },
    },
    {
      monthStart: 1_783_843_200,
      isCurrentMonth: true,
      persistentTotal: {
        hourly: 25,
        daily: 145,
        monthly: 1450,
      },
      monthlyAdjustments: {
        hourly: 0,
        daily: 0,
        monthly: 0,
      },
      recharge: {
        credits: 0,
        quota: {
          hourly: 0,
          daily: 0,
          monthly: 0,
        },
      },
      effectiveTotal: {
        hourly: 25,
        daily: 145,
        monthly: 1450,
      },
    },
    {
      monthStart: 1_786_521_600,
      isCurrentMonth: false,
      persistentTotal: {
        hourly: 25,
        daily: 145,
        monthly: 1450,
      },
      monthlyAdjustments: {
        hourly: 0,
        daily: 0,
        monthly: 0,
      },
      recharge: {
        credits: 0,
        quota: {
          hourly: 0,
          daily: 0,
          monthly: 0,
        },
      },
      effectiveTotal: {
        hourly: 25,
        daily: 145,
        monthly: 1450,
      },
    },
  ],
}

const rechargeConfig: RechargeConfig = {
  visible: true,
  enabled: true,
  unitCredits: 1000,
  unitPriceLdc: 50,
  minCredits: 1000,
  maxCredits: 20000,
  creditsStep: 1000,
  defaultCredits: 3000,
  minMonths: 1,
  maxMonths: 12,
  quotaDeltaBaseCredits: 1000,
  hourlyDeltaPerQuotaUnit: 20,
  dailyDeltaPerQuotaUnit: 100,
  monthlyDeltaPerQuotaUnit: 1000,
  testPriceEnabled: false,
  currentMonthStart: 1_783_843_200,
  currentEntitlementCredits: 3000,
  currentEntitlementHourlyDelta: 60,
  currentEntitlementDailyDelta: 300,
  currentEntitlementMonthlyDelta: 3000,
  effectiveUntilMonthStart: 1_786_521_600,
}

const rechargeQuote: RechargeQuote = {
  requestedCredits: 3000,
  requestedMonths: 2,
  quoteMonthStart: 1_783_843_200,
  remainingDaysInclusive: 17,
  unitCredits: 1000,
  unitPriceCents: 5000,
  fullMonthHourlyDelta: 60,
  fullMonthDailyDelta: 300,
  fullMonthMonthlyDelta: 3000,
  fullMonthMoneyCents: 15000,
  currentMonthFinalHourlyDelta: 60,
  currentMonthFinalDailyDelta: 300,
  currentMonthFinalMonthlyDelta: 3000,
  currentMonthFinalMoneyCents: 15000,
  fullOrderMoneyCents: 30000,
  finalOrderMoneyCents: 30000,
  monthEndClampApplied: false,
  orderName: 'Linux.do Credit 3000 credits',
  schedule: [],
}

const rechargeOrders: RechargeOrder[] = [
  {
    outTradeNo: 'ldc_order_paid_001',
    status: 'paid',
    credits: 3000,
    months: 3,
    money: '450.00',
    quoteMonthStart: 1_783_843_200,
    finalMoneyCents: 45000,
    finalHourlyDelta: 60,
    finalDailyDelta: 300,
    finalMonthlyDelta: 3000,
    monthEndClampApplied: false,
    tradeNo: 'trade-001',
    paymentUrl: null,
    createdAt: storyNow - 86_400 * 3,
    payExpiresAt: storyNow - 86_400 * 3 + 600,
    cancelAfterAt: storyNow - 86_400 * 2,
    cancelledAt: null,
    updatedAt: storyNow - 86_400 * 3 + 900,
    paidAt: storyNow - 86_400 * 3 + 900,
    refundedAt: null,
    refundActor: null,
    lastNotifyAt: storyNow - 86_400 * 3 + 960,
    refundRetryAfterAt: null,
    refundAttempts: 0,
    lastError: null,
  },
  {
    outTradeNo: 'ldc_order_pending_002',
    status: 'pending',
    credits: 1000,
    months: 1,
    money: '50.00',
    quoteMonthStart: 1_783_843_200,
    finalMoneyCents: 5000,
    finalHourlyDelta: 20,
    finalDailyDelta: 100,
    finalMonthlyDelta: 1000,
    monthEndClampApplied: true,
    tradeNo: null,
    paymentUrl: 'https://example.test/pay',
    createdAt: storyNow - 180,
    payExpiresAt: storyNow + 420,
    cancelAfterAt: storyNow + 86_220,
    cancelledAt: null,
    updatedAt: storyNow - 180,
    paidAt: null,
    refundedAt: null,
    refundActor: null,
    lastNotifyAt: null,
    refundRetryAfterAt: null,
    refundAttempts: 0,
    lastError: null,
  },
  {
    outTradeNo: 'ldc_order_expired_003',
    status: 'expired',
    credits: 1000,
    months: 1,
    money: '50.00',
    quoteMonthStart: 1_783_843_200,
    finalMoneyCents: 5000,
    finalHourlyDelta: 20,
    finalDailyDelta: 100,
    finalMonthlyDelta: 1000,
    monthEndClampApplied: false,
    tradeNo: null,
    paymentUrl: null,
    createdAt: storyNow - 3600,
    payExpiresAt: storyNow - 3000,
    cancelAfterAt: storyNow + 82_800,
    cancelledAt: null,
    updatedAt: storyNow - 3000,
    paidAt: null,
    refundedAt: null,
    refundActor: null,
    lastNotifyAt: null,
    refundRetryAfterAt: null,
    refundAttempts: 0,
    lastError: 'payment entry closed locally after 10 minutes',
  },
  {
    outTradeNo: 'ldc_order_cancelled_004',
    status: 'cancelled',
    credits: 1000,
    months: 1,
    money: '50.00',
    quoteMonthStart: 1_783_843_200,
    finalMoneyCents: 5000,
    finalHourlyDelta: 20,
    finalDailyDelta: 100,
    finalMonthlyDelta: 1000,
    monthEndClampApplied: false,
    tradeNo: null,
    paymentUrl: null,
    createdAt: storyNow - 90_000,
    payExpiresAt: storyNow - 89_400,
    cancelAfterAt: storyNow - 3_600,
    cancelledAt: storyNow - 3_540,
    updatedAt: storyNow - 3_540,
    paidAt: null,
    refundedAt: null,
    refundActor: null,
    lastNotifyAt: null,
    refundRetryAfterAt: null,
    refundAttempts: 0,
    lastError: 'order cancelled after 24 hours',
  },
  {
    outTradeNo: 'ldc_order_refunding_005',
    status: 'refunding',
    credits: 1000,
    months: 1,
    money: '30.00',
    quoteMonthStart: 1_783_843_200,
    finalMoneyCents: 3000,
    finalHourlyDelta: 12,
    finalDailyDelta: 60,
    finalMonthlyDelta: 600,
    monthEndClampApplied: true,
    tradeNo: 'trade-005',
    paymentUrl: null,
    createdAt: storyNow - 7200,
    payExpiresAt: storyNow - 6600,
    cancelAfterAt: storyNow + 79_200,
    cancelledAt: null,
    updatedAt: storyNow - 240,
    paidAt: storyNow - 600,
    refundedAt: null,
    refundActor: 'system:auto',
    lastNotifyAt: storyNow - 540,
    refundRetryAfterAt: storyNow + 300,
    refundAttempts: 2,
    lastError: 'paid month no longer matches quote month',
  },
  {
    outTradeNo: 'ldc_order_refunded_006',
    status: 'refunded',
    credits: 1000,
    months: 1,
    money: '30.00',
    quoteMonthStart: 1_783_843_200,
    finalMoneyCents: 3000,
    finalHourlyDelta: 12,
    finalDailyDelta: 60,
    finalMonthlyDelta: 600,
    monthEndClampApplied: true,
    tradeNo: 'trade-006',
    paymentUrl: null,
    createdAt: storyNow - 172_800,
    payExpiresAt: storyNow - 172_200,
    cancelAfterAt: storyNow - 86_400,
    cancelledAt: null,
    updatedAt: storyNow - 86_100,
    paidAt: storyNow - 86_700,
    refundedAt: storyNow - 86_100,
    refundActor: 'system:auto',
    lastNotifyAt: storyNow - 86_640,
    refundRetryAfterAt: null,
    refundAttempts: 1,
    lastError: null,
  },
  ...Array.from({ length: 6 }, (_, index): RechargeOrder => {
    const orderNumber = index + 7
    const credits = (index + 2) * 1000
    const months = index % 2 === 0 ? 1 : 2
    const createdAt = storyNow - 86_400 * (index + 5)
    const paidAt = createdAt + 720
    return {
      outTradeNo: `ldc_order_history_${String(orderNumber).padStart(3, '0')}`,
      status: index % 3 === 0 ? 'paid' : index % 3 === 1 ? 'refundOnly' : 'failed',
      credits,
      months,
      money: formatMoney((credits / 1000) * months * 50),
      quoteMonthStart: 1_781_164_800,
      finalMoneyCents: (credits / 1000) * months * 5000,
      finalHourlyDelta: (credits / 1000) * 20,
      finalDailyDelta: (credits / 1000) * 100,
      finalMonthlyDelta: credits,
      monthEndClampApplied: false,
      tradeNo: index % 3 === 2 ? null : `trade-history-${orderNumber}`,
      paymentUrl: null,
      createdAt,
      payExpiresAt: createdAt + 600,
      cancelAfterAt: createdAt + 86_400,
      cancelledAt: null,
      updatedAt: index % 3 === 2 ? createdAt + 900 : paidAt,
      paidAt: index % 3 === 2 ? null : paidAt,
      refundedAt: index % 3 === 1 ? paidAt + 600 : null,
      refundActor: index % 3 === 1 ? 'builtin-admin' : null,
      lastNotifyAt: index % 3 === 2 ? null : paidAt + 60,
      refundRetryAfterAt: null,
      refundAttempts: 0,
      lastError: index % 3 === 2 ? 'payment failed' : null,
    }
  }),
]

function BillingPageStory({
  summary,
  config,
  orders,
  loading = false,
  credits = 3000,
  months = 2,
  quote = rechargeQuote,
  busy = false,
  error = null,
}: BillingPageStoryProps): JSX.Element {
  return (
    <LanguageProvider>
      <ThemeProvider>
        <div style={{ maxWidth: 1200, margin: '0 auto', padding: 24 }}>
          <BillingPage
            text={EN.billing}
            rechargeText={EN.recharge}
            summary={summary}
            config={config}
            orders={orders}
            loading={loading}
            credits={credits}
            months={months}
            quote={quote}
            busy={busy}
            error={error}
            language="en"
            onCreditsChange={() => undefined}
            onMonthsChange={() => undefined}
            onCreateOrder={() => undefined}
          />
        </div>
      </ThemeProvider>
    </LanguageProvider>
  )
}

const meta = {
  title: 'User Console/Billing/Billing Page',
  component: BillingPageStory,
  parameters: {
    layout: 'fullscreen',
    docs: {
      description: {
        component: 'Dedicated /console/billing route covering entitlement composition, pricing, future natural months, recent orders, and purchase fallback states.',
      },
    },
  },
  args: {
    summary: billingSummaryWithFuture,
    config: rechargeConfig,
    orders: rechargeOrders,
    loading: false,
    credits: 3000,
    months: 2,
    quote: rechargeQuote,
    busy: false,
    error: null,
  },
  argTypes: {
    summary: { control: false },
    config: { control: false },
    orders: { control: false },
    quote: { control: false },
  },
} satisfies Meta<typeof BillingPageStory>

export default meta

type Story = StoryObj<typeof meta>

const mobileViewport = { viewport: { defaultViewport: '0390-device-iphone-14' } } as const

export const Default: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await expect(canvas.getByText('Selected month details')).toBeInTheDocument()
    await expect(canvas.getByText('Natural-month schedule')).toBeInTheDocument()
    await expect(canvas.getByText('Recent orders')).toBeInTheDocument()
    await expect(canvas.getByText('Purchase guide')).toBeInTheDocument()
    await expect(canvas.getByText('One tier')).toBeInTheDocument()
    await expect(canvas.queryByText('{credits} credits = {price} LDC / month')).not.toBeInTheDocument()
    await expect(canvas.queryByText('Buy more quota')).not.toBeInTheDocument()
    await expect(canvas.getByText('50.00 LDC exchanges for 1,000 monthly credits')).toBeInTheDocument()
    await expect(canvas.getByText('1,000 monthly credits')).toBeInTheDocument()
    await expect(canvas.getByText('1 - 12 months')).toBeInTheDocument()
    await expect(canvas.queryByText('Choose a monthly-credit tier, then choose how many natural months it stays active.')).not.toBeInTheDocument()
    await expect(canvas.queryByText('Per tier: +20 requests/hour · +100 credits/day · +1,000 credits/month')).not.toBeInTheDocument()
    await expect(canvas.getByText('+60 requests')).toBeInTheDocument()
    await expect(canvas.getByText('+300 credits')).toBeInTheDocument()
    await expect(canvas.getByText('+3,000 credits')).toBeInTheDocument()
    await expect(canvas.getByRole('button', { name: 'Show details for Jul 2026' })).toHaveAttribute(
      'aria-pressed',
      'true',
    )
    await expect(canvas.getByRole('button', { name: 'Show details for Jun 2026' })).toHaveAttribute(
      'aria-pressed',
      'false',
    )

    await userEvent.click(canvas.getByRole('button', { name: 'Show details for Aug 2026' }))

    const summarySection = canvasElement.querySelector<HTMLElement>('.user-console-billing-summary-section')
    if (summarySection == null) {
      throw new Error('Expected selected month details section to render.')
    }

    const summary = within(summarySection)
    await expect(summary.getByText('Aug 2026')).toBeInTheDocument()
    await expect(summary.getByText('Scheduled month')).toBeInTheDocument()
    await expect(summary.getByText('Base entitlements')).toBeInTheDocument()
    await expect(summary.queryByText('Base access')).not.toBeInTheDocument()
    await expect(summary.queryByText('Long-lived entitlements')).not.toBeInTheDocument()
    await expect(summary.queryByText('Tag adjustments')).not.toBeInTheDocument()
    await expect(summary.getByText('4,450')).toBeInTheDocument()
  },
}

export const LifecycleStates: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    for (const label of ['Pending', 'Closed', 'Cancelled', 'Refunding', 'Refunded']) {
      await expect(canvas.getByText(label)).toBeInTheDocument()
    }
    const workbench = canvasElement.querySelector<HTMLElement>('.user-console-billing-workbench')
    const ordersSection = canvasElement.querySelector<HTMLElement>('.user-console-billing-orders-section')
    if (!workbench || !ordersSection) {
      throw new Error('Expected the billing page workbench and full-width orders section to render.')
    }
    const workbenchWidth = workbench.getBoundingClientRect().width
    const ordersWidth = ordersSection.getBoundingClientRect().width
    if (workbenchWidth > 0 && ordersWidth / workbenchWidth < 0.92) {
      throw new Error('Expected the recent orders section to span the billing page width.')
    }
    await expect(canvas.getByText('Page 1 / 2 · 12 orders')).toBeInTheDocument()
    await userEvent.click(canvas.getByRole('button', { name: /^Next$/ }))
    await expect(canvas.getByText('Page 2 / 2 · 12 orders')).toBeInTheDocument()
    await expect(canvas.getByText('6,000 / 1')).toBeInTheDocument()
    await expect(canvas.getByText('7,000 / 2')).toBeInTheDocument()
    await userEvent.click(canvas.getByRole('button', { name: /^Previous$/ }))
    await expect(canvas.getByText('Page 1 / 2 · 12 orders')).toBeInTheDocument()
  },
}

export const NoOrdersNoFuture: Story = {
  args: {
    summary: billingSummaryCurrentOnly,
    orders: [],
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await expect(canvas.getByText('No recharge orders yet.')).toBeInTheDocument()
    await expect(canvas.getByText('No time-limited entitlements are scheduled beyond the durable baseline right now.')).toBeInTheDocument()
  },
}

export const RechargeDisabled: Story = {
  args: {
    config: {
      ...rechargeConfig,
      enabled: false,
    },
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await expect(canvas.getByText('Unavailable')).toBeInTheDocument()
  },
}

export const Loading: Story = {
  args: {
    summary: null,
    orders: [],
    loading: true,
    quote: null,
  },
}

export const Mobile: Story = {
  parameters: mobileViewport,
}
