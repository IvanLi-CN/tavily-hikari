import { type ReactNode, useEffect, useMemo, useRef, useState } from 'react'

import { Icon } from '../lib/icons'
import { cn } from '../lib/utils'
import { Input } from './ui/input'
import { DropdownMenu, DropdownMenuContent, DropdownMenuSeparator, DropdownMenuTrigger } from './ui/dropdown-menu'

export interface SearchableFacetSelectOption {
  value: string
  label?: string
  count?: number
}

export interface SearchableFacetSelectProps {
  value: string | null
  options: SearchableFacetSelectOption[]
  summary: string
  allLabel: string
  emptyLabel: string
  searchPlaceholder: string
  searchAriaLabel: string
  triggerAriaLabel: string
  listAriaLabel: string
  onChange: (nextValue: string | null) => void
  disabled?: boolean
  align?: 'start' | 'center' | 'end'
  triggerClassName?: string
  contentClassName?: string
  labelVariant?: 'default' | 'mono'
  renderOptionLabel?: (option: SearchableFacetSelectOption) => ReactNode
}

export default function SearchableFacetSelect({
  value,
  options,
  summary,
  allLabel,
  emptyLabel,
  searchPlaceholder,
  searchAriaLabel,
  triggerAriaLabel,
  listAriaLabel,
  onChange,
  disabled = false,
  align = 'end',
  triggerClassName,
  contentClassName,
  labelVariant = 'default',
  renderOptionLabel,
}: SearchableFacetSelectProps): JSX.Element {
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const inputRef = useRef<HTMLInputElement | null>(null)

  useEffect(() => {
    if (!open) {
      setQuery('')
      return
    }
    window.setTimeout(() => {
      inputRef.current?.focus()
      inputRef.current?.select()
    }, 0)
  }, [open])

  const normalizedQuery = query.trim().toLowerCase()
  const filteredOptions = useMemo(() => {
    if (!normalizedQuery) return options
    return options.filter((option) => {
      const haystacks = [option.value, option.label ?? '']
      return haystacks.some((candidate) => candidate.toLowerCase().includes(normalizedQuery))
    })
  }, [normalizedQuery, options])

  const renderLabel = (option: SearchableFacetSelectOption): ReactNode => {
    if (renderOptionLabel) return renderOptionLabel(option)
    const label = option.label ?? option.value
    return (
      <span
        className={cn(
          'searchable-facet-select__label',
          labelVariant === 'mono' && 'searchable-facet-select__label--mono',
        )}
      >
        {label}
      </span>
    )
  }

  return (
    <DropdownMenu open={open} onOpenChange={setOpen}>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          className={cn('searchable-facet-select__trigger', triggerClassName)}
          aria-label={triggerAriaLabel}
          disabled={disabled}
        >
          <span className="searchable-facet-select__summary">{summary}</span>
          <Icon icon="mdi:chevron-down" width={16} height={16} aria-hidden="true" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align={align}
        className={cn('searchable-facet-select__content', contentClassName)}
      >
        <div className="searchable-facet-select__search-box">
          <Input
            ref={inputRef}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={searchPlaceholder}
            className="searchable-facet-select__input"
            aria-label={searchAriaLabel}
          />
        </div>
        <DropdownMenuSeparator />
        <div className="searchable-facet-select__list" role="listbox" aria-label={listAriaLabel}>
          <button
            type="button"
            className={cn('searchable-facet-select__option', !value && 'searchable-facet-select__option--active')}
            onClick={() => {
              onChange(null)
              setOpen(false)
            }}
          >
            <span className="searchable-facet-select__mark" aria-hidden="true">
              {!value ? <Icon icon="mdi:check" width={16} height={16} /> : null}
            </span>
            <span className="searchable-facet-select__option-body">
              <span className="searchable-facet-select__label">{allLabel}</span>
            </span>
          </button>
          {filteredOptions.length === 0 ? (
            <div className="searchable-facet-select__empty">{emptyLabel}</div>
          ) : (
            filteredOptions.map((option) => (
              <button
                key={option.value}
                type="button"
                className={cn(
                  'searchable-facet-select__option',
                  value === option.value && 'searchable-facet-select__option--active',
                )}
                onClick={() => {
                  onChange(option.value)
                  setOpen(false)
                }}
              >
                <span className="searchable-facet-select__mark" aria-hidden="true">
                  {value === option.value ? <Icon icon="mdi:check" width={16} height={16} /> : null}
                </span>
                <span className="searchable-facet-select__option-body">
                  {renderLabel(option)}
                  {typeof option.count === 'number' ? (
                    <span className="searchable-facet-select__count">{`x${option.count}`}</span>
                  ) : null}
                </span>
              </button>
            ))
          )}
        </div>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
