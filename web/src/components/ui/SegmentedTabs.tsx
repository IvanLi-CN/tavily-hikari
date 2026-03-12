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
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={active}
            className={cn('segmented-tab', active && 'is-active')}
            onClick={() => onChange(option.value)}
            disabled={disabled || option.disabled}
          >
            {option.label}
          </button>
        )
      })}
    </div>
  )
}
