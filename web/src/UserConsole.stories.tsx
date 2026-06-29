import { useEffect, useLayoutEffect, useMemo, useState } from 'react'
import type { Meta, StoryObj } from '@storybook/react-vite'

import type {
  Announcement,
  Profile,
  RechargeConfig,
  RechargeOrder,
  RechargeQuote,
  RequestRate,
  RequestRateScope,
  UserDashboard,
  UserDashboardOverview,
  UserTokenSummary,
} from './api'
import UserConsole from './UserConsole'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from './components/ui/dropdown-menu'
import { Icon, getGuideClientIconName } from './lib/icons'
import { userConsoleRouteToPath } from './lib/userConsoleRoutes'

type ConsoleView = 'Console Home' | 'Token Detail'
type LandingFocus = 'Overview Focus' | 'Token Focus'
type TokenListState = 'Single Token' | 'Multiple Tokens' | 'Empty'
type TokenDetailPreview = 'Overview' | 'Token Revealed'
type PushStatusPreview = 'Live' | 'Reconnecting' | 'Unsupported'
type AnnouncementPreview = 'Active' | 'Ticker Bodyless' | 'Closed' | 'History Open' | 'None'
type RechargePreview = 'normal' | 'test-price' | 'disabled' | 'hidden'
type RechargeQuotePreview = 'normal' | 'month-end-clamp'

type CopyRecoveryMode = 'none' | 'list-manual-bubble' | 'detail-inline'
type GuideRevealMode = 'none' | 'landing-guide' | 'detail-guide'

interface UserConsoleStoryArgs {
  consoleView: ConsoleView
  isAdmin: boolean
  landingFocus: LandingFocus
  tokenListState: TokenListState
  tokenDetailPreview: TokenDetailPreview
  routePathOverride?: string
  pushStatusPreview?: PushStatusPreview
  pushStatusBubbleOpen?: boolean
  autoOpenAccountMenu?: boolean
  announcementPreview?: AnnouncementPreview
  rechargePreview?: RechargePreview
  rechargeQuotePreview?: RechargeQuotePreview
}

interface UserConsoleStoryState {
  autoRevealToken: boolean
  isAdmin: boolean
  rechargePreview: RechargePreview
  rechargeQuotePreview: RechargeQuotePreview
  routePath: string
  tokenListMode: 'single' | 'multiple' | 'empty'
  announcementPreview: AnnouncementPreview
}

type MockEventSourceShape = EventSource & {
  dispatchEvent: (event: Event) => boolean
}

const TOKEN_DETAIL_PATH = '/console/tokens/a1b2'
const guideProofLabels = [
  { id: 'codex', label: 'Codex CLI' },
  { id: 'claude', label: 'Claude Code' },
  { id: 'vscode', label: 'VS Code' },
] as const
const removedTokenQuotaLabels = ['配额窗口', 'Quota Windows', '任意请求(1h)', 'Any Req (1h)']
const removedTokenQuotaLabelsMobile = [...removedTokenQuotaLabels, '小时', '日', '月']
const removedTokenInternalLabels = ['primary', 'backup', '备注', 'Note']

function createRequestRate(
  used: number,
  limit: number,
  scope: RequestRateScope,
  windowMinutes = 5,
): RequestRate {
  return {
    used,
    limit,
    windowMinutes,
    scope,
  }
}

function withUserQuotaCanonical<
  T extends {
    requestRate: RequestRate
    businessCalls1h: UserDashboard['businessCalls1h']
    dailyCreditsUsed: number
    dailyCreditsLimit: number
    monthlyCreditsUsed: number
    monthlyCreditsLimit: number
  },
>(value: T): T & {
  hourlyAnyUsed: number
  hourlyAnyLimit: number
  quotaHourlyUsed: number
  quotaHourlyLimit: number
  quotaDailyUsed: number
  quotaDailyLimit: number
  quotaMonthlyUsed: number
  quotaMonthlyLimit: number
} {
  return {
    ...value,
    hourlyAnyUsed: value.requestRate.used,
    hourlyAnyLimit: value.requestRate.limit,
    quotaHourlyUsed: value.businessCalls1h.totalCount,
    quotaHourlyLimit: value.businessCalls1h.limit,
    quotaDailyUsed: value.dailyCreditsUsed,
    quotaDailyLimit: value.dailyCreditsLimit,
    quotaMonthlyUsed: value.monthlyCreditsUsed,
    quotaMonthlyLimit: value.monthlyCreditsLimit,
  }
}

function withOverviewProgressCanonical<
  T extends {
    requestRate: UserDashboardOverview['progress']['requestRate']
    businessCalls1h: UserDashboardOverview['progress']['businessCalls1h']
    dailyCredits: UserDashboardOverview['progress']['dailyCredits']
    monthlyCredits: UserDashboardOverview['progress']['monthlyCredits']
  },
>(value: T): T {
  return value
}

async function expectTokenListProof(
  canvasElement: HTMLElement,
  selector: string,
  removed: string[],
  expectedGroups: string[][],
  errorPrefix: string,
): Promise<void> {
  await new Promise((resolve) => window.setTimeout(resolve, 140))

  const container = canvasElement.querySelector(selector)
  if (container == null) {
    throw new Error(`Expected ${errorPrefix} to render ${selector}.`)
  }

  const proofText = container.textContent ?? ''
  for (const removedLabel of removed) {
    if (proofText.includes(removedLabel)) {
      throw new Error(`Expected ${errorPrefix} to remove ${removedLabel} from the token list.`)
    }
  }

  for (const expectedGroup of expectedGroups) {
    if (!expectedGroup.some((expected) => proofText.includes(expected))) {
      throw new Error(`Expected ${errorPrefix} to keep one of ${expectedGroup.join(' / ')} visible.`)
    }
  }
}

const mobileViewport = { viewport: { defaultViewport: '0390-device-iphone-14' } } as const

const consoleHomeOverviewArgs: UserConsoleStoryArgs = {
  consoleView: 'Console Home',
  isAdmin: false,
  landingFocus: 'Overview Focus',
  tokenListState: 'Single Token',
  tokenDetailPreview: 'Overview',
}

const consoleHomeAdminOverviewArgs: UserConsoleStoryArgs = {
  ...consoleHomeOverviewArgs,
  isAdmin: true,
}

const consoleHomeTokenFocusArgs: UserConsoleStoryArgs = {
  ...consoleHomeOverviewArgs,
  landingFocus: 'Token Focus',
}

const consoleHomeAdminTokenFocusArgs: UserConsoleStoryArgs = {
  ...consoleHomeTokenFocusArgs,
  isAdmin: true,
}

const tokenDetailOverviewArgs: UserConsoleStoryArgs = {
  ...consoleHomeOverviewArgs,
  consoleView: 'Token Detail',
}

const tokenDetailAdminOverviewArgs: UserConsoleStoryArgs = {
  ...tokenDetailOverviewArgs,
  isAdmin: true,
}

const dashboardSample: UserDashboard = withUserQuotaCanonical({
  debugInfoShared: false,
  requestRate: createRequestRate(58, 60, 'user'),
  businessCalls1h: { successCount: 73, failureCount: 9, totalCount: 82, limit: 100, windowMinutes: 60 },
  dailyCreditsUsed: 356,
  dailyCreditsLimit: 500,
  monthlyCreditsUsed: 4120,
  monthlyCreditsLimit: 5000,
  dailySuccess: 301,
  dailyFailure: 17,
  monthlySuccess: 3478,
  lastActivity: 1_762_386_800,
  recharge: {
    currentMonthStart: 1_762_041_600,
    currentEntitlementCredits: 3000,
    currentEntitlementHourlyDelta: 60,
    currentEntitlementDailyDelta: 300,
    currentEntitlementMonthlyDelta: 3000,
    effectiveUntilMonthStart: 1_767_225_600,
  },
})

function createOverviewPoints(values: Array<number | null>, limit: number) {
  return values.map((value, index) => ({
    bucketStart: 1_762_041_600 + index * 300,
    displayBucketStart: null,
    value,
    limitValue: limit,
  }))
}

const dashboardOverviewSample: UserDashboardOverview = {
  summary: dashboardSample,
  progress: withOverviewProgressCanonical({
    requestRate: {
      used: dashboardSample.requestRate.used,
      limit: dashboardSample.requestRate.limit,
      points: createOverviewPoints([8, 10, 9, 15, 14, 16, 21, 23, 29, 35, 42, 58], dashboardSample.requestRate.limit),
    },
    businessCalls1h: {
      used: dashboardSample.businessCalls1h.totalCount,
      limit: dashboardSample.businessCalls1h.limit,
      points: createOverviewPoints([7, 12, 18, 24, 31, 40, 52, 63, 72, 82, null, null], dashboardSample.businessCalls1h.limit),
    },
    dailyCredits: {
      used: dashboardSample.dailyCreditsUsed,
      limit: dashboardSample.dailyCreditsLimit,
      points: createOverviewPoints(
        [11, 19, 28, 36, 49, 63, 78, 92, 108, 126, 145, 169, 194, 228, 264, 302, 356, null, null, null, null, null, null, null],
        dashboardSample.dailyCreditsLimit,
      ),
    },
    monthlyCredits: {
      used: dashboardSample.monthlyCreditsUsed,
      limit: dashboardSample.monthlyCreditsLimit,
      points: createOverviewPoints(
        [130, 248, 364, 508, 672, 821, 983, 1_156, 1_344, 1_525, 1_711, 1_904, 2_118, 2_347, 2_589, 2_846, 3_124, 3_411, 3_762, 4_120, null, null, null, null, null, null, null, null, null, null],
        dashboardSample.monthlyCreditsLimit,
      ),
    },
  }),
}

const rechargeConfigSample: RechargeConfig = {
  visible: true,
  enabled: true,
  unitCredits: 1000,
  unitPriceLdc: 50,
  minCredits: 1000,
  maxCredits: 20_000,
  creditsStep: 1000,
  defaultCredits: 1000,
  minMonths: 1,
  maxMonths: 12,
  quotaDeltaBaseCredits: 1000,
  hourlyDeltaPerQuotaUnit: 20,
  dailyDeltaPerQuotaUnit: 100,
  monthlyDeltaPerQuotaUnit: 1000,
  testPriceEnabled: false,
  currentMonthStart: 1_762_041_600,
  currentEntitlementCredits: 3000,
  currentEntitlementHourlyDelta: 60,
  currentEntitlementDailyDelta: 300,
  currentEntitlementMonthlyDelta: 3000,
  effectiveUntilMonthStart: 1_767_225_600,
}

const rechargeTestPriceConfigSample: RechargeConfig = {
  ...rechargeConfigSample,
  defaultCredits: 1,
  testPriceEnabled: true,
  currentEntitlementCredits: 1,
}

const rechargeDisabledConfigSample: RechargeConfig = {
  ...rechargeConfigSample,
  enabled: false,
}

const rechargeHiddenConfigSample: RechargeConfig = {
  ...rechargeConfigSample,
  visible: false,
  enabled: false,
}

const rechargeOrdersSample: RechargeOrder[] = [
  {
    outTradeNo: 'ldc_story_paid',
    status: 'paid',
    credits: 3000,
    months: 2,
    money: '300.00',
    quoteMonthStart: 1_762_041_600,
    finalMoneyCents: 30_000,
    finalHourlyDelta: 60,
    finalDailyDelta: 300,
    finalMonthlyDelta: 3000,
    monthEndClampApplied: false,
    tradeNo: 'linuxdo-story-001',
    paymentUrl: 'https://credit.linux.do/story-paid',
    createdAt: 1_762_214_400,
    updatedAt: 1_762_214_520,
    paidAt: 1_762_214_520,
    lastNotifyAt: 1_762_214_520,
    lastError: null,
  },
  {
    outTradeNo: 'ldc_story_pending',
    status: 'pending',
    credits: 1000,
    months: 1,
    money: '50.00',
    quoteMonthStart: 1_762_386_000,
    finalMoneyCents: 5000,
    finalHourlyDelta: 20,
    finalDailyDelta: 100,
    finalMonthlyDelta: 1000,
    monthEndClampApplied: false,
    tradeNo: null,
    paymentUrl: 'https://credit.linux.do/story-pending',
    createdAt: 1_762_386_200,
    updatedAt: 1_762_386_200,
    paidAt: null,
    lastNotifyAt: null,
    lastError: null,
  },
  {
    outTradeNo: 'ldc_story_expired',
    status: 'expired',
    credits: 1000,
    months: 1,
    money: '30.00',
    quoteMonthStart: 1_762_041_600,
    finalMoneyCents: 3000,
    finalHourlyDelta: 12,
    finalDailyDelta: 60,
    finalMonthlyDelta: 600,
    monthEndClampApplied: true,
    tradeNo: null,
    paymentUrl: null,
    createdAt: 1_762_386_000,
    updatedAt: 1_762_389_000,
    paidAt: null,
    lastNotifyAt: null,
    lastError: 'expired when month changed',
  },
]

const rechargeQuoteSample: RechargeQuote = {
  requestedCredits: 1000,
  requestedMonths: 1,
  quoteMonthStart: 1_762_041_600,
  remainingDaysInclusive: 30,
  unitCredits: 1000,
  unitPriceCents: 5000,
  fullMonthHourlyDelta: 20,
  fullMonthDailyDelta: 100,
  fullMonthMonthlyDelta: 1000,
  fullMonthMoneyCents: 5000,
  currentMonthFinalHourlyDelta: 20,
  currentMonthFinalDailyDelta: 100,
  currentMonthFinalMonthlyDelta: 1000,
  currentMonthFinalMoneyCents: 5000,
  fullOrderMoneyCents: 5000,
  finalOrderMoneyCents: 5000,
  monthEndClampApplied: false,
  orderName: 'Linux.do Credit recharge',
  schedule: [
    {
      monthIndex: 0,
      monthStart: 1_762_041_600,
      isCurrentMonth: true,
      hourlyDelta: 20,
      dailyDelta: 100,
      monthlyDelta: 1000,
      fullMonthlyDelta: 1000,
      monthMoneyCents: 5000,
      monthDiscountCents: 0,
      monthEndClampApplied: false,
      discountReason: null,
    },
  ],
}

const rechargeClampQuoteSample: RechargeQuote = {
  requestedCredits: 1000,
  requestedMonths: 1,
  quoteMonthStart: 1_762_041_600,
  remainingDaysInclusive: 3,
  unitCredits: 1000,
  unitPriceCents: 5000,
  fullMonthHourlyDelta: 20,
  fullMonthDailyDelta: 100,
  fullMonthMonthlyDelta: 1000,
  fullMonthMoneyCents: 5000,
  currentMonthFinalHourlyDelta: 12,
  currentMonthFinalDailyDelta: 60,
  currentMonthFinalMonthlyDelta: 600,
  currentMonthFinalMoneyCents: 3000,
  fullOrderMoneyCents: 5000,
  finalOrderMoneyCents: 3000,
  monthEndClampApplied: true,
  orderName: 'Linux.do Credit recharge',
  schedule: [
    {
      monthIndex: 0,
      monthStart: 1_762_041_600,
      isCurrentMonth: true,
      hourlyDelta: 12,
      dailyDelta: 60,
      monthlyDelta: 600,
      fullMonthlyDelta: 1000,
      monthMoneyCents: 3000,
      monthDiscountCents: 2000,
      monthEndClampApplied: true,
      discountReason: 'remaining_days_inclusive',
    },
  ],
}

const tokenSample: UserTokenSummary = withUserQuotaCanonical({
  tokenId: 'a1b2',
  enabled: true,
  note: 'primary',
  lastUsedAt: 1_762_386_800,
  requestRate: createRequestRate(58, 60, 'user'),
  businessCalls1h: { successCount: 73, failureCount: 9, totalCount: 82, limit: 100, windowMinutes: 60 },
  dailyCreditsUsed: 356,
  dailyCreditsLimit: 500,
  monthlyCreditsUsed: 4120,
  monthlyCreditsLimit: 5000,
  dailySuccess: 301,
  dailyFailure: 17,
  monthlySuccess: 3478,
})

const tokenSecondarySample: UserTokenSummary = withUserQuotaCanonical({
  tokenId: 'c3d4',
  enabled: true,
  note: 'backup',
  lastUsedAt: 1_762_386_100,
  requestRate: createRequestRate(58, 60, 'user'),
  businessCalls1h: { successCount: 10, failureCount: 2, totalCount: 12, limit: 100, windowMinutes: 60 },
  dailyCreditsUsed: 84,
  dailyCreditsLimit: 500,
  monthlyCreditsUsed: 933,
  monthlyCreditsLimit: 5000,
  dailySuccess: 76,
  dailyFailure: 4,
  monthlySuccess: 827,
})

const tokenDetailSample: UserTokenSummary = withUserQuotaCanonical({
  ...tokenSample,
  requestRate: createRequestRate(58, 60, 'user'),
  businessCalls1h: { successCount: 78, failureCount: 10, totalCount: 88, limit: 100, windowMinutes: 60 },
  dailyCreditsUsed: 371,
  dailyCreditsLimit: 500,
  monthlyCreditsUsed: 4188,
  monthlyCreditsLimit: 5000,
  dailySuccess: 315,
  dailyFailure: 19,
  monthlySuccess: 3510,
})

interface ServerPublicTokenLogMock {
  id: number
  method: string
  path: string
  query: string | null
  httpStatus: number | null
  mcpStatus: number | null
  businessCredits: number | null
  countsBusinessQuota: boolean
  resultStatus: string
  errorMessage: string | null
  createdAt: number
}

const tokenLogTemplates: Array<Omit<ServerPublicTokenLogMock, 'id' | 'createdAt'>> = [
  {
    method: 'POST',
    path: '/api/tavily/search',
    query: 'q=rust',
    httpStatus: 200,
    mcpStatus: 200,
    businessCredits: 2,
    countsBusinessQuota: true,
    resultStatus: 'success',
    errorMessage: null,
  },
  {
    method: 'POST',
    path: '/mcp',
    query: null,
    httpStatus: 429,
    mcpStatus: 429,
    businessCredits: null,
    countsBusinessQuota: true,
    resultStatus: 'quota_exhausted',
    errorMessage: 'Account hourly limit reached',
  },
  {
    method: 'POST',
    path: '/mcp',
    query: null,
    httpStatus: 200,
    mcpStatus: 200,
    businessCredits: null,
    countsBusinessQuota: false,
    resultStatus: 'neutral',
    errorMessage: null,
  },
  {
    method: 'POST',
    path: '/api/tavily/extract',
    query: null,
    httpStatus: 500,
    mcpStatus: 500,
    businessCredits: null,
    countsBusinessQuota: true,
    resultStatus: 'error',
    errorMessage: 'upstream timeout',
  },
  {
    method: 'GET',
    path: '/api/tavily/usage',
    query: null,
    httpStatus: 200,
    mcpStatus: null,
    businessCredits: null,
    countsBusinessQuota: false,
    resultStatus: 'success',
    errorMessage: null,
  },
]

const tokenLogsSample: ServerPublicTokenLogMock[] = Array.from({ length: 50 }, (_, index) => {
  const template = tokenLogTemplates[index % tokenLogTemplates.length]
  return {
    ...template,
    id: 150 - index,
    query: template.query ? `${template.query}&sample=${index + 1}` : null,
    createdAt: 1_762_386_640 - index * 37,
  }
})

const announcementModalSample: Announcement = {
  id: 'ann-modal-01',
  title: 'Maintenance window',
  body: '**Tavily Hikari will restart tonight** between 23:00 and 23:10.\n\n- Existing MCP sessions may reconnect once.\n- API requests should retry normally.',
  displayKind: 'modal',
  status: 'published',
  createdAt: 1_762_380_000,
  updatedAt: 1_762_386_000,
  publishedAt: 1_762_386_000,
  archivedAt: null,
}

const announcementTickerSample: Announcement = {
  id: 'ann-ticker-01',
  title: 'Quota refresh',
  body: 'Daily quota counters have refreshed. Token detail pages now include `live request` updates.',
  displayKind: 'ticker',
  status: 'published',
  createdAt: 1_762_378_000,
  updatedAt: 1_762_385_000,
  publishedAt: 1_762_385_000,
  archivedAt: null,
}

const announcementBodylessTickerSample: Announcement = {
  ...announcementTickerSample,
  id: 'ann-ticker-empty-01',
  title: 'Quota refresh complete',
  body: '',
}

const announcementArchivedSample: Announcement = {
  id: 'ann-archived-01',
  title: 'Endpoint migration completed',
  body: 'The previous Tavily-compatible endpoint migration has been completed. See [migration notes](https://example.com).',
  displayKind: 'ticker',
  status: 'archived',
  createdAt: 1_762_200_000,
  updatedAt: 1_762_250_000,
  publishedAt: 1_762_210_000,
  archivedAt: 1_762_250_000,
}

const storyAvatarDataUrl =
  'data:image/svg+xml;utf8,' +
  encodeURIComponent(
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
      <defs>
        <linearGradient id="g" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stop-color="#3b82f6" />
          <stop offset="100%" stop-color="#1d4ed8" />
        </linearGradient>
      </defs>
      <rect width="64" height="64" rx="32" fill="url(#g)" />
      <circle cx="32" cy="25" r="13" fill="#dbeafe" />
      <path d="M14 56c2-10 9.7-16 18-16s16 6 18 16" fill="#dbeafe" />
    </svg>`,
  )

const profileSample: Profile = {
  displayName: 'Ivan',
  isAdmin: false,
  forwardAuthEnabled: true,
  builtinAuthEnabled: true,
  allowRegistration: true,
  userLoggedIn: true,
  userProvider: 'linuxdo',
  userDisplayName: 'Ivan',
  userAvatarUrl: storyAvatarDataUrl,
}

const adminProfileSample: Profile = {
  ...profileSample,
  isAdmin: true,
}

const versionSample = {
  backend: '0.2.0-dev',
  frontend: '0.2.0-dev',
}

const activeEventSources = new Set<MockEventSourceShape>()

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

function routePathFromView(view: ConsoleView, landingFocus: LandingFocus, routePathOverride?: string): string {
  if (typeof routePathOverride === 'string') return routePathOverride
  if (view === 'Token Detail') return TOKEN_DETAIL_PATH
  return userConsoleRouteToPath({
    name: 'landing',
    section: landingFocus === 'Token Focus' ? 'tokens' : 'dashboard',
  })
}

function resolveStoryState(args: UserConsoleStoryArgs): UserConsoleStoryState {
  const tokenListMode = args.consoleView !== 'Console Home'
    ? 'single'
    : args.tokenListState === 'Empty'
      ? 'empty'
      : args.tokenListState === 'Multiple Tokens'
        ? 'multiple'
        : 'single'

  return {
    autoRevealToken: args.consoleView === 'Token Detail' && args.tokenDetailPreview === 'Token Revealed',
    isAdmin: args.isAdmin,
    rechargePreview: args.rechargePreview ?? 'normal',
    rechargeQuotePreview: args.rechargeQuotePreview ?? 'normal',
    routePath: routePathFromView(args.consoleView, args.landingFocus, args.routePathOverride),
    tokenListMode,
    announcementPreview: args.announcementPreview ?? 'Active',
  }
}

function UserConsoleMobileGuideMenuProof(): JSX.Element {
  const active = guideProofLabels[0]

  return (
    <div
      style={{
        display: 'grid',
        gap: 20,
        maxWidth: 420,
        margin: '0 auto',
      }}
    >
      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>Mobile guide menu proof</h2>
            <p className="panel-description">
              The console guide dropdown uses the shared portal layer and must not clip inside the mobile token card.
            </p>
          </div>
        </div>
        <div
          style={{
            overflow: 'hidden',
            borderRadius: 28,
            border: '1px dashed hsl(var(--accent) / 0.42)',
            background: 'linear-gradient(180deg, hsl(var(--card) / 0.98), hsl(var(--muted) / 0.3))',
            padding: 18,
          }}
        >
          <div style={{ minHeight: 120 }}>
            <DropdownMenu open>
              <DropdownMenuTrigger asChild>
                <button type="button" className="btn btn-outline w-full justify-between btn-sm md:btn-md">
                  <span className="inline-flex items-center gap-2">
                    <Icon
                      icon={getGuideClientIconName(active.id)}
                      width={18}
                      height={18}
                      aria-hidden="true"
                      style={{ color: '#475569' }}
                    />
                    {active.label}
                  </span>
                  <Icon
                    icon="mdi:chevron-down"
                    width={16}
                    height={16}
                    aria-hidden="true"
                    style={{ color: '#647589' }}
                  />
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="guide-select-menu p-1">
                {guideProofLabels.map((tab) => (
                  <DropdownMenuItem
                    key={tab.id}
                    className={`flex items-center gap-2 ${tab.id === active.id ? 'bg-accent/45 text-accent-foreground' : ''}`}
                  >
                    <Icon
                      icon={getGuideClientIconName(tab.id)}
                      width={16}
                      height={16}
                      aria-hidden="true"
                      style={{ color: '#475569' }}
                    />
                    <span className="truncate">{tab.label}</span>
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </div>
      </section>
    </div>
  )
}

export const __testables = {
  resolveStoryState,
}

function installUserConsoleFetchMock(state: UserConsoleStoryState): () => void {
  const originalFetch = window.fetch.bind(window)
  const researchRequestId = 'rq-story-001'
  const tokenList = state.tokenListMode === 'empty'
    ? []
    : state.tokenListMode === 'multiple'
      ? [tokenSample, tokenSecondarySample]
      : [tokenSample]

  window.fetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const request = input instanceof Request
      ? input
      : new Request(input, init)
    const url = new URL(request.url, window.location.origin)

    if (url.pathname === '/api/profile') {
      return jsonResponse(state.isAdmin ? adminProfileSample : profileSample)
    }

    if (url.pathname === '/api/user/dashboard') {
      return jsonResponse(dashboardSample)
    }

    if (url.pathname === '/api/user/dashboard/overview') {
      return jsonResponse(dashboardOverviewSample)
    }

    if (url.pathname === '/api/user/recharge/config') {
      switch (state.rechargePreview) {
        case 'test-price':
          return jsonResponse(rechargeTestPriceConfigSample)
        case 'disabled':
          return jsonResponse(rechargeDisabledConfigSample)
        case 'hidden':
          return jsonResponse(rechargeHiddenConfigSample)
        default:
          return jsonResponse(rechargeConfigSample)
      }
    }

    if (url.pathname === '/api/user/recharge/quote') {
      return jsonResponse(state.rechargeQuotePreview === 'month-end-clamp' ? rechargeClampQuoteSample : rechargeQuoteSample)
    }

    if (url.pathname === '/api/user/recharge/orders') {
      if (request.method === 'POST') {
        const body = await request.clone().json().catch(() => ({}))
        if (!body.quote) {
          return jsonResponse({ message: 'Missing quote' }, 400)
        }
        return jsonResponse({
          order: rechargeOrdersSample[1],
          paymentUrl: 'https://credit.linux.do/story-checkout',
        })
      }
      return jsonResponse({ items: rechargeOrdersSample })
    }

    const rechargeOrderRoute = url.pathname.match(/^\/api\/user\/recharge\/orders\/([^/]+)$/)
    if (rechargeOrderRoute) {
      const outTradeNo = decodeURIComponent(rechargeOrderRoute[1])
      const order = rechargeOrdersSample.find((item) => item.outTradeNo === outTradeNo)
      return order ? jsonResponse(order) : jsonResponse({ message: 'Not Found' }, 404)
    }

    if (url.pathname === '/api/version') {
      return jsonResponse(versionSample)
    }

    if (url.pathname === '/api/user/logout') {
      return new Response(null, { status: 204 })
    }

    if (url.pathname === '/api/user/tokens') {
      return jsonResponse(tokenList)
    }

    if (url.pathname === '/api/user/announcements') {
      const activeAnnouncements = state.announcementPreview === 'Ticker Bodyless'
        ? [announcementBodylessTickerSample]
        : [announcementModalSample, announcementTickerSample]
      return jsonResponse({
        items: state.announcementPreview === 'None'
          ? []
          : activeAnnouncements,
      })
    }

    if (url.pathname === '/api/user/announcements/history') {
      const historyAnnouncements = state.announcementPreview === 'Ticker Bodyless'
        ? [announcementBodylessTickerSample, announcementArchivedSample]
        : [announcementModalSample, announcementTickerSample, announcementArchivedSample]
      return jsonResponse({
        items: state.announcementPreview === 'None'
          ? []
          : historyAnnouncements,
      })
    }

    const tokenRoute = url.pathname.match(/^\/api\/user\/tokens\/([^/]+)(?:\/(secret|logs)(?:\/rotate)?)?$/)
    if (tokenRoute) {
      const tokenId = decodeURIComponent(tokenRoute[1])
      const action = tokenRoute[2] ?? 'detail'

      if (tokenId !== tokenSample.tokenId) {
        return jsonResponse({ message: 'Not Found' }, 404)
      }

      if (action === 'secret') {
        if (request.method === 'POST' && url.pathname.endsWith('/secret/rotate')) {
          return jsonResponse({ token: 'th-a1b2-reset1234567890abcdef' })
        }
        return jsonResponse({ token: 'th-a1b2-1234567890abcdef' })
      }

      if (action === 'logs') {
        const limit = Number.parseInt(url.searchParams.get('limit') ?? '50', 10)
        const billing = url.searchParams.get('billing') ?? 'all'
        const source = billing === 'billable'
          ? tokenLogsSample.filter((log) => log.countsBusinessQuota)
          : tokenLogsSample
        return jsonResponse(source.slice(0, Number.isFinite(limit) ? limit : 50))
      }

      return jsonResponse(tokenDetailSample)
    }

    if (url.pathname === '/mcp') {
      const payload = await request.clone().json().catch(() => ({}))
      const method = typeof payload?.method === 'string' ? payload.method : ''
      const accept = request.headers.get('Accept') ?? ''
      const acceptsProbeFormats = accept.includes('application/json') && accept.includes('text/event-stream')

      if (method === 'tools/list' && !acceptsProbeFormats) {
        return jsonResponse({
          jsonrpc: '2.0',
          id: 'server-error',
          error: {
            code: -32600,
            message: 'Not Acceptable: Client must accept both application/json and text/event-stream',
          },
        }, 406)
      }

      if (method === 'tools/list') {
        return new Response(
          `event: message\ndata: ${JSON.stringify({
            jsonrpc: '2.0',
            id: payload?.id ?? null,
            result: {
              tools: [
                { name: 'tavily-search' },
                { name: 'tavily-extract' },
                { name: 'tavily-crawl' },
                { name: 'tavily-map' },
                { name: 'tavily-research' },
              ],
            },
          })}\n\n`,
          {
            status: 200,
            headers: { 'Content-Type': 'text/event-stream' },
          },
        )
      }

      if (method === 'tools/call') {
        return jsonResponse({
          jsonrpc: '2.0',
          id: payload?.id ?? null,
          result: {
            ok: true,
            tool: payload?.params?.name ?? null,
          },
        })
      }

      return jsonResponse({
        jsonrpc: '2.0',
        id: payload?.id ?? null,
        result: {
          ok: true,
          method,
        },
      })
    }

    if (url.pathname.startsWith('/api/tavily/')) {
      if (url.pathname === '/api/tavily/search') {
        return jsonResponse({ status: 200, results: [] })
      }
      if (url.pathname === '/api/tavily/extract') {
        return jsonResponse({ status: 200, results: [] })
      }
      if (url.pathname === '/api/tavily/crawl') {
        return jsonResponse({ status: 200, results: [] })
      }
      if (url.pathname === '/api/tavily/map') {
        return jsonResponse({ status: 200, results: [] })
      }
      if (url.pathname === '/api/tavily/research') {
        return jsonResponse({
          request_id: researchRequestId,
          status: 'pending',
        })
      }
      if (url.pathname === `/api/tavily/research/${researchRequestId}`) {
        return jsonResponse({
          request_id: researchRequestId,
          status: 'pending',
        })
      }
    }

    return originalFetch(input, init)
  }

  return () => {
    window.fetch = originalFetch
  }
}

function installClipboardFailureMock(): () => void {
  const originalClipboardDescriptor = Object.getOwnPropertyDescriptor(navigator, 'clipboard')
  const originalExecCommand = document.execCommand
  let clipboardMockInstalled = false

  try {
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: {
        writeText: async () => {
          throw new Error('storybook-copy-blocked')
        },
      },
    })
    clipboardMockInstalled = true
  } catch {
    // Ignore if the browser refuses to override clipboard in the mock canvas.
  }

  try {
    document.execCommand = (() => false) as typeof document.execCommand
  } catch {
    // Ignore if execCommand cannot be replaced in the current runtime.
  }

  return () => {
    try {
      if (originalClipboardDescriptor) {
        Object.defineProperty(navigator, 'clipboard', originalClipboardDescriptor)
      } else if (clipboardMockInstalled) {
        Reflect.deleteProperty(navigator, 'clipboard')
      }
    } catch {
      // Ignore restore failures inside Storybook.
    }

    try {
      document.execCommand = originalExecCommand
    } catch {
      // Ignore restore failures inside Storybook.
    }
  }
}

function installEventSourceMock(mode: PushStatusPreview): () => void {
  const OriginalEventSource = window.EventSource

  if (mode === 'Unsupported') {
    ;(window as Window & { EventSource?: typeof EventSource }).EventSource = undefined
    return () => {
      window.EventSource = OriginalEventSource
    }
  }

  class MockEventSource {
    static CONNECTING = 0
    static OPEN = 1
    static CLOSED = 2

    public readonly url: string
    public readonly withCredentials = false
    public readyState = MockEventSource.OPEN
    public onopen: ((this: EventSource, ev: Event) => unknown) | null = null
    public onerror: ((this: EventSource, ev: Event) => unknown) | null = null
    public onmessage: ((this: EventSource, ev: MessageEvent) => unknown) | null = null

    private listeners = new Map<string, Set<EventListenerOrEventListenerObject>>()

    constructor(url: string) {
      this.url = url
      activeEventSources.add(this as unknown as MockEventSourceShape)
      window.setTimeout(() => {
        if (mode === 'Reconnecting') {
          this.readyState = MockEventSource.CONNECTING
          this.onerror?.call(this as unknown as EventSource, new Event('error'))
          return
        }
        this.onopen?.call(this as unknown as EventSource, new Event('open'))
      }, 0)
    }

    addEventListener(type: string, listener: EventListenerOrEventListenerObject): void {
      if (!this.listeners.has(type)) {
        this.listeners.set(type, new Set())
      }
      this.listeners.get(type)?.add(listener)
    }

    removeEventListener(type: string, listener: EventListenerOrEventListenerObject): void {
      this.listeners.get(type)?.delete(listener)
    }

    dispatchEvent(event: Event): boolean {
      const bucket = this.listeners.get(event.type)
      if (!bucket) return true
      bucket.forEach((listener) => {
        if (typeof listener === 'function') {
          listener.call(this, event)
        } else {
          listener.handleEvent(event)
        }
      })
      return true
    }

    close(): void {
      this.readyState = MockEventSource.CLOSED
      activeEventSources.delete(this as unknown as MockEventSourceShape)
    }
  }

  ;(window as Window & { EventSource: typeof EventSource }).EventSource =
    MockEventSource as unknown as typeof EventSource

  return () => {
    window.EventSource = OriginalEventSource
  }
}

function emitUserTokenSnapshot(): void {
  const event = new MessageEvent('snapshot', {
    data: JSON.stringify({
      token: {
        ...tokenDetailSample,
        requestRate: {
          ...tokenDetailSample.requestRate,
          used: tokenDetailSample.requestRate.used + 3,
        },
        businessCalls1h: {
          ...tokenDetailSample.businessCalls1h,
          totalCount: tokenDetailSample.businessCalls1h.totalCount + 2,
        },
        dailyCreditsUsed: tokenDetailSample.dailyCreditsUsed + 6,
      },
      logs: [
        {
          id: 104,
          method: 'POST',
          path: '/mcp',
          query: null,
          httpStatus: 200,
          mcpStatus: 200,
          businessCredits: 1,
          resultStatus: 'success',
          errorMessage: null,
          createdAt: 1_762_386_780,
        },
        ...tokenLogsSample,
      ],
    }),
  })
  activeEventSources.forEach((source) => {
    source.dispatchEvent(event)
  })
}

function UserConsoleStory(
  args: UserConsoleStoryArgs & {
    copyRecoveryMode?: CopyRecoveryMode
    guideRevealMode?: GuideRevealMode
  },
): JSX.Element {
  const [ready, setReady] = useState(false)
  const storyState = useMemo(
    () => resolveStoryState(args),
    [
      args.consoleView,
      args.isAdmin,
      args.landingFocus,
      args.rechargePreview,
      args.tokenDetailPreview,
      args.tokenListState,
      args.routePathOverride,
    ],
  )
  const copyRecoveryMode = args.copyRecoveryMode ?? 'none'
  const guideRevealMode = args.guideRevealMode ?? 'none'
  const pushStatusPreview = args.pushStatusPreview ?? 'Live'
  const pushStatusBubbleOpen = args.pushStatusBubbleOpen ?? false

  useLayoutEffect(() => {
    const previousLocation = `${window.location.pathname}${window.location.search}${window.location.hash}`
    const cleanupFetch = installUserConsoleFetchMock(storyState)
    const cleanupEventSource = installEventSourceMock(pushStatusPreview)
    const cleanupClipboard = copyRecoveryMode === 'none' ? null : installClipboardFailureMock()
    window.history.replaceState(null, '', storyState.routePath)
    const storageKey = 'tavily-hikari:user-console-announcement-closed'
    if (storyState.announcementPreview === 'Closed') {
      window.localStorage.setItem(storageKey, JSON.stringify({
        [announcementModalSample.id]: 1_762_390_000,
        [announcementTickerSample.id]: 1_762_390_120,
        [announcementBodylessTickerSample.id]: 1_762_390_120,
      }))
    } else {
      window.localStorage.removeItem(storageKey)
    }
    setReady(true)

    return () => {
      cleanupFetch()
      cleanupEventSource()
      cleanupClipboard?.()
      window.localStorage.removeItem(storageKey)
      window.history.replaceState(null, '', previousLocation)
      setReady(false)
    }
  }, [
    copyRecoveryMode,
    pushStatusPreview,
    storyState.announcementPreview,
    storyState.isAdmin,
    storyState.rechargePreview,
    storyState.routePath,
    storyState.tokenListMode,
  ])

  useEffect(() => {
    if (!ready || storyState.announcementPreview !== 'History Open') return
    const timer = window.setTimeout(() => {
      document.querySelector<HTMLButtonElement>('.user-console-announcements-trigger')?.click()
    }, 180)
    return () => window.clearTimeout(timer)
  }, [ready, storyState.announcementPreview])

  useEffect(() => {
    if (!ready || !storyState.autoRevealToken) return
    const timer = window.setTimeout(() => {
      const button = document.querySelector<HTMLButtonElement>('.user-console-token-box .token-visibility-button')
      button?.click()
    }, 80)
    return () => window.clearTimeout(timer)
  }, [ready, storyState.autoRevealToken])

  useEffect(() => {
    if (!ready || copyRecoveryMode === 'none') return
    const timer = window.setTimeout(() => {
      const selector = copyRecoveryMode === 'list-manual-bubble'
        ? 'tbody .table-actions button'
        : '.user-console-token-box .token-copy-button'
      const button = document.querySelector<HTMLButtonElement>(selector)
      button?.click()
    }, 180)
    return () => window.clearTimeout(timer)
  }, [copyRecoveryMode, ready])

  useEffect(() => {
    if (!ready || guideRevealMode === 'none') return
    const timer = window.setTimeout(() => {
      if (guideRevealMode === 'detail-guide') {
        document.querySelector<HTMLButtonElement>('.user-console-guide-disclosure-trigger')?.click()
      }
      const button = document.querySelector<HTMLButtonElement>('.guide-token-toggle')
      button?.click()
    }, guideRevealMode === 'landing-guide' ? 200 : 120)
    return () => window.clearTimeout(timer)
  }, [guideRevealMode, ready])

  useEffect(() => {
    if (!ready || storyState.routePath !== TOKEN_DETAIL_PATH || pushStatusPreview !== 'Live') return
    const timer = window.setTimeout(() => {
      emitUserTokenSnapshot()
    }, 500)
    return () => window.clearTimeout(timer)
  }, [pushStatusPreview, ready, storyState.routePath])

  useEffect(() => {
    if (!ready || !pushStatusBubbleOpen || storyState.routePath !== TOKEN_DETAIL_PATH) return
    const timer = window.setTimeout(() => {
      const trigger = document.querySelector<HTMLButtonElement>('.user-console-push-status-trigger')
      trigger?.focus()
    }, 220)
    return () => window.clearTimeout(timer)
  }, [pushStatusBubbleOpen, ready, storyState.routePath])

  useEffect(() => {
    if (!ready || args.autoOpenAccountMenu !== true) return
    const timer = window.setTimeout(() => {
      const trigger = document.querySelector<HTMLButtonElement>('.user-console-account-trigger')
      if (!trigger) return
      trigger.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0 }))
      trigger.click()
    }, 120)
    return () => window.clearTimeout(timer)
  }, [args.autoOpenAccountMenu, ready])

  if (!ready) {
    return <div style={{ minHeight: '100vh' }} />
  }

  const storyKey = [
    storyState.routePath,
    storyState.isAdmin ? 'admin' : 'user',
    storyState.tokenListMode,
    storyState.rechargePreview,
    storyState.rechargeQuotePreview,
    storyState.autoRevealToken ? 'revealed' : 'hidden',
    guideRevealMode,
    pushStatusPreview,
    pushStatusBubbleOpen ? 'push-open' : 'push-closed',
    storyState.announcementPreview,
  ].join(':')

  return <UserConsole key={storyKey} />
}

const meta = {
  title: 'User Console/UserConsole',
  excludeStories: ['__testables'],
  tags: ['autodocs'],
  parameters: {
    controls: { expanded: true },
    docs: {
      description: {
        component: [
          'Merged user-console acceptance surface for the dashboard landing and token-detail preview flows.',
          '',
          'Public docs: [Quick Start](../quick-start.html) · [Configuration & Access](../configuration-access.html) · [Storybook Guide](../storybook-guide.html)',
        ].join('\n'),
      },
    },
    layout: 'fullscreen',
    viewport: { defaultViewport: '1440-device-desktop' },
  },
  args: consoleHomeOverviewArgs,
  argTypes: {
    consoleView: {
      name: 'Console view',
      description: 'Pick the merged console landing page or the dedicated token detail page.',
      options: ['Console Home', 'Token Detail'],
      control: { type: 'inline-radio' },
    },
    isAdmin: {
      name: 'Admin session',
      description: 'Toggle the console between a regular user session and an admin session.',
      control: { type: 'boolean' },
    },
    landingFocus: {
      name: 'Landing focus',
      description: 'Preview which merged section the path route should auto-focus.',
      options: ['Overview Focus', 'Token Focus'],
      control: { type: 'inline-radio' },
      if: { arg: 'consoleView', eq: 'Console Home' },
    },
    tokenListState: {
      name: 'Token list state',
      description: 'Pick the token list presentation for the merged landing page.',
      options: ['Single Token', 'Multiple Tokens', 'Empty'],
      control: { type: 'inline-radio' },
      if: { arg: 'consoleView', eq: 'Console Home' },
    },
    tokenDetailPreview: {
      name: 'Token detail preview',
      description: 'Pick the standard token detail page or the revealed-token variant.',
      options: ['Overview', 'Token Revealed'],
      control: { type: 'select' },
      if: { arg: 'consoleView', eq: 'Token Detail' },
    },
    routePathOverride: {
      table: { disable: true },
      control: false,
    },
    pushStatusPreview: {
      table: { disable: true },
      control: false,
    },
    pushStatusBubbleOpen: {
      table: { disable: true },
      control: false,
    },
    autoOpenAccountMenu: {
      table: { disable: true },
      control: false,
    },
    announcementPreview: {
      table: { disable: true },
      control: false,
    },
    rechargeQuotePreview: {
      table: { disable: true },
      control: false,
    },
  },
  render: (args) => <UserConsoleStory {...args} />,
} satisfies Meta<UserConsoleStoryArgs>

export default meta

type Story = StoryObj<typeof meta>

export const ConsoleHome: Story = {
  args: {
    consoleView: 'Console Home',
    isAdmin: false,
    landingFocus: 'Overview Focus',
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 120))

    for (const selector of [
      '.user-console-header',
      '.user-console-header-inline-meta',
      '.user-console-announcements-trigger',
      '.user-console-account-trigger',
      '.user-console-landing-stack',
      '.user-console-landing-rail',
      '.user-console-recharge-section',
    ]) {
      if (canvasElement.querySelector(selector) == null) {
        throw new Error(`Expected ConsoleHome to render ${selector}`)
      }
    }
    if (canvasElement.querySelector('.user-console-announcements-trigger svg') == null) {
      throw new Error('Expected ConsoleHome to render a local svg icon inside the announcements trigger.')
    }
    const rechargeText = canvasElement.querySelector('.user-console-recharge-section')?.textContent ?? ''
    for (const expected of ['50.00 LDC', '+20', '+100', '+1,000']) {
      if (!rechargeText.includes(expected)) {
        throw new Error(`Expected recharge section to include ${expected}`)
      }
    }
  },
}

export const ConsoleHomeDark: Story = {
  args: {
    consoleView: 'Console Home',
    isAdmin: true,
    landingFocus: 'Overview Focus',
    tokenListState: 'Single Token',
  },
  globals: {
    language: 'zh',
    themeMode: 'dark',
  },
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
    docs: {
      description: {
        story:
          'Dark-theme user-console landing proof for the repaired low-light header, quota surfaces, token cards, and recent-request regions.',
      },
    },
  },
  play: ConsoleHome.play,
}

export const ConsoleHomeRechargeTestPrice: Story = {
  name: 'Console Home Recharge Test Price',
  args: {
    consoleView: 'Console Home',
    isAdmin: false,
    landingFocus: 'Overview Focus',
    rechargePreview: 'test-price',
  },
  globals: {
    language: 'zh',
  },
}

export const ConsoleHomeRechargeMonthEndClamp: Story = {
  name: 'Console Home Recharge Month End Clamp',
  args: {
    consoleView: 'Console Home',
    isAdmin: false,
    landingFocus: 'Overview Focus',
    rechargeQuotePreview: 'month-end-clamp',
    rechargePreview: 'normal',
  },
  globals: {
    language: 'zh',
  },
  parameters: {
    docs: {
      description: {
        story: 'Month-end recharge quote proof showing the discounted current-month quota, reduced amount, and expired-order marker in history.',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 120))

    const section = canvasElement.querySelector('.user-console-recharge-section')
    if (section == null) {
      throw new Error('Expected month-end clamp proof to render the recharge section.')
    }
    const content = section.textContent ?? ''
    for (const expected of ['本月最终额度', '折后应付', '当前月月额度已折抵', '30.00 LDC', '已过期']) {
      if (!content.includes(expected)) {
        throw new Error(`Expected month-end clamp proof to include ${expected}`)
      }
    }
  },
}

export const ConsoleHomeRechargeDisabled: Story = {
  name: 'Console Home Recharge Disabled',
  args: {
    consoleView: 'Console Home',
    isAdmin: false,
    landingFocus: 'Overview Focus',
    rechargePreview: 'disabled',
  },
  globals: {
    language: 'zh',
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 120))

    const section = canvasElement.querySelector('.user-console-recharge-section')
    if (section == null) {
      throw new Error('Expected disabled recharge proof to keep the recharge section visible.')
    }

    const content = section.textContent ?? ''
    if (!content.includes('不可用')) {
      throw new Error('Expected disabled recharge proof to render the unavailable badge.')
    }
    if (!content.includes('当前服务尚未配置充值功能。')) {
      throw new Error('Expected disabled recharge proof to render the disabled-state copy.')
    }
    if (content.includes('创建订单')) {
      throw new Error('Expected disabled recharge proof to hide the active recharge form.')
    }
  },
}

export const ConsoleHomeRechargeHidden: Story = {
  name: 'Console Home Recharge Hidden',
  args: {
    consoleView: 'Console Home',
    isAdmin: false,
    landingFocus: 'Overview Focus',
    rechargePreview: 'hidden',
  },
  globals: {
    language: 'zh',
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 120))

    if (canvasElement.querySelector('.user-console-recharge-section') != null) {
      throw new Error('Expected hidden recharge proof to remove the recharge section entirely.')
    }

    const landingStack = canvasElement.querySelector('.user-console-landing-stack')
    if (!(landingStack instanceof HTMLElement)) {
      throw new Error('Expected hidden recharge proof to keep the landing stack visible.')
    }
    if (landingStack.classList.contains('has-rail')) {
      throw new Error('Expected hidden recharge proof to remove the landing rail layout class.')
    }
  },
}

export const ConsoleHomeRoot: Story = {
  name: 'Console Home Root',
  args: {
    consoleView: 'Console Home',
    isAdmin: false,
    landingFocus: 'Overview Focus',
    routePathOverride: '/console',
  },
}

export const ConsoleHomeAdmin: Story = {
  name: 'Console Home Admin',
  args: {
    consoleView: 'Console Home',
    isAdmin: true,
    landingFocus: 'Overview Focus',
  },
}

export const ConsoleHomeAnnouncements: Story = {
  name: 'Console Home Announcements',
  args: {
    consoleView: 'Console Home',
    isAdmin: false,
    landingFocus: 'Overview Focus',
    announcementPreview: 'Active',
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 180))

    if (canvasElement.querySelector('.user-console-announcement-ticker') == null) {
      throw new Error('Expected ticker announcement to render.')
    }
    const ticker = canvasElement.querySelector<HTMLElement>('.user-console-announcement-ticker')
    if (ticker?.textContent?.includes('Daily quota counters have refreshed')) {
      throw new Error('Expected ticker announcement to show title only before opening details.')
    }
    if (canvasElement.ownerDocument.querySelector('.user-console-announcement-dialog') == null) {
      throw new Error('Expected modal announcement dialog to render.')
    }
    const acknowledgeButton = Array.from(canvasElement.ownerDocument.querySelectorAll<HTMLButtonElement>('button'))
      .find((button) => ['Got it', '知道了'].includes(button.textContent?.trim() ?? ''))
    acknowledgeButton?.click()
    await new Promise((resolve) => window.setTimeout(resolve, 120))

    const tickerAction = canvasElement.querySelector<HTMLButtonElement>('.user-console-announcement-ticker .user-console-announcement-close')
    tickerAction?.click()
    await new Promise((resolve) => window.setTimeout(resolve, 120))

    const tickerDialog = canvasElement.ownerDocument.querySelector<HTMLElement>('.user-console-announcement-dialog')
    if (!tickerDialog?.textContent?.includes(announcementTickerSample.title)) {
      throw new Error('Expected clicking ticker announcement to open its detail dialog.')
    }
    if (!tickerDialog.textContent?.includes('Daily quota counters have refreshed')) {
      throw new Error('Expected ticker detail dialog to render the announcement body.')
    }
    if (canvasElement.querySelector('.user-console-announcement-ticker') == null) {
      throw new Error('Expected ticker announcement to remain visible while its detail dialog is open.')
    }

    const tickerAcknowledgeButton = Array.from(tickerDialog.querySelectorAll<HTMLButtonElement>('button'))
      .find((button) => ['Got it', '知道了'].includes(button.textContent?.trim() ?? ''))
    if (tickerAcknowledgeButton == null) {
      throw new Error('Expected ticker detail dialog to render an acknowledge action.')
    }
    tickerAcknowledgeButton?.click()
    await new Promise((resolve) => window.setTimeout(resolve, 120))

    if (canvasElement.querySelector('.user-console-announcement-ticker') != null) {
      throw new Error('Expected acknowledging ticker details to dismiss the ticker announcement.')
    }
    if (canvasElement.ownerDocument.querySelector('.user-console-announcement-dialog') != null) {
      throw new Error('Expected acknowledging ticker details to close the detail dialog.')
    }
  },
}

export const ConsoleHomeTickerDetailClose: Story = {
  name: 'Console Home Ticker Detail Close',
  args: {
    consoleView: 'Console Home',
    isAdmin: false,
    landingFocus: 'Overview Focus',
    announcementPreview: 'Active',
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 180))

    const modalAcknowledgeButton = Array.from(canvasElement.ownerDocument.querySelectorAll<HTMLButtonElement>('button'))
      .find((button) => ['Got it', '知道了'].includes(button.textContent?.trim() ?? ''))
    if (modalAcknowledgeButton == null) {
      throw new Error('Expected initial modal announcement to render an acknowledge action.')
    }
    modalAcknowledgeButton?.click()
    await new Promise((resolve) => window.setTimeout(resolve, 120))

    const tickerMain = canvasElement.querySelector<HTMLButtonElement>('.user-console-announcement-ticker-main')
    if (tickerMain == null) {
      throw new Error('Expected bodyful ticker announcement to render a title trigger.')
    }
    tickerMain?.click()
    await new Promise((resolve) => window.setTimeout(resolve, 120))

    const tickerDialog = canvasElement.ownerDocument.querySelector<HTMLElement>('.user-console-announcement-dialog')
    if (!tickerDialog?.textContent?.includes(announcementTickerSample.title)) {
      throw new Error('Expected clicking ticker title to open its detail dialog.')
    }

    const closeButton = Array.from(tickerDialog.querySelectorAll<HTMLButtonElement>('button'))
      .find((button) => button.textContent?.trim() === 'Close')
    if (closeButton == null) {
      throw new Error('Expected ticker detail dialog to render a close button.')
    }
    closeButton?.click()
    await new Promise((resolve) => window.setTimeout(resolve, 120))

    if (canvasElement.querySelector('.user-console-announcement-ticker') != null) {
      throw new Error('Expected closing ticker details to dismiss the ticker announcement.')
    }
    if (canvasElement.ownerDocument.querySelector('.user-console-announcement-dialog') != null) {
      throw new Error('Expected closing ticker details to close the detail dialog.')
    }
  },
}

export const ConsoleHomeBodylessTicker: Story = {
  name: 'Console Home Bodyless Ticker',
  args: {
    consoleView: 'Console Home',
    isAdmin: false,
    landingFocus: 'Overview Focus',
    announcementPreview: 'Ticker Bodyless',
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 180))

    const ticker = canvasElement.querySelector<HTMLElement>('.user-console-announcement-ticker')
    if (ticker == null) {
      throw new Error('Expected bodyless ticker announcement to render.')
    }
    if (canvasElement.querySelector('.user-console-announcement-ticker-main--static') == null) {
      throw new Error('Expected bodyless ticker copy to render without a details trigger.')
    }

    const closeAction = canvasElement.querySelector<HTMLButtonElement>('.user-console-announcement-ticker .user-console-announcement-close')
    closeAction?.click()
    await new Promise((resolve) => window.setTimeout(resolve, 120))

    if (canvasElement.querySelector('.user-console-announcement-ticker') != null) {
      throw new Error('Expected bodyless ticker close action to dismiss the announcement.')
    }
    if (canvasElement.ownerDocument.querySelector('.user-console-announcement-dialog') != null) {
      throw new Error('Expected bodyless ticker close action to avoid opening a detail dialog.')
    }
  },
}

export const ConsoleHomeAnnouncementHistory: Story = {
  name: 'Console Home Announcement History',
  args: {
    consoleView: 'Console Home',
    isAdmin: true,
    landingFocus: 'Overview Focus',
    announcementPreview: 'History Open',
  },
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 260))

    if (canvasElement.ownerDocument.querySelector('.user-console-announcement-history') == null) {
      throw new Error('Expected announcement history drawer to render.')
    }
  },
}

export const ConsoleHomeAdminMobile: Story = {
  name: 'Console Home Admin Mobile',
  args: consoleHomeAdminOverviewArgs,
  parameters: mobileViewport,
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 120))

    for (const selector of [
      '.user-console-header',
      '.user-console-header-actions',
      '.user-console-account-trigger',
    ]) {
      if (canvasElement.querySelector(selector) == null) {
        throw new Error(`Expected ConsoleHomeAdminMobile to render ${selector}`)
      }
    }

    const menuTrigger = canvasElement.querySelector<HTMLElement>('.user-console-account-trigger')
    if (menuTrigger == null) {
      throw new Error('Expected ConsoleHomeAdminMobile to render a compact account menu trigger.')
    }

    menuTrigger.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0 }))
    menuTrigger.click()
    await new Promise((resolve) => window.setTimeout(resolve, 120))

    for (const selector of [
      '.user-console-account-menu-admin',
      '.user-console-account-menu-logout',
    ]) {
      if (canvasElement.ownerDocument.querySelector(selector) == null) {
        throw new Error(`Expected ConsoleHomeAdminMobile menu to render ${selector}`)
      }
    }
  },
}

export const ConsoleHomeDarkMobile: Story = {
  name: 'Console Home Dark Mobile',
  args: consoleHomeAdminOverviewArgs,
  globals: {
    language: 'zh',
    themeMode: 'dark',
  },
  parameters: {
    ...mobileViewport,
    docs: {
      description: {
        story:
          'Mobile dark-theme user-console proof for touch targets, compact account chrome, and repaired clay contrast.',
      },
    },
  },
  play: ConsoleHomeAdminMobile.play,
}

export const ConsoleHomeAdminMobileMenuOpen: Story = {
  name: 'Console Home Admin Mobile Menu Open',
  args: {
    ...consoleHomeAdminOverviewArgs,
    autoOpenAccountMenu: true,
  },
  parameters: mobileViewport,
}

export const ConsoleHomeTokensFocus: Story = {
  name: 'Console Home Tokens Focus',
  args: consoleHomeTokenFocusArgs,
  play: async ({ canvasElement }) => {
    await expectTokenListProof(
      canvasElement,
      '.user-console-tokens-table',
      [...removedTokenQuotaLabels, ...removedTokenInternalLabels],
      [
        ['Status', '状态与最近使用'],
        ['Last Used', '最近使用'],
        ['Daily Success', '今日成功'],
      ],
      'token-focus landing proof',
    )
  },
}

export const ConsoleHomeTokensFocusAdmin: Story = {
  name: 'Console Home Tokens Focus Admin',
  args: consoleHomeAdminTokenFocusArgs,
}

export const ConsoleHomeTokensFocusMobile: Story = {
  name: 'Console Home Tokens Focus Mobile',
  args: consoleHomeTokenFocusArgs,
  parameters: mobileViewport,
  play: async ({ canvasElement }) => {
    await expectTokenListProof(
      canvasElement,
      '.user-console-mobile-card',
      [...removedTokenQuotaLabelsMobile, ...removedTokenInternalLabels],
      [
        ['状态与最近使用', 'Status & Activity'],
        ['最近使用', 'Last Used'],
        ['今日成功', 'Daily Success'],
        ['详情', 'Details'],
      ],
      'token-focus mobile proof',
    )
  },
}

export const ConsoleHomeMultipleTokens: Story = {
  name: 'Console Home Multiple Tokens',
  args: {
    ...consoleHomeTokenFocusArgs,
    tokenListState: 'Multiple Tokens',
  },
}

export const ConsoleHomeEmptyTokens: Story = {
  name: 'Console Home Empty Tokens',
  args: {
    ...consoleHomeTokenFocusArgs,
    tokenListState: 'Empty',
  },
}

export const ConsoleHomeCopyFailureRecovery: Story = {
  name: 'Console Home Copy Failure Recovery',
  args: consoleHomeTokenFocusArgs,
  render: (args) => <UserConsoleStory {...args} copyRecoveryMode="list-manual-bubble" />,
}

export const ConsoleHomeGuideTokenRevealed: Story = {
  name: 'Console Home Guide Token Revealed',
  args: consoleHomeTokenFocusArgs,
  render: (args) => <UserConsoleStory {...args} guideRevealMode="landing-guide" />,
}

export const TokenDetailOverview: Story = {
  name: 'Token Detail Overview',
  args: tokenDetailOverviewArgs,
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 160))

    const guideTrigger = canvasElement.querySelector<HTMLButtonElement>('.user-console-guide-disclosure-trigger')
    if (guideTrigger == null) {
      throw new Error('Expected token detail to render a collapsed setup guide trigger.')
    }

    if (guideTrigger.getAttribute('aria-expanded') !== 'false') {
      throw new Error('Expected setup guide to be collapsed by default.')
    }

    if (canvasElement.querySelector('.user-console-guide-disclosure-body') != null) {
      throw new Error('Expected setup guide body to stay out of the initial detail layout.')
    }

    if (canvasElement.querySelector('.user-console-logs-table th:nth-child(3)')?.textContent?.trim() !== 'Credits') {
      throw new Error('Expected token detail logs table to render the Credits column between transport and result.')
    }

    if (!canvasElement.textContent?.includes('Recent Requests (50)')) {
      throw new Error('Expected token detail logs heading to advertise the 50-row recent request window.')
    }

    const initialRows = canvasElement.querySelectorAll('.user-console-logs-table tbody tr')
    if (initialRows.length !== 50) {
      throw new Error(`Expected token detail logs table to render 50 recent rows, got ${initialRows.length}.`)
    }

    const creditedRows = Array.from(canvasElement.querySelectorAll('.user-console-log-credits'))
      .map((node) => node.textContent?.trim())
    if (!creditedRows.includes('2') || !creditedRows.includes('—')) {
      throw new Error('Expected token detail logs to render both charged and uncharged credit values.')
    }

    const billableButton = Array.from(canvasElement.querySelectorAll<HTMLButtonElement>('.user-console-log-filter-tabs .segmented-tab'))
      .find((button) => button.textContent?.trim() === 'Quota usage')
    if (billableButton == null) {
      throw new Error('Expected token detail logs to render the quota-usage filter.')
    }
    billableButton.click()
    await new Promise((resolve) => window.setTimeout(resolve, 180))

    const filteredRows = canvasElement.querySelectorAll('.user-console-logs-table tbody tr')
    if (filteredRows.length === 0 || filteredRows.length >= initialRows.length) {
      throw new Error('Expected quota-usage filter to reduce the recent request list to billable request kinds.')
    }
    const filteredText = canvasElement.querySelector('.user-console-logs-table')?.textContent ?? ''
    if (filteredText.includes('/api/tavily/usage')) {
      throw new Error('Expected quota-usage filter to exclude non-billable usage requests.')
    }
  },
}

export const TokenDetailMobileCredits: Story = {
  name: 'Token Detail Mobile Logs Entry',
  args: tokenDetailOverviewArgs,
  globals: {
    language: 'zh',
  },
  parameters: {
    ...mobileViewport,
    docs: {
      description: {
        story:
          'Mobile token detail proof that recent requests collapse into a dedicated entry instead of crowding the detail page.',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 160))

    const entry = canvasElement.querySelector<HTMLButtonElement>('.user-console-mobile-log-entry')
    if (entry == null || entry.textContent?.includes('查看近期请求') !== true) {
      throw new Error('Expected mobile token detail to render a recent-request entry button.')
    }
    if (entry.querySelector('.user-console-mobile-log-entry-icon') != null || entry.querySelector('.user-console-mobile-log-entry-action')?.textContent?.trim() !== '›') {
      throw new Error('Expected mobile token detail request entry to look like a navigable row.')
    }
    if (entry.getAttribute('aria-label')?.includes('最近 50 条请求') !== true) {
      throw new Error('Expected mobile token detail request entry to expose a concise accessible label.')
    }
    if (canvasElement.querySelector('.user-console-log-card') != null) {
      throw new Error('Expected mobile token detail to keep request cards off the detail page.')
    }
  },
}

export const TokenLogsMobile: Story = {
  name: 'Token Logs Mobile',
  args: {
    ...tokenDetailOverviewArgs,
    routePathOverride: `${TOKEN_DETAIL_PATH}/logs`,
  },
  globals: {
    language: 'zh',
  },
  parameters: {
    ...mobileViewport,
    docs: {
      description: {
        story:
          'Mobile recent-request page proof that the dedicated log surface keeps the billing filter in the header row.',
      },
    },
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 160))

    if (canvasElement.querySelector('[aria-label="近期请求额度筛选"]') == null) {
      throw new Error('Expected mobile token logs page to expose the billing filter select.')
    }
    const metaText = Array.from(canvasElement.querySelectorAll('.user-console-log-card-meta'))
      .map((node) => node.textContent ?? '')
      .join(' ')
    if (!metaText.includes('积分 2') || !metaText.includes('积分 —')) {
      throw new Error('Expected mobile token logs page to render charged and uncharged credit values.')
    }
  },
}

export const TokenDetailDark: Story = {
  name: 'Token Detail Dark',
  args: tokenDetailOverviewArgs,
  globals: {
    language: 'zh',
    themeMode: 'dark',
  },
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
    docs: {
      description: {
        story:
          'Dark-theme token detail proof for charts, setup disclosure, live logs, and token controls on the repaired material system.',
      },
    },
  },
  play: TokenDetailOverview.play,
}

export const TokenDetailLiveLogs: Story = {
  name: 'Token Detail Live Logs',
  args: tokenDetailOverviewArgs,
}

export const TokenDetailPushWarning: Story = {
  name: 'Token Detail Push Warning',
  args: {
    ...tokenDetailOverviewArgs,
    pushStatusPreview: 'Reconnecting',
    pushStatusBubbleOpen: true,
  },
}

export const TokenDetailCopyFailureRecovery: Story = {
  name: 'Token Detail Copy Failure Recovery',
  args: tokenDetailOverviewArgs,
  render: (args) => <UserConsoleStory {...args} copyRecoveryMode="detail-inline" />,
}

export const TokenRevealed: Story = {
  name: 'Token Revealed',
  args: {
    consoleView: 'Token Detail',
    isAdmin: false,
    tokenDetailPreview: 'Token Revealed',
  },
}

export const TokenDetailGuideTokenRevealed: Story = {
  name: 'Token Detail Guide Token Revealed',
  args: tokenDetailOverviewArgs,
  render: (args) => <UserConsoleStory {...args} guideRevealMode="detail-guide" />,
}

export const TokenDetailAdmin: Story = {
  name: 'Token Detail Admin',
  args: tokenDetailAdminOverviewArgs,
}

export const MobileGuideMenuProof: Story = {
  name: 'Mobile Guide Menu Proof',
  args: consoleHomeOverviewArgs,
  render: () => <UserConsoleMobileGuideMenuProof />,
  parameters: {
    layout: 'padded',
    ...mobileViewport,
  },
}
