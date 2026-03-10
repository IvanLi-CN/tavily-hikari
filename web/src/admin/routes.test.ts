import { describe, expect, it } from 'bun:test'

import {
  isSameAdminRoute,
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

  it('preserves users list filter context when building a user detail path', () => {
    expect(userDetailPath('usr_alice', 'L2', 'linuxdo_l2')).toBe('/admin/users/usr_alice?q=L2&tagId=linuxdo_l2')
    expect(userDetailPath('usr_alice')).toBe('/admin/users/usr_alice')
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
