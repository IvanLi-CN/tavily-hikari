import type { ReactNode } from 'react'

import type { QueryLoadState } from '../admin/queryLoadState'
import { cn } from '../lib/utils'
import AdminLoadingRegion from './AdminLoadingRegion'
import { Table, type TableDensity } from './ui/table'

interface AdminTableShellProps {
  children: ReactNode
  className?: string
  density?: TableDensity
  tableContainerClassName?: string
  tableClassName?: string
  loadState?: QueryLoadState
  loadingLabel?: ReactNode
  errorLabel?: ReactNode
  minHeight?: number | string
  skeletonRows?: number
}

export default function AdminTableShell({
  children,
  className,
  density = 'comfortable',
  tableContainerClassName,
  tableClassName,
  loadState = 'ready',
  loadingLabel,
  errorLabel,
  minHeight,
  skeletonRows,
}: AdminTableShellProps): JSX.Element {
  return (
    <AdminLoadingRegion
      className={cn('table-wrapper', className)}
      loadState={loadState}
      loadingLabel={loadingLabel}
      errorLabel={errorLabel}
      minHeight={minHeight}
      skeletonRows={skeletonRows}
    >
      <Table className={tableClassName} containerClassName={tableContainerClassName} density={density}>
        {children}
      </Table>
    </AdminLoadingRegion>
  )
}
