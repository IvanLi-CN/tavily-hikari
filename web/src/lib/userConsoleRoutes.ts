export type UserConsoleLandingSection = 'dashboard' | 'tokens'

export type UserConsoleRoute
  = | { name: 'landing'; section: UserConsoleLandingSection | null }
    | { name: 'token'; id: string }

export function parseUserConsoleHash(hash: string): UserConsoleRoute {
  const normalizedHash = hash.trim()
  const tokenMatch = normalizedHash.match(/^#\/tokens\/([^/?#]+)/)
  if (tokenMatch) {
    try {
      return { name: 'token', id: decodeURIComponent(tokenMatch[1]) }
    } catch {
      return { name: 'landing', section: 'tokens' }
    }
  }

  if (/^#\/tokens(?:$|[/?].*)/.test(normalizedHash)) {
    return { name: 'landing', section: 'tokens' }
  }
  if (/^#\/dashboard(?:$|[/?].*)/.test(normalizedHash)) {
    return { name: 'landing', section: 'dashboard' }
  }

  return { name: 'landing', section: null }
}

export function userConsoleRouteToHash(route: UserConsoleRoute): string {
  if (route.name === 'token') {
    return `#/tokens/${encodeURIComponent(route.id)}`
  }
  if (route.section === 'tokens') {
    return '#/tokens'
  }
  if (route.section === 'dashboard') {
    return '#/dashboard'
  }
  return ''
}
