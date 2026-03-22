import { type FocusEvent, type MouseEvent, useEffect, useRef } from 'react'
import { createPortal } from 'react-dom'
import { X } from 'lucide-react'

import { selectAllReadonlyText } from '../lib/clipboard'
import { useAnchoredFloatingLayer } from '../lib/useAnchoredFloatingLayer'
import { cn } from '../lib/utils'
import { Button } from './ui/button'
import { Input } from './ui/input'
import { Textarea } from './ui/textarea'

export interface ManualCopyBubbleProps {
  open: boolean
  anchorEl: HTMLElement | null
  title: string
  description: string
  fieldLabel: string
  value: string
  closeLabel: string
  multiline?: boolean
  className?: string
  onClose: () => void
}

const VIEWPORT_MARGIN = 12
const ANCHOR_GAP = 10
const ARROW_MARGIN = 18

export default function ManualCopyBubble({
  open,
  anchorEl,
  title,
  description,
  fieldLabel,
  value,
  closeLabel,
  multiline = false,
  className,
  onClose,
}: ManualCopyBubbleProps): JSX.Element | null {
  const fieldRef = useRef<HTMLInputElement | HTMLTextAreaElement | null>(null)
  const { layerRef: bubbleRef, position } = useAnchoredFloatingLayer<HTMLDivElement>({
    open,
    anchorEl,
    placement: 'bottom',
    align: 'center',
    offset: ANCHOR_GAP,
    viewportMargin: VIEWPORT_MARGIN,
    arrowPadding: ARROW_MARGIN,
  })

  useEffect(() => {
    if (!open) return

    const handlePointerDown = (event: PointerEvent) => {
      const bubble = bubbleRef.current
      const target = event.target as Node | null
      if (!target) return
      if (bubble?.contains(target)) return
      if (anchorEl?.contains(target)) return
      onClose()
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        onClose()
      }
    }

    document.addEventListener('pointerdown', handlePointerDown)
    document.addEventListener('keydown', handleKeyDown)
    return () => {
      document.removeEventListener('pointerdown', handlePointerDown)
      document.removeEventListener('keydown', handleKeyDown)
    }
  }, [anchorEl, onClose, open])

  useEffect(() => {
    if (!open) return
    const frame = window.requestAnimationFrame(() => {
      selectAllReadonlyText(fieldRef.current)
    })
    return () => window.cancelAnimationFrame(frame)
  }, [open, value])

  if (!open || !anchorEl || typeof document === 'undefined') {
    return null
  }

  const fieldProps = {
    readOnly: true,
    spellCheck: false,
    value,
    onClick: (event: MouseEvent<HTMLInputElement | HTMLTextAreaElement>) => {
      selectAllReadonlyText(event.currentTarget)
    },
    onFocus: (event: FocusEvent<HTMLInputElement | HTMLTextAreaElement>) => {
      selectAllReadonlyText(event.currentTarget)
    },
    className: 'manual-copy-bubble-field',
  }

  return createPortal(
    <div
      ref={bubbleRef}
      className={cn('manual-copy-bubble layer-popover', className)}
      role="dialog"
      aria-modal="false"
      style={{
        top: `${position?.top ?? 0}px`,
        left: `${position?.left ?? 0}px`,
        visibility: position ? 'visible' : 'hidden',
        pointerEvents: position ? 'auto' : 'none',
        ['--manual-copy-arrow-left' as string]: `${position?.arrowOffset ?? 40}px`,
      }}
      data-placement={position?.placement ?? 'bottom'}
    >
      <div className="manual-copy-bubble-header">
        <div className="manual-copy-bubble-copy">
          <strong className="manual-copy-bubble-title">{title}</strong>
          <p className="manual-copy-bubble-description">{description}</p>
        </div>
        <button type="button" className="manual-copy-bubble-close" onClick={onClose} aria-label={closeLabel}>
          <X className="h-4 w-4" />
        </button>
      </div>
      <label className="manual-copy-bubble-label">{fieldLabel}</label>
      {multiline ? (
        <Textarea
          {...fieldProps}
          ref={(node) => {
            fieldRef.current = node
          }}
          rows={4}
        />
      ) : (
        <Input
          {...fieldProps}
          ref={(node) => {
            fieldRef.current = node
          }}
          type="text"
        />
      )}
      <div className="manual-copy-bubble-actions">
        <Button type="button" variant="outline" size="sm" onClick={onClose}>
          {closeLabel}
        </Button>
      </div>
    </div>,
    document.body,
  )
}
