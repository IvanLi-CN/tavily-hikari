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

  it('prefers the test override over the document marker', () => {
    document.head.innerHTML = '<meta name="tavily-hikari-build-version" content="html-version" />'
    ;(globalThis as VersionOverrideGlobal).__TAVILY_HIKARI_APP_VERSION_OVERRIDE__ = ' override-version '

    expect(getBundledFrontendVersion()).toBe('override-version')
  })

  it('reads the current page version from the build marker', () => {
    document.head.innerHTML = '<meta name="tavily-hikari-build-version" content=" html-version " />'

    expect(getBundledFrontendVersion()).toBe('html-version')
  })

  it('does not expose the source placeholder as a release version', () => {
    document.head.innerHTML = '<meta name="tavily-hikari-build-version" content="__TAVILY_HIKARI_BUILD_VERSION__" />'

    expect(getBundledFrontendVersion()).toBeNull()
  })
})
