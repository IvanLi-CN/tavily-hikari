type VersionOverrideGlobal = typeof globalThis & {
  __TAVILY_HIKARI_APP_VERSION_OVERRIDE__?: string
}

const BUILD_VERSION_PLACEHOLDER = '__TAVILY_HIKARI_BUILD_VERSION__'

export function getBundledFrontendVersion(): string | null {
  const override = (globalThis as VersionOverrideGlobal).__TAVILY_HIKARI_APP_VERSION_OVERRIDE__
  if (typeof override === 'string') {
    const trimmed = override.trim()
    return trimmed.length > 0 ? trimmed : null
  }

  if (typeof document === 'undefined') return null

  const marker = document.querySelector('meta[name="tavily-hikari-build-version"]')
  const value = marker?.getAttribute('content')
  if (typeof value !== 'string') return null

  const trimmed = value.trim()
  if (trimmed.length === 0 || trimmed === BUILD_VERSION_PLACEHOLDER) return null

  return trimmed
}
