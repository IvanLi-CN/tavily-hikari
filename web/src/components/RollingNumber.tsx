import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'

export interface RollingNumberProps {
  value: number | null | undefined
  loading?: boolean
  className?: string
}

export interface RollingNumberDigitCell {
  kind: 'digit'
  char: string
  animate: boolean
  direction: 'up' | 'down' | 'none'
  loops: number
  startIndex: number
  endIndex: number
  steps: number
}

export interface RollingNumberSeparatorCell {
  kind: 'separator'
  char: string
}

export type RollingNumberCell = RollingNumberDigitCell | RollingNumberSeparatorCell

const DIGITS = Array.from({ length: 50 }, (_, index) => index % 10)
const NUMBER_FORMATTER = new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 })
const DIGIT_GROUP_SIZE = 3
const STATIC_TARGET_INDEX = 20

const useIsomorphicLayoutEffect = typeof window === 'undefined' ? useEffect : useLayoutEffect

function formatValue(value: number | null | undefined, loading = false): string {
  if (loading) return '—'
  if (value == null || Number.isNaN(value)) return '—'
  return NUMBER_FORMATTER.format(value)
}

function padDigits(value: string, width: number): string {
  return value.padStart(width, '0')
}

function buildDigitCell(
  currentDigit: number,
  previousDigit: number,
  animate: boolean,
  direction: 'up' | 'down' | 'none',
): RollingNumberDigitCell {
  const startIndex = STATIC_TARGET_INDEX + previousDigit

  if (!animate || direction === 'none') {
    return {
      kind: 'digit',
      char: String(currentDigit),
      animate: false,
      direction: 'none',
      loops: 0,
      startIndex: STATIC_TARGET_INDEX + currentDigit,
      endIndex: STATIC_TARGET_INDEX + currentDigit,
      steps: 0,
    }
  }

  const rawSteps = direction === 'down'
    ? (currentDigit - previousDigit + 10) % 10
    : (previousDigit - currentDigit + 10) % 10
  const steps = rawSteps === 0 ? 10 : rawSteps

  return {
    kind: 'digit',
    char: String(currentDigit),
    animate: true,
    direction,
    loops: Math.ceil(steps / 10),
    startIndex,
    endIndex: direction === 'down' ? startIndex + steps : startIndex - steps,
    steps,
  }
}

export function buildRollingCells(
  previousValue: number | null | undefined,
  nextValue: number | null | undefined,
  options?: { loading?: boolean; reducedMotion?: boolean },
): RollingNumberCell[] {
  const formattedNext = formatValue(nextValue, options?.loading)
  if (formattedNext === '—') {
    return [{ kind: 'separator', char: '—' }]
  }

  const formattedPrevious = formatValue(previousValue)
  const reducedMotion = options?.reducedMotion ?? false
  const nextDigits = formattedNext.replace(/\D/g, '')
  const previousDigits = formattedPrevious === '—' ? '' : formattedPrevious.replace(/\D/g, '')
  const width = Math.max(nextDigits.length, previousDigits.length)

  if (width === 0) {
    return formattedNext.split('').map((char) => ({ kind: 'separator', char }))
  }

  const direction: 'up' | 'down' | 'none' = nextValue == null || previousValue == null
    ? 'none'
    : nextValue > previousValue
      ? 'down'
      : nextValue < previousValue
        ? 'up'
        : 'none'

  const previousPadded = padDigits(previousDigits, width)
  const nextPadded = padDigits(nextDigits, width)
  const animatedMask = new Array<boolean>(width).fill(false)
  const nextDigitOffset = width - nextDigits.length

  if (!reducedMotion && direction !== 'none') {
    const suffixStart = Math.max(0, width - DIGIT_GROUP_SIZE)
    let highestChangedIndex = -1

    for (let index = width - 1; index >= suffixStart; index -= 1) {
      if (previousPadded[index] !== nextPadded[index]) {
        highestChangedIndex = index
      }
    }

    if (highestChangedIndex !== -1) {
      for (let index = highestChangedIndex; index < width; index += 1) {
        animatedMask[index] = true
      }
    }
  }

  const cells: RollingNumberCell[] = []
  let digitIndex = 0

  for (const char of formattedNext) {
    if (char < '0' || char > '9') {
      cells.push({ kind: 'separator', char })
      continue
    }

    const paddedIndex = nextDigitOffset + digitIndex
    const currentDigit = Number(char)
    const previousDigit = Number(previousPadded[paddedIndex] ?? char)
    const animate = animatedMask[paddedIndex]
    cells.push(buildDigitCell(currentDigit, previousDigit, animate, direction))
    digitIndex += 1
  }

  return cells
}

function usePrefersReducedMotion(): boolean {
  const [prefersReducedMotion, setPrefersReducedMotion] = useState(() => (
    typeof window !== 'undefined' && typeof window.matchMedia === 'function'
      ? window.matchMedia('(prefers-reduced-motion: reduce)').matches
      : false
  ))

  useEffect(() => {
    if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return

    const mediaQuery = window.matchMedia('(prefers-reduced-motion: reduce)')
    const apply = () => setPrefersReducedMotion(mediaQuery.matches)
    apply()

    if (typeof mediaQuery.addEventListener === 'function') {
      mediaQuery.addEventListener('change', apply)
      return () => mediaQuery.removeEventListener('change', apply)
    }

    mediaQuery.addListener(apply)
    return () => mediaQuery.removeListener(apply)
  }, [])

  return prefersReducedMotion
}

function RollingDigitColumn({
  cell,
  digitHeight,
  columnIndex,
}: {
  cell: RollingNumberDigitCell
  digitHeight: number
  columnIndex: number
}): JSX.Element {
  const [isActive, setIsActive] = useState(!cell.animate)

  useEffect(() => {
    if (!cell.animate) {
      setIsActive(true)
      return
    }

    setIsActive(false)
    const scheduleFrame = typeof window.requestAnimationFrame === 'function'
      ? window.requestAnimationFrame.bind(window)
      : (callback: FrameRequestCallback) => window.setTimeout(() => callback(performance.now()), 16)
    const cancelFrame = typeof window.cancelAnimationFrame === 'function'
      ? window.cancelAnimationFrame.bind(window)
      : window.clearTimeout.bind(window)
    const frameId = scheduleFrame(() => setIsActive(true))
    return () => cancelFrame(frameId)
  }, [cell.animate, cell.endIndex, cell.startIndex])

  const translate = digitHeight * (isActive ? cell.endIndex : cell.startIndex)

  return (
    <span
      className={`rn-col${cell.animate ? ` rn-${cell.direction}` : ''}`}
      style={{ height: digitHeight || undefined }}
      data-rn-digit={cell.char}
      data-rn-animate={cell.animate ? 'true' : 'false'}
      data-rn-direction={cell.direction}
      data-rn-steps={cell.steps}
      data-rn-start-index={cell.startIndex}
      data-rn-end-index={cell.endIndex}
    >
      <span
        className="rn-strip"
        style={{ transform: `translateY(${-translate}px)` }}
      >
        {DIGITS.map((digit, stripIndex) => (
          <span key={`${columnIndex}-${stripIndex}`} className="rn-digit">
            {digit}
          </span>
        ))}
      </span>
    </span>
  )
}

export default function RollingNumber({ value, loading, className }: RollingNumberProps): JSX.Element {
  const [digitHeight, setDigitHeight] = useState<number>(0)
  const probeRef = useRef<HTMLSpanElement | null>(null)
  const previousValueRef = useRef<number | null | undefined>(value ?? null)
  const prefersReducedMotion = usePrefersReducedMotion()

  useIsomorphicLayoutEffect(() => {
    const probe = probeRef.current
    if (!probe) return

    const measure = () => {
      const height = probe.getBoundingClientRect().height
      if (height > 0) setDigitHeight(height)
    }

    measure()

    const resizeObserver = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(measure) : null
    resizeObserver?.observe(probe)
    window.addEventListener('resize', measure)

    return () => {
      resizeObserver?.disconnect()
      window.removeEventListener('resize', measure)
    }
  }, [])

  const formatted = useMemo(() => formatValue(value, loading), [value, loading])
  const cells = useMemo(
    () => buildRollingCells(previousValueRef.current, value, { loading, reducedMotion: prefersReducedMotion }),
    [loading, prefersReducedMotion, value],
  )

  useEffect(() => {
    if (typeof value === 'number' && Number.isFinite(value)) {
      previousValueRef.current = value
    }
  }, [value])

  return (
    <span
      className={`rolling-number${className ? ' ' + className : ''}`}
      aria-label={formatted}
      role="text"
    >
      <span className="rn-probe" ref={probeRef} aria-hidden="true">
        0
      </span>
      <span aria-hidden="true" className="rn-visual">
        {cells.map((cell, index) => {
          if (cell.kind === 'separator') {
            return (
              <span key={`sep-${index}`} className="rn-sep">
                {cell.char}
              </span>
            )
          }

          return (
            <RollingDigitColumn
              key={`digit-${index}-${cell.char}-${cell.animate ? `${cell.startIndex}-${cell.endIndex}` : 'static'}`}
              cell={cell}
              digitHeight={digitHeight}
              columnIndex={index}
            />
          )
        })}
      </span>
    </span>
  )
}
