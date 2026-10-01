/**
 * Admin key-table provider column, filter, and add/batch provider select
 * (plan todo 22).
 *
 * All substantial provider-column logic lives here so
 * `AdminDashboardRuntime.tsx` only carries minimal wiring. Text badges are
 * always rendered as visible labels (never color alone — PRODUCT.md:33), and
 * the filter/select controls use the existing clay utility vocabulary with
 * 44px hit targets.
 */
import { PROVIDER_IDS, providerLabel, type ProviderId } from '../api'
import { fetchApiKeys, requestJson, type PaginatedApiKeys } from '../api'
import { useTranslate } from '../i18n'
import { Badge } from '../components/ui/badge'

/** Sentinel for "no provider filter" in the table filter row. */
export const PROVIDER_FILTER_ALL = 'all'

export type ProviderFilterValue = ProviderId | typeof PROVIDER_FILTER_ALL

const PROVIDER_BADGE_VARIANTS: Record<ProviderId, 'neutral' | 'info' | 'secondary' | 'warning'> = {
  tavily: 'neutral',
  serpapi: 'info',
  exa: 'secondary',
  brave: 'warning',
}

function providerDataId(value: unknown): ProviderId | 'unknown' {
  if (typeof value !== 'string') return 'tavily'
  const normalized = value.trim().toLowerCase()
  return (PROVIDER_IDS as readonly string[]).includes(normalized)
    ? (normalized as ProviderId)
    : 'unknown'
}

/**
 * Provider text badge. Legacy `NULL`/absent providers render `Tavily`; an
 * unknown wire string is echoed verbatim (never mislabeled, never a crash).
 */
export function KeyProviderBadge({ provider }: { provider?: unknown }): JSX.Element {
  const dataId = providerDataId(provider)
  const label = providerLabel(provider)
  const variant = dataId === 'unknown' ? 'neutral' : PROVIDER_BADGE_VARIANTS[dataId]
  return (
    <Badge
      variant={variant}
      className="key-provider-badge whitespace-nowrap"
      data-provider={dataId}
      aria-label={`Provider: ${label}`}
    >
      {label}
    </Badge>
  )
}

const selectClassName = 'h-11 rounded-xl border border-base-300 bg-base-100 px-3 text-sm'

/** Table filter row provider select (all providers + the stable provider order). */
export function KeyProviderFilter({
  value,
  onChange,
  disabled,
}: {
  value: ProviderFilterValue
  onChange: (value: ProviderFilterValue) => void
  disabled?: boolean
}): JSX.Element {
  const providers = useTranslate().admin.providers.shared
  return (
    <select
      className={selectClassName}
      style={{ minWidth: 148 }}
      value={value}
      disabled={disabled}
      aria-label={providers.filterLabel}
      onChange={(event) => onChange(event.target.value as ProviderFilterValue)}
    >
      <option value={PROVIDER_FILTER_ALL}>{providers.filterAll}</option>
      {PROVIDER_IDS.map((id) => (
        <option key={id} value={id}>
          {providerLabel(id)}
        </option>
      ))}
    </select>
  )
}

/**
 * Add/batch provider select. One provider applies to the whole batch; the hint
 * documents that contract per `docs/admin-api-keys-batch-input.md` conventions.
 */
export function KeyProviderSelect({
  value,
  onChange,
  disabled,
}: {
  value: ProviderId
  onChange: (value: ProviderId) => void
  disabled?: boolean
}): JSX.Element {
  const providers = useTranslate().admin.providers.shared
  return (
    <span className="key-provider-select inline-flex flex-col gap-1" style={{ minWidth: 160 }}>
      <select
        className={selectClassName}
        value={value}
        disabled={disabled}
        aria-label={providers.selectLabel}
        onChange={(event) => onChange(event.target.value as ProviderId)}
      >
        {PROVIDER_IDS.map((id) => (
          <option key={id} value={id}>
            {providerLabel(id)}
          </option>
        ))}
      </select>
      <span className="text-xs opacity-70">{providers.selectBatchHint}</span>
    </span>
  )
}

export interface AdminKeysQueryOptions {
  groups?: string[]
  statuses?: string[]
  registrationIp?: string | null
  regions?: string[]
  provider?: ProviderFilterValue
}

/**
 * Provider-aware `/api/keys` page fetch. When no provider filter is active
 * this delegates to the untouched `fetchApiKeys` (legacy byte-identical
 * request). A selected provider is appended as `?provider=` — the backend
 * filter added by plan todo 10 — so pagination totals stay server-authoritative.
 */
export function fetchAdminKeysPage(
  page = 1,
  perPage = 20,
  options?: AdminKeysQueryOptions,
  signal?: AbortSignal,
): Promise<PaginatedApiKeys> {
  const provider = options?.provider
  if (!provider || provider === PROVIDER_FILTER_ALL) {
    return fetchApiKeys(
      page,
      perPage,
      {
        groups: options?.groups,
        statuses: options?.statuses,
        registrationIp: options?.registrationIp,
        regions: options?.regions,
      },
      signal,
    )
  }
  const params = new URLSearchParams({ page: String(page), per_page: String(perPage) })
  for (const group of options?.groups ?? []) {
    const normalized = group.trim()
    if (normalized) params.append('group', normalized)
  }
  for (const status of options?.statuses ?? []) {
    const normalized = status.trim().toLowerCase()
    if (normalized) params.append('status', normalized)
  }
  const normalizedRegistrationIp = options?.registrationIp?.trim()
  if (normalizedRegistrationIp) params.set('registration_ip', normalizedRegistrationIp)
  for (const region of options?.regions ?? []) {
    const normalized = region.trim()
    if (normalized) params.append('region', normalized)
  }
  params.set('provider', provider)
  return requestJson<PaginatedApiKeys>(`/api/keys?${params.toString()}`, { signal })
}