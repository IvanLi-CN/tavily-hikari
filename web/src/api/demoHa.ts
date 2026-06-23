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
    peerNodes: Array<{
      nodeId: string
      publicOrigin: string | null
      role: string | null
      allowsBasicBusiness: boolean
      allowsFullWrites: boolean
      lastSyncAt: number | null
      syncLagSeconds: number | null
      recoveryStatus: string | null
      message: string | null
      lastSeenAt: number | null
      stale: boolean
      roleHint: 'standby_candidate' | 'observer'
      plannedCutoverEligible: boolean
    }>
    plannedCutoverEligible: boolean
  }
  nodeInteractions?: Record<string, Array<{
    id: number
    eventKind: string
    category: 'planned_cutover' | 'manual_failover' | 'edgeone' | 'peer' | 'sync' | 'recovery' | 'role'
    status: 'info' | 'running' | 'success' | 'warning' | 'error'
    nodeId: string | null
    operationId: string | null
    summary: string
    detail: string | null
    technicalDetails: Record<string, unknown> | null
    createdAt: number
  }>>
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
    directOriginHost: 'ha-active.internal.example.net',
    directOriginPort: 58087,
    originGroupId: null,
    target: 'ha-active.internal.example.net:58087',
  }
  return {
    mode: 'active_standby',
    nodeId: 'demo-active',
    nodePublicOrigin: 'ha-active.internal.example.net:58087',
    role: 'full_master',
    degraded: false,
    allowsBasicBusiness: true,
    allowsFullWrites: true,
    edgeoneDomain: 'api.example.com',
    edgeoneOrigin: 'ha-active.internal.example.net:58087',
    edgeoneExpectedOrigin: 'ha-active.internal.example.net:58087',
    edgeoneCurrentTarget: 'ha-active.internal.example.net:58087',
    edgeoneExpectedTarget: 'ha-active.internal.example.net:58087',
    edgeoneCurrentSourceKind: 'direct',
    edgeoneExpectedSourceKind: 'direct',
    edgeoneCurrentOriginGroupId: null,
    edgeoneExpectedOriginGroupId: null,
    haSourceDefaults: defaultSource,
    haSourceOverride: null,
    haSourceEffective: defaultSource,
    edgeoneApiConfigured: true,
    lastEdgeoneCheckAt: nowSeconds(-8),
    lastSyncAt: nowSeconds(-4),
    syncLagSeconds: 4,
    recoveryStatus: null,
    message: 'full master is ready to drain traffic for planned maintenance',
    peerNodes: [
      {
        nodeId: 'demo-standby',
        publicOrigin: 'ha-standby.internal.example.net:58087',
        role: 'standby',
        allowsBasicBusiness: true,
        allowsFullWrites: false,
        lastSyncAt: nowSeconds(-5),
        syncLagSeconds: 5,
        recoveryStatus: null,
        message: 'standby probe is healthy and synced within cutover threshold',
        lastSeenAt: nowSeconds(-5),
        stale: false,
        roleHint: 'standby_candidate',
        plannedCutoverEligible: true,
      },
      {
        nodeId: 'demo-observer-a',
        publicOrigin: 'observer-a.internal.example.net:58087',
        role: 'standby',
        allowsBasicBusiness: true,
        allowsFullWrites: false,
        lastSyncAt: nowSeconds(-42),
        syncLagSeconds: 42,
        recoveryStatus: null,
        message: 'observer tracks replication only; lag is above the cutover threshold',
        lastSeenAt: nowSeconds(-4),
        stale: false,
        roleHint: 'observer',
        plannedCutoverEligible: false,
      },
      {
        nodeId: 'demo-observer-b',
        publicOrigin: 'observer-b.internal.example.net:58087',
        role: null,
        allowsBasicBusiness: false,
        allowsFullWrites: false,
        lastSyncAt: null,
        syncLagSeconds: null,
        recoveryStatus: null,
        message: 'observer probe timed out during the latest sweep',
        lastSeenAt: nowSeconds(-186),
        stale: true,
        roleHint: 'observer',
        plannedCutoverEligible: false,
      },
    ],
    plannedCutoverEligible: false,
  }
}

export function handleDemoHaRoute(
  path: string,
  method: string,
  state: DemoHaState,
  body: Record<string, unknown>,
): Response | null {
  const nowSeconds = (offset = 0) => Math.floor(Date.now() / 1000) + offset
  const nodeInteractions = state.nodeInteractions ?? {
    'demo-standby': [
      {
        id: 42,
        eventKind: 'planned_cutover_started',
        category: 'planned_cutover',
        status: 'running',
        nodeId: 'demo-standby',
        operationId: 'planned-cutover-demo-42',
        summary: 'demo-active started planned cutover to demo-standby',
        detail: 'Current node switched the EdgeOne route and is waiting for demo-standby to finalize.',
        technicalDetails: {
          fromNodeId: 'demo-active',
          targetNodeId: 'demo-standby',
          routeBefore: 'ha-active.internal.example.net:58087',
        },
        createdAt: nowSeconds(-1800),
      },
      {
        id: 41,
        eventKind: 'edgeone_modifyaccelerationdomain',
        category: 'edgeone',
        status: 'success',
        nodeId: null,
        operationId: 'planned-cutover-demo-42',
        summary: 'EdgeOne ModifyAccelerationDomain switched traffic to demo-standby',
        detail: 'The control plane updated the effective source to the standby target route.',
        technicalDetails: {
          domain: 'api.example.com',
          effectiveTarget: 'ha-standby.internal.example.net:58087',
        },
        createdAt: nowSeconds(-1805),
      },
      {
        id: 40,
        eventKind: 'planned_cutover_finalize',
        category: 'planned_cutover',
        status: 'success',
        nodeId: 'demo-standby',
        operationId: 'planned-cutover-demo-42',
        summary: 'demo-standby finalized to full_master',
        detail: 'The standby node completed provisional_master -> full_master without manual login.',
        technicalDetails: {
          role: 'full_master',
          currentNodeId: 'demo-active',
        },
        createdAt: nowSeconds(-1790),
      },
      {
        id: 39,
        eventKind: 'planned_cutover_succeeded',
        category: 'planned_cutover',
        status: 'success',
        nodeId: 'demo-standby',
        operationId: 'planned-cutover-demo-42',
        summary: 'demo-active and demo-standby completed planned cutover',
        detail: 'demo-standby is now serving traffic and demo-active entered recovery.',
        technicalDetails: {
          targetNodeId: 'demo-standby',
          localRoleAfter: 'recovery',
        },
        createdAt: nowSeconds(-1785),
      },
    ],
    'demo-observer-a': [
      {
        id: 32,
        eventKind: 'sync_lag_threshold_exceeded',
        category: 'sync',
        status: 'warning',
        nodeId: 'demo-observer-a',
        operationId: null,
        summary: 'demo-observer-a is above the sync lag threshold',
        detail: 'The node is still visible to the current node but only remains observation-only.',
        technicalDetails: {
          syncLagSeconds: 42,
          currentNodeId: 'demo-active',
        },
        createdAt: nowSeconds(-600),
      },
    ],
    'demo-observer-b': [
      {
        id: 28,
        eventKind: 'peer_probe_failed',
        category: 'peer',
        status: 'error',
        nodeId: 'demo-observer-b',
        operationId: null,
        summary: 'The current node could not reach demo-observer-b',
        detail: 'The latest peer probe from demo-active timed out and the observer is marked stale.',
        technicalDetails: {
          currentNodeId: 'demo-active',
          stale: true,
        },
        createdAt: nowSeconds(-240),
      },
    ],
  }
  if (path === '/api/ha/status' || path === '/api/admin/ha/status') return jsonResponse(state.haStatus)
  if (path.startsWith('/api/admin/ha/nodes/')) {
    const nodeId = decodeURIComponent(path.slice('/api/admin/ha/nodes/'.length))
    const node = state.haStatus.peerNodes.find((peer) => peer.nodeId === nodeId)
    if (!node) {
      return jsonResponse({ detail: `unknown HA peer node: ${nodeId}` }, { status: 404 })
    }
      return jsonResponse({
        currentNodeId: state.haStatus.nodeId,
        node,
        edgeoneDomain: state.haStatus.edgeoneDomain,
        edgeoneCurrentTarget: state.haStatus.edgeoneCurrentTarget,
        edgeoneCurrentSourceKind: state.haStatus.edgeoneCurrentSourceKind,
        haSourceEffective: state.haStatus.haSourceEffective,
        timeline: {
          events: nodeInteractions[nodeId] ?? [],
          nextCursor: null,
        },
      })
  }
  if (path === '/api/admin/ha/timeline') {
    return jsonResponse({
      events: [
        {
          id: 24,
          eventKind: 'sync_lag_cleared',
          category: 'sync',
          status: 'success',
          nodeId: 'demo-standby',
          operationId: null,
          summary: 'demo-standby returned within the 30 second sync lag threshold',
          detail: 'Replication lag cleared, so planned cutover became eligible again.',
          technicalDetails: { nodeId: 'demo-standby', syncLagSeconds: 5 },
          createdAt: nowSeconds(-180),
        },
        {
          id: 18,
          eventKind: 'peer_probe_failed',
          category: 'peer',
          status: 'error',
          nodeId: 'demo-observer-b',
          operationId: null,
          summary: 'observer probe failed for demo-observer-b',
          detail: 'The observer node missed the latest control-plane probe and is marked stale.',
          technicalDetails: { nodeId: 'demo-observer-b', stale: true },
          createdAt: nowSeconds(-120),
        },
        {
          id: 12,
          eventKind: 'planned_cutover_ready',
          category: 'planned_cutover',
          status: 'success',
          nodeId: 'demo-active',
          operationId: 'planned-cutover-demo-12',
          summary: 'demo-standby passed planned cutover prechecks',
          detail: 'The standby candidate is fresh, synchronized, and can receive maintenance cutover.',
          technicalDetails: { targetNodeId: 'demo-standby', currentRoute: 'ha-active.internal.example.net:58087' },
          createdAt: nowSeconds(-60),
        },
      ],
      nextCursor: null,
    })
  }
  if (path === '/api/admin/ha/planned-cutover' && method === 'POST') {
    state.haStatus = {
      ...state.haStatus,
      role: 'recovery',
      degraded: true,
      allowsBasicBusiness: false,
      allowsFullWrites: false,
      edgeoneOrigin: '203.0.113.10:58087',
      edgeoneCurrentTarget: '203.0.113.10:58087',
      recoveryStatus: 'planned cutover completed; import recovery data from old master',
      message: 'demo planned cutover completed',
      peerNodes: [
        {
          nodeId: 'demo-standby',
          publicOrigin: '203.0.113.10:58087',
          role: 'full_master',
          allowsBasicBusiness: true,
          allowsFullWrites: true,
          lastSyncAt: nowSeconds(-2),
          syncLagSeconds: 2,
          recoveryStatus: null,
          message: 'demo standby is now serving full traffic',
          lastSeenAt: nowSeconds(-1),
          stale: false,
          roleHint: 'standby_candidate',
          plannedCutoverEligible: false,
        },
      ],
      plannedCutoverEligible: false,
    }
    return jsonResponse({ operationId: 'planned-cutover-demo', status: 'success' })
  }
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
