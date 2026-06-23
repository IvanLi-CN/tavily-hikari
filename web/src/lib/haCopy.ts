import type { HaPeerNode, HaStatus, HaTimelineEvent } from '../api'
import type { AdminTranslations } from '../i18n'

type HaStrings = AdminTranslations['systemSettings']['ha']

function nodeLabel(nodeId: string | null | undefined): string {
  return nodeId?.trim() ? nodeId : '—'
}

function currentNodeLabel(
  event: HaTimelineEvent,
): string {
  const currentNodeId =
    typeof event.technicalDetails?.currentNodeId === 'string'
      ? event.technicalDetails.currentNodeId
      : typeof event.technicalDetails?.fromNodeId === 'string'
        ? event.technicalDetails.fromNodeId
        : event.nodeId
  return nodeLabel(currentNodeId)
}

function targetLabel(details: Record<string, unknown> | null | undefined): string {
  const value = details?.targetNodeId
  if (typeof value === 'string' && value.trim()) return value
  const nodeValue = details?.nodeId
  if (typeof nodeValue === 'string' && nodeValue.trim()) return nodeValue
  return routeLabel(details)
}

function routeLabel(details: Record<string, unknown> | null | undefined): string {
  const value = details?.currentRoute ?? details?.routeBefore ?? details?.effectiveTarget ?? details?.toTarget
  return typeof value === 'string' && value.trim() ? value : '—'
}

function syncLagLabel(details: Record<string, unknown> | null | undefined, fallback?: number | null): string {
  const value = details?.syncLagSeconds
  if (typeof value === 'number' && Number.isFinite(value)) return `${value}`
  if (typeof fallback === 'number' && Number.isFinite(fallback)) return `${fallback}`
  return '—'
}

export function formatHaTimelineStatusLabel(
  status: HaTimelineEvent['status'],
  strings: HaStrings,
): string {
  if (status === 'running') return strings.timelineStatusRunning
  if (status === 'success') return strings.timelineStatusSuccess
  if (status === 'warning') return strings.timelineStatusWarning
  if (status === 'error') return strings.timelineStatusError
  return strings.timelineStatusInfo
}

export function formatHaTimelineSummary(
  event: HaTimelineEvent,
  strings: HaStrings,
  options?: { currentNodeId?: string | null },
): string {
  const currentNode = nodeLabel(options?.currentNodeId) !== '—'
    ? nodeLabel(options?.currentNodeId)
    : currentNodeLabel(event)
  const targetNode = targetLabel(event.technicalDetails) === '—'
    ? nodeLabel(event.nodeId)
    : targetLabel(event.technicalDetails)
  const eventNode = nodeLabel(event.nodeId)

  switch (event.eventKind) {
    case 'planned_cutover_ready':
      return strings.timelineSummaryPlannedCutoverReady
        .replace('{targetNodeId}', targetNode)
        .replace('{currentNodeId}', currentNode)
    case 'planned_cutover_started':
      return strings.timelineSummaryPlannedCutoverStarted
        .replace('{currentNodeId}', currentNode)
        .replace('{targetNodeId}', targetNode)
    case 'planned_cutover_finalize':
      return strings.timelineSummaryPlannedCutoverFinalize.replace('{targetNodeId}', targetNode)
    case 'planned_cutover_succeeded':
      return strings.timelineSummaryPlannedCutoverSucceeded
        .replace('{currentNodeId}', currentNode)
        .replace('{targetNodeId}', targetNode)
    case 'planned_cutover_rejected_stale':
      return strings.timelineSummaryPlannedCutoverRejectedStale.replace('{targetNodeId}', targetNode)
    case 'planned_cutover_rejected_precheck':
      return strings.timelineSummaryPlannedCutoverRejectedPrecheck.replace('{targetNodeId}', targetNode)
    case 'manual_promote':
      return strings.timelineSummaryManualPromote.replace('{currentNodeId}', currentNode)
    case 'manual_finalize':
      return strings.timelineSummaryManualFinalize.replace('{currentNodeId}', currentNode)
    case 'edgeone_modifyaccelerationdomain':
    case 'edgeone_modify_acceleration_domain':
      return strings.timelineSummaryEdgeoneSwitched.replace('{targetNodeId}', targetNode)
    case 'peer_probe_failed':
      return strings.timelineSummaryPeerProbeFailed.replace('{nodeId}', eventNode)
    case 'sync_lag_threshold_exceeded':
      return strings.timelineSummarySyncLagExceeded.replace('{nodeId}', eventNode)
    case 'sync_lag_cleared':
      return strings.timelineSummarySyncLagCleared.replace('{nodeId}', eventNode)
    case 'recovery_import':
      return strings.timelineSummaryRecoveryImport.replace('{currentNodeId}', currentNode)
    default:
      return event.summary
  }
}

export function formatHaTimelineDetail(
  event: HaTimelineEvent,
  strings: HaStrings,
  options?: { currentNodeId?: string | null },
): string | null {
  const currentNode = nodeLabel(options?.currentNodeId) !== '—'
    ? nodeLabel(options?.currentNodeId)
    : currentNodeLabel(event)
  const targetNode = targetLabel(event.technicalDetails) === '—'
    ? nodeLabel(event.nodeId)
    : targetLabel(event.technicalDetails)
  const route = routeLabel(event.technicalDetails)
  const eventNode = nodeLabel(event.nodeId)
  const syncLag = syncLagLabel(event.technicalDetails)

  switch (event.eventKind) {
    case 'planned_cutover_ready':
      return strings.timelineDetailPlannedCutoverReady
        .replace('{targetNodeId}', targetNode)
        .replace('{currentRoute}', route)
    case 'planned_cutover_started':
      return strings.timelineDetailPlannedCutoverStarted
        .replace('{targetNodeId}', targetNode)
        .replace('{currentNodeId}', currentNode)
    case 'planned_cutover_finalize':
      return strings.timelineDetailPlannedCutoverFinalize.replace('{targetNodeId}', targetNode)
    case 'planned_cutover_succeeded':
      return strings.timelineDetailPlannedCutoverSucceeded
        .replace('{targetNodeId}', targetNode)
        .replace('{currentNodeId}', currentNode)
    case 'planned_cutover_rejected_stale':
      return strings.timelineDetailPlannedCutoverRejectedStale.replace('{targetNodeId}', targetNode)
    case 'planned_cutover_rejected_precheck':
      return strings.timelineDetailPlannedCutoverRejectedPrecheck
        .replace('{targetNodeId}', targetNode)
        .replace('{syncLagSeconds}', syncLag)
    case 'manual_promote':
      return strings.timelineDetailManualPromote.replace('{currentNodeId}', currentNode)
    case 'manual_finalize':
      return strings.timelineDetailManualFinalize.replace('{currentNodeId}', currentNode)
    case 'edgeone_modifyaccelerationdomain':
    case 'edgeone_modify_acceleration_domain':
      return strings.timelineDetailEdgeoneSwitched.replace('{targetNodeId}', targetNode)
    case 'peer_probe_failed':
      return strings.timelineDetailPeerProbeFailed
        .replace('{nodeId}', eventNode)
        .replace('{currentNodeId}', currentNode)
    case 'sync_lag_threshold_exceeded':
      return strings.timelineDetailSyncLagExceeded
        .replace('{nodeId}', eventNode)
        .replace('{syncLagSeconds}', syncLag)
    case 'sync_lag_cleared':
      return strings.timelineDetailSyncLagCleared.replace('{nodeId}', eventNode)
    case 'recovery_import':
      return strings.timelineDetailRecoveryImport.replace('{currentNodeId}', currentNode)
    default:
      return event.detail
  }
}

export function formatHaStatusMessage(
  status: Pick<HaStatus, 'role' | 'message' | 'edgeoneCurrentSourceKind'>,
  strings: HaStrings,
): string | null {
  if (!status.message) return null
  switch (status.message) {
    case 'standby is synchronized and ready for manual promotion':
      return strings.messageStandbyManualPromoteReady
    case 'promoted by EdgeOne origin switch; finalize required':
    case 'EdgeOne origin now points to this node; finalize required':
      return strings.messageFinalizeRequired
    case 'node is serving as active master':
      return strings.messageNodeServingActiveMaster
    case 'node is serving through an EdgeOne origin group':
      return strings.messageNodeServingOriginGroup
    case 'node is serving through a direct IP/domain origin':
      return strings.messageNodeServingDirectOrigin
    case 'full master is ready to drain traffic for planned maintenance':
      return strings.messageFullMasterMaintenanceReady
    case 'planned cutover is in progress':
      return strings.messagePlannedCutoverInProgress
    case 'demo promote completed; finalize required':
      return strings.messageDemoPromoteFinalizeRequired
    case 'demo failover finalized':
      return strings.messageDemoFailoverFinalized
    case 'demo planned cutover completed':
      return strings.messageDemoPlannedCutoverCompleted
    default:
      if (
        status.message.startsWith('demo source settings saved (')
        && status.message.endsWith(')')
      ) {
        const sourceKind = status.message.slice(
          'demo source settings saved ('.length,
          -1,
        )
        const sourceLabel =
          sourceKind === 'origin_group' ? strings.sourceKindOriginGroup : strings.sourceKindDirect
        return strings.messageDemoSourceSaved.replace('{sourceKind}', sourceLabel)
      }
      if (
        status.message.startsWith('EdgeOne origin moved to ')
        && status.message.endsWith('; recovery import required')
      ) {
        const target = status.message.slice(
          'EdgeOne origin moved to '.length,
          -'; recovery import required'.length,
        )
        return strings.messageRecoveryImportRequired.replace('{target}', target)
      }
      if (status.message === 'EdgeOne origin switched to the configured source') {
        return strings.messageEdgeoneSwitchedToConfiguredSource
      }
      if (status.message === 'EdgeOne origin switched to the configured source; finalize required') {
        return strings.messageEdgeoneSwitchedFinalizeRequired
      }
      return status.message
  }
}

export function formatHaPeerMessage(
  peer: Pick<
    HaPeerNode,
    'nodeId' | 'message' | 'syncLagSeconds' | 'plannedCutoverEligible' | 'stale' | 'recoveryStatus'
  >,
  strings: HaStrings,
): string | null {
  if (!peer.message) return null
  switch (peer.message) {
    case 'standby is synchronized and ready':
    case 'standby probe is healthy and synced within cutover threshold':
    case 'standby is synchronized and ready for maintenance cutover':
      return strings.messageStandbyHealthyCutoverReady
    case 'active node is serving traffic':
      return strings.messagePeerServingTraffic
    case 'peer status probe is older than 30 seconds':
      return strings.messagePeerProbeStale
    case 'peer internal status endpoint is unreachable':
      return strings.messagePeerUnreachable
    case 'observer probe timed out during the latest sweep':
      return strings.messageObserverProbeTimedOut
    case 'observer tracks replication only; lag is above the cutover threshold':
      return strings.messageObserverLagHigh
    case 'sync lag exceeds 30 seconds threshold':
      return strings.messageSyncLagExceeded
    case 'peer is waiting for finalize':
      return strings.messagePeerWaitingFinalize
    case 'planned cutover completed; node-b now serves traffic':
      return strings.messagePlannedCutoverCompletedServing.replace('{nodeId}', peer.nodeId)
    case 'demo standby is now serving full traffic':
      return strings.messagePeerServingFullTraffic.replace('{nodeId}', peer.nodeId)
    default:
      return peer.message
  }
}

export function formatHaRecoveryStatus(
  value: string | null | undefined,
  strings: HaStrings,
): string | null | undefined {
  if (!value) return value
  if (value === 'peer unreachable') return strings.recoveryPeerUnreachable
  if (value === 'planned cutover completed; import recovery data from old master') {
    return strings.recoveryImportFromOldMaster
  }
  if (value.startsWith('importing ')) {
    return strings.recoveryImportingBatch.replace('{batchId}', value.slice('importing '.length))
  }
  if (
    value.startsWith('EdgeOne target moved to ')
    && value.endsWith('; recovery import required')
  ) {
    const target = value.slice(
      'EdgeOne target moved to '.length,
      -'; recovery import required'.length,
    )
    return strings.recoveryTargetMovedImportRequired.replace('{target}', target)
  }
  return value
}
