import '../../test/happydom'

import { afterEach, describe, expect, it } from 'bun:test'
import { act, type ReactElement } from 'react'
import { createRoot } from 'react-dom/client'
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

function wrapWithProviders(node: ReactElement): ReactElement {
  return (
    <LanguageProvider>
      <ThemeProvider>{node}</ThemeProvider>
    </LanguageProvider>
  )
}

afterEach(() => {
  document.body.innerHTML = ''
})

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
    expect(html).toContain('User Workspace')
    expect(html).toContain('Your account dashboard and token management')
    expect(html).toContain('Same token-level modules as home page.')
    expect(html).toContain('Signed in as: Ivan')
    expect(html).toContain('user-console-header-inline-meta')
    expect(html).toContain('user-console-header-context')
    expect(html).toContain('user-console-account-trigger')
    expect(html).toContain('user-console-account-avatar-image')
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
    expect(html).toContain('User Workspace')
    expect(html).toContain('Track account-level quotas.')
    expect(html).toContain('user-console-account-trigger')
    expect(html).not.toContain('Sign out')
  })

  it('retries avatar rendering after a broken image url is replaced', async () => {
    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)

    await act(async () => {
      root.render(wrapWithProviders(
        <UserConsoleHeader
          title="User Console"
          subtitle="Your account dashboard and token management"
          eyebrow="User Workspace"
          currentViewLabel="Current View"
          currentViewTitle="Account Overview"
          currentViewDescription="Track account-level quotas."
          sessionLabel="Signed in as"
          sessionDisplayName="Ivan"
          sessionProviderLabel="LinuxDo"
          sessionAvatarUrl="https://broken.example/avatar.png"
          adminLabel="Admin"
          isAdmin={false}
          logoutVisible
          isLoggingOut={false}
          logoutLabel="Sign out"
          loggingOutLabel="Signing out…"
          onLogout={() => undefined}
        />,
      ))
    })

    const dispatchImageError = () => {
      const image = container.querySelector<HTMLImageElement>('.user-console-account-trigger img')
      expect(image?.getAttribute('src')).toBe('https://broken.example/avatar.png')
      image?.dispatchEvent(new Event('error', { bubbles: true }))
    }

    await act(async () => {
      dispatchImageError()
    })

    expect(container.querySelector('.user-console-account-trigger img')).toBeNull()
    expect(
      container.querySelector('.user-console-account-trigger .user-console-account-avatar-fallback')?.textContent,
    ).toBe('I')

    await act(async () => {
      root.render(wrapWithProviders(
        <UserConsoleHeader
          title="User Console"
          subtitle="Your account dashboard and token management"
          eyebrow="User Workspace"
          currentViewLabel="Current View"
          currentViewTitle="Account Overview"
          currentViewDescription="Track account-level quotas."
          sessionLabel="Signed in as"
          sessionDisplayName="Ivan"
          sessionProviderLabel="LinuxDo"
          sessionAvatarUrl="https://connect.linux.do/user_avatar/connect.linux.do/ivan/96/1.png"
          adminLabel="Admin"
          isAdmin={false}
          logoutVisible
          isLoggingOut={false}
          logoutLabel="Sign out"
          loggingOutLabel="Signing out…"
          onLogout={() => undefined}
        />,
      ))
    })

    expect(
      container.querySelector<HTMLImageElement>('.user-console-account-trigger img')?.getAttribute('src'),
    ).toBe('https://connect.linux.do/user_avatar/connect.linux.do/ivan/96/1.png')

    await act(async () => {
      root.unmount()
    })
  })
})
