type JsonResponse = (payload: unknown, status?: number) => Response
type NowSeconds = (offset?: number) => number

export interface DemoRechargeOrder {
  outTradeNo: string
  userId: string
  userDisplayName: string
  username: string
  status: string
  credits: number
  months: number
  money: string
  tradeNo: string | null
  paymentUrl: string | null
  createdAt: number
  updatedAt: number
  paidAt: number | null
  refundedAt: number | null
  refundActor: string | null
  lastNotifyAt: number | null
  lastError: string | null
}

export function createDemoRechargeOrders(nowSeconds: NowSeconds, origin: string): DemoRechargeOrder[] {
  return [
    createDemoRechargeOrder(nowSeconds, origin, 'ldc_demo_paid_001', 'user-demo-admin', 'Hikari Demo Admin', 'hikari-demo', 'paid', 3000, 3, 450, -3600 * 5, -3600 * 4, null),
    createDemoRechargeOrder(nowSeconds, origin, 'ldc_demo_refund_002', 'user-research', 'Research Team', 'research-team', 'refunded', 1000, 1, 50, -86400 * 3, -86400 * 3 + 900, -86400),
    createDemoRechargeOrder(nowSeconds, origin, 'ldc_demo_only_003', 'user-demo-admin', 'Hikari Demo Admin', 'hikari-demo', 'refundOnly', 2000, 2, 200, -86400 * 10, -86400 * 10 + 1200, -86400 * 2),
  ]
}

function createDemoRechargeOrder(
  nowSeconds: NowSeconds,
  origin: string,
  outTradeNo: string,
  userId: string,
  userDisplayName: string,
  username: string,
  status: string,
  credits: number,
  months: number,
  amountLdc: number,
  createdOffset: number,
  paidOffset: number | null,
  refundedOffset: number | null,
): DemoRechargeOrder {
  return {
    outTradeNo,
    userId,
    userDisplayName,
    username,
    status,
    credits,
    months,
    money: amountLdc.toFixed(2),
    tradeNo: paidOffset == null ? null : `linuxdo_${outTradeNo.slice(-3)}`,
    paymentUrl: paidOffset == null ? `${origin}/console/dashboard?demo_checkout=${encodeURIComponent(outTradeNo)}` : null,
    createdAt: nowSeconds(createdOffset),
    updatedAt: nowSeconds(refundedOffset ?? paidOffset ?? createdOffset),
    paidAt: paidOffset == null ? null : nowSeconds(paidOffset),
    refundedAt: refundedOffset == null ? null : nowSeconds(refundedOffset),
    refundActor: refundedOffset == null ? null : 'demo-admin',
    lastNotifyAt: paidOffset == null ? null : nowSeconds(paidOffset + 60),
    lastError: null,
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
    tradeNo: order.tradeNo,
    paymentUrl: order.paymentUrl,
    orderName: `Linux.do Credit ${order.credits} credits`,
    createdAt: order.createdAt,
    updatedAt: order.updatedAt,
    paidAt: order.paidAt,
    refundedAt: order.refundedAt,
    refundActor: order.refundActor,
    lastNotifyAt: order.lastNotifyAt,
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
  return jsonResponse(demoAdminRechargeOrder(order))
}

export function demoAdminUserRechargeAudit(orders: DemoRechargeOrder[], userId: string) {
  const userOrders = orders.filter((order) => order.userId === userId)
  const entitlements = userOrders
    .filter((order) => order.paidAt != null && order.status !== 'refunded')
    .flatMap((order, orderIndex) => range(order.months).map((month) => ({
      id: orderIndex * 100 + month + 1,
      outTradeNo: order.outTradeNo,
      monthStart: monthStartSeconds(month),
      credits: order.credits,
      createdAt: order.paidAt ?? order.createdAt,
    })))
  return {
    currentMonthEntitlementCredits: entitlements
      .filter((item) => item.monthStart === monthStartSeconds(0))
      .reduce((sum, item) => sum + item.credits, 0),
    effectiveUntilMonthStart: entitlements.length > 0
      ? Math.max(...entitlements.map((item) => item.monthStart))
      : null,
    orders: userOrders.map((order) => ({
      outTradeNo: order.outTradeNo,
      status: order.status,
      credits: order.credits,
      months: order.months,
      money: order.money,
      tradeNo: order.tradeNo,
      paymentUrl: order.paymentUrl,
      createdAt: order.createdAt,
      updatedAt: order.updatedAt,
      paidAt: order.paidAt,
      refundedAt: order.refundedAt,
      refundActor: order.refundActor,
      lastNotifyAt: order.lastNotifyAt,
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
