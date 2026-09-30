import type { LogFacetOption } from '../api'
import {
  PROVIDER_IDS,
  normalizePayloadProvider,
  parseProviderId,
  providerLabel,
  type ProviderId,
} from '../api'

/**
 * Plan todo 23 provider dimension for request-log surfaces.
 *
 * The wire contract is committed in T20/T21: `RequestLog.provider` (coalesced
 * for legacy rows), `RequestLogFacets.providers` (only on `/api/logs`), and the
 * `?provider=` query param emitted by `appendRequestLogsPageFilters`. This
 * module is the single presentation-layer join between rows, facets, and the
 * filter UI so the panel never re-implements the legacy NULL -> Tavily rule.
 */

/** Sentinel used by the panel segmented filter for "all providers". */
export const logProviderAllFilterValue = '__all__'

/** Minimal row shape shared by `RequestLog` and public token-log rows. */
export interface ProviderBearingRow {
  provider?: string | null
}

/** Legacy/absent/unknown payload providers coalesce to the typed Tavily id. */
export function resolveLogProvider(
  value: ProviderBearingRow | string | null | undefined,
): ProviderId {
  if (typeof value === 'string') return parseProviderId(value)
  return normalizePayloadProvider(value)
}

/** Brand label for known providers; unknown raw strings stay visible as-is. */
export function logProviderDisplayLabel(value: unknown): string {
  return providerLabel(value)
}

function providerOrderIndex(provider: string): number {
  const index = PROVIDER_IDS.indexOf(provider as ProviderId)
  return index === -1 ? PROVIDER_IDS.length : index
}

function sortProviderFacetOptions(left: LogFacetOption, right: LogFacetOption): number {
  const orderDelta = providerOrderIndex(left.value) - providerOrderIndex(right.value)
  return orderDelta !== 0 ? orderDelta : left.value.localeCompare(right.value)
}

/** Facet options derived from the rows currently on screen (fallback path). */
export function providerFacetOptionsFromLogs(
  logs: ReadonlyArray<ProviderBearingRow>,
): LogFacetOption[] {
  const counts = new Map<string, number>()
  for (const row of logs) {
    const provider = resolveLogProvider(row)
    counts.set(provider, (counts.get(provider) ?? 0) + 1)
  }
  return Array.from(counts, ([value, count]) => ({ value, count })).sort(sortProviderFacetOptions)
}

/**
 * Merge the todo-20 facet list with row-derived providers. Facet counts are
 * whole-window authoritative; a provider that only appears in the current page
 * (stale facet payload) is still selectable via its row count.
 */
export function mergeProviderFacetOptions(
  facets: ReadonlyArray<LogFacetOption> | undefined,
  logs: ReadonlyArray<ProviderBearingRow>,
): LogFacetOption[] {
  const counts = new Map<string, number>()
  const facetProviders = new Set<string>()
  for (const option of facets ?? []) {
    const raw = option.value?.trim()
    if (!raw) continue
    const provider = resolveLogProvider({ provider: raw })
    if (!facetProviders.has(provider)) {
      facetProviders.add(provider)
      counts.set(provider, option.count)
    }
  }
  for (const row of logs) {
    const provider = resolveLogProvider(row)
    if (facetProviders.has(provider)) continue
    counts.set(provider, (counts.get(provider) ?? 0) + 1)
  }
  return Array.from(counts, ([value, count]) => ({ value, count })).sort(sortProviderFacetOptions)
}

/**
 * Narrow rows to one provider. `null`/empty keeps every row; the legacy
 * NULL-provider row matches `tavily` exactly like the backend filter does.
 */
export function filterLogsByProvider<T extends ProviderBearingRow>(
  logs: ReadonlyArray<T>,
  provider: string | null | undefined,
): T[] {
  const raw = typeof provider === 'string' ? provider.trim() : ''
  if (!raw || raw === logProviderAllFilterValue) return [...logs]
  const target = resolveLogProvider({ provider: raw })
  return logs.filter((row) => resolveLogProvider(row) === target)
}

/**
 * The `?provider=` value T21 emits through `appendRequestLogsPageFilters`.
 * `undefined` means "no provider filter" (all rows, including legacy NULL).
 */
export function providerFilterQueryValue(provider: string | null | undefined): string | undefined {
  const raw = typeof provider === 'string' ? provider.trim() : ''
  if (!raw || raw === logProviderAllFilterValue) return undefined
  return raw
}