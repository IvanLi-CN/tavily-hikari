import { Icon } from '@iconify/react'

import { languageOptions, type Language, useLanguage, useTranslate } from '../i18n'
import { Button } from './ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from './ui/dropdown-menu'

const LANGUAGE_META: Record<Language, { icon: string; short: string }> = {
  en: { icon: 'twemoji:flag-united-kingdom', short: 'EN' },
  zh: { icon: 'twemoji:flag-china', short: '中文' },
}

function LanguageSwitcher(): JSX.Element {
  const { language, setLanguage } = useLanguage()
  const strings = useTranslate()
  const activeMeta = LANGUAGE_META[language]

  const handleSelect = (next: Language) => {
    if (next === language) return
    setLanguage(next)
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="language-switcher-trigger"
          aria-label={`${strings.common.languageLabel}: ${strings.common[language === 'en' ? 'englishLabel' : 'chineseLabel']}`}
        >
          <span className="sr-only">{strings.common.languageLabel}</span>
          <span className="language-flag" aria-hidden="true">
            <Icon icon={activeMeta.icon} width={18} height={18} />
          </span>
          <span className="language-short">{activeMeta.short}</span>
          <Icon icon="mdi:chevron-down" width={16} height={16} aria-hidden="true" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="language-switcher-menu w-44 p-1">
        {languageOptions.map((option) => {
          const meta = LANGUAGE_META[option.value]
          const isActive = option.value === language
          return (
            <DropdownMenuItem
              key={option.value}
              className={`language-option cursor-pointer ${isActive ? 'active' : ''}`}
              onClick={() => handleSelect(option.value as Language)}
            >
              <span className="language-flag" aria-hidden="true">
                <Icon icon={meta.icon} width={18} height={18} />
              </span>
              <span className="language-full">{strings.common[option.labelKey]}</span>
            </DropdownMenuItem>
          )
        })}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

export default LanguageSwitcher
