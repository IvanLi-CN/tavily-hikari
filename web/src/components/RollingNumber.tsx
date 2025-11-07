import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'

export interface RollingNumberProps {
  value: number | null | undefined
  loading?: boolean
  className?: string
}

// A lightweight rolling number that animates all digit columns up or down
// based on the overall delta (increase => roll up; decrease => roll down).
// Separators like commas are rendered as static glyphs.
export default function RollingNumber({ value, loading, className }: RollingNumberProps): JSX.Element {
  const [prev, setPrev] = useState<number>(value ?? 0)
  const [digitHeight, setDigitHeight] = useState<number>(0)
  const probeRef = useRef<HTMLDivElement | null>(null)

  useLayoutEffect(() => {
    if (probeRef.current) {
      const h = probeRef.current.getBoundingClientRect().height
      if (h > 0) setDigitHeight(h)
    }
  }, [])

  useEffect(() => {
    if (typeof value === 'number' && !Number.isNaN(value)) {
      setPrev((p) => p)
    }
  }, [value])

  const formatted = useMemo(() => {
    if (loading) return '—'
    if (value == null) return '—'
    return new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 }).format(value)
  }, [value, loading])

  const direction: 'up' | 'down' | 'none' = useMemo(() => {
    if (value == null) return 'none'
    if (prev == null) return 'up'
    if (value > prev) return 'up'
    if (value < prev) return 'down'
    return 'none'
  }, [value, prev])

  useEffect(() => {
    if (typeof value === 'number' && !Number.isNaN(value)) {
      setPrev(value)
    }
  }, [value])

  const cells = useMemo(() => formatted.split(''), [formatted])

  return (
    <span className={`rolling-number${className ? ' ' + className : ''}`}>
      {/* probe cell to measure height (1em) */}
      <div className="rn-probe" ref={probeRef} aria-hidden>
        0
      </div>
      {cells.map((ch, idx) => {
        if (ch < '0' || ch > '9') {
          return (
            <span key={idx} className="rn-sep">
              {ch}
            </span>
          )
        }
        const target = Number(ch)
        // render strip 0..9 twice to allow wrap when direction changes across boundary
        const digits = Array.from({ length: 20 }, (_, i) => i % 10)
        // compute translate target index depending on direction
        const baseIndex = direction === 'up' ? target + 10 : target
        const translate = digitHeight * baseIndex
        return (
          <span key={idx} className={`rn-col rn-${direction}`} style={{ height: digitHeight || undefined }}>
            <span
              className="rn-strip"
              style={{ transform: `translateY(${-translate}px)` }}
              aria-hidden
            >
              {digits.map((d, i) => (
                <span key={i} className="rn-digit">
                  {d}
                </span>
              ))}
            </span>
            {/* accessibility: render current digit for screen readers */}
            <span className="sr-only">{target}</span>
          </span>
        )
      })}
    </span>
  )
}
