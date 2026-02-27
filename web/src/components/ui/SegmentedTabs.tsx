import * as React from 'react'

import { cn } from '../../lib/utils'

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
}

export default function SegmentedTabs<T extends string = string>({
  value,
  onChange,
  options,
  ariaLabel,
  className,
}: SegmentedTabsProps<T>): JSX.Element {
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
            disabled={option.disabled}
          >
            {option.label}
          </button>
        )
      })}
    </div>
  )
}
