export type AdminModuleId =
  | 'dashboard'
  | 'tokens'
  | 'keys'
  | 'requests'
  | 'jobs'
  | 'users'
  | 'alerts'
  | 'proxy-settings'

export type AdminPathRoute =
  | { name: 'module'; module: AdminModuleId }
  | { name: 'token'; id: string }
  | { name: 'token-usage' }
  | { name: 'user'; id: string }
  | { name: 'user-tags' }
  | { name: 'user-tag-editor'; mode: 'create' }
  | { name: 'user-tag-editor'; mode: 'edit'; id: string }
  | { name: 'key'; id: string }

const ADMIN_BASE = '/admin'

function normalize(pathname: string): string {
  if (!pathname) return ADMIN_BASE
  const trimmed = pathname.endsWith('/') && pathname !== '/' ? pathname.slice(0, -1) : pathname
  return trimmed || ADMIN_BASE
}

function decodeSegment(raw: string): string | null {
  if (!raw || raw.includes('/')) return null
  try {
    return decodeURIComponent(raw)
  } catch {
    return null
  }
}

export function parseAdminPath(pathname: string): AdminPathRoute {
  const path = normalize(pathname)

  if (path === ADMIN_BASE || path === `${ADMIN_BASE}/dashboard`) {
    return { name: 'module', module: 'dashboard' }
  }
  if (path === `${ADMIN_BASE}/tokens`) {
    return { name: 'module', module: 'tokens' }
  }
  if (path === `${ADMIN_BASE}/tokens/leaderboard`) {
    return { name: 'token-usage' }
  }
  if (path.startsWith(`${ADMIN_BASE}/tokens/`)) {
    const id = decodeSegment(path.slice(`${ADMIN_BASE}/tokens/`.length))
    if (id) return { name: 'token', id }
    return { name: 'module', module: 'tokens' }
  }
  if (path === `${ADMIN_BASE}/keys`) {
    return { name: 'module', module: 'keys' }
  }
  if (path.startsWith(`${ADMIN_BASE}/keys/`)) {
    const id = decodeSegment(path.slice(`${ADMIN_BASE}/keys/`.length))
    if (id) return { name: 'key', id }
    return { name: 'module', module: 'keys' }
  }
  if (path === `${ADMIN_BASE}/requests`) {
    return { name: 'module', module: 'requests' }
  }
  if (path === `${ADMIN_BASE}/jobs`) {
    return { name: 'module', module: 'jobs' }
  }
  if (path === `${ADMIN_BASE}/users`) {
    return { name: 'module', module: 'users' }
  }
  if (path === `${ADMIN_BASE}/users/tags`) {
    return { name: 'user-tags' }
  }
  if (path === `${ADMIN_BASE}/users/tags/new`) {
    return { name: 'user-tag-editor', mode: 'create' }
  }
  if (path.startsWith(`${ADMIN_BASE}/users/tags/`)) {
    const id = decodeSegment(path.slice(`${ADMIN_BASE}/users/tags/`.length))
    if (id) return { name: 'user-tag-editor', mode: 'edit', id }
    return { name: 'user-tags' }
  }
  if (path.startsWith(`${ADMIN_BASE}/users/`)) {
    const id = decodeSegment(path.slice(`${ADMIN_BASE}/users/`.length))
    if (id) return { name: 'user', id }
    return { name: 'module', module: 'users' }
  }
  if (path === `${ADMIN_BASE}/alerts`) {
    return { name: 'module', module: 'alerts' }
  }
  if (path === `${ADMIN_BASE}/proxy-settings`) {
    return { name: 'module', module: 'proxy-settings' }
  }

  return { name: 'module', module: 'dashboard' }
}

export function isSameAdminRoute(left: AdminPathRoute, right: AdminPathRoute): boolean {
  if (left.name !== right.name) return false
  if (left.name === 'module' && right.name === 'module') {
    return left.module === right.module
  }
  if (left.name === 'token' && right.name === 'token') {
    return left.id === right.id
  }
  if (left.name === 'user' && right.name === 'user') {
    return left.id === right.id
  }
  if (left.name === 'user-tags' && right.name === 'user-tags') {
    return true
  }
  if (left.name === 'user-tag-editor' && right.name === 'user-tag-editor') {
    if (left.mode === 'create' && right.mode === 'create') return true
    if (left.mode === 'edit' && right.mode === 'edit') return left.id === right.id
    return false
  }
  if (left.name === 'key' && right.name === 'key') {
    return left.id === right.id
  }
  return left.name === 'token-usage' && right.name === 'token-usage'
}

export function modulePath(module: AdminModuleId): string {
  if (module === 'dashboard') return `${ADMIN_BASE}/dashboard`
  return `${ADMIN_BASE}/${module}`
}

export function tokenDetailPath(id: string): string {
  return `${ADMIN_BASE}/tokens/${encodeURIComponent(id)}`
}

export function tokenLeaderboardPath(): string {
  return `${ADMIN_BASE}/tokens/leaderboard`
}

export function userDetailPath(id: string, query?: string, tagId?: string | null): string {
  const path = `${ADMIN_BASE}/users/${encodeURIComponent(id)}`
  const params = new URLSearchParams()
  const normalizedQuery = query?.trim()
  const normalizedTagId = tagId?.trim()
  if (normalizedQuery) params.set('q', normalizedQuery)
  if (normalizedTagId) params.set('tagId', normalizedTagId)
  const search = params.toString()
  return search ? `${path}?${search}` : path
}

export function userTagsPath(): string {
  return `${ADMIN_BASE}/users/tags`
}

export function userTagCreatePath(): string {
  return `${ADMIN_BASE}/users/tags/new`
}

export function userTagEditPath(id: string): string {
  return `${ADMIN_BASE}/users/tags/${encodeURIComponent(id)}`
}

export function keyDetailPath(id: string): string {
  return `${ADMIN_BASE}/keys/${encodeURIComponent(id)}`
}
