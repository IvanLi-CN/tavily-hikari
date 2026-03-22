import { Icon } from '@iconify/react'
import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'

import type { AdminTranslations } from '../i18n'
import { useAnchoredFloatingLayer } from '../lib/useAnchoredFloatingLayer'
import { Input } from '../components/ui/input'
import { Switch } from '../components/ui/switch'
import type { ForwardProxyDialogProgressState } from './forwardProxyDialogProgress'
import ForwardProxyProgressBubble from './ForwardProxyProgressBubble'

export interface ForwardProxyEgressControlProps {
  strings: AdminTranslations['proxySettings']
  enabled: boolean
  url: string
  loading: boolean
  controlsDisabled: boolean
  inputLocked: boolean
  errorMessage?: string | null
  errorPresentation?: 'hint' | 'alert'
  progress: ForwardProxyDialogProgressState | null
  onToggle: (checked: boolean) => void
  onUrlChange: (value: string) => void
  onUrlBlur?: () => void
  onRequireUrl?: () => void
}

function ForwardProxyAnchoredProgressBubble({
  anchorEl,
  strings,
  progress,
}: {
  anchorEl: HTMLElement | null
  strings: AdminTranslations['proxySettings']
  progress: ForwardProxyDialogProgressState
}): JSX.Element | null {
  const { layerRef: bubbleRef, position } = useAnchoredFloatingLayer<HTMLDivElement>({
    open: Boolean(anchorEl),
    anchorEl,
    placement: 'bottom',
    align: 'center',
    offset: 10,
    viewportMargin: 12,
    arrowPadding: 18,
  })

  if (!anchorEl || typeof document === 'undefined') {
    return null
  }

  return createPortal(
    <div
      ref={bubbleRef}
      className="forward-proxy-progress-bubble-shell layer-popover"
      data-placement={position?.placement ?? 'bottom'}
      style={{
        top: `${position?.top ?? 0}px`,
        left: `${position?.left ?? 0}px`,
        visibility: position ? 'visible' : 'hidden',
        pointerEvents: 'none',
        ['--forward-proxy-progress-bubble-arrow-left' as string]: `${position?.arrowOffset ?? 40}px`,
      }}
    >
      <ForwardProxyProgressBubble
        strings={strings}
        progress={progress}
        className="forward-proxy-progress-bubble-surface"
      />
    </div>,
    document.body,
  )
}

export default function ForwardProxyEgressControl({
  strings,
  enabled,
  url,
  loading,
  controlsDisabled,
  inputLocked,
  errorMessage = null,
  errorPresentation = 'hint',
  progress,
  onToggle,
  onUrlChange,
  onUrlBlur,
  onRequireUrl,
}: ForwardProxyEgressControlProps): JSX.Element {
  const switchAnchorRef = useRef<HTMLDivElement | null>(null)
  const switchRef = useRef<HTMLButtonElement | null>(null)
  const inputRef = useRef<HTMLInputElement | null>(null)
  const [bubbleVisible, setBubbleVisible] = useState(false)
  const previousHasProgressRef = useRef(false)

  const hasProgress = progress != null

  useEffect(() => {
    if (hasProgress && !previousHasProgressRef.current) {
      setBubbleVisible(true)
    } else if (!hasProgress && previousHasProgressRef.current) {
      setBubbleVisible(false)
    }
    previousHasProgressRef.current = hasProgress
  }, [hasProgress])

  useEffect(() => {
    if (!errorMessage || errorPresentation !== 'alert' || inputLocked) return
    inputRef.current?.focus()
  }, [errorMessage, errorPresentation, inputLocked])

  useEffect(() => {
    if (!hasProgress || !bubbleVisible) return

    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target as Node | null
      if (!target) return
      if (switchAnchorRef.current?.contains(target)) return
      setBubbleVisible(false)
    }

    document.addEventListener('pointerdown', handlePointerDown, true)
    return () => {
      document.removeEventListener('pointerdown', handlePointerDown, true)
    }
  }, [hasProgress, bubbleVisible])

  const handleCheckedChange = (checked: boolean) => {
    if (checked && url.trim().length === 0) {
      inputRef.current?.focus()
      onRequireUrl?.()
      return
    }
    onToggle(checked)
  }

  return (
    <section className="space-y-3" aria-label={strings.config.egressTitle}>
      <div className="flex items-start justify-between gap-4">
        <h3 className="text-base font-semibold tracking-tight">{strings.config.egressTitle}</h3>
        <div
          ref={switchAnchorRef}
          className="ml-auto shrink-0"
          onMouseEnter={() => {
            if (hasProgress) setBubbleVisible(true)
          }}
          onFocus={() => {
            if (hasProgress) setBubbleVisible(true)
          }}
        >
          <Switch
            ref={switchRef}
            aria-label={strings.config.egressSwitchLabel}
            checked={enabled}
            onCheckedChange={handleCheckedChange}
            loading={loading}
            disabled={controlsDisabled}
          />
        </div>
      </div>
      {progress && bubbleVisible && (
        <ForwardProxyAnchoredProgressBubble
          anchorEl={switchAnchorRef.current ?? switchRef.current}
          strings={strings}
          progress={progress}
        />
      )}
      <Input
        ref={inputRef}
        name="egress-socks5-url"
        aria-label={strings.config.egressUrlLabel}
        value={url}
        onChange={(event) => onUrlChange(event.target.value)}
        onBlur={() => onUrlBlur?.()}
        placeholder={strings.config.egressUrlPlaceholder}
        disabled={controlsDisabled || inputLocked}
        readOnly={inputLocked}
        aria-invalid={errorMessage ? true : undefined}
        className={errorMessage ? 'border-destructive focus-visible:ring-destructive' : undefined}
      />
      <div
        className={`flex min-h-10 items-start gap-2 px-1 text-sm leading-5 ${
          errorMessage ? 'text-destructive' : 'text-muted-foreground'
        }`}
        role={errorMessage && errorPresentation === 'alert' ? 'alert' : undefined}
        aria-live={errorMessage && errorPresentation === 'alert' ? 'assertive' : undefined}
      >
        <Icon
          icon={
            errorMessage
              ? 'mdi:alert-circle-outline'
              : inputLocked
                ? 'mdi:lock-outline'
                : 'mdi:information-outline'
          }
          className="mt-0.5 shrink-0 text-base"
        />
        <p className={errorMessage ? 'line-clamp-2' : 'panel-description'} title={errorMessage ?? undefined}>
          {errorMessage
            ? errorPresentation === 'alert'
              ? `${strings.config.egressErrorTitle}：${errorMessage}`
              : errorMessage
            : inputLocked
              ? strings.config.egressLockedHint
              : strings.config.egressUrlHint}
        </p>
      </div>
      <div className="sr-only" aria-live="polite">
        {loading ? strings.config.egressApplying : progress?.message ?? ''}
      </div>
    </section>
  )
}
