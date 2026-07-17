const OCTO_RILL_RELEASES_URL = 'https://octo-rill.ivanli.cc/IvanLi-CN/tavily-hikari/releases'
const RELEASE_VERSION_PATTERN = /^(?:v)?\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/
const BLOCKED_PRERELEASE_CHANNELS = new Set(['ci', 'dev', 'local'])

export interface ReleaseLink {
  href: string
  label: string
}

function extractPrereleaseChannel(tag: string): string | null {
  const match = tag.match(/^[v]?\d+\.\d+\.\d+-([0-9A-Za-z.-]+)$/)
  if (!match) return null

  const [channel = ''] = match[1].split('.', 1)
  return channel.toLowerCase() || null
}

export function formatVersionDisplay(version: string | null | undefined): string | null {
  const trimmed = version?.trim() ?? ''
  if (trimmed.length === 0) {
    return null
  }

  if (RELEASE_VERSION_PATTERN.test(trimmed) && !trimmed.startsWith('v')) {
    return `v${trimmed}`
  }

  return trimmed
}

export function normalizeReleaseTag(version: string | null | undefined): string | null {
  const displayVersion = formatVersionDisplay(version)
  if (!displayVersion || !RELEASE_VERSION_PATTERN.test(displayVersion)) {
    return null
  }

  const prereleaseChannel = extractPrereleaseChannel(displayVersion)
  if (prereleaseChannel && BLOCKED_PRERELEASE_CHANNELS.has(prereleaseChannel)) {
    return null
  }

  return displayVersion
}

export function buildOctoRillReleaseLink(version: string | null | undefined): ReleaseLink | null {
  const tag = normalizeReleaseTag(version)
  if (!tag) {
    return null
  }

  const selector = `tag:${tag}`
  const query = new URLSearchParams()
  query.append('highlight', selector)
  query.set('highlight_active', selector)

  return {
    href: `${OCTO_RILL_RELEASES_URL}?${query.toString()}`,
    label: tag,
  }
}
