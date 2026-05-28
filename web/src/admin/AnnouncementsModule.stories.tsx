import { useLayoutEffect, useState } from 'react'
import type { Meta, StoryObj } from '@storybook/react-vite'

import AnnouncementsModule, { type AnnouncementRouteMode } from './AnnouncementsModule'
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
  routeMode,
}: {
  items?: Announcement[]
  initialMode?: 'list' | 'create'
  routeMode?: AnnouncementRouteMode
}): JSX.Element {
  const [ready, setReady] = useState(false)
  const [storyRouteMode, setStoryRouteMode] = useState<AnnouncementRouteMode>(
    routeMode ?? (initialMode === 'create' ? { kind: 'create' } : { kind: 'list' }),
  )

  useLayoutEffect(() => {
    const cleanup = installAnnouncementsFetchMock(items)
    setReady(true)
    return () => {
      cleanup()
    }
  }, [items])

  useLayoutEffect(() => {
    setStoryRouteMode(routeMode ?? (initialMode === 'create' ? { kind: 'create' } : { kind: 'list' }))
  }, [initialMode, routeMode])

  const navigate = (path: string) => {
    const editMatch = path.match(/^\/admin\/announcements\/([^/]+)\/edit$/)
    if (path === '/admin/announcements/new') {
      setStoryRouteMode({ kind: 'create' })
    } else if (editMatch?.[1]) {
      setStoryRouteMode({ kind: 'edit', id: decodeURIComponent(editMatch[1]) })
    } else {
      setStoryRouteMode({ kind: 'list' })
    }
  }

  if (!ready) {
    return <div style={{ minHeight: 360 }} />
  }

  return (
    <div style={{ padding: 24, background: 'hsl(var(--background))' }}>
      <AnnouncementsModule language="zh" routeMode={storyRouteMode} onNavigate={navigate} />
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
    const duplicatePageHeader = Array.from(canvasElement.querySelectorAll('h2'))
      .some((heading) => heading.textContent?.trim() === '公告')
    if (duplicatePageHeader) {
      throw new Error('Expected announcements module to leave the page title to the admin shell.')
    }
    const actionRows = Array.from(canvasElement.querySelectorAll<HTMLElement>('.announcements-actions'))
    if (actionRows.length === 0) {
      throw new Error('Expected announcement action rows to render.')
    }
    for (const row of actionRows) {
      const buttonTopLines = new Set(
        Array.from(row.querySelectorAll<HTMLElement>('button'))
          .map((button) => Math.round(button.getBoundingClientRect().top)),
      )
      if (buttonTopLines.size > 1) {
        throw new Error('Expected desktop announcement actions to stay on one line.')
      }
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
    const moduleSurface = canvasElement.querySelector<HTMLElement>('.announcements-module')
    const editor = canvasElement.querySelector<HTMLElement>('.announcements-editor')
    if (moduleSurface == null || editor == null) {
      throw new Error('Expected create editor to render inside the announcements module.')
    }
    if (editor.getBoundingClientRect().width < moduleSurface.getBoundingClientRect().width * 0.96) {
      throw new Error('Expected create editor to use the available module width.')
    }
    if (canvasElement.querySelector('.markdown-editor-shell') == null) {
      throw new Error('Expected Markdown editor to render.')
    }
    for (const modeLabel of ['Markdown', '左右对比', '所见即所得']) {
      if (!canvasElement.textContent?.includes(modeLabel)) {
        throw new Error(`Expected create editor to expose ${modeLabel} mode.`)
      }
    }
    if (canvasElement.querySelector('.announcements-body-milkdown-preview') == null) {
      throw new Error('Expected split mode to render a Milkdown-backed read-only render.')
    }
    const splitEditor = canvasElement.querySelector<HTMLElement>('.announcements-body-split')
    const splitInput = canvasElement.querySelector<HTMLElement>('.announcements-body-split > .announcements-body-fallback')
    const splitPreview = canvasElement.querySelector<HTMLElement>('.announcements-body-split > .markdown-editor-shell')
    if (splitEditor == null || splitInput == null || splitPreview == null) {
      throw new Error('Expected split mode to render as one joined editor surface.')
    }
    const splitStyle = window.getComputedStyle(splitEditor)
    const previewStyle = window.getComputedStyle(splitPreview)
    const splitHeight = splitEditor.getBoundingClientRect().height
    if (splitStyle.gap !== '0px') {
      throw new Error('Expected split mode panes to share one surface without a wide gap.')
    }
    if (previewStyle.borderLeftWidth !== '1px') {
      throw new Error('Expected split mode preview to use a light divider.')
    }
    if (splitHeight < 160 || splitHeight > 480) {
      throw new Error('Expected split mode editor height to adapt to content and viewport instead of forcing a page-bottom action area.')
    }
    const bodyHeading = canvasElement.querySelector<HTMLElement>('.announcements-body-heading')
    const modeTabs = canvasElement.querySelector<HTMLElement>('.announcements-body-mode-tabs')
    if (bodyHeading == null || modeTabs == null) {
      throw new Error('Expected body mode tabs to render in the body heading.')
    }
    const headingRight = Math.round(bodyHeading.getBoundingClientRect().right)
    const tabsRight = Math.round(modeTabs.getBoundingClientRect().right)
    if (Math.abs(headingRight - tabsRight) > 4) {
      throw new Error('Expected body mode tabs to align to the right edge of the body heading.')
    }
    const editorActions = canvasElement.querySelector<HTMLElement>('.announcements-editor-header .announcements-editor-actions')
    if (editorActions == null) {
      throw new Error('Expected editor save actions to stay in the editor header.')
    }
    if (canvasElement.querySelector('.announcements-preview') != null) {
      throw new Error('Expected create editor to avoid editor-side user preview.')
    }
    if (!canvasElement.textContent?.includes('保存并发布')) {
      throw new Error('Expected create editor to expose save-and-publish action.')
    }
    const wysiwygTab = Array.from(canvasElement.querySelectorAll<HTMLElement>('button, [role="radio"]'))
      .find((element) => element.textContent?.trim() === '所见即所得')
    wysiwygTab?.click()
    await new Promise((resolve) => window.setTimeout(resolve, 420))
    const wysiwygEditor = canvasElement.querySelector<HTMLElement>('.markdown-editor-shell:not(.markdown-editor-shell--readonly)')
    const wysiwygHeight = wysiwygEditor?.getBoundingClientRect().height ?? 0
    if (wysiwygEditor == null || wysiwygHeight < 160 || wysiwygHeight > 480) {
      throw new Error('Expected WYSIWYG mode to keep an adaptive editor workspace.')
    }
    if (canvasElement.querySelector('.milkdown-toolbar') == null) {
      throw new Error('Expected WYSIWYG mode to expose the floating formatting toolbar.')
    }
    if (canvasElement.querySelector('.milkdown-block-handle') == null) {
      throw new Error('Expected WYSIWYG mode to expose the floating block handle.')
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
    const actionStyle = window.getComputedStyle(canvasElement.querySelector<HTMLElement>('.announcements-editor-actions')!)
    if (actionStyle.position === 'sticky') {
      throw new Error('Expected mobile create actions to avoid sticking to the page bottom.')
    }
    if (canvasElement.querySelector('.announcements-preview') != null) {
      throw new Error('Expected mobile create view to avoid editor-side preview.')
    }
  },
}

export const EditAnnouncementRoute: Story = {
  render: () => <AnnouncementsModuleStory routeMode={{ kind: 'edit', id: 'ann-ticker-01' }} />,
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 180))
    const editor = canvasElement.querySelector('.announcements-editor')
    if (editor == null) {
      throw new Error('Expected edit route to render the announcement editor directly.')
    }
    const titleInput = canvasElement.querySelector<HTMLInputElement>('#announcement-title')
    if (titleInput?.value !== '额度计数已刷新') {
      throw new Error('Expected edit route to load the selected announcement draft.')
    }
    const backButton = Array.from(canvasElement.querySelectorAll<HTMLButtonElement>('button'))
      .find((button) => button.textContent?.includes('返回列表'))
    backButton?.click()
    await new Promise((resolve) => window.setTimeout(resolve, 120))
    if (canvasElement.querySelector('.announcements-table') == null) {
      throw new Error('Expected returning from the edit route to show the announcements list.')
    }
  },
}

export const EditAnnouncementRouteStatic: Story = {
  render: () => <AnnouncementsModuleStory routeMode={{ kind: 'edit', id: 'ann-ticker-01' }} />,
}

export const PreviewFromList: Story = {
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 180))
    const previewButtons = Array.from(canvasElement.querySelectorAll('button'))
      .filter((button) => button.textContent?.trim() === '预览') as HTMLButtonElement[]
    if (previewButtons.length < 2) {
      throw new Error('Expected list rows to expose preview actions.')
    }

    previewButtons[1].click()
    await new Promise((resolve) => window.setTimeout(resolve, 120))
    if (canvasElement.querySelector('.user-console-announcement-ticker') == null) {
      throw new Error('Expected ticker preview to reuse the user-console ticker display.')
    }

    previewButtons[0].click()
    await new Promise((resolve) => window.setTimeout(resolve, 120))
    if (document.body.querySelector('.user-console-announcement-dialog') == null) {
      throw new Error('Expected modal preview to reuse the user-console dialog display.')
    }
  },
}

export const Mobile: Story = {
  parameters: {
    viewport: { defaultViewport: '0390-device-iphone-14' },
  },
  render: () => <AnnouncementsModuleStory />,
}
