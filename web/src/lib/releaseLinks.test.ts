import { describe, expect, it } from 'bun:test'

import { buildOctoRillReleaseLink, formatVersionDisplay, normalizeReleaseTag } from './releaseLinks'

describe('releaseLinks', () => {
  it('builds an OctoRill highlight link for stable releases', () => {
    expect(buildOctoRillReleaseLink('0.81.1')).toEqual({
      href: 'https://octo-rill.ivanli.cc/IvanLi-CN/tavily-hikari/releases?highlight=tag%3Av0.81.1&highlight_active=tag%3Av0.81.1',
      label: 'v0.81.1',
    })
  })

  it('keeps prerelease suffixes in the highlighted tag', () => {
    expect(buildOctoRillReleaseLink('0.81.1-rc.1')).toEqual({
      href: 'https://octo-rill.ivanli.cc/IvanLi-CN/tavily-hikari/releases?highlight=tag%3Av0.81.1-rc.1&highlight_active=tag%3Av0.81.1-rc.1',
      label: 'v0.81.1-rc.1',
    })
    expect(normalizeReleaseTag('v0.81.1-beta.2')).toBe('v0.81.1-beta.2')
  })

  it('rejects non-release channels that should stay plain text', () => {
    expect(buildOctoRillReleaseLink('0.81.1-dev')).toBeNull()
    expect(buildOctoRillReleaseLink('0.81.1-ci.3')).toBeNull()
    expect(buildOctoRillReleaseLink('passkey-local')).toBeNull()
  })

  it('formats release-looking versions with a v prefix but leaves opaque labels untouched', () => {
    expect(formatVersionDisplay('0.81.1')).toBe('v0.81.1')
    expect(formatVersionDisplay('0.81.1-rc.1')).toBe('v0.81.1-rc.1')
    expect(formatVersionDisplay('ci-deadbeef')).toBe('ci-deadbeef')
  })
})
