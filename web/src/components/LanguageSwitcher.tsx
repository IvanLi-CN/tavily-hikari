import { Icon } from '@iconify/react'
import { useLanguage, useTranslate, languageOptions, type Language } from '../i18n'

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
    <div className="dropdown dropdown-end language-switcher">
      <label
        tabIndex={0}
        className="btn btn-ghost btn-sm language-switcher-trigger"
        aria-label={`${strings.common.languageLabel}: ${strings.common[language === 'en' ? 'englishLabel' : 'chineseLabel']}`}
      >
        <span className="sr-only">{strings.common.languageLabel}</span>
        <span className="language-flag" aria-hidden="true">
          <Icon icon={activeMeta.icon} width={18} height={18} />
        </span>
        <span className="language-short">{activeMeta.short}</span>
        <Icon icon="mdi:chevron-down" width={16} height={16} aria-hidden="true" />
      </label>
      <ul tabIndex={0} className="dropdown-content menu menu-sm bg-base-100 rounded-box shadow language-switcher-menu">
        {languageOptions.map((option) => {
          const meta = LANGUAGE_META[option.value]
          const isActive = option.value === language
          return (
            <li key={option.value}>
              <button
                type="button"
                className={`language-option${isActive ? ' active' : ''}`}
                onClick={() => handleSelect(option.value as Language)}
              >
                <span className="language-flag" aria-hidden="true">
                  <Icon icon={meta.icon} width={18} height={18} />
                </span>
                <span className="language-short">{meta.short}</span>
                <span className="language-full">{strings.common[option.labelKey]}</span>
              </button>
            </li>
          )
        })}
      </ul>
    </div>
  )
}

export default LanguageSwitcher
