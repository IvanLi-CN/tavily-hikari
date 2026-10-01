import '../../test/happydom'

import { afterEach, describe, expect, it } from 'bun:test'

import { addProviderAwareApiKeysBatch } from './keyProviderBatch'

const originalFetch = window.fetch

afterEach(() => {
  window.fetch = originalFetch
})

describe('addProviderAwareApiKeysBatch (plan todo 22)', () => {
  it('keeps the legacy Tavily path on the batch endpoint', async () => {
    const calls: Array<{ url: string; body: Record<string, unknown> }> = []
    window.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = typeof input === 'string' ? input : input.toString()
      calls.push({ url, body: JSON.parse(String(init?.body ?? '{}')) })
      return new Response(
        JSON.stringify({
          summary: {
            input_lines: 1,
            valid_lines: 1,
            unique_in_input: 1,
            created: 1,
            undeleted: 0,
            existed: 0,
            duplicate_in_input: 0,
            failed: 0,
          },
          results: [{ api_key: 'tvly-1', status: 'created', id: 'key-1' }],
        }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      )
    }) as typeof window.fetch

    await addProviderAwareApiKeysBatch('tavily', [{ api_key: 'tvly-1' }], 'prod')
    expect(calls.length).toBe(1)
    expect(calls[0]?.url).toContain('/api/keys/batch')
  })

  it('stamps every non-Tavily key with the batch provider via the single-key endpoint', async () => {
    const calls: Array<{ url: string; body: Record<string, unknown> }> = []
    window.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = typeof input === 'string' ? input : input.toString()
      calls.push({ url, body: JSON.parse(String(init?.body ?? '{}')) })
      return new Response(JSON.stringify({ id: `key-${calls.length}` }), {
        status: 201,
        headers: { 'content-type': 'application/json' },
      })
    }) as typeof window.fetch

    const response = await addProviderAwareApiKeysBatch(
      'brave',
      [{ api_key: 'brave-1' }, { api_key: 'brave-2' }],
      'search',
    )
    expect(calls.length).toBe(2)
    expect(calls.every((call) => call.url.includes('/api/keys'))).toBe(true)
    expect(calls.every((call) => call.body.provider === 'brave')).toBe(true)
    expect(calls.every((call) => call.body.group === 'search')).toBe(true)
    expect(response.summary.created).toBe(2)
    expect(response.summary.failed).toBe(0)
    expect(response.results.map((result) => result.status)).toEqual(['created', 'created'])
  })

  it('reports per-key failures without aborting the batch', async () => {
    let call = 0
    window.fetch = (async () => {
      call += 1
      if (call === 1) {
        return new Response(JSON.stringify({ error: 'provider_conflict' }), {
          status: 409,
          headers: { 'content-type': 'application/json' },
        })
      }
      return new Response(JSON.stringify({ id: 'key-2' }), {
        status: 201,
        headers: { 'content-type': 'application/json' },
      })
    }) as typeof window.fetch

    const response = await addProviderAwareApiKeysBatch('exa', [{ api_key: 'a' }, { api_key: 'b' }])
    expect(response.summary.created).toBe(1)
    expect(response.summary.failed).toBe(1)
    expect(response.results[0]?.status).toBe('failed')
    expect(response.results[1]?.status).toBe('created')
  })
})