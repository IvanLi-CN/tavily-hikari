/**
 * Per-provider key detail quota block + sync-usage button (plan todo 22).
 *
 * The Tavily rendering is byte-stable: `KeyQuotaPanel` reproduces the exact
 * legacy markup ("Quota" / "Tavily Usage for this key" / Used / Remaining /
 * Synced) so existing snapshots and stories do not shift. SerpApi/Brave/Exa
 * switch on `quotaDisplayKind` and read their localized note from the stable
 * `admin.providers.<id>.quotaNote` registration point.
 */
import type { ComponentProps } from 'react'

import { parseProviderId, quotaDisplayKind, supportsSyncUsage, syncApiKeyUsage, type ApiKeyStats } from '../api'
import AdminLoadingRegion from '../components/AdminLoadingRegion'
import { Button } from '../components/ui/button'
import { useTranslate } from '../i18n'
import { Icon } from '../lib/icons'
import type { QueryLoadState } from './queryLoadState'

type SyncState = 'idle' | 'syncing' | 'success'

export interface KeyQuotaPanelProps {
  detail: ApiKeyStats | null
  loadState: QueryLoadState
  loadingLabel: string
  syncNotice?: string | null
  formatNumber: (value: number) => string
  formatTimestamp: (value: number | null) => string
  formatPercent: (numerator: number, denominator: number) => string
}

interface MetricCard {
  id: string
  label: string
  value: string
  subtitle: string
}

function tavilyMetricCards(
  detail: ApiKeyStats | null,
  formatNumber: KeyQuotaPanelProps['formatNumber'],
  formatTimestamp: KeyQuotaPanelProps['formatTimestamp'],
  formatPercent: KeyQuotaPanelProps['formatPercent'],
): MetricCard[] {
  const limit = detail?.quota_limit ?? null
  const remaining = detail?.quota_remaining ?? null
  const used = limit != null && remaining != null ? Math.max(limit - remaining, 0) : null
  const percent = limit && remaining != null && limit > 0 ? formatPercent(remaining, limit) : '—'
  return [
    {
      id: 'used',
      label: 'Used',
      value: used != null ? formatNumber(used) : '—',
      subtitle: limit != null ? `of ${formatNumber(limit)}` : '—',
    },
    {
      id: 'remaining',
      label: 'Remaining',
      value: remaining != null ? formatNumber(remaining) : '—',
      subtitle: percent,
    },
    {
      id: 'synced',
      label: 'Synced',
      value: detail?.quota_synced_at ? formatTimestamp(detail.quota_synced_at) : '—',
      subtitle: '',
    },
  ]
}

export function KeyQuotaPanel({
  detail,
  loadState,
  loadingLabel,
  syncNotice,
  formatNumber,
  formatTimestamp,
  formatPercent,
}: KeyQuotaPanelProps): JSX.Element {
  const admin = useTranslate().admin
  const keyDetailsStrings = admin.keyDetails
  const providerId = detail?.provider
  const kind = quotaDisplayKind(providerId)
  const providerCopy = admin.providers[parseProviderId(detail?.provider)]
  const shared = admin.providers.shared

  const cards = ((): MetricCard[] => {
    if (!detail) return []
    if (kind === 'searches_with_renewal') {
      const limit = detail.quota_limit ?? null
      const remaining = detail.quota_remaining ?? null
      const used = limit != null && remaining != null ? Math.max(limit - remaining, 0) : null
      return [
        {
          id: 'used',
          label: shared.quotaUsed,
          value: used != null ? formatNumber(used) : '—',
          subtitle: limit != null ? `/ ${formatNumber(limit)}` : '—',
        },
        {
          id: 'remaining',
          label: shared.quotaRemaining,
          value: remaining != null ? formatNumber(remaining) : '—',
          subtitle: limit && remaining != null && limit > 0 ? formatPercent(remaining, limit) : '—',
        },
        {
          id: 'renews',
          label: shared.quotaRenewalDate,
          value: detail.quota_renews_at ? formatTimestamp(detail.quota_renews_at) : '—',
          subtitle: '',
        },
        {
          id: 'synced',
          label: shared.quotaSynced,
          value: detail.quota_synced_at ? formatTimestamp(detail.quota_synced_at) : '—',
          subtitle: '',
        },
      ]
    }
    if (kind === 'harvested_remaining') {
      return [
        {
          id: 'remaining',
          label: shared.quotaRemaining,
          value: detail.quota_remaining != null ? formatNumber(detail.quota_remaining) : '—',
          subtitle: detail.quota_limit != null ? `/ ${formatNumber(detail.quota_limit)}` : '—',
        },
        {
          id: 'synced',
          label: shared.quotaSynced,
          value: detail.quota_synced_at ? formatTimestamp(detail.quota_synced_at) : '—',
          subtitle: '',
        },
      ]
    }
    return []
  })()

  const isTavily = kind === 'credits'

  return (
    <section className="surface panel">
      <div className="panel-header">
        <div>
          <h2>Quota</h2>
          <p className="panel-description">
            {isTavily ? 'Tavily Usage for this key' : providerCopy.quotaNote}
          </p>
        </div>
      </div>
      <AdminLoadingRegion loadState={loadState} loadingLabel={loadingLabel} minHeight={180}>
        <section className="metrics-grid">
          {!detail ? (
            <div className="empty-state alert" style={{ gridColumn: '1 / -1' }}>
              {keyDetailsStrings.loading}
            </div>
          ) : (
            (isTavily
              ? tavilyMetricCards(detail, formatNumber, formatTimestamp, formatPercent)
              : cards
            ).map((m) => (
              <div key={m.id} className="metric-card">
                <h3>{m.label}</h3>
                <div className="metric-value">{m.value}</div>
                <div className="metric-subtitle">{m.subtitle}</div>
              </div>
            ))
          )}
        </section>
        {syncNotice ? (
          <p className="panel-description" role="status" style={{ marginTop: 8 }}>
            {syncNotice}
          </p>
        ) : null}
      </AdminLoadingRegion>
    </section>
  )
}

export interface KeyUsageSyncButtonProps {
  provider?: unknown
  syncState: SyncState
  onSync: () => void
  variant?: ComponentProps<typeof Button>['variant']
  className?: string
}

/**
 * Sync-usage button, enabled only where `supportsSyncUsage(provider)` holds.
 * Exa renders disabled with a native tooltip explaining the missing upstream
 * balance API; Tavily/SerpApi/Brave keep the exact legacy button markup.
 */
export function KeyUsageSyncButton({
  provider,
  syncState,
  onSync,
  variant,
  className,
}: KeyUsageSyncButtonProps): JSX.Element {
  const admin = useTranslate().admin
  const keyDetailsStrings = admin.keyDetails
  const shared = admin.providers.shared
  const supported = supportsSyncUsage(provider)
  const disabled = syncState === 'syncing' || !supported
  const label =
    syncState === 'syncing'
      ? keyDetailsStrings.syncing
      : syncState === 'success'
        ? keyDetailsStrings.syncSuccess
        : keyDetailsStrings.syncAction
  return (
    <Button
      type="button"
      variant={variant ?? (syncState === 'success' ? 'success' : 'default')}
      className={className}
      onClick={onSync}
      disabled={disabled}
      aria-busy={syncState === 'syncing'}
      title={supported ? undefined : shared.syncUnsupportedHint}
    >
      <Icon
        icon={syncState === 'syncing' ? 'mdi:loading' : syncState === 'success' ? 'mdi:check-bold' : 'mdi:refresh'}
        width={18}
        height={18}
        className={syncState === 'syncing' ? 'icon-spin' : undefined}
      />
      {label}
    </Button>
  )
}

export type KeyUsageSyncOutcome =
  | { kind: 'no_content' }
  | { kind: 'snapshot'; source: 'live' | 'passive' }
  | { kind: 'unsupported' }

interface TypedSyncError extends Error {
  status?: number
  body?: { error?: string } | null
}

/**
 * Runs a single-key usage sync and normalizes the T10 arms: legacy Tavily 204,
 * a live/passive provider snapshot, or the typed Exa `sync_unsupported` 409.
 */
export async function performKeyUsageSync(id: string): Promise<KeyUsageSyncOutcome> {
  try {
    const outcome = await syncApiKeyUsage(id)
    if (outcome.kind === 'no_content') return { kind: 'no_content' }
    return { kind: 'snapshot', source: outcome.snapshot.source }
  } catch (err) {
    const typed = err as TypedSyncError
    if (typed.status === 409 && typed.body?.error === 'sync_unsupported') {
      return { kind: 'unsupported' }
    }
    throw err
  }
}

/** Localized sync failure copy, including the typed `sync_unsupported` 409. */
export function usageSyncErrorCopy(err: unknown, fallback: string, unsupportedCopy: string): string {
  const typed = err as TypedSyncError
  if (typed?.status === 409 && typed.body?.error === 'sync_unsupported') return unsupportedCopy
  return err instanceof Error && err.message ? err.message : fallback
}