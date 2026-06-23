import { ArrowRight, CircleAlert, Crown, RotateCcw, Server, ShieldCheck } from 'lucide-react'

import type { HaStatus, HaTimelineEvent } from '../api'
import type { AdminTranslations } from '../i18n'
import { useLanguage, useTranslate } from '../i18n'
import {
  formatHaPeerMessage,
  formatHaRecoveryStatus,
  formatHaStatusMessage,
  formatHaTimelineDetail,
  formatHaTimelineStatusLabel,
  formatHaTimelineSummary,
} from '../lib/haCopy'
import { Button } from './ui/button'
import { StatusBadge, type StatusTone } from './StatusBadge'

interface HaStatusBannerProps {
  status: HaStatus | null
  audience: 'admin' | 'user'
  strings?: AdminTranslations['systemSettings']['ha']
  language?: 'en' | 'zh'
  adminVariant?: 'panel' | 'compact'
  onConfigureSource?: () => void
  onPromote?: () => void
  onFinalize?: () => void
  onPlannedCutover?: (targetNodeId: string) => void
  busy?: boolean
  compactHref?: string
  compactTitle?: string
  compactDescription?: string
  compactActionLabel?: string
  onCompactClick?: () => void
  onOpenNodeDetails?: (nodeId: string) => void
  timeline?: HaTimelineEvent[]
  timelineLoading?: boolean
  onLoadMoreTimeline?: (() => void) | null
  hasMoreTimeline?: boolean
}

function localeFor(language: 'en' | 'zh'): string {
  return language === 'zh' ? 'zh-CN' : 'en-US'
}

function formatTimestamp(value: number | null, language: 'en' | 'zh'): string {
  if (value == null) return '—'
  return new Intl.DateTimeFormat(localeFor(language), {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  }).format(new Date(value * 1000))
}

function formatCompactTimestamp(value: number | null, language: 'en' | 'zh'): string {
  if (value == null) return '—'
  return new Intl.DateTimeFormat(localeFor(language), {
    month: language === 'zh' ? 'numeric' : 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  }).format(new Date(value * 1000))
}

function formatDateTimeAttr(value: number | null): string | undefined {
  if (value == null) return undefined
  return new Date(value * 1000).toISOString()
}

function formatLag(value: number | null, language: 'en' | 'zh'): string {
  if (value == null) return '—'
  if (value < 60) return language === 'zh' ? `${value}秒` : `${value}s`
  const minutes = Math.floor(value / 60)
  const seconds = value % 60
  if (language === 'zh') return seconds === 0 ? `${minutes}分` : `${minutes}分${seconds}秒`
  return seconds === 0 ? `${minutes}m` : `${minutes}m ${seconds}s`
}

function roleLabel(role: HaStatus['role'], strings: AdminTranslations['systemSettings']['ha']): string {
  if (role === 'full_master') return strings.roleFullMaster
  if (role === 'provisional_master') return strings.roleProvisionalMaster
  if (role === 'standby') return strings.roleStandby
  return strings.roleRecovery
}

function sourceKindLabel(kind: string | null, strings: AdminTranslations['systemSettings']['ha']): string {
  if (kind === 'direct') return strings.sourceKindDirect
  if (kind === 'origin_group') return strings.sourceKindOriginGroup
  return '—'
}

interface HaNodeRow {
  key: string
  nodeId: string
  relation: string
  isLocalNode: boolean
  role: string
  origin: string
  health: string
  healthTone: StatusTone
  lastSync: string
  lastSyncTitle?: string
  lastSyncDateTime?: string
  promotedAt: string
  promotedAtTitle?: string
  promotedAtDateTime?: string
  actionKind: 'promote' | 'finalize' | 'serving' | 'blocked' | 'planned_cutover' | 'observe'
  targetNodeId?: string
}

function buildNodeRows(status: HaStatus, strings: AdminTranslations['systemSettings']['ha'], language: 'en' | 'zh'): HaNodeRow[] {
  const rows: HaNodeRow[] = [
    {
      key: 'local',
      nodeId: status.nodeId,
      relation: strings.thisAdminNodeLabel,
      isLocalNode: true,
      role: roleLabel(status.role, strings),
      origin: status.edgeoneCurrentTarget ?? status.nodePublicOrigin ?? '—',
      health:
        status.role === 'full_master'
          ? strings.healthServingWrites
          : status.role === 'provisional_master'
            ? strings.healthFinalizeRequired
            : status.role === 'standby'
              ? strings.healthReadyStandby
              : strings.healthRecoveryRequired,
      healthTone:
        status.role === 'full_master'
          ? 'success'
          : status.role === 'provisional_master'
            ? 'warning'
            : status.role === 'standby'
              ? 'info'
              : 'error',
      lastSync: formatCompactTimestamp(status.lastSyncAt, language),
      lastSyncTitle: formatTimestamp(status.lastSyncAt, language),
      lastSyncDateTime: formatDateTimeAttr(status.lastSyncAt),
      promotedAt:
        status.role === 'full_master' || status.role === 'provisional_master'
          ? formatCompactTimestamp(status.lastEdgeoneCheckAt, language)
          : '—',
      promotedAtTitle:
        status.role === 'full_master' || status.role === 'provisional_master'
          ? formatTimestamp(status.lastEdgeoneCheckAt, language)
          : undefined,
      promotedAtDateTime:
        status.role === 'full_master' || status.role === 'provisional_master'
          ? formatDateTimeAttr(status.lastEdgeoneCheckAt)
          : undefined,
      actionKind:
        status.role === 'standby'
          ? 'promote'
          : status.role === 'provisional_master'
            ? 'finalize'
            : status.role === 'full_master'
              ? 'serving'
              : 'blocked',
    },
  ]
  for (const peer of status.peerNodes ?? []) {
    const relation = peer.roleHint === 'standby_candidate'
      ? strings.relationStandbyCandidate
      : strings.relationObserver
    const healthTone: StatusTone = peer.stale
      ? 'warning'
      : peer.role === 'full_master'
        ? 'success'
        : peer.role === 'standby'
          ? 'info'
          : peer.role === 'recovery'
            ? 'error'
            : 'neutral'
    const health = peer.stale
      ? strings.healthStale
      : peer.recoveryStatus
        ? strings.healthRecoveryRequired
        : peer.plannedCutoverEligible
          ? strings.healthReadyStandby
          : peer.role === 'full_master'
            ? strings.healthServingWrites
            : formatHaPeerMessage(peer, strings)
              ?? (peer.role === 'standby' ? strings.healthConfigured : '—')
    rows.push({
      key: `peer-${peer.nodeId}`,
      nodeId: peer.nodeId,
      relation,
      isLocalNode: false,
      role: peer.role ? roleLabel(peer.role, strings) : '—',
      origin: peer.publicOrigin ?? '—',
      health,
      healthTone,
      lastSync: formatCompactTimestamp(peer.lastSyncAt, language),
      lastSyncTitle: formatTimestamp(peer.lastSyncAt, language),
      lastSyncDateTime: formatDateTimeAttr(peer.lastSyncAt),
      promotedAt: formatCompactTimestamp(peer.lastSeenAt, language),
      promotedAtTitle: formatTimestamp(peer.lastSeenAt, language),
      promotedAtDateTime: formatDateTimeAttr(peer.lastSeenAt),
      actionKind: peer.plannedCutoverEligible ? 'planned_cutover' : 'observe',
      targetNodeId: peer.nodeId,
    })
  }

  return rows
}

function adminNeedsAttention(status: HaStatus): boolean {
  return status.mode !== 'single' && (status.degraded || status.role !== 'full_master' || !status.allowsFullWrites)
}

export default function HaStatusBanner({
  status,
  audience,
  strings,
  language,
  adminVariant = 'panel',
  onConfigureSource,
  onPromote,
  onFinalize,
  onPlannedCutover,
  busy = false,
  compactHref,
  compactTitle,
  compactDescription,
  compactActionLabel,
  onCompactClick,
  timeline = [],
  timelineLoading = false,
  onLoadMoreTimeline = null,
  hasMoreTimeline = false,
  onOpenNodeDetails,
}: HaStatusBannerProps): JSX.Element | null {
  const fallbackStrings = useTranslate().admin.systemSettings.ha
  const fallbackLanguage = useLanguage().language
  const admin = audience === 'admin'
  if (!status || status.mode === 'single' || (!admin && !status.degraded)) return null
  const labels = strings ?? fallbackStrings
  const lang = language ?? fallbackLanguage

  const title =
    status.role === 'provisional_master'
      ? labels.panelTitle
      : status.role === 'standby'
        ? labels.panelTitle
        : status.role === 'recovery'
          ? labels.panelTitle
          : labels.panelTitle
  const detail =
    status.role === 'provisional_master'
      ? labels.panelDescriptionProvisionalMaster
      : status.role === 'standby'
        ? labels.panelDescriptionStandby
        : status.role === 'recovery'
          ? labels.panelDescriptionRecovery
          : labels.panelDescriptionFullMaster
  const toneClass = status.role === 'full_master' ? 'ha-status-banner-active' : ''
  const rows = buildNodeRows(status, labels, lang)
  const authorityLabel = status.allowsFullWrites
    ? labels.authorityFullWrites
    : status.allowsBasicBusiness
      ? labels.authorityBasicTraffic
      : labels.authorityWritesBlocked
  const authorityTone: StatusTone = status.allowsFullWrites ? 'success' : status.allowsBasicBusiness ? 'warning' : 'neutral'

  if (admin && adminVariant === 'compact') {
    if (!adminNeedsAttention(status)) return null
    return (
      <section className="ha-status-banner ha-status-banner-compact" role="status" aria-live="polite">
        <div className="ha-status-banner-head">
          <div className="ha-status-banner-icon" aria-hidden="true">
            <CircleAlert size={20} strokeWidth={2.4} />
          </div>
          <div className="ha-status-banner-copy">
            <div className="ha-status-banner-title">{compactTitle ?? labels.compactTitle}</div>
            <p>{compactDescription ?? labels.compactDescription}</p>
          </div>
          {compactHref && compactActionLabel && (
            <Button asChild size="sm" variant="outline" className="ha-status-banner-action">
              <a
                href={compactHref}
                onClick={(event) => {
                  if (!onCompactClick) return
                  event.preventDefault()
                  onCompactClick()
                }}
              >
                <span>{compactActionLabel}</span>
                <ArrowRight className="h-4 w-4" aria-hidden="true" />
              </a>
            </Button>
          )}
        </div>
      </section>
    )
  }

  if (admin) {
    return (
      <section className="ha-node-panel" aria-labelledby="ha-node-panel-title">
        <div className="ha-node-panel-head">
          <div className="ha-node-panel-title-group">
            <div className="ha-node-panel-kicker">{labels.panelKicker}</div>
            <h2 id="ha-node-panel-title">{title}</h2>
            <p>{detail}</p>
          </div>
          <div className="ha-node-panel-head-actions">
            {onConfigureSource ? (
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="ha-node-configure-button"
                onClick={onConfigureSource}
              >
                {labels.configureSource}
              </Button>
            ) : null}
            <StatusBadge tone={authorityTone}>{authorityLabel}</StatusBadge>
          </div>
        </div>

        <dl className="ha-status-summary" aria-label={labels.title}>
          <div><dt>{labels.summaryEdgeoneDomain}</dt><dd>{status.edgeoneDomain ?? '—'}</dd></div>
          <div><dt>{labels.summaryCurrentOrigin}</dt><dd>{status.edgeoneCurrentTarget ?? status.edgeoneOrigin ?? '—'}</dd></div>
          <div><dt>{labels.summaryExpectedOrigin}</dt><dd>{status.edgeoneExpectedOrigin ?? '—'}</dd></div>
          <div><dt>{labels.summaryCurrentSource}</dt><dd>{sourceKindLabel(status.edgeoneCurrentSourceKind, labels)}</dd></div>
          <div><dt>{labels.summaryExpectedSource}</dt><dd>{sourceKindLabel(status.edgeoneExpectedSourceKind, labels)}</dd></div>
          <div><dt>{labels.summarySyncLag}</dt><dd>{formatLag(status.syncLagSeconds, lang)}</dd></div>
          <div><dt>{labels.summaryEdgeoneApi}</dt><dd>{status.edgeoneApiConfigured ? labels.healthServingEdgeone : labels.healthNotRouted}</dd></div>
          <div><dt>{labels.summaryRecovery}</dt><dd>{formatHaRecoveryStatus(status.recoveryStatus, labels) ?? '—'}</dd></div>
        </dl>

        <div className="ha-node-list" aria-label={labels.nodeInventoryTitle}>
          <div className="ha-node-list-title">
            <Server size={18} aria-hidden="true" />
            <span>{labels.nodeInventoryTitle}</span>
          </div>
          <div className="ha-node-grid" role="table" aria-label={labels.nodeInventoryTitle}>
            <div className="ha-node-grid-row ha-node-grid-head" role="row">
              <div className="ha-node-cell ha-node-cell--identity" role="columnheader">{labels.nodeHeader}</div>
              <div className="ha-node-cell ha-node-cell--role" role="columnheader">{labels.roleHeader}</div>
              <div className="ha-node-cell ha-node-cell--origin" role="columnheader">{labels.originHeader}</div>
              <div className="ha-node-cell ha-node-cell--health" role="columnheader">{labels.healthHeader}</div>
              <div className="ha-node-cell ha-node-cell--time" role="columnheader">{labels.lastSyncHeader}</div>
              <div className="ha-node-cell ha-node-cell--time" role="columnheader">{labels.promotedAtHeader}</div>
              <div className="ha-node-cell ha-node-cell--action" role="columnheader">{labels.actionHeader}</div>
            </div>
            {rows.map((row) => (
              <div className="ha-node-grid-row" role="row" key={row.key}>
                <div
                  role="cell"
                  className="ha-node-cell ha-node-cell--identity ha-node-identity"
                  data-label={labels.nodeHeader}
                >
                  {onOpenNodeDetails && !row.isLocalNode ? (
                    <button
                      type="button"
                      className="ha-node-link"
                      onClick={() => onOpenNodeDetails(row.nodeId)}
                    >
                      <strong>{row.nodeId}</strong>
                    </button>
                  ) : (
                    <strong>{row.nodeId}</strong>
                  )}
                  <span>{row.relation}</span>
                </div>
                <div role="cell" className="ha-node-cell ha-node-cell--role" data-label={labels.roleHeader}>
                  {row.role}
                </div>
                <div role="cell" className="ha-node-cell ha-node-cell--origin" data-label={labels.originHeader}>
                  <code>{row.origin}</code>
                </div>
                <div role="cell" className="ha-node-cell ha-node-cell--health" data-label={labels.healthHeader}>
                  <StatusBadge tone={row.healthTone}>{row.health}</StatusBadge>
                </div>
                <div role="cell" className="ha-node-cell ha-node-cell--time" data-label={labels.lastSyncHeader}>
                  <time dateTime={row.lastSyncDateTime} title={row.lastSyncTitle}>
                    {row.lastSync}
                  </time>
                </div>
                <div role="cell" className="ha-node-cell ha-node-cell--time" data-label={labels.promotedAtHeader}>
                  <time dateTime={row.promotedAtDateTime} title={row.promotedAtTitle}>
                    {row.promotedAt}
                  </time>
                </div>
                <div
                  role="cell"
                  className="ha-node-cell ha-node-cell--action ha-node-action"
                  data-label={labels.actionHeader}
                >
                  {row.actionKind === 'promote' && onPromote && (
                    <Button
                      type="button"
                      size="sm"
                      variant="warning"
                      className="ha-node-action-button"
                      onClick={onPromote}
                      disabled={busy}
                    >
                      <Crown className="h-4 w-4" aria-hidden="true" />
                      {labels.promoteToMaster}
                    </Button>
                  )}
                  {row.actionKind === 'finalize' && onFinalize && (
                    <Button
                      type="button"
                      size="sm"
                      variant="success"
                      className="ha-node-action-button"
                      onClick={onFinalize}
                      disabled={busy}
                    >
                      <ShieldCheck className="h-4 w-4" aria-hidden="true" />
                      {labels.finalizeMaster}
                    </Button>
                  )}
                  {row.actionKind === 'serving' && (
                    <span className="ha-node-action-note">{labels.actionServing}</span>
                  )}
                  {row.actionKind === 'blocked' && (
                    <span className="ha-node-action-note">{labels.actionRecoverFirst}</span>
                  )}
                  {row.actionKind === 'planned_cutover' && row.targetNodeId && onPlannedCutover && (
                    <Button
                      type="button"
                      size="sm"
                      variant="warning"
                      className="ha-node-action-button"
                      onClick={() => onPlannedCutover(row.targetNodeId!)}
                      disabled={busy}
                    >
                      <ArrowRight className="h-4 w-4" aria-hidden="true" />
                      {labels.actionPlannedCutover}
                    </Button>
                  )}
                  {row.actionKind === 'observe' && (
                    <span className="ha-node-action-note">
                      {row.relation === labels.relationStandbyCandidate
                        ? labels.actionNotEligibleNow
                        : labels.actionObserveOnly}
                    </span>
                  )}
                </div>
              </div>
            ))}
          </div>
        </div>

        <div className="ha-node-list" aria-label={labels.plannedCutoverTitle}>
          <div className="ha-node-list-title">
            <Crown size={18} aria-hidden="true" />
            <span>{labels.plannedCutoverTitle}</span>
          </div>
          <div className="ha-status-message">
            <span>{labels.plannedCutoverDescription}</span>
          </div>
        </div>

        <div className="ha-node-list" aria-label={labels.timelineTitle}>
          <div className="ha-node-list-title">
            <RotateCcw size={18} aria-hidden="true" />
            <span>{labels.timelineTitle}</span>
          </div>
          {timeline.length === 0 ? (
            <div className="ha-status-message">
              <span>{timelineLoading ? labels.timelineLoading : labels.timelineEmpty}</span>
            </div>
          ) : (
            <div className="ha-timeline-list">
              {timeline.map((event) => (
                <details key={event.id} className="ha-timeline-item">
                  <summary>
                    <span>{formatHaTimelineSummary(event, labels)}</span>
                    <StatusBadge
                      tone={
                        event.status === 'success'
                          ? 'success'
                          : event.status === 'running'
                            ? 'warning'
                            : event.status === 'error'
                              ? 'error'
                            : 'neutral'
                      }
                    >
                      {formatHaTimelineStatusLabel(event.status, labels)}
                    </StatusBadge>
                  </summary>
                  <div className="ha-timeline-meta">
                    <div>{formatTimestamp(event.createdAt, lang)}</div>
                    {formatHaTimelineDetail(event, labels) ? <p>{formatHaTimelineDetail(event, labels)}</p> : null}
                    {event.technicalDetails ? (
                      <pre>{JSON.stringify(event.technicalDetails, null, 2)}</pre>
                    ) : null}
                  </div>
                </details>
              ))}
              {hasMoreTimeline && onLoadMoreTimeline && (
                <Button type="button" variant="outline" size="sm" onClick={onLoadMoreTimeline} disabled={timelineLoading}>
                  {timelineLoading ? labels.timelineLoading : labels.timelineLoadMore}
                </Button>
              )}
            </div>
          )}
        </div>

        {formatHaStatusMessage(status, labels) && (
          <div className="ha-status-message">
            <RotateCcw size={16} aria-hidden="true" />
            <span>{formatHaStatusMessage(status, labels)}</span>
          </div>
        )}
      </section>
    )
  }

  return (
    <section className={`ha-status-banner ${toneClass}`} role="status" aria-live="polite">
      <div className="ha-status-banner-head">
        <div className="ha-status-banner-icon" aria-hidden="true">
          <CircleAlert size={22} strokeWidth={2.4} />
        </div>
        <div className="ha-status-banner-copy">
          <div className="ha-status-banner-title">{title}</div>
          <p>{detail}</p>
        </div>
      </div>
    </section>
  )
}
