import { Icon } from '../lib/icons'
import { createContext, type PropsWithChildren, type ReactNode, useContext, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { ADMIN_SIDEBAR_STACK_MAX, useResponsiveModes } from '../lib/responsive'

import AdminNavButton from './AdminNavButton'
import type { AdminModuleId } from './routes'

export type AdminNavTarget = AdminModuleId | 'user-usage'

export interface AdminNavItem {
  target: AdminNavTarget
  label: string
  icon: ReactNode
}

interface AdminShellProps extends PropsWithChildren {
  activeItem: AdminNavTarget
  navItems: AdminNavItem[]
  skipToContentLabel: string
  onSelectItem: (target: AdminNavTarget) => void
}

const AdminSidebarUtilityContext = createContext<HTMLDivElement | null>(null)

function readStackedSidebarMode(): boolean {
  if (typeof window === 'undefined') return false
  return window.matchMedia(`(max-width: ${ADMIN_SIDEBAR_STACK_MAX}px)`).matches
}

export default function AdminShell({
  activeItem,
  navItems,
  skipToContentLabel,
  onSelectItem,
  children,
}: AdminShellProps): JSX.Element {
  const contentRef = useRef<HTMLElement>(null)
  const { viewportMode, contentMode, isCompactLayout } = useResponsiveModes(contentRef)
  const [isStackedSidebar, setIsStackedSidebar] = useState<boolean>(() => readStackedSidebarMode())
  const [isMenuOpen, setIsMenuOpen] = useState(false)
  const [sidebarUtilityHost, setSidebarUtilityHost] = useState<HTMLDivElement | null>(null)

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
  }, [activeItem, isStackedSidebar])

  useEffect(() => {
    if (!isStackedSidebar || !isMenuOpen) return
    const previousOverflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => {
      document.body.style.overflow = previousOverflow
    }
  }, [isMenuOpen, isStackedSidebar])

  return (
    <AdminSidebarUtilityContext.Provider value={sidebarUtilityHost}>
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
                const active = item.target === activeItem
                return (
                  <AdminNavButton
                    key={item.target}
                    icon={item.icon}
                    active={active}
                    onClick={() => onSelectItem(item.target)}
                  >
                    <span>{item.label}</span>
                  </AdminNavButton>
                )
              })}
            </nav>
            <div ref={setSidebarUtilityHost} className="admin-sidebar-utility admin-desktop-only" />
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
    </AdminSidebarUtilityContext.Provider>
  )
}

export function AdminShellSidebarUtility({ children }: PropsWithChildren): JSX.Element | null {
  const host = useContext(AdminSidebarUtilityContext)

  if (!host) {
    return <div className="admin-sidebar-utility admin-sidebar-utility-fallback admin-desktop-only">{children}</div>
  }

  return createPortal(children, host)
}
