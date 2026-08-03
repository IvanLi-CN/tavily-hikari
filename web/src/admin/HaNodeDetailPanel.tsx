import { ArrowLeft, RotateCcw, Server } from 'lucide-react'

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

function numberFormat(value: number, language: 'en' | 'zh'): string {
  return new Intl.NumberFormat(localeFor(language), { maximumFractionDigits: 1 }).format(value)
}

function channelLabel(channel: string, language: 'en' | 'zh'): string {
  if (language === 'zh') {
    return channel === 'control' ? 'Control' : channel === 'billing' ? 'Billing' : 'Runtime'
  }
  return channel.charAt(0).toUpperCase() + channel.slice(1)
}

function channelStateLabel(state: string, language: 'en' | 'zh'): string {
  const labels: Record<string, [string, string]> = {
    healthy: ['健康', 'Healthy'],
    catching_up: ['追赶中', 'Catching up'],
    baseline_required: ['需要 baseline', 'Baseline required'],
    expired_backlog: ['存在过期积压', 'Expired backlog'],
    unavailable: ['源端不可用', 'Source unavailable'],
    source: ['源端', 'Source'],
  }
  return labels[state]?.[language === 'zh' ? 0 : 1] ?? state
}

function gcStateLabel(state: string, language: 'en' | 'zh'): string {
  const labels: Record<string, [string, string]> = {
    idle: ['空闲', 'Idle'], draining: ['清理中', 'Draining'], deferred: ['已让步', 'Deferred'],
    stalled: ['停滞', 'Stalled'], unknown: ['未知', 'Unknown'],
  }
  return labels[state]?.[language === 'zh' ? 0 : 1] ?? state
}

function gcSloLabel(state: string, language: 'en' | 'zh'): string {
  const labels: Record<string, [string, string]> = {
    clear: ['已清除', 'Clear'],
    on_track: ['按 SLO 推进', 'On track'],
    breached: ['已超 SLO', 'SLO breached'],
    not_applicable: ['不适用', 'N/A'],
    unknown: ['未知', 'Unknown'],
  }
  return labels[state]?.[language === 'zh' ? 0 : 1] ?? state
}

function formatRetention(value: number, language: 'en' | 'zh'): string {
  const days = Math.round(value / 86_400)
  return language === 'zh' ? `${days} 天` : `${days} days`
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

export interface HaNodeDetailPanelProps {
  detail: HaNodeDetail | null
  strings: AdminTranslations['systemSettings']['ha']
  language: 'en' | 'zh'
  loading?: boolean
  onBack: () => void
  onLoadMoreTimeline?: (() => void) | null
  hasMoreTimeline?: boolean
}

export default function HaNodeDetailPanel({
  detail,
  strings,
  language,
  loading = false,
  onBack,
  onLoadMoreTimeline = null,
  hasMoreTimeline = false,
}: HaNodeDetailPanelProps): JSX.Element {
  const node = detail?.node ?? null
  const timeline = detail?.timeline.events ?? []
  const cutoverStatus = node ? { tone: cutoverTone(node), label: cutoverLabel(node, strings) } : null
  const trafficStatus = node ? trafficAuthority(node, strings) : null
  const writeStatus = node ? writeAuthority(node, strings) : null
  const nodeMessage = node ? formatHaPeerMessage(node, strings) : null
  const channelHealth = node?.channelHealth ?? []
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

        <article className="ha-node-detail-card ha-node-detail-card--channels" aria-label="HA channel health">
          <div className="ha-node-detail-card-head">
            <div className="ha-node-list-title">
              <Server size={18} aria-hidden="true" />
              <span>{language === 'zh' ? '复制 ACK 与 GC 健康' : 'Replication ACK and GC health'}</span>
            </div>
          </div>
          {channelHealth.length === 0 ? (
            <div className="ha-status-message"><span>—</span></div>
          ) : (
            <div className="ha-channel-health-list">
              {channelHealth.map((health) => (
                <div key={health.channel} className="ha-channel-health-row">
                  <div className="ha-channel-health-heading">
                    <strong>{channelLabel(health.channel, language)}</strong>
                    <StatusBadge tone={health.cursorState === 'healthy' ? 'success' : 'warning'}>
                      {channelStateLabel(health.cursorState, language)}
                    </StatusBadge>
                  </div>
                  <dl>
                    <div><dt>{language === 'zh' ? 'ACK 序号' : 'ACK'}</dt><dd>{health.ackedSeq ?? '—'}</dd></div>
                    <div><dt>{language === 'zh' ? '高水位' : 'High watermark'}</dt><dd>{health.highWatermark}</dd></div>
                    <div><dt>{language === 'zh' ? 'ACK 延迟' : 'ACK lag'}</dt><dd>{health.ackLag ?? '—'}</dd></div>
                    <div><dt>{language === 'zh' ? '保留期' : 'Retention'}</dt><dd>{formatRetention(health.retentionSecs, language)}</dd></div>
                    <div><dt>{language === 'zh' ? '过期积压' : 'Expired backlog'}</dt><dd>{health.expiredBacklog ? (language === 'zh' ? '是' : 'Yes') : (language === 'zh' ? '否' : 'No')}</dd></div>
                    <div><dt>{language === 'zh' ? 'GC 状态' : 'GC state'}</dt><dd>{gcStateLabel(health.gcState, language)}</dd></div>
                    <div><dt>{language === 'zh' ? '最老事件' : 'Oldest event'}</dt><dd>{formatLag(health.oldestAgeSecs, language)}</dd></div>
                    <div><dt>{language === 'zh' ? '批量' : 'Batch'}</dt><dd>{health.batchSize}</dd></div>
                    <div><dt>{language === 'zh' ? '最近进展' : 'Last progress'}</dt><dd>{formatTimestamp(health.lastProgressAt, language)}</dd></div>
                    <div><dt>{language === 'zh' ? '让步原因' : 'Defer reason'}</dt><dd>{health.lastDeferReason ?? '—'}</dd></div>
                    <div><dt>{language === 'zh' ? '下次重试' : 'Next retry'}</dt><dd>{formatTimestamp(health.nextRetryAt, language)}</dd></div>
                    <div><dt>{language === 'zh' ? '债务模式' : 'Debt mode'}</dt><dd>{health.gcDebtMode || '—'}</dd></div>
                    <div><dt>{language === 'zh' ? '删除速率' : 'Delete rate'}</dt><dd>{numberFormat(health.gcDeletedRowsPerMinute, language)} / min</dd></div>
                    <div><dt>{language === 'zh' ? '前台 RPS' : 'Foreground RPS'}</dt><dd>{numberFormat(health.gcForegroundRps, language)}</dd></div>
                    <div><dt>{language === 'zh' ? 'GC SLO' : 'GC SLO'}</dt><dd>{gcSloLabel(health.gcSloState, language)}</dd></div>
                    <div><dt>{language === 'zh' ? '恢复截止' : 'Recovery deadline'}</dt><dd>{formatTimestamp(health.gcRecoveryDeadlineAt, language)}</dd></div>
                    <div><dt>{language === 'zh' ? '观测时间' : 'Observed at'}</dt><dd>{formatTimestamp(health.gcObservedAt, language)}</dd></div>
                  </dl>
                </div>
              ))}
            </div>
          )}
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
