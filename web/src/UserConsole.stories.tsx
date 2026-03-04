import { useEffect, useLayoutEffect, useState } from 'react'
import type { Meta, StoryObj } from '@storybook/react-vite'

import type { Profile, UserDashboard, UserTokenSummary } from './api'
import UserConsole from './UserConsole'

type Scenario =
  | 'dashboard'
  | 'tokens'
  | 'tokens-empty'
  | 'token-detail'
  | 'token-detail-probe-running'
  | 'token-detail-probe-success'
  | 'token-detail-probe-partial'
  | 'token-detail-probe-auth-fail'

interface UserConsoleStoryArgs {
  scenario: Scenario
}

const PROBE_STEP_DELAY_MS = 900

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

type ProbeMockMode = 'none' | 'running' | 'success' | 'partial' | 'auth-fail'

function probeModeFromScenario(scenario: Scenario): ProbeMockMode {
  if (scenario === 'token-detail-probe-running') return 'running'
  if (scenario === 'token-detail-probe-success') return 'success'
  if (scenario === 'token-detail-probe-partial') return 'partial'
  if (scenario === 'token-detail-probe-auth-fail') return 'auth-fail'
  return 'none'
}

function autoProbeTargetFromScenario(scenario: Scenario): 'mcp' | 'api' | null {
  if (scenario === 'token-detail-probe-running') return 'api'
  if (scenario === 'token-detail-probe-success') return 'api'
  if (scenario === 'token-detail-probe-partial') return 'api'
  if (scenario === 'token-detail-probe-auth-fail') return 'mcp'
  return null
}

function scenarioHash(scenario: Scenario): string {
  if (scenario === 'tokens') return '#/tokens'
  if (scenario === 'tokens-empty') return '#/tokens'
  if (
    scenario === 'token-detail'
    || scenario === 'token-detail-probe-running'
    || scenario === 'token-detail-probe-success'
    || scenario === 'token-detail-probe-partial'
    || scenario === 'token-detail-probe-auth-fail'
  ) {
    return '#/tokens/a1b2'
  }
  return '#/dashboard'
}

function installUserConsoleFetchMock(scenario: Scenario): () => void {
  const originalFetch = window.fetch.bind(window)
  const probeMode = probeModeFromScenario(scenario)
  const researchRequestId = 'rq-story-001'

  window.fetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const request = input instanceof Request
      ? input
      : new Request(input, init)
    const url = new URL(request.url, window.location.origin)

    if (url.pathname === '/api/profile') {
      return jsonResponse(profileSample)
    }

    if (url.pathname === '/api/user/dashboard') {
      return jsonResponse(dashboardSample)
    }

    if (url.pathname === '/api/user/tokens') {
      return jsonResponse(scenario === 'tokens-empty' ? [] : [tokenSample])
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
      if (probeMode === 'auth-fail') {
        return jsonResponse({ error: 'invalid or disabled token' }, 401)
      }
      if (probeMode !== 'none') {
        await sleep(PROBE_STEP_DELAY_MS)
      }
      const payload = await request.clone().json().catch(() => ({}))
      const method = typeof payload?.method === 'string' ? payload.method : ''
      if (probeMode === 'partial' && method === 'tools/list') {
        return jsonResponse({ error: { code: -32001, message: 'tools/list unavailable' } })
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
      if (probeMode === 'auth-fail') {
        return jsonResponse({ error: 'invalid or disabled token' }, 401)
      }
      if (probeMode !== 'none') {
        await sleep(PROBE_STEP_DELAY_MS)
      }

      if (url.pathname === '/api/tavily/search') {
        if (probeMode === 'running') {
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
        if (probeMode === 'partial') {
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
  const autoProbeTarget = autoProbeTargetFromScenario(args.scenario)

  useLayoutEffect(() => {
    const previousHash = window.location.hash
    const cleanupFetch = installUserConsoleFetchMock(args.scenario)
    window.location.hash = scenarioHash(args.scenario)
    setReady(true)

    return () => {
      cleanupFetch()
      window.location.hash = previousHash
      setReady(false)
    }
  }, [args.scenario])

  useEffect(() => {
    if (!ready || !autoProbeTarget) return
    const timer = window.setTimeout(() => {
      const selector = `[data-probe-kind="${autoProbeTarget}"]`
      const button = document.querySelector<HTMLButtonElement>(selector)
      button?.click()
    }, 80)
    return () => window.clearTimeout(timer)
  }, [autoProbeTarget, ready])

  if (!ready) {
    return <div style={{ minHeight: '100vh' }} />
  }

  return <UserConsole />
}

const meta = {
  title: 'User Console/UserConsole',
  parameters: {
    layout: 'fullscreen',
  },
  render: (args) => <UserConsoleStory {...args} />,
} satisfies Meta<UserConsoleStoryArgs>

export default meta

type Story = StoryObj<typeof meta>

export const Dashboard: Story = {
  args: {
    scenario: 'dashboard',
  },
}

export const Tokens: Story = {
  args: {
    scenario: 'tokens',
  },
}

export const TokenDetail: Story = {
  args: {
    scenario: 'token-detail',
  },
}

export const TokenDetailProbeSuccess: Story = {
  args: {
    scenario: 'token-detail-probe-success',
  },
}

export const TokenDetailProbeRunning: Story = {
  args: {
    scenario: 'token-detail-probe-running',
  },
}

export const TokenDetailProbePartialFail: Story = {
  args: {
    scenario: 'token-detail-probe-partial',
  },
}

export const TokenDetailProbeAuthFail: Story = {
  args: {
    scenario: 'token-detail-probe-auth-fail',
  },
}

export const TokensEmpty: Story = {
  args: {
    scenario: 'tokens-empty',
  },
}
