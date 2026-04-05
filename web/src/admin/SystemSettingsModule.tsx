import { useEffect, useState } from 'react'

import type { SystemSettings } from '../api'
import type { QueryLoadState } from './queryLoadState'
import type { AdminTranslations } from '../i18n'
import AdminLoadingRegion from '../components/AdminLoadingRegion'
import { Icon } from '../lib/icons'
import { Button } from '../components/ui/button'
import { Input } from '../components/ui/input'

interface SystemSettingsModuleProps {
  strings: AdminTranslations['systemSettings']
  settings: SystemSettings | null
  loadState: QueryLoadState
  error: string | null
  saving: boolean
  onApply: (mcpSessionAffinityKeyCount: number) => Promise<void> | void
}

function isValidCountDraft(value: string): value is `${number}` {
  if (!/^\d+$/.test(value)) return false
  const parsed = Number.parseInt(value, 10)
  return Number.isSafeInteger(parsed) && parsed >= 1 && parsed <= 1000
}

export default function SystemSettingsModule({
  strings,
  settings,
  loadState,
  error,
  saving,
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
          <p className="panel-description">{strings.description}</p>
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
            <p className="panel-description" style={{ marginTop: 6 }}>
              {strings.form.description}
            </p>
          </div>

          <div
            style={{
              display: 'grid',
              gap: 12,
              gridTemplateColumns: 'minmax(220px, 320px)',
            }}
          >
            <label className="text-sm font-medium" htmlFor="system-settings-affinity-count">
              {strings.form.countLabel}
            </label>
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
            <p className="text-xs text-muted-foreground">{strings.form.countHint}</p>
            {settings && (
              <p className="text-xs text-muted-foreground">
                {strings.form.currentValue.replace('{count}', String(settings.mcpSessionAffinityKeyCount))}
              </p>
            )}
            <p className="text-xs text-muted-foreground">{strings.form.applyScopeHint}</p>
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
