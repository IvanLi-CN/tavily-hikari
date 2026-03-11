import { Icon } from '@iconify/react'
import type { ButtonHTMLAttributes } from 'react'

import { cn } from '../lib/utils'
import { Button } from '../components/ui/button'

interface AdminNavButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  icon: string
  active?: boolean
}

export default function AdminNavButton({ icon, active = false, className, children, ...props }: AdminNavButtonProps): JSX.Element {
  return (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      className={cn('admin-nav-item h-auto w-full justify-start px-3 py-2.5 text-sm shadow-none', active && 'admin-nav-item-active', className)}
      aria-current={active ? 'page' : undefined}
      {...props}
    >
      <Icon icon={icon} width={18} height={18} aria-hidden="true" />
      <span>{children}</span>
    </Button>
  )
}
