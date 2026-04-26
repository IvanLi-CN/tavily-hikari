import { useEffect, useState } from 'react'

import type { SystemSettings } from '../api'
import type { QueryLoadState } from './queryLoadState'
import type { AdminTranslations } from '../i18n'
import AdminLoadingRegion from '../components/AdminLoadingRegion'
import { Icon } from '../lib/icons'
import { Button } from '../components/ui/button'
import { Input } from '../components/ui/input'
import { Switch } from '../components/ui/switch'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '../components/ui/tooltip'

interface SystemSettingsModuleProps {
  strings: AdminTranslations['systemSettings']
  settings: SystemSettings | null
  loadState: QueryLoadState
  error: string | null
  saving: boolean
  helpBubbleOpen?: boolean
  onApply: (settings: SystemSettings) => Promise<void> | void
}

function isValidCountDraft(value: string): value is `${number}` {
  if (!/^\d+$/.test(value)) return false
  const parsed = Number.parseInt(value, 10)
  return Number.isSafeInteger(parsed) && parsed >= 1 && parsed <= 1000
}

function isValidNonNegativeIntegerDraft(value: string): value is `${number}` {
  if (!/^\d+$/.test(value)) return false
  const parsed = Number.parseInt(value, 10)
  return Number.isSafeInteger(parsed) && parsed >= 0
}

function isValidRequestRateLimitDraft(value: string): value is `${number}` {
  if (!/^\d+$/.test(value)) return false
  const parsed = Number.parseInt(value, 10)
  return Number.isSafeInteger(parsed) && parsed >= 1
}

function isValidPercentDraft(value: string): value is `${number}` {
  if (!/^\d+$/.test(value)) return false
  const parsed = Number.parseInt(value, 10)
  return Number.isSafeInteger(parsed) && parsed >= 0 && parsed <= 100
}

function SystemSettingsHelpBubble({
  strings,
  open,
}: {
  strings: AdminTranslations['systemSettings']
  open?: boolean
}): JSX.Element {
  return (
    <TooltipProvider>
      <Tooltip {...(open == null ? {} : { open })}>
        <TooltipTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            size="xs"
            className="h-7 w-7 rounded-full px-0 text-muted-foreground hover:text-foreground"
            aria-label={strings.helpLabel}
            data-testid="system-settings-help-trigger"
          >
            <Icon icon="mdi:help-circle-outline" width={16} height={16} aria-hidden="true" />
          </Button>
        </TooltipTrigger>
        <TooltipContent side="right" align="start" className="max-w-[min(24rem,calc(100vw-2rem))]">
          <div style={{ display: 'grid', gap: 8 }}>
            <p>{strings.description}</p>
            <p>{strings.form.description}</p>
            <p>{strings.form.requestRateLimitHint}</p>
            <p>{strings.form.countHint}</p>
            <p>{strings.form.rebalanceHint}</p>
            <p>{strings.form.percentHint}</p>
            <p>{strings.form.blockedKeyBaseLimitHint}</p>
            <p>{strings.form.applyScopeHint}</p>
          </div>
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  )
}

export default function SystemSettingsModule({
  strings,
  settings,
  loadState,
  error,
  saving,
  helpBubbleOpen,
  onApply,
}: SystemSettingsModuleProps): JSX.Element {
  const [draftRequestRateLimit, setDraftRequestRateLimit] = useState(() =>
    settings ? String(settings.requestRateLimit) : '100',
  )
  const [draftCount, setDraftCount] = useState(() =>
    settings ? String(settings.mcpSessionAffinityKeyCount) : '',
  )
  const [draftRebalanceEnabled, setDraftRebalanceEnabled] = useState(
    settings?.rebalanceMcpEnabled ?? false,
  )
  const [draftPercent, setDraftPercent] = useState(() =>
    settings ? String(settings.rebalanceMcpSessionPercent) : '100',
  )
  const [draftBlockedKeyBaseLimit, setDraftBlockedKeyBaseLimit] = useState(() =>
    settings ? String(settings.userBlockedKeyBaseLimit) : '5',
  )

  useEffect(() => {
    setDraftRequestRateLimit(settings ? String(settings.requestRateLimit) : '100')
    setDraftCount(settings ? String(settings.mcpSessionAffinityKeyCount) : '')
    setDraftRebalanceEnabled(settings?.rebalanceMcpEnabled ?? false)
    setDraftPercent(settings ? String(settings.rebalanceMcpSessionPercent) : '100')
    setDraftBlockedKeyBaseLimit(settings ? String(settings.userBlockedKeyBaseLimit) : '5')
  }, [
    settings?.requestRateLimit,
    settings?.mcpSessionAffinityKeyCount,
    settings?.rebalanceMcpEnabled,
    settings?.rebalanceMcpSessionPercent,
    settings?.userBlockedKeyBaseLimit,
  ])

  const normalizedRequestRateLimit = draftRequestRateLimit.trim()
  const normalizedCount = draftCount.trim()
  const normalizedPercent = draftPercent.trim()
  const normalizedBlockedKeyBaseLimit = draftBlockedKeyBaseLimit.trim()
  const parsedRequestRateLimit = isValidRequestRateLimitDraft(normalizedRequestRateLimit)
    ? Number.parseInt(normalizedRequestRateLimit, 10)
    : null
  const parsedCount = isValidCountDraft(normalizedCount) ? Number.parseInt(normalizedCount, 10) : null
  const parsedPercent = isValidPercentDraft(normalizedPercent)
    ? Number.parseInt(normalizedPercent, 10)
    : null
  const parsedBlockedKeyBaseLimit = isValidNonNegativeIntegerDraft(normalizedBlockedKeyBaseLimit)
    ? Number.parseInt(normalizedBlockedKeyBaseLimit, 10)
    : null
  const changed =
    settings != null &&
    parsedRequestRateLimit != null &&
    parsedCount != null &&
    parsedPercent != null &&
    parsedBlockedKeyBaseLimit != null &&
    (parsedRequestRateLimit !== settings.requestRateLimit ||
      parsedCount !== settings.mcpSessionAffinityKeyCount ||
      draftRebalanceEnabled !== settings.rebalanceMcpEnabled ||
      parsedPercent !== settings.rebalanceMcpSessionPercent ||
      parsedBlockedKeyBaseLimit !== settings.userBlockedKeyBaseLimit)
  const inlineError =
    normalizedRequestRateLimit.length > 0 && parsedRequestRateLimit == null
      ? strings.form.invalidRequestRateLimit
      : normalizedCount.length > 0 && parsedCount == null
      ? strings.form.invalidCount
      : normalizedPercent.length > 0 && parsedPercent == null
        ? strings.form.invalidPercent
        : normalizedBlockedKeyBaseLimit.length > 0 && parsedBlockedKeyBaseLimit == null
          ? strings.form.invalidBlockedKeyBaseLimit
          : error

  return (
    <section className="surface panel">
      <div className="panel-header">
        <div>
          <h2>{strings.title}</h2>
        </div>
      </div>

      <AdminLoadingRegion
        loadState={loadState}
        loadingLabel={strings.description}
        errorLabel={error ?? undefined}
        minHeight={260}
      >
        <div
          className="rounded-2xl border border-border/60 bg-background/55 p-5 shadow-sm backdrop-blur"
          style={{ display: 'grid', gap: 20 }}
        >
          <div>
            <h3 className="text-base font-semibold">{strings.form.title}</h3>
          </div>

          <div style={{ display: 'grid', gap: 12, gridTemplateColumns: 'minmax(220px, 420px)' }}>
            <div style={{ display: 'grid', gap: 8 }}>
              <label className="text-sm font-medium" htmlFor="system-settings-request-rate-limit">
                {strings.form.requestRateLimitLabel}
              </label>
              <Input
                id="system-settings-request-rate-limit"
                type="number"
                inputMode="numeric"
                min={1}
                step={1}
                value={draftRequestRateLimit}
                disabled={saving}
                onChange={(event) => setDraftRequestRateLimit(event.target.value)}
                aria-invalid={inlineError ? true : undefined}
              />
              {settings && (
                <p className="text-xs text-muted-foreground">
                  {strings.form.currentRequestRateLimitValue.replace(
                    '{count}',
                    String(settings.requestRateLimit),
                  )}
                </p>
              )}
              <p className="text-xs text-muted-foreground">{strings.form.requestRateLimitHint}</p>
            </div>

            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <label className="text-sm font-medium" htmlFor="system-settings-affinity-count">
                {strings.form.countLabel}
              </label>
              <SystemSettingsHelpBubble strings={strings} open={helpBubbleOpen} />
            </div>
            <Input
              id="system-settings-affinity-count"
              type="number"
              inputMode="numeric"
              min={1}
              max={1000}
              step={1}
              value={draftCount}
              disabled={saving}
              onChange={(event) => setDraftCount(event.target.value)}
              aria-invalid={inlineError ? true : undefined}
            />
            {settings && (
              <p className="text-xs text-muted-foreground">
                {strings.form.currentValue.replace('{count}', String(settings.mcpSessionAffinityKeyCount))}
              </p>
            )}

            <div className="mt-2 flex items-start justify-between gap-4 rounded-2xl border border-border/60 bg-background/70 px-4 py-3">
              <div style={{ display: 'grid', gap: 4 }}>
                <label className="text-sm font-medium" htmlFor="system-settings-rebalance-switch">
                  {strings.form.rebalanceLabel}
                </label>
                <p className="text-xs text-muted-foreground">{strings.form.rebalanceHint}</p>
              </div>
              <Switch
                aria-label={strings.form.rebalanceLabel}
                id="system-settings-rebalance-switch"
                checked={draftRebalanceEnabled}
                onCheckedChange={setDraftRebalanceEnabled}
                disabled={saving}
              />
            </div>

            <div style={{ display: 'grid', gap: 8 }}>
              <label className="text-sm font-medium" htmlFor="system-settings-rebalance-percent">
                {strings.form.percentLabel}
              </label>
              <div className="grid gap-3 md:grid-cols-[minmax(0,1fr),96px] md:items-center">
                <input
                  id="system-settings-rebalance-percent"
                  className="range"
                  type="range"
                  min={0}
                  max={100}
                  step={1}
                  value={parsedPercent ?? 0}
                  disabled={saving || !draftRebalanceEnabled}
                  onChange={(event) => setDraftPercent(event.target.value)}
                  aria-label={strings.form.percentLabel}
                />
                <Input
                  type="number"
                  inputMode="numeric"
                  min={0}
                  max={100}
                  step={1}
                  value={draftPercent}
                  disabled={saving || !draftRebalanceEnabled}
                  onChange={(event) => setDraftPercent(event.target.value)}
                  aria-invalid={inlineError ? true : undefined}
                />
              </div>
              {settings && (
                <p className="text-xs text-muted-foreground">
                  {strings.form.currentPercentValue.replace(
                    '{percent}',
                    String(settings.rebalanceMcpSessionPercent),
                  )}
                </p>
              )}
              <p className="text-xs text-muted-foreground">
                {draftRebalanceEnabled ? strings.form.percentHint : strings.form.percentDisabledHint}
              </p>
            </div>

            <div style={{ display: 'grid', gap: 8 }}>
              <label className="text-sm font-medium" htmlFor="system-settings-blocked-key-base-limit">
                {strings.form.blockedKeyBaseLimitLabel}
              </label>
              <Input
                id="system-settings-blocked-key-base-limit"
                type="number"
                inputMode="numeric"
                min={0}
                step={1}
                value={draftBlockedKeyBaseLimit}
                disabled={saving}
                onChange={(event) => setDraftBlockedKeyBaseLimit(event.target.value)}
                aria-invalid={inlineError ? true : undefined}
              />
              {settings && (
                <p className="text-xs text-muted-foreground">
                  {strings.form.currentBlockedKeyBaseLimitValue.replace(
                    '{count}',
                    String(settings.userBlockedKeyBaseLimit),
                  )}
                </p>
              )}
              <p className="text-xs text-muted-foreground">{strings.form.blockedKeyBaseLimitHint}</p>
            </div>
          </div>

          {(inlineError || saving) && (
            <p
              className="text-sm font-medium"
              role="status"
              aria-live="polite"
              style={{ color: inlineError ? 'hsl(var(--destructive))' : undefined }}
            >
              {inlineError ?? strings.actions.applying}
            </p>
          )}

          <div style={{ display: 'flex', justifyContent: 'flex-start' }}>
            <Button
              type="button"
              onClick={() => {
                if (
                  parsedRequestRateLimit == null ||
                  parsedCount == null ||
                  parsedPercent == null ||
                  parsedBlockedKeyBaseLimit == null ||
                  saving ||
                  !changed
                ) return
                void onApply({
                  requestRateLimit: parsedRequestRateLimit,
                  mcpSessionAffinityKeyCount: parsedCount,
                  rebalanceMcpEnabled: draftRebalanceEnabled,
                  rebalanceMcpSessionPercent: parsedPercent,
                  userBlockedKeyBaseLimit: parsedBlockedKeyBaseLimit,
                })
              }}
              disabled={
                saving ||
                !changed ||
                parsedRequestRateLimit == null ||
                parsedCount == null ||
                parsedPercent == null ||
                parsedBlockedKeyBaseLimit == null
              }
              data-testid="system-settings-apply"
            >
              <Icon
                icon={saving ? 'mdi:loading' : 'mdi:check-circle-outline'}
                width={16}
                height={16}
                className={saving ? 'icon-spin' : undefined}
                aria-hidden="true"
              />
              <span>{saving ? strings.actions.applying : strings.actions.apply}</span>
            </Button>
          </div>
        </div>
      </AdminLoadingRegion>
    </section>
  )
}
