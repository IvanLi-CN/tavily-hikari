import { describe, expect, it } from 'bun:test'

import {
  buildAdminKeysPath,
  buildAdminUsersPath,
  isSameAdminRoute,
  keyDetailPath,
  parseAdminPath,
  tokenDetailPath,
  unboundTokenUsagePath,
  userDetailPath,
  userTagCreatePath,
  userTagEditPath,
  userTagsPath,
  userUsagePath,
} from './routes'

describe('admin user tag routes', () => {
  it('parses the user tag index before user detail fallback', () => {
    expect(parseAdminPath('/admin/users/tags')).toEqual({ name: 'user-tags' })
  })

  it('parses the dedicated user usage page before user detail fallback', () => {
    expect(parseAdminPath('/admin/users/usage')).toEqual({ name: 'user-usage' })
  })

  it('parses the unbound token usage page before token detail fallback', () => {
    expect(parseAdminPath('/admin/tokens/leaderboard')).toEqual({ name: 'unbound-token-usage' })
  })

  it('parses the system settings module route', () => {
    expect(parseAdminPath('/admin/system-settings')).toEqual({ name: 'module', module: 'system-settings' })
  })

  it('parses the user tag create page', () => {
    expect(parseAdminPath('/admin/users/tags/new')).toEqual({ name: 'user-tag-editor', mode: 'create' })
  })

  it('parses the user tag edit page without colliding with user detail routes', () => {
    expect(parseAdminPath('/admin/users/tags/linuxdo_l2')).toEqual({
      name: 'user-tag-editor',
      mode: 'edit',
      id: 'linuxdo_l2',
    })
    expect(parseAdminPath('/admin/users/usr_alice')).toEqual({ name: 'user', id: 'usr_alice' })
  })

  it('builds stable user tag management paths', () => {
    expect(userTagsPath()).toBe('/admin/users/tags')
    expect(userUsagePath()).toBe('/admin/users/usage')
    expect(unboundTokenUsagePath()).toBe('/admin/tokens/leaderboard')
    expect(unboundTokenUsagePath('ops', 2, 'quotaMonthlyUsed', 'asc')).toBe(
      '/admin/tokens/leaderboard?q=ops&page=2&sort=quotaMonthlyUsed&order=asc',
    )
    expect(tokenDetailPath('tok 42')).toBe('/admin/tokens/tok%2042')
    expect(tokenDetailPath('tok 42', 'ops', 2, 'quotaMonthlyUsed', 'asc', 'unbound-usage')).toBe(
      '/admin/tokens/tok%2042?q=ops&page=2&sort=quotaMonthlyUsed&order=asc&view=unbound-usage',
    )
    expect(userTagCreatePath()).toBe('/admin/users/tags/new')
    expect(userTagEditPath('linuxdo l2')).toBe('/admin/users/tags/linuxdo%20l2')
  })

  it('preserves full users list context when building cross-page routes', () => {
    expect(buildAdminUsersPath('L2', 'linuxdo_l2', 3, 'monthlySuccessRate', 'asc')).toBe(
      '/admin/users?q=L2&tagId=linuxdo_l2&page=3&sort=monthlySuccessRate&order=asc',
    )
    expect(userDetailPath('usr_alice', 'L2', 'linuxdo_l2', 3, 'monthlySuccessRate', 'asc')).toBe(
      '/admin/users/usr_alice?q=L2&tagId=linuxdo_l2&page=3&sort=monthlySuccessRate&order=asc',
    )
    expect(userTagsPath('L2', 'linuxdo_l2', 3, 'monthlySuccessRate', 'asc')).toBe(
      '/admin/users/tags?q=L2&tagId=linuxdo_l2&page=3&sort=monthlySuccessRate&order=asc',
    )
    expect(userUsagePath('L2', 'linuxdo_l2', 3, 'monthlySuccessRate', 'asc')).toBe(
      '/admin/users/usage?q=L2&tagId=linuxdo_l2&page=3&sort=monthlySuccessRate&order=asc',
    )
    expect(userTagCreatePath('L2', 'linuxdo_l2', 3, 'monthlySuccessRate', 'asc')).toBe(
      '/admin/users/tags/new?q=L2&tagId=linuxdo_l2&page=3&sort=monthlySuccessRate&order=asc',
    )
    expect(userTagEditPath('linuxdo l2', 'L2', 'linuxdo_l2', 3, 'monthlySuccessRate', 'asc')).toBe(
      '/admin/users/tags/linuxdo%20l2?q=L2&tagId=linuxdo_l2&page=3&sort=monthlySuccessRate&order=asc',
    )
    expect(userDetailPath('usr_alice')).toBe('/admin/users/usr_alice')
  })

  it('marks detail and tag routes that were opened from the usage view', () => {
    expect(userDetailPath('usr_alice', 'L2', 'linuxdo_l2', 3, 'monthlySuccessRate', 'asc', 'usage')).toBe(
      '/admin/users/usr_alice?q=L2&tagId=linuxdo_l2&page=3&sort=monthlySuccessRate&order=asc&view=usage',
    )
    expect(userTagsPath('L2', 'linuxdo_l2', 3, 'monthlySuccessRate', 'asc', 'usage')).toBe(
      '/admin/users/tags?q=L2&tagId=linuxdo_l2&page=3&sort=monthlySuccessRate&order=asc&view=usage',
    )
  })

  it('builds stable key list paths with pagination and repeated filters', () => {
    expect(buildAdminKeysPath()).toBe('/admin/keys')
    expect(
      buildAdminKeysPath({
        page: 2,
        perPage: 50,
        groups: ['ops', '', 'ops'],
        statuses: ['active', 'Quarantined', 'active'],
        registrationIp: '8.8.8.8',
        regions: ['US', 'US', 'US Westfield (MA)'],
      }),
    ).toBe(
      '/admin/keys?page=2&perPage=50&group=ops&group=&status=active&status=quarantined&registrationIp=8.8.8.8&region=US&region=US+Westfield+%28MA%29',
    )
    expect(
      keyDetailPath('key 42', {
        page: 3,
        perPage: 100,
        groups: ['ops'],
        statuses: ['disabled'],
        registrationIp: '8.8.4.4',
        regions: ['US Westfield (MA)'],
      }),
    ).toBe(
      '/admin/keys/key%2042?page=3&perPage=100&group=ops&status=disabled&registrationIp=8.8.4.4&region=US+Westfield+%28MA%29',
    )
  })

  it('compares user tag editor routes by mode and id', () => {
    expect(
      isSameAdminRoute(
        { name: 'user-tag-editor', mode: 'create' },
        { name: 'user-tag-editor', mode: 'create' },
      ),
    ).toBe(true)
    expect(
      isSameAdminRoute(
        { name: 'user-tag-editor', mode: 'edit', id: 'tag-a' },
        { name: 'user-tag-editor', mode: 'edit', id: 'tag-b' },
      ),
    ).toBe(false)
  })

  it('compares user usage routes as the same logical page', () => {
    expect(isSameAdminRoute({ name: 'user-usage' }, { name: 'user-usage' })).toBe(true)
  })
})
