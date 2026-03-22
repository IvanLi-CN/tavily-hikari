import type { Meta, StoryObj } from '@storybook/react-vite'
import { ChartColumnIncreasing } from 'lucide-react'
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
import { Icon } from '../lib/icons'
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
  const tokenId = ['9vsN', 'Vn7D', 'Q4sE'][id % 3]
  const requestKinds = [
    {
      key: 'api:search',
      label: 'API | search',
      detail: null,
      credits: 2,
      result: 'success',
      keyEffectCode: 'none',
      keyEffectSummary: null,
    },
    {
      key: 'mcp:raw:/mcp',
      label: 'MCP | /mcp',
      detail: null,
      credits: null,
      result: 'quota_exhausted',
      keyEffectCode: 'marked_exhausted',
      keyEffectSummary: 'Automatically marked this key as exhausted',
    },
    {
      key: 'api:extract',
      label: 'API | extract',
      detail: null,
      credits: 3,
      result: 'error',
      keyEffectCode: 'none',
      keyEffectSummary: null,
    },
  ] as const
  const kind = requestKinds[id % requestKinds.length]
  return {
    id,
    key_id: REVIEW_KEY_ID,
    auth_token_id: tokenId,
    method: 'POST',
    path: kind.key === 'api:extract' ? '/api/tavily/extract' : kind.key === 'api:search' ? '/api/tavily/search' : '/mcp',
    query: null,
    http_status: kind.result === 'error' ? 502 : 200,
    mcp_status: kind.result === 'quota_exhausted' ? 432 : 200,
    business_credits: kind.credits,
    request_kind_key: kind.key,
    request_kind_label: kind.label,
    request_kind_detail: kind.detail,
    result_status: kind.result,
    created_at: Math.floor(Date.parse(isoTime) / 1000),
    error_message: kind.result === 'error' ? 'Bad gateway from upstream' : null,
    key_effect_code: kind.keyEffectCode,
    key_effect_summary: kind.keyEffectSummary,
    request_body: null,
    response_body: null,
    forwarded_headers: [],
    dropped_headers: [],
    operationalClass: 'success',
    requestKindProtocolGroup: 'api',
    requestKindBillingGroup: 'billable',
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

function buildFacetOptions(values: Array<string | null | undefined>): Array<{ value: string; count: number }> {
  const counts = new Map<string, number>()
  for (const raw of values) {
    const value = raw?.trim()
    if (!value) continue
    counts.set(value, (counts.get(value) ?? 0) + 1)
  }
  return Array.from(counts.entries())
    .sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))
    .map(([value, count]) => ({ value, count }))
}

function buildKeyLogsPage(source: RequestLog[], searchParams: URLSearchParams) {
  const page = Number(searchParams.get('page') ?? '1')
  const perPage = Number(searchParams.get('per_page') ?? '20')
  const requestKinds = searchParams.getAll('request_kind')
  const result = searchParams.get('result')?.trim() ?? ''
  const keyEffect = searchParams.get('key_effect')?.trim() ?? ''
  const tokenId = searchParams.get('auth_token_id')?.trim() ?? ''
  const filtered = source.filter((log) => {
    if (requestKinds.length > 0 && !(log.request_kind_key && requestKinds.includes(log.request_kind_key))) {
      return false
    }
    if (tokenId && log.auth_token_id !== tokenId) {
      return false
    }
    if (result && log.result_status !== result) {
      return false
    }
    if (keyEffect && (log.key_effect_code ?? 'none') !== keyEffect) {
      return false
    }
    return true
  })
  const start = (page - 1) * perPage
  const requestKindOptions = [
    { key: 'api:extract', label: 'API | extract', protocol_group: 'api', billing_group: 'billable' },
    { key: 'api:search', label: 'API | search', protocol_group: 'api', billing_group: 'billable' },
    { key: 'mcp:raw:/mcp', label: 'MCP | /mcp', protocol_group: 'mcp', billing_group: 'billable' },
  ].map((option) => ({
    ...option,
    count: source.filter((log) => log.request_kind_key === option.key).length,
  }))
  return {
    items: filtered.slice(start, start + perPage),
    page,
    per_page: perPage,
    total: filtered.length,
    request_kind_options: requestKindOptions,
    facets: {
      results: buildFacetOptions(filtered.map((log) => log.result_status)),
      key_effects: buildFacetOptions(filtered.map((log) => log.key_effect_code ?? 'none')),
      tokens: buildFacetOptions(filtered.map((log) => log.auth_token_id)),
    },
  }
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

    if (url.pathname === `/api/keys/${REVIEW_KEY_ID}/logs/page`) {
      return jsonResponse(buildKeyLogsPage(keyLogsMock, url.searchParams))
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
    { target: 'dashboard', label: adminStrings.nav.dashboard, icon: <Icon icon="mdi:view-dashboard-outline" width={18} height={18} /> },
    { target: 'user-usage', label: adminStrings.nav.usage, icon: <ChartColumnIncreasing size={18} strokeWidth={2.2} /> },
    { target: 'tokens', label: adminStrings.nav.tokens, icon: <Icon icon="mdi:key-chain-variant" width={18} height={18} /> },
    { target: 'keys', label: adminStrings.nav.keys, icon: <Icon icon="mdi:key-outline" width={18} height={18} /> },
    { target: 'requests', label: adminStrings.nav.requests, icon: <Icon icon="mdi:file-document-outline" width={18} height={18} /> },
    { target: 'jobs', label: adminStrings.nav.jobs, icon: <Icon icon="mdi:calendar-clock-outline" width={18} height={18} /> },
    { target: 'users', label: adminStrings.nav.users, icon: <Icon icon="mdi:account-group-outline" width={18} height={18} /> },
    { target: 'alerts', label: adminStrings.nav.alerts, icon: <Icon icon="mdi:bell-ring-outline" width={18} height={18} /> },
    { target: 'proxy-settings', label: adminStrings.nav.proxySettings, icon: <Icon icon="mdi:tune-variant" width={18} height={18} /> },
  ]

  return (
    <AdminShell
      activeItem="keys"
      navItems={navItems}
      skipToContentLabel={adminStrings.accessibility.skipToContent}
      onSelectItem={() => undefined}
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
