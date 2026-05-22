export type AdminDisplayDensity = 'comfortable' | 'compact'

export const ADMIN_DISPLAY_DENSITY_STORAGE_KEY = 'tavily-hikari-admin-display-density'

export function normalizeAdminDisplayDensity(value: string | null | undefined): AdminDisplayDensity | null {
  if (value === 'comfortable' || value === 'compact') return value
  return null
}

export function readStoredAdminDisplayDensity(): AdminDisplayDensity {
  if (typeof window === 'undefined') return 'comfortable'
  try {
    return normalizeAdminDisplayDensity(window.localStorage.getItem(ADMIN_DISPLAY_DENSITY_STORAGE_KEY)) ?? 'comfortable'
  } catch {
    return 'comfortable'
  }
}

export function persistAdminDisplayDensity(density: AdminDisplayDensity): void {
  if (typeof window === 'undefined') return
  try {
    window.localStorage.setItem(ADMIN_DISPLAY_DENSITY_STORAGE_KEY, density)
  } catch {
    // Private-mode or embedded contexts may reject storage writes.
  }
}

export function applyAdminDisplayDensity(density: AdminDisplayDensity): void {
  if (typeof document === 'undefined') return
  document.documentElement.dataset.adminDensity = density
}
