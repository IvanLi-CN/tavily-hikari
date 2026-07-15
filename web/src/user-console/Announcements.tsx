import { useMemo, useState } from 'react'

import type { Announcement } from '../api'
import MarkdownContent from '../components/MarkdownContent'
import { StatusBadge } from '../components/StatusBadge'
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
import type { Language } from '../i18n'
import { Icon } from '../lib/icons'
import { parseAnnouncementContent } from '../lib/announcementContent'
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

function AnnouncementTitleMarkdown({
  markdown,
  className,
}: {
  markdown: string
  className?: string
}): JSX.Element {
  return <MarkdownContent content={markdown} inline className={className} />
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

  const modalParsed = useMemo(
    () => (modalAnnouncement ? parseAnnouncementContent(modalAnnouncement.content) : null),
    [modalAnnouncement],
  )
  const tickerParsed = useMemo(
    () => (tickerAnnouncement ? parseAnnouncementContent(tickerAnnouncement.content) : null),
    [tickerAnnouncement],
  )
  const tickerHasDetails = Boolean(tickerParsed?.hasTitle && tickerParsed.hasBody)
  const tickerHasTitle = Boolean(tickerParsed?.hasTitle)
  const tickerDetailAnnouncement = tickerAnnouncement?.id === tickerDetailId && tickerHasDetails ? tickerAnnouncement : null
  const tickerDetailParsed = useMemo(
    () => (tickerDetailAnnouncement ? parseAnnouncementContent(tickerDetailAnnouncement.content) : null),
    [tickerDetailAnnouncement],
  )

  const closeTickerDetailAnnouncement = (id: string) => {
    setTickerDetailId(null)
    onCloseAnnouncement(id)
  }

  return (
    <>
      {tickerAnnouncement && tickerParsed ? (
        <section className="surface user-console-announcement-ticker" aria-live="polite">
          <div
            className={[
              'user-console-announcement-ticker-main',
              tickerHasTitle ? 'user-console-announcement-ticker-main--titled' : 'user-console-announcement-ticker-main--untitled',
            ].join(' ')}
          >
            <span className="user-console-announcement-ticker-icon" aria-hidden="true">
              <Icon icon="mdi:bullhorn-outline" width={18} height={18} />
            </span>
            <span className="user-console-announcement-ticker-copy">
              {tickerHasTitle && tickerParsed.titleMarkdown ? (
                <AnnouncementTitleMarkdown
                  markdown={tickerParsed.titleMarkdown}
                  className="user-console-announcement-ticker-title"
                />
              ) : (
                <MarkdownContent
                  content={tickerParsed.fullContent}
                  compactWrap
                  className="user-console-announcement-ticker-content"
                />
              )}
            </span>
          </div>
          {tickerHasDetails ? (
            <Button
              type="button"
              variant="ghost"
              size="xs"
              className="user-console-announcement-action"
              aria-label={strings.tickerOpen.replace('{title}', tickerParsed.titleText ?? strings.ticker)}
              onClick={() => setTickerDetailId(tickerAnnouncement.id)}
            >
              <Icon icon="mdi:open-in-new" width={16} height={16} aria-hidden="true" />
              <span>{strings.tickerDetails}</span>
            </Button>
          ) : (
            <Button
              type="button"
              variant="ghost"
              size="xs"
              className="user-console-announcement-close"
              aria-label={strings.tickerClose}
              onClick={() => {
                setTickerDetailId(null)
                onCloseAnnouncement(tickerAnnouncement.id)
              }}
            >
              <Icon icon="mdi:close" width={16} height={16} aria-hidden="true" />
            </Button>
          )}
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
        {modalAnnouncement && modalParsed?.titleMarkdown ? (
          <DialogContent className="user-console-announcement-dialog" aria-describedby={undefined}>
            <DialogHeader>
              <DialogTitle>
                <AnnouncementTitleMarkdown
                  markdown={modalParsed.titleMarkdown}
                  className="user-console-announcement-dialog-title"
                />
              </DialogTitle>
            </DialogHeader>
            <MarkdownContent
              content={modalParsed.bodyMarkdown}
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
        {tickerDetailAnnouncement && tickerDetailParsed?.titleMarkdown ? (
          <DialogContent className="user-console-announcement-dialog" aria-describedby={undefined}>
            <DialogHeader>
              <DialogTitle>
                <AnnouncementTitleMarkdown
                  markdown={tickerDetailParsed.titleMarkdown}
                  className="user-console-announcement-dialog-title"
                />
              </DialogTitle>
            </DialogHeader>
            <MarkdownContent
              content={tickerDetailParsed.bodyMarkdown}
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
              historyAnnouncements.map((item) => {
                const parsed = parseAnnouncementContent(item.content)
                const historyContent = parsed.hasTitle ? parsed.bodyMarkdown : parsed.fullContent

                return (
                  <article key={item.id} className="user-console-announcement-history-item">
                    <header>
                      <div>
                        {parsed.titleMarkdown ? (
                          <AnnouncementTitleMarkdown
                            markdown={parsed.titleMarkdown}
                            className="user-console-announcement-history-title"
                          />
                        ) : null}
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
                    {historyContent ? (
                      <MarkdownContent
                        content={historyContent}
                        className="user-console-announcement-history-body"
                      />
                    ) : null}
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
                )
              })
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
