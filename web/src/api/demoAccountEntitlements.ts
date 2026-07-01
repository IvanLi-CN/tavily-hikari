function nowSeconds(offsetSeconds = 0): number {
  return Math.floor(Date.now() / 1000) + offsetSeconds
}

function monthStartSeconds(monthOffset = 0): number {
  const now = new Date()
  return Math.floor(new Date(now.getFullYear(), now.getMonth() + monthOffset, 1).getTime() / 1000)
}

export function demoUserEntitlements(userId: string) {
  const currentMonthStart = monthStartSeconds(0)
  return {
    currentMonthStart,
    currentMonthDelta: { businessCalls1hDelta: 25, dailyCreditsDelta: 180, monthlyCreditsDelta: 1800 },
    currentPermanentDelta: { businessCalls1hDelta: 5, dailyCreditsDelta: 25, monthlyCreditsDelta: 250 },
    items: [
      {
        id: 9001,
        userId,
        scopeKind: 'month',
        monthStart: currentMonthStart,
        businessCalls1hDelta: 25,
        dailyCreditsDelta: 180,
        monthlyCreditsDelta: 1800,
        backendNote: 'demo monthly admin adjustment',
        frontendNote: 'monthly bonus',
        sourceKind: 'admin',
        sourceId: 'admin-demo-month',
        actorUserId: null,
        actorDisplayName: 'demo-admin',
        createdAt: nowSeconds(-3600),
      },
      {
        id: 9002,
        userId,
        scopeKind: 'permanent',
        monthStart: 0,
        businessCalls1hDelta: 5,
        dailyCreditsDelta: 25,
        monthlyCreditsDelta: 250,
        backendNote: 'demo permanent entitlement',
        frontendNote: 'permanent quota uplift',
        sourceKind: 'admin',
        sourceId: 'admin-demo-permanent',
        actorUserId: null,
        actorDisplayName: 'demo-admin',
        createdAt: nowSeconds(-7200),
      },
    ],
  }
}
