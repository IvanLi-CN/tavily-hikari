import { afterEach, describe, expect, it, mock } from 'bun:test'

import {
  bindAdminUserTag,
  fetchAdminRegistrationSettings,
  fetchAdminUsers,
  fetchAdminUserTags,
  fetchApiKeys,
  fetchJobs,
  updateAdminRegistrationSettings,
  updateAdminUserQuota,
} from './api'

const originalFetch = globalThis.fetch

afterEach(() => {
  globalThis.fetch = originalFetch
})

describe('admin user tag api helpers', () => {
  it('unwraps tag catalog list responses', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            items: [
              {
                id: 'linuxdo_l2',
                name: 'linuxdo_l2',
                displayName: 'L2',
                icon: 'linuxdo',
                systemKey: 'linuxdo_l2',
                effectKind: 'quota_delta',
                hourlyAnyDelta: 0,
                hourlyDelta: 0,
                dailyDelta: 0,
                monthlyDelta: 0,
                userCount: 4,
              },
            ],
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    const tags = await fetchAdminUserTags()

    expect(tags).toHaveLength(1)
    expect(tags[0]).toMatchObject({
      id: 'linuxdo_l2',
      displayName: 'L2',
      systemKey: 'linuxdo_l2',
      effectKind: 'quota_delta',
    })
  })

  it('sends user tag binding requests to the user-scoped endpoint', async () => {
    const fetchMock = mock(() => Promise.resolve(new Response(null, { status: 204 })))
    globalThis.fetch = fetchMock as typeof fetch

    await bindAdminUserTag('usr_alice', 'team_lead')

    expect(fetchMock).toHaveBeenCalledTimes(1)
    const [input, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(input).toBe('/api/users/usr_alice/tags')
    expect(init.method).toBe('POST')
    expect(init.body).toBe(JSON.stringify({ tagId: 'team_lead' }))
  })

  it('sends exact tag filters when listing admin users', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            items: [],
            total: 0,
            page: 1,
            per_page: 20,
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    await fetchAdminUsers(1, 20, 'L2', 'linuxdo_l2')

    expect(fetchMock).toHaveBeenCalledTimes(1)
    const [input] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(input).toBe('/api/users?page=1&per_page=20&q=L2&tagId=linuxdo_l2')
  })

  it('sends repeated key group and status filters when listing paginated api keys', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            items: [],
            total: 0,
            page: 2,
            perPage: 50,
            facets: {
              groups: [{ value: 'ops', count: 3 }],
              statuses: [{ value: 'quarantined', count: 2 }],
              regions: [{ value: 'US California', count: 1 }],
            },
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    const result = await fetchApiKeys(2, 50, {
      groups: ['ops', ''],
      statuses: ['Quarantined', 'disabled'],
      registrationIp: '8.8.8.8',
      regions: ['US California', 'JP Tokyo'],
    })

    expect(fetchMock).toHaveBeenCalledTimes(1)
    const [input] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(input).toBe(
      '/api/keys?page=2&per_page=50&group=ops&group=&status=quarantined&status=disabled&registration_ip=8.8.8.8&region=US+California&region=JP+Tokyo',
    )
    expect(result.page).toBe(2)
    expect(result.perPage).toBe(50)
    expect(result.facets.groups[0]).toEqual({ value: 'ops', count: 3 })
    expect(result.facets.regions[0]).toEqual({ value: 'US California', count: 1 })
  })

  it('patches base quota through the existing user quota endpoint', async () => {
    const fetchMock = mock(() => Promise.resolve(new Response(null, { status: 204 })))
    globalThis.fetch = fetchMock as typeof fetch

    await updateAdminUserQuota('usr_alice', {
      hourlyAnyLimit: 1200,
      hourlyLimit: 1000,
      dailyLimit: 24000,
      monthlyLimit: 600000,
    })

    expect(fetchMock).toHaveBeenCalledTimes(1)
    const [input, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(input).toBe('/api/users/usr_alice/quota')
    expect(init.method).toBe('PATCH')
    expect(init.body).toBe(
      JSON.stringify({
        hourlyAnyLimit: 1200,
        hourlyLimit: 1000,
        dailyLimit: 24000,
        monthlyLimit: 600000,
      }),
    )
  })

  it('reads admin registration settings from the dedicated endpoint', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(JSON.stringify({ allowRegistration: false }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    const settings = await fetchAdminRegistrationSettings()

    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/admin/registration')
    expect(settings).toEqual({ allowRegistration: false })
  })

  it('patches admin registration settings through the dedicated endpoint', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(JSON.stringify({ allowRegistration: true }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    const settings = await updateAdminRegistrationSettings(true)

    expect(fetchMock).toHaveBeenCalledTimes(1)
    const [input, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(input).toBe('/api/admin/registration')
    expect(init.method).toBe('PATCH')
    expect(init.body).toBe(JSON.stringify({ allowRegistration: true }))
    expect(settings).toEqual({ allowRegistration: true })
  })

  it('normalizes jobs responses to the snake_case shape used by the admin UI', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(
          JSON.stringify({
            items: [
              {
                id: 37696,
                jobType: 'quota_sync',
                keyId: '7QZ5',
                keyGroup: 'ops',
                status: 'error',
                attempt: 1,
                message: 'usage_http 401',
                startedAt: 1_773_344_460,
                finishedAt: 1_773_344_470,
              },
            ],
            total: 1,
            page: 1,
            perPage: 10,
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    const jobs = await fetchJobs()

    expect(jobs.page).toBe(1)
    expect(jobs.perPage).toBe(10)
    expect(jobs.items[0]).toEqual({
      id: 37696,
      job_type: 'quota_sync',
      key_id: '7QZ5',
      key_group: 'ops',
      status: 'error',
      attempt: 1,
      message: 'usage_http 401',
      started_at: 1_773_344_460,
      finished_at: 1_773_344_470,
    })
  })
})
