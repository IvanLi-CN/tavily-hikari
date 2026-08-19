import { afterEach, describe, expect, it, mock } from 'bun:test'

import {
  fetchAdminHaStatus,
  fetchPublicHaStatus,
  finalizeHaFailover,
  promoteHaNode,
  updateAdminHaSourceSettings,
} from './runtime'
import { normalizeHaStatus, type HaStatusWire } from './haStatus'

const originalFetch = globalThis.fetch

const legacyStatus: HaStatusWire = {
  mode: 'active_standby',
  nodeId: 'node-a',
  nodePublicOrigin: 'node-a.example:443',
  role: 'full_master',
  degraded: false,
  allowsBasicBusiness: true,
  allowsFullWrites: true,
  edgeoneDomain: 'api.example.com',
  edgeoneOrigin: 'node-a.example:443',
  edgeoneExpectedOrigin: null,
  edgeoneCurrentTarget: 'node-a.example:443',
  edgeoneExpectedTarget: null,
  edgeoneCurrentSourceKind: 'direct',
  edgeoneExpectedSourceKind: null,
  edgeoneCurrentOriginGroupId: null,
  edgeoneExpectedOriginGroupId: null,
  haSourceDefaults: null,
  haSourceOverride: null,
  haSourceEffective: null,
  edgeoneApiConfigured: true,
  lastEdgeoneCheckAt: 1_700_000_000,
  lastSyncAt: 1_700_000_000,
  syncLagSeconds: 0,
  recoveryStatus: null,
  message: null,
  peerNodes: [
    {
      nodeId: 'node-b',
      publicOrigin: 'node-b.example:443',
      sourceConfigTarget: 'node-b.internal:58087',
      role: 'standby',
      allowsBasicBusiness: false,
      allowsFullWrites: false,
      lastSyncAt: 1_700_000_000,
      syncLagSeconds: 2,
      recoveryStatus: null,
      message: null,
      lastSeenAt: 1_700_000_000,
      stale: false,
      roleHint: 'standby_candidate',
      plannedCutoverEligible: true,
    },
  ],
  plannedCutoverEligible: false,
}

afterEach(() => {
  globalThis.fetch = originalFetch
})

describe('HA status normalization', () => {
  it('supports rolling upgrades from responses without topology diagnostics', () => {
    const normalized = normalizeHaStatus(legacyStatus)

    expect(normalized.dualActiveEnabled).toBe(false)
    expect(normalized.fullMasterNodeId).toBeNull()
    expect(normalized.peerCount).toBe(legacyStatus.peerNodes.length)
    expect(normalized.syncDisabledReason).toBeNull()
  })

  it('preserves current topology diagnostics', () => {
    const normalized = normalizeHaStatus({
      ...legacyStatus,
      dualActiveEnabled: true,
      fullMasterNodeId: 'node-a',
      peerCount: 3,
      syncDisabledReason: 'no_configured_peers',
    })

    expect(normalized).toMatchObject({
      dualActiveEnabled: true,
      fullMasterNodeId: 'node-a',
      peerCount: 3,
      syncDisabledReason: 'no_configured_peers',
    })
  })

  it('normalizes every HA status API response', async () => {
    const fetchMock = mock(() => Promise.resolve(new Response(JSON.stringify(legacyStatus), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    })))
    globalThis.fetch = fetchMock as typeof fetch

    const statuses = await Promise.all([
      fetchAdminHaStatus(),
      fetchPublicHaStatus(),
      updateAdminHaSourceSettings({ sourceKind: 'direct' }),
      promoteHaNode(),
      finalizeHaFailover(),
    ])

    expect(statuses).toHaveLength(5)
    for (const status of statuses) {
      expect(status.peerCount).toBe(1)
      expect(status.dualActiveEnabled).toBe(false)
      expect(status.fullMasterNodeId).toBeNull()
      expect(status.syncDisabledReason).toBeNull()
    }
  })
})
