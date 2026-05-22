import { type ReactNode } from 'react'

interface AdminCompactIntroProps {
  title: ReactNode
  description?: ReactNode
  meta?: ReactNode
  actions?: ReactNode
  className?: string
}

export default function AdminCompactIntro({
  title,
  description,
  meta,
  actions,
  className,
}: AdminCompactIntroProps): JSX.Element {
  const classes = ['admin-compact-intro', actions ? 'admin-compact-intro--with-actions' : null, className]
    .filter(Boolean)
    .join(' ')

  return (
    <section className={classes}>
      <div className="admin-compact-intro-main">
        <h1>{title}</h1>
        {description ? <p className="admin-compact-intro-description">{description}</p> : null}
      </div>
      {actions ? <div className="admin-compact-intro-actions">{actions}</div> : null}
      {meta ? <div className="admin-compact-intro-meta">{meta}</div> : null}
    </section>
  )
}
