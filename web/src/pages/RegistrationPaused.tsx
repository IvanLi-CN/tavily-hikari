import BrandLockup from '../components/BrandLockup'
import ThemeToggle from '../components/ThemeToggle'
import LanguageSwitcher from '../components/LanguageSwitcher'
import { ConnectedUpdateAvailableBanner } from '../components/UpdateAvailableBanner'
import { Button } from '../components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../components/ui/card'
import { useTranslate } from '../i18n'
import { useTheme } from '../theme'

function RegistrationPaused(): JSX.Element {
  const translations = useTranslate()
  const strings = translations.public.registrationPaused
  const { resolvedTheme } = useTheme()
  const isDark = resolvedTheme === 'dark'

  return (
    <div
      className={`min-h-screen text-foreground ${
        isDark
          ? 'bg-[radial-gradient(circle_at_top,_hsl(var(--primary)/0.12),_hsl(var(--background))_38%,_oklch(0.145_0.024_305)_78%)]'
          : 'bg-[radial-gradient(circle_at_top,_rgba(255,244,214,0.95),_rgba(255,251,235,0.88)_32%,_rgba(255,255,255,0.98)_72%)]'
      }`}
    >
      <div className="mx-auto flex w-full max-w-4xl flex-col gap-6 px-6 py-10">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="space-y-2">
            <BrandLockup
              title="Tavily Hikari"
              className="registration-paused-brand-lockup"
              markClassName="registration-paused-brand-mark"
            />
            <div
              className={`inline-flex items-center rounded-full px-3 py-1 text-xs font-semibold uppercase tracking-[0.2em] ${
                isDark
                  ? 'border border-warning/30 bg-warning/10 text-warning'
                  : 'border border-amber-300/80 bg-amber-100 text-amber-900'
              }`}
            >
              {strings.badge}
            </div>
            <h1 className={`text-3xl font-semibold tracking-tight ${isDark ? 'text-foreground' : 'text-amber-950'}`}>
              {strings.title}
            </h1>
            <p className={`max-w-2xl text-sm ${isDark ? 'text-muted-foreground' : 'text-amber-900/70'}`}>
              {strings.description}
            </p>
          </div>
          <div className="flex items-center gap-2">
            <ThemeToggle />
            <LanguageSwitcher />
          </div>
        </div>

        <ConnectedUpdateAvailableBanner strings={translations.public.updateBanner} />

        <Card
          className={`border-amber-300/60 ${
            isDark
              ? 'border-warning/30 bg-card/90 shadow-clayCard'
              : 'bg-white/90 shadow-[0_28px_80px_-44px_rgba(180,83,9,0.28)]'
          }`}
        >
          <CardHeader>
            <CardTitle className={isDark ? 'text-foreground' : 'text-amber-950'}>{strings.badge}</CardTitle>
            <CardDescription className={isDark ? 'text-muted-foreground' : 'text-amber-900/70'}>
              {strings.description}
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-5">
            <div
              className={`rounded-2xl border p-4 text-sm leading-6 ${
                isDark
                  ? 'border-warning/25 bg-warning/10 text-foreground'
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
