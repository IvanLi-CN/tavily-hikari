import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { CircleAlert } from 'lucide-react'

import type {
  HaSourceKind,
  HaSourceScheme,
  HaSourceSettings,
  HaSourceSettingsApiError,
  HaStatus,
} from '../api'
import { updateAdminHaSourceSettings } from '../api'
import type { AdminTranslations } from '../i18n'
import { Alert, AlertDescription, AlertTitle } from '../components/ui/alert'
import { Button } from '../components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '../components/ui/dialog'
import { Input } from '../components/ui/input'
import SegmentedTabs from '../components/ui/SegmentedTabs'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../components/ui/select'

type SubmitFailureState = {
  title: string
  description: string
  technicalDetail: string | null
}

type SubmittedSourceSnapshot = {
  sourceKind: HaSourceKind
}

const useIsomorphicLayoutEffect = typeof window === 'undefined' ? useEffect : useLayoutEffect

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
  submitSourceSettings?: typeof updateAdminHaSourceSettings
  dialogPortalContainer?: HTMLElement | null
}

function focusInput(ref: React.RefObject<HTMLInputElement | null>): void {
  ref.current?.focus()
}

export default function HaSourceSettingsDialog({
  open,
  status,
  strings,
  onOpenChange,
  onSaved,
  submitSourceSettings = updateAdminHaSourceSettings,
  dialogPortalContainer,
}: HaSourceSettingsDialogProps): JSX.Element {
  const submitFailureRef = useRef<HTMLDivElement | null>(null)
  const directHostInputRef = useRef<HTMLInputElement | null>(null)
  const directPortInputRef = useRef<HTMLInputElement | null>(null)
  const originGroupInputRef = useRef<HTMLInputElement | null>(null)
  const [sourceKind, setSourceKind] = useState<HaSourceKind>('direct')
  const [directOriginScheme, setDirectOriginScheme] = useState<HaSourceScheme>('https')
  const [directOriginHost, setDirectOriginHost] = useState('')
  const [directOriginPort, setDirectOriginPort] = useState('443')
  const [originGroupId, setOriginGroupId] = useState('')
  const [saving, setSaving] = useState(false)
  const [submitFailure, setSubmitFailure] = useState<SubmitFailureState | null>(null)
  const [localValidationAttempted, setLocalValidationAttempted] = useState(false)
  const [success, setSuccess] = useState<string | null>(null)
  const [technicalDetailsOpen, setTechnicalDetailsOpen] = useState(false)

  const draft = useMemo(() => toDraftSourceSettings(status), [status])
  const canApplyToEdgeone = status?.role === 'full_master' || status?.role === 'provisional_master'

  useEffect(() => {
    if (!open) return
    setSourceKind(draft.sourceKind)
    setDirectOriginScheme(draft.directOriginScheme ?? 'https')
    setDirectOriginHost(draft.directOriginHost ?? '')
    setDirectOriginPort(draft.directOriginPort != null ? String(draft.directOriginPort) : '443')
    setOriginGroupId(draft.originGroupId ?? '')
    setSubmitFailure(null)
    setLocalValidationAttempted(false)
    setSuccess(null)
    setTechnicalDetailsOpen(false)
  }, [canApplyToEdgeone, draft, open])

  const directHostError = sourceKind === 'direct' && directOriginHost.trim().length === 0 ? strings.sourceInvalidDirectHost : null
  const directPortError = sourceKind === 'direct' && validatePort(directOriginPort) == null ? strings.sourceInvalidDirectPort : null
  const originGroupError =
    sourceKind === 'origin_group' && originGroupId.trim().length === 0 ? strings.sourceInvalidOriginGroup : null
  const currentTargetLabel = status?.haSourceEffective?.target ?? status?.edgeoneCurrentTarget ?? status?.edgeoneOrigin ?? '—'

  const directFieldError = directHostError ?? directPortError
  useEffect(() => {
    if (!submitFailure) return
    submitFailureRef.current?.focus()
  }, [submitFailure])

  useIsomorphicLayoutEffect(() => {
    if (!localValidationAttempted) return
    if (!open) return
    if (sourceKind === 'direct' && directHostError) {
      focusInput(directHostInputRef)
      return
    }
    if (sourceKind === 'direct' && directPortError) {
      focusInput(directPortInputRef)
      return
    }
    if (sourceKind === 'origin_group' && originGroupError) {
      focusInput(originGroupInputRef)
    }
  }, [directHostError, directPortError, open, originGroupError, sourceKind])

  function clearSubmitFailure(): void {
    setSubmitFailure(null)
    setTechnicalDetailsOpen(false)
  }

  function buildSubmitFailure(
    error: HaSourceSettingsApiError,
    applyToEdgeone: boolean,
    submittedSnapshot: SubmittedSourceSnapshot,
  ): SubmitFailureState {
    const rawDetail = error.rawDetail?.trim() ?? error.message.trim()
    const description =
      submittedSnapshot.sourceKind === 'direct'
        ? strings.sourceSubmitFailedDirectDescription
        : strings.sourceSubmitFailedOriginGroupDescription
    return {
      title: applyToEdgeone ? strings.sourceApplyFailedTitle : strings.sourceSaveFailedTitle,
      description,
      technicalDetail: rawDetail.length > 0 ? rawDetail : null,
    }
  }

  async function handleSubmit(applyToEdgeone: boolean): Promise<void> {
    if (!status) return
    const port = sourceKind === 'direct' ? validatePort(directOriginPort) : null
    if (sourceKind === 'direct' && (!directOriginHost.trim() || port == null)) {
      setLocalValidationAttempted(true)
      setSubmitFailure(null)
      if (!directOriginHost.trim()) {
        focusInput(directHostInputRef)
      } else if (port == null) {
        focusInput(directPortInputRef)
      }
      return
    }
    if (sourceKind === 'origin_group' && !originGroupId.trim()) {
      setLocalValidationAttempted(true)
      setSubmitFailure(null)
      focusInput(originGroupInputRef)
      return
    }

    setLocalValidationAttempted(false)
    setSaving(true)
    setSubmitFailure(null)
    const submittedSnapshot: SubmittedSourceSnapshot = { sourceKind }
    setSuccess(null)
    setTechnicalDetailsOpen(false)
    try {
      const nextStatus = await submitSourceSettings({
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
      const fallbackError = new Error(strings.sourceSaveFailed) as HaSourceSettingsApiError
      setSubmitFailure(
        buildSubmitFailure(
          err instanceof Error ? (err as HaSourceSettingsApiError) : fallbackError,
          applyToEdgeone,
          submittedSnapshot,
        ),
      )
    } finally {
      setSaving(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl" portalContainer={dialogPortalContainer}>
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
              disabled={saving}
              onChange={(nextSourceKind) => {
                clearSubmitFailure()
                setSourceKind(nextSourceKind)
              }}
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
                  <Select
                    value={directOriginScheme}
                    disabled={saving}
                    onValueChange={(value) => {
                      clearSubmitFailure()
                      setDirectOriginScheme(value as HaSourceScheme)
                    }}
                  >
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
                    ref={directHostInputRef}
                    value={directOriginHost}
                    disabled={saving}
                    aria-invalid={directHostError ? true : undefined}
                    aria-describedby={directHostError ? 'ha-source-direct-host-error' : undefined}
                    onChange={(event) => {
                      clearSubmitFailure()
                      setDirectOriginHost(event.target.value)
                    }}
                    placeholder="203.0.113.9"
                    className={directHostError ? 'border-destructive focus-visible:ring-destructive' : undefined}
                  />
                  {directHostError && (
                    <p id="ha-source-direct-host-error" className="text-sm font-medium text-destructive">
                      {directHostError}
                    </p>
                  )}
                </label>
                <label className="grid gap-2">
                  <span className="text-sm font-semibold">{strings.sourcePortLabel}</span>
                  <Input
                    ref={directPortInputRef}
                    inputMode="numeric"
                    value={directOriginPort}
                    disabled={saving}
                    aria-invalid={directPortError ? true : undefined}
                    aria-describedby={directPortError ? 'ha-source-direct-port-error' : undefined}
                    onChange={(event) => {
                      clearSubmitFailure()
                      setDirectOriginPort(event.target.value)
                    }}
                    placeholder="443"
                    className={directPortError ? 'border-destructive focus-visible:ring-destructive' : undefined}
                  />
                  {directPortError && (
                    <p id="ha-source-direct-port-error" className="text-sm font-medium text-destructive">
                      {directPortError}
                    </p>
                  )}
                </label>
              </div>
            </div>
          ) : (
            <div className="grid gap-2">
              <label className="grid gap-2">
                <span className="text-sm font-semibold">{strings.sourceGroupIdLabel}</span>
                <Input
                  ref={originGroupInputRef}
                  value={originGroupId}
                  disabled={saving}
                  aria-invalid={originGroupError ? true : undefined}
                  aria-describedby={originGroupError ? 'ha-source-origin-group-error' : undefined}
                  onChange={(event) => {
                    clearSubmitFailure()
                    setOriginGroupId(event.target.value)
                  }}
                  placeholder="eo-group-123"
                  className={originGroupError ? 'border-destructive focus-visible:ring-destructive' : undefined}
                />
              </label>
              {originGroupError && (
                <p id="ha-source-origin-group-error" className="text-sm font-medium text-destructive">
                  {originGroupError}
                </p>
              )}
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

          {submitFailure && (
            <Alert
              ref={submitFailureRef}
              variant="destructive"
              emphasis="prominent"
              aria-live="assertive"
              tabIndex={-1}
              className="grid gap-3 rounded-[28px] border-destructive/48 bg-destructive/13 px-5 py-4 text-foreground outline-none focus:outline-none"
            >
              <div className="flex items-start gap-3">
                <div
                  className="mt-0.5 flex h-11 w-11 shrink-0 items-center justify-center rounded-full border border-destructive/20 bg-destructive/18 text-destructive shadow-clayPressed"
                  aria-hidden="true"
                >
                  <CircleAlert size={18} strokeWidth={2.2} />
                </div>
                <div className="min-w-0 flex-1">
                  <AlertTitle className="text-[0.98rem] font-bold text-destructive">{submitFailure.title}</AlertTitle>
                  <AlertDescription className="mt-1 text-[0.92rem] text-destructive-readable">
                    <p>{submitFailure.description}</p>
                  </AlertDescription>
                </div>
              </div>
              {submitFailure.technicalDetail && (
                <details
                  className="rounded-[24px] border border-destructive/24 bg-destructive/8 px-4 py-3 shadow-clayPressed"
                  open={technicalDetailsOpen}
                  onToggle={(event) => setTechnicalDetailsOpen((event.currentTarget as HTMLDetailsElement).open)}
                >
                  <summary className="cursor-pointer select-none text-sm font-semibold text-destructive marker:text-destructive/70">
                    {strings.sourceTechnicalDetailsLabel}
                  </summary>
                  {technicalDetailsOpen && (
                    <pre className="mt-3 overflow-x-auto whitespace-pre-wrap break-words rounded-[18px] border border-destructive/18 bg-card/86 px-3 py-3 font-mono text-xs leading-5 text-destructive-readable shadow-clayPressed">
                      {submitFailure.technicalDetail}
                    </pre>
                  )}
                </details>
              )}
            </Alert>
          )}

          {!submitFailure && success && (
            <p className="text-sm font-medium text-success" role="status" aria-live="polite">
              {success}
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
            aria-disabled={saving}
            className={submitFailure ? 'opacity-70 saturate-[0.82] hover:translate-y-0 hover:shadow-clayButton' : undefined}
            onClick={() => void handleSubmit(false)}
          >
            {strings.sourceSave}
          </Button>
          {canApplyToEdgeone && (
            <Button
              type="button"
              disabled={saving}
              aria-disabled={saving}
              className={submitFailure ? 'opacity-72 saturate-[0.72] contrast-90 hover:translate-y-0 hover:shadow-clayButton' : undefined}
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
