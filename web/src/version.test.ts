import '../test/happydom'

import { afterEach, describe, expect, it } from 'bun:test'

import { getBundledFrontendVersion } from './version'

type VersionOverrideGlobal = typeof globalThis & {
  __TAVILY_HIKARI_APP_VERSION_OVERRIDE__?: string
}

describe('getBundledFrontendVersion', () => {
  afterEach(() => {
    document.head.innerHTML = ''
    delete (globalThis as VersionOverrideGlobal).__TAVILY_HIKARI_APP_VERSION_OVERRIDE__
  })

  it('prefers the test override over the compiled application version', () => {
    document.head.innerHTML = '<meta name="tavily-hikari-build-version" content="html-version" />'
    ;(globalThis as VersionOverrideGlobal).__TAVILY_HIKARI_APP_VERSION_OVERRIDE__ = ' override-version '

    expect(getBundledFrontendVersion()).toBe('override-version')
  })

  it('does not read a version from HTML metadata', () => {
    document.head.innerHTML = '<meta name="tavily-hikari-build-version" content=" html-version " />'

    expect(getBundledFrontendVersion()).toBe(process.env.VITE_APP_VERSION?.trim() || null)
  })
})
