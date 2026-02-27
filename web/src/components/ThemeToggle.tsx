import { Check, Monitor, Moon, Sun } from 'lucide-react'

import { useLanguage } from '../i18n'
import { type ThemeMode, useTheme } from '../theme'
import { Button } from './ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from './ui/dropdown-menu'

const labels = {
  en: {
    trigger: 'Theme',
    light: 'Light',
    dark: 'Dark',
    system: 'System',
  },
  zh: {
    trigger: '主题',
    light: '浅色',
    dark: '深色',
    system: '跟随系统',
  },
} as const

function ThemeIcon({ mode }: { mode: ThemeMode }): JSX.Element {
  if (mode === 'dark') return <Moon className="h-4 w-4" aria-hidden="true" />
  if (mode === 'light') return <Sun className="h-4 w-4" aria-hidden="true" />
  return <Monitor className="h-4 w-4" aria-hidden="true" />
}

export default function ThemeToggle(): JSX.Element {
  const { language } = useLanguage()
  const copy = labels[language]
  const { mode, setMode } = useTheme()

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button type="button" variant="ghost" size="sm" className="theme-toggle-trigger">
          <ThemeIcon mode={mode} />
          <span className="theme-toggle-label">{copy.trigger}</span>
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-44">
        <DropdownMenuItem onClick={() => setMode('light')} className="cursor-pointer">
          <Sun className="mr-2 h-4 w-4" />
          <span>{copy.light}</span>
          {mode === 'light' ? <Check className="ml-auto h-4 w-4" /> : null}
        </DropdownMenuItem>
        <DropdownMenuItem onClick={() => setMode('dark')} className="cursor-pointer">
          <Moon className="mr-2 h-4 w-4" />
          <span>{copy.dark}</span>
          {mode === 'dark' ? <Check className="ml-auto h-4 w-4" /> : null}
        </DropdownMenuItem>
        <DropdownMenuItem onClick={() => setMode('system')} className="cursor-pointer">
          <Monitor className="mr-2 h-4 w-4" />
          <span>{copy.system}</span>
          {mode === 'system' ? <Check className="ml-auto h-4 w-4" /> : null}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
