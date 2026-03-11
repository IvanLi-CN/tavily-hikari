import type { ReactNode } from 'react'

import { cn } from '../lib/utils'
import { Table } from './ui/table'

interface AdminTableShellProps {
  children: ReactNode
  className?: string
  tableClassName?: string
}

export default function AdminTableShell({ children, className, tableClassName }: AdminTableShellProps): JSX.Element {
  return (
    <div className={cn('table-wrapper', className)}>
      <Table className={tableClassName}>{children}</Table>
    </div>
  )
}
