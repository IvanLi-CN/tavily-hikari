import '../../test/happydom'

import { afterEach, describe, expect, it, mock } from 'bun:test'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'

import type { Announcement } from '../api'
import { EN } from './text'
import UserConsoleAnnouncements from './Announcements'

function tickerAnnouncement(patch: Partial<Announcement> = {}): Announcement {
  return {
    id: 'ann-ticker-1',
    title: 'Quota refreshed',
    body: 'Daily quota counters have refreshed.',
    displayKind: 'ticker',
    status: 'published',
    createdAt: 1,
    updatedAt: 2,
    publishedAt: 2,
    archivedAt: null,
    ...patch,
  }
}

async function renderAnnouncements(
  announcement: Announcement,
  onCloseAnnouncement = mock(() => {}),
): Promise<{ root: Root; onCloseAnnouncement: ReturnType<typeof mock> }> {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)

  await act(async () => {
    root.render(
      <UserConsoleAnnouncements
        language="en"
        text={EN}
        activeAnnouncements={[announcement]}
        historyAnnouncements={[]}
        closedRecords={{}}
        historyOpen={false}
        onHistoryOpenChange={() => {}}
        onCloseAnnouncement={onCloseAnnouncement}
      />,
    )
  })

  return { root, onCloseAnnouncement }
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('UserConsoleAnnouncements', () => {
  it('opens ticker details instead of dismissing when body content exists', async () => {
    const item = tickerAnnouncement()
    const { root, onCloseAnnouncement } = await renderAnnouncements(item)

    const ticker = document.querySelector<HTMLElement>('.user-console-announcement-ticker')
    expect(ticker?.textContent).toContain(item.title)
    expect(ticker?.textContent).not.toContain(item.body)

    const detailButton = document.querySelector<HTMLButtonElement>(
      `button[aria-label="${EN.announcements.tickerOpen.replace('{title}', item.title)}"]`,
    )
    expect(detailButton).not.toBeNull()

    await act(async () => {
      detailButton?.click()
    })

    expect(onCloseAnnouncement).not.toHaveBeenCalled()
    expect(document.querySelector('.user-console-announcement-ticker')).not.toBeNull()

    await act(async () => root.unmount())
  })

  it('dismisses ticker notifications directly when body content is empty', async () => {
    const item = tickerAnnouncement({ body: '' })
    const { root, onCloseAnnouncement } = await renderAnnouncements(item)

    expect(document.querySelector('.user-console-announcement-ticker-main--static')).not.toBeNull()

    const closeButton = document.querySelector<HTMLButtonElement>(
      `button[aria-label="${EN.announcements.tickerClose}"]`,
    )
    expect(closeButton).not.toBeNull()

    await act(async () => {
      closeButton?.click()
    })

    expect(onCloseAnnouncement).toHaveBeenCalledWith(item.id)
    expect(document.querySelector('.user-console-announcement-dialog')).toBeNull()

    await act(async () => root.unmount())
  })
})
