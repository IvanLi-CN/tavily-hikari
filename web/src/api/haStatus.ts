import type { HaMode, HaNodeRole, HaPeerNode, HaSourceKind, HaSourceSettings } from './runtime'

export interface HaStatus {
  mode: HaMode
  nodeId: string
  nodePublicOrigin: string | null
  role: HaNodeRole
  dualActiveEnabled: boolean
  fullMasterNodeId: string | null
  degraded: boolean
  allowsBasicBusiness: boolean
  allowsFullWrites: boolean
  edgeoneDomain: string | null
  edgeoneOrigin: string | null
  edgeoneExpectedOrigin: string | null
  edgeoneCurrentTarget: string | null
  edgeoneExpectedTarget: string | null
  edgeoneCurrentSourceKind: HaSourceKind | null
  edgeoneExpectedSourceKind: HaSourceKind | null
  edgeoneCurrentOriginGroupId: string | null
  edgeoneExpectedOriginGroupId: string | null
  haSourceDefaults: HaSourceSettings | null
  haSourceOverride: HaSourceSettings | null
  haSourceEffective: HaSourceSettings | null
  edgeoneApiConfigured: boolean
  lastEdgeoneCheckAt: number | null
  lastSyncAt: number | null
  syncLagSeconds: number | null
  recoveryStatus: string | null
  message: string | null
  peerCount: number
  syncDisabledReason: string | null
  peerNodes: HaPeerNode[]
  plannedCutoverEligible: boolean
}

type TopologyDiagnosticKey =
  | 'dualActiveEnabled'
  | 'fullMasterNodeId'
  | 'peerCount'
  | 'syncDisabledReason'

export type HaStatusWire = Omit<HaStatus, TopologyDiagnosticKey>
  & Partial<Pick<HaStatus, TopologyDiagnosticKey>>

export function normalizeHaStatus(status: HaStatusWire): HaStatus {
  return {
    ...status,
    dualActiveEnabled: status.dualActiveEnabled ?? false,
    fullMasterNodeId: status.fullMasterNodeId ?? null,
    peerCount: status.peerCount ?? status.peerNodes.length,
    syncDisabledReason: status.syncDisabledReason ?? null,
  }
}
