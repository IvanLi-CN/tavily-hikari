import ThemeToggle from '../components/ThemeToggle'
import LanguageSwitcher from '../components/LanguageSwitcher'
import { Button } from '../components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../components/ui/card'
import { useTranslate } from '../i18n'
import { useTheme } from '../theme'

function RegistrationPaused(): JSX.Element {
  const strings = useTranslate().public.registrationPaused
  const { resolvedTheme } = useTheme()
  const isDark = resolvedTheme === 'dark'

  return (
    <div
      className={`min-h-screen text-foreground ${
        isDark
          ? 'bg-[radial-gradient(circle_at_top,_rgba(31,64,175,0.26),_rgba(9,18,38,0.98)_34%,_rgba(3,7,18,1)_72%)]'
          : 'bg-[radial-gradient(circle_at_top,_rgba(255,244,214,0.95),_rgba(255,251,235,0.88)_32%,_rgba(255,255,255,0.98)_72%)]'
      }`}
    >
      <div className="mx-auto flex w-full max-w-4xl flex-col gap-6 px-6 py-10">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="space-y-2">
            <div
              className={`inline-flex items-center rounded-full px-3 py-1 text-xs font-semibold uppercase tracking-[0.2em] ${
                isDark
                  ? 'border border-amber-300/30 bg-amber-200/10 text-amber-200'
                  : 'border border-amber-300/80 bg-amber-100 text-amber-900'
              }`}
            >
              {strings.badge}
            </div>
            <h1 className={`text-3xl font-semibold tracking-tight ${isDark ? 'text-amber-50' : 'text-amber-950'}`}>
              {strings.title}
            </h1>
            <p className={`max-w-2xl text-sm ${isDark ? 'text-slate-300' : 'text-amber-900/70'}`}>
              {strings.description}
            </p>
          </div>
          <div className="flex items-center gap-2">
            <ThemeToggle />
            <LanguageSwitcher />
          </div>
        </div>

        <Card
          className={`border-amber-300/60 backdrop-blur ${
            isDark
              ? 'border-amber-300/22 bg-slate-950/72 shadow-[0_28px_80px_-52px_rgba(245,158,11,0.28)]'
              : 'bg-white/90 shadow-[0_28px_80px_-44px_rgba(180,83,9,0.28)]'
          }`}
        >
          <CardHeader>
            <CardTitle className={isDark ? 'text-amber-50' : 'text-amber-950'}>{strings.title}</CardTitle>
            <CardDescription className={isDark ? 'text-slate-300' : 'text-amber-900/70'}>
              {strings.description}
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-5">
            <div
              className={`rounded-2xl border p-4 text-sm leading-6 ${
                isDark
                  ? 'border-amber-300/18 bg-amber-200/8 text-amber-50/88'
                  : 'border-amber-200 bg-amber-50/80 text-amber-950/85'
              }`}
            >
              {strings.continueHint}
            </div>
            <div className="flex flex-wrap items-center justify-end gap-3">
              <Button asChild>
                <a href="/">{strings.returnHome}</a>
              </Button>
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  )
}

export default RegistrationPaused
