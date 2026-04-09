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
  it('renders a compact header with inline context and an account trigger', () => {
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
        sessionAvatarUrl="https://connect.linux.do/user_avatar/connect.linux.do/ivan/96/1.png"
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
    expect(html).toContain('Token Detail')
    expect(html).toContain('Signed in as: Ivan')
    expect(html).toContain('user-console-header-inline-meta')
    expect(html).toContain('user-console-account-trigger')
    expect(html).toContain('user-console-account-avatar-image')
    expect(html).not.toContain('Your account dashboard and token management')
  })

  it('keeps the account trigger but omits sign out when no user session is available', () => {
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

    expect(html).toContain('Signed in as: dev-mode')
    expect(html).toContain('user-console-account-trigger')
    expect(html).not.toContain('Sign out')
  })
})
