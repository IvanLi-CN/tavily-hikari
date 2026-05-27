import { useLayoutEffect, useState } from 'react'
import type { Meta, StoryObj } from '@storybook/react-vite'

import AnnouncementsModule from './AnnouncementsModule'
import type { Announcement } from '../api'

const sampleAnnouncements: Announcement[] = [
  {
    id: 'ann-modal-01',
    title: '维护窗口通知',
    body: '**今晚 23:00 至 23:10** 会重启 Tavily Hikari 服务。\n\n- MCP 会话可能短暂重连\n- HTTP API 会自动重试',
    displayKind: 'modal',
    status: 'published',
    createdAt: 1_762_380_000,
    updatedAt: 1_762_386_000,
    publishedAt: 1_762_386_000,
    archivedAt: null,
  },
  {
    id: 'ann-ticker-01',
    title: '额度计数已刷新',
    body: '每日额度窗口已刷新，用户控制台的 `Token` 详情现在也显示实时请求更新。',
    displayKind: 'ticker',
    status: 'draft',
    createdAt: 1_762_378_000,
    updatedAt: 1_762_385_000,
    publishedAt: null,
    archivedAt: null,
  },
  {
    id: 'ann-archived-01',
    title: '端点迁移完成',
    body: 'Tavily 兼容端点迁移已完成，详见 [迁移记录](https://example.com)。',
    displayKind: 'ticker',
    status: 'archived',
    createdAt: 1_762_200_000,
    updatedAt: 1_762_250_000,
    publishedAt: 1_762_210_000,
    archivedAt: 1_762_250_000,
  },
]
const emptyAnnouncements: Announcement[] = []

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

function installAnnouncementsFetchMock(items: Announcement[]): () => void {
  const originalFetch = window.fetch.bind(window)
  let currentItems = [...items]

  window.fetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const request = input instanceof Request
      ? input
      : new Request(input, init)
    const url = new URL(request.url, window.location.origin)

    if (url.pathname === '/api/announcements' && request.method === 'GET') {
      return jsonResponse({ items: currentItems })
    }

    if (url.pathname === '/api/announcements' && request.method === 'POST') {
      const payload = await request.clone().json().catch(() => ({}))
      const next: Announcement = {
        id: `ann-new-${currentItems.length + 1}`,
        title: payload.title ?? 'New announcement',
        body: payload.body ?? 'Body',
        displayKind: payload.displayKind === 'ticker' ? 'ticker' : 'modal',
        status: 'draft',
        createdAt: 1_762_390_000,
        updatedAt: 1_762_390_000,
        publishedAt: null,
        archivedAt: null,
      }
      currentItems = [next, ...currentItems]
      return jsonResponse(next)
    }

    const match = url.pathname.match(/^\/api\/announcements\/([^/]+)(?:\/(publish|archive))?$/)
    if (match) {
      const id = decodeURIComponent(match[1])
      const action = match[2] ?? 'update'
      const item = currentItems.find((candidate) => candidate.id === id)
      if (!item) return jsonResponse({ message: 'Not found' }, 404)

      if (action === 'publish') {
        item.status = 'published'
        item.publishedAt = item.publishedAt ?? 1_762_391_000
        item.updatedAt = 1_762_391_000
        return jsonResponse(item)
      }
      if (action === 'archive') {
        item.status = 'archived'
        item.archivedAt = item.archivedAt ?? 1_762_391_000
        item.updatedAt = 1_762_391_000
        return jsonResponse(item)
      }
      const payload = await request.clone().json().catch(() => ({}))
      item.title = payload.title ?? item.title
      item.body = payload.body ?? item.body
      item.displayKind = payload.displayKind === 'ticker' ? 'ticker' : 'modal'
      item.updatedAt = 1_762_391_000
      return jsonResponse(item)
    }

    return originalFetch(input, init)
  }

  return () => {
    window.fetch = originalFetch
  }
}

function AnnouncementsModuleStory({
  items = sampleAnnouncements,
  initialMode = 'list',
}: {
  items?: Announcement[]
  initialMode?: 'list' | 'create'
}): JSX.Element {
  const [ready, setReady] = useState(false)

  useLayoutEffect(() => {
    const cleanup = installAnnouncementsFetchMock(items)
    setReady(true)
    return () => {
      cleanup()
    }
  }, [items])

  if (!ready) {
    return <div style={{ minHeight: 360 }} />
  }

  return (
    <div style={{ padding: 24, background: 'hsl(var(--background))' }}>
      <AnnouncementsModule language="zh" initialMode={initialMode} />
    </div>
  )
}

const meta = {
  title: 'Admin/AnnouncementsModule',
  component: AnnouncementsModule,
  tags: ['autodocs'],
  parameters: {
    layout: 'fullscreen',
    docs: {
      description: {
        component:
          'Admin announcement management surface covering draft creation, publish/archive actions, and status scanning.',
      },
    },
  },
  args: {
    language: 'zh',
    refreshToken: 0,
    initialMode: 'list',
  },
  render: () => <AnnouncementsModuleStory />,
} satisfies Meta<typeof AnnouncementsModule>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 180))
    if (canvasElement.querySelector('.announcements-editor') != null) {
      throw new Error('Expected list view to keep the editor off the page.')
    }
    if (canvasElement.querySelector('.announcements-table') == null) {
      throw new Error('Expected announcements table to render.')
    }
    if (canvasElement.querySelector('.announcements-list-header') == null) {
      throw new Error('Expected announcements list header to render.')
    }
  },
}

export const Empty: Story = {
  render: () => <AnnouncementsModuleStory items={emptyAnnouncements} />,
}

export const CreateAnnouncement: Story = {
  render: () => <AnnouncementsModuleStory initialMode="create" />,
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 180))
    if (canvasElement.querySelector('.announcements-table') != null) {
      throw new Error('Expected create view to hide the announcement list.')
    }
    if (canvasElement.querySelector('.announcements-editor') == null) {
      throw new Error('Expected create editor to render.')
    }
    if (canvasElement.querySelector('.markdown-editor-shell') == null) {
      throw new Error('Expected Markdown editor to render.')
    }
    for (const modeLabel of ['Markdown', '左右对比', '所见即所得']) {
      if (!canvasElement.textContent?.includes(modeLabel)) {
        throw new Error(`Expected create editor to expose ${modeLabel} mode.`)
      }
    }
    if (canvasElement.querySelector('.markdown-editor-storybook-toolbar') != null) {
      throw new Error('Expected create editor to avoid persistent toolbar chrome.')
    }
    if (canvasElement.querySelector('.announcements-body-milkdown-preview') == null) {
      throw new Error('Expected split mode to render a Milkdown-backed read-only preview.')
    }
    if (canvasElement.querySelector('.announcements-preview') == null) {
      throw new Error('Expected user-side announcement preview to render.')
    }
    if (!canvasElement.textContent?.includes('保存并发布')) {
      throw new Error('Expected create editor to expose save-and-publish action.')
    }
  },
}

export const CreateAnnouncementMobile: Story = {
  parameters: {
    viewport: { defaultViewport: '0390-device-iphone-14' },
  },
  render: () => <AnnouncementsModuleStory initialMode="create" />,
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 180))
    if (canvasElement.querySelector('.announcements-editor-actions') == null) {
      throw new Error('Expected mobile create view to render editor actions.')
    }
    if (canvasElement.querySelector('.announcements-preview') == null) {
      throw new Error('Expected mobile create view to render preview.')
    }
  },
}

export const Mobile: Story = {
  parameters: {
    viewport: { defaultViewport: '0390-device-iphone-14' },
  },
  render: () => <AnnouncementsModuleStory />,
}
