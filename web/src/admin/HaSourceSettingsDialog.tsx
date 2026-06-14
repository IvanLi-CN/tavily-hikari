import { useEffect, useMemo, useState } from 'react'

import type { HaSourceKind, HaSourceScheme, HaSourceSettings, HaStatus } from '../api'
import { updateAdminHaSourceSettings } from '../api'
import type { AdminTranslations } from '../i18n'
import { Button } from '../components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '../components/ui/dialog'
import { Input } from '../components/ui/input'
import SegmentedTabs from '../components/ui/SegmentedTabs'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../components/ui/select'

type SourceDialogMode = 'save' | 'apply'

const sourceKindOptions: ReadonlyArray<{ value: HaSourceKind; labelKey: keyof AdminTranslations['systemSettings']['ha'] }> = [
  { value: 'direct', labelKey: 'sourceKindDirect' },
  { value: 'origin_group', labelKey: 'sourceKindOriginGroup' },
]

const sourceSchemeOptions: ReadonlyArray<{ value: HaSourceScheme; label: string }> = [
  { value: 'http', label: 'HTTP' },
  { value: 'https', label: 'HTTPS' },
  { value: 'follow', label: 'FOLLOW' },
]

function toDraftSourceSettings(status: HaStatus | null): HaSourceSettings {
  const settings = status?.haSourceEffective ?? status?.haSourceOverride ?? status?.haSourceDefaults
  if (settings) return settings
  return {
    sourceKind: 'direct',
    directOriginScheme: 'https',
    directOriginHost: status?.nodePublicOrigin?.split(':')[0] ?? null,
    directOriginPort: status?.nodePublicOrigin ? Number.parseInt(status.nodePublicOrigin.split(':').pop() ?? '443', 10) || 443 : 443,
    originGroupId: null,
    target: status?.nodePublicOrigin ?? null,
  }
}

function formatTargetPreview(settings: HaSourceSettings): string {
  if (settings.sourceKind === 'origin_group') return settings.originGroupId ?? '—'
  const host = settings.directOriginHost ?? '—'
  const port = settings.directOriginPort ?? '—'
  return `${host}:${port}`
}

function formatSourceSelectionSummary(
  sourceKind: HaSourceKind,
  directOriginScheme: HaSourceScheme,
  directOriginHost: string,
  directOriginPort: string,
  originGroupId: string,
  strings: AdminTranslations['systemSettings']['ha'],
): string {
  if (sourceKind === 'origin_group') {
    return originGroupId.trim() || strings.sourceInvalidOriginGroup
  }
  const host = directOriginHost.trim() || '—'
  const port = validatePort(directOriginPort)
  return `${directOriginScheme.toUpperCase()} · ${host}:${port ?? '—'}`
}

function validatePort(value: string): number | null {
  if (!/^\d+$/.test(value)) return null
  const parsed = Number.parseInt(value, 10)
  if (!Number.isInteger(parsed) || parsed < 1 || parsed > 65535) return null
  return parsed
}

interface HaSourceSettingsDialogProps {
  open: boolean
  status: HaStatus | null
  strings: AdminTranslations['systemSettings']['ha']
  onOpenChange: (open: boolean) => void
  onSaved: (status: HaStatus) => void
}

export default function HaSourceSettingsDialog({
  open,
  status,
  strings,
  onOpenChange,
  onSaved,
}: HaSourceSettingsDialogProps): JSX.Element {
  const [sourceKind, setSourceKind] = useState<HaSourceKind>('direct')
  const [directOriginScheme, setDirectOriginScheme] = useState<HaSourceScheme>('https')
  const [directOriginHost, setDirectOriginHost] = useState('')
  const [directOriginPort, setDirectOriginPort] = useState('443')
  const [originGroupId, setOriginGroupId] = useState('')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [success, setSuccess] = useState<string | null>(null)

  const draft = useMemo(() => toDraftSourceSettings(status), [status])
  const canApplyToEdgeone = status?.role === 'full_master' || status?.role === 'provisional_master'

  useEffect(() => {
    if (!open) return
    setSourceKind(draft.sourceKind)
    setDirectOriginScheme(draft.directOriginScheme ?? 'https')
    setDirectOriginHost(draft.directOriginHost ?? '')
    setDirectOriginPort(draft.directOriginPort != null ? String(draft.directOriginPort) : '443')
    setOriginGroupId(draft.originGroupId ?? '')
    setError(null)
    setSuccess(null)
  }, [canApplyToEdgeone, draft, open])

  const directHostError = sourceKind === 'direct' && directOriginHost.trim().length === 0 ? strings.sourceInvalidDirectHost : null
  const directPortError = sourceKind === 'direct' && validatePort(directOriginPort) == null ? strings.sourceInvalidDirectPort : null
  const originGroupError =
    sourceKind === 'origin_group' && originGroupId.trim().length === 0 ? strings.sourceInvalidOriginGroup : null
  const currentTargetLabel = status?.haSourceEffective?.target ?? status?.edgeoneCurrentTarget ?? status?.edgeoneOrigin ?? '—'

  async function handleSubmit(applyToEdgeone: boolean): Promise<void> {
    if (!status) return
    const port = sourceKind === 'direct' ? validatePort(directOriginPort) : null
    if (sourceKind === 'direct' && (!directOriginHost.trim() || port == null)) {
      setError(directHostError ?? directPortError ?? strings.sourceSaveFailed)
      return
    }
    if (sourceKind === 'origin_group' && !originGroupId.trim()) {
      setError(originGroupError ?? strings.sourceSaveFailed)
      return
    }

    setSaving(true)
    setError(null)
    setSuccess(null)
    try {
      const nextStatus = await updateAdminHaSourceSettings({
        sourceKind,
        directOriginScheme: sourceKind === 'direct' ? directOriginScheme : null,
        directOriginHost: sourceKind === 'direct' ? directOriginHost.trim() : null,
        directOriginPort: sourceKind === 'direct' ? port : null,
        originGroupId: sourceKind === 'origin_group' ? originGroupId.trim() : null,
        applyToEdgeone,
      })
      onSaved(nextStatus)
      setSuccess(applyToEdgeone ? strings.sourceApplied : strings.sourceSaved)
      onOpenChange(false)
    } catch (err) {
      setError(err instanceof Error ? err.message : strings.sourceSaveFailed)
    } finally {
      setSaving(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{strings.sourceDialogTitle}</DialogTitle>
          <DialogDescription>{strings.sourceDialogDescription}</DialogDescription>
        </DialogHeader>

        <div className="grid gap-4 py-2">
          <dl className="grid gap-3 rounded-[18px] border border-border/60 bg-muted/30 px-4 py-3 text-sm">
            <div className="grid gap-1">
              <dt className="font-semibold">{strings.summaryCurrentOrigin}</dt>
              <dd className="text-muted-foreground">{currentTargetLabel}</dd>
            </div>
            <div className="grid gap-1">
              <dt className="font-semibold">{strings.summaryCurrentSource}</dt>
              <dd className="text-muted-foreground">{draft.sourceKind === 'direct' ? strings.sourceKindDirect : strings.sourceKindOriginGroup}</dd>
            </div>
          </dl>

          <div className="grid gap-2 ha-source-kind-field">
            <span className="text-sm font-semibold">{strings.sourceKindLabel}</span>
            <SegmentedTabs<HaSourceKind>
              className="ha-source-kind-tabs"
              value={sourceKind}
              onChange={setSourceKind}
              ariaLabel={strings.sourceKindLabel}
              options={sourceKindOptions.map((option) => ({
                value: option.value,
                label: strings[option.labelKey],
              }))}
            />
          </div>

          <div className="ha-source-selection-card text-sm">
            <span className="font-semibold">
              {sourceKind === 'direct' ? strings.sourceSelectedDirectLabel : strings.sourceSelectedOriginGroupLabel}
            </span>
            <code className="ha-source-selection-preview">{formatSourceSelectionSummary(
              sourceKind,
              directOriginScheme,
              directOriginHost,
              directOriginPort,
              originGroupId,
              strings,
            )}</code>
            <p className="text-xs text-muted-foreground">{strings.sourceSelectedHint}</p>
          </div>

          {sourceKind === 'direct' ? (
            <div className="grid gap-4">
              <div className="grid gap-2">
                <label className="grid gap-2">
                  <span className="text-sm font-semibold">{strings.sourceSchemeLabel}</span>
                  <Select value={directOriginScheme} onValueChange={(value) => setDirectOriginScheme(value as HaSourceScheme)}>
                    <SelectTrigger>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {sourceSchemeOptions.map((option) => (
                        <SelectItem key={option.value} value={option.value}>
                          {option.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </label>
                <p className="text-xs text-muted-foreground">{strings.sourceDirectHint}</p>
              </div>

              <div className="grid gap-4 sm:grid-cols-[minmax(0,1.5fr)_minmax(10rem,0.6fr)]">
                <label className="grid gap-2">
                  <span className="text-sm font-semibold">{strings.sourceHostLabel}</span>
                  <Input
                    value={directOriginHost}
                    disabled={saving}
                    onChange={(event) => setDirectOriginHost(event.target.value)}
                    placeholder="203.0.113.9"
                  />
                </label>
                <label className="grid gap-2">
                  <span className="text-sm font-semibold">{strings.sourcePortLabel}</span>
                  <Input
                    inputMode="numeric"
                    value={directOriginPort}
                    disabled={saving}
                    onChange={(event) => setDirectOriginPort(event.target.value)}
                    placeholder="443"
                  />
                </label>
              </div>
            </div>
          ) : (
            <div className="grid gap-2">
              <label className="grid gap-2">
                <span className="text-sm font-semibold">{strings.sourceGroupIdLabel}</span>
                <Input
                  value={originGroupId}
                  disabled={saving}
                  onChange={(event) => setOriginGroupId(event.target.value)}
                  placeholder="eo-group-123"
                />
              </label>
              <p className="text-xs text-muted-foreground">{strings.sourceGroupHint}</p>
            </div>
          )}

          <div className="grid gap-1 text-sm">
            <div className="flex flex-wrap items-center gap-2">
              <span className="font-semibold">{strings.summaryExpectedOrigin}</span>
              <code className="rounded-full bg-muted px-2 py-1 text-xs">{formatTargetPreview({
                sourceKind,
                directOriginScheme,
                directOriginHost: sourceKind === 'direct' ? directOriginHost.trim() : null,
                directOriginPort: sourceKind === 'direct' ? validatePort(directOriginPort) : null,
                originGroupId: sourceKind === 'origin_group' ? originGroupId.trim() : null,
                target: null,
              })}</code>
            </div>
            <p className="text-xs text-muted-foreground">
              {canApplyToEdgeone ? strings.sourceSaveAndApply : strings.sourceSave}
            </p>
          </div>

          {(error || success || directHostError || directPortError || originGroupError) && (
            <p className="text-sm font-medium" role="status" aria-live="polite">
              {error ?? success ?? directHostError ?? directPortError ?? originGroupError}
            </p>
          )}
        </div>

        <DialogFooter className="gap-2 sm:justify-end">
          <Button type="button" variant="outline" disabled={saving} onClick={() => onOpenChange(false)}>
            {strings.sourceDialogCancel}
          </Button>
          <Button
            type="button"
            variant="secondary"
            disabled={saving}
            onClick={() => void handleSubmit(false)}
          >
            {strings.sourceSave}
          </Button>
          {canApplyToEdgeone && (
            <Button
              type="button"
              disabled={saving}
              onClick={() => void handleSubmit(true)}
            >
              {strings.sourceSaveAndApply}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
