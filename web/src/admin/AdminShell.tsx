import { Icon } from '@iconify/react'
import type { PropsWithChildren } from 'react'

import type { AdminModuleId } from './routes'

export interface AdminNavItem {
  module: AdminModuleId
  label: string
  icon: string
}

interface AdminShellProps extends PropsWithChildren {
  activeModule: AdminModuleId
  navItems: AdminNavItem[]
  skipToContentLabel: string
  onSelectModule: (module: AdminModuleId) => void
}

export default function AdminShell({
  activeModule,
  navItems,
  skipToContentLabel,
  onSelectModule,
  children,
}: AdminShellProps): JSX.Element {
  return (
    <div className="admin-layout">
      <a className="admin-skip-link" href="#admin-main-content">
        {skipToContentLabel}
      </a>

      <aside className="admin-sidebar surface" aria-label="Admin navigation">
        <div className="admin-sidebar-brand">
          <span className="admin-sidebar-brand-dot" aria-hidden="true" />
          <span>Tavily Hikari</span>
        </div>
        <nav className="admin-sidebar-nav">
          {navItems.map((item) => {
            const active = item.module === activeModule
            return (
              <button
                key={item.module}
                type="button"
                className={`admin-nav-item${active ? ' admin-nav-item-active' : ''}`}
                onClick={() => onSelectModule(item.module)}
                aria-current={active ? 'page' : undefined}
              >
                <Icon icon={item.icon} width={18} height={18} aria-hidden="true" />
                <span>{item.label}</span>
              </button>
            )
          })}
        </nav>
      </aside>

      <section id="admin-main-content" className="admin-main-content" role="main">
        <div className="app-shell admin-shell-content">{children}</div>
      </section>
    </div>
  )
}
