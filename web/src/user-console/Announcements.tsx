import { useState } from 'react'

import type { Announcement } from '../api'
import { StatusBadge } from '../components/StatusBadge'
import MarkdownContent from '../components/MarkdownContent'
import { Button } from '../components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../components/ui/dialog'
import {
  Drawer,
  DrawerContent,
  DrawerDescription,
  DrawerHeader,
  DrawerTitle,
} from '../components/ui/drawer'
import { Icon } from '../lib/icons'
import type { Language } from '../i18n'
import type { EN } from './text'

type UserConsoleText = typeof EN

interface UserConsoleAnnouncementsProps {
  language: Language
  text: UserConsoleText
  activeAnnouncements: Announcement[]
  historyAnnouncements: Announcement[]
  closedRecords: Record<string, number>
  historyOpen: boolean
  onHistoryOpenChange: (open: boolean) => void
  onCloseAnnouncement: (id: string) => void
}

interface UserConsoleAnnouncementsSectionProps extends UserConsoleAnnouncementsProps {
  hidden: boolean
}

function formatAnnouncementTime(value: number | null, language: Language): string {
  if (!value) return '-'
  try {
    return new Intl.DateTimeFormat(language === 'zh' ? 'zh-CN' : 'en-US', {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    }).format(new Date(value * 1000))
  } catch {
    return '-'
  }
}

function isClosed(item: Announcement, closedRecords: Record<string, number>): boolean {
  return closedRecords[item.id] != null
}

function announcementHistoryTime(item: Announcement): number | null {
  if (item.status === 'archived') {
    return item.archivedAt ?? item.publishedAt ?? item.updatedAt
  }
  return item.publishedAt ?? item.updatedAt
}

function hasAnnouncementBody(item: Announcement): boolean {
  return item.body.trim().length > 0
}

export default function UserConsoleAnnouncements({
  language,
  text,
  activeAnnouncements,
  historyAnnouncements,
  closedRecords,
  historyOpen,
  onHistoryOpenChange,
  onCloseAnnouncement,
}: UserConsoleAnnouncementsProps): JSX.Element {
  const strings = text.announcements
  const [tickerDetailId, setTickerDetailId] = useState<string | null>(null)
  const modalAnnouncement = activeAnnouncements.find((item) => item.displayKind === 'modal' && !isClosed(item, closedRecords))
    ?? null
  const tickerAnnouncement = activeAnnouncements.find((item) => item.displayKind === 'ticker' && !isClosed(item, closedRecords))
    ?? null
  const tickerHasBody = tickerAnnouncement ? hasAnnouncementBody(tickerAnnouncement) : false
  const tickerDetailAnnouncement = tickerAnnouncement?.id === tickerDetailId && tickerHasBody ? tickerAnnouncement : null
  const closeTickerDetailAnnouncement = (id: string) => {
    setTickerDetailId(null)
    onCloseAnnouncement(id)
  }

  return (
    <>
      {tickerAnnouncement ? (
        <section className="surface user-console-announcement-ticker" aria-live="polite">
          {tickerHasBody ? (
            <button
              type="button"
              className="user-console-announcement-ticker-main"
              aria-haspopup="dialog"
              aria-label={strings.tickerOpen.replace('{title}', tickerAnnouncement.title)}
              onClick={() => setTickerDetailId(tickerAnnouncement.id)}
            >
              <span className="user-console-announcement-ticker-icon" aria-hidden="true">
                <Icon icon="mdi:bullhorn-outline" width={18} height={18} />
              </span>
              <span className="user-console-announcement-ticker-copy">
                <strong>{tickerAnnouncement.title}</strong>
              </span>
            </button>
          ) : (
            <div className="user-console-announcement-ticker-main user-console-announcement-ticker-main--static">
              <span className="user-console-announcement-ticker-icon" aria-hidden="true">
                <Icon icon="mdi:bullhorn-outline" width={18} height={18} />
              </span>
              <span className="user-console-announcement-ticker-copy">
                <strong>{tickerAnnouncement.title}</strong>
              </span>
            </div>
          )}
          <Button
            type="button"
            variant="ghost"
            size="xs"
            className="user-console-announcement-close"
            aria-label={tickerHasBody ? strings.tickerOpen.replace('{title}', tickerAnnouncement.title) : strings.tickerClose}
            onClick={() => {
              if (tickerHasBody) {
                setTickerDetailId(tickerAnnouncement.id)
                return
              }
              setTickerDetailId(null)
              onCloseAnnouncement(tickerAnnouncement.id)
            }}
          >
            <Icon
              icon={tickerHasBody ? 'mdi:open-in-new' : 'mdi:close'}
              width={16}
              height={16}
              aria-hidden="true"
            />
          </Button>
        </section>
      ) : null}

      <Dialog
        open={modalAnnouncement != null}
        onOpenChange={(open) => {
          if (!open && modalAnnouncement) {
            onCloseAnnouncement(modalAnnouncement.id)
          }
        }}
      >
        {modalAnnouncement ? (
          <DialogContent className="user-console-announcement-dialog" aria-describedby={undefined}>
            <DialogHeader>
              <DialogTitle>{modalAnnouncement.title}</DialogTitle>
            </DialogHeader>
            <MarkdownContent
              content={modalAnnouncement.body}
              className="user-console-announcement-dialog-body"
            />
            <DialogFooter>
              <Button type="button" onClick={() => onCloseAnnouncement(modalAnnouncement.id)}>
                {strings.modalAcknowledge}
              </Button>
            </DialogFooter>
          </DialogContent>
        ) : null}
      </Dialog>

      <Dialog
        open={tickerDetailAnnouncement != null}
        onOpenChange={(open) => {
          if (!open && tickerDetailAnnouncement) {
            closeTickerDetailAnnouncement(tickerDetailAnnouncement.id)
          }
        }}
      >
        {tickerDetailAnnouncement ? (
          <DialogContent className="user-console-announcement-dialog" aria-describedby={undefined}>
            <DialogHeader>
              <DialogTitle>{tickerDetailAnnouncement.title}</DialogTitle>
            </DialogHeader>
            <MarkdownContent
              content={tickerDetailAnnouncement.body}
              className="user-console-announcement-dialog-body"
            />
            <DialogFooter>
              <Button
                type="button"
                onClick={() => closeTickerDetailAnnouncement(tickerDetailAnnouncement.id)}
              >
                {strings.modalAcknowledge}
              </Button>
            </DialogFooter>
          </DialogContent>
        ) : null}
      </Dialog>

      <Drawer open={historyOpen} onOpenChange={onHistoryOpenChange} shouldScaleBackground={false}>
        <DrawerContent className="user-console-announcement-history">
          <DrawerHeader>
            <DrawerTitle>{strings.historyTitle}</DrawerTitle>
            <DrawerDescription>{strings.historyDescription}</DrawerDescription>
          </DrawerHeader>
          <div className="user-console-announcement-history-list">
            {historyAnnouncements.length === 0 ? (
              <div className="empty-state alert">{strings.emptyHistory}</div>
            ) : (
              historyAnnouncements.map((item) => (
                <article key={item.id} className="user-console-announcement-history-item">
                  <header>
                    <div>
                      <strong>{item.title}</strong>
                      <span>
                        {item.displayKind === 'ticker' ? strings.ticker : strings.modal}
                        {' · '}
                        {formatAnnouncementTime(announcementHistoryTime(item), language)}
                      </span>
                    </div>
                    <StatusBadge tone={item.status === 'published' ? 'success' : 'neutral'}>
                      {item.status === 'published' ? strings.published : strings.archived}
                    </StatusBadge>
                  </header>
                  <MarkdownContent
                    content={item.body}
                    className="user-console-announcement-history-body"
                  />
                  {isClosed(item, closedRecords) ? (
                    <div className="user-console-announcement-closed">
                      <Icon icon="mdi:check-circle-outline" width={16} height={16} aria-hidden="true" />
                      <span>
                        {strings.closedAt.replace(
                          '{time}',
                          formatAnnouncementTime(closedRecords[item.id], language),
                        )}
                      </span>
                    </div>
                  ) : null}
                </article>
              ))
            )}
          </div>
        </DrawerContent>
      </Drawer>
    </>
  )
}

export function UserConsoleAnnouncementsSection({
  hidden,
  ...props
}: UserConsoleAnnouncementsSectionProps): JSX.Element | null {
  if (hidden) return null
  return <UserConsoleAnnouncements {...props} />
}
