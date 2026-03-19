import type { Meta, StoryObj } from '@storybook/react-vite'
import { useLayoutEffect, useState } from 'react'

import { KeyDetails } from '../AdminDashboard'
import type {
  ApiKeyStats,
  KeySummary,
  RequestLog,
  StickyNodesResponse,
  StickyUsersResponse,
} from '../api'
import { useTranslate } from '../i18n'
import AdminShell, { type AdminNavItem } from '../admin/AdminShell'
import {
  stickyNodesReviewStoryData,
  stickyUsersEmptyStoryData,
} from '../admin/keyStickyStoryData'

const REVIEW_KEY_ID = 'CBoX'
const REVIEW_AT = Date.parse('2026-03-19T18:09:33+08:00')

const keyDetailMock: ApiKeyStats = {
  id: REVIEW_KEY_ID,
  status: 'active',
  group: 'production',
  registration_ip: '8.8.8.8',
  registration_region: 'US',
  status_changed_at: Math.floor((REVIEW_AT - 34 * 60 * 1000) / 1000),
  last_used_at: Math.floor(REVIEW_AT / 1000),
  deleted_at: null,
  quota_limit: 32_000,
  quota_remaining: 4_980,
  quota_synced_at: Math.floor((REVIEW_AT - 5 * 60 * 1000) / 1000),
  total_requests: 20_112,
  success_count: 19_488,
  error_count: 624,
  quota_exhausted_count: 0,
  quarantine: null,
}

const keyMetricsMock: KeySummary = {
  total_requests: 20_112,
  success_count: 16_994,
  error_count: 0,
  quota_exhausted_count: 0,
  active_keys: 1,
  exhausted_keys: 0,
  last_activity: Math.floor(REVIEW_AT / 1000),
}

function createRequestLog(id: number, isoTime: string): RequestLog {
  return {
    id,
    key_id: REVIEW_KEY_ID,
    auth_token_id: null,
    method: 'POST',
    path: '/api/tavily/search',
    query: null,
    http_status: 200,
    mcp_status: null,
    result_status: 'success',
    created_at: Math.floor(Date.parse(isoTime) / 1000),
    error_message: null,
    request_body: null,
    response_body: null,
    forwarded_headers: [],
    dropped_headers: [],
  }
}

const keyLogsMock: RequestLog[] = [
  createRequestLog(10_001, '2026-03-19T18:09:33+08:00'),
  createRequestLog(10_002, '2026-03-19T18:09:13+08:00'),
  createRequestLog(10_003, '2026-03-19T18:08:40+08:00'),
  createRequestLog(10_004, '2026-03-19T18:08:11+08:00'),
  createRequestLog(10_005, '2026-03-19T18:07:41+08:00'),
  createRequestLog(10_006, '2026-03-19T18:07:10+08:00'),
  createRequestLog(10_007, '2026-03-19T18:06:41+08:00'),
  createRequestLog(10_008, '2026-03-19T18:06:10+08:00'),
]

const stickyUsersMock: StickyUsersResponse = {
  items: stickyUsersEmptyStoryData,
  total: 0,
  page: 1,
  perPage: 20,
}

const stickyNodesMock: StickyNodesResponse = {
  rangeStart: '2026-03-18T18:00:00+08:00',
  rangeEnd: '2026-03-19T18:00:00+08:00',
  bucketSeconds: 3600,
  nodes: stickyNodesReviewStoryData,
}

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

function installKeyDetailFetchMock(): () => void {
  const originalFetch = window.fetch.bind(window)

  window.fetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const request = input instanceof Request ? input : new Request(input, init)
    const url = new URL(request.url, window.location.origin)

    if (url.pathname === `/api/keys/${REVIEW_KEY_ID}`) {
      return jsonResponse(keyDetailMock)
    }

    if (url.pathname === `/api/keys/${REVIEW_KEY_ID}/metrics`) {
      return jsonResponse(keyMetricsMock)
    }

    if (url.pathname === `/api/keys/${REVIEW_KEY_ID}/logs`) {
      return jsonResponse(keyLogsMock)
    }

    if (url.pathname === `/api/keys/${REVIEW_KEY_ID}/sticky-users`) {
      return jsonResponse(stickyUsersMock)
    }

    if (url.pathname === `/api/keys/${REVIEW_KEY_ID}/sticky-nodes`) {
      return jsonResponse(stickyNodesMock)
    }

    if (url.pathname === `/api/keys/${REVIEW_KEY_ID}/sync-usage`) {
      return new Response(null, { status: 204 })
    }

    return originalFetch(input, init)
  }

  return () => {
    window.fetch = originalFetch
  }
}

function KeyDetailRouteSurface(): JSX.Element {
  const adminStrings = useTranslate().admin

  const navItems: AdminNavItem[] = [
    { module: 'dashboard', label: adminStrings.nav.dashboard, icon: 'mdi:view-dashboard-outline' },
    { module: 'tokens', label: adminStrings.nav.tokens, icon: 'mdi:key-chain-variant' },
    { module: 'keys', label: adminStrings.nav.keys, icon: 'mdi:key-outline' },
    { module: 'requests', label: adminStrings.nav.requests, icon: 'mdi:file-document-outline' },
    { module: 'jobs', label: adminStrings.nav.jobs, icon: 'mdi:calendar-clock-outline' },
    { module: 'users', label: adminStrings.nav.users, icon: 'mdi:account-group-outline' },
    { module: 'alerts', label: adminStrings.nav.alerts, icon: 'mdi:bell-ring-outline' },
    { module: 'proxy-settings', label: adminStrings.nav.proxySettings, icon: 'mdi:tune-variant' },
  ]

  return (
    <AdminShell
      activeModule="keys"
      navItems={navItems}
      skipToContentLabel={adminStrings.accessibility.skipToContent}
      onSelectModule={() => undefined}
    >
      <KeyDetails id={REVIEW_KEY_ID} onBack={() => undefined} onOpenUser={() => undefined} />
    </AdminShell>
  )
}

function KeyDetailRouteStoryCanvas(): JSX.Element {
  const [ready, setReady] = useState(false)

  useLayoutEffect(() => {
    const cleanupFetch = installKeyDetailFetchMock()
    setReady(true)

    return () => {
      cleanupFetch()
      setReady(false)
    }
  }, [])

  if (!ready) {
    return <div style={{ minHeight: '100vh', background: 'hsl(var(--background))' }} />
  }

  return <KeyDetailRouteSurface />
}

const meta = {
  title: 'Admin/Pages/KeyDetailRoute',
  component: KeyDetailRouteStoryCanvas,
  tags: ['autodocs'],
  parameters: {
    layout: 'fullscreen',
    docs: {
      description: {
        component:
          '真实 AdminShell + KeyDetails 路由视图，使用页面级 fetch mock 复刻 `/admin/keys/CBoX` 的审阅界面。',
      },
    },
  },
} satisfies Meta<typeof KeyDetailRouteStoryCanvas>

export default meta

type Story = StoryObj<typeof meta>

export const CBoXReview: Story = {
  globals: {
    language: 'zh',
    themeMode: 'dark',
  },
  parameters: {
    viewport: { defaultViewport: '1440-device-desktop' },
  },
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 200))
    const text = canvasElement.ownerDocument.body.textContent ?? ''
    for (const expected of [
      'Sticky 用户',
      'Sticky 节点',
      '当前没有用户 sticky 到这把密钥。',
      '主7 · 备2',
      '主1 · 备6',
      REVIEW_KEY_ID,
    ]) {
      if (!text.includes(expected)) {
        throw new Error(`Expected route story to contain: ${expected}`)
      }
    }
  },
}
