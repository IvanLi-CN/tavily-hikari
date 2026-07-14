export type UserConsoleLandingSection = 'dashboard' | 'tokens'

export type UserConsoleRoute
  = | { name: 'landing'; section: UserConsoleLandingSection | null }
    | { name: 'billing' }
    | { name: 'setup' }
    | { name: 'oauthCallback'; provider: string }
    | { name: 'token'; id: string }
    | { name: 'tokenLogs'; id: string }

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
  const oauthCallbackMatch = normalizedPath.match(/^\/console\/oauth\/([^/?#]+)\/callback$/)
  if (oauthCallbackMatch) {
    try {
      return { name: 'oauthCallback', provider: decodeURIComponent(oauthCallbackMatch[1]) }
    } catch {
      return { name: 'landing', section: null }
    }
  }

  const tokenLogsMatch = normalizedPath.match(/^\/console\/tokens\/([^/?#]+)\/logs$/)
  if (tokenLogsMatch) {
    try {
      return { name: 'tokenLogs', id: decodeURIComponent(tokenLogsMatch[1]) }
    } catch {
      return { name: 'landing', section: 'tokens' }
    }
  }

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
  if (normalizedPath === '/console/billing') {
    return { name: 'billing' }
  }
  if (normalizedPath === '/console/setup') {
    return { name: 'setup' }
  }
  if (normalizedPath === '/console/dashboard') {
    return { name: 'landing', section: 'dashboard' }
  }

  return { name: 'landing', section: null }
}

export function userConsoleRouteToPath(route: UserConsoleRoute): string {
  if (route.name === 'billing') {
    return '/console/billing'
  }
  if (route.name === 'setup') {
    return '/console/setup'
  }
  if (route.name === 'oauthCallback') {
    return `/console/oauth/${encodeURIComponent(route.provider)}/callback`
  }
  if (route.name === 'token' || route.name === 'tokenLogs') {
    const suffix = route.name === 'tokenLogs' ? '/logs' : ''
    return `/console/tokens/${encodeURIComponent(route.id)}${suffix}`
  }
  if (route.section === 'tokens') {
    return '/console/tokens'
  }
  if (route.section === 'dashboard') {
    return '/console/dashboard'
  }
  return '/console'
}
