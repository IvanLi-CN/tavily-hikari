import { describe, expect, it } from 'bun:test'

import { createApiKeyBulkSyncBubblePinnedPosition } from './apiKeyBulkSyncBubblePosition'

describe('createApiKeyBulkSyncBubblePinnedPosition', () => {
  it('keeps the pinned bubble inside the viewport near the left edge', () => {
    const position = createApiKeyBulkSyncBubblePinnedPosition(
      { bottom: 48, left: 8, width: 40 },
      320,
    )

    expect(position.top).toBe(58)
    expect(position.left).toBe(160)
  })

  it('keeps the pinned bubble inside the viewport near the right edge', () => {
    const position = createApiKeyBulkSyncBubblePinnedPosition(
      { bottom: 48, left: 280, width: 40 },
      320,
    )

    expect(position.left).toBe(160)
  })

  it('preserves the anchor center when the viewport is wide enough', () => {
    const position = createApiKeyBulkSyncBubblePinnedPosition(
      { bottom: 64, left: 500, width: 60 },
      1440,
    )

    expect(position.top).toBe(74)
    expect(position.left).toBe(530)
  })
})
