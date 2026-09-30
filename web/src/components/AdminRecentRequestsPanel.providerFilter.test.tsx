import '../../test/happydom'

import { afterEach, describe, expect, it } from 'bun:test'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'

import type { RequestLog } from '../api'
import { LanguageProvider, translations } from '../i18n'
import { ThemeProvider } from '../theme'
import AdminRecentRequestsPanel from './AdminRecentRequestsPanel'
import { TooltipProvider } from './ui/tooltip'

const now = 1_774_693_640

function requestLog(overrides: Partial<RequestLog> & { id: number; key_id: string }): RequestLog {
  return {
    auth_token_id: 'T900',
    method: 'POST',
    path: '/api/tavily/search',
    query: null,
    http_status: 200,
    mcp_status: 200,
    result_status: 'success',
    created_at: now,
    error_message: null,
    request_body: null,
    response_body: null,
    forwarded_headers: [],
    dropped_headers: [],
    operationalClass: 'success',
    requestKindProtocolGroup: 'api',
    requestKindBillingGroup: 'billable',
    ...overrides,
  }
}

/**
 * Mixed-provider fixture with one legacy row that has NO provider field at all
 * (the wire-absent case the panel must coalesce to Tavily).
 */
const mixedProviderLogs: RequestLog[] = [
  requestLog({
    id: 9001,
    key_id: 'exa-row',
    provider: 'exa',
    path: '/api/exa/search',
    request_kind_key: 'api:exa/search',
    request_kind_label: 'API | exa/search',
  }),
  requestLog({
    id: 9002,
    key_id: 'brave-row',
    provider: 'brave',
    path: '/api/brave/res/v1/web/search',
    request_kind_key: 'api:brave/web',
    request_kind_label: 'API | brave/web',
  }),
  requestLog({
    id: 9003,
    key_id: 'legacy-row',
    path: '/api/tavily/search',
    request_kind_key: 'api:search',
    request_kind_label: 'API | search',
  }),
]

/** Whole-window facet payload mirroring `RequestLogFacets.providers` (T20). */
const providerFacets = [
  { value: 'tavily', count: 1 },
  { value: 'exa', count: 1 },
  { value: 'brave', count: 1 },
]

let root: Root | null = null

afterEach(() => {
  act(() => {
    root?.unmount()
  })
  root = null
  document.body.innerHTML = ''
})

function renderPanel(selectedProvider?: string | null) {
  const container = document.createElement('div')
  document.body.appendChild(container)
  root = createRoot(container)
  act(() => {
    root!.render(
      <LanguageProvider initialLanguage="zh">
        <ThemeProvider>
          <TooltipProvider>
            <AdminRecentRequestsPanel
              variant="admin"
              language="zh"
              strings={translations.zh.admin}
              title="Provider Filter Proof"
              description="Mixed-provider rows"
              emptyLabel="No logs."
              loadState="ready"
              loadingLabel="Loading…"
              logs={mixedProviderLogs}
              requestKindOptions={[]}
              requestKindQuickBilling="all"
              requestKindQuickProtocol="all"
              selectedRequestKinds={[]}
              onRequestKindQuickFiltersChange={() => undefined}
              onToggleRequestKind={() => undefined}
              onClearRequestKinds={() => undefined}
              outcomeFilter={null}
              resultOptions={[]}
              keyEffectOptions={[]}
              onOutcomeFilterChange={() => undefined}
              keyOptions={[]}
              selectedKeyId={null}
              providerOptions={providerFacets}
              selectedProvider={selectedProvider}
              onProviderFilterChange={() => undefined}
              showKeyColumn
              showTokenColumn={false}
              perPage={20}
              hasOlder={false}
              hasNewer={false}
              onNewerPage={() => undefined}
              onOlderPage={() => undefined}
              onPerPageChange={() => undefined}
              formatTime={() => 'time'}
              loadLogBodies={() => Promise.resolve({ request_body: null, response_body: null })}
            />
          </TooltipProvider>
        </ThemeProvider>
      </LanguageProvider>,
    )
  })
  return container
}

describe('AdminRecentRequestsPanel provider dimension (plan todo 23 QA)', () => {
  it('renders every provider label and coalesces the legacy NULL row to Tavily', () => {
    const container = renderPanel(null)
    const text = container.textContent ?? ''

    // todo-20 facets drive the filter options; the legacy row (no provider
    // field) still renders an explicit Tavily provider label.
    expect(container.querySelector('.searchable-facet-select__summary')?.textContent).toBe(
      '全部提供方',
    )
    expect(text).toContain('legacy-row')
    expect(text).toContain('Tavily')
    expect(text).toContain('Exa')
    expect(text).toContain('Brave')
  })

  it('provider Exa selection yields only exa rows (QA happy path)', () => {
    const container = renderPanel('exa')
    const text = container.textContent ?? ''

    // The filter trigger reflects the selected provider ...
    expect(container.querySelector('.searchable-facet-select__summary')?.textContent).toBe('Exa')
    // ... and only exa rows are rendered.
    expect(text).toContain('exa-row')
    expect(text).not.toContain('brave-row')
    expect(text).not.toContain('legacy-row')
  })

  it('provider Tavily selection keeps the legacy NULL-provider row (coalesced)', () => {
    const container = renderPanel('tavily')
    const text = container.textContent ?? ''

    expect(text).toContain('legacy-row')
    expect(text).not.toContain('exa-row')
    expect(text).not.toContain('brave-row')
  })

  it('derives the provider filter from row providers when the facet payload is absent', () => {
    const container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
    act(() => {
      root!.render(
        <LanguageProvider initialLanguage="zh">
          <ThemeProvider>
            <TooltipProvider>
              <AdminRecentRequestsPanel
                variant="admin"
                language="zh"
                strings={translations.zh.admin}
                title="Provider Filter Proof"
                description="Mixed-provider rows"
                emptyLabel="No logs."
                loadState="ready"
                loadingLabel="Loading…"
                logs={mixedProviderLogs}
                requestKindOptions={[]}
                requestKindQuickBilling="all"
                requestKindQuickProtocol="all"
                selectedRequestKinds={[]}
                onRequestKindQuickFiltersChange={() => undefined}
                onToggleRequestKind={() => undefined}
                onClearRequestKinds={() => undefined}
                outcomeFilter={null}
                resultOptions={[]}
                keyEffectOptions={[]}
                onOutcomeFilterChange={() => undefined}
                keyOptions={[]}
                selectedKeyId={null}
                showKeyColumn
                showTokenColumn={false}
                perPage={20}
                hasOlder={false}
                hasNewer={false}
                onNewerPage={() => undefined}
                onOlderPage={() => undefined}
                onPerPageChange={() => undefined}
                formatTime={() => 'time'}
                loadLogBodies={() => Promise.resolve({ request_body: null, response_body: null })}
              />
            </TooltipProvider>
          </ThemeProvider>
        </LanguageProvider>,
      )
    })

    // No `providerOptions` prop: the filter still renders from row providers.
    expect(container.querySelector('.searchable-facet-select__trigger')).not.toBeNull()
  })
})