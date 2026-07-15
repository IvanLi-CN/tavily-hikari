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
    content: '# Quota refreshed\n\nDaily quota counters have refreshed.',
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
    expect(ticker?.textContent).toContain('Quota refreshed')
    expect(ticker?.textContent).not.toContain('Daily quota counters have refreshed.')

    const detailButton = document.querySelector<HTMLButtonElement>(
      `button[aria-label="${EN.announcements.tickerOpen.replace('{title}', 'Quota refreshed')}"]`,
    )
    expect(detailButton).not.toBeNull()

    await act(async () => {
      detailButton?.click()
    })

    expect(onCloseAnnouncement).not.toHaveBeenCalled()
    expect(document.querySelector('.user-console-announcement-ticker')).not.toBeNull()

    await act(async () => root.unmount())
  })

  it('keeps inline markdown links clickable inside derived titles', async () => {
    const item = tickerAnnouncement({
      content: '# Check the [status page](https://example.com)\n\nAdditional details.',
    })
    const { root } = await renderAnnouncements(item)

    const titleLink = document.querySelector<HTMLAnchorElement>('.user-console-announcement-ticker-title a')
    expect(titleLink?.getAttribute('href')).toBe('https://example.com')

    const detailButton = document.querySelector<HTMLButtonElement>(
      `button[aria-label="${EN.announcements.tickerOpen.replace('{title}', 'Check the status page')}"]`,
    )
    expect(detailButton).not.toBeNull()

    await act(async () => root.unmount())
  })

  it('dismisses ticker notifications directly when only a title exists', async () => {
    const item = tickerAnnouncement({ content: '# Quota refreshed' })
    const { root, onCloseAnnouncement } = await renderAnnouncements(item)

    expect(document.querySelector('.user-console-announcement-ticker-main--titled')).not.toBeNull()

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

  it('renders untitled ticker content inline without opening details', async () => {
    const item = tickerAnnouncement({
      id: 'ann-ticker-untitled',
      content: 'Check the [status page](https://example.com) for live updates.',
    })
    const { root, onCloseAnnouncement } = await renderAnnouncements(item)

    const ticker = document.querySelector<HTMLElement>('.user-console-announcement-ticker')
    expect(ticker?.textContent).toContain('Check the status page for live updates.')
    expect(document.querySelector('.user-console-announcement-ticker-main--untitled')).not.toBeNull()
    expect(document.querySelector(`button[aria-label="${EN.announcements.tickerDetails}"]`)).toBeNull()

    const link = document.querySelector<HTMLAnchorElement>('.user-console-announcement-ticker-content a')
    expect(link?.getAttribute('href')).toBe('https://example.com')

    const closeButton = document.querySelector<HTMLButtonElement>(
      `button[aria-label="${EN.announcements.tickerClose}"]`,
    )
    expect(closeButton).not.toBeNull()

    await act(async () => {
      closeButton?.click()
    })

    expect(onCloseAnnouncement).toHaveBeenCalledWith(item.id)
    await act(async () => root.unmount())
  })
})
