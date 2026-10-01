import '../../test/happydom'

import { afterEach, describe, expect, it } from 'bun:test'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'

import type { ApiKeyStats } from '../api'
import { LanguageProvider } from '../i18n'
import { ThemeProvider } from '../theme'
import {
  KeyQuotaPanel,
  KeyUsageSyncButton,
  performKeyUsageSync,
  usageSyncErrorCopy,
} from './keyProviderQuota'

const roots: Root[] = []
const originalFetch = window.fetch

const numberFormatter = (value: number) => String(value)
const timestampFormatter = (value: number | null) => (value ? `T${value}` : '—')
const percentFormatter = (numerator: number, denominator: number) =>
  denominator === 0 ? '—' : `${Math.round((numerator / denominator) * 100)}%`

function keyStats(overrides: Partial<ApiKeyStats>): ApiKeyStats {
  return {
    id: 'key-1',
    provider: 'tavily',
    status: 'active',
    group: null,
    registration_ip: null,
    registration_region: null,
    status_changed_at: null,
    last_used_at: null,
    deleted_at: null,
    quota_limit: null,
    quota_remaining: null,
    quota_synced_at: null,
    total_requests: 0,
    success_count: 0,
    error_count: 0,
    quota_exhausted_count: 0,
    quarantine: null,
    transient_backoff: null,
    ...overrides,
  }
}

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

function renderQuota(detail: ApiKeyStats | null, language: 'en' | 'zh' = 'en'): HTMLElement {
  return render(
    <KeyQuotaPanel
      detail={detail}
      loadState="ready"
      loadingLabel="Loading"
      syncNotice={null}
      formatNumber={numberFormatter}
      formatTimestamp={timestampFormatter}
      formatPercent={percentFormatter}
    />,
    language,
  )
}

afterEach(() => {
  for (const root of roots.splice(0)) {
    act(() => root.unmount())
  }
  window.fetch = originalFetch
  document.body.innerHTML = ''
})

describe('KeyQuotaPanel quotaDisplayKind variants (plan todo 22)', () => {
  it('keeps the Tavily block byte-stable', () => {
    const container = renderQuota(
      keyStats({ provider: 'tavily', quota_limit: 100, quota_remaining: 40, quota_synced_at: 1_700_000_000 }),
    )
    const text = container.textContent ?? ''
    expect(text).toContain('Tavily Usage for this key')
    expect(text).toContain('Used')
    expect(text).toContain('Remaining')
    expect(text).toContain('Synced')
    expect(text).toContain('60')
    expect(text).toContain('40')
  })

  it('renders SerpApi remaining/limit, renewal date, and the /account note', () => {
    const container = renderQuota(
      keyStats({
        provider: 'serpapi',
        quota_limit: 30000,
        quota_remaining: 28740,
        quota_renews_at: 1_800_000_000,
        quota_synced_at: 1_700_000_000,
      }),
    )
    const text = container.textContent ?? ''
    expect(text).toContain('SerpApi plan quota synced from /account')
    expect(text).toContain('Renews')
    expect(text).toContain('28740')
  })

  it('renders zh SerpApi copy and renewal label', () => {
    const container = renderQuota(
      keyStats({ provider: 'serpapi', quota_limit: 30000, quota_remaining: 28740, quota_renews_at: 1_800_000_000 }),
      'zh',
    )
    const text = container.textContent ?? ''
    expect(text).toContain('SerpApi 套餐额度，来自 /account 同步')
    expect(text).toContain('续费')
  })

  it('renders the Brave harvested-from-headers note', () => {
    const container = renderQuota(
      keyStats({ provider: 'brave', quota_limit: 2000, quota_remaining: 1750, quota_synced_at: 1_700_000_000 }),
    )
    expect(container.textContent ?? '').toContain('harvested from upstream response headers')
  })

  it('renders the Exa no-balance-API note without metric cards', () => {
    const container = renderQuota(keyStats({ provider: 'exa' }))
    expect(container.textContent ?? '').toContain('no upstream balance API')
    expect(container.querySelectorAll('.metric-card').length).toBe(0)
  })
})

describe('KeyUsageSyncButton gating (plan todo 22)', () => {
  it('disables Exa with an explanatory tooltip', () => {
    const container = render(<KeyUsageSyncButton provider="exa" syncState="idle" onSync={() => undefined} />)
    const button = container.querySelector('button')!
    expect(button.disabled).toBe(true)
    expect(button.getAttribute('title')).toBe('Exa has no upstream balance API, so usage cannot be synced.')
  })

  it('leaves Tavily enabled for the legacy sync flow', () => {
    const container = render(<KeyUsageSyncButton provider="tavily" syncState="idle" onSync={() => undefined} />)
    const button = container.querySelector('button')!
    expect(button.disabled).toBe(false)
    expect(button.getAttribute('title')).toBeNull()
  })
})

describe('performKeyUsageSync (plan todo 22)', () => {
  it('returns no_content for the legacy Tavily 204', async () => {
    window.fetch = (async () => new Response(null, { status: 204 })) as typeof window.fetch
    expect(await performKeyUsageSync('key-1')).toEqual({ kind: 'no_content' })
  })

  it('returns the passive Brave snapshot source', async () => {
    window.fetch = (async () =>
      new Response(
        JSON.stringify({
          source: 'passive',
          provider: 'brave',
          quota_limit: null,
          quota_remaining: 12,
          quota_renews_at: null,
          quota_synced_at: null,
        }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      )) as typeof window.fetch
    expect(await performKeyUsageSync('key-1')).toEqual({ kind: 'snapshot', source: 'passive' })
  })

  it('normalizes the typed Exa sync_unsupported 409', async () => {
    window.fetch = (async () =>
      new Response(JSON.stringify({ error: 'sync_unsupported', detail: 'exa has no upstream balance API' }), {
        status: 409,
        headers: { 'content-type': 'application/json' },
      })) as typeof window.fetch
    expect(await performKeyUsageSync('key-1')).toEqual({ kind: 'unsupported' })
  })

  it('maps the typed 409 to the localized unsupported copy', () => {
    const err = Object.assign(new Error('exa has no upstream balance API'), {
      status: 409,
      body: { error: 'sync_unsupported' },
    })
    expect(usageSyncErrorCopy(err, 'fallback', 'unsupported copy')).toBe('unsupported copy')
    expect(usageSyncErrorCopy(new Error('boom'), 'fallback', 'unsupported copy')).toBe('boom')
    expect(usageSyncErrorCopy('nope', 'fallback', 'unsupported copy')).toBe('fallback')
  })
})