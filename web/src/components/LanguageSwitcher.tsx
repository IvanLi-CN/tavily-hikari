import { useEffect, useRef, useState } from 'react'
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
  const dropdownRef = useRef<HTMLDivElement | null>(null)
  const [isOpen, setIsOpen] = useState(false)

  useEffect(() => {
    if (!isOpen) return
    if (typeof document === 'undefined') return

    const handleOutsideClick = (event: MouseEvent) => {
      const dropdownElement = dropdownRef.current
      if (!dropdownElement) return
      if (dropdownElement.contains(event.target as Node)) return
      setIsOpen(false)
    }

    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setIsOpen(false)
      }
    }

    document.addEventListener('mousedown', handleOutsideClick)
    document.addEventListener('keydown', handleEscape)
    return () => {
      document.removeEventListener('mousedown', handleOutsideClick)
      document.removeEventListener('keydown', handleEscape)
    }
  }, [isOpen])

  const toggleDropdown = () => {
    setIsOpen((prev) => !prev)
  }

  const handleSelect = (next: Language) => {
    setIsOpen(false)
    if (next === language) return
    setLanguage(next)
  }

  return (
    <div ref={dropdownRef} className={`dropdown dropdown-end language-switcher${isOpen ? ' dropdown-open' : ''}`}>
      <button
        type="button"
        className="btn btn-ghost btn-sm language-switcher-trigger"
        aria-label={`${strings.common.languageLabel}: ${strings.common[language === 'en' ? 'englishLabel' : 'chineseLabel']}`}
        aria-haspopup="listbox"
        aria-expanded={isOpen}
        onClick={toggleDropdown}
      >
        <span className="sr-only">{strings.common.languageLabel}</span>
        <span className="language-flag" aria-hidden="true">
          <Icon icon={activeMeta.icon} width={18} height={18} />
        </span>
        <span className="language-short">{activeMeta.short}</span>
        <Icon icon="mdi:chevron-down" width={16} height={16} aria-hidden="true" />
      </button>
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
