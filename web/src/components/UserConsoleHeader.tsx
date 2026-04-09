import { Icon } from '../lib/icons'

import LanguageSwitcher from './LanguageSwitcher'
import ThemeToggle from './ThemeToggle'
import { Button } from './ui/button'

interface UserConsoleHeaderProps {
  title: string
  subtitle: string
  eyebrow: string
  currentViewLabel: string
  currentViewTitle: string
  currentViewDescription: string
  sessionLabel: string
  sessionDisplayName?: string | null
  sessionProviderLabel?: string | null
  adminLabel: string
  isAdmin: boolean
  adminHref?: string | null
  adminActionLabel?: string | null
  logoutVisible: boolean
  isLoggingOut: boolean
  logoutLabel: string
  loggingOutLabel: string
  onLogout: () => void
}

export default function UserConsoleHeader(props: UserConsoleHeaderProps): JSX.Element {
  const shouldRenderSessionCard = Boolean(props.sessionDisplayName || props.sessionProviderLabel || props.isAdmin)

  return (
    <section className="surface app-header user-console-header">
      <div className="user-console-header-main">
        <div className="user-console-header-eyebrow-row">
          <span className="user-console-header-eyebrow">{props.eyebrow}</span>
        </div>

        <div className="user-console-header-copy">
          <h1>{props.title}</h1>
          <p className="user-console-header-subtitle">{props.subtitle}</p>
        </div>

        <div className={`user-console-header-summary-grid${shouldRenderSessionCard ? '' : ' is-single-card'}`}>
          <article className="user-console-header-card">
            <span className="user-console-header-card-label">{props.currentViewLabel}</span>
            <strong className="user-console-header-card-value">{props.currentViewTitle}</strong>
            <p className="user-console-header-card-description">{props.currentViewDescription}</p>
          </article>

          {shouldRenderSessionCard && (
            <article className="user-console-header-card user-console-header-card-session">
              <span className="user-console-header-card-label">{props.sessionLabel}</span>
              <strong className="user-console-header-card-value">
                {props.sessionDisplayName ?? props.adminLabel}
              </strong>
              <div className="user-console-header-chip-row">
                {props.sessionProviderLabel && (
                  <span className="user-console-header-chip">{props.sessionProviderLabel}</span>
                )}
                {props.isAdmin && (
                  <span className="user-console-header-chip user-console-header-chip-admin">
                    {props.adminLabel}
                  </span>
                )}
              </div>
            </article>
          )}
        </div>
      </div>

      <div className="user-console-header-side">
        <div className="user-console-header-tools">
          <ThemeToggle />
          <LanguageSwitcher />
        </div>

        <div className="user-console-header-actions">
          {props.adminHref && props.adminActionLabel && (
            <Button asChild variant="outline" size="sm" className="user-console-header-action user-console-admin-entry">
              <a href={props.adminHref}>
                <Icon icon="mdi:crown-outline" width={16} height={16} aria-hidden="true" />
                <span>{props.adminActionLabel}</span>
              </a>
            </Button>
          )}

          {props.logoutVisible && (
            <Button
              type="button"
              variant="secondary"
              size="sm"
              className="user-console-header-action user-console-logout-button"
              onClick={props.onLogout}
              disabled={props.isLoggingOut}
            >
              <Icon
                icon={props.isLoggingOut ? 'mdi:loading' : 'mdi:logout-variant'}
                width={16}
                height={16}
                className={props.isLoggingOut ? 'icon-spin' : undefined}
                aria-hidden="true"
              />
              <span>{props.isLoggingOut ? props.loggingOutLabel : props.logoutLabel}</span>
            </Button>
          )}
        </div>
      </div>
    </section>
  )
}
