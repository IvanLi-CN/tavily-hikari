import type { ReactNode } from 'react'

import { Button } from './ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './ui/select'

interface AdminTablePaginationProps {
  page: number
  totalPages: number
  pageSummary?: ReactNode
  perPage?: number
  perPageLabel?: ReactNode
  perPageOptions?: number[]
  perPageAriaLabel?: string
  previousLabel?: string
  nextLabel?: string
  previousDisabled?: boolean
  nextDisabled?: boolean
  disabled?: boolean
  onPrevious: () => void | Promise<void>
  onNext: () => void | Promise<void>
  onPerPageChange?: (value: number) => void | Promise<void>
}

export default function AdminTablePagination({
  page,
  totalPages,
  pageSummary,
  perPage,
  perPageLabel = 'Per page',
  perPageOptions = [10, 20, 50, 100],
  perPageAriaLabel = 'Rows per page',
  previousLabel = 'Previous',
  nextLabel = 'Next',
  previousDisabled = false,
  nextDisabled = false,
  disabled = false,
  onPrevious,
  onNext,
  onPerPageChange,
}: AdminTablePaginationProps): JSX.Element {
  const resolvedPerPageOptions =
    typeof perPage === 'number' && !perPageOptions.includes(perPage)
      ? [...perPageOptions, perPage].sort((left, right) => left - right)
      : perPageOptions

  return (
    <div className="table-pagination">
      <span>{perPageLabel}</span>
      {typeof perPage === 'number' && onPerPageChange ? (
        <Select value={String(perPage)} onValueChange={(value) => onPerPageChange(Number(value))} disabled={disabled}>
          <SelectTrigger aria-label={perPageAriaLabel} className="table-pagination-select w-[96px]" disabled={disabled}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent align="start">
            {resolvedPerPageOptions.map((option) => (
              <SelectItem key={option} value={String(option)}>
                {option}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      ) : null}
      <Button
        type="button"
        variant="outline"
        size="sm"
        className="table-pagination-button"
        onClick={() => void onPrevious()}
        disabled={disabled || previousDisabled}
      >
        {previousLabel}
      </Button>
      <span>{pageSummary ?? `Page ${page} / ${totalPages}`}</span>
      <Button
        type="button"
        variant="outline"
        size="sm"
        className="table-pagination-button"
        onClick={() => void onNext()}
        disabled={disabled || nextDisabled}
      >
        {nextLabel}
      </Button>
    </div>
  )
}
