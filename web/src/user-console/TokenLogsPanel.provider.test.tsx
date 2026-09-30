import '../../test/happydom'

import { afterEach, describe, expect, it } from 'bun:test'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'

import type { PublicTokenLog } from '../api'
import { ThemeProvider } from '../theme'
import TokenLogsPanel from './TokenLogsPanel'
import type { UserTokenLogFilter } from './TokenLogsHeader'
import { EN } from './text'

const now = 1_774_693_640

function tokenLog(overrides: Partial<PublicTokenLog> & { id: number }): PublicTokenLog {
  return {
    method: 'POST',
    path: '/api/tavily/search',
    query: null,
    http_status: 200,
    mcp_status: 200,
    business_credits: 1,
    counts_business_quota: true,
    result_status: 'success',
    error_message: null,
    created_at: now,
    ...overrides,
  }
}

/**
 * Mixed-provider fixture with one legacy row that has NO provider field at all
 * (the wire-absent case the panel must coalesce to Tavily), mirroring the
 * committed `PublicTokenLog.provider` wire from src/server/handlers/user.rs.
 */
const providerLogs: PublicTokenLog[] = [
  tokenLog({ id: 1, provider: 'serpapi', path: '/api/serpapi/search' }),
  tokenLog({ id: 2, path: '/api/tavily/search' }),
  tokenLog({ id: 3, provider: 'exa', path: '/api/exa/search' }),
]

let root: Root | null = null

afterEach(() => {
  act(() => {
    root?.unmount()
  })
  root = null
  document.body.innerHTML = ''
})

function renderPanel(logs: PublicTokenLog[] = providerLogs): HTMLElement {
  const container = document.createElement('div')
  document.body.appendChild(container)
  root = createRoot(container)
  act(() => {
    root!.render(
      <ThemeProvider>
        <TokenLogsPanel
          logs={logs}
          text={EN.detail}
          filter={'all' satisfies UserTokenLogFilter}
          filterOptions={[{ value: 'all', label: 'All' }, { value: 'billable', label: 'Quota usage' }]}
          filterDisabled={false}
          pushIssue={null}
          mode="full"
          onFilterChange={() => undefined}
          onOpenFull={() => undefined}
          formatTimestamp={() => 'time'}
          formatLogCredits={() => '1'}
          statusTone={() => 'neutral'}
        />
      </ThemeProvider>,
    )
  })
  return container
}

function providerBadgeLabels(container: HTMLElement): string[] {
  return Array.from(container.querySelectorAll('.user-console-log-provider-badge')).map(
    (element) => element.textContent?.trim() ?? '',
  )
}

function providerFilterButton(container: HTMLElement, label: string): HTMLButtonElement {
  const group = container.querySelector(
    `[role="radiogroup"][aria-label="${EN.detail.logFilters.provider}"]`,
  )
  if (!group) throw new Error('provider filter radiogroup not found')
  const button = Array.from(group.querySelectorAll('button')).find(
    (element) => element.textContent?.trim() === label,
  )
  if (!button) throw new Error(`provider option ${label} not found`)
  return button as HTMLButtonElement
}

describe('TokenLogsPanel provider dimension (plan todo 23b)', () => {
  it('renders the wire provider badge and coalesces the legacy row to Tavily', () => {
    const container = renderPanel()

    expect(providerBadgeLabels(container)).toEqual(['SerpApi', 'Tavily', 'Exa'])
  })

  it('client-side provider filter narrows the rendered rows to the selected provider', () => {
    const container = renderPanel()

    act(() => {
      providerFilterButton(container, 'SerpApi').dispatchEvent(
        new MouseEvent('click', { bubbles: true }),
      )
    })

    expect(providerBadgeLabels(container)).toEqual(['SerpApi'])
    expect(container.textContent).toContain('/api/serpapi/search')
    expect(container.textContent).not.toContain('/api/exa/search')

    act(() => {
      providerFilterButton(container, 'Tavily').dispatchEvent(
        new MouseEvent('click', { bubbles: true }),
      )
    })

    expect(providerBadgeLabels(container)).toEqual(['Tavily'])
    expect(container.textContent).toContain('/api/tavily/search')
    expect(container.textContent).not.toContain('/api/serpapi/search')
  })
})