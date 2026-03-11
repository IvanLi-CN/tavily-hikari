import { Icon } from '@iconify/react'
import { type PropsWithChildren, useEffect, useRef, useState } from 'react'
import { ADMIN_SIDEBAR_STACK_MAX, useResponsiveModes } from '../lib/responsive'

import AdminNavButton from './AdminNavButton'
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

function readStackedSidebarMode(): boolean {
  if (typeof window === 'undefined') return false
  return window.matchMedia(`(max-width: ${ADMIN_SIDEBAR_STACK_MAX}px)`).matches
}

export default function AdminShell({
  activeModule,
  navItems,
  skipToContentLabel,
  onSelectModule,
  children,
}: AdminShellProps): JSX.Element {
  const contentRef = useRef<HTMLElement>(null)
  const { viewportMode, contentMode, isCompactLayout } = useResponsiveModes(contentRef)
  const [isStackedSidebar, setIsStackedSidebar] = useState<boolean>(() => readStackedSidebarMode())
  const [isMenuOpen, setIsMenuOpen] = useState(false)

  useEffect(() => {
    const media = window.matchMedia(`(max-width: ${ADMIN_SIDEBAR_STACK_MAX}px)`)
    const apply = () => setIsStackedSidebar(media.matches)
    apply()
    media.addEventListener('change', apply)
    return () => media.removeEventListener('change', apply)
  }, [])

  useEffect(() => {
    if (!isStackedSidebar) {
      setIsMenuOpen(false)
      return
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setIsMenuOpen(false)
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [isStackedSidebar])

  useEffect(() => {
    if (isStackedSidebar) setIsMenuOpen(false)
  }, [activeModule, isStackedSidebar])

  useEffect(() => {
    if (!isStackedSidebar || !isMenuOpen) return
    const previousOverflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => {
      document.body.style.overflow = previousOverflow
    }
  }, [isMenuOpen, isStackedSidebar])

  return (
    <div
      className={`admin-layout viewport-${viewportMode} content-${contentMode}${isCompactLayout ? ' is-compact-layout' : ''}`}
    >
      <a className="admin-skip-link" href="#admin-main-content">
        {skipToContentLabel}
      </a>

      {isStackedSidebar && isMenuOpen && (
        <button
          type="button"
          className="admin-sidebar-backdrop"
          aria-label="Close navigation menu"
          onClick={() => setIsMenuOpen(false)}
        />
      )}

      <aside className={`admin-sidebar surface${isStackedSidebar ? ' is-stacked' : ''}`} aria-label="Admin navigation">
        <div className="admin-sidebar-topbar">
          <div className="admin-sidebar-brand">
            <span className="admin-sidebar-brand-dot" aria-hidden="true" />
            <span>Tavily Hikari</span>
          </div>
          {isStackedSidebar && (
            <button
              type="button"
              className={`admin-menu-toggle${isMenuOpen ? ' is-open' : ''}`}
              aria-expanded={isMenuOpen}
              aria-controls="admin-sidebar-nav"
              onClick={() => setIsMenuOpen((open) => !open)}
            >
              <Icon icon={isMenuOpen ? 'mdi:close' : 'mdi:menu'} width={18} height={18} aria-hidden="true" />
              <span>{isMenuOpen ? 'Close' : 'Menu'}</span>
            </button>
          )}
        </div>
        <div className={`admin-sidebar-menu${!isStackedSidebar || isMenuOpen ? ' is-open' : ''}`}>
          <nav id="admin-sidebar-nav" className="admin-sidebar-nav">
            {navItems.map((item) => {
              const active = item.module === activeModule
              return (
                <AdminNavButton
                  key={item.module}
                  icon={item.icon}
                  active={active}
                  onClick={() => onSelectModule(item.module)}
                >
                  <span>{item.label}</span>
                </AdminNavButton>
              )
            })}
          </nav>
        </div>
      </aside>

      <section
        ref={contentRef}
        id="admin-main-content"
        className={`admin-main-content viewport-${viewportMode} content-${contentMode}${isCompactLayout ? ' is-compact-layout' : ''}`}
        role="main"
      >
        <div className="app-shell admin-shell-content">{children}</div>
      </section>
    </div>
  )
}
