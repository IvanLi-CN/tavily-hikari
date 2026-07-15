import { describe, expect, it } from 'bun:test'

import { markdownToPlainText, parseAnnouncementContent } from './announcementContent'

describe('parseAnnouncementContent', () => {
  it('extracts an ATX title and removes it from the body', () => {
    const parsed = parseAnnouncementContent('# [Launch](https://example.com)\n\nBody copy')

    expect(parsed.hasTitle).toBe(true)
    expect(parsed.titleMarkdown).toBe('[Launch](https://example.com)')
    expect(parsed.titleText).toBe('Launch')
    expect(parsed.bodyMarkdown).toBe('Body copy')
    expect(parsed.summary).toBe('Launch')
  })

  it('extracts a Setext title and keeps the remaining body markdown', () => {
    const parsed = parseAnnouncementContent('Release window\n---\n\n- one\n- two')

    expect(parsed.hasTitle).toBe(true)
    expect(parsed.titleMarkdown).toBe('Release window')
    expect(parsed.bodyMarkdown).toBe('- one\n- two')
  })

  it('treats non-heading content as body-only content', () => {
    const parsed = parseAnnouncementContent('Read the [guide](https://example.com) before continuing.')

    expect(parsed.hasTitle).toBe(false)
    expect(parsed.titleMarkdown).toBeNull()
    expect(parsed.bodyMarkdown).toBe('Read the [guide](https://example.com) before continuing.')
    expect(parsed.summary).toBe('Read the guide before continuing.')
  })
})

describe('markdownToPlainText', () => {
  it('strips basic markdown syntax for labels and summaries', () => {
    expect(markdownToPlainText('**Bold** `code` [link](https://example.com)')).toBe('Bold code link')
  })
})
