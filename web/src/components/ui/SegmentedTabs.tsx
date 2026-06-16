import * as React from 'react'

import { useViewportMode } from '../../lib/responsive'
import { cn } from '../../lib/utils'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './select'

export interface SegmentedTabsOption<T extends string = string> {
  value: T
  label: React.ReactNode
  disabled?: boolean
}

interface SegmentedTabsProps<T extends string = string> {
  value: T
  onChange: (value: T) => void
  options: ReadonlyArray<SegmentedTabsOption<T>>
  ariaLabel: string
  className?: string
  disabled?: boolean
}

function labelToPlainText(node: React.ReactNode): string {
  if (typeof node === 'string' || typeof node === 'number') return String(node)
  if (Array.isArray(node)) return node.map((item) => labelToPlainText(item)).join('').trim()
  if (React.isValidElement<{ children?: React.ReactNode }>(node)) {
    return labelToPlainText(node.props.children).trim()
  }
  return ''
}

export default function SegmentedTabs<T extends string = string>({
  value,
  onChange,
  options,
  ariaLabel,
  className,
  disabled = false,
}: SegmentedTabsProps<T>): JSX.Element {
  const viewportMode = useViewportMode()
  const buttonRefs = React.useRef<Array<HTMLButtonElement | null>>([])

  function findNextEnabledIndex(startIndex: number, step: 1 | -1): number {
    if (options.length === 0) return -1
    for (let offset = 1; offset <= options.length; offset += 1) {
      const nextIndex = (startIndex + offset * step + options.length) % options.length
      if (!options[nextIndex]?.disabled) return nextIndex
    }
    return startIndex
  }

  function findBoundaryEnabledIndex(fromEnd: boolean): number {
    const ordered = fromEnd ? [...options].reverse() : options
    const match = ordered.find((option) => !option.disabled)
    if (!match) return -1
    return options.findIndex((option) => option.value === match.value)
  }

  if (viewportMode === 'small') {
    const selectedOption = options.find((option) => option.value === value)
    const selectedLabel = selectedOption ? labelToPlainText(selectedOption.label) : ''

    return (
      <div className={cn('segmented-tabs segmented-tabs-mobile', className)}>
        <Select value={value} onValueChange={(next) => onChange(next as T)} disabled={disabled}>
          <SelectTrigger aria-label={ariaLabel} className="segmented-tabs-select-trigger" disabled={disabled}>
            <SelectValue>{selectedLabel || value}</SelectValue>
          </SelectTrigger>
          <SelectContent align="start" className="segmented-tabs-select-content">
            {options.map((option) => (
              <SelectItem key={option.value} value={option.value} disabled={disabled || option.disabled}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
    )
  }

  return (
    <div className={cn('segmented-tabs', className)} role="radiogroup" aria-label={ariaLabel}>
      {options.map((option) => {
        const active = option.value === value
        const activeIndex = options.findIndex((candidate) => candidate.value === value)
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={active}
            tabIndex={active ? 0 : -1}
            className={cn('segmented-tab', active && 'is-active')}
            ref={(node) => {
              buttonRefs.current[options.findIndex((candidate) => candidate.value === option.value)] = node
            }}
            onClick={() => onChange(option.value)}
            onKeyDown={(event) => {
              if (disabled || option.disabled) return

              let nextIndex = -1
              if (event.key === 'ArrowRight' || event.key === 'ArrowDown') {
                nextIndex = findNextEnabledIndex(activeIndex, 1)
              } else if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') {
                nextIndex = findNextEnabledIndex(activeIndex, -1)
              } else if (event.key === 'Home') {
                nextIndex = findBoundaryEnabledIndex(false)
              } else if (event.key === 'End') {
                nextIndex = findBoundaryEnabledIndex(true)
              }

              if (nextIndex === -1 || nextIndex === activeIndex) return

              event.preventDefault()
              onChange(options[nextIndex]!.value)
              buttonRefs.current[nextIndex]?.focus()
            }}
            disabled={disabled || option.disabled}
          >
            {option.label}
          </button>
        )
      })}
    </div>
  )
}
