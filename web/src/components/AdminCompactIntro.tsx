import { type ReactNode } from 'react'

interface AdminCompactIntroProps {
  title: ReactNode
  description?: ReactNode
  meta?: ReactNode
  className?: string
}

export default function AdminCompactIntro({
  title,
  description,
  meta,
  className,
}: AdminCompactIntroProps): JSX.Element {
  const classes = ['admin-compact-intro', className].filter(Boolean).join(' ')

  return (
    <section className={classes}>
      <div className="admin-compact-intro-main">
        <h1>{title}</h1>
        {description ? <p className="admin-compact-intro-description">{description}</p> : null}
      </div>
      {meta ? <div className="admin-compact-intro-meta">{meta}</div> : null}
    </section>
  )
}
