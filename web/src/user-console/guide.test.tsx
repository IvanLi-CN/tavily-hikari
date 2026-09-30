import { describe, expect, it } from 'bun:test'
import { Fragment, createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import {
  buildGuideContent,
  buildHikariCliInstallSnippet,
  guideSnippetToPlainText,
  resolveGuideSamples,
} from './guide'
import type { GuideContent, GuideKey } from './runtime'

describe('Hikari CLI guide content', () => {
  it('renders configured installer and optional skills commands', () => {
    const guides = buildGuideContent('en', 'https://hikari.example.com', 'th-a1b2-secretsecret')
    const samples = resolveGuideSamples(guides.hikariCli)

    expect(guides.hikariCli.title).toBe('CLI + Agent Skills')
    expect(samples).toHaveLength(2)
    expect(samples[0].snippet).toContain('install-tvly-hikari.sh')
    expect(samples[0].snippet).toContain('--base-url "https://hikari.example.com"')
    expect(samples[0].snippet).toContain('--token "th-a1b2-secretsecret"')
    expect(samples[1].snippet).toBe('npx skills add https://github.com/IvanLi-CN/tavily-hikari --global')
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

const PROVIDER_BASE_URL = 'https://hikari.example.com'
const PROVIDER_TOKEN = 'th-a1b2-secretsecret'

interface ProviderExpectation {
  key: 'serpapi' | 'exa' | 'brave'
  mcpPath: string
  httpPath: string
  httpAuthMarker: string
  boundary: { en: string, zh: string }
}

const PROVIDER_EXPECTATIONS: ProviderExpectation[] = [
  {
    key: 'serpapi',
    mcpPath: '/serpapi/mcp',
    httpPath: '/api/serpapi/search',
    httpAuthMarker: `api_key=${PROVIDER_TOKEN}`,
    boundary: { en: '/account', zh: '/account' },
  },
  {
    key: 'exa',
    mcpPath: '/exa/mcp',
    httpPath: '/api/exa/search',
    httpAuthMarker: `x-api-key: ${PROVIDER_TOKEN}`,
    boundary: { en: 'no streaming', zh: '流式' },
  },
  {
    key: 'brave',
    mcpPath: '/brave/mcp',
    httpPath: '/api/brave/res/v1/web/search',
    httpAuthMarker: `X-Subscription-Token: ${PROVIDER_TOKEN}`,
    boundary: { en: 'no URL extraction', zh: 'URL 抽取' },
  },
]

function readProviderGuide(
  guides: Record<GuideKey, GuideContent>,
  key: ProviderExpectation['key'],
): GuideContent | undefined {
  return (guides as Record<string, GuideContent | undefined>)[key]
}

function renderSteps(steps: GuideContent['steps']): string {
  return renderToStaticMarkup(createElement(Fragment, null, ...steps))
}

describe('provider setup guides', () => {
  it('renders every provider MCP + HTTP snippet with the correct auth slot in both languages', () => {
    for (const language of ['en', 'zh'] as const) {
      const guides = buildGuideContent(language, PROVIDER_BASE_URL, PROVIDER_TOKEN)
      for (const provider of PROVIDER_EXPECTATIONS) {
        const content = readProviderGuide(guides, provider.key)
        expect(content).toBeDefined()
        if (!content) continue

        const samples = resolveGuideSamples(content).map((sample) => ({
          language: sample.language,
          text: guideSnippetToPlainText(sample.snippet),
        }))

        const mcpSamples = samples.filter((sample) =>
          sample.text.includes(`${PROVIDER_BASE_URL}${provider.mcpPath}`),
        )
        expect(mcpSamples.length).toBeGreaterThanOrEqual(3)
        expect(
          mcpSamples.some((sample) => sample.language === 'toml' && sample.text.includes('bearer_token_env_var')),
        ).toBe(true)
        expect(
          mcpSamples.some((sample) =>
            sample.text.includes('claude mcp add-json') && sample.text.includes(`Bearer ${PROVIDER_TOKEN}`),
          ),
        ).toBe(true)
        expect(
          mcpSamples.some((sample) =>
            sample.language === 'json' && sample.text.includes('"servers"') && sample.text.includes(`Bearer ${PROVIDER_TOKEN}`),
          ),
        ).toBe(true)

        const httpSamples = samples.filter((sample) =>
          sample.text.includes(`${PROVIDER_BASE_URL}${provider.httpPath}`),
        )
        expect(httpSamples.length).toBeGreaterThanOrEqual(1)
        expect(httpSamples.some((sample) => sample.text.includes(provider.httpAuthMarker))).toBe(true)

        const boundaryText = renderSteps(content.steps)
        expect(boundaryText).toContain(provider.boundary[language])
      }
    }
  })

  it('documents the Brave Cherry Studio mapping and the OpenAI-style client boundary in both languages', () => {
    for (const language of ['en', 'zh'] as const) {
      const guides = buildGuideContent(language, PROVIDER_BASE_URL, PROVIDER_TOKEN)
      const brave = readProviderGuide(guides, 'brave')
      expect(brave).toBeDefined()
      if (!brave) continue
      const steps = renderSteps(brave.steps)
      expect(steps).toContain(`${PROVIDER_BASE_URL}/api/brave`)
      expect(steps).toContain('OpenAI')
    }
  })

  it('keeps the Tavily sections byte-stable', () => {
    const guides = buildGuideContent('en', PROVIDER_BASE_URL, PROVIDER_TOKEN)

    expect(guides.codex.snippet).toBe(
      [
        '<span class="hl-comment"># ~/.codex/config.toml</span>',
        '<span class="hl-key">experimental_use_rmcp_client</span> = <span class="hl-boolean">true</span>',
        '',
        '[<span class="hl-section">mcp_servers.tavily_hikari</span>]',
        `<span class="hl-key">url</span> = <span class="hl-string">"${PROVIDER_BASE_URL}/mcp"</span>`,
        '<span class="hl-key">bearer_token_env_var</span> = <span class="hl-string">"TAVILY_HIKARI_TOKEN"</span>',
      ].join('\n'),
    )

    const tavilyText = [guides.codex, guides.claude, guides.vscode, guides.other]
      .flatMap((content) => resolveGuideSamples(content))
      .map((sample) => guideSnippetToPlainText(sample.snippet))
      .join('\n')
    expect(tavilyText).toContain(`${PROVIDER_BASE_URL}/mcp`)
    expect(tavilyText).toContain(`${PROVIDER_BASE_URL}/api/tavily/search`)
    expect(tavilyText).not.toContain('/serpapi/mcp')
    expect(tavilyText).not.toContain('/exa/mcp')
    expect(tavilyText).not.toContain('/brave/mcp')
  })
})