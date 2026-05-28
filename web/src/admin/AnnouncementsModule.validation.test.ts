import { describe, expect, it } from 'bun:test'

import { estimateAnnouncementBodyRows, isAnnouncementBodyRequired } from './AnnouncementsModule'

describe('announcement editor validation helpers', () => {
  it('keeps modal bodies required while allowing bodyless ticker announcements', () => {
    expect(isAnnouncementBodyRequired('modal')).toBe(true)
    expect(isAnnouncementBodyRequired('ticker')).toBe(false)
  })

  it('estimates editor rows from body length within useful bounds', () => {
    expect(estimateAnnouncementBodyRows('Short notice')).toBe(6)
    expect(estimateAnnouncementBodyRows(Array.from({ length: 20 }, (_, index) => `Line ${index}`).join('\n'))).toBe(18)
    expect(estimateAnnouncementBodyRows('x'.repeat(360))).toBeGreaterThan(6)
  })
})
