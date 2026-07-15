import '../../test/happydom'

import { afterEach, describe, expect, it, mock } from 'bun:test'
import { act } from 'react'
import { createRoot } from 'react-dom/client'

import type { Announcement } from '../api'
import { useUserConsoleAnnouncements } from './useAnnouncements'

const originalFetch = globalThis.fetch
const storageKey = 'tavily-hikari:user-console-announcement-closed'

async function flushEffects(): Promise<void> {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
    await new Promise<void>((resolve) => setTimeout(resolve, 0))
  })
}

const modalAnnouncement: Announcement = {
  id: 'ann-modal-1',
  content: '# Modal notice\n\nModal body',
  displayKind: 'modal',
  status: 'published',
  createdAt: 1,
  updatedAt: 2,
  publishedAt: 2,
  archivedAt: null,
}

const archivedAnnouncement: Announcement = {
  id: 'ann-archived-1',
  content: '# Archived notice\n\nArchived body',
  displayKind: 'ticker',
  status: 'archived',
  createdAt: 1,
  updatedAt: 2,
  publishedAt: 2,
  archivedAt: 3,
}

afterEach(() => {
  globalThis.fetch = originalFetch
  window.localStorage.clear()
  document.body.innerHTML = ''
})

describe('useUserConsoleAnnouncements', () => {
  it('loads active and history announcements and persists closed records locally', async () => {
    const fetchMock = mock((input: RequestInfo | URL) => {
      const path = String(input)
      if (path === '/api/user/announcements') {
        return Promise.resolve(
          new Response(JSON.stringify({ items: [modalAnnouncement] }), {
            status: 200,
            headers: { 'Content-Type': 'application/json' },
          }),
        )
      }
      if (path === '/api/user/announcements/history') {
        return Promise.resolve(
          new Response(JSON.stringify({ items: [modalAnnouncement, archivedAnnouncement] }), {
            status: 200,
            headers: { 'Content-Type': 'application/json' },
          }),
        )
      }
      return Promise.resolve(new Response('not found', { status: 404 }))
    })
    globalThis.fetch = fetchMock as typeof fetch

    let latest:
      | ReturnType<typeof useUserConsoleAnnouncements>
      | null = null
    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)

    function Harness(): null {
      latest = useUserConsoleAnnouncements('enabled', {
        announcements: 'Announcements',
        announcementsUnread: 'Announcements ({count})',
      })
      return null
    }

    await act(async () => {
      root.render(<Harness />)
    })
    await flushEffects()

    expect(fetchMock.mock.calls.map((call) => call[0])).toEqual([
      '/api/user/announcements',
      '/api/user/announcements/history',
    ])
    expect(latest?.activeAnnouncements).toEqual([modalAnnouncement])
    expect(latest?.announcementHistory).toEqual([modalAnnouncement, archivedAnnouncement])
    expect(latest?.visibleAnnouncementCount).toBe(1)
    expect(latest?.announcementButtonLabel).toBe('Announcements (1)')

    await act(async () => {
      latest?.closeAnnouncement(modalAnnouncement.id)
    })
    await flushEffects()

    expect(latest?.visibleAnnouncementCount).toBe(0)
    expect(latest?.announcementButtonLabel).toBe('Announcements')
    const stored = JSON.parse(window.localStorage.getItem(storageKey) ?? '{}') as Record<string, number>
    expect(typeof stored[modalAnnouncement.id]).toBe('number')

    await act(async () => root.unmount())
  })

  it('clears announcement state when the console is not available', async () => {
    const fetchMock = mock(() =>
      Promise.resolve(
        new Response(JSON.stringify({ items: [modalAnnouncement] }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    let latest:
      | ReturnType<typeof useUserConsoleAnnouncements>
      | null = null
    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)

    function Harness({ available }: { available: 'enabled' | 'disabled' }): null {
      latest = useUserConsoleAnnouncements(available, {
        announcements: 'Announcements',
        announcementsUnread: 'Announcements ({count})',
      })
      return null
    }

    await act(async () => {
      root.render(<Harness available="enabled" />)
    })
    await flushEffects()
    expect(latest?.activeAnnouncements).toEqual([modalAnnouncement])

    await act(async () => {
      root.render(<Harness available="disabled" />)
    })
    await flushEffects()

    expect(latest?.activeAnnouncements).toEqual([])
    expect(latest?.announcementHistory).toEqual([])
    expect(latest?.announcementHistoryOpen).toBe(false)

    await act(async () => root.unmount())
  })
})
