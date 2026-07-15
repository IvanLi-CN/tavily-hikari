import { describe, expect, it } from 'bun:test'

import {
  estimateAnnouncementContentRows,
  validateAnnouncementContentInput,
} from './AnnouncementsModule'

describe('announcement editor validation helpers', () => {
  it('requires content, modal titles, and modal bodies while allowing ticker title-only content', () => {
    expect(validateAnnouncementContentInput('', 'modal')).toBe('content')
    expect(validateAnnouncementContentInput('Just content', 'modal')).toBe('modal_title')
    expect(validateAnnouncementContentInput('# Title only', 'modal')).toBe('modal_body')
    expect(validateAnnouncementContentInput('# Title only', 'ticker')).toBeNull()
    expect(validateAnnouncementContentInput('Just content', 'ticker')).toBeNull()
  })

  it('estimates editor rows from content length within useful bounds', () => {
    expect(estimateAnnouncementContentRows('Short notice')).toBe(6)
    expect(estimateAnnouncementContentRows(Array.from({ length: 20 }, (_, index) => `Line ${index}`).join('\n'))).toBe(18)
    expect(estimateAnnouncementContentRows('x'.repeat(360))).toBeGreaterThan(6)
  })
})
