import { type PropsWithChildren, type ReactNode } from 'react'

interface AdminSidebarUtilityCardProps extends PropsWithChildren {
  title?: ReactNode
  description?: ReactNode
  className?: string
}

interface AdminSidebarUtilityStackProps extends PropsWithChildren {
  className?: string
}

export function AdminSidebarUtilityStack({
  children,
  className,
}: AdminSidebarUtilityStackProps): JSX.Element {
  const classes = ['admin-sidebar-utility-stack', className].filter(Boolean).join(' ')

  return <div className={classes}>{children}</div>
}

export function AdminSidebarUtilityCard({
  title,
  description,
  className,
  children,
}: AdminSidebarUtilityCardProps): JSX.Element {
  const classes = ['admin-sidebar-utility-card', className].filter(Boolean).join(' ')

  return (
    <section className={classes}>
      {(title || description) ? (
        <header className="admin-sidebar-utility-card-header">
          {title ? <h2 className="admin-sidebar-utility-card-title">{title}</h2> : null}
          {description ? <p className="admin-sidebar-utility-card-description">{description}</p> : null}
        </header>
      ) : null}
      {children}
    </section>
  )
}
