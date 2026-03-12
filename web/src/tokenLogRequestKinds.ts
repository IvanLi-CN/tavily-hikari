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
