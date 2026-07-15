export interface DemoAnnouncement {
  id: string
  content: string
  displayKind: 'modal' | 'ticker'
  status: 'draft' | 'published' | 'archived'
  createdAt: number
  updatedAt: number
  publishedAt: number | null
  archivedAt: number | null
}

interface DemoAnnouncementRouteContext {
  announcements: DemoAnnouncement[]
  path: string
  method: string
  init?: RequestInit
  nowSeconds: () => number
  readJsonBody: (init?: RequestInit) => Promise<Record<string, unknown>>
  jsonResponse: (payload: unknown, status?: number) => Response
}

export function createDemoAnnouncements(nowSeconds: (offset?: number) => number): DemoAnnouncement[] {
  return [
    {
      id: 'ann-demo-modal',
      content: '# 维护窗口通知\n\n**今晚 23:00 至 23:10** 会重启 Tavily Hikari 服务。\n\n- MCP 会话可能短暂重连\n- HTTP API 会自动重试',
      displayKind: 'modal',
      status: 'published',
      createdAt: nowSeconds(-7200),
      updatedAt: nowSeconds(-3600),
      publishedAt: nowSeconds(-3600),
      archivedAt: null,
    },
    {
      id: 'ann-demo-ticker',
      content: '# 额度计数已刷新\n\n每日额度窗口已刷新，用户控制台的 `Token` 详情现在也显示实时请求更新。',
      displayKind: 'ticker',
      status: 'draft',
      createdAt: nowSeconds(-5400),
      updatedAt: nowSeconds(-3000),
      publishedAt: null,
      archivedAt: null,
    },
    {
      id: 'ann-demo-archived',
      content: '# 端点迁移完成\n\nTavily 兼容端点迁移已完成，详见 [迁移记录](https://example.com)。',
      displayKind: 'ticker',
      status: 'archived',
      createdAt: nowSeconds(-172800),
      updatedAt: nowSeconds(-86400),
      publishedAt: nowSeconds(-160000),
      archivedAt: nowSeconds(-86400),
    },
  ]
}

export function demoUserActiveAnnouncements(announcements: DemoAnnouncement[]): DemoAnnouncement[] {
  return ['modal', 'ticker'].flatMap((displayKind) => {
    const item = announcements
      .filter((announcement) => announcement.status === 'published' && announcement.displayKind === displayKind)
      .sort((a, b) => (b.publishedAt ?? b.updatedAt) - (a.publishedAt ?? a.updatedAt))[0]
    return item ? [item] : []
  })
}

export function demoUserAnnouncementHistory(announcements: DemoAnnouncement[]): DemoAnnouncement[] {
  return announcements
    .filter((announcement) => announcement.status === 'published' || (announcement.status === 'archived' && announcement.publishedAt != null))
    .sort((a, b) => historyTime(b) - historyTime(a))
}

export async function handleAnnouncementsRoute({
  announcements,
  path,
  method,
  init,
  nowSeconds,
  readJsonBody,
  jsonResponse,
}: DemoAnnouncementRouteContext): Promise<Response> {
  if (path === '/api/announcements' && method === 'GET') return jsonResponse({ items: announcements })

  if (path === '/api/announcements' && method === 'POST') {
    const payload = await demoAnnouncementPayload(readJsonBody, init)
    const now = nowSeconds()
    const item: DemoAnnouncement = {
      id: nextDemoAnnouncementId(announcements),
      ...payload,
      status: 'draft',
      createdAt: now,
      updatedAt: now,
      publishedAt: null,
      archivedAt: null,
    }
    announcements.unshift(item)
    return jsonResponse(item)
  }

  const match = path.match(/^\/api\/announcements\/([^/]+)(?:\/(publish|archive))?$/)
  if (!match) return jsonResponse({ items: announcements })

  const id = decodeURIComponent(match[1])
  const action = match[2] ?? 'update'
  const item = announcements.find((announcement) => announcement.id === id)
  if (!item) return jsonResponse({ message: 'announcement not found' }, 404)
  const now = nowSeconds()

  if (action === 'archive') {
    item.status = 'archived'
    item.updatedAt = now
    item.archivedAt = item.archivedAt ?? now
    return jsonResponse(item)
  }

  if (action === 'publish') {
    if (item.status === 'archived') {
      const next: DemoAnnouncement = {
        ...item,
        id: nextDemoAnnouncementId(announcements),
        status: 'published',
        createdAt: now,
        updatedAt: now,
        publishedAt: now,
        archivedAt: null,
      }
      announcements.unshift(next)
      return jsonResponse(next)
    }
    item.status = 'published'
    item.updatedAt = now
    item.publishedAt = item.publishedAt ?? now
    item.archivedAt = null
    return jsonResponse(item)
  }

  const payload = await demoAnnouncementPayload(readJsonBody, init)
  if (item.status === 'published') {
    item.status = 'archived'
    item.updatedAt = now
    item.archivedAt = now
    const next: DemoAnnouncement = {
      id: nextDemoAnnouncementId(announcements),
      ...payload,
      status: 'published',
      createdAt: now,
      updatedAt: now,
      publishedAt: now,
      archivedAt: null,
    }
    announcements.unshift(next)
    return jsonResponse(next)
  }

  if (item.status === 'archived') {
    const next: DemoAnnouncement = {
      id: nextDemoAnnouncementId(announcements),
      ...payload,
      status: 'draft',
      createdAt: now,
      updatedAt: now,
      publishedAt: null,
      archivedAt: null,
    }
    announcements.unshift(next)
    return jsonResponse(next)
  }

  Object.assign(item, payload, { updatedAt: now })
  return jsonResponse(item)
}

function historyTime(announcement: DemoAnnouncement): number {
  if (announcement.status === 'archived') {
    return announcement.archivedAt ?? announcement.publishedAt ?? announcement.updatedAt
  }
  return announcement.publishedAt ?? announcement.updatedAt
}

function nextDemoAnnouncementId(announcements: DemoAnnouncement[]): string {
  return `ann-demo-${announcements.length + 1}-${Date.now().toString(36)}`
}

async function demoAnnouncementPayload(
  readJsonBody: (init?: RequestInit) => Promise<Record<string, unknown>>,
  init?: RequestInit,
) {
  const body = await readJsonBody(init)
  return {
    content: typeof body.content === 'string' && body.content.trim()
      ? body.content.trim()
      : '# Demo announcement\n\nDemo announcement body.',
    displayKind: body.displayKind === 'ticker' ? 'ticker' as const : 'modal' as const,
  }
}
