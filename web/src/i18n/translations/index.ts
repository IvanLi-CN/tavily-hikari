import type { Language, TranslationShape } from '../types'
import { EN } from './en'
import { ZH } from './zh'

export type LanguageOptionKey = 'englishLabel' | 'chineseLabel'

export const translations: Record<Language, TranslationShape> = {
  en: EN,
  zh: ZH,
}

export const languageOptions: Array<{ value: Language; labelKey: LanguageOptionKey }> = [
  { value: 'en', labelKey: 'englishLabel' },
  { value: 'zh', labelKey: 'chineseLabel' },
]
