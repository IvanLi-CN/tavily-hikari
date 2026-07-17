import { describe, expect, it } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'

import UserConsoleFooter, { buildUserConsoleFooterRelease } from './UserConsoleFooter'

const strings = {
  title: 'Tavily Hikari User Console',
  githubAria: 'Open GitHub repository',
  githubLabel: 'GitHub',
  loadingVersion: '· Loading version…',
  errorVersion: '· Version unavailable',
  tagPrefix: '· ',
}

describe('UserConsoleFooter', () => {
  it('renders the GitHub and release link when the backend version is available', () => {
    const html = renderToStaticMarkup(
      <UserConsoleFooter
        strings={strings}
        versionState={{ status: 'ready', value: { backend: '0.2.0', frontend: '0.2.0' } }}
      />,
    )

    expect(html).toContain('Tavily Hikari User Console')
    expect(html).toContain('Open GitHub repository')
    expect(html).toContain('href="https://github.com/IvanLi-CN/tavily-hikari"')
    expect(html).toContain('href="https://octo-rill.ivanli.cc/IvanLi-CN/tavily-hikari/releases?highlight=tag%3Av0.2.0&amp;highlight_active=tag%3Av0.2.0"')
    expect(html).toContain('v0.2.0')
  })

  it('renders a prerelease link without truncating the suffix', () => {
    const html = renderToStaticMarkup(
      <UserConsoleFooter
        strings={strings}
        versionState={{ status: 'ready', value: { backend: '0.2.0-rc.1', frontend: '0.2.0-rc.1' } }}
      />,
    )

    expect(html).toContain('href="https://octo-rill.ivanli.cc/IvanLi-CN/tavily-hikari/releases?highlight=tag%3Av0.2.0-rc.1&amp;highlight_active=tag%3Av0.2.0-rc.1"')
    expect(html).toContain('v0.2.0-rc.1')
  })

  it('renders plain text when the version is a blocked non-release build', () => {
    const html = renderToStaticMarkup(
      <UserConsoleFooter
        strings={strings}
        versionState={{ status: 'ready', value: { backend: '0.2.0-dev', frontend: '0.2.0-dev' } }}
      />,
    )

    expect(html).toContain('v0.2.0-dev')
    expect(html).not.toContain('octo-rill.ivanli.cc')
  })

  it('falls back to the loading copy while version data is still loading', () => {
    const html = renderToStaticMarkup(<UserConsoleFooter strings={strings} versionState={{ status: 'loading' }} />)

    expect(html).toContain('· Loading version…')
    expect(html).not.toContain('/releases/tag/')
  })

  it('shows an error placeholder when the version request fails', () => {
    const html = renderToStaticMarkup(<UserConsoleFooter strings={strings} versionState={{ status: 'error' }} />)

    expect(html).toContain('· Version unavailable')
    expect(html).not.toContain('/releases/tag/')
  })
})

describe('buildUserConsoleFooterRelease', () => {
  it('builds a release link for stable and prerelease versions but rejects blocked dev channels', () => {
    expect(buildUserConsoleFooterRelease({ backend: '0.2.0', frontend: '0.2.0' })).toEqual({
      href: 'https://octo-rill.ivanli.cc/IvanLi-CN/tavily-hikari/releases?highlight=tag%3Av0.2.0&highlight_active=tag%3Av0.2.0',
      label: 'v0.2.0',
    })
    expect(buildUserConsoleFooterRelease({ backend: '0.2.0-rc.1', frontend: '0.2.0-rc.1' })).toEqual({
      href: 'https://octo-rill.ivanli.cc/IvanLi-CN/tavily-hikari/releases?highlight=tag%3Av0.2.0-rc.1&highlight_active=tag%3Av0.2.0-rc.1',
      label: 'v0.2.0-rc.1',
    })
    expect(buildUserConsoleFooterRelease({ backend: '0.2.0-dev', frontend: '0.2.0-dev' })).toBeNull()
    expect(buildUserConsoleFooterRelease({ backend: 'ci-deadbeef', frontend: 'ci-deadbeef' })).toBeNull()
  })
})
