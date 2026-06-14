type DemoHaState = {
  haStatus: {
    mode: string
    nodeId: string
    nodePublicOrigin: string | null
    role: string
    degraded: boolean
    allowsBasicBusiness: boolean
    allowsFullWrites: boolean
    edgeoneDomain: string | null
    edgeoneOrigin: string | null
    edgeoneExpectedOrigin: string | null
    edgeoneCurrentTarget: string | null
    edgeoneExpectedTarget: string | null
    edgeoneCurrentSourceKind: string | null
    edgeoneExpectedSourceKind: string | null
    edgeoneCurrentOriginGroupId: string | null
    edgeoneExpectedOriginGroupId: string | null
    haSourceDefaults: {
      sourceKind: string
      directOriginScheme: string | null
      directOriginHost: string | null
      directOriginPort: number | null
      originGroupId: string | null
      target: string | null
    } | null
    haSourceOverride: {
      sourceKind: string
      directOriginScheme: string | null
      directOriginHost: string | null
      directOriginPort: number | null
      originGroupId: string | null
      target: string | null
    } | null
    haSourceEffective: {
      sourceKind: string
      directOriginScheme: string | null
      directOriginHost: string | null
      directOriginPort: number | null
      originGroupId: string | null
      target: string | null
    } | null
    edgeoneApiConfigured: boolean
    lastEdgeoneCheckAt: number | null
    lastSyncAt: number | null
    syncLagSeconds: number | null
    recoveryStatus: string | null
    message: string | null
  }
}

function jsonResponse(data: unknown, init?: ResponseInit): Response {
  return new Response(JSON.stringify(data), {
    ...init,
    headers: { 'Content-Type': 'application/json', ...(init?.headers || {}) },
  })
}

export function createDemoHaStatus(nowSeconds: (offset?: number) => number): DemoHaState['haStatus'] {
  const defaultSource = {
    sourceKind: 'direct',
    directOriginScheme: 'https',
    directOriginHost: '203.0.113.9',
    directOriginPort: 58087,
    originGroupId: null,
    target: '203.0.113.9:58087',
  }
  return {
    mode: 'active_standby',
    nodeId: 'demo-standby',
    nodePublicOrigin: '203.0.113.10:58087',
    role: 'provisional_master',
    degraded: true,
    allowsBasicBusiness: true,
    allowsFullWrites: false,
    edgeoneDomain: 'api.example.com',
    edgeoneOrigin: '203.0.113.10:58087',
    edgeoneExpectedOrigin: '203.0.113.9:58087',
    edgeoneCurrentTarget: '203.0.113.10:58087',
    edgeoneExpectedTarget: '203.0.113.9:58087',
    edgeoneCurrentSourceKind: 'direct',
    edgeoneExpectedSourceKind: 'direct',
    edgeoneCurrentOriginGroupId: null,
    edgeoneExpectedOriginGroupId: null,
    haSourceDefaults: defaultSource,
    haSourceOverride: null,
    haSourceEffective: defaultSource,
    edgeoneApiConfigured: true,
    lastEdgeoneCheckAt: nowSeconds(-10),
    lastSyncAt: nowSeconds(-8),
    syncLagSeconds: 8,
    recoveryStatus: null,
    message: 'demo failover is waiting for administrator finalize',
  }
}

export function handleDemoHaRoute(
  path: string,
  method: string,
  state: DemoHaState,
  body: Record<string, unknown>,
): Response | null {
  if (path === '/api/ha/status' || path === '/api/admin/ha/status') return jsonResponse(state.haStatus)
  if (path === '/api/admin/ha/promote' && method === 'POST') {
    state.haStatus = {
      ...state.haStatus,
      role: 'provisional_master',
      allowsBasicBusiness: true,
      allowsFullWrites: false,
      edgeoneOrigin: state.haStatus.nodePublicOrigin,
      edgeoneCurrentTarget: state.haStatus.nodePublicOrigin,
      message: 'demo promote completed; finalize required',
    }
    return jsonResponse(state.haStatus)
  }
  if (path === '/api/admin/ha/finalize' && method === 'POST') {
    state.haStatus = {
      ...state.haStatus,
      role: 'full_master',
      degraded: false,
      allowsBasicBusiness: true,
      allowsFullWrites: true,
      message: 'demo failover finalized',
    }
    return jsonResponse(state.haStatus)
  }
  if (path === '/api/admin/ha/source' && method === 'PUT') {
    const next = body.sourceKind === 'origin_group'
      ? {
          sourceKind: 'origin_group',
          directOriginScheme: null,
          directOriginHost: null,
          directOriginPort: null,
          originGroupId: typeof body.originGroupId === 'string' && body.originGroupId.trim() ? body.originGroupId.trim() : 'eo-group-demo',
          target: typeof body.originGroupId === 'string' && body.originGroupId.trim() ? body.originGroupId.trim() : 'eo-group-demo',
        }
      : {
          sourceKind: 'direct',
          directOriginScheme: (body.directOriginScheme as string | null) ?? 'https',
          directOriginHost: (body.directOriginHost as string | null) ?? '203.0.113.9',
          directOriginPort: (body.directOriginPort as number | null) ?? 58087,
          originGroupId: null,
          target: '203.0.113.9:58087',
        }
    state.haStatus = {
      ...state.haStatus,
      edgeoneExpectedOrigin: next.target,
      edgeoneExpectedTarget: next.target,
      edgeoneExpectedSourceKind: next.sourceKind,
      edgeoneExpectedOriginGroupId: next.originGroupId,
      haSourceDefaults: next,
      haSourceOverride: next,
      haSourceEffective: next,
      message: `demo source settings saved (${next.sourceKind})`,
    }
    return jsonResponse(state.haStatus)
  }
  return null
}
