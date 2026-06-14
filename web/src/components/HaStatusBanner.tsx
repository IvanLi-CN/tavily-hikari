import { ArrowRight, CircleAlert, Crown, RotateCcw, Server, ShieldCheck, Settings2 } from 'lucide-react'

import type { HaStatus } from '../api'
import type { AdminTranslations } from '../i18n'
import { useLanguage, useTranslate } from '../i18n'
import { Button } from './ui/button'
import { StatusBadge, type StatusTone } from './StatusBadge'

interface HaStatusBannerProps {
  status: HaStatus | null
  audience: 'admin' | 'user'
  strings?: AdminTranslations['systemSettings']['ha']
  language?: 'en' | 'zh'
  adminVariant?: 'panel' | 'compact'
  onPromote?: () => void
  onFinalize?: () => void
  onConfigureSource?: () => void
  busy?: boolean
  compactHref?: string
  compactTitle?: string
  compactDescription?: string
  compactActionLabel?: string
  onCompactClick?: () => void
}

function formatTimestamp(value: number | null): string {
  if (value == null) return '—'
  return new Date(value * 1000).toLocaleString()
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
  role: string
  origin: string
  health: string
  healthTone: StatusTone
  lastSync: string
  promotedAt: string
  actionKind: 'promote' | 'finalize' | 'serving' | 'blocked' | 'remote'
  canConfigureSource?: boolean
}

function buildNodeRows(status: HaStatus, strings: AdminTranslations['systemSettings']['ha']): HaNodeRow[] {
  const rows: HaNodeRow[] = [
    {
      key: 'local',
      nodeId: status.nodeId,
      relation: strings.thisAdminNodeLabel,
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
      lastSync: formatTimestamp(status.lastSyncAt),
      promotedAt:
        status.role === 'full_master' || status.role === 'provisional_master'
          ? formatTimestamp(status.lastEdgeoneCheckAt)
          : '—',
      actionKind:
        status.role === 'standby'
          ? 'promote'
          : status.role === 'provisional_master'
            ? 'finalize'
            : status.role === 'full_master'
              ? 'serving'
              : 'blocked',
      canConfigureSource: true,
    },
  ]

  const remoteOrigins = [status.edgeoneOrigin, status.edgeoneExpectedOrigin]
    .map((origin) => origin?.trim())
    .filter((origin): origin is string => Boolean(origin && origin !== status.nodePublicOrigin))

  for (const origin of Array.from(new Set(remoteOrigins))) {
    rows.push({
      key: `remote-${origin}`,
      nodeId: origin === status.edgeoneExpectedOrigin ? 'configured-peer' : 'edgeone-origin',
      relation: origin === status.edgeoneExpectedOrigin ? strings.configuredPeerLabel : strings.edgeoneTargetLabel,
      role:
        status.edgeoneOrigin === origin
          ? strings.roleFullMaster
          : status.edgeoneExpectedOrigin === origin
            ? strings.roleStandby
            : strings.roleRecovery,
      origin,
      health:
        status.edgeoneOrigin === origin
          ? strings.healthServingEdgeone
          : status.edgeoneExpectedOrigin === origin
            ? strings.healthNotRouted
            : strings.healthConfigured,
      healthTone:
        status.edgeoneOrigin === origin
          ? 'success'
          : status.edgeoneExpectedOrigin === origin
            ? 'warning'
            : 'neutral',
      lastSync: origin === status.edgeoneExpectedOrigin ? formatTimestamp(status.lastSyncAt) : '—',
      promotedAt: status.edgeoneOrigin === origin ? formatTimestamp(status.lastEdgeoneCheckAt) : '—',
      actionKind: 'remote',
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
  onPromote,
  onFinalize,
  onConfigureSource,
  busy = false,
  compactHref,
  compactTitle,
  compactDescription,
  compactActionLabel,
  onCompactClick,
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
  const rows = buildNodeRows(status, labels)
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
          <div className="ha-node-panel-state">
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
          <div><dt>{labels.summaryRecovery}</dt><dd>{status.recoveryStatus ?? '—'}</dd></div>
        </dl>

        <div className="ha-node-list" aria-label={labels.nodeInventoryTitle}>
          <div className="ha-node-list-title">
            <Server size={18} aria-hidden="true" />
            <span>{labels.nodeInventoryTitle}</span>
          </div>
          <div className="ha-node-grid" role="table" aria-label={labels.nodeInventoryTitle}>
            <div className="ha-node-grid-row ha-node-grid-head" role="row">
              <div role="columnheader">{labels.nodeHeader}</div>
              <div role="columnheader">{labels.roleHeader}</div>
              <div role="columnheader">{labels.originHeader}</div>
              <div role="columnheader">{labels.healthHeader}</div>
              <div role="columnheader">{labels.lastSyncHeader}</div>
              <div role="columnheader">{labels.promotedAtHeader}</div>
              <div role="columnheader">{labels.actionHeader}</div>
            </div>
            {rows.map((row) => (
              <div className="ha-node-grid-row" role="row" key={row.key}>
                <div role="cell" className="ha-node-identity">
                  <strong>{row.nodeId}</strong>
                  <span>{row.relation}</span>
                </div>
                <div role="cell">{row.role}</div>
                <div role="cell"><code>{row.origin}</code></div>
                <div role="cell"><StatusBadge tone={row.healthTone}>{row.health}</StatusBadge></div>
                <div role="cell">{row.lastSync}</div>
                <div role="cell">{row.promotedAt}</div>
                <div role="cell" className="ha-node-action">
                  {row.canConfigureSource && (
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      className="ha-node-action-button"
                      onClick={onConfigureSource}
                      disabled={busy || !onConfigureSource}
                    >
                      <Settings2 className="h-4 w-4" aria-hidden="true" />
                      {labels.configureSource}
                    </Button>
                  )}
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
                  {row.actionKind === 'remote' && (
                    <span className="ha-node-action-note">{labels.actionUseThatNodeAdmin}</span>
                  )}
                </div>
              </div>
            ))}
          </div>
        </div>

        {status.message && (
          <div className="ha-status-message">
            <RotateCcw size={16} aria-hidden="true" />
            <span>{status.message}</span>
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
