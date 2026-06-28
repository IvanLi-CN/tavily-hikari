import React, { type ReactNode } from 'react'
import BrandLockup from './BrandLockup'
import { Icon } from '../lib/icons'

import type { PublicMetrics } from '../api'
import type { Translations } from '../i18n'
import RollingNumber from './RollingNumber'
import { Button } from './ui/button'

export interface PublicHomeHeroCardProps {
  publicStrings: Translations['public']
  metricsLoading: boolean
  summaryLoading: boolean
  metrics: PublicMetrics | null
  availableKeys: number | null
  totalKeys: number | null
  error: string | null
  showAuthStatusLoading?: boolean
  showAuthStatusUnavailable?: boolean
  showLinuxDoLogin: boolean
  showRegistrationPausedNotice?: boolean
  showTokenAccessButton: boolean
  showAdminAction: boolean
  adminActionLabel: string
  topControls?: ReactNode
  linuxDoHref?: string
  onLinuxDoLogin?: () => void
  onTokenAccessClick?: () => void
  onAdminActionClick?: () => void
}

const heroSecondaryButtonClassName =
  'h-auto rounded-full border-foreground/20 bg-card/95 px-4 py-[0.72rem] text-foreground no-underline shadow-[0_10px_20px_-18px_hsl(var(--foreground)/0.5)] hover:-translate-y-[1px] hover:border-primary/50 hover:bg-card hover:text-foreground'

const heroPrimaryButtonClassName = 'h-auto rounded-full px-4 py-[0.72rem]'

function PublicHomeHeroCard({
  publicStrings,
  metricsLoading,
  summaryLoading,
  metrics,
  availableKeys,
  totalKeys,
  error,
  showAuthStatusLoading = false,
  showAuthStatusUnavailable = false,
  showLinuxDoLogin,
  showRegistrationPausedNotice = false,
  showTokenAccessButton,
  showAdminAction,
  adminActionLabel,
  topControls,
  linuxDoHref = '/auth/linuxdo',
  onLinuxDoLogin,
  onTokenAccessClick,
  onAdminActionClick,
}: PublicHomeHeroCardProps): JSX.Element {
  const showAuthStatus = showAuthStatusLoading || showAuthStatusUnavailable
  const shouldShowActions = showAuthStatus || showLinuxDoLogin || showTokenAccessButton || showAdminAction
  const authStatusText = showAuthStatusUnavailable
    ? publicStrings.authStatus.unavailable
    : publicStrings.authStatus.checking
  const authStatusActionText = showAuthStatusUnavailable
    ? publicStrings.authStatus.unavailableAction
    : publicStrings.authStatus.checkingAction
  const authStatusIcon = showAuthStatusUnavailable ? 'mdi:alert-circle-outline' : 'mdi:loading'
  const linuxDoContent = (
    <>
      <img src="/assets/linuxdo-logo.svg" alt={publicStrings.linuxDoLogin.logoAlt} width={20} height={20} />
      <span>{publicStrings.linuxDoLogin.button}</span>
    </>
  )

  return (
    <section className="surface public-home-hero">
      <div className="language-switcher-row">{topControls}</div>
      <div className="public-home-brand-wrap">
        <BrandLockup
          title="Tavily Hikari"
          className="public-home-brand-lockup"
          markClassName="public-home-brand-mark"
        />
      </div>
      <h1 className="hero-title">{publicStrings.heroTitle}</h1>
      <p className="public-home-description">{publicStrings.heroDescription}</p>
      {error && <div className="surface error-banner" role="status">{error}</div>}
      {showRegistrationPausedNotice && (
        <div
          className="mx-auto mt-4 flex w-full max-w-5xl items-start gap-4 rounded-2xl border border-amber-300/65 bg-[linear-gradient(180deg,rgba(255,251,235,0.96),rgba(255,247,237,0.92))] px-4 py-3.5 text-left text-amber-950 shadow-[0_12px_24px_-24px_rgba(180,83,9,0.18)] dark:border-amber-300/28 dark:bg-[linear-gradient(180deg,rgba(120,53,15,0.28),rgba(69,38,10,0.78))] dark:text-amber-50 dark:backdrop-blur-sm dark:shadow-[0_16px_32px_-30px_rgba(245,158,11,0.2)]"
          role="status"
          aria-live="polite"
        >
          <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-2xl border border-amber-300/60 bg-amber-100/90 text-amber-700 shadow-inner dark:border-amber-200/16 dark:bg-amber-200/10 dark:text-amber-100">
            <Icon icon="mdi:pause-circle-outline" width={22} height={22} aria-hidden="true" />
          </div>
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <div className="inline-flex items-center rounded-full bg-amber-200/82 px-2.5 py-1 text-[11px] font-semibold uppercase tracking-[0.16em] text-amber-950 dark:bg-amber-200/14 dark:text-amber-50">
                {publicStrings.registrationPaused.badge}
              </div>
              <div className="text-base font-semibold tracking-[-0.01em] text-amber-950 dark:text-amber-50">
                {publicStrings.registrationPausedNotice.title}
              </div>
            </div>
            <p className="mb-0 mt-1.5 text-sm leading-6 text-amber-900/88 dark:text-amber-50/92">
              {publicStrings.registrationPausedNotice.description}
            </p>
          </div>
        </div>
      )}
      {showAuthStatus && (
        <div
          className={`public-home-auth-status${showAuthStatusUnavailable ? ' public-home-auth-status-error' : ''}`}
          role="status"
          aria-live="polite"
        >
          <Icon
            icon={authStatusIcon}
            width={18}
            height={18}
            className={showAuthStatusLoading ? 'icon-spin' : undefined}
            aria-hidden="true"
          />
          <span>{authStatusText}</span>
        </div>
      )}
      <div className="public-home-traffic-board" aria-label={publicStrings.metrics.pool.title}>
        <div className="public-home-traffic-stage" aria-hidden="true">
          <div className="traffic-node traffic-node-client">
            <span className="traffic-node-icon">
              <Icon icon="mdi:console-network-outline" width={24} height={24} />
            </span>
            <strong>Client</strong>
            <small>{publicStrings.metrics.daily.subtitle}</small>
          </div>
          <div className="traffic-pulse-track">
            <span />
            <span />
          </div>
          <div className="traffic-node traffic-node-core">
            <span className="traffic-node-icon">
              <Icon icon="mdi:shield-key-outline" width={25} height={25} />
            </span>
            <strong>Hikari</strong>
            <small>{publicStrings.metrics.monthly.subtitle}</small>
          </div>
          <div className="traffic-pulse-track traffic-pulse-track-alt">
            <span />
            <span />
          </div>
          <div className="traffic-node traffic-node-upstream">
            <span className="traffic-node-icon">
              <Icon icon="mdi:cloud-check-outline" width={24} height={24} />
            </span>
            <strong>Upstream</strong>
            <small>{publicStrings.metrics.pool.subtitle}</small>
          </div>
        </div>
        <div className="public-home-metric-rail">
          <div className="metric-card public-home-metric-pill">
            <p className="metric-card-title">{publicStrings.metrics.monthly.title}</p>
            <div className="metric-value">
              <RollingNumber value={metricsLoading ? null : metrics?.monthlySuccess ?? 0} />
            </div>
          </div>
          <div className="metric-card public-home-metric-pill">
            <p className="metric-card-title">{publicStrings.metrics.daily.title}</p>
            <div className="metric-value">
              <RollingNumber value={metricsLoading ? null : metrics?.dailySuccess ?? 0} />
            </div>
          </div>
          <div className="metric-card public-home-metric-pill">
            <p className="metric-card-title">{publicStrings.metrics.pool.title}</p>
            <div className="metric-value">
              {summaryLoading ? '—' : availableKeys != null && totalKeys != null ? `${availableKeys}/${totalKeys}` : '—'}
            </div>
          </div>
        </div>
      </div>
      {shouldShowActions && (
        <div className="public-home-actions">
          {showAuthStatus && (
            <Button
              type="button"
              variant="outline"
              className={`auth-status-button ${heroSecondaryButtonClassName}`}
              disabled
              aria-label={authStatusActionText}
            >
              <Icon
                icon={authStatusIcon}
                width={20}
                height={20}
                className={showAuthStatusLoading ? 'icon-spin' : undefined}
                aria-hidden="true"
              />
              <span>{authStatusActionText}</span>
            </Button>
          )}
          {showLinuxDoLogin && (
            onLinuxDoLogin
              ? (
                  <Button
                    type="button"
                    className={`linuxdo-login-button ${heroPrimaryButtonClassName}`}
                    aria-label={publicStrings.linuxDoLogin.button}
                    onClick={onLinuxDoLogin}
                  >
                    {linuxDoContent}
                  </Button>
                )
              : (
                  <Button asChild className={`linuxdo-login-button ${heroPrimaryButtonClassName}`}>
                    <a href={linuxDoHref} aria-label={publicStrings.linuxDoLogin.button}>
                      {linuxDoContent}
                    </a>
                  </Button>
                )
          )}
          {showTokenAccessButton && (
            <Button
              type="button"
              variant="outline"
              className={`token-access-button ${heroSecondaryButtonClassName}`}
              onClick={onTokenAccessClick}
            >
              <Icon icon="mdi:key-outline" aria-hidden="true" className="token-access-icon" />
              <span>{publicStrings.tokenAccess.button}</span>
            </Button>
          )}
          {showAdminAction && (
            <Button
              type="button"
              className={`public-home-admin-button ${heroPrimaryButtonClassName}`}
              onClick={onAdminActionClick}
            >
              {adminActionLabel}
            </Button>
          )}
        </div>
      )}
    </section>
  )
}

export default PublicHomeHeroCard
