import { createContext, type ReactNode, useContext, useMemo, useState } from 'react'

import type { Language, LanguageContextValue, TranslationShape } from './types'
import { translations } from './translations'

const LANGUAGE_STORAGE_KEY = 'tavily-hikari-language'
const DEFAULT_LANGUAGE: Language = 'en'
const LanguageContext = createContext<LanguageContextValue | undefined>(undefined)

function readStoredLanguage(): Language | null {
  if (typeof window === 'undefined') return null
  const stored = window.localStorage.getItem(LANGUAGE_STORAGE_KEY)
  if (stored === 'en' || stored === 'zh') return stored
  return null
}

function detectBrowserLanguage(): Language | null {
  if (typeof navigator === 'undefined') return null
  const preferred = Array.isArray(navigator.languages) ? navigator.languages : []
  const fallbacks = typeof navigator.language === 'string' ? [navigator.language] : []
  const candidates = [...preferred, ...fallbacks]

  for (const locale of candidates) {
    const normalized = locale?.toLowerCase()
    if (!normalized) continue
    if (normalized.startsWith('zh')) return 'zh'
    if (normalized.startsWith('en')) return 'en'
  }

  return null
}

function persistLanguage(language: Language): void {
  if (typeof window === 'undefined') return
  window.localStorage.setItem(LANGUAGE_STORAGE_KEY, language)
}

export type Translations = TranslationShape
export type AdminTranslations = TranslationShape['admin']

export function LanguageProvider({
  children,
  initialLanguage,
}: {
  children: ReactNode
  initialLanguage?: Language
}): JSX.Element {
  const [language, setLanguageState] = useState<Language>(
    () => initialLanguage ?? readStoredLanguage() ?? detectBrowserLanguage() ?? DEFAULT_LANGUAGE,
  )

  const setLanguage = (next: Language) => {
    setLanguageState(next)
    persistLanguage(next)
  }

  const value = useMemo(
    () => ({
      language,
      setLanguage,
    }),
    [language],
  )

  return <LanguageContext.Provider value={value}>{children}</LanguageContext.Provider>
}

export function useLanguage(): LanguageContextValue {
  const context = useContext(LanguageContext)
  if (!context) {
    throw new Error('LanguageProvider is missing. Wrap your app with LanguageProvider.')
  }
  return context
}

export function useTranslate(): Translations {
  const { language } = useLanguage()
  return translations[language]
}
