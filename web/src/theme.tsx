import { createContext, type ReactNode, useContext, useEffect, useMemo, useState } from 'react'

export type ThemeMode = 'light' | 'dark' | 'system'
export type ResolvedTheme = 'light' | 'dark'

const THEME_STORAGE_KEY = 'tavily-hikari-theme-mode'

interface ThemeContextValue {
  mode: ThemeMode
  resolvedTheme: ResolvedTheme
  setMode: (mode: ThemeMode) => void
}

const ThemeContext = createContext<ThemeContextValue | undefined>(undefined)

function readStoredThemeMode(): ThemeMode | null {
  if (typeof window === 'undefined') return null
  const value = window.localStorage.getItem(THEME_STORAGE_KEY)
  if (value === 'light' || value === 'dark' || value === 'system') return value
  return null
}

function getSystemTheme(): ResolvedTheme {
  if (typeof window === 'undefined') return 'light'
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

function resolveTheme(mode: ThemeMode): ResolvedTheme {
  return mode === 'system' ? getSystemTheme() : mode
}

function persistThemeMode(mode: ThemeMode): void {
  if (typeof window === 'undefined') return
  window.localStorage.setItem(THEME_STORAGE_KEY, mode)
}

function applyTheme(resolvedTheme: ResolvedTheme): void {
  if (typeof document === 'undefined') return
  const root = document.documentElement
  root.classList.toggle('dark', resolvedTheme === 'dark')
  root.style.colorScheme = resolvedTheme
}

export function ThemeProvider({ children }: { children: ReactNode }): JSX.Element {
  const [mode, setModeState] = useState<ThemeMode>(() => readStoredThemeMode() ?? 'system')
  const [resolvedTheme, setResolvedTheme] = useState<ResolvedTheme>(() => resolveTheme(readStoredThemeMode() ?? 'system'))

  useEffect(() => {
    const next = resolveTheme(mode)
    setResolvedTheme(next)
    applyTheme(next)
    persistThemeMode(mode)
  }, [mode])

  useEffect(() => {
    if (typeof window === 'undefined') return
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const onChange = () => {
      if (mode !== 'system') return
      const next = getSystemTheme()
      setResolvedTheme(next)
      applyTheme(next)
    }
    media.addEventListener('change', onChange)
    return () => media.removeEventListener('change', onChange)
  }, [mode])

  const value = useMemo<ThemeContextValue>(
    () => ({
      mode,
      resolvedTheme,
      setMode: (next) => setModeState(next),
    }),
    [mode, resolvedTheme],
  )

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
}

export function useTheme(): ThemeContextValue {
  const context = useContext(ThemeContext)
  if (!context) {
    throw new Error('ThemeProvider is missing. Wrap your app with ThemeProvider.')
  }
  return context
}
