import '../../test/happydom'

import { afterEach, describe, expect, it } from 'bun:test'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'

import { LanguageProvider } from '../i18n'
import { ThemeProvider } from '../theme'
import { ProviderResultExtras, type KeyValidationRow } from './ApiKeysValidationDialog'

const roots: Root[] = []

function row(overrides: Partial<KeyValidationRow>): KeyValidationRow {
  return { api_key: 'k', status: 'ok', attempts: 1, ...overrides }
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

afterEach(() => {
  for (const root of roots.splice(0)) {
    act(() => root.unmount())
  }
  document.body.innerHTML = ''
})

describe('validation dialog per-provider results (plan todo 22)', () => {
  it('renders the SerpApi plan fields from the probe detail', () => {
    const container = render(
      <ProviderResultExtras
        row={row({
          provider: 'serpapi',
          status: 'ok',
          detail:
            'plan_searches_left=120; total_searches_left=95; searches_per_month=500; plan_renewal_date=2026-04-01; account_status=Active',
        })}
      />,
    )
    const text = container.textContent ?? ''
    expect(text).toContain('Plan searches left')
    expect(text).toContain('120')
    expect(text).toContain('Account status')
    expect(text).toContain('Active')
  })

  it('renders a Brave reason chip with the machine code and localized label', () => {
    const container = render(
      <ProviderResultExtras
        row={row({ provider: 'brave', status: 'invalid', reason: 'SUBSCRIPTION_TOKEN_INVALID' })}
      />,
    )
    const chip = container.querySelector('[data-reason="SUBSCRIPTION_TOKEN_INVALID"]')!
    expect(chip).not.toBeNull()
    expect(chip.textContent).toContain('SUBSCRIPTION_TOKEN_INVALID')
    expect(chip.textContent).toContain('Subscription token invalid')

    const zhContainer = render(
      <ProviderResultExtras
        row={row({ provider: 'brave', status: 'invalid', reason: 'SUBSCRIPTION_TOKEN_INVALID' })}
      />,
      'zh',
    )
    expect(zhContainer.textContent).toContain('订阅令牌无效')
  })

  it('renders the Exa authenticated-vs-rejected wording in en and zh', () => {
    const valid = render(<ProviderResultExtras row={row({ provider: 'exa', status: 'ok', reason: 'UNBILLED_VALIDATION_ERROR' })} />)
    expect(valid.textContent).toContain('Key authenticated (unbilled validation error)')

    const invalid = render(<ProviderResultExtras row={row({ provider: 'exa', status: 'unauthorized', reason: 'UNAUTHORIZED' })} />)
    expect(invalid.textContent).toContain('Key rejected (401 unauthorized)')

    const zhInvalid = render(
      <ProviderResultExtras row={row({ provider: 'exa', status: 'unauthorized', reason: 'UNAUTHORIZED' })} />,
      'zh',
    )
    expect(zhInvalid.textContent).toContain('密钥被拒绝（401 未授权）')
  })
})