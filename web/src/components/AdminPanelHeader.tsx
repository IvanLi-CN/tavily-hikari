import { Icon } from '@iconify/react'

import LanguageSwitcher from './LanguageSwitcher'
import ThemeToggle from './ThemeToggle'
import { Button } from './ui/button'

interface AdminPanelHeaderProps {
  title: string
  subtitle: string
  displayName?: string | null
  isAdmin: boolean
  updatedPrefix: string
  updatedTime: string | null
  isRefreshing: boolean
  refreshLabel: string
  refreshingLabel: string
  onRefresh: () => void
}

export default function AdminPanelHeader(props: AdminPanelHeaderProps): JSX.Element {
  return (
    <section className="surface app-header admin-panel-header">
      <div className="admin-panel-header-main">
        <h1>{props.title}</h1>
        <p className="admin-panel-header-subtitle">{props.subtitle}</p>
      </div>

      <div className="admin-panel-header-side">
        <div className="admin-panel-header-tools">
          <div className="admin-language-switcher">
            <ThemeToggle />
            <LanguageSwitcher />
          </div>
          {props.displayName && (
            <div className={`user-badge${props.isAdmin ? ' user-badge-admin' : ''}`}>
              {props.isAdmin && <Icon icon="mdi:crown-outline" className="user-badge-icon" aria-hidden="true" />}
              <span>{props.displayName}</span>
            </div>
          )}
        </div>

        <div className="admin-panel-header-actions">
          {props.updatedTime && (
            <span className="admin-panel-header-time" aria-live="polite">
              <Icon icon="mdi:clock-time-four-outline" width={14} height={14} className="admin-panel-header-time-icon" aria-hidden="true" />
              <span className="admin-panel-header-time-label">{props.updatedPrefix}</span>
              <span className="admin-panel-header-time-value">{props.updatedTime}</span>
            </span>
          )}

          <Button type="button" variant="outline" size="sm" className="admin-panel-refresh-button" onClick={props.onRefresh} disabled={props.isRefreshing}>
            <Icon
              icon={props.isRefreshing ? 'mdi:loading' : 'mdi:refresh'}
              width={16}
              height={16}
              className={props.isRefreshing ? 'icon-spin' : undefined}
              aria-hidden="true"
            />
            <span>{props.isRefreshing ? props.refreshingLabel : props.refreshLabel}</span>
          </Button>
        </div>
      </div>
    </section>
  )
}
