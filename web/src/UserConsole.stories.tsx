import { useEffect, useLayoutEffect, useMemo, useState } from 'react'
import type { Meta, StoryObj } from '@storybook/react-vite'

import type { Profile, UserDashboard, UserTokenSummary } from './api'
import UserConsole from './UserConsole'

type ConsoleView = 'Dashboard' | 'Tokens' | 'Token Detail'
type TokenListState = 'Default List' | 'Empty'
type TokenDetailPreview =
  | 'Overview'
  | 'API Check Running'
  | 'All Checks Pass'
  | 'Partial Availability'
  | 'Authentication Failed'
  | 'Quota Blocked'

type ProbeMockMode = 'none' | 'running' | 'success' | 'partial' | 'auth-fail' | 'exhausted'

interface UserConsoleStoryArgs {
  consoleView: ConsoleView
  isAdmin: boolean
  tokenListState: TokenListState
  tokenDetailPreview: TokenDetailPreview
}

interface UserConsoleStoryState {
  autoProbeTarget: 'mcp' | 'api' | null
  isAdmin: boolean
  probeMode: ProbeMockMode
  routeHash: string
  tokenListEmpty: boolean
}

const PROBE_STEP_DELAY_MS = 900
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

const tokenDetailExhaustedSample: UserTokenSummary = {
  ...tokenSample,
  quotaHourlyUsed: 100,
  quotaHourlyLimit: 100,
  quotaDailyUsed: 500,
  quotaDailyLimit: 500,
  quotaMonthlyUsed: 4188,
  quotaMonthlyLimit: 5000,
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
  userLoggedIn: true,
  userProvider: 'linuxdo',
  userDisplayName: 'Ivan',
}

const adminProfileSample: Profile = {
  ...profileSample,
  isAdmin: true,
}

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => {
    window.setTimeout(resolve, ms)
  })
}

function probeModeFromPreview(preview: TokenDetailPreview): ProbeMockMode {
  if (preview === 'API Check Running') return 'running'
  if (preview === 'All Checks Pass') return 'success'
  if (preview === 'Partial Availability') return 'partial'
  if (preview === 'Authentication Failed') return 'auth-fail'
  if (preview === 'Quota Blocked') return 'exhausted'
  return 'none'
}

function autoProbeTargetFromPreview(preview: TokenDetailPreview): 'mcp' | 'api' | null {
  if (
    preview === 'API Check Running'
    || preview === 'All Checks Pass'
    || preview === 'Partial Availability'
  ) {
    return 'api'
  }
  if (preview === 'Authentication Failed' || preview === 'Quota Blocked') {
    return 'mcp'
  }
  return null
}

function routeHashFromView(view: ConsoleView): string {
  if (view === 'Tokens') return '#/tokens'
  if (view === 'Token Detail') return TOKEN_DETAIL_HASH
  return '#/dashboard'
}

function resolveStoryState(args: UserConsoleStoryArgs): UserConsoleStoryState {
  return {
    autoProbeTarget: args.consoleView === 'Token Detail'
      ? autoProbeTargetFromPreview(args.tokenDetailPreview)
      : null,
    isAdmin: args.isAdmin,
    probeMode: args.consoleView === 'Token Detail'
      ? probeModeFromPreview(args.tokenDetailPreview)
      : 'none',
    routeHash: routeHashFromView(args.consoleView),
    tokenListEmpty: args.consoleView === 'Tokens' && args.tokenListState === 'Empty',
  }
}

function installUserConsoleFetchMock(state: UserConsoleStoryState): () => void {
  const originalFetch = window.fetch.bind(window)
  const researchRequestId = 'rq-story-001'

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

    if (url.pathname === '/api/user/tokens') {
      return jsonResponse(state.tokenListEmpty ? [] : [tokenSample])
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

      return jsonResponse(state.probeMode === 'exhausted' ? tokenDetailExhaustedSample : tokenDetailSample)
    }

    if (url.pathname === '/mcp') {
      if (state.probeMode === 'auth-fail') {
        return jsonResponse({ error: 'invalid or disabled token' }, 401)
      }
      if (state.probeMode !== 'none') {
        await sleep(PROBE_STEP_DELAY_MS)
      }
      const payload = await request.clone().json().catch(() => ({}))
      const method = typeof payload?.method === 'string' ? payload.method : ''
      const accept = request.headers.get('Accept') ?? ''
      const acceptsProbeFormats = accept.includes('application/json') && accept.includes('text/event-stream')

      if (state.probeMode === 'exhausted' && method === 'ping') {
        return jsonResponse({
          error: 'quota_exceeded',
          window: 'day',
          hourly: { limit: 100, used: 100 },
          daily: { limit: 500, used: 500 },
          monthly: { limit: 5000, used: 4188 },
        }, 429)
      }

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

      if (state.probeMode === 'partial' && method === 'tools/list') {
        return jsonResponse({ error: { code: -32001, message: 'tools/list unavailable' } })
      }

      if (method === 'tools/list') {
        return new Response(
          `event: message\ndata: ${JSON.stringify({
            jsonrpc: '2.0',
            id: payload?.id ?? null,
            result: {
              tools: [{ name: 'tavily_search' }],
            },
          })}\n\n`,
          {
            status: 200,
            headers: { 'Content-Type': 'text/event-stream' },
          },
        )
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
      if (state.probeMode === 'auth-fail') {
        return jsonResponse({ error: 'invalid or disabled token' }, 401)
      }
      if (state.probeMode !== 'none') {
        await sleep(PROBE_STEP_DELAY_MS)
      }

      if (url.pathname === '/api/tavily/search') {
        if (state.probeMode === 'running') {
          await sleep(60_000)
        }
        return jsonResponse({ status: 200, results: [] })
      }
      if (url.pathname === '/api/tavily/extract') {
        return jsonResponse({ status: 200, results: [] })
      }
      if (url.pathname === '/api/tavily/crawl') {
        return jsonResponse({ status: 200, results: [] })
      }
      if (url.pathname === '/api/tavily/map') {
        if (state.probeMode === 'partial') {
          return jsonResponse({ error: 'map endpoint timeout' }, 500)
        }
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

function UserConsoleStory(args: UserConsoleStoryArgs): JSX.Element {
  const [ready, setReady] = useState(false)
  const storyState = useMemo(
    () => resolveStoryState(args),
    [args.consoleView, args.isAdmin, args.tokenListState, args.tokenDetailPreview],
  )

  useLayoutEffect(() => {
    const previousHash = window.location.hash
    const cleanupFetch = installUserConsoleFetchMock(storyState)
    window.location.hash = storyState.routeHash
    setReady(true)

    return () => {
      cleanupFetch()
      window.location.hash = previousHash
      setReady(false)
    }
  }, [storyState.isAdmin, storyState.probeMode, storyState.routeHash, storyState.tokenListEmpty])

  useEffect(() => {
    if (!ready || !storyState.autoProbeTarget) return
    const timer = window.setTimeout(() => {
      const selector = `[data-probe-kind="${storyState.autoProbeTarget}"]`
      const button = document.querySelector<HTMLButtonElement>(selector)
      button?.click()
    }, 80)
    return () => window.clearTimeout(timer)
  }, [ready, storyState.autoProbeTarget])

  if (!ready) {
    return <div style={{ minHeight: '100vh' }} />
  }

  const storyKey = [
    storyState.routeHash,
    storyState.isAdmin ? 'admin' : 'user',
    storyState.tokenListEmpty ? 'empty' : 'default',
    storyState.probeMode,
  ].join(':')

  return <UserConsole key={storyKey} />
}

const meta = {
  title: 'User Console/UserConsole',
  parameters: {
    controls: { expanded: true },
    layout: 'fullscreen',
    viewport: { defaultViewport: '1440-device-desktop' },
  },
  args: {
    consoleView: 'Dashboard',
    isAdmin: false,
    tokenListState: 'Default List',
    tokenDetailPreview: 'Overview',
  },
  argTypes: {
    consoleView: {
      name: 'Console view',
      description: 'Pick the main console page to preview.',
      options: ['Dashboard', 'Tokens', 'Token Detail'],
      control: { type: 'inline-radio' },
    },
    isAdmin: {
      name: 'Admin session',
      description: 'Toggle the console between a regular user session and an admin session.',
      control: { type: 'boolean' },
    },
    tokenListState: {
      name: 'Token list state',
      description: 'Pick the list presentation for the Tokens page.',
      options: ['Default List', 'Empty'],
      control: { type: 'inline-radio' },
      if: { arg: 'consoleView', eq: 'Tokens' },
    },
    tokenDetailPreview: {
      name: 'Token detail preview',
      description: 'Pick the overview or special state to preview on the Token Detail page.',
      options: [
        'Overview',
        'API Check Running',
        'All Checks Pass',
        'Partial Availability',
        'Authentication Failed',
        'Quota Blocked',
      ],
      control: { type: 'select' },
      if: { arg: 'consoleView', eq: 'Token Detail' },
    },
  },
  render: (args) => <UserConsoleStory {...args} />,
} satisfies Meta<UserConsoleStoryArgs>

export default meta

type Story = StoryObj<typeof meta>

export const Dashboard: Story = {
  args: {
    consoleView: 'Dashboard',
    isAdmin: false,
  },
}

export const DashboardAdmin: Story = {
  name: 'Dashboard Admin',
  args: {
    consoleView: 'Dashboard',
    isAdmin: true,
  },
}

export const DashboardAdminMobile: Story = {
  name: 'Dashboard Admin Mobile',
  args: {
    consoleView: 'Dashboard',
    isAdmin: true,
  },
  parameters: {
    viewport: { defaultViewport: '0390-device-iphone-14' },
  },
}

export const Tokens: Story = {
  args: {
    consoleView: 'Tokens',
    isAdmin: false,
    tokenListState: 'Default List',
  },
}

export const TokensAdmin: Story = {
  name: 'Tokens Admin',
  args: {
    consoleView: 'Tokens',
    isAdmin: true,
    tokenListState: 'Default List',
  },
}

export const TokensEmpty: Story = {
  name: 'Tokens Empty',
  args: {
    consoleView: 'Tokens',
    isAdmin: false,
    tokenListState: 'Empty',
  },
}

export const TokenDetailOverview: Story = {
  name: 'Token Detail Overview',
  args: {
    consoleView: 'Token Detail',
    isAdmin: false,
    tokenDetailPreview: 'Overview',
  },
}

export const TokenDetailAdmin: Story = {
  name: 'Token Detail Admin',
  args: {
    consoleView: 'Token Detail',
    isAdmin: true,
    tokenDetailPreview: 'Overview',
  },
}

export const ApiCheckRunning: Story = {
  name: 'API Check Running',
  args: {
    consoleView: 'Token Detail',
    isAdmin: false,
    tokenDetailPreview: 'API Check Running',
  },
}

export const AllChecksPass: Story = {
  name: 'All Checks Pass',
  args: {
    consoleView: 'Token Detail',
    isAdmin: false,
    tokenDetailPreview: 'All Checks Pass',
  },
}

export const PartialAvailability: Story = {
  name: 'Partial Availability',
  args: {
    consoleView: 'Token Detail',
    isAdmin: false,
    tokenDetailPreview: 'Partial Availability',
  },
}

export const AuthenticationFailed: Story = {
  name: 'Authentication Failed',
  args: {
    consoleView: 'Token Detail',
    isAdmin: false,
    tokenDetailPreview: 'Authentication Failed',
  },
}

export const QuotaBlocked: Story = {
  name: 'Quota Blocked',
  args: {
    consoleView: 'Token Detail',
    isAdmin: false,
    tokenDetailPreview: 'Quota Blocked',
  },
}
