type JsonResponse = (payload: unknown, status?: number) => Response
type NowSeconds = (offset?: number) => number
type DemoRechargeOrderStatus =
  | 'pending'
  | 'paid'
  | 'failed'
  | 'expired'
  | 'cancelled'
  | 'refunding'
  | 'refunded'
  | 'refundOnly'

export interface DemoRechargeOrder {
  outTradeNo: string
  userId: string
  userDisplayName: string
  username: string
  status: DemoRechargeOrderStatus
  credits: number
  months: number
  money: string
  quoteMonthStart: number
  finalMoneyCents: number
  finalHourlyDelta: number
  finalDailyDelta: number
  finalMonthlyDelta: number
  monthEndClampApplied: boolean
  tradeNo: string | null
  paymentUrl: string | null
  createdAt: number
  payExpiresAt: number
  cancelAfterAt: number
  cancelledAt: number | null
  updatedAt: number
  paidAt: number | null
  refundedAt: number | null
  refundActor: string | null
  lastNotifyAt: number | null
  refundRetryAfterAt: number | null
  refundAttempts: number
  lastError: string | null
}

export function createDemoRechargeOrders(nowSeconds: NowSeconds, origin: string): DemoRechargeOrder[] {
  return [
    createDemoRechargeOrder(nowSeconds, origin, {
      outTradeNo: 'ldc_demo_paid_001',
      userId: 'user-demo-admin',
      userDisplayName: 'Hikari Demo Admin',
      username: 'hikari-demo',
      status: 'paid',
      credits: 3000,
      months: 3,
      amountLdc: 450,
      monthEndClampApplied: false,
      createdOffset: -3600 * 5,
      paidOffset: -3600 * 4,
    }),
    createDemoRechargeOrder(nowSeconds, origin, {
      outTradeNo: 'ldc_demo_refund_002',
      userId: 'user-research',
      userDisplayName: 'Research Team',
      username: 'research-team',
      status: 'refunded',
      credits: 1000,
      months: 1,
      amountLdc: 50,
      monthEndClampApplied: false,
      createdOffset: -86400 * 3,
      paidOffset: -86400 * 3 + 900,
      refundedOffset: -86400,
      refundActor: 'system:auto',
      refundAttempts: 1,
    }),
    createDemoRechargeOrder(nowSeconds, origin, {
      outTradeNo: 'ldc_demo_only_003',
      userId: 'user-demo-admin',
      userDisplayName: 'Hikari Demo Admin',
      username: 'hikari-demo',
      status: 'refundOnly',
      credits: 2000,
      months: 2,
      amountLdc: 200,
      monthEndClampApplied: false,
      createdOffset: -86400 * 10,
      paidOffset: -86400 * 10 + 1200,
      refundedOffset: -86400 * 2,
      refundActor: 'demo-admin',
    }),
    createDemoRechargeOrder(nowSeconds, origin, {
      outTradeNo: 'ldc_demo_pending_004',
      userId: 'user-demo-admin',
      userDisplayName: 'Hikari Demo Admin',
      username: 'hikari-demo',
      status: 'pending',
      credits: 1000,
      months: 1,
      amountLdc: 50,
      monthEndClampApplied: false,
      createdOffset: -120,
    }),
    createDemoRechargeOrder(nowSeconds, origin, {
      outTradeNo: 'ldc_demo_expired_005',
      userId: 'user-demo-admin',
      userDisplayName: 'Hikari Demo Admin',
      username: 'hikari-demo',
      status: 'expired',
      credits: 1000,
      months: 1,
      amountLdc: 50,
      monthEndClampApplied: true,
      createdOffset: -3600 * 2,
      lastError: 'payment entry closed locally after 10 minutes',
    }),
    createDemoRechargeOrder(nowSeconds, origin, {
      outTradeNo: 'ldc_demo_cancelled_006',
      userId: 'user-demo-admin',
      userDisplayName: 'Hikari Demo Admin',
      username: 'hikari-demo',
      status: 'cancelled',
      credits: 1000,
      months: 1,
      amountLdc: 50,
      monthEndClampApplied: false,
      createdOffset: -90000,
      cancelledOffset: -3600,
      lastError: 'order cancelled after 24 hours',
    }),
    createDemoRechargeOrder(nowSeconds, origin, {
      outTradeNo: 'ldc_demo_refunding_007',
      userId: 'user-research',
      userDisplayName: 'Research Team',
      username: 'research-team',
      status: 'refunding',
      credits: 1000,
      months: 1,
      amountLdc: 30,
      monthEndClampApplied: true,
      createdOffset: -86400,
      paidOffset: -1800,
      refundActor: 'system:auto',
      refundRetryAfterOffset: 300,
      refundAttempts: 2,
      lastError: 'paid month no longer matches quote month',
    }),
    createDemoRechargeOrder(nowSeconds, origin, {
      outTradeNo: 'ldc_demo_paid_008',
      userId: 'user-demo-admin',
      userDisplayName: 'Hikari Demo Admin',
      username: 'hikari-demo',
      status: 'paid',
      credits: 2000,
      months: 1,
      amountLdc: 100,
      monthEndClampApplied: false,
      createdOffset: -86400 * 6,
      paidOffset: -86400 * 6 + 720,
    }),
    createDemoRechargeOrder(nowSeconds, origin, {
      outTradeNo: 'ldc_demo_only_009',
      userId: 'user-research',
      userDisplayName: 'Research Team',
      username: 'research-team',
      status: 'refundOnly',
      credits: 3000,
      months: 2,
      amountLdc: 300,
      monthEndClampApplied: false,
      createdOffset: -86400 * 7,
      paidOffset: -86400 * 7 + 960,
      refundedOffset: -86400 * 3,
      refundActor: 'demo-admin',
    }),
    createDemoRechargeOrder(nowSeconds, origin, {
      outTradeNo: 'ldc_demo_failed_010',
      userId: 'user-demo-admin',
      userDisplayName: 'Hikari Demo Admin',
      username: 'hikari-demo',
      status: 'failed',
      credits: 4000,
      months: 1,
      amountLdc: 200,
      monthEndClampApplied: false,
      createdOffset: -86400 * 8,
      lastError: 'payment failed',
    }),
    createDemoRechargeOrder(nowSeconds, origin, {
      outTradeNo: 'ldc_demo_paid_011',
      userId: 'user-ops',
      userDisplayName: 'Ops Team',
      username: 'ops-team',
      status: 'paid',
      credits: 5000,
      months: 2,
      amountLdc: 500,
      monthEndClampApplied: false,
      createdOffset: -86400 * 9,
      paidOffset: -86400 * 9 + 720,
    }),
    createDemoRechargeOrder(nowSeconds, origin, {
      outTradeNo: 'ldc_demo_refund_012',
      userId: 'user-demo-admin',
      userDisplayName: 'Hikari Demo Admin',
      username: 'hikari-demo',
      status: 'refunded',
      credits: 6000,
      months: 1,
      amountLdc: 300,
      monthEndClampApplied: false,
      createdOffset: -86400 * 11,
      paidOffset: -86400 * 11 + 960,
      refundedOffset: -86400 * 4,
      refundActor: 'system:auto',
      refundAttempts: 1,
    }),
  ]
}

interface CreateDemoRechargeOrderInput {
  outTradeNo: string
  userId: string
  userDisplayName: string
  username: string
  status: DemoRechargeOrderStatus
  credits: number
  months: number
  amountLdc: number
  monthEndClampApplied: boolean
  createdOffset: number
  paidOffset?: number | null
  refundedOffset?: number | null
  cancelledOffset?: number | null
  refundRetryAfterOffset?: number | null
  refundActor?: string | null
  refundAttempts?: number
  lastError?: string | null
}

function createDemoRechargeOrder(
  nowSeconds: NowSeconds,
  origin: string,
  input: CreateDemoRechargeOrderInput,
): DemoRechargeOrder {
  const createdAt = nowSeconds(input.createdOffset)
  const payExpiresAt = createdAt + 600
  const cancelAfterAt = createdAt + 86_400
  const paidAt = input.paidOffset == null ? null : nowSeconds(input.paidOffset)
  const refundedAt = input.refundedOffset == null ? null : nowSeconds(input.refundedOffset)
  const cancelledAt = input.cancelledOffset == null
    ? input.status === 'cancelled' ? cancelAfterAt : null
    : nowSeconds(input.cancelledOffset)
  const paymentUrl = input.status === 'pending'
    ? `${origin}/console/dashboard?demo_checkout=${encodeURIComponent(input.outTradeNo)}`
    : null
  return {
    outTradeNo: input.outTradeNo,
    userId: input.userId,
    userDisplayName: input.userDisplayName,
    username: input.username,
    status: input.status,
    credits: input.credits,
    months: input.months,
    money: input.monthEndClampApplied ? '30.00' : input.amountLdc.toFixed(2),
    quoteMonthStart: monthStartSeconds(),
    finalMoneyCents: Math.round(input.amountLdc * 100),
    finalHourlyDelta: input.credits >= 1000 ? 20 * (input.credits / 1000) : input.credits,
    finalDailyDelta: input.credits >= 1000 ? 100 * (input.credits / 1000) : input.credits,
    finalMonthlyDelta: input.credits,
    monthEndClampApplied: input.monthEndClampApplied,
    tradeNo: paidAt == null ? null : `linuxdo_${input.outTradeNo.slice(-3)}`,
    paymentUrl,
    createdAt,
    payExpiresAt,
    cancelAfterAt,
    cancelledAt,
    updatedAt: refundedAt ?? cancelledAt ?? paidAt ?? createdAt,
    paidAt,
    refundedAt,
    refundActor: input.refundActor ?? (refundedAt == null ? null : 'demo-admin'),
    lastNotifyAt: paidAt == null ? null : paidAt + 60,
    refundRetryAfterAt: input.status === 'refunding'
      ? input.refundRetryAfterOffset == null ? nowSeconds() + 300 : nowSeconds(input.refundRetryAfterOffset)
      : null,
    refundAttempts: input.status === 'refunding' ? (input.refundAttempts ?? 1) : 0,
    lastError: input.lastError ?? null,
  }
}

export function demoAdminRechargeOrder(order: DemoRechargeOrder) {
  return {
    outTradeNo: order.outTradeNo,
    user: { id: order.userId, displayName: order.userDisplayName, username: order.username, avatarTemplate: null },
    status: order.status,
    credits: order.credits,
    months: order.months,
    moneyCents: Math.round(Number(order.money) * 100),
    money: order.money,
    quoteMonthStart: order.quoteMonthStart,
    finalMoneyCents: order.finalMoneyCents,
    finalHourlyDelta: order.finalHourlyDelta,
    finalDailyDelta: order.finalDailyDelta,
    finalMonthlyDelta: order.finalMonthlyDelta,
    monthEndClampApplied: order.monthEndClampApplied,
    tradeNo: order.tradeNo,
    paymentUrl: order.paymentUrl,
    orderName: `Linux.do Credit ${order.credits} credits`,
    createdAt: order.createdAt,
    payExpiresAt: order.payExpiresAt,
    cancelAfterAt: order.cancelAfterAt,
    cancelledAt: order.cancelledAt,
    updatedAt: order.updatedAt,
    paidAt: order.paidAt,
    refundedAt: order.refundedAt,
    refundActor: order.refundActor,
    lastNotifyAt: order.lastNotifyAt,
    refundRetryAfterAt: order.refundRetryAfterAt,
    refundAttempts: order.refundAttempts,
    lastError: order.lastError,
  }
}

export function handleDemoAdminRecharges(orders: DemoRechargeOrder[], url: URL, jsonResponse: JsonResponse): Response {
  const status = url.searchParams.get('status')
  const userQuery = (url.searchParams.get('user') ?? '').trim().toLowerCase()
  const sort = url.searchParams.get('sort') ?? 'createdAt'
  const orderDirection = url.searchParams.get('order') === 'asc' ? 1 : -1
  const filtered = orders.filter((item) => {
    if (status && status !== 'all' && item.status !== status) return false
    if (!userQuery) return true
    return item.userDisplayName.toLowerCase().includes(userQuery)
      || item.username.toLowerCase().includes(userQuery)
      || item.userId.toLowerCase().includes(userQuery)
  })
  const sorted = [...filtered].sort((left, right) => {
    const leftValue = readSortValue(left, sort)
    const rightValue = readSortValue(right, sort)
    if (typeof leftValue === 'string' || typeof rightValue === 'string') {
      return String(leftValue).localeCompare(String(rightValue)) * orderDirection
    }
    return (leftValue - rightValue) * orderDirection
  })
  const page = paginate(sorted.map(demoAdminRechargeOrder), url, 25)
  return jsonResponse({
    hasRechargeOrders: orders.length > 0,
    items: page.items,
    groups: demoAdminRechargeGroups(sorted),
    total: page.total,
    page: page.page,
    perPage: page.perPage,
  })
}

export function handleDemoAdminRechargeAction(
  orders: DemoRechargeOrder[],
  path: string,
  method: string,
  jsonResponse: JsonResponse,
  nowSeconds: () => number,
): Response | null {
  const match = path.match(/^\/api\/admin\/recharges\/([^/]+)\/(refund|refund-only)$/)
  if (!match || method !== 'POST') return null
  const outTradeNo = decodeURIComponent(match[1])
  const order = orders.find((item) => item.outTradeNo === outTradeNo)
  if (!order) return jsonResponse({ message: 'Demo recharge order not found' }, 404)
  order.status = match[2] === 'refund-only' ? 'refundOnly' : 'refunded'
  order.refundedAt = nowSeconds()
  order.refundActor = 'demo-admin'
  order.updatedAt = nowSeconds()
  order.refundRetryAfterAt = null
  order.refundAttempts = 0
  return jsonResponse(demoAdminRechargeOrder(order))
}

export function demoAdminUserRechargeAudit(orders: DemoRechargeOrder[], userId: string) {
  const userOrders = orders.filter((order) => order.userId === userId)
  const entitlements = userOrders
    .filter((order) => order.paidAt != null
      && order.status !== 'refunded'
      && !(order.status === 'refunding' && order.refundActor === 'system:auto'))
    .flatMap((order, orderIndex) => range(order.months).map((month) => ({
      id: orderIndex * 100 + month + 1,
      outTradeNo: order.outTradeNo,
      monthStart: monthStartSeconds(month),
      credits: order.credits,
      hourlyDelta: order.finalHourlyDelta,
      dailyDelta: order.finalDailyDelta,
      monthlyDelta: order.finalMonthlyDelta,
      createdAt: order.paidAt ?? order.createdAt,
    })))
  return {
    currentMonthEntitlementCredits: entitlements
      .filter((item) => item.monthStart === monthStartSeconds(0))
      .reduce((sum, item) => sum + item.credits, 0),
    currentMonthEntitlementHourlyDelta: entitlements
      .filter((item) => item.monthStart === monthStartSeconds(0))
      .reduce((sum, item) => sum + item.hourlyDelta, 0),
    currentMonthEntitlementDailyDelta: entitlements
      .filter((item) => item.monthStart === monthStartSeconds(0))
      .reduce((sum, item) => sum + item.dailyDelta, 0),
    currentMonthEntitlementMonthlyDelta: entitlements
      .filter((item) => item.monthStart === monthStartSeconds(0))
      .reduce((sum, item) => sum + item.monthlyDelta, 0),
    effectiveUntilMonthStart: entitlements.length > 0
      ? Math.max(...entitlements.map((item) => item.monthStart))
      : null,
    orders: userOrders.map((order) => ({
      outTradeNo: order.outTradeNo,
      status: order.status,
      credits: order.credits,
      months: order.months,
      money: order.money,
      quoteMonthStart: order.quoteMonthStart,
      finalMoneyCents: order.finalMoneyCents,
      finalHourlyDelta: order.finalHourlyDelta,
      finalDailyDelta: order.finalDailyDelta,
      finalMonthlyDelta: order.finalMonthlyDelta,
      monthEndClampApplied: order.monthEndClampApplied,
      tradeNo: order.tradeNo,
      paymentUrl: order.paymentUrl,
      createdAt: order.createdAt,
      payExpiresAt: order.payExpiresAt,
      cancelAfterAt: order.cancelAfterAt,
      cancelledAt: order.cancelledAt,
      updatedAt: order.updatedAt,
      paidAt: order.paidAt,
      refundedAt: order.refundedAt,
      refundActor: order.refundActor,
      lastNotifyAt: order.lastNotifyAt,
      refundRetryAfterAt: order.refundRetryAfterAt,
      refundAttempts: order.refundAttempts,
      lastError: order.lastError,
    })),
    entitlements,
  }
}

function readSortValue(item: DemoRechargeOrder, sort: string): number | string {
  if (sort === 'paidAt') return item.paidAt ?? 0
  if (sort === 'refundedAt') return item.refundedAt ?? 0
  if (sort === 'status') return item.status
  return item.createdAt
}

function demoAdminRechargeGroups(orders: DemoRechargeOrder[]) {
  const groups = new Map<string, ReturnType<typeof demoAdminRechargeOrder>[]>()
  for (const order of orders) {
    const mapped = demoAdminRechargeOrder(order)
    const list = groups.get(mapped.user.id) ?? []
    list.push(mapped)
    groups.set(mapped.user.id, list)
  }
  return Array.from(groups.values()).map((ordersForUser) => {
    const [first] = ordersForUser
    return {
      user: first.user,
      orderCount: ordersForUser.length,
      paidOrderCount: ordersForUser.filter((item) => item.paidAt != null).length,
      refundedOrderCount: ordersForUser.filter((item) => item.refundedAt != null).length,
      totalCredits: ordersForUser.reduce((sum, item) => sum + item.credits, 0),
      totalMoneyCents: ordersForUser.reduce((sum, item) => sum + item.moneyCents, 0),
      latestOrderCreatedAt: Math.max(...ordersForUser.map((item) => item.createdAt)),
      latestPaidAt: Math.max(0, ...ordersForUser.map((item) => item.paidAt ?? 0)) || null,
      latestRefundedAt: Math.max(0, ...ordersForUser.map((item) => item.refundedAt ?? 0)) || null,
    }
  })
}

function paginate<T>(items: T[], url: URL, defaultPerPage: number) {
  const page = Math.max(1, Number(url.searchParams.get('page') ?? '1') || 1)
  const perPage = Math.max(1, Number(url.searchParams.get('per_page') ?? url.searchParams.get('limit') ?? defaultPerPage) || defaultPerPage)
  const start = (page - 1) * perPage
  return { items: items.slice(start, start + perPage), total: items.length, page, perPage }
}

function monthStartSeconds(monthOffset = 0): number {
  const now = new Date()
  return Math.floor(new Date(now.getFullYear(), now.getMonth() + monthOffset, 1).getTime() / 1000)
}

function range(length: number): number[] {
  return Array.from({ length }, (_, index) => index)
}
