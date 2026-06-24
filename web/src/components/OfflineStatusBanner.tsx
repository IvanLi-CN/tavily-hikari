import { Icon } from '../lib/icons'

interface OfflineStatusBannerProps {
  title: string
  description: string
}

export default function OfflineStatusBanner({
  title,
  description,
}: OfflineStatusBannerProps): JSX.Element {
  return (
    <section className="surface error-banner offline-status-banner" role="status" aria-live="polite">
      <div className="offline-status-banner-icon" aria-hidden="true">
        <Icon icon="mdi:web-off" width={20} height={20} />
      </div>
      <div className="offline-status-banner-copy">
        <strong>{title}</strong>
        <span>{description}</span>
      </div>
    </section>
  )
}
