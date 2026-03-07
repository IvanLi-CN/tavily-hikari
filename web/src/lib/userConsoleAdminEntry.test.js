import { describe, expect, it } from 'bun:test'

import { getUserConsoleAdminHref, USER_CONSOLE_ADMIN_HREF } from './userConsoleAdminEntry'

describe('getUserConsoleAdminHref', () => {
  it('hides the admin entry when the session is not an admin', () => {
    expect(getUserConsoleAdminHref(undefined)).toBeNull()
    expect(getUserConsoleAdminHref(null)).toBeNull()
    expect(getUserConsoleAdminHref({ isAdmin: false })).toBeNull()
  })

  it('routes admin sessions to the admin dashboard', () => {
    expect(getUserConsoleAdminHref({ isAdmin: true })).toBe(USER_CONSOLE_ADMIN_HREF)
  })
})
