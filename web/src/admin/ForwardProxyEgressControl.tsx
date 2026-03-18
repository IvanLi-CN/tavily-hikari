import { Icon } from '@iconify/react'
import type { RefObject } from 'react'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'

import type { AdminTranslations } from '../i18n'
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
  bubbleRef,
}: {
  anchorEl: HTMLElement | null
  strings: AdminTranslations['proxySettings']
  progress: ForwardProxyDialogProgressState
  bubbleRef?: RefObject<HTMLDivElement>
}): JSX.Element | null {
  const localBubbleRef = useRef<HTMLDivElement | null>(null)
  const [position, setPosition] = useState<{
    top: number
    left: number
    placement: 'top' | 'bottom'
    arrowLeft: number
  } | null>(null)

  useLayoutEffect(() => {
    if (!anchorEl || typeof window === 'undefined') {
      setPosition(null)
      return
    }

    const viewportMargin = 12
    const anchorGap = 10
    const arrowMargin = 18

    const updatePosition = () => {
      const bubble = (bubbleRef?.current ?? localBubbleRef.current)
      if (!bubble || !anchorEl.isConnected) {
        setPosition(null)
        return
      }

      const anchorRect = anchorEl.getBoundingClientRect()
      const bubbleRect = bubble.getBoundingClientRect()

      let top = anchorRect.bottom + anchorGap
      let placement: 'top' | 'bottom' = 'bottom'

      if (top + bubbleRect.height > window.innerHeight - viewportMargin) {
        const nextTop = anchorRect.top - bubbleRect.height - anchorGap
        if (nextTop >= viewportMargin) {
          top = nextTop
          placement = 'top'
        }
      }

      top = Math.max(viewportMargin, Math.min(top, window.innerHeight - bubbleRect.height - viewportMargin))

      let left = anchorRect.left + anchorRect.width / 2 - bubbleRect.width / 2
      left = Math.max(viewportMargin, Math.min(left, window.innerWidth - bubbleRect.width - viewportMargin))

      const arrowLeft = Math.max(
        arrowMargin,
        Math.min(anchorRect.left + anchorRect.width / 2 - left, bubbleRect.width - arrowMargin),
      )

      setPosition({ top, left, placement, arrowLeft })
    }

    updatePosition()

    const resizeObserver = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(updatePosition) : null
    resizeObserver?.observe(anchorEl)
    if ((bubbleRef?.current ?? localBubbleRef.current)) {
      resizeObserver?.observe((bubbleRef?.current ?? localBubbleRef.current)!)
    }

    window.addEventListener('resize', updatePosition)
    window.addEventListener('scroll', updatePosition, true)

    return () => {
      resizeObserver?.disconnect()
      window.removeEventListener('resize', updatePosition)
      window.removeEventListener('scroll', updatePosition, true)
    }
  }, [anchorEl, progress])

  if (!anchorEl || typeof document === 'undefined') {
    return null
  }

  return createPortal(
    <div
      ref={bubbleRef ?? localBubbleRef}
      className="forward-proxy-progress-bubble-shell"
      data-placement={position?.placement ?? 'bottom'}
      style={{
        top: `${position?.top ?? 0}px`,
        left: `${position?.left ?? 0}px`,
        visibility: position ? 'visible' : 'hidden',
        pointerEvents: 'none',
        ['--forward-proxy-progress-bubble-arrow-left' as string]: `${position?.arrowLeft ?? 40}px`,
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
  const bubbleShellRef = useRef<HTMLDivElement>(null)
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
      if (bubbleShellRef.current?.contains(target)) return
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
          bubbleRef={bubbleShellRef}
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
