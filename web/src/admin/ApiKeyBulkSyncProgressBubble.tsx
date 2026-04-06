import { type CSSProperties, useEffect } from 'react'
import { createPortal } from 'react-dom'

import type { AdminTranslations } from '../i18n'
import { Icon } from '../lib/icons'
import { useAnchoredFloatingLayer } from '../lib/useAnchoredFloatingLayer'
import type { ApiKeyBulkSyncProgressState } from './apiKeyBulkSyncProgress'

interface ApiKeyBulkSyncProgressBubbleProps {
  strings: AdminTranslations['keys']['bulkSyncProgress']
  progress: ApiKeyBulkSyncProgressState
  className?: string
  style?: CSSProperties
}

interface AnchoredApiKeyBulkSyncProgressBubbleProps {
  anchorEl: HTMLElement | null
  strings: AdminTranslations['keys']['bulkSyncProgress']
  progress: ApiKeyBulkSyncProgressState
  className?: string
  fallbackPosition?: {
    top: number
    left: number
  } | null
  onDismiss?: () => void
}

const numberFormatter = new Intl.NumberFormat()

function formatCount(value: number): string {
  return numberFormatter.format(value)
}

function resultToneClass(status: 'success' | 'skipped' | 'failed'): string {
  if (status === 'success') return 'text-success'
  if (status === 'skipped') return 'text-warning'
  return 'text-destructive'
}

function resultLabel(
  strings: AdminTranslations['keys']['bulkSyncProgress'],
  status: 'success' | 'skipped' | 'failed',
): string {
  if (status === 'success') return strings.result.success
  if (status === 'skipped') return strings.result.skipped
  return strings.result.failed
}

export function ApiKeyBulkSyncProgressBubble({
  strings,
  progress,
  className,
  style,
}: ApiKeyBulkSyncProgressBubbleProps): JSX.Element {
  return (
    <div
      className={`rounded-2xl border border-primary/25 bg-popover/98 px-4 py-3 text-popover-foreground shadow-[0_18px_44px_-28px_hsl(var(--primary)/0.75)] backdrop-blur ${className ?? ''}`}
      style={style}
    >
      <div className="space-y-1">
        <p className="text-sm font-semibold text-foreground">{strings.title}</p>
        <p className="text-xs text-muted-foreground">
          {progress.error
            ? progress.error
            : progress.completed
              ? strings.finished
              : strings.running}
        </p>
      </div>

      <div className="mt-3 flex flex-wrap gap-2">
        <span className="rounded-full border border-border/70 bg-background/80 px-2.5 py-1 text-[11px] font-medium text-foreground">
          {strings.counters.progress
            .replace('{current}', formatCount(progress.current))
            .replace('{total}', formatCount(progress.total))}
        </span>
        <span className="rounded-full border border-success/25 bg-success/10 px-2.5 py-1 text-[11px] font-medium text-success">
          {strings.counters.success.replace('{count}', formatCount(progress.summary.succeeded))}
        </span>
        <span className="rounded-full border border-warning/25 bg-warning/10 px-2.5 py-1 text-[11px] font-medium text-warning">
          {strings.counters.skipped.replace('{count}', formatCount(progress.summary.skipped))}
        </span>
        <span className="rounded-full border border-destructive/25 bg-destructive/10 px-2.5 py-1 text-[11px] font-medium text-destructive">
          {strings.counters.failed.replace('{count}', formatCount(progress.summary.failed))}
        </span>
      </div>

      <div className="mt-3 space-y-2">
        {progress.steps.map((step) => {
          const icon =
            step.status === 'done'
              ? 'mdi:check-circle'
              : step.status === 'error'
                ? 'mdi:alert-circle'
                : step.status === 'running'
                  ? 'mdi:loading'
                  : 'mdi:circle-outline'
          const toneClass =
            step.status === 'done'
              ? 'text-success'
              : step.status === 'error'
                ? 'text-destructive'
                : step.status === 'running'
                  ? 'text-primary'
                  : 'text-muted-foreground'
          const label =
            step.key === 'prepare_request'
              ? strings.steps.prepareRequest
              : step.key === 'sync_usage'
                ? strings.steps.syncUsage
                : strings.steps.refreshUi
          const detail =
            step.detail
            ?? (step.status === 'done'
              ? strings.status.done
              : step.status === 'error'
                ? strings.status.failed
                : step.status === 'running'
                  ? strings.status.running
                  : strings.status.waiting)

          return (
            <div
              key={step.key}
              className={`flex items-start gap-3 rounded-2xl border px-3 py-2 ${
                step.status === 'running'
                  ? 'border-primary/35 bg-background/88'
                  : step.status === 'error'
                    ? 'border-destructive/30 bg-destructive/5'
                    : step.status === 'done'
                      ? 'border-success/25 bg-success/5'
                      : 'border-border/60 bg-background/72'
              }`}
            >
              <Icon
                icon={icon}
                className={`${toneClass} mt-0.5 text-base ${step.status === 'running' ? 'animate-spin' : ''}`}
              />
              <div className="min-w-0">
                <p className="text-sm font-medium text-foreground">{label}</p>
                <p className="text-xs text-muted-foreground">{detail}</p>
              </div>
            </div>
          )
        })}
      </div>

      {progress.lastResult ? (
        <div className="mt-3 rounded-2xl border border-border/70 bg-background/78 px-3 py-2">
          <div className="flex items-center justify-between gap-3 text-xs">
            <span className="font-medium text-muted-foreground">{strings.lastResultLabel}</span>
            <span className={`font-semibold ${resultToneClass(progress.lastResult.status)}`}>
              {resultLabel(strings, progress.lastResult.status)}
            </span>
          </div>
          <p className="mt-1 text-sm font-medium text-foreground">{progress.lastResult.keyId}</p>
          <p className="mt-1 text-xs text-muted-foreground">
            {progress.lastResult.detail ?? strings.result.noDetail}
          </p>
        </div>
      ) : null}
    </div>
  )
}

export function AnchoredApiKeyBulkSyncProgressBubble({
  anchorEl,
  strings,
  progress,
  className,
  fallbackPosition,
  onDismiss,
}: AnchoredApiKeyBulkSyncProgressBubbleProps): JSX.Element | null {
  const { layerRef, position } = useAnchoredFloatingLayer<HTMLDivElement>({
    open: Boolean(anchorEl),
    anchorEl,
    placement: 'bottom',
    align: 'center',
    offset: 10,
    viewportMargin: 12,
    arrowPadding: 18,
  })

  useEffect(() => {
    if (!onDismiss) return

    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target as Node | null
      if (!target) return
      if (anchorEl?.contains(target)) return
      if (layerRef.current?.contains(target)) return
      onDismiss()
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      onDismiss()
      anchorEl?.focus()
    }

    document.addEventListener('pointerdown', handlePointerDown, true)
    document.addEventListener('keydown', handleKeyDown)
    return () => {
      document.removeEventListener('pointerdown', handlePointerDown, true)
      document.removeEventListener('keydown', handleKeyDown)
    }
  }, [anchorEl, layerRef, onDismiss])

  const resolvedPosition = position
    ? { top: position.top, left: position.left, useTransform: false }
    : fallbackPosition
      ? { top: fallbackPosition.top, left: fallbackPosition.left, useTransform: true }
      : null

  if ((!anchorEl && !fallbackPosition) || typeof document === 'undefined') {
    return null
  }

  return createPortal(
    <div
      ref={layerRef}
      className={`layer-popover ${className ?? ''}`}
      data-placement={position?.placement ?? 'bottom'}
      style={{
        position: 'fixed',
        top: `${resolvedPosition?.top ?? 0}px`,
        left: `${resolvedPosition?.left ?? 0}px`,
        transform: resolvedPosition?.useTransform ? 'translateX(-50%)' : undefined,
        visibility: resolvedPosition ? 'visible' : 'hidden',
        pointerEvents: resolvedPosition ? 'auto' : 'none',
        width: 'min(26rem, calc(100vw - 1.5rem))',
      }}
    >
      <ApiKeyBulkSyncProgressBubble strings={strings} progress={progress} />
    </div>,
    document.body,
  )
}
