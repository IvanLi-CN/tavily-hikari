import '../../test/happydom'

import { afterEach, describe, expect, it } from 'bun:test'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'

import { LanguageProvider } from '../i18n'
import { ThemeProvider } from '../theme'
import {
  KeyProviderBadge,
  KeyProviderFilter,
  KeyProviderSelect,
  PROVIDER_FILTER_ALL,
  fetchAdminKeysPage,
} from './keyProviderColumn'

const roots: Root[] = []
const originalFetch = window.fetch

function render(node: JSX.Element, language: 'en' | 'zh' = 'en'): HTMLElement {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)
  roots.push(root)
  act(() => {
    root.render(
      <LanguageProvider initialLanguage={language}>
        <ThemeProvider>{node}</ThemeProvider>
      </LanguageProvider>,
    )
  })
  return container
}

afterEach(() => {
  for (const root of roots.splice(0)) {
    act(() => root.unmount())
  }
  window.fetch = originalFetch
  document.body.innerHTML = ''
})

describe('KeyProviderBadge (plan todo 22)', () => {
  it('renders a text badge per provider and coalesces legacy NULL to Tavily', () => {
    const container = render(
      <>
        <KeyProviderBadge provider="tavily" />
        <KeyProviderBadge provider="serpapi" />
        <KeyProviderBadge provider="exa" />
        <KeyProviderBadge provider="brave" />
        <KeyProviderBadge provider={undefined} />
        <KeyProviderBadge provider={null} />
        <KeyProviderBadge provider="openai" />
      </>,
    )
    const badges = Array.from(container.querySelectorAll<HTMLElement>('[data-provider]'))
    expect(badges.map((badge) => badge.dataset.provider)).toEqual([
      'tavily',
      'serpapi',
      'exa',
      'brave',
      'tavily',
      'tavily',
      'unknown',
    ])
    expect(badges.map((badge) => badge.textContent)).toEqual([
      'Tavily',
      'SerpApi',
      'Exa',
      'Brave',
      'Tavily',
      'Tavily',
      'openai',
    ])
  })
})

describe('KeyProviderFilter (plan todo 22)', () => {
  it('lists all providers in stable order and reports the selection', () => {
    const changes: string[] = []
    const container = render(
      <KeyProviderFilter value={PROVIDER_FILTER_ALL} onChange={(value) => changes.push(value)} />,
    )
    const select = container.querySelector('select')!
    expect(select.value).toBe(PROVIDER_FILTER_ALL)
    expect(Array.from(select.options).map((option) => option.value)).toEqual([
      'all',
      'tavily',
      'serpapi',
      'exa',
      'brave',
    ])
    expect(select.options[0]?.textContent).toBe('All providers')

    act(() => {
      select.value = 'brave'
      select.dispatchEvent(new Event('change', { bubbles: true }))
    })
    expect(changes).toEqual(['brave'])
  })

  it('resolves the localized filter label in zh', () => {
    const container = render(<KeyProviderFilter value="all" onChange={() => undefined} />, 'zh')
    expect(container.querySelector('select')?.getAttribute('aria-label')).toBe('提供商')
  })
})

describe('KeyProviderSelect (plan todo 22)', () => {
  it('defaults to Tavily and documents one-provider-per-batch', () => {
    const container = render(<KeyProviderSelect value="tavily" onChange={() => undefined} />)
    const select = container.querySelector('select')!
    expect(select.value).toBe('tavily')
    expect(Array.from(select.options).map((option) => option.value)).toEqual([
      'tavily',
      'serpapi',
      'exa',
      'brave',
    ])
    expect(container.textContent).toContain('One provider applies to the whole batch.')
  })
})

describe('fetchAdminKeysPage (plan todo 22)', () => {
  it('omits ?provider when no filter is active (legacy byte-identical)', async () => {
    const urls: string[] = []
    window.fetch = (async (input: RequestInfo | URL) => {
      urls.push(typeof input === 'string' ? input : input.toString())
      return new Response(
        JSON.stringify({ items: [], total: 0, page: 1, per_page: 20, facets: { groups: [], statuses: [], regions: [] } }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      )
    }) as typeof window.fetch

    await fetchAdminKeysPage(1, 20, { provider: PROVIDER_FILTER_ALL })
    expect(urls[0]).not.toContain('provider=')
  })

  it('appends ?provider= and returns only the matching rows', async () => {
    const urls: string[] = []
    const allRows = [
      { id: 'k-tavily', provider: 'tavily' },
      { id: 'k-brave', provider: 'brave' },
    ]
    window.fetch = (async (input: RequestInfo | URL) => {
      const url = typeof input === 'string' ? input : input.toString()
      urls.push(url)
      const provider = new URL(url, 'http://localhost').searchParams.get('provider')
      const items = provider ? allRows.filter((row) => row.provider === provider) : allRows
      return new Response(
        JSON.stringify({ items, total: items.length, page: 1, per_page: 20, facets: { groups: [], statuses: [], regions: [] } }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      )
    }) as typeof window.fetch

    const page = await fetchAdminKeysPage(1, 20, { provider: 'brave' })
    expect(urls[0]).toContain('provider=brave')
    expect(page.items.map((item) => item.id)).toEqual(['k-brave'])
  })
})