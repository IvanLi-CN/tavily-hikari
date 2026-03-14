import { describe, expect, it } from 'bun:test'

import {
  buildAdminKeysPath,
  buildAdminUsersPath,
  isSameAdminRoute,
  keyDetailPath,
  parseAdminPath,
  userDetailPath,
  userTagCreatePath,
  userTagEditPath,
  userTagsPath,
} from './routes'

describe('admin user tag routes', () => {
  it('parses the user tag index before user detail fallback', () => {
    expect(parseAdminPath('/admin/users/tags')).toEqual({ name: 'user-tags' })
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
    expect(userTagCreatePath()).toBe('/admin/users/tags/new')
    expect(userTagEditPath('linuxdo l2')).toBe('/admin/users/tags/linuxdo%20l2')
  })

  it('preserves full users list context when building cross-page routes', () => {
    expect(buildAdminUsersPath('L2', 'linuxdo_l2', 3)).toBe('/admin/users?q=L2&tagId=linuxdo_l2&page=3')
    expect(userDetailPath('usr_alice', 'L2', 'linuxdo_l2', 3)).toBe(
      '/admin/users/usr_alice?q=L2&tagId=linuxdo_l2&page=3',
    )
    expect(userTagsPath('L2', 'linuxdo_l2', 3)).toBe('/admin/users/tags?q=L2&tagId=linuxdo_l2&page=3')
    expect(userTagCreatePath('L2', 'linuxdo_l2', 3)).toBe(
      '/admin/users/tags/new?q=L2&tagId=linuxdo_l2&page=3',
    )
    expect(userTagEditPath('linuxdo l2', 'L2', 'linuxdo_l2', 3)).toBe(
      '/admin/users/tags/linuxdo%20l2?q=L2&tagId=linuxdo_l2&page=3',
    )
    expect(userDetailPath('usr_alice')).toBe('/admin/users/usr_alice')
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
        regions: ['US California', 'US California', 'JP Tokyo'],
      }),
    ).toBe(
      '/admin/keys?page=2&perPage=50&group=ops&group=&status=active&status=quarantined&registrationIp=8.8.8.8&region=US+California&region=JP+Tokyo',
    )
    expect(
      keyDetailPath('key 42', {
        page: 3,
        perPage: 100,
        groups: ['ops'],
        statuses: ['disabled'],
        registrationIp: '1.1.1.1',
        regions: ['AU Sydney'],
      }),
    ).toBe('/admin/keys/key%2042?page=3&perPage=100&group=ops&status=disabled&registrationIp=1.1.1.1&region=AU+Sydney')
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
})
