import { AlertTriangle, DownloadCloud, Loader2, RefreshCw } from 'lucide-react'

import type { PublicTranslations } from '../i18n'
import type { PwaUpdateStatus } from '../pwa/runtime'
import { Button } from './ui/button'
import useUpdateAvailable from '../hooks/useUpdateAvailable'

interface UpdateAvailableBannerProps {
  strings: PublicTranslations['updateBanner']
  currentVersion: string | null
  availableVersion: string | null
  status: PwaUpdateStatus
  loading: boolean
  onUpdate: () => void
  onDismiss: () => void
}

export default function UpdateAvailableBanner({
  strings,
  currentVersion,
  availableVersion,
  status,
  loading,
  onUpdate,
  onDismiss,
}: UpdateAvailableBannerProps): JSX.Element {
  const isActivating = status === 'activating'
  const isFailed = status === 'activation-failed'
  const isPreparing = loading && !isActivating
  const description = isFailed
    ? strings.failureDescription
    : isActivating
    ? strings.activating
    : isPreparing
      ? strings.preparing
      : strings.description(currentVersion ?? 'unknown', availableVersion ?? 'latest')

  return (
    <section className={`surface update-banner${isFailed ? ' update-banner-failed' : ''}`} role="status" aria-live="polite">
      <div className="update-banner-status" aria-hidden="true">
        {loading
          ? <Loader2 className="update-banner-spinner" size={19} />
          : isFailed
            ? <AlertTriangle size={19} />
            : <DownloadCloud size={19} />}
      </div>
      <div className="update-banner-text">
        <strong>{isFailed ? strings.failureTitle : strings.title}</strong>
        <span>{description}</span>
      </div>
      <div className="update-banner-actions">
        <Button type="button" onClick={onUpdate} disabled={isActivating} aria-busy={loading}>
          {loading ? <Loader2 className="update-banner-button-spinner" size={16} aria-hidden="true" /> : <RefreshCw size={16} aria-hidden="true" />}
          {loading ? strings.refreshing : isFailed ? strings.retry : strings.refresh}
        </Button>
        <Button type="button" variant="ghost" onClick={onDismiss} disabled={isActivating}>
          {strings.dismiss}
        </Button>
      </div>
    </section>
  )
}

export function ConnectedUpdateAvailableBanner({
  strings,
}: {
  strings: PublicTranslations['updateBanner']
}): JSX.Element | null {
  const updateBanner = useUpdateAvailable()

  if (!updateBanner.visible) {
    return null
  }

  return (
    <UpdateAvailableBanner
      strings={strings}
      currentVersion={updateBanner.currentVersion}
      availableVersion={updateBanner.availableVersion}
      status={updateBanner.status}
      loading={updateBanner.loading}
      onUpdate={updateBanner.applyUpdate}
      onDismiss={updateBanner.dismiss}
    />
  )
}
