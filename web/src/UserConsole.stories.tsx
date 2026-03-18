import { useEffect, useLayoutEffect, useMemo, useState } from 'react'
import type { Meta, StoryObj } from '@storybook/react-vite'

import type { Profile, UserDashboard, UserTokenSummary } from './api'
import UserConsole from './UserConsole'
import { userConsoleRouteToHash } from './lib/userConsoleRoutes'

type ConsoleView = 'Console Home' | 'Token Detail'
type LandingFocus = 'Overview Focus' | 'Token Focus'
type TokenListState = 'Single Token' | 'Multiple Tokens' | 'Empty'
type TokenDetailPreview = 'Overview' | 'Token Revealed'

type CopyRecoveryMode = 'none' | 'list-manual-bubble' | 'detail-inline'

interface UserConsoleStoryArgs {
  consoleView: ConsoleView
  isAdmin: boolean
  landingFocus: LandingFocus
  tokenListState: TokenListState
  tokenDetailPreview: TokenDetailPreview
  routeHashOverride?: string
}

interface UserConsoleStoryState {
  autoRevealToken: boolean
  isAdmin: boolean
  routeHash: string
  tokenListMode: 'single' | 'multiple' | 'empty'
}

const TOKEN_DETAIL_HASH = '#/tokens/a1b2'

const dashboardSample: UserDashboard = {
  hourlyAnyUsed: 126,
  hourlyAnyLimit: 200,
  quotaHourlyUsed: 82,
  quotaHourlyLimit: 100,
  quotaDailyUsed: 356,
  quotaDailyLimit: 500,
  quotaMonthlyUsed: 4120,
  quotaMonthlyLimit: 5000,
  dailySuccess: 301,
  dailyFailure: 17,
  monthlySuccess: 3478,
  lastActivity: 1_762_386_800,
}

const tokenSample: UserTokenSummary = {
  tokenId: 'a1b2',
  enabled: true,
  note: 'primary',
  lastUsedAt: 1_762_386_800,
  hourlyAnyUsed: 126,
  hourlyAnyLimit: 200,
  quotaHourlyUsed: 82,
  quotaHourlyLimit: 100,
  quotaDailyUsed: 356,
  quotaDailyLimit: 500,
  quotaMonthlyUsed: 4120,
  quotaMonthlyLimit: 5000,
  dailySuccess: 301,
  dailyFailure: 17,
  monthlySuccess: 3478,
}

const tokenSecondarySample: UserTokenSummary = {
  tokenId: 'c3d4',
  enabled: true,
  note: 'backup',
  lastUsedAt: 1_762_386_100,
  hourlyAnyUsed: 28,
  hourlyAnyLimit: 200,
  quotaHourlyUsed: 12,
  quotaHourlyLimit: 100,
  quotaDailyUsed: 84,
  quotaDailyLimit: 500,
  quotaMonthlyUsed: 933,
  quotaMonthlyLimit: 5000,
  dailySuccess: 76,
  dailyFailure: 4,
  monthlySuccess: 827,
}

const tokenDetailSample: UserTokenSummary = {
  ...tokenSample,
  hourlyAnyUsed: 131,
  quotaHourlyUsed: 88,
  quotaDailyUsed: 371,
  quotaMonthlyUsed: 4188,
  dailySuccess: 315,
  dailyFailure: 19,
  monthlySuccess: 3510,
}

interface ServerPublicTokenLogMock {
  id: number
  method: string
  path: string
  query: string | null
  httpStatus: number | null
  mcpStatus: number | null
  resultStatus: string
  errorMessage: string | null
  createdAt: number
}

const tokenLogsSample: ServerPublicTokenLogMock[] = [
  {
    id: 101,
    method: 'POST',
    path: '/api/tavily/search',
    query: 'q=rust',
    httpStatus: 200,
    mcpStatus: 200,
    resultStatus: 'success',
    errorMessage: null,
    createdAt: 1_762_386_640,
  },
  {
    id: 102,
    method: 'POST',
    path: '/mcp',
    query: null,
    httpStatus: 429,
    mcpStatus: 429,
    resultStatus: 'quota_exhausted',
    errorMessage: 'Account hourly limit reached',
    createdAt: 1_762_386_590,
  },
  {
    id: 103,
    method: 'POST',
    path: '/api/tavily/extract',
    query: null,
    httpStatus: 500,
    mcpStatus: 500,
    resultStatus: 'error',
    errorMessage: 'upstream timeout',
    createdAt: 1_762_386_520,
  },
]

const profileSample: Profile = {
  displayName: 'Ivan',
  isAdmin: false,
  forwardAuthEnabled: true,
  builtinAuthEnabled: true,
  allowRegistration: true,
  userLoggedIn: true,
  userProvider: 'linuxdo',
  userDisplayName: 'Ivan',
}

const adminProfileSample: Profile = {
  ...profileSample,
  isAdmin: true,
}

const versionSample = {
  backend: '0.2.0-dev',
  frontend: '0.2.0-dev',
}

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

function routeHashFromView(view: ConsoleView, landingFocus: LandingFocus, routeHashOverride?: string): string {
  if (view === 'Token Detail') return TOKEN_DETAIL_HASH
  if (typeof routeHashOverride === 'string') return routeHashOverride
  return userConsoleRouteToHash({
    name: 'landing',
    section: landingFocus === 'Token Focus' ? 'tokens' : 'dashboard',
  })
}

function resolveStoryState(args: UserConsoleStoryArgs): UserConsoleStoryState {
  const tokenListMode = args.consoleView !== 'Console Home'
    ? 'single'
    : args.tokenListState === 'Empty'
      ? 'empty'
      : args.tokenListState === 'Multiple Tokens'
        ? 'multiple'
        : 'single'

  return {
    autoRevealToken: args.consoleView === 'Token Detail' && args.tokenDetailPreview === 'Token Revealed',
    isAdmin: args.isAdmin,
    routeHash: routeHashFromView(args.consoleView, args.landingFocus, args.routeHashOverride),
    tokenListMode,
  }
}

export const __testables = {
  resolveStoryState,
}

function installUserConsoleFetchMock(state: UserConsoleStoryState): () => void {
  const originalFetch = window.fetch.bind(window)
  const researchRequestId = 'rq-story-001'
  const tokenList = state.tokenListMode === 'empty'
    ? []
    : state.tokenListMode === 'multiple'
      ? [tokenSample, tokenSecondarySample]
      : [tokenSample]

  window.fetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const request = input instanceof Request
      ? input
      : new Request(input, init)
    const url = new URL(request.url, window.location.origin)

    if (url.pathname === '/api/profile') {
      return jsonResponse(state.isAdmin ? adminProfileSample : profileSample)
    }

    if (url.pathname === '/api/user/dashboard') {
      return jsonResponse(dashboardSample)
    }

    if (url.pathname === '/api/version') {
      return jsonResponse(versionSample)
    }

    if (url.pathname === '/api/user/tokens') {
      return jsonResponse(tokenList)
    }

    const tokenRoute = url.pathname.match(/^\/api\/user\/tokens\/([^/]+)(?:\/(secret|logs))?$/)
    if (tokenRoute) {
      const tokenId = decodeURIComponent(tokenRoute[1])
      const action = tokenRoute[2] ?? 'detail'

      if (tokenId !== tokenSample.tokenId) {
        return jsonResponse({ message: 'Not Found' }, 404)
      }

      if (action === 'secret') {
        return jsonResponse({ token: 'th-a1b2-1234567890abcdef' })
      }

      if (action === 'logs') {
        return jsonResponse(tokenLogsSample)
      }

      return jsonResponse(tokenDetailSample)
    }

    if (url.pathname === '/mcp') {
      const payload = await request.clone().json().catch(() => ({}))
      const method = typeof payload?.method === 'string' ? payload.method : ''
      const accept = request.headers.get('Accept') ?? ''
      const acceptsProbeFormats = accept.includes('application/json') && accept.includes('text/event-stream')

      if (method === 'tools/list' && !acceptsProbeFormats) {
        return jsonResponse({
          jsonrpc: '2.0',
          id: 'server-error',
          error: {
            code: -32600,
            message: 'Not Acceptable: Client must accept both application/json and text/event-stream',
          },
        }, 406)
      }

      if (method === 'tools/list') {
        return new Response(
          `event: message\ndata: ${JSON.stringify({
            jsonrpc: '2.0',
            id: payload?.id ?? null,
            result: {
              tools: [
                { name: 'tavily-search' },
                { name: 'tavily-extract' },
                { name: 'tavily-crawl' },
                { name: 'tavily-map' },
                { name: 'tavily-research' },
              ],
            },
          })}\n\n`,
          {
            status: 200,
            headers: { 'Content-Type': 'text/event-stream' },
          },
        )
      }

      if (method === 'tools/call') {
        return jsonResponse({
          jsonrpc: '2.0',
          id: payload?.id ?? null,
          result: {
            ok: true,
            tool: payload?.params?.name ?? null,
          },
        })
      }

      return jsonResponse({
        jsonrpc: '2.0',
        id: payload?.id ?? null,
        result: {
          ok: true,
          method,
        },
      })
    }

    if (url.pathname.startsWith('/api/tavily/')) {
      if (url.pathname === '/api/tavily/search') {
        return jsonResponse({ status: 200, results: [] })
      }
      if (url.pathname === '/api/tavily/extract') {
        return jsonResponse({ status: 200, results: [] })
      }
      if (url.pathname === '/api/tavily/crawl') {
        return jsonResponse({ status: 200, results: [] })
      }
      if (url.pathname === '/api/tavily/map') {
        return jsonResponse({ status: 200, results: [] })
      }
      if (url.pathname === '/api/tavily/research') {
        return jsonResponse({
          request_id: researchRequestId,
          status: 'pending',
        })
      }
      if (url.pathname === `/api/tavily/research/${researchRequestId}`) {
        return jsonResponse({
          request_id: researchRequestId,
          status: 'pending',
        })
      }
    }

    return originalFetch(input, init)
  }

  return () => {
    window.fetch = originalFetch
  }
}

function installClipboardFailureMock(): () => void {
  const originalClipboardDescriptor = Object.getOwnPropertyDescriptor(navigator, 'clipboard')
  const originalExecCommand = document.execCommand
  let clipboardMockInstalled = false

  try {
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: {
        writeText: async () => {
          throw new Error('storybook-copy-blocked')
        },
      },
    })
    clipboardMockInstalled = true
  } catch {
    // Ignore if the browser refuses to override clipboard in the mock canvas.
  }

  try {
    document.execCommand = (() => false) as typeof document.execCommand
  } catch {
    // Ignore if execCommand cannot be replaced in the current runtime.
  }

  return () => {
    try {
      if (originalClipboardDescriptor) {
        Object.defineProperty(navigator, 'clipboard', originalClipboardDescriptor)
      } else if (clipboardMockInstalled) {
        Reflect.deleteProperty(navigator, 'clipboard')
      }
    } catch {
      // Ignore restore failures inside Storybook.
    }

    try {
      document.execCommand = originalExecCommand
    } catch {
      // Ignore restore failures inside Storybook.
    }
  }
}

function UserConsoleStory(
  args: UserConsoleStoryArgs & {
    copyRecoveryMode?: CopyRecoveryMode
  },
): JSX.Element {
  const [ready, setReady] = useState(false)
  const storyState = useMemo(
    () => resolveStoryState(args),
    [args.consoleView, args.isAdmin, args.landingFocus, args.tokenListState, args.tokenDetailPreview, args.routeHashOverride],
  )
  const copyRecoveryMode = args.copyRecoveryMode ?? 'none'

  useLayoutEffect(() => {
    const previousHash = window.location.hash
    const cleanupFetch = installUserConsoleFetchMock(storyState)
    const cleanupClipboard = copyRecoveryMode === 'none' ? null : installClipboardFailureMock()
    window.location.hash = storyState.routeHash
    setReady(true)

    return () => {
      cleanupFetch()
      cleanupClipboard?.()
      window.location.hash = previousHash
      setReady(false)
    }
  }, [copyRecoveryMode, storyState.isAdmin, storyState.routeHash, storyState.tokenListMode])

  useEffect(() => {
    if (!ready || !storyState.autoRevealToken) return
    const timer = window.setTimeout(() => {
      const button = document.querySelector<HTMLButtonElement>('.user-console-token-box .token-visibility-button')
      button?.click()
    }, 80)
    return () => window.clearTimeout(timer)
  }, [ready, storyState.autoRevealToken])

  useEffect(() => {
    if (!ready || copyRecoveryMode === 'none') return
    const timer = window.setTimeout(() => {
      const selector = copyRecoveryMode === 'list-manual-bubble'
        ? 'tbody .table-actions button'
        : '.user-console-token-box .token-copy-button'
      const button = document.querySelector<HTMLButtonElement>(selector)
      button?.click()
    }, 180)
    return () => window.clearTimeout(timer)
  }, [copyRecoveryMode, ready])

  if (!ready) {
    return <div style={{ minHeight: '100vh' }} />
  }

  const storyKey = [
    storyState.routeHash,
    storyState.isAdmin ? 'admin' : 'user',
    storyState.tokenListMode,
    storyState.autoRevealToken ? 'revealed' : 'hidden',
  ].join(':')

  return <UserConsole key={storyKey} />
}

const meta = {
  title: 'User Console/UserConsole',
  excludeStories: ['__testables'],
  parameters: {
    controls: { expanded: true },
    layout: 'fullscreen',
    viewport: { defaultViewport: '1440-device-desktop' },
  },
  args: {
    consoleView: 'Console Home',
    isAdmin: false,
    landingFocus: 'Overview Focus',
    tokenListState: 'Single Token',
    tokenDetailPreview: 'Overview',
  },
  argTypes: {
    consoleView: {
      name: 'Console view',
      description: 'Pick the merged console landing page or the dedicated token detail page.',
      options: ['Console Home', 'Token Detail'],
      control: { type: 'inline-radio' },
    },
    isAdmin: {
      name: 'Admin session',
      description: 'Toggle the console between a regular user session and an admin session.',
      control: { type: 'boolean' },
    },
    landingFocus: {
      name: 'Landing focus',
      description: 'Preview which merged section the legacy hash should auto-focus.',
      options: ['Overview Focus', 'Token Focus'],
      control: { type: 'inline-radio' },
      if: { arg: 'consoleView', eq: 'Console Home' },
    },
    tokenListState: {
      name: 'Token list state',
      description: 'Pick the token list presentation for the merged landing page.',
      options: ['Single Token', 'Multiple Tokens', 'Empty'],
      control: { type: 'inline-radio' },
      if: { arg: 'consoleView', eq: 'Console Home' },
    },
    tokenDetailPreview: {
      name: 'Token detail preview',
      description: 'Pick the standard token detail page or the revealed-token variant.',
      options: ['Overview', 'Token Revealed'],
      control: { type: 'select' },
      if: { arg: 'consoleView', eq: 'Token Detail' },
    },
    routeHashOverride: {
      table: { disable: true },
      control: false,
    },
  },
  render: (args) => <UserConsoleStory {...args} />,
} satisfies Meta<UserConsoleStoryArgs>

export default meta

type Story = StoryObj<typeof meta>

export const ConsoleHome: Story = {
  args: {
    consoleView: 'Console Home',
    isAdmin: false,
    landingFocus: 'Overview Focus',
  },
}

export const ConsoleHomeRoot: Story = {
  name: 'Console Home Root',
  args: {
    consoleView: 'Console Home',
    isAdmin: false,
    landingFocus: 'Overview Focus',
    routeHashOverride: '',
  },
}

export const ConsoleHomeAdmin: Story = {
  name: 'Console Home Admin',
  args: {
    consoleView: 'Console Home',
    isAdmin: true,
    landingFocus: 'Overview Focus',
  },
}

export const ConsoleHomeAdminMobile: Story = {
  name: 'Console Home Admin Mobile',
  args: {
    consoleView: 'Console Home',
    isAdmin: true,
    landingFocus: 'Overview Focus',
  },
  parameters: {
    viewport: { defaultViewport: '0390-device-iphone-14' },
  },
}

export const ConsoleHomeTokensFocus: Story = {
  name: 'Console Home Tokens Focus',
  args: {
    consoleView: 'Console Home',
    isAdmin: false,
    landingFocus: 'Token Focus',
    tokenListState: 'Single Token',
  },
}

export const ConsoleHomeTokensFocusAdmin: Story = {
  name: 'Console Home Tokens Focus Admin',
  args: {
    consoleView: 'Console Home',
    isAdmin: true,
    landingFocus: 'Token Focus',
    tokenListState: 'Single Token',
  },
}

export const ConsoleHomeMultipleTokens: Story = {
  name: 'Console Home Multiple Tokens',
  args: {
    consoleView: 'Console Home',
    isAdmin: false,
    landingFocus: 'Token Focus',
    tokenListState: 'Multiple Tokens',
  },
}

export const ConsoleHomeEmptyTokens: Story = {
  name: 'Console Home Empty Tokens',
  args: {
    consoleView: 'Console Home',
    isAdmin: false,
    landingFocus: 'Token Focus',
    tokenListState: 'Empty',
  },
}

export const ConsoleHomeCopyFailureRecovery: Story = {
  name: 'Console Home Copy Failure Recovery',
  args: {
    consoleView: 'Console Home',
    isAdmin: false,
    landingFocus: 'Token Focus',
    tokenListState: 'Single Token',
  },
  render: (args) => <UserConsoleStory {...args} copyRecoveryMode="list-manual-bubble" />,
}

export const TokenDetailOverview: Story = {
  name: 'Token Detail Overview',
  args: {
    consoleView: 'Token Detail',
    isAdmin: false,
    landingFocus: 'Overview Focus',
    tokenDetailPreview: 'Overview',
  },
}

export const TokenDetailCopyFailureRecovery: Story = {
  name: 'Token Detail Copy Failure Recovery',
  args: {
    consoleView: 'Token Detail',
    isAdmin: false,
    landingFocus: 'Overview Focus',
    tokenDetailPreview: 'Overview',
  },
  render: (args) => <UserConsoleStory {...args} copyRecoveryMode="detail-inline" />,
}

export const TokenRevealed: Story = {
  name: 'Token Revealed',
  args: {
    consoleView: 'Token Detail',
    isAdmin: false,
    tokenDetailPreview: 'Token Revealed',
  },
}

export const TokenDetailAdmin: Story = {
  name: 'Token Detail Admin',
  args: {
    consoleView: 'Token Detail',
    isAdmin: true,
    landingFocus: 'Overview Focus',
    tokenDetailPreview: 'Overview',
  },
}
