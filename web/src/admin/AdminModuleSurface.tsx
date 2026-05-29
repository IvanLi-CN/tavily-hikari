import type { ReactNode } from 'react'

import { cn } from '../lib/utils'

interface AdminModuleSurfaceProps {
  children: ReactNode
  className?: string
  toolbar?: ReactNode
  toolbarClassName?: string
}

export default function AdminModuleSurface({
  children,
  className,
  toolbar,
  toolbarClassName,
}: AdminModuleSurfaceProps): JSX.Element {
  return (
    <section className={cn('surface panel admin-module-surface', className)}>
      {toolbar ? (
        <div className={cn('admin-module-toolbar', toolbarClassName)}>
          {toolbar}
        </div>
      ) : null}
      {children}
    </section>
  )
}
