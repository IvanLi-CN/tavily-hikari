import '../../test/happydom'

import { afterEach, describe, expect, it, mock } from 'bun:test'
import { act } from 'react'
import type { ComponentProps, ReactNode } from 'react'
import { createRoot, type Root } from 'react-dom/client'

import type { Announcement } from '../api'
import { EN } from './text'

mock.module('../components/ui/dialog', () => {
  const Dialog = ({ open, children }: { open: boolean; children?: ReactNode }) => open ? <>{children}</> : null
  const DialogContent = ({ children, ...props }: ComponentProps<'div'>) => <div {...props}>{children}</div>
  const DialogHeader = ({ children, ...props }: ComponentProps<'div'>) => <div {...props}>{children}</div>
  const DialogFooter = ({ children, ...props }: ComponentProps<'div'>) => <div {...props}>{children}</div>
  const DialogTitle = ({ children, ...props }: ComponentProps<'h2'>) => <h2 {...props}>{children}</h2>
  const DialogDescription = ({ children, ...props }: ComponentProps<'p'>) => <p {...props}>{children}</p>

  return { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle }
})

mock.module('../components/ui/drawer', () => {
  const Drawer = ({ open, children }: { open: boolean; children?: ReactNode }) => open ? <>{children}</> : null
  const DrawerClose = ({ children }: { children?: ReactNode }) => <>{children}</>
  const DrawerContent = ({ children, direction: _direction, ...props }: ComponentProps<'div'> & {
    direction?: 'bottom' | 'right'
  }) => <div data-state="open" {...props}>{children}</div>
  const DrawerHeader = ({ children, ...props }: ComponentProps<'div'>) => <div {...props}>{children}</div>
  const DrawerDescription = ({ children, ...props }: ComponentProps<'p'>) => <p {...props}>{children}</p>
  const DrawerTitle = ({ children, ...props }: ComponentProps<'h2'>) => <h2 {...props}>{children}</h2>

  return { Drawer, DrawerClose, DrawerContent, DrawerDescription, DrawerHeader, DrawerTitle }
})

const { default: UserConsoleAnnouncements } = await import('./Announcements')

const renderedRoots = new Set<Root>()

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

function renderAnnouncements({
  activeAnnouncements = [tickerAnnouncement()],
  historyAnnouncements = [],
  closedRecords = {},
  historyOpen = false,
  onHistoryOpenChange = mock(() => {}),
  onCloseAnnouncement = mock(() => {}),
}: {
  activeAnnouncements?: Announcement[]
  historyAnnouncements?: Announcement[]
  closedRecords?: Record<string, number>
  historyOpen?: boolean
  onHistoryOpenChange?: ReturnType<typeof mock>
  onCloseAnnouncement?: ReturnType<typeof mock>
} = {}): { root: Root; onCloseAnnouncement: ReturnType<typeof mock> } {
  const container = document.createElement('div')
  const root = createRoot(container)
  renderedRoots.add(root)

  act(() => {
    root.render(
      <UserConsoleAnnouncements
        language="en"
        text={EN}
        activeAnnouncements={activeAnnouncements}
        historyAnnouncements={historyAnnouncements}
        closedRecords={closedRecords}
        historyOpen={historyOpen}
        onHistoryOpenChange={onHistoryOpenChange}
        onCloseAnnouncement={onCloseAnnouncement}
      />,
    )
  })

  return { container, root, onCloseAnnouncement }
}

async function unmountRoot(root: Root): Promise<void> {
  renderedRoots.delete(root)
  await act(async () => root.unmount())
}

afterEach(async () => {
  for (const root of renderedRoots) {
    await unmountRoot(root)
  }
})

describe('UserConsoleAnnouncements', () => {
  it('opens ticker details instead of dismissing when body content exists', async () => {
    const item = tickerAnnouncement()
    const { container, root, onCloseAnnouncement } = renderAnnouncements({ activeAnnouncements: [item] })

    const ticker = container.querySelector<HTMLElement>('.user-console-announcement-ticker')
    expect(ticker?.textContent).toContain('Quota refreshed')
    expect(ticker?.textContent).not.toContain('Daily quota counters have refreshed.')

    const detailButton = container.querySelector<HTMLButtonElement>(
      `button[aria-label="${EN.announcements.tickerOpen.replace('{title}', 'Quota refreshed')}"]`,
    )
    expect(detailButton).not.toBeNull()

    await act(async () => {
      detailButton?.click()
    })

    expect(onCloseAnnouncement).not.toHaveBeenCalled()
    expect(container.querySelector('.user-console-announcement-ticker')).not.toBeNull()

    await unmountRoot(root)
  })

  it('keeps inline markdown links clickable inside derived titles', async () => {
    const item = tickerAnnouncement({
      content: '# Check the [status page](https://example.com)\n\nAdditional details.',
    })
    const { container, root } = renderAnnouncements({ activeAnnouncements: [item] })

    const titleLink = container.querySelector<HTMLAnchorElement>('.user-console-announcement-ticker-title a')
    expect(titleLink?.getAttribute('href')).toBe('https://example.com')

    const detailButton = container.querySelector<HTMLButtonElement>(
      `button[aria-label="${EN.announcements.tickerOpen.replace('{title}', 'Check the status page')}"]`,
    )
    expect(detailButton).not.toBeNull()

    await unmountRoot(root)
  })

  it('dismisses ticker notifications directly when only a title exists', async () => {
    const item = tickerAnnouncement({ content: '# Quota refreshed' })
    const { container, root, onCloseAnnouncement } = renderAnnouncements({ activeAnnouncements: [item] })

    expect(container.querySelector('.user-console-announcement-ticker-main--titled')).not.toBeNull()

    const closeButton = container.querySelector<HTMLButtonElement>(
      `button[aria-label="${EN.announcements.tickerClose}"]`,
    )
    expect(closeButton).not.toBeNull()

    await act(async () => {
      closeButton?.click()
    })

    expect(onCloseAnnouncement).toHaveBeenCalledWith(item.id)
    expect(container.querySelector('.user-console-announcement-dialog')).toBeNull()

    await unmountRoot(root)
  })

  it('renders untitled ticker content inline without opening details', async () => {
    const item = tickerAnnouncement({
      id: 'ann-ticker-untitled',
      content: 'Check the [status page](https://example.com) for live updates.',
    })
    const { container, root, onCloseAnnouncement } = renderAnnouncements({ activeAnnouncements: [item] })

    const ticker = container.querySelector<HTMLElement>('.user-console-announcement-ticker')
    expect(ticker?.textContent).toContain('Check the status page for live updates.')
    expect(container.querySelector('.user-console-announcement-ticker-main--untitled')).not.toBeNull()
    expect(container.querySelector(`button[aria-label="${EN.announcements.tickerDetails}"]`)).toBeNull()

    const link = container.querySelector<HTMLAnchorElement>('.user-console-announcement-ticker-content a')
    expect(link?.getAttribute('href')).toBe('https://example.com')

    const closeButton = container.querySelector<HTMLButtonElement>(
      `button[aria-label="${EN.announcements.tickerClose}"]`,
    )
    expect(closeButton).not.toBeNull()

    await act(async () => {
      closeButton?.click()
    })

    expect(onCloseAnnouncement).toHaveBeenCalledWith(item.id)
    await unmountRoot(root)
  })

  it('offers mark as read only for an unclosed published ticker in history', async () => {
    const item = tickerAnnouncement({ id: 'ann-history-unread' })
    const onCloseAnnouncement = mock(() => {})
    const { container, root } = renderAnnouncements({
      activeAnnouncements: [],
      historyAnnouncements: [item],
      historyOpen: true,
      onCloseAnnouncement,
    })

    const historyItem = container.querySelector<HTMLElement>('.user-console-announcement-history-item')
    expect(historyItem?.textContent).toContain('Quota refreshed')
    expect(historyItem?.textContent).toContain(EN.announcements.published)
    const markReadButton = Array.from(historyItem?.querySelectorAll('button') ?? [])
      .find((button) => button.textContent?.includes(EN.announcements.markRead))
    expect(markReadButton).not.toBeNull()

    await act(async () => {
      markReadButton?.click()
    })

    expect(onCloseAnnouncement).toHaveBeenCalledWith(item.id)
    await unmountRoot(root)
  })

  it('shows handled time without a mark-read action for closed published tickers', async () => {
    const item = tickerAnnouncement({ id: 'ann-history-closed' })
    const { container, root } = renderAnnouncements({
      activeAnnouncements: [],
      historyAnnouncements: [item],
      closedRecords: { [item.id]: 1_762_390_120 },
      historyOpen: true,
    })

    const historyItem = container.querySelector<HTMLElement>('.user-console-announcement-history-item')
    expect(historyItem?.textContent).toContain('Handled')
    expect(historyItem?.textContent).not.toContain(EN.announcements.markRead)

    await unmountRoot(root)
  })

  it('does not offer mark as read for published modal notices', async () => {
    const item = tickerAnnouncement({
      id: 'ann-history-modal',
      displayKind: 'modal',
      content: '# Scheduled maintenance\n\nThe service will restart tonight.',
    })
    const { container, root } = renderAnnouncements({
      activeAnnouncements: [],
      historyAnnouncements: [item],
      historyOpen: true,
    })

    const historyItem = container.querySelector<HTMLElement>('.user-console-announcement-history-item')
    expect(historyItem?.textContent).toContain('Scheduled maintenance')
    expect(historyItem?.textContent).toContain(EN.announcements.published)
    expect(historyItem?.textContent).not.toContain(EN.announcements.markRead)
    expect(historyItem?.querySelector('button')).toBeNull()

    await unmountRoot(root)
  })

  it('keeps archived notices in history without an archived badge or mark-read action', async () => {
    const item = tickerAnnouncement({
      id: 'ann-history-archived',
      status: 'archived',
      archivedAt: 3,
      content: '# Migration complete\n\nThe endpoint migration is complete.',
    })
    const { container, root } = renderAnnouncements({
      activeAnnouncements: [],
      historyAnnouncements: [item],
      historyOpen: true,
    })

    const historyItem = container.querySelector<HTMLElement>('.user-console-announcement-history-item')
    expect(historyItem?.textContent).toContain('Migration complete')
    expect(historyItem?.textContent).not.toContain('Archived')
    expect(historyItem?.textContent).not.toContain(EN.announcements.markRead)
    expect(historyItem?.querySelector('.status-badge')).toBeNull()

    await unmountRoot(root)
  })

  it('provides a named close action and an empty state in announcement history', async () => {
    const { container, root } = renderAnnouncements({
      activeAnnouncements: [],
      historyOpen: true,
    })

    expect(container.querySelector(`button[aria-label="${EN.announcements.closeHistory}"]`)).not.toBeNull()
    expect(container.querySelector('.user-console-announcement-history-list')?.textContent)
      .toContain(EN.announcements.emptyHistory)

    await unmountRoot(root)
  })
})
