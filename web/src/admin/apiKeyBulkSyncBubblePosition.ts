export interface ApiKeyBulkSyncBubblePinnedPosition {
  top: number
  left: number
}

interface PinnedPositionOptions {
  viewportPadding?: number
  verticalOffset?: number
  maxBubbleWidth?: number
}

interface BubbleAnchorRectLike {
  bottom: number
  left: number
  width: number
}

export function createApiKeyBulkSyncBubblePinnedPosition(
  rect: BubbleAnchorRectLike,
  viewportWidth: number,
  options?: PinnedPositionOptions,
): ApiKeyBulkSyncBubblePinnedPosition {
  const viewportPadding = options?.viewportPadding ?? 12
  const verticalOffset = options?.verticalOffset ?? 10
  const maxBubbleWidth = options?.maxBubbleWidth ?? 416
  const safeViewportWidth = Math.max(viewportPadding * 2, viewportWidth)
  const bubbleWidth = Math.min(maxBubbleWidth, safeViewportWidth - viewportPadding * 2)
  const bubbleHalfWidth = bubbleWidth / 2
  const anchorCenter = rect.left + rect.width / 2
  const minCenter = viewportPadding + bubbleHalfWidth
  const maxCenter = Math.max(minCenter, safeViewportWidth - viewportPadding - bubbleHalfWidth)

  return {
    top: Math.max(viewportPadding, rect.bottom + verticalOffset),
    left: Math.min(maxCenter, Math.max(minCenter, anchorCenter)),
  }
}
