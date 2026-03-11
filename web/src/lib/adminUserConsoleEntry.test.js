import { describe, expect, it } from 'bun:test'

import { ADMIN_USER_CONSOLE_HREF } from './adminUserConsoleEntry'

describe('ADMIN_USER_CONSOLE_HREF', () => {
  it('routes admin pages back to the user console root', () => {
    expect(ADMIN_USER_CONSOLE_HREF).toBe('/console')
  })
})
