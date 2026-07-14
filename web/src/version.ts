type VersionOverrideGlobal = typeof globalThis & {
  __TAVILY_HIKARI_APP_VERSION_OVERRIDE__?: string
}

export function getBundledFrontendVersion(): string | null {
  const override = (globalThis as VersionOverrideGlobal).__TAVILY_HIKARI_APP_VERSION_OVERRIDE__
  if (typeof override === 'string') {
    const trimmed = override.trim()
    return trimmed.length > 0 ? trimmed : null
  }

  if (typeof __TAVILY_HIKARI_APP_VERSION__ !== 'undefined') {
    const trimmed = __TAVILY_HIKARI_APP_VERSION__.trim()
    return trimmed.length > 0 ? trimmed : null
  }

  return null
}
