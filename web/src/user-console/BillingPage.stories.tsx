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
    createdAt: 1_784_000_000,
    updatedAt: 1_784_000_900,
    paidAt: 1_784_000_900,
    lastNotifyAt: 1_784_000_960,
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
    createdAt: 1_784_005_000,
    updatedAt: 1_784_005_000,
    paidAt: null,
    lastNotifyAt: null,
    lastError: null,
  },
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
