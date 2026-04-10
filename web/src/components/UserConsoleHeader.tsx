import { useEffect, useState } from 'react'

import { Icon } from '../lib/icons'

import LanguageSwitcher from './LanguageSwitcher'
import ThemeToggle from './ThemeToggle'
import { Button } from './ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from './ui/dropdown-menu'

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
  sessionAvatarUrl?: string | null
  adminLabel: string
  isAdmin: boolean
  adminHref?: string | null
  adminActionLabel?: string | null
  adminMenuLabel?: string | null
  logoutVisible: boolean
  isLoggingOut: boolean
  logoutLabel: string
  loggingOutLabel: string
  onLogout: () => void
}

interface UserConsoleAvatarProps {
  avatarUrl?: string | null
  displayName: string
  className: string
  imageClassName: string
}

function UserConsoleAvatar(props: UserConsoleAvatarProps): JSX.Element {
  const [broken, setBroken] = useState(false)
  const initial = props.displayName.trim().charAt(0).toUpperCase() || '?'

  useEffect(() => {
    setBroken(false)
  }, [props.avatarUrl])

  if (props.avatarUrl && !broken) {
    return (
      <img
        src={props.avatarUrl}
        alt=""
        aria-hidden="true"
        className={props.imageClassName}
        loading="lazy"
        referrerPolicy="no-referrer"
        onError={() => setBroken(true)}
      />
    )
  }

  return (
    <span className={props.className} aria-hidden="true">
      {initial}
    </span>
  )
}

export default function UserConsoleHeader(props: UserConsoleHeaderProps): JSX.Element {
  const hasAdminAction = Boolean(props.adminHref && props.adminActionLabel)
  const accountName = props.sessionDisplayName ?? props.adminLabel
  const accountMeta = [props.sessionProviderLabel, props.isAdmin ? props.adminLabel : null]
    .filter((value): value is string => Boolean(value))
    .join(' · ')
  const contextSummary = props.currentViewDescription === props.subtitle
    ? props.subtitle
    : `${props.subtitle} · ${props.currentViewDescription}`
  const showAccountMenu = Boolean(
    props.sessionDisplayName || props.sessionProviderLabel || props.isAdmin || hasAdminAction || props.logoutVisible,
  )

  return (
    <section className="surface app-header user-console-header">
      <div className="user-console-header-primary">
        <div className="user-console-header-context" title={contextSummary}>
          <span className="user-console-header-eyebrow">{props.eyebrow}</span>
          <span className="user-console-header-summary">{contextSummary}</span>
        </div>
        <div className="user-console-header-title-row">
          <h1>{props.title}</h1>
          <div className="user-console-header-inline-meta" aria-label={`${props.currentViewLabel}: ${props.currentViewTitle}`}>
            <span className="user-console-header-inline-chip user-console-header-inline-chip-view">
              {props.currentViewTitle}
            </span>
          </div>
        </div>
      </div>

      <div className="user-console-header-actions" aria-label={props.sessionLabel}>
        <ThemeToggle />
        <LanguageSwitcher />

        {showAccountMenu && (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                type="button"
                variant="outline"
                size="xs"
                className="user-console-account-trigger"
                aria-label={`${props.sessionLabel}: ${accountName}`}
              >
                <UserConsoleAvatar
                  avatarUrl={props.sessionAvatarUrl}
                  displayName={accountName}
                  className="user-console-account-avatar user-console-account-avatar-fallback user-console-account-trigger-icon"
                  imageClassName="user-console-account-avatar user-console-account-avatar-image user-console-account-trigger-icon"
                />
                <span className="user-console-account-name">{accountName}</span>
                <Icon
                  icon="mdi:chevron-down"
                  width={14}
                  height={14}
                  aria-hidden="true"
                  className="user-console-account-trigger-chevron"
                />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" sideOffset={8} className="user-console-account-menu">
              <div className="user-console-account-summary">
                <UserConsoleAvatar
                  avatarUrl={props.sessionAvatarUrl}
                  displayName={accountName}
                  className="user-console-account-avatar user-console-account-avatar-fallback user-console-account-summary-icon"
                  imageClassName="user-console-account-avatar user-console-account-avatar-image user-console-account-summary-icon"
                />
                <div className="user-console-account-summary-body">
                  <span className="user-console-account-summary-name">{accountName}</span>
                  {accountMeta && <span className="user-console-account-summary-meta">{accountMeta}</span>}
                </div>
              </div>

              {(hasAdminAction || props.logoutVisible) && <DropdownMenuSeparator />}

              {hasAdminAction && (
                <DropdownMenuItem
                  className="user-console-account-menu-item user-console-account-menu-admin"
                  onSelect={() => {
                    if (props.adminHref) {
                      window.location.href = props.adminHref
                    }
                  }}
                >
                  <Icon icon="mdi:crown-outline" width={16} height={16} aria-hidden="true" />
                  <span>{props.adminMenuLabel ?? props.adminActionLabel}</span>
                </DropdownMenuItem>
              )}

              {props.logoutVisible && (
                <DropdownMenuItem
                  className="user-console-account-menu-item user-console-account-menu-logout"
                  onSelect={(event) => {
                    event.preventDefault()
                    if (!props.isLoggingOut) {
                      props.onLogout()
                    }
                  }}
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
                </DropdownMenuItem>
              )}
            </DropdownMenuContent>
          </DropdownMenu>
        )}
      </div>
    </section>
  )
}
