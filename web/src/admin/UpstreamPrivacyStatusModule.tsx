import { useId, useMemo } from 'react'

import type { QueryLoadState } from './queryLoadState'
import type { Language, AdminTranslations } from '../i18n'
import type { UpstreamPrivacyGate, UpstreamPrivacyStatus, UpstreamProjectIdMode } from '../api'
import AdminLoadingRegion from '../components/AdminLoadingRegion'
import { StatusBadge } from '../components/StatusBadge'
import { Icon } from '../lib/icons'
import { Button } from '../components/ui/button'
import { Switch } from '../components/ui/switch'

interface UpstreamPrivacyStatusModuleProps {
  strings: AdminTranslations['systemSettings']['privacy']
  formStrings: AdminTranslations['systemSettings']['form']
  language: Language
  status: UpstreamPrivacyStatus | null
  loadState: QueryLoadState
  error: string | null
  refreshing: boolean
  autoRefreshEnabled: boolean
  onAutoRefreshChange: (next: boolean) => void
  onOpenMcpSessionBindings: () => void
  onRefresh: () => Promise<void> | void
}

function phaseTone(phase: UpstreamPrivacyStatus['phase']): 'neutral' | 'info' | 'success' | 'warning' | 'error' {
  switch (phase) {
    case 'active':
      return 'success'
    case 'pending':
    case 'compare':
      return 'info'
    case 'draining':
      return 'warning'
    case 'degraded':
      return 'error'
    default:
      return 'neutral'
  }
}

function formatOptionalValue(value: string | null | undefined, emptyLabel: string): string {
  return value && value.length > 0 ? value : emptyLabel
}

function formatSignedCount(value: number): string {
  if (value > 0) return `+${value}`
  return String(value)
}

interface StatusIssue {
  key: string
  title: string
  detail: string
  tone: 'warning' | 'error' | 'info'
}

function gateLabel(
  strings: AdminTranslations['systemSettings']['privacy'],
  language: Language,
  gate: UpstreamPrivacyGate,
): string {
  switch (gate.key) {
    case 'accessTokenMode':
      return strings.gateAccessTokenMode
    case 'apiRebalance':
      return language === 'zh' ? 'API Rebalance 已启用' : 'API Rebalance enabled'
    case 'mcpRebalance':
      return language === 'zh' ? 'Rebalance MCP 已启用' : 'Rebalance MCP enabled'
    case 'controlSessionsDrained':
      return language === 'zh' ? '`upstream_mcp` session 已排空' : '`upstream_mcp` sessions drained'
    default:
      return gate.key
  }
}

function modeLabel(
  strings: AdminTranslations['systemSettings']['form'],
  mode: UpstreamProjectIdMode,
): string {
  switch (mode) {
    case 'passthrough':
      return strings.upstreamProjectIdModePassthrough
    case 'fixed':
      return strings.upstreamProjectIdModeFixed
    case 'accessToken':
      return strings.upstreamProjectIdModeAccessToken
    default:
      return mode
  }
}

export default function UpstreamPrivacyStatusModule({
  strings,
  formStrings,
  language,
  status,
  loadState,
  error,
  refreshing,
  autoRefreshEnabled,
  onAutoRefreshChange,
  onOpenMcpSessionBindings,
  onRefresh,
}: UpstreamPrivacyStatusModuleProps): JSX.Element {
  const autoRefreshLabelId = useId()
  const timestampFormatter = useMemo(
    () =>
      new Intl.DateTimeFormat(language === 'zh' ? 'zh-CN' : 'en-US', {
        dateStyle: 'medium',
        timeStyle: 'short',
      }),
    [language],
  )
  const numberFormatter = useMemo(
    () => new Intl.NumberFormat(language === 'zh' ? 'zh-CN' : 'en-US'),
    [language],
  )
  const sessionBindingCardLabel =
    language === 'zh' ? '活跃 upstream_mcp session' : 'Active upstream_mcp sessions'
  const sessionBindingSummaryLabel = sessionBindingCardLabel

  const phaseLabel = status
    ? ({
        configured: strings.phaseConfigured,
        draining: strings.phaseDraining,
        pending: strings.phasePending,
        compare: strings.phaseCompare,
        active: strings.phaseActive,
        degraded: strings.phaseDegraded,
      } satisfies Record<UpstreamPrivacyStatus['phase'], string>)[status.phase]
    : strings.phaseConfigured

  const phaseDescription = status
    ? ({
        configured: strings.phaseConfiguredDescription,
        draining: strings.phaseDrainingDescription,
        pending: strings.phasePendingDescription,
        compare: strings.phaseCompareDescription,
        active: strings.phaseActiveDescription,
        degraded: strings.phaseDegradedDescription,
      } satisfies Record<UpstreamPrivacyStatus['phase'], string>)[status.phase]
    : strings.phaseConfiguredDescription

  const statusIssues = useMemo<StatusIssue[]>(() => {
    if (!status) return []

    const issues: StatusIssue[] = []
    const pendingGates = status.gates.filter((gate) => !gate.ready)

    pendingGates.forEach((gate) => {
      issues.push({
        key: `gate:${gate.key}`,
        title: gateLabel(strings, language, gate),
        detail: gate.detail,
        tone: 'warning',
      })
    })

    if (status.pendingResearch > 0) {
      issues.push({
        key: 'pendingResearch',
        title: strings.issuePendingResearch,
        detail: numberFormatter.format(status.pendingResearch),
        tone: 'info',
      })
    }

    if (status.queuedSettlements > 0) {
      issues.push({
        key: 'queuedSettlements',
        title: strings.issueQueuedSettlements,
        detail: numberFormatter.format(status.queuedSettlements),
        tone: 'info',
      })
    }

    if (status.degradedSettlements > 0) {
      issues.push({
        key: 'degradedSettlements',
        title: strings.issueDegradedSettlements,
        detail: numberFormatter.format(status.degradedSettlements),
        tone: 'error',
      })
    }

    return issues
  }, [language, numberFormatter, status, strings])

  const summarySignals = useMemo(
    () =>
      !status
        ? []
        : [
            ...(status.activeUpstreamMcpSessions > 0
              ? [{ label: sessionBindingSummaryLabel, value: numberFormatter.format(status.activeUpstreamMcpSessions) }]
              : []),
            ...(status.pendingResearch > 0
              ? [{ label: strings.counterPendingResearch, value: numberFormatter.format(status.pendingResearch) }]
              : []),
            ...(status.queuedSettlements > 0
              ? [{ label: strings.counterQueuedSettlements, value: numberFormatter.format(status.queuedSettlements) }]
              : []),
            ...(status.degradedSettlements > 0
              ? [{ label: strings.counterDegradedSettlements, value: numberFormatter.format(status.degradedSettlements) }]
              : []),
          ],
    [numberFormatter, sessionBindingSummaryLabel, status, strings],
  )

  const configurationDriftCount = status
    ? Number(status.configuredProjectIdMode !== status.effectiveProjectIdMode)
      + Number(formatOptionalValue(status.configuredMcpUserAgent, strings.statusOmitted)
        !== formatOptionalValue(status.effectiveMcpUserAgent, strings.statusOmitted))
    : 0

  const showFixedProjectIdState = status
    ? status.configuredProjectIdMode === 'fixed' || status.effectiveProjectIdMode === 'fixed'
    : false
  const sessionBindingCardDescription = status?.activeUpstreamMcpSessions
    ? language === 'zh'
      ? '这些会话仍会阻塞 precise cutover。点击进入绑定记录页后可逐条、批量或按当前筛选全部释放。'
      : 'These sessions still block precise cutover. Open the binding records page to release one, selected, or all active matches.'
    : language === 'zh'
      ? '当前没有待处理的 legacy `upstream_mcp` session，precise cutover 不再受该门禁阻塞。'
      : 'No legacy `upstream_mcp` sessions are pending. This gate no longer blocks precise cutover.'
  const reconciliationModeLabel = status
    ? status.phase === 'active'
      ? strings.statusActive
      : status.phase === 'compare'
        ? strings.statusCompareOnly
        : strings.statusConfigured
    : strings.statusConfigured

  return (
    <section className="surface panel upstream-privacy-shell">
      <div className="upstream-privacy-shell__toolbar">
        {status ? (
          <p className="upstream-privacy-shell__meta">
            {strings.generatedAt} · {timestampFormatter.format(new Date(status.generatedAt * 1000))}
          </p>
        ) : null}
        <div className="upstream-privacy-shell__actions">
          <div className="upstream-privacy-auto-refresh" role="group" aria-labelledby={autoRefreshLabelId}>
            <span id={autoRefreshLabelId}>{strings.autoRefresh}</span>
            <Switch
              aria-labelledby={autoRefreshLabelId}
              checked={autoRefreshEnabled}
              onCheckedChange={onAutoRefreshChange}
            />
          </div>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="upstream-privacy-refresh-button"
            onClick={() => void onRefresh()}
            disabled={refreshing}
          >
            <Icon icon={refreshing ? 'mdi:loading' : 'mdi:refresh'} width={16} height={16} className={refreshing ? 'icon-spin' : undefined} />
            <span>{strings.refreshNow}</span>
          </Button>
        </div>
      </div>

      <AdminLoadingRegion
        loadState={loadState}
        loadingLabel={strings.loading}
        errorLabel={error ?? strings.loadFailed}
        minHeight={280}
      >
        {!status ? (
          <div className="empty-state alert">{strings.empty}</div>
        ) : (
          <div className="upstream-privacy-layout">
            <section className="upstream-privacy-overview">
              <div className="upstream-privacy-overview__main">
                <div className="upstream-privacy-hero__headline">
                  <StatusBadge tone={phaseTone(status.phase)}>{phaseLabel}</StatusBadge>
                  <StatusBadge tone={status.completedGates === status.totalGates ? 'success' : 'warning'}>
                    {numberFormatter.format(status.completedGates)}/{numberFormatter.format(status.totalGates)}
                  </StatusBadge>
                </div>
                <p className="upstream-privacy-overview__summary">{phaseDescription}</p>
                {summarySignals.length > 0 ? (
                  <div className="upstream-privacy-signal-list">
                    {summarySignals.map((signal) => (
                      <article key={signal.label} className="upstream-privacy-signal">
                        <span>{signal.label}</span>
                        <strong>{signal.value}</strong>
                      </article>
                    ))}
                  </div>
                ) : null}
              </div>
              <div className="upstream-privacy-overview__side">
                <PrivacyStat
                  label={strings.projectIdModeEffective}
                  value={modeLabel(formStrings, status.effectiveProjectIdMode)}
                />
                <PrivacyStat
                  label={strings.currentPeriod}
                  value={status.currentPeriodCode}
                  supportingText={`${strings.currentPeriodEndsAt} · ${timestampFormatter.format(new Date(status.currentPeriodEndsAt * 1000))}`}
                  monospace
                />
                <PrivacyStat
                  label={strings.nextEpochAt}
                  value={
                    status.nextEpochAt == null
                      ? strings.statusMissing
                      : timestampFormatter.format(new Date(status.nextEpochAt * 1000))
                  }
                />
                <PrivacyStat
                  label={strings.reconciliationMode}
                  value={reconciliationModeLabel}
                />
                <PrivacyStat
                  label={strings.userAgentEffective}
                  value={formatOptionalValue(status.effectiveMcpUserAgent, strings.statusOmitted)}
                />
              </div>
            </section>

            <section className="upstream-privacy-section">
              <button
                type="button"
                className="upstream-privacy-stat"
                style={{ width: '100%', textAlign: 'left', cursor: 'pointer' }}
                onClick={onOpenMcpSessionBindings}
              >
                <span>{sessionBindingCardLabel}</span>
                <div style={{ display: 'flex', justifyContent: 'space-between', gap: 12, alignItems: 'center' }}>
                  <strong>{numberFormatter.format(status.activeUpstreamMcpSessions)}</strong>
                  <StatusBadge tone={status.activeUpstreamMcpSessions > 0 ? 'warning' : 'success'}>
                    {status.activeUpstreamMcpSessions > 0 ? strings.gateWaiting : strings.gateReady}
                  </StatusBadge>
                </div>
                <small>{sessionBindingCardDescription}</small>
              </button>
            </section>

            <section className="upstream-privacy-section">
              <div className="panel-header">
                <div>
                  <h3>{strings.attentionTitle}</h3>
                  <p className="panel-description">
                    {statusIssues.length === 0 ? strings.attentionClear : strings.attentionDescription}
                  </p>
                </div>
                <StatusBadge tone={phaseTone(status.phase)}>{phaseLabel}</StatusBadge>
              </div>
              {statusIssues.length === 0 ? (
                <div className="upstream-privacy-empty-note">{strings.attentionClear}</div>
              ) : (
                <div className="upstream-privacy-issue-list">
                  {statusIssues.map((issue) => (
                    <article key={issue.key} className="upstream-privacy-issue">
                      <div className="upstream-privacy-issue__copy">
                        <strong>{issue.title}</strong>
                        <p>{issue.detail}</p>
                      </div>
                      <StatusBadge tone={issue.tone}>
                        {issue.tone === 'error' ? strings.phaseDegraded : strings.gateWaiting}
                      </StatusBadge>
                    </article>
                  ))}
                </div>
              )}
            </section>

            <section className="upstream-privacy-section">
              <div className="panel-header">
                <div>
                  <h3>{strings.countersTitle}</h3>
                </div>
              </div>
              <div className="upstream-privacy-counters">
                <PrivacyStat label={sessionBindingSummaryLabel} value={numberFormatter.format(status.activeUpstreamMcpSessions)} />
                <PrivacyStat label={strings.counterPendingResearch} value={numberFormatter.format(status.pendingResearch)} />
                <PrivacyStat label={strings.counterQueuedSettlements} value={numberFormatter.format(status.queuedSettlements)} />
                <PrivacyStat label={strings.counterDegradedSettlements} value={numberFormatter.format(status.degradedSettlements)} />
              </div>
            </section>

            <details className="upstream-privacy-details" data-testid="system-status-technical-details">
              <summary className="upstream-privacy-details__summary">
                <div>
                  <strong>{strings.detailsTitle}</strong>
                  <p>{strings.detailsDescription}</p>
                </div>
                <div className="upstream-privacy-details__meta">
                  <span>
                    {strings.configurationTitle} · {numberFormatter.format(configurationDriftCount)}
                  </span>
                  <span>
                    {strings.adjustmentsTitle} · {numberFormatter.format(status.recentAdjustments.length)}
                  </span>
                </div>
              </summary>
              <div className="upstream-privacy-details__body">
                <section className="upstream-privacy-detail-section">
                  <div className="panel-header">
                    <div>
                      <h3>{strings.configurationTitle}</h3>
                      <p className="panel-description">
                        {configurationDriftCount === 0 ? strings.configurationAligned : strings.detailsDescription}
                      </p>
                    </div>
                  </div>
                  <div className="upstream-privacy-counters">
                    <PrivacyStat
                      label={strings.projectIdModeConfigured}
                      value={modeLabel(formStrings, status.configuredProjectIdMode)}
                    />
                    <PrivacyStat
                      label={strings.projectIdModeEffective}
                      value={modeLabel(formStrings, status.effectiveProjectIdMode)}
                    />
                    {showFixedProjectIdState ? (
                      <PrivacyStat
                        label={strings.fixedConfigured}
                        value={status.fixedProjectIdConfigured ? strings.statusConfigured : strings.statusMissing}
                      />
                    ) : null}
                    <PrivacyStat
                      label={strings.userAgentConfigured}
                      value={formatOptionalValue(status.configuredMcpUserAgent, strings.statusOmitted)}
                    />
                    <PrivacyStat
                      label={strings.userAgentEffective}
                      value={formatOptionalValue(status.effectiveMcpUserAgent, strings.statusOmitted)}
                    />
                    <PrivacyStat
                      label={strings.reconciliationMode}
                      value={reconciliationModeLabel}
                    />
                    <PrivacyStat label={strings.generatedAt} value={timestampFormatter.format(new Date(status.generatedAt * 1000))} />
                  </div>
                </section>

                <section className="upstream-privacy-detail-section">
                  <div className="panel-header">
                    <div>
                      <h3>{strings.gateTitle}</h3>
                      <p className="panel-description">{strings.gateDescription}</p>
                    </div>
                  </div>
                  <div className="upstream-privacy-gates">
                    {status.gates.map((gate) => (
                      <article key={gate.key} className="upstream-privacy-gate">
                        <div className="upstream-privacy-gate__head">
                          <strong>{gateLabel(strings, language, gate)}</strong>
                          <StatusBadge tone={gate.ready ? 'success' : 'warning'}>
                            {gate.ready ? strings.gateReady : strings.gateWaiting}
                          </StatusBadge>
                        </div>
                        <code>{gate.detail}</code>
                      </article>
                    ))}
                  </div>
                </section>

                <section className="upstream-privacy-detail-section">
                  <div className="panel-header">
                    <div>
                      <h3>{strings.headersTitle}</h3>
                    </div>
                  </div>
                  <div className="upstream-privacy-header-groups">
                    <HeaderList title={strings.headersHttpTitle} items={status.httpAllowedHeaders} />
                    <HeaderList title={strings.headersControlTitle} items={status.controlMcpAllowedHeaders} />
                  </div>
                </section>

                <section className="upstream-privacy-detail-section">
                  <div className="panel-header">
                    <div>
                      <h3>{strings.adjustmentsTitle}</h3>
                    </div>
                  </div>
                  {status.recentAdjustments.length === 0 ? (
                    <div className="empty-state alert">{strings.adjustmentsEmpty}</div>
                  ) : (
                    <div className="upstream-privacy-adjustments">
                      {status.recentAdjustments.map((adjustment) => (
                        <article key={adjustment.settlementKey} className="upstream-privacy-adjustment">
                          <div className="upstream-privacy-adjustment__head">
                            <strong>{adjustment.periodCode}</strong>
                            <StatusBadge tone={adjustment.deltaCredits >= 0 ? 'warning' : 'success'}>
                              {formatSignedCount(adjustment.deltaCredits)}
                            </StatusBadge>
                          </div>
                          <dl>
                            <PrivacyDetail label={strings.adjustmentSubject} value={`${adjustment.billingSubjectKind}:${adjustment.tokenIdHint}`} />
                            <PrivacyDetail label={strings.adjustmentCreatedAt} value={timestampFormatter.format(new Date(adjustment.createdAt * 1000))} />
                            <PrivacyDetail label={strings.adjustmentSettlementKey} value={adjustment.settlementKey} monospace />
                            {adjustment.degradedReason ? (
                              <PrivacyDetail label={strings.degradedReason} value={adjustment.degradedReason} />
                            ) : null}
                          </dl>
                        </article>
                      ))}
                    </div>
                  )}
                </section>
              </div>
            </details>
          </div>
        )}
      </AdminLoadingRegion>
    </section>
  )
}

function HeaderList({ title, items }: { title: string; items: string[] }): JSX.Element {
  return (
    <div className="upstream-privacy-header-list">
      <strong>{title}</strong>
      {items.length === 0 ? (
        <span className="panel-description">—</span>
      ) : (
        <div className="upstream-privacy-pill-list">
          {items.map((item) => (
            <code key={item}>{item}</code>
          ))}
        </div>
      )}
    </div>
  )
}

function PrivacyStat({
  label,
  value,
  supportingText,
  monospace = false,
}: {
  label: string
  value: string
  supportingText?: string
  monospace?: boolean
}): JSX.Element {
  return (
    <article className="upstream-privacy-stat">
      <span>{label}</span>
      <strong className={monospace ? 'font-mono' : undefined}>{value}</strong>
      {supportingText ? <small>{supportingText}</small> : null}
    </article>
  )
}

function PrivacyDetail({
  label,
  value,
  monospace = false,
}: {
  label: string
  value: string
  monospace?: boolean
}): JSX.Element {
  return (
    <div className="upstream-privacy-detail">
      <dt>{label}</dt>
      <dd className={monospace ? 'font-mono' : undefined}>{value}</dd>
    </div>
  )
}
