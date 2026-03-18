export type TokenLogRequestKindProtocolGroup = 'api' | 'mcp'
export type TokenLogRequestKindBillingGroup = 'billable' | 'non_billable'
export type TokenLogRequestKindQuickBilling = 'all' | TokenLogRequestKindBillingGroup
export type TokenLogRequestKindQuickProtocol = 'all' | TokenLogRequestKindProtocolGroup

export interface TokenLogRequestKindOption {
  key: string
  label: string
  protocol_group: TokenLogRequestKindProtocolGroup | ''
  billing_group: TokenLogRequestKindBillingGroup | ''
}

export interface TokenLogsPagePathInput {
  tokenId: string
  page: number
  perPage: number
  sinceIso: string
  untilIso: string
  forceEmptyMatch?: boolean
  requestKinds: string[]
}

export interface TokenLogRequestKindLabelSource {
  request_kind_key: string
  request_kind_label: string
}

export interface TokenLogRequestKindQuickFilters {
  billing: TokenLogRequestKindQuickBilling
  protocol: TokenLogRequestKindQuickProtocol
}

export interface TokenLogRequestKindOptionsRefreshResolution {
  quickSelection: string[]
  effectiveSelection: string[]
  hasEmptyMatch: boolean
  selectionChanged: boolean
}

export const defaultTokenLogRequestKindQuickFilters: TokenLogRequestKindQuickFilters = {
  billing: 'all',
  protocol: 'all',
}

export const tokenLogRequestKindEmptySelectionKey = '__token_request_kind_empty_selection__'

export function uniqueSelectedRequestKinds(requestKinds: string[]): string[] {
  const seen = new Set<string>()
  const normalized: string[] = []
  for (const raw of requestKinds) {
    const value = raw.trim()
    if (!value || seen.has(value)) continue
    seen.add(value)
    normalized.push(value)
  }
  return normalized
}

export function mergeRequestKindOptionsByKey(
  current: Record<string, TokenLogRequestKindOption>,
  options: TokenLogRequestKindOption[],
): Record<string, TokenLogRequestKindOption> {
  const next = { ...current }
  for (const option of options) {
    const key = option.key.trim()
    if (key) next[key] = option
  }
  return next
}

export function buildVisibleRequestKindOptions(
  selected: string[],
  options: TokenLogRequestKindOption[],
  optionsByKey: Record<string, TokenLogRequestKindOption>,
): TokenLogRequestKindOption[] {
  const byKey = new Map(options.map((option) => [option.key, option]))
  for (const key of uniqueSelectedRequestKinds(selected)) {
    if (byKey.has(key)) continue
    byKey.set(
      key,
      optionsByKey[key] ?? {
        key,
        label: key,
        protocol_group: '',
        billing_group: '',
      },
    )
  }
  return Array.from(byKey.values()).sort((left, right) => left.label.localeCompare(right.label) || left.key.localeCompare(right.key))
}

export function toggleRequestKindSelection(selected: string[], nextKey: string): string[] {
  const key = nextKey.trim()
  if (!key) return uniqueSelectedRequestKinds(selected)
  const normalized = uniqueSelectedRequestKinds(selected)
  return normalized.includes(key)
    ? normalized.filter((value) => value !== key)
    : [...normalized, key]
}

export function summarizeSelectedRequestKinds(
  selected: string[],
  options: TokenLogRequestKindOption[],
  emptyLabel = 'All request types',
): string {
  const normalized = uniqueSelectedRequestKinds(selected)
  if (normalized.length === 0) return emptyLabel

  const labelsByKey = new Map(options.map((option) => [option.key, option.label]))
  const labels = normalized.map((key) => labelsByKey.get(key) ?? key)
  if (labels.length <= 2) {
    return labels.join(' + ')
  }
  return `${labels.length} selected`
}

export function hasActiveRequestKindQuickFilters(filters: TokenLogRequestKindQuickFilters): boolean {
  return filters.billing !== 'all' || filters.protocol !== 'all'
}

export function buildRequestKindQuickFilterSelection(
  options: TokenLogRequestKindOption[],
  filters: TokenLogRequestKindQuickFilters,
): string[] {
  if (!hasActiveRequestKindQuickFilters(filters)) return []
  return uniqueSelectedRequestKinds(
    options
      .filter((option) => {
        const billingMatches = filters.billing === 'all' || option.billing_group === filters.billing
        const protocolMatches = filters.protocol === 'all' || option.protocol_group === filters.protocol
        return billingMatches && protocolMatches
      })
      .map((option) => option.key),
  )
}

export function requestKindSelectionsMatch(left: string[], right: string[]): boolean {
  const normalizedLeft = uniqueSelectedRequestKinds(left)
  const normalizedRight = uniqueSelectedRequestKinds(right)
  if (normalizedLeft.length !== normalizedRight.length) return false
  const rightSet = new Set(normalizedRight)
  return normalizedLeft.every((key) => rightSet.has(key))
}

export function resolveManualRequestKindQuickFilters(
  nextSelected: string[],
  activeFilters: TokenLogRequestKindQuickFilters,
  activeQuickSelection: string[],
  options: TokenLogRequestKindOption[],
): TokenLogRequestKindQuickFilters {
  return requestKindSelectionsMatch(nextSelected, activeQuickSelection)
    ? activeFilters
    : deriveRequestKindQuickFilters(nextSelected, options)
}

export function resolveEffectiveRequestKindSelection(
  selected: string[],
  activeFilters: TokenLogRequestKindQuickFilters,
  quickSelection: string[],
): string[] {
  return hasActiveRequestKindQuickFilters(activeFilters)
    ? uniqueSelectedRequestKinds(quickSelection)
    : uniqueSelectedRequestKinds(selected)
}

export function resolveRequestKindOptionsRefresh(
  options: TokenLogRequestKindOption[],
  selected: string[],
  activeFilters: TokenLogRequestKindQuickFilters,
  currentEffectiveSelection: string[],
  currentHasEmptyMatch: boolean,
): TokenLogRequestKindOptionsRefreshResolution {
  const quickSelection = buildRequestKindQuickFilterSelection(options, activeFilters)
  const hasEmptyMatch = hasActiveRequestKindQuickFilters(activeFilters) && quickSelection.length === 0
  const effectiveSelection = resolveEffectiveRequestKindSelection(selected, activeFilters, quickSelection)
  return {
    quickSelection,
    effectiveSelection,
    hasEmptyMatch,
    selectionChanged:
      !requestKindSelectionsMatch(effectiveSelection, currentEffectiveSelection) ||
      hasEmptyMatch !== currentHasEmptyMatch,
  }
}

export function deriveRequestKindQuickFilters(
  selected: string[],
  options: TokenLogRequestKindOption[],
): TokenLogRequestKindQuickFilters {
  const normalized = uniqueSelectedRequestKinds(selected)
  if (normalized.length === 0) return defaultTokenLogRequestKindQuickFilters

  const candidates: TokenLogRequestKindQuickFilters[] = [
    { billing: 'billable', protocol: 'api' },
    { billing: 'billable', protocol: 'mcp' },
    { billing: 'non_billable', protocol: 'api' },
    { billing: 'non_billable', protocol: 'mcp' },
    { billing: 'billable', protocol: 'all' },
    { billing: 'non_billable', protocol: 'all' },
    { billing: 'all', protocol: 'api' },
    { billing: 'all', protocol: 'mcp' },
  ]
  const matches = candidates.filter((candidate) =>
    requestKindSelectionsMatch(normalized, buildRequestKindQuickFilterSelection(options, candidate)),
  )
  return matches.length === 1 ? matches[0] : defaultTokenLogRequestKindQuickFilters
}

export function summarizeRequestKindQuickFilters(filters: TokenLogRequestKindQuickFilters): string {
  if (!hasActiveRequestKindQuickFilters(filters)) return 'All request types'

  const billingLabel =
    filters.billing === 'all' ? 'All' : filters.billing === 'billable' ? 'Paid' : 'Free'
  const protocolLabel =
    filters.protocol === 'all' ? 'Request types' : filters.protocol === 'api' ? 'API' : 'MCP'

  if (filters.billing === 'all') return `${protocolLabel} request types`
  if (filters.protocol === 'all') return `${billingLabel} request types`
  return `${billingLabel} + ${protocolLabel}`
}

export function buildTokenLogsPagePath({
  tokenId,
  page,
  perPage,
  sinceIso,
  untilIso,
  forceEmptyMatch = false,
  requestKinds,
}: TokenLogsPagePathInput): string {
  const search = new URLSearchParams({
    page: String(page),
    per_page: String(perPage),
    since: sinceIso,
    until: untilIso,
  })
  const normalizedRequestKinds = uniqueSelectedRequestKinds(requestKinds)
  const queryRequestKinds =
    normalizedRequestKinds.length === 0 && forceEmptyMatch
      ? [tokenLogRequestKindEmptySelectionKey]
      : normalizedRequestKinds
  for (const key of queryRequestKinds) {
    search.append('request_kind', key)
  }
  return `/api/tokens/${encodeURIComponent(tokenId)}/logs/page?${search.toString()}`
}
