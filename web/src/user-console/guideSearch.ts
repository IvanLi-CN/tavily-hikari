import type { GuideKey } from './runtime'

const GUIDE_QUERY_PARAM = 'guide'

export const DEFAULT_GUIDE_KEY: GuideKey = 'codex'
export const GUIDE_KEY_ORDER: readonly GuideKey[] = [
  'codex',
  'hikariCli',
  'claude',
  'vscode',
  'claudeDesktop',
  'cursor',
  'windsurf',
  'cherryStudio',
  'other',
]

export function resolveSetupGuide(search: string): GuideKey {
  const requestedGuide = new URLSearchParams(search).get(GUIDE_QUERY_PARAM)
  if (requestedGuide && GUIDE_KEY_ORDER.includes(requestedGuide as GuideKey)) {
    return requestedGuide as GuideKey
  }
  return DEFAULT_GUIDE_KEY
}

export function buildSetupGuideSearch(search: string, tokenId: string | null, guide: GuideKey): string {
  const params = new URLSearchParams(search)
  if (tokenId) {
    params.set('token', tokenId)
  } else {
    params.delete('token')
  }
  params.set(GUIDE_QUERY_PARAM, guide)
  const nextSearch = params.toString()
  return nextSearch ? `?${nextSearch}` : ''
}
