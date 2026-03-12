export interface TokenLogRequestKindOption {
  key: string
  label: string
}

export interface TokenLogsPagePathInput {
  tokenId: string
  page: number
  perPage: number
  sinceIso: string
  untilIso: string
  requestKinds: string[]
}

export interface TokenLogRequestKindLabelSource {
  request_kind_key: string
  request_kind_label: string
}

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

export function mergeRequestKindLabels(
  current: Record<string, string>,
  options: TokenLogRequestKindOption[],
  logs: TokenLogRequestKindLabelSource[] = [],
): Record<string, string> {
  const next = { ...current }
  for (const option of options) {
    const key = option.key.trim()
    const label = option.label.trim()
    if (key && label) next[key] = label
  }
  for (const log of logs) {
    const key = log.request_kind_key.trim()
    const label = log.request_kind_label.trim()
    if (key && label) next[key] = label
  }
  return next
}

export function buildVisibleRequestKindOptions(
  selected: string[],
  options: TokenLogRequestKindOption[],
  labelsByKey: Record<string, string>,
): TokenLogRequestKindOption[] {
  const byKey = new Map(options.map((option) => [option.key, option]))
  for (const key of uniqueSelectedRequestKinds(selected)) {
    if (byKey.has(key)) continue
    byKey.set(key, { key, label: labelsByKey[key] ?? key })
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

export function buildTokenLogsPagePath({
  tokenId,
  page,
  perPage,
  sinceIso,
  untilIso,
  requestKinds,
}: TokenLogsPagePathInput): string {
  const search = new URLSearchParams({
    page: String(page),
    per_page: String(perPage),
    since: sinceIso,
    until: untilIso,
  })
  for (const key of uniqueSelectedRequestKinds(requestKinds)) {
    search.append('request_kind', key)
  }
  return `/api/tokens/${encodeURIComponent(tokenId)}/logs/page?${search.toString()}`
}
