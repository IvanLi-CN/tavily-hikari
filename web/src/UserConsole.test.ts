import { describe, expect, it } from 'bun:test'

import { __testables } from './UserConsole'

describe('UserConsole landing guide helpers', () => {
  it('shows the landing guide only when exactly one token is visible on the merged landing page', () => {
    expect(__testables.shouldRenderLandingGuide({ name: 'landing', section: 'dashboard' }, 1)).toBe(true)
    expect(__testables.shouldRenderLandingGuide({ name: 'landing', section: 'tokens' }, 1)).toBe(true)
    expect(__testables.shouldRenderLandingGuide({ name: 'landing', section: 'tokens' }, 0)).toBe(false)
    expect(__testables.shouldRenderLandingGuide({ name: 'landing', section: 'tokens' }, 2)).toBe(false)
    expect(__testables.shouldRenderLandingGuide({ name: 'token', id: 'a1b2' }, 1)).toBe(false)
  })

  it('prefers the detail token id and otherwise falls back to the single landing token mask', () => {
    expect(__testables.resolveGuideToken({ name: 'token', id: 'a1b2' }, [])).toBe(
      'th-a1b2-************************',
    )
    expect(__testables.resolveGuideToken(
      { name: 'landing', section: 'tokens' },
      [{ tokenId: 'c3d4' } as any],
    )).toBe('th-c3d4-************************')
    expect(__testables.resolveGuideToken(
      { name: 'landing', section: 'dashboard' },
      [{ tokenId: 'a1b2' } as any, { tokenId: 'c3d4' } as any],
    )).toBe('th-xxxx-xxxxxxxxxxxx')
  })
})
