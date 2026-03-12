import { describe, expect, it } from 'bun:test'

import { parseUserConsoleHash, userConsoleRouteToHash } from './userConsoleRoutes'

describe('userConsoleRoutes', () => {
  it('parses landing hashes into shared landing sections', () => {
    expect(parseUserConsoleHash('')).toEqual({ name: 'landing', section: null })
    expect(parseUserConsoleHash('#/dashboard')).toEqual({ name: 'landing', section: 'dashboard' })
    expect(parseUserConsoleHash('#/tokens')).toEqual({ name: 'landing', section: 'tokens' })
  })

  it('falls back to the default landing view for unknown hash suffixes', () => {
    expect(parseUserConsoleHash('#/dashboard-copy')).toEqual({ name: 'landing', section: null })
    expect(parseUserConsoleHash('#/tokens-old')).toEqual({ name: 'landing', section: null })
  })

  it('keeps legacy landing hash variants pinned to their sections', () => {
    expect(parseUserConsoleHash('#/dashboard/')).toEqual({ name: 'landing', section: 'dashboard' })
    expect(parseUserConsoleHash('#/dashboard?from=history')).toEqual({ name: 'landing', section: 'dashboard' })
    expect(parseUserConsoleHash('#/tokens/')).toEqual({ name: 'landing', section: 'tokens' })
    expect(parseUserConsoleHash('#/tokens?from=history')).toEqual({ name: 'landing', section: 'tokens' })
  })

  it('keeps token detail hashes on the dedicated detail route', () => {
    expect(parseUserConsoleHash('#/tokens/a1b2')).toEqual({ name: 'token', id: 'a1b2' })
    expect(parseUserConsoleHash('#/tokens/a%2Fb')).toEqual({ name: 'token', id: 'a/b' })
  })

  it('falls back to the token landing section when token detail decoding fails', () => {
    expect(parseUserConsoleHash('#/tokens/%E0%A4%A')).toEqual({ name: 'landing', section: 'tokens' })
  })

  it('serializes landing and token routes back to hashes', () => {
    expect(userConsoleRouteToHash({ name: 'landing', section: null })).toBe('')
    expect(userConsoleRouteToHash({ name: 'landing', section: 'dashboard' })).toBe('#/dashboard')
    expect(userConsoleRouteToHash({ name: 'landing', section: 'tokens' })).toBe('#/tokens')
    expect(userConsoleRouteToHash({ name: 'token', id: 'a/b' })).toBe('#/tokens/a%2Fb')
  })
})
