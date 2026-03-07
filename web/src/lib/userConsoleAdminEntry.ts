import type { Profile } from '../api'

export const USER_CONSOLE_ADMIN_HREF = '/admin' as const

export function getUserConsoleAdminHref(
  profile: Pick<Profile, 'isAdmin'> | null | undefined,
): typeof USER_CONSOLE_ADMIN_HREF | null {
  return profile?.isAdmin ? USER_CONSOLE_ADMIN_HREF : null
}
