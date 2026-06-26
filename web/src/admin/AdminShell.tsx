import BrandLockup from '../components/BrandLockup'
import { Icon } from '../lib/icons'
import { createContext, type PropsWithChildren, type ReactNode, useCallback, useContext, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { ADMIN_SIDEBAR_STACK_MAX, useResponsiveModes } from '../lib/responsive'

import AdminNavButton from './AdminNavButton'
import type { AdminAnalysisView, AdminModuleId } from './routes'

export type AdminNavTarget =
  | AdminModuleId
  | 'analysis-usage'
  | 'analysis-rankings'
  | 'analysis-pressure'
  | 'system-settings-ha'

export interface AdminNavSubItem {
  target: AdminNavTarget
  label: string
}

export interface AdminNavItem {
  target: AdminNavTarget
  label: string
  icon: ReactNode
  children?: AdminNavSubItem[]
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
  const activeLayoutClass = `admin-layout--${activeItem.replaceAll('_', '-')}`
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

  const handleSelectItem = useCallback((target: AdminNavTarget) => {
    if (isStackedSidebar) setIsMenuOpen(false)
    onSelectItem(target)
  }, [isStackedSidebar, onSelectItem])

  return (
    <AdminSidebarUtilityContext.Provider value={sidebarUtilityHost}>
      <div
        className={`admin-layout ${activeLayoutClass} viewport-${viewportMode} content-${contentMode}${isCompactLayout ? ' is-compact-layout' : ''}`}
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
            <BrandLockup
              title="Tavily Hikari"
              compact
              className="admin-sidebar-brand"
              markClassName="admin-sidebar-brand-mark"
            />
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
                const childActive = item.children?.some((child) => child.target === activeItem) ?? false
                return (
                  <div key={item.target} className="admin-nav-group">
                    <AdminNavButton
                      icon={item.icon}
                      active={active}
                      className={childActive ? 'admin-nav-item-parent-active' : undefined}
                      onClick={() => handleSelectItem(item.target)}
                    >
                      <span>{item.label}</span>
                    </AdminNavButton>
                    {item.children && item.children.length > 0 && (
                      <div className="admin-nav-subitems" aria-label={item.label}>
                        {item.children.map((child) => (
                          <button
                            key={child.target}
                            type="button"
                            className={`admin-nav-subitem${child.target === activeItem ? ' admin-nav-subitem-active' : ''}`}
                            aria-current={child.target === activeItem ? 'page' : undefined}
                            onClick={() => handleSelectItem(child.target)}
                          >
                            <span className="admin-nav-subitem-marker" aria-hidden="true" />
                            <span>{child.label}</span>
                          </button>
                        ))}
                      </div>
                    )}
                  </div>
                )
              })}
            </nav>
            <div ref={setSidebarUtilityHost} className="admin-sidebar-utility" />
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
    return null
  }

  return createPortal(children, host)
}
