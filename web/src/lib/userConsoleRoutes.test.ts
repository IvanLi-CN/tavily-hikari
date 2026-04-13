import { describe, expect, it } from 'bun:test'

import {
  normalizeUserConsolePathname,
  parseUserConsolePath,
  userConsoleRouteToPath,
} from './userConsoleRoutes'

describe('userConsoleRoutes', () => {
  it('parses landing paths into shared landing sections', () => {
    expect(parseUserConsolePath('/console')).toEqual({ name: 'landing', section: null })
    expect(parseUserConsolePath('/console/dashboard')).toEqual({ name: 'landing', section: 'dashboard' })
    expect(parseUserConsolePath('/console/tokens')).toEqual({ name: 'landing', section: 'tokens' })
  })

  it('falls back to the default landing view for unknown path suffixes', () => {
    expect(parseUserConsolePath('/console/dashboard-copy')).toEqual({ name: 'landing', section: null })
    expect(parseUserConsolePath('/console/tokens-old')).toEqual({ name: 'landing', section: null })
  })

  it('normalizes trailing slashes and console.html variants before parsing', () => {
    expect(normalizeUserConsolePathname('/console/')).toBe('/console')
    expect(normalizeUserConsolePathname('/console/dashboard/')).toBe('/console/dashboard')
    expect(normalizeUserConsolePathname('/console/tokens/')).toBe('/console/tokens')
    expect(normalizeUserConsolePathname('/console.html')).toBe('/console')
    expect(normalizeUserConsolePathname('/console.html/tokens/')).toBe('/console/tokens')

    expect(parseUserConsolePath('/console/dashboard/')).toEqual({ name: 'landing', section: 'dashboard' })
    expect(parseUserConsolePath('/console/tokens/')).toEqual({ name: 'landing', section: 'tokens' })
    expect(parseUserConsolePath('/console.html/tokens')).toEqual({ name: 'landing', section: 'tokens' })
  })

  it('keeps token detail paths on the dedicated detail route', () => {
    expect(parseUserConsolePath('/console/tokens/a1b2')).toEqual({ name: 'token', id: 'a1b2' })
    expect(parseUserConsolePath('/console/tokens/a%2Fb')).toEqual({ name: 'token', id: 'a/b' })
    expect(parseUserConsolePath('/console.html/tokens/a%2Fb')).toEqual({ name: 'token', id: 'a/b' })
  })

  it('falls back to the token landing section when token detail decoding fails', () => {
    expect(parseUserConsolePath('/console/tokens/%E0%A4%A')).toEqual({ name: 'landing', section: 'tokens' })
  })

  it('serializes landing and token routes back to canonical paths', () => {
    expect(userConsoleRouteToPath({ name: 'landing', section: null })).toBe('/console')
    expect(userConsoleRouteToPath({ name: 'landing', section: 'dashboard' })).toBe('/console/dashboard')
    expect(userConsoleRouteToPath({ name: 'landing', section: 'tokens' })).toBe('/console/tokens')
    expect(userConsoleRouteToPath({ name: 'token', id: 'a/b' })).toBe('/console/tokens/a%2Fb')
  })
})
