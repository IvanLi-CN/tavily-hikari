export type UserConsoleLandingSection = 'dashboard' | 'tokens'

export type UserConsoleRoute
  = | { name: 'landing'; section: UserConsoleLandingSection | null }
    | { name: 'token'; id: string }

export function normalizeUserConsolePathname(pathname: string): string {
  const trimmed = pathname.trim()
  if (!trimmed) return '/console'

  let normalizedPath = trimmed.startsWith('/') ? trimmed : `/${trimmed}`
  if (normalizedPath === '/console.html') {
    return '/console'
  }
  if (normalizedPath.startsWith('/console.html/')) {
    normalizedPath = `/console${normalizedPath.slice('/console.html'.length)}`
  }
  if (normalizedPath.length > 1) {
    normalizedPath = normalizedPath.replace(/\/+$|(?<!:)\/+(?=\?)/g, '')
  }
  if (normalizedPath.length > 1 && normalizedPath.endsWith('/')) {
    normalizedPath = normalizedPath.replace(/\/+$/, '') || '/'
  }
  return normalizedPath
}

export function parseUserConsolePath(pathname: string): UserConsoleRoute {
  const normalizedPath = normalizeUserConsolePathname(pathname)
  const tokenMatch = normalizedPath.match(/^\/console\/tokens\/([^/?#]+)$/)
  if (tokenMatch) {
    try {
      return { name: 'token', id: decodeURIComponent(tokenMatch[1]) }
    } catch {
      return { name: 'landing', section: 'tokens' }
    }
  }

  if (normalizedPath === '/console/tokens') {
    return { name: 'landing', section: 'tokens' }
  }
  if (normalizedPath === '/console/dashboard') {
    return { name: 'landing', section: 'dashboard' }
  }

  return { name: 'landing', section: null }
}

export function userConsoleRouteToPath(route: UserConsoleRoute): string {
  if (route.name === 'token') {
    return `/console/tokens/${encodeURIComponent(route.id)}`
  }
  if (route.section === 'tokens') {
    return '/console/tokens'
  }
  if (route.section === 'dashboard') {
    return '/console/dashboard'
  }
  return '/console'
}
