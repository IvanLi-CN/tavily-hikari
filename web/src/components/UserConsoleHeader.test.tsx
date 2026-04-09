import { describe, expect, it } from 'bun:test'
import type { ReactElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import { LanguageProvider } from '../i18n'
import { ThemeProvider } from '../theme'
import UserConsoleHeader from './UserConsoleHeader'

function renderWithProviders(node: ReactElement): string {
  return renderToStaticMarkup(
    <LanguageProvider>
      <ThemeProvider>{node}</ThemeProvider>
    </LanguageProvider>
  )
}

describe('UserConsoleHeader', () => {
  it('renders the hero cards, admin CTA, and logout action together', () => {
    const html = renderWithProviders(
      <UserConsoleHeader
        title="User Console"
        subtitle="Your account dashboard and token management"
        eyebrow="User Workspace"
        currentViewLabel="Current View"
        currentViewTitle="Token Detail"
        currentViewDescription="Same token-level modules as home page."
        sessionLabel="Signed in as"
        sessionDisplayName="Ivan"
        sessionProviderLabel="LinuxDo"
        adminLabel="Admin"
        isAdmin
        adminHref="/admin"
        adminActionLabel="Open Admin Dashboard"
        logoutVisible
        isLoggingOut={false}
        logoutLabel="Sign out"
        loggingOutLabel="Signing out…"
        onLogout={() => undefined}
      />
    )

    expect(html).toContain('User Console')
    expect(html).toContain('Current View')
    expect(html).toContain('Token Detail')
    expect(html).toContain('Ivan')
    expect(html).toContain('LinuxDo')
    expect(html).toContain('Open Admin Dashboard')
    expect(html).toContain('Sign out')
    expect(html).toContain('user-console-header')
  })

  it('keeps the admin CTA but hides logout when no user session is available', () => {
    const html = renderWithProviders(
      <UserConsoleHeader
        title="User Console"
        subtitle="Your account dashboard and token management"
        eyebrow="User Workspace"
        currentViewLabel="Current View"
        currentViewTitle="Account Overview"
        currentViewDescription="Track account-level quotas."
        sessionLabel="Signed in as"
        sessionDisplayName="dev-mode"
        adminLabel="Admin"
        isAdmin
        adminHref="/admin"
        adminActionLabel="Open Admin Dashboard"
        logoutVisible={false}
        isLoggingOut={false}
        logoutLabel="Sign out"
        loggingOutLabel="Signing out…"
        onLogout={() => undefined}
      />
    )

    expect(html).toContain('Open Admin Dashboard')
    expect(html).not.toContain('Sign out')
  })
})
