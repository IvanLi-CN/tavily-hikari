import { ArrowLeft, RotateCcw, Route, Server, Settings2 } from 'lucide-react'

import type { HaNodeDetail, HaTimelineEvent } from '../api'
import type { AdminTranslations } from '../i18n'
import { Button } from '../components/ui/button'
import { StatusBadge, type StatusTone } from '../components/StatusBadge'
import {
  formatHaPeerMessage,
  formatHaRecoveryStatus,
  formatHaTimelineDetail,
  formatHaTimelineStatusLabel,
  formatHaTimelineSummary,
} from '../lib/haCopy'

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

function formatLag(value: number | null, language: 'en' | 'zh'): string {
  if (value == null) return '—'
  if (value < 60) return language === 'zh' ? `${value}秒` : `${value}s`
  const minutes = Math.floor(value / 60)
  const seconds = value % 60
  if (language === 'zh') return seconds === 0 ? `${minutes}分` : `${minutes}分${seconds}秒`
  return seconds === 0 ? `${minutes}m` : `${minutes}m ${seconds}s`
}

function roleLabel(
  role: HaNodeDetail['node']['role'],
  strings: AdminTranslations['systemSettings']['ha'],
): string {
  if (role === 'full_master') return strings.roleFullMaster
  if (role === 'provisional_master') return strings.roleProvisionalMaster
  if (role === 'standby') return strings.roleStandby
  if (role === 'recovery') return strings.roleRecovery
  return '—'
}

function timelineStatusTone(status: HaTimelineEvent['status']): StatusTone {
  if (status === 'success') return 'success'
  if (status === 'running' || status === 'warning') return 'warning'
  if (status === 'error') return 'error'
  return 'neutral'
}

function roleTone(role: HaNodeDetail['node']['role']): StatusTone {
  if (role === 'full_master') return 'success'
  if (role === 'provisional_master' || role === 'recovery') return 'warning'
  if (role === 'standby') return 'neutral'
  return 'neutral'
}

function cutoverTone(node: HaNodeDetail['node']): StatusTone {
  if (node.plannedCutoverEligible) return 'success'
  if (node.stale) return 'warning'
  return 'neutral'
}

function cutoverLabel(
  node: HaNodeDetail['node'],
  strings: AdminTranslations['systemSettings']['ha'],
): string {
  if (node.plannedCutoverEligible) return strings.nodeDetailEligible
  if (node.stale) return strings.healthStale
  if (node.roleHint === 'standby_candidate') return strings.actionNotEligibleNow
  return strings.actionObserveOnly
}

function trafficAuthority(
  node: HaNodeDetail['node'],
  strings: AdminTranslations['systemSettings']['ha'],
): { tone: StatusTone; label: string } {
  if (node.allowsFullWrites) {
    return { tone: 'success', label: strings.authorityFullWrites }
  }
  if (node.allowsBasicBusiness) {
    return { tone: 'neutral', label: strings.authorityBasicTraffic }
  }
  return { tone: 'warning', label: strings.authorityWritesBlocked }
}

function writeAuthority(
  node: HaNodeDetail['node'],
  strings: AdminTranslations['systemSettings']['ha'],
): { tone: StatusTone; label: string } {
  if (node.allowsFullWrites) {
    return { tone: 'success', label: strings.authorityFullWrites }
  }
  return { tone: 'warning', label: strings.authorityWritesBlocked }
}

function sourceKindLabel(
  value: HaNodeDetail['edgeoneCurrentSourceKind'],
  strings: AdminTranslations['systemSettings']['ha'],
): string {
  if (value === 'direct') return strings.sourceKindDirect
  if (value === 'origin_group') return strings.sourceKindOriginGroup
  return '—'
}

function formatEffectiveSource(
  settings: HaNodeDetail['haSourceEffective'],
  strings: AdminTranslations['systemSettings']['ha'],
): string {
  if (!settings) return '—'
  if (settings.sourceKind === 'origin_group') {
    return settings.originGroupId ?? '—'
  }
  const scheme = settings.directOriginScheme?.toUpperCase() ?? '—'
  const host = settings.directOriginHost ?? '—'
  const port = settings.directOriginPort ?? '—'
  return `${scheme} · ${host}:${port}`
}

export interface HaNodeDetailPanelProps {
  detail: HaNodeDetail | null
  strings: AdminTranslations['systemSettings']['ha']
  language: 'en' | 'zh'
  loading?: boolean
  onBack: () => void
  onConfigureSource?: () => void
  onLoadMoreTimeline?: (() => void) | null
  hasMoreTimeline?: boolean
}

export default function HaNodeDetailPanel({
  detail,
  strings,
  language,
  loading = false,
  onBack,
  onConfigureSource,
  onLoadMoreTimeline = null,
  hasMoreTimeline = false,
}: HaNodeDetailPanelProps): JSX.Element {
  const node = detail?.node ?? null
  const timeline = detail?.timeline.events ?? []
  const cutoverStatus = node ? { tone: cutoverTone(node), label: cutoverLabel(node, strings) } : null
  const trafficStatus = node ? trafficAuthority(node, strings) : null
  const writeStatus = node ? writeAuthority(node, strings) : null
  const nodeMessage = node ? formatHaPeerMessage(node, strings) : null
  return (
    <section className="ha-node-panel" aria-labelledby="ha-node-detail-title">
      <div className="ha-node-panel-head">
        <div className="ha-node-panel-title-group">
          <button type="button" className="ha-node-detail-back" onClick={onBack}>
            <ArrowLeft className="h-4 w-4" aria-hidden="true" />
            <span>{strings.nodeDetailBack}</span>
          </button>
          <div className="ha-node-panel-kicker">{strings.nodeDetailKicker}</div>
          <h2 id="ha-node-detail-title">
            {node ? strings.nodeDetailTitle.replace('{nodeId}', node.nodeId) : strings.nodeDetailLoading}
          </h2>
          <p>
            {node
              ? strings.nodeDetailDescription
                .replace('{nodeId}', node.nodeId)
                .replace('{currentNodeId}', detail?.currentNodeId ?? '—')
              : strings.nodeDetailLoading}
          </p>
        </div>
        {node && (
          <div className="ha-node-panel-state">
            {cutoverStatus ? <StatusBadge tone={cutoverStatus.tone}>{cutoverStatus.label}</StatusBadge> : null}
          </div>
        )}
      </div>

      <div className="ha-node-detail-summary">
        <article className="ha-node-detail-card ha-node-detail-card--overview" aria-label={strings.nodeDetailInfoTitle}>
          <div className="ha-node-detail-card-head">
            <div className="ha-node-list-title">
              <Server size={18} aria-hidden="true" />
              <span>{strings.nodeDetailInfoTitle}</span>
            </div>
            {node ? <StatusBadge tone={roleTone(node.role)}>{roleLabel(node.role, strings)}</StatusBadge> : null}
          </div>
          {node ? (
            <>
              <div className="ha-node-detail-overview-grid">
                <div className="ha-node-detail-primary">
                  <div className="ha-node-detail-primary-block">
                    <span className="ha-node-detail-primary-label">{strings.nodeHeader}</span>
                    <strong className="ha-node-detail-primary-value">{node.nodeId}</strong>
                  </div>
                  <div className="ha-node-detail-primary-block">
                    <span className="ha-node-detail-primary-label">{strings.originHeader}</span>
                    <code className="ha-node-detail-code">{node.publicOrigin ?? '—'}</code>
                  </div>
                  <div className="ha-node-detail-primary-badges">
                    {trafficStatus ? <StatusBadge tone={trafficStatus.tone}>{trafficStatus.label}</StatusBadge> : null}
                    {writeStatus ? <StatusBadge tone={writeStatus.tone}>{writeStatus.label}</StatusBadge> : null}
                  </div>
                </div>
                <dl className="ha-node-detail-overview-facts">
                  <div>
                    <dt>{strings.summarySyncLag}</dt>
                    <dd>{formatLag(node.syncLagSeconds, language)}</dd>
                  </div>
                  <div>
                    <dt>{strings.lastSyncHeader}</dt>
                    <dd>{formatTimestamp(node.lastSyncAt, language)}</dd>
                  </div>
                  <div>
                    <dt>{strings.nodeDetailLastSeenLabel}</dt>
                    <dd>{formatTimestamp(node.lastSeenAt, language)}</dd>
                  </div>
                  <div>
                    <dt>{strings.summaryRecovery}</dt>
                    <dd>{formatHaRecoveryStatus(node.recoveryStatus, strings) ?? '—'}</dd>
                  </div>
                  <div className="ha-node-detail-overview-fact-wide">
                    <dt>{strings.nodeDetailRoleHintLabel}</dt>
                    <dd>
                      <code className="ha-node-detail-code">{node.roleHint}</code>
                    </dd>
                  </div>
                </dl>
              </div>
              {nodeMessage ? (
                <div className="ha-status-message">
                  <RotateCcw size={16} aria-hidden="true" />
                  <span>{nodeMessage}</span>
                </div>
              ) : null}
            </>
          ) : (
            <div className="ha-status-message">
              <span>{strings.nodeDetailLoading}</span>
            </div>
          )}
        </article>

        <article className="ha-node-detail-card ha-node-detail-card--aside" aria-label={strings.nodeDetailContextTitle}>
          <h3>{strings.nodeDetailContextTitle}</h3>
          <dl className="ha-node-detail-context-grid">
            <div>
              <dt>{strings.nodeDetailCurrentNodeLabel}</dt>
              <dd>
                <code className="ha-node-detail-code">{detail?.currentNodeId ?? '—'}</code>
              </dd>
            </div>
            <div>
              <dt>{strings.timelineTitle}</dt>
              <dd>{strings.nodeDetailRetentionWindow}</dd>
            </div>
            <div>
              <dt>{strings.nodeDetailTrafficLabel}</dt>
              <dd>{trafficStatus ? <StatusBadge tone={trafficStatus.tone}>{trafficStatus.label}</StatusBadge> : '—'}</dd>
            </div>
            <div>
              <dt>{strings.nodeDetailWriteLabel}</dt>
              <dd>{writeStatus ? <StatusBadge tone={writeStatus.tone}>{writeStatus.label}</StatusBadge> : '—'}</dd>
            </div>
          </dl>
          <p className="ha-node-detail-context-note">{strings.nodeDetailRetentionDescription}</p>
        </article>

        <article className="ha-node-detail-card ha-node-detail-card--edgeone" aria-label={strings.nodeDetailEdgeoneTitle}>
          <div className="ha-node-detail-card-head">
            <div className="ha-node-list-title">
              <Route size={18} aria-hidden="true" />
              <span>{strings.nodeDetailEdgeoneTitle}</span>
            </div>
            {onConfigureSource ? (
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="ha-node-detail-configure-button"
                onClick={onConfigureSource}
              >
                <Settings2 className="h-4 w-4" aria-hidden="true" />
                {strings.configureSource}
              </Button>
            ) : null}
          </div>
          <dl className="ha-node-detail-edgeone-grid">
            <div>
              <dt>{strings.nodeDetailEdgeoneDomainLabel}</dt>
              <dd>{detail?.edgeoneDomain ?? '—'}</dd>
            </div>
            <div>
              <dt>{strings.nodeDetailEdgeoneSourceLabel}</dt>
              <dd>{sourceKindLabel(detail?.edgeoneCurrentSourceKind ?? null, strings)}</dd>
            </div>
            <div className="ha-node-detail-edgeone-wide">
              <dt>{strings.nodeDetailEdgeoneTargetLabel}</dt>
              <dd>
                <code className="ha-node-detail-code">{detail?.edgeoneCurrentTarget ?? '—'}</code>
              </dd>
            </div>
          </dl>
          <div className="ha-node-detail-config">
            <span className="ha-node-detail-config-label">{strings.nodeDetailEdgeoneEffectiveLabel}</span>
            <code className="ha-node-detail-code">
              {formatEffectiveSource(detail?.haSourceEffective ?? null, strings)}
            </code>
          </div>
        </article>
      </div>

      <div className="ha-node-list" aria-label={strings.nodeDetailInteractionsTitle}>
        <div className="ha-node-list-title">
          <RotateCcw size={18} aria-hidden="true" />
          <span>{strings.nodeDetailInteractionsTitle}</span>
        </div>
        {timeline.length === 0 ? (
          <div className="ha-status-message">
            <span>{loading ? strings.timelineLoading : strings.nodeDetailTimelineEmpty}</span>
          </div>
        ) : (
          <div className="ha-timeline-list">
            {timeline.map((event) => (
              <details key={event.id} className="ha-timeline-item">
                <summary>
                  <span>{formatHaTimelineSummary(event, strings, { currentNodeId: detail?.currentNodeId ?? null })}</span>
                  <StatusBadge tone={timelineStatusTone(event.status)}>
                    {formatHaTimelineStatusLabel(event.status, strings)}
                  </StatusBadge>
                </summary>
                <div className="ha-timeline-meta">
                  <div>{formatTimestamp(event.createdAt, language)}</div>
                  {formatHaTimelineDetail(event, strings, { currentNodeId: detail?.currentNodeId ?? null })
                    ? <p>{formatHaTimelineDetail(event, strings, { currentNodeId: detail?.currentNodeId ?? null })}</p>
                    : null}
                  {event.technicalDetails ? <pre>{JSON.stringify(event.technicalDetails, null, 2)}</pre> : null}
                </div>
              </details>
            ))}
            {hasMoreTimeline && onLoadMoreTimeline && (
              <Button type="button" variant="outline" size="sm" onClick={onLoadMoreTimeline} disabled={loading}>
                {loading ? strings.timelineLoading : strings.timelineLoadMore}
              </Button>
            )}
          </div>
        )}
      </div>
    </section>
  )
}
