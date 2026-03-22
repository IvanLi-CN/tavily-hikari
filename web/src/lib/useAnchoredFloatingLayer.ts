import { useLayoutEffect, useRef, useState } from 'react'

export type AnchoredFloatingPlacement = 'top' | 'bottom' | 'left' | 'right'
export type AnchoredFloatingAlign = 'start' | 'center' | 'end'

export interface AnchoredFloatingPosition {
  top: number
  left: number
  placement: AnchoredFloatingPlacement
  arrowOffset: number
}

interface UseAnchoredFloatingLayerOptions {
  open: boolean
  anchorEl: HTMLElement | null
  placement?: AnchoredFloatingPlacement
  align?: AnchoredFloatingAlign
  offset?: number
  viewportMargin?: number
  arrowPadding?: number
}

interface EvaluatedPlacement extends AnchoredFloatingPosition {
  overflowScore: number
}

function clamp(value: number, min: number, max: number): number {
  if (Number.isNaN(value)) return min
  return Math.min(Math.max(value, min), max)
}

function oppositePlacement(placement: AnchoredFloatingPlacement): AnchoredFloatingPlacement {
  if (placement === 'top') return 'bottom'
  if (placement === 'bottom') return 'top'
  if (placement === 'left') return 'right'
  return 'left'
}

function computeAxisPosition(
  anchorStart: number,
  anchorSize: number,
  layerSize: number,
  align: AnchoredFloatingAlign,
): number {
  if (align === 'start') return anchorStart
  if (align === 'end') return anchorStart + anchorSize - layerSize
  return anchorStart + anchorSize / 2 - layerSize / 2
}

function evaluatePlacement(
  anchorRect: DOMRect,
  layerRect: DOMRect,
  placement: AnchoredFloatingPlacement,
  align: AnchoredFloatingAlign,
  offset: number,
  viewportMargin: number,
  arrowPadding: number,
): EvaluatedPlacement {
  const maxTop = Math.max(viewportMargin, window.innerHeight - layerRect.height - viewportMargin)
  const maxLeft = Math.max(viewportMargin, window.innerWidth - layerRect.width - viewportMargin)

  let top = 0
  let left = 0

  if (placement === 'top') {
    top = anchorRect.top - layerRect.height - offset
    left = computeAxisPosition(anchorRect.left, anchorRect.width, layerRect.width, align)
  } else if (placement === 'bottom') {
    top = anchorRect.bottom + offset
    left = computeAxisPosition(anchorRect.left, anchorRect.width, layerRect.width, align)
  } else if (placement === 'left') {
    left = anchorRect.left - layerRect.width - offset
    top = computeAxisPosition(anchorRect.top, anchorRect.height, layerRect.height, align)
  } else {
    left = anchorRect.right + offset
    top = computeAxisPosition(anchorRect.top, anchorRect.height, layerRect.height, align)
  }

  const overflowTop = Math.max(0, viewportMargin - top)
  const overflowBottom = Math.max(0, top + layerRect.height - (window.innerHeight - viewportMargin))
  const overflowLeft = Math.max(0, viewportMargin - left)
  const overflowRight = Math.max(0, left + layerRect.width - (window.innerWidth - viewportMargin))
  const overflowScore =
    (placement === 'top' || placement === 'bottom'
      ? (overflowTop + overflowBottom) * 10 + overflowLeft + overflowRight
      : (overflowLeft + overflowRight) * 10 + overflowTop + overflowBottom)

  const clampedTop = clamp(top, viewportMargin, maxTop)
  const clampedLeft = clamp(left, viewportMargin, maxLeft)
  const arrowOffset =
    placement === 'top' || placement === 'bottom'
      ? clamp(anchorRect.left + anchorRect.width / 2 - clampedLeft, arrowPadding, layerRect.width - arrowPadding)
      : clamp(anchorRect.top + anchorRect.height / 2 - clampedTop, arrowPadding, layerRect.height - arrowPadding)

  return {
    top: clampedTop,
    left: clampedLeft,
    placement,
    arrowOffset,
    overflowScore,
  }
}

export function useAnchoredFloatingLayer<T extends HTMLElement>({
  open,
  anchorEl,
  placement = 'bottom',
  align = 'center',
  offset = 10,
  viewportMargin = 12,
  arrowPadding = 18,
}: UseAnchoredFloatingLayerOptions): {
  layerRef: React.RefObject<T>
  position: AnchoredFloatingPosition | null
} {
  const layerRef = useRef<T | null>(null)
  const [position, setPosition] = useState<AnchoredFloatingPosition | null>(null)

  useLayoutEffect(() => {
    if (!open || !anchorEl || typeof window === 'undefined') {
      setPosition(null)
      return
    }

    const updatePosition = () => {
      const layer = layerRef.current
      if (!layer || !anchorEl.isConnected) {
        setPosition(null)
        return
      }

      const anchorRect = anchorEl.getBoundingClientRect()
      const layerRect = layer.getBoundingClientRect()
      const candidates = [
        evaluatePlacement(anchorRect, layerRect, placement, align, offset, viewportMargin, arrowPadding),
        evaluatePlacement(
          anchorRect,
          layerRect,
          oppositePlacement(placement),
          align,
          offset,
          viewportMargin,
          arrowPadding,
        ),
      ]

      candidates.sort((leftCandidate, rightCandidate) => leftCandidate.overflowScore - rightCandidate.overflowScore)
      const next = candidates[0]
      setPosition({
        top: next.top,
        left: next.left,
        placement: next.placement,
        arrowOffset: next.arrowOffset,
      })
    }

    updatePosition()

    const resizeObserver = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(updatePosition) : null
    resizeObserver?.observe(anchorEl)
    if (layerRef.current) {
      resizeObserver?.observe(layerRef.current)
    }

    window.addEventListener('resize', updatePosition)
    window.addEventListener('scroll', updatePosition, true)

    return () => {
      resizeObserver?.disconnect()
      window.removeEventListener('resize', updatePosition)
      window.removeEventListener('scroll', updatePosition, true)
    }
  }, [align, anchorEl, arrowPadding, offset, open, placement, viewportMargin])

  return { layerRef: layerRef as React.RefObject<T>, position }
}
