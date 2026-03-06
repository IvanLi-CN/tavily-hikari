import { type RefObject, useEffect, useMemo, useState } from 'react'

export const VIEWPORT_SMALL_MAX = 767
export const CONTENT_COMPACT_MAX = 920
export const ADMIN_SIDEBAR_STACK_MAX = 1100

export type ViewportMode = 'small' | 'normal'
export type ContentMode = 'compact' | 'normal'

function readViewportMode(): ViewportMode {
  if (typeof window === 'undefined') return 'normal'
  return window.matchMedia(`(max-width: ${VIEWPORT_SMALL_MAX}px)`).matches ? 'small' : 'normal'
}

export function useViewportMode(): ViewportMode {
  const [mode, setMode] = useState<ViewportMode>(() => readViewportMode())

  useEffect(() => {
    const media = window.matchMedia(`(max-width: ${VIEWPORT_SMALL_MAX}px)`)
    const apply = () => setMode(media.matches ? 'small' : 'normal')
    apply()
    media.addEventListener('change', apply)
    return () => media.removeEventListener('change', apply)
  }, [])

  return mode
}

function readContentMode<T extends HTMLElement>(ref: RefObject<T>, maxWidth: number): ContentMode {
  const width = ref.current?.getBoundingClientRect().width
  if (typeof width !== 'number' || Number.isNaN(width)) {
    return 'normal'
  }
  return width <= maxWidth ? 'compact' : 'normal'
}

export function useContentMode<T extends HTMLElement>(
  ref: RefObject<T>,
  maxWidth: number = CONTENT_COMPACT_MAX,
): ContentMode {
  const [mode, setMode] = useState<ContentMode>(() => readContentMode(ref, maxWidth))

  useEffect(() => {
    const node = ref.current
    if (!node) {
      setMode('normal')
      return
    }

    const update = () => setMode(readContentMode(ref, maxWidth))
    update()

    const resizeObserver = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(update) : null
    resizeObserver?.observe(node)
    window.addEventListener('resize', update)

    return () => {
      resizeObserver?.disconnect()
      window.removeEventListener('resize', update)
    }
  }, [maxWidth, ref])

  return mode
}

export function useResponsiveModes<T extends HTMLElement>(ref: RefObject<T>): {
  viewportMode: ViewportMode
  contentMode: ContentMode
  isCompactLayout: boolean
} {
  const viewportMode = useViewportMode()
  const contentMode = useContentMode(ref)
  const isCompactLayout = useMemo(
    () => viewportMode === 'small' || contentMode === 'compact',
    [contentMode, viewportMode],
  )

  return { viewportMode, contentMode, isCompactLayout }
}
