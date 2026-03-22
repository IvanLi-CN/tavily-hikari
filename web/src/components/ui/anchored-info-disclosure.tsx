import {
  type ButtonHTMLAttributes,
  type FocusEvent,
  type MouseEvent,
  type ReactNode,
  useEffect,
  useId,
  useRef,
  useState,
} from 'react'
import { createPortal } from 'react-dom'

import { useAnchoredFloatingLayer } from '../../lib/useAnchoredFloatingLayer'
import { cn } from '../../lib/utils'

export interface AnchoredInfoDisclosureProps
  extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children'> {
  bubbleContent: ReactNode
  children: ReactNode
  bubbleClassName?: string
}

export function AnchoredInfoDisclosure({
  bubbleContent,
  children,
  className,
  bubbleClassName,
  onBlur,
  onClick,
  onFocus,
  type = 'button',
  ...buttonProps
}: AnchoredInfoDisclosureProps): JSX.Element {
  const triggerRef = useRef<HTMLButtonElement | null>(null)
  const bubbleId = useId()
  const [open, setOpen] = useState(false)
  const { layerRef: bubbleRef, position } = useAnchoredFloatingLayer<HTMLDivElement>({
    open,
    anchorEl: triggerRef.current,
    placement: 'top',
    align: 'center',
    offset: 10,
    viewportMargin: 12,
    arrowPadding: 18,
  })

  useEffect(() => {
    if (!open) return

    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target as Node | null
      if (!target) return
      if (triggerRef.current?.contains(target)) return
      if (bubbleRef.current?.contains(target)) return
      setOpen(false)
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      setOpen(false)
      triggerRef.current?.focus()
    }

    document.addEventListener('pointerdown', handlePointerDown)
    document.addEventListener('keydown', handleKeyDown)
    return () => {
      document.removeEventListener('pointerdown', handlePointerDown)
      document.removeEventListener('keydown', handleKeyDown)
    }
  }, [bubbleRef, open])

  const handleClick = (event: MouseEvent<HTMLButtonElement>) => {
    onClick?.(event)
    if (event.defaultPrevented) return
    if (event.detail === 0) {
      setOpen(true)
      return
    }
    setOpen((currentOpen) => !currentOpen)
  }

  const handleFocus = (event: FocusEvent<HTMLButtonElement>) => {
    onFocus?.(event)
    if (event.defaultPrevented) return
    if (event.currentTarget.matches(':focus-visible')) {
      setOpen(true)
    }
  }

  const handleBlur = (event: FocusEvent<HTMLButtonElement>) => {
    onBlur?.(event)
    if (event.defaultPrevented) return
    const nextFocusedNode = event.relatedTarget as Node | null
    if (nextFocusedNode && (triggerRef.current?.contains(nextFocusedNode) || bubbleRef.current?.contains(nextFocusedNode))) {
      return
    }
    setOpen(false)
  }

  return (
    <>
      <button
        {...buttonProps}
        ref={triggerRef}
        type={type}
        className={className}
        aria-describedby={open ? bubbleId : undefined}
        aria-expanded={open}
        aria-haspopup="dialog"
        onClick={handleClick}
        onFocus={handleFocus}
        onBlur={handleBlur}
      >
        {children}
      </button>
      {open && typeof document !== 'undefined'
        ? createPortal(
            <div
              ref={bubbleRef}
              id={bubbleId}
              className={cn('anchored-info-disclosure-bubble layer-popover', bubbleClassName)}
              role="tooltip"
              data-placement={position?.placement ?? 'top'}
              style={{
                top: `${position?.top ?? 0}px`,
                left: `${position?.left ?? 0}px`,
                visibility: position ? 'visible' : 'hidden',
                pointerEvents: position ? 'auto' : 'none',
                ['--anchored-info-disclosure-arrow-offset' as string]: `${position?.arrowOffset ?? 24}px`,
              }}
            >
              {bubbleContent}
            </div>,
            document.body,
          )
        : null}
    </>
  )
}
