import React, { type ReactNode } from 'react'
import BrandLockup from './BrandLockup'
import { Icon } from '../lib/icons'

import type { PublicMetrics } from '../api'
import type { Translations } from '../i18n'
import { useTheme } from '../theme'
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

const ingressParticlePaths = [
  { id: 'hero-flow-in-1', duration: 4.8, offset: 0 },
  { id: 'hero-flow-in-2', duration: 5.2, offset: 0.32 },
  { id: 'hero-flow-in-3', duration: 5, offset: 0.64 },
  { id: 'hero-flow-in-4', duration: 4.4, offset: 0.96 },
  { id: 'hero-flow-in-5', duration: 5.1, offset: 1.28 },
  { id: 'hero-flow-in-6', duration: 4.9, offset: 1.6 },
  { id: 'hero-flow-in-7', duration: 5.4, offset: 1.92 },
] as const

const egressParticlePaths = [
  { id: 'hero-flow-out-purple', colorClassName: 'hero-flow-particle-purple', duration: 3.6, offset: 0 },
  { id: 'hero-flow-out-blue', colorClassName: 'hero-flow-particle-blue', duration: 3.7, offset: 0.45 },
  { id: 'hero-flow-out-green', colorClassName: 'hero-flow-particle-green', duration: 3.9, offset: 0.9 },
  { id: 'hero-flow-out-amber', colorClassName: 'hero-flow-particle-amber', duration: 4.1, offset: 1.35 },
] as const

const particleTrail = [
  { delay: 0, radius: 4.8, className: 'hero-flow-particle-core' },
  { delay: 0.52, radius: 3.4, className: 'hero-flow-particle-mid' },
  { delay: 1.04, radius: 2.5, className: 'hero-flow-particle-soft' },
] as const

const egressParticleTrail = [
  { delay: 0, radius: 5.6, className: 'hero-flow-particle-core' },
  { delay: 0.38, radius: 4.2, className: 'hero-flow-particle-mid' },
  { delay: 0.76, radius: 3.2, className: 'hero-flow-particle-soft' },
  { delay: 1.14, radius: 2.4, className: 'hero-flow-particle-soft' },
] as const

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
  const { resolvedTheme } = useTheme()
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
  const loadBalancerImageSrc = resolvedTheme === 'dark'
    ? '/assets/public-hero-load-balancer-dark.png'
    : '/assets/public-hero-load-balancer.png'

  return (
    <section className="surface public-home-hero">
      <div className="language-switcher-row">{topControls}</div>
      <h1 className="sr-only">{publicStrings.heroTitle}</h1>
      <div className="public-home-brand-wrap">
        <BrandLockup
          title="Tavily Hikari"
          className="public-home-brand-lockup"
          markClassName="public-home-brand-mark"
        />
      </div>
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
        <div className="public-home-traffic-stage public-home-load-balancer-stage" aria-hidden="true">
          <img
            src={loadBalancerImageSrc}
            alt=""
            className={`public-home-load-balancer-image public-home-load-balancer-image-${resolvedTheme}`}
            width={1672}
            height={941}
            decoding="async"
            loading="eager"
            draggable={false}
          />
          <div className="public-home-load-balancer-motion">
            <svg
              className="public-home-load-balancer-debug"
              viewBox="0 0 1672 941"
              preserveAspectRatio="none"
              focusable="false"
              aria-hidden="true"
            >
              <defs>
                <path
                  id="hero-flow-in-1"
                  d="M 131.5 214.5 H 360 C 390 214.5 410 218 430 230 C 455 250 470 275 495 292 C 515 306 540 314 568 314"
                />
                <path
                  id="hero-flow-in-2"
                  d="M 130.8 285.5 H 360 C 388 286 404 300 426 319 C 452 339 486 350 552 358"
                />
                <path
                  id="hero-flow-in-3"
                  d="M 130.9 356 H 360 C 392 356 416 361 441 372 C 466 383 492 389 526 397"
                />
                <path id="hero-flow-in-4" d="M 128 438.2 H 555" />
                <path
                  id="hero-flow-in-5"
                  d="M 130 518.7 H 360 C 398 518.7 419 507 444 495 C 474 482 507 489 546 493"
                />
                <path
                  id="hero-flow-in-6"
                  d="M 130.2 603.6 H 360 C 395 603.6 421 585 449 566 C 481 547 522 532 562 536"
                />
                <path
                  id="hero-flow-in-7"
                  d="M 129.6 684.1 H 380 C 412 684.1 430 676 447 655 C 466 632 470 604 497 584 C 517 571 542 560 568 559"
                />
                <path
                  id="hero-flow-out-purple"
                  d="M 1018 314 C 1020.2 314.2 1026.8 314.2 1031 315 C 1035.2 315.8 1038 319.5 1043 319 C 1048 318.5 1053.3 314.8 1061 312 C 1068.7 309.2 1080.2 305.8 1089 302 C 1097.8 298.2 1106.2 294 1114 289 C 1121.8 284 1129.3 278.2 1136 272 C 1142.7 265.8 1148 258.3 1154 252 C 1160 245.7 1165.8 239.7 1172 234 C 1178.2 228.3 1184.2 222.7 1191 218 C 1197.8 213.3 1205.2 209.3 1213 206 C 1220.8 202.7 1229.5 200.2 1238 198 C 1246.5 195.8 1255.8 193.8 1264 193 C 1272.2 192.2 1280.5 193 1287 193 C 1293.5 193 1294.2 193.5 1303 193 C 1311.8 192.5 1333.8 190.5 1340 190"
                />
                <path
                  id="hero-flow-out-blue"
                  d="M 1018 430 C 1027.2 430 1059.2 429.7 1073 430 C 1086.8 430.3 1092 432 1101 432 C 1110 432 1119.2 431 1127 430 C 1134.8 429 1140.7 427.8 1148 426 C 1155.3 424.2 1163.3 422 1171 419 C 1178.7 416 1186.2 412 1194 408 C 1201.8 404 1210 398.7 1218 395 C 1226 391.3 1234.2 388.2 1242 386 C 1249.8 383.8 1257.5 382.8 1265 382 C 1272.5 381.2 1280 381.2 1287 381 C 1294 380.8 1298.2 380.8 1307 381 C 1315.8 381.2 1334.5 381.8 1340 382"
                />
                <path
                  id="hero-flow-out-green"
                  d="M 1018 522 C 1025.2 524.3 1050.2 533.7 1061 536 C 1071.8 538.3 1075.3 535.5 1083 536 C 1090.7 536.5 1099 537.7 1107 539 C 1115 540.3 1123.2 541.8 1131 544 C 1138.8 546.2 1146.8 549 1154 552 C 1161.2 555 1167.7 558.7 1174 562 C 1180.3 565.3 1185.7 569 1192 572 C 1198.3 575 1205.5 577.8 1212 580 C 1218.5 582.2 1224.3 583.8 1231 585 C 1237.7 586.2 1244.8 586.7 1252 587 C 1259.2 587.3 1266.3 587 1274 587 C 1281.7 587 1289.8 587.2 1298 587 C 1306.2 586.8 1316 586.5 1323 586 C 1330 585.5 1337.2 584.3 1340 584"
                />
                <path
                  id="hero-flow-out-amber"
                  d="M 990 626 C 994.3 625.3 1009.7 620.2 1016 622 C 1022.3 623.8 1021.7 633.2 1028 637 C 1034.3 640.8 1047.3 642.7 1054 645 C 1060.7 647.3 1062.5 647.8 1068 651 C 1073.5 654.2 1080.7 659.2 1087 664 C 1093.3 668.8 1100.7 674.8 1106 680 C 1111.3 685.2 1115.5 691 1119 695 C 1122.5 699 1124 700.5 1127 704 C 1130 707.5 1132.3 711 1137 716 C 1141.7 721 1148.8 728.5 1155 734 C 1161.2 739.5 1167.5 744.7 1174 749 C 1180.5 753.3 1187 756.8 1194 760 C 1201 763.2 1208.2 765.8 1216 768 C 1223.8 770.2 1232.7 772 1241 773 C 1249.3 774 1256.7 773.8 1266 774 C 1275.3 774.2 1287.7 773.8 1297 774 C 1306.3 774.2 1314.8 776.5 1322 775 C 1329.2 773.5 1337 766.7 1340 765"
                />
              </defs>
              <g className="hero-flow-orbs hero-flow-orbs-ingress">
                {ingressParticlePaths.flatMap((flow) =>
                  particleTrail.map((particle, particleIndex) => (
                    <circle
                      key={`${flow.id}-${particle.className}-${particleIndex}`}
                      className={`hero-flow-particle hero-flow-particle-ingress ${particle.className}`}
                      r={particle.radius}
                    >
                      <animateMotion
                        dur={`${flow.duration}s`}
                        repeatCount="indefinite"
                        begin={`${-(flow.offset + particle.delay)}s`}
                      >
                        <mpath href={`#${flow.id}`} />
                      </animateMotion>
                    </circle>
                  )),
                )}
              </g>
              <g className="hero-flow-orbs hero-flow-orbs-egress">
                {egressParticlePaths.flatMap((flow) =>
                  egressParticleTrail.map((particle, particleIndex) => (
                    <circle
                      key={`${flow.id}-${particle.className}-${particleIndex}`}
                      className={`hero-flow-particle ${flow.colorClassName} ${particle.className}`}
                      r={particle.radius}
                    >
                      <animateMotion
                        dur={`${flow.duration}s`}
                        repeatCount="indefinite"
                        begin={`${-(flow.offset + particle.delay)}s`}
                      >
                        <mpath href={`#${flow.id}`} />
                      </animateMotion>
                    </circle>
                  )),
                )}
              </g>
            </svg>
            <span className="hero-dial-breath" />
            <span className="hero-pointer-breath" />
            <span className="hero-key-breath hero-key-breath-purple" />
            <span className="hero-key-breath hero-key-breath-blue" />
            <span className="hero-key-breath hero-key-breath-green" />
            <span className="hero-key-breath hero-key-breath-amber" />
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
