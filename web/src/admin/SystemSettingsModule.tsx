import { useEffect, useState } from 'react'

import type { SystemSettings } from '../api'
import type { QueryLoadState } from './queryLoadState'
import type { AdminTranslations } from '../i18n'
import AdminLoadingRegion from '../components/AdminLoadingRegion'
import { Icon } from '../lib/icons'
import { Button } from '../components/ui/button'
import { Input } from '../components/ui/input'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '../components/ui/tooltip'

interface SystemSettingsModuleProps {
  strings: AdminTranslations['systemSettings']
  settings: SystemSettings | null
  loadState: QueryLoadState
  error: string | null
  saving: boolean
  helpBubbleOpen?: boolean
  onApply: (mcpSessionAffinityKeyCount: number) => Promise<void> | void
}

function isValidCountDraft(value: string): value is `${number}` {
  if (!/^\d+$/.test(value)) return false
  const parsed = Number.parseInt(value, 10)
  return Number.isSafeInteger(parsed) && parsed >= 1 && parsed <= 1000
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
            <p>{strings.form.countHint}</p>
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
  const [draftCount, setDraftCount] = useState(() =>
    settings ? String(settings.mcpSessionAffinityKeyCount) : '',
  )

  useEffect(() => {
    setDraftCount(settings ? String(settings.mcpSessionAffinityKeyCount) : '')
  }, [settings?.mcpSessionAffinityKeyCount])

  const normalizedDraft = draftCount.trim()
  const parsedCount = isValidCountDraft(normalizedDraft)
    ? Number.parseInt(normalizedDraft, 10)
    : null
  const changed = settings != null && parsedCount != null && parsedCount !== settings.mcpSessionAffinityKeyCount
  const inlineError =
    normalizedDraft.length > 0 && parsedCount == null ? strings.form.invalidCount : error

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
        minHeight={220}
      >
        <div
          className="rounded-2xl border border-border/60 bg-background/55 p-5 shadow-sm backdrop-blur"
          style={{ display: 'grid', gap: 16 }}
        >
          <div>
            <h3 className="text-base font-semibold">{strings.form.title}</h3>
          </div>

          <div
            style={{
              display: 'grid',
              gap: 12,
              gridTemplateColumns: 'minmax(220px, 320px)',
            }}
          >
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
                if (parsedCount == null || saving || !changed) return
                void onApply(parsedCount)
              }}
              disabled={saving || !changed || parsedCount == null}
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
