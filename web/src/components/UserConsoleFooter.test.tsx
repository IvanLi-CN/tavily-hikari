import { describe, expect, it } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'

import UserConsoleFooter, { buildUserConsoleFooterRelease } from './UserConsoleFooter'

const strings = {
  title: 'Tavily Hikari User Console',
  githubAria: 'Open GitHub repository',
  githubLabel: 'GitHub',
  loadingVersion: '· Loading version…',
  tagPrefix: '· ',
}

describe('UserConsoleFooter', () => {
  it('renders the GitHub and release link when the backend version is available', () => {
    const html = renderToStaticMarkup(
      <UserConsoleFooter
        strings={strings}
        version={{ backend: '0.2.0-dev', frontend: '0.2.0-dev' }}
      />,
    )

    expect(html).toContain('Tavily Hikari User Console')
    expect(html).toContain('Open GitHub repository')
    expect(html).toContain('href="https://github.com/IvanLi-CN/tavily-hikari"')
    expect(html).toContain('href="https://github.com/IvanLi-CN/tavily-hikari/releases/tag/v0.2.0"')
    expect(html).toContain('v0.2.0-dev')
  })

  it('falls back to the loading copy when version data is unavailable', () => {
    const html = renderToStaticMarkup(<UserConsoleFooter strings={strings} version={null} />)

    expect(html).toContain('· Loading version…')
    expect(html).not.toContain('/releases/tag/')
  })
})

describe('buildUserConsoleFooterRelease', () => {
  it('strips prerelease suffixes when building the release tag URL', () => {
    expect(buildUserConsoleFooterRelease({ backend: '0.2.0-dev', frontend: '0.2.0-dev' })).toEqual({
      href: 'https://github.com/IvanLi-CN/tavily-hikari/releases/tag/v0.2.0',
      label: 'v0.2.0-dev',
    })
  })
})
