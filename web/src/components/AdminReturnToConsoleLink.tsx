import { Icon } from '@iconify/react'

import { ADMIN_USER_CONSOLE_HREF } from '../lib/adminUserConsoleEntry'

interface AdminReturnToConsoleLinkProps {
  label: string
  href?: string
  className?: string
}

export default function AdminReturnToConsoleLink({
  label,
  href = ADMIN_USER_CONSOLE_HREF,
  className,
}: AdminReturnToConsoleLinkProps): JSX.Element {
  const classes = ['admin-return-link', className].filter(Boolean).join(' ')

  return (
    <a href={href} className={classes} aria-label={label}>
      <Icon icon="mdi:monitor-dashboard" width={16} height={16} aria-hidden="true" />
      <span>{label}</span>
    </a>
  )
}
