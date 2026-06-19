import type { AdminUserRankingIdentity } from '../api/adminRankings'

const AVATAR_PALETTE = [
  ['#7c3aed', '#2563eb'],
  ['#0f766e', '#14b8a6'],
  ['#be185d', '#ec4899'],
  ['#1d4ed8', '#60a5fa'],
  ['#b45309', '#f59e0b'],
  ['#0f766e', '#34d399'],
  ['#7c2d12', '#fb7185'],
  ['#4338ca', '#818cf8'],
  ['#15803d', '#4ade80'],
  ['#9a3412', '#fb923c'],
] as const

function hashSeed(value: string): number {
  let hash = 0
  for (let index = 0; index < value.length; index += 1) {
    hash = (hash * 33 + value.charCodeAt(index)) >>> 0
  }
  return hash
}

function escapeSvgText(value: string): string {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&apos;')
}

function pickDisplayName(identity: AdminUserRankingIdentity, fallback: string): string {
  return identity.displayName?.trim() || identity.username?.trim() || identity.userId || fallback
}

export function normalizeRankingAvatarUrl(avatarUrl: string | null | undefined): string | null {
  const value = avatarUrl?.trim()
  return value ? value : null
}

export function buildRankingMockAvatarDataUrl(identity: AdminUserRankingIdentity, fallback: string): string {
  const displayName = pickDisplayName(identity, fallback)
  const seed = `${identity.userId}|${identity.username ?? ''}|${displayName}`
  const paletteIndex = hashSeed(seed) % AVATAR_PALETTE.length
  const [startColor, endColor] = AVATAR_PALETTE[paletteIndex] ?? AVATAR_PALETTE[0]
  const accentX = 64 + (hashSeed(`${seed}:accent-x`) % 14)
  const accentY = 18 + (hashSeed(`${seed}:accent-y`) % 12)
  const accentRadius = 10 + (hashSeed(`${seed}:accent-r`) % 6)
  const label = escapeSvgText(displayName)

  return `data:image/svg+xml;utf8,${encodeURIComponent(
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 96 96" role="img" aria-label="${label}">
      <defs>
        <linearGradient id="g" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stop-color="${startColor}" />
          <stop offset="100%" stop-color="${endColor}" />
        </linearGradient>
      </defs>
      <rect width="96" height="96" rx="48" fill="url(#g)" />
      <circle cx="${accentX}" cy="${accentY}" r="${accentRadius}" fill="rgba(255,255,255,0.16)" />
      <circle cx="48" cy="36" r="18" fill="rgba(255,255,255,0.90)" />
      <path d="M20 79c3-15 15-24 28-24s25 9 28 24" fill="rgba(255,255,255,0.88)" />
      <path d="M18 79c0-8 5-14 12-18 5 8 11 11 18 11s13-3 18-11c7 4 12 10 12 18" fill="rgba(255,255,255,0.22)" />
    </svg>`,
  )}`
}
