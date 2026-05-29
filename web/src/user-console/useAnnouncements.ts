import { useCallback, useEffect, useState } from 'react'

import {
  fetchUserAnnouncementHistory,
  fetchUserAnnouncements,
  type Announcement,
} from '../api'
import type { UserConsoleAvailability } from '../lib/userConsoleAvailability'

const USER_CONSOLE_ANNOUNCEMENTS_STORAGE_KEY = 'tavily-hikari:user-console-announcement-closed'

interface AnnouncementHeaderText {
  announcements: string
  announcementsUnread: string
}

function formatTemplate(
  template: string,
  values: Record<string, string | number>,
): string {
  return Object.entries(values).reduce(
    (current, [key, value]) => current.replace(new RegExp(`\\{${key}\\}`, 'g'), String(value)),
    template,
  )
}

function readClosedAnnouncementRecords(): Record<string, number> {
  if (typeof window === 'undefined') return {}
  try {
    const raw = window.localStorage.getItem(USER_CONSOLE_ANNOUNCEMENTS_STORAGE_KEY)
    if (!raw) return {}
    const parsed = JSON.parse(raw) as unknown
    if (!parsed || typeof parsed !== 'object') return {}
    const records: Record<string, number> = {}
    for (const [id, value] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof value === 'number' && Number.isFinite(value)) records[id] = value
    }
    return records
  } catch {
    return {}
  }
}

function writeClosedAnnouncementRecords(records: Record<string, number>): void {
  if (typeof window === 'undefined') return
  try {
    window.localStorage.setItem(USER_CONSOLE_ANNOUNCEMENTS_STORAGE_KEY, JSON.stringify(records))
  } catch {
    // Storage can be unavailable in private or restricted browser contexts.
  }
}

export function useUserConsoleAnnouncements(
  consoleAvailability: UserConsoleAvailability,
  text: AnnouncementHeaderText,
): {
  activeAnnouncements: Announcement[]
  announcementHistory: Announcement[]
  closedAnnouncements: Record<string, number>
  announcementHistoryOpen: boolean
  visibleAnnouncementCount: number
  announcementButtonLabel: string
  setAnnouncementHistoryOpen: (open: boolean) => void
  closeAnnouncement: (id: string) => void
  clearAnnouncements: () => void
} {
  const [activeAnnouncements, setActiveAnnouncements] = useState<Announcement[]>([])
  const [announcementHistory, setAnnouncementHistory] = useState<Announcement[]>([])
  const [closedAnnouncements, setClosedAnnouncements] = useState<Record<string, number>>(
    () => readClosedAnnouncementRecords(),
  )
  const [announcementHistoryOpen, setAnnouncementHistoryOpen] = useState(false)

  const clearAnnouncements = useCallback(() => {
    setActiveAnnouncements([])
    setAnnouncementHistory([])
    setAnnouncementHistoryOpen(false)
  }, [])

  useEffect(() => {
    if (consoleAvailability !== 'enabled') {
      clearAnnouncements()
      return
    }
    const controller = new AbortController()
    Promise.all([
      fetchUserAnnouncements(controller.signal),
      fetchUserAnnouncementHistory(controller.signal),
    ])
      .then(([active, history]) => {
        if (controller.signal.aborted) return
        setActiveAnnouncements(active.items)
        setAnnouncementHistory(history.items)
      })
      .catch((err) => {
        if (!controller.signal.aborted) console.error('failed to load user announcements', err)
      })
    return () => controller.abort()
  }, [clearAnnouncements, consoleAvailability])

  const closeAnnouncement = useCallback((id: string) => {
    setClosedAnnouncements((current) => {
      const nextRecords = { ...current, [id]: Math.floor(Date.now() / 1000) }
      writeClosedAnnouncementRecords(nextRecords)
      return nextRecords
    })
  }, [])

  const visibleAnnouncementCount = activeAnnouncements.filter((item) => closedAnnouncements[item.id] == null).length
  const announcementButtonLabel = visibleAnnouncementCount > 0
    ? formatTemplate(text.announcementsUnread, { count: visibleAnnouncementCount })
    : text.announcements

  return {
    activeAnnouncements,
    announcementHistory,
    closedAnnouncements,
    announcementHistoryOpen,
    visibleAnnouncementCount,
    announcementButtonLabel,
    setAnnouncementHistoryOpen,
    closeAnnouncement,
    clearAnnouncements,
  }
}
