type VersionOverrideGlobal = typeof globalThis & {
  __TAVILY_HIKARI_APP_VERSION_OVERRIDE__?: string
}

export function getBundledFrontendVersion(): string | null {
  const override = (globalThis as VersionOverrideGlobal).__TAVILY_HIKARI_APP_VERSION_OVERRIDE__
  if (typeof override === 'string') {
    const trimmed = override.trim()
    return trimmed.length > 0 ? trimmed : null
  }

  const version = import.meta.env?.VITE_APP_VERSION
  if (typeof version !== 'string') return null

  const trimmed = version.trim()
  return trimmed.length > 0 ? trimmed : null
}
