import { requestJson } from './runtime'

export type AnnouncementDisplayKind = 'modal' | 'ticker'
export type AnnouncementStatus = 'draft' | 'published' | 'archived'

export interface Announcement {
  id: string
  content: string
  displayKind: AnnouncementDisplayKind
  status: AnnouncementStatus
  createdAt: number
  updatedAt: number
  publishedAt: number | null
  archivedAt: number | null
}

export interface AnnouncementsResponse {
  items: Announcement[]
}

export interface AnnouncementMutationPayload {
  content: string
  displayKind: AnnouncementDisplayKind
}

export function fetchAnnouncements(signal?: AbortSignal): Promise<AnnouncementsResponse> {
  return requestJson('/api/announcements', { signal })
}

export function createAnnouncement(payload: AnnouncementMutationPayload): Promise<Announcement> {
  return requestJson('/api/announcements', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  })
}

export function updateAnnouncement(id: string, payload: AnnouncementMutationPayload): Promise<Announcement> {
  const encoded = encodeURIComponent(id)
  return requestJson(`/api/announcements/${encoded}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  })
}

export function publishAnnouncement(id: string): Promise<Announcement> {
  const encoded = encodeURIComponent(id)
  return requestJson(`/api/announcements/${encoded}/publish`, { method: 'POST' })
}

export function archiveAnnouncement(id: string): Promise<Announcement> {
  const encoded = encodeURIComponent(id)
  return requestJson(`/api/announcements/${encoded}/archive`, { method: 'POST' })
}

export function fetchUserAnnouncements(signal?: AbortSignal): Promise<AnnouncementsResponse> {
  return requestJson('/api/user/announcements', { signal })
}

export function fetchUserAnnouncementHistory(signal?: AbortSignal): Promise<AnnouncementsResponse> {
  return requestJson('/api/user/announcements/history', { signal })
}
