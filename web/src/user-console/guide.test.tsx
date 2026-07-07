import { describe, expect, it } from 'bun:test'

import {
  buildGuideContent,
  buildHikariCliInstallSnippet,
  guideSnippetToPlainText,
  resolveGuideSamples,
} from './guide'

describe('Hikari CLI guide content', () => {
  it('renders configured installer and optional skills commands', () => {
    const guides = buildGuideContent('en', 'https://hikari.example.com', 'th-a1b2-secretsecret')
    const samples = resolveGuideSamples(guides.hikariCli)

    expect(guides.hikariCli.title).toBe('CLI + Agent Skills')
    expect(samples).toHaveLength(2)
    expect(samples[0].snippet).toContain('install-tvly-hikari.sh')
    expect(samples[0].snippet).toContain('--base-url "https://hikari.example.com"')
    expect(samples[0].snippet).toContain('--token "th-a1b2-secretsecret"')
    expect(samples[1].snippet).toBe('npx skills add https://github.com/IvanLi-CN/tavily-hikari')
  })

  it('converts highlighted snippets to copyable plain text', () => {
    const snippet = buildHikariCliInstallSnippet('http://127.0.0.1:58087', 'th-test-secretsecret')
    const plain = guideSnippetToPlainText(snippet)

    expect(plain).toContain('curl -fsSL')
    expect(plain).toContain('--base-url "http://127.0.0.1:58087"')
    expect(plain).toContain('--token "th-test-secretsecret"')
    expect(plain).not.toContain('<span')
  })
})
