import { describe, expect, it } from 'bun:test'

import { __testables } from './PublicHome'

describe('PublicHome guide token visibility', () => {
  const placeholder = 'th-xxxx-xxxxxxxxxxxx'

  it('keeps the guide masked by default even when a full token is available', () => {
    expect(__testables.resolvePublicGuideToken('th-a1b2-1234567890abcdef', placeholder, false)).toBe(placeholder)
  })

  it('reveals the full token only when explicitly toggled on with a valid secret', () => {
    expect(__testables.resolvePublicGuideToken('th-a1b2-1234567890abcdef', placeholder, true)).toBe(
      'th-a1b2-1234567890abcdef',
    )
  })

  it('hides the guide immediately when the revealed token no longer matches the current token', () => {
    expect(__testables.shouldRevealPublicGuideToken(
      'th-b2c3-fedcba0987654321',
      'th-a1b2-1234567890abcdef',
    )).toBe(false)
  })

  it('falls back to the placeholder when the current value is incomplete', () => {
    expect(__testables.resolvePublicGuideToken('th-a1b2-', placeholder, true)).toBe(placeholder)
    expect(__testables.resolvePublicGuideToken('', placeholder, true)).toBe(placeholder)
  })

  it('normalizes legacy single-sample guides into an array and preserves multi-sample guides', () => {
    expect(__testables.resolveGuideSamples({
      title: 'Legacy',
      steps: [],
      sampleTitle: 'Example',
      snippetLanguage: 'bash',
      snippet: 'echo ok',
      reference: { label: 'Docs', url: 'https://example.com' },
    })).toEqual([
      {
        title: 'Example',
        language: 'bash',
        snippet: 'echo ok',
        reference: { label: 'Docs', url: 'https://example.com' },
      },
    ])

    const samples = [
      { title: 'One', language: 'json', snippet: '{}' },
      { title: 'Two', language: 'bash', snippet: 'curl ...' },
    ]
    expect(__testables.resolveGuideSamples({ title: 'Modern', steps: [], samples })).toBe(samples)
  })
})
