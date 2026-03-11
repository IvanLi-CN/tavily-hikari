import { describe, expect, it } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'

import { LanguageProvider } from '../i18n'
import { ADMIN_USER_CONSOLE_HREF } from '../lib/adminUserConsoleEntry'
import { ThemeProvider } from '../theme'
import AdminPanelHeader from './AdminPanelHeader'
import TokenUsageHeader from './TokenUsageHeader'

function renderWithProviders(node: JSX.Element): string {
  return renderToStaticMarkup(
    <LanguageProvider>
      <ThemeProvider>{node}</ThemeProvider>
    </LanguageProvider>,
  )
}

describe('admin return-to-console CTA', () => {
  it('renders the shared href in the admin dashboard header', () => {
    const html = renderWithProviders(
      <AdminPanelHeader
        title="Overview"
        subtitle="Monitor system health."
        displayName="Ops Admin"
        isAdmin
        updatedPrefix="Updated"
        updatedTime="11:42:10"
        isRefreshing={false}
        refreshLabel="Refresh"
        refreshingLabel="Refreshing"
        userConsoleLabel="Back to User Console"
        userConsoleHref={ADMIN_USER_CONSOLE_HREF}
        onRefresh={() => undefined}
      />,
    )

    expect(html).toContain('Back to User Console')
    expect(html).toContain(`href="${ADMIN_USER_CONSOLE_HREF}"`)
    expect(html).toContain('admin-return-link')
  })

  it('renders the shared href in the leaderboard/detail-style header', () => {
    const html = renderWithProviders(
      <TokenUsageHeader
        title="Token Usage"
        subtitle="Focus on heavy hitters."
        backLabel="Back"
        refreshLabel="Refresh"
        refreshingLabel="Refreshing"
        userConsoleLabel="Back to User Console"
        userConsoleHref={ADMIN_USER_CONSOLE_HREF}
        isRefreshing={false}
        period="day"
        focus="usage"
        periodOptions={[
          { value: 'day', label: 'Today' },
          { value: 'month', label: 'Month' },
          { value: 'all', label: 'All time' },
        ]}
        focusOptions={[
          { value: 'usage', label: 'Usage' },
          { value: 'errors', label: 'Errors' },
          { value: 'other', label: 'Other' },
        ]}
        onBack={() => undefined}
        onRefresh={() => undefined}
        onPeriodChange={() => undefined}
        onFocusChange={() => undefined}
      />,
    )

    expect(html).toContain('Back to User Console')
    expect(html).toContain(`href="${ADMIN_USER_CONSOLE_HREF}"`)
    expect(html).toContain('token-usage-header')
  })
})
