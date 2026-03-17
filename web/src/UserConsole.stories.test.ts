import { describe, expect, it } from 'bun:test'

import meta, * as userConsoleStories from './UserConsole.stories'

describe('UserConsole Storybook acceptance controls', () => {
  it('exposes only acceptance-facing controls', () => {
    expect(meta.args).toEqual({
      consoleView: 'Console Home',
      isAdmin: false,
      landingFocus: 'Overview Focus',
      tokenListState: 'Single Token',
      tokenDetailPreview: 'Overview',
    })

    expect(meta.argTypes).not.toHaveProperty('scenario')

    expect(meta.argTypes?.consoleView).toMatchObject({
      name: 'Console view',
      options: ['Console Home', 'Token Detail'],
      control: { type: 'inline-radio' },
    })

    expect(meta.argTypes?.isAdmin).toMatchObject({
      name: 'Admin session',
      control: { type: 'boolean' },
    })

    expect(meta.argTypes?.landingFocus).toMatchObject({
      name: 'Landing focus',
      options: ['Overview Focus', 'Token Focus'],
      control: { type: 'inline-radio' },
      if: { arg: 'consoleView', eq: 'Console Home' },
    })

    expect(meta.argTypes?.tokenListState).toMatchObject({
      name: 'Token list state',
      options: ['Single Token', 'Multiple Tokens', 'Empty'],
      control: { type: 'inline-radio' },
      if: { arg: 'consoleView', eq: 'Console Home' },
    })

    expect(meta.argTypes?.tokenDetailPreview).toMatchObject({
      name: 'Token detail preview',
      options: [
        'Overview',
        'Token Revealed',
        'API Check Running',
        'All Checks Pass',
        'Partial Availability',
        'Authentication Failed',
        'Quota Blocked',
      ],
      control: { type: 'select' },
      if: { arg: 'consoleView', eq: 'Token Detail' },
    })

    expect(meta.argTypes?.routeHashOverride).toMatchObject({
      table: { disable: true },
      control: false,
    })
  })

  it('keeps business-readable preset stories and drops legacy scenario exports', () => {
    expect(userConsoleStories.ConsoleHome.args).toMatchObject({
      consoleView: 'Console Home',
      isAdmin: false,
      landingFocus: 'Overview Focus',
    })
    expect(userConsoleStories.ConsoleHomeRoot).toMatchObject({
      name: 'Console Home Root',
      args: { consoleView: 'Console Home', isAdmin: false, landingFocus: 'Overview Focus', routeHashOverride: '' },
    })
    expect(userConsoleStories.ConsoleHomeAdmin).toMatchObject({
      name: 'Console Home Admin',
      args: { consoleView: 'Console Home', isAdmin: true, landingFocus: 'Overview Focus' },
    })
    expect(userConsoleStories.ConsoleHomeAdminMobile).toMatchObject({
      name: 'Console Home Admin Mobile',
      args: { consoleView: 'Console Home', isAdmin: true, landingFocus: 'Overview Focus' },
    })
    expect(userConsoleStories.ConsoleHomeTokensFocus).toMatchObject({
      name: 'Console Home Tokens Focus',
      args: { consoleView: 'Console Home', isAdmin: false, landingFocus: 'Token Focus', tokenListState: 'Single Token' },
    })
    expect(userConsoleStories.ConsoleHomeTokensFocusAdmin).toMatchObject({
      name: 'Console Home Tokens Focus Admin',
      args: { consoleView: 'Console Home', isAdmin: true, landingFocus: 'Token Focus', tokenListState: 'Single Token' },
    })
    expect(userConsoleStories.ConsoleHomeMultipleTokens).toMatchObject({
      name: 'Console Home Multiple Tokens',
      args: { consoleView: 'Console Home', isAdmin: false, landingFocus: 'Token Focus', tokenListState: 'Multiple Tokens' },
    })
    expect(userConsoleStories.ConsoleHomeEmptyTokens).toMatchObject({
      name: 'Console Home Empty Tokens',
      args: { consoleView: 'Console Home', landingFocus: 'Token Focus', tokenListState: 'Empty' },
    })
    expect(userConsoleStories.TokenDetailOverview).toMatchObject({
      name: 'Token Detail Overview',
      args: { consoleView: 'Token Detail', isAdmin: false, landingFocus: 'Overview Focus', tokenDetailPreview: 'Overview' },
    })
    expect(userConsoleStories.TokenRevealed).toMatchObject({
      name: 'Token Revealed',
      args: { consoleView: 'Token Detail', isAdmin: false, tokenDetailPreview: 'Token Revealed' },
    })
    expect(userConsoleStories.TokenDetailAdmin).toMatchObject({
      name: 'Token Detail Admin',
      args: { consoleView: 'Token Detail', isAdmin: true, landingFocus: 'Overview Focus', tokenDetailPreview: 'Overview' },
    })
    expect(userConsoleStories.ApiCheckRunning).toMatchObject({
      name: 'API Check Running',
      args: { consoleView: 'Token Detail', landingFocus: 'Overview Focus', tokenDetailPreview: 'API Check Running' },
    })
    expect(userConsoleStories.AllChecksPass).toMatchObject({
      name: 'All Checks Pass',
      args: { consoleView: 'Token Detail', landingFocus: 'Overview Focus', tokenDetailPreview: 'All Checks Pass' },
    })
    expect(userConsoleStories.PartialAvailability).toMatchObject({
      name: 'Partial Availability',
      args: { consoleView: 'Token Detail', landingFocus: 'Overview Focus', tokenDetailPreview: 'Partial Availability' },
    })
    expect(userConsoleStories.AuthenticationFailed).toMatchObject({
      name: 'Authentication Failed',
      args: { consoleView: 'Token Detail', landingFocus: 'Overview Focus', tokenDetailPreview: 'Authentication Failed' },
    })
    expect(userConsoleStories.QuotaBlocked).toMatchObject({
      name: 'Quota Blocked',
      args: { consoleView: 'Token Detail', landingFocus: 'Overview Focus', tokenDetailPreview: 'Quota Blocked' },
    })

    expect(userConsoleStories).not.toHaveProperty('Dashboard')
    expect(userConsoleStories).not.toHaveProperty('DashboardAdmin')
    expect(userConsoleStories).not.toHaveProperty('DashboardAdminMobile')
    expect(userConsoleStories).not.toHaveProperty('Tokens')
    expect(userConsoleStories).not.toHaveProperty('TokensAdmin')
    expect(userConsoleStories).not.toHaveProperty('TokensEmpty')
  })

  it('covers the no-hash console root as the merged landing default', () => {
    const rootArgs = {
      ...meta.args,
      ...userConsoleStories.ConsoleHomeRoot.args,
    }

    expect(userConsoleStories.__testables.resolveStoryState(rootArgs).routeHash).toBe('')
    expect(userConsoleStories.__testables.resolveStoryState(meta.args as typeof rootArgs).routeHash).toBe('#/dashboard')
  })
})
