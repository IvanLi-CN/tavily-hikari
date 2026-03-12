import type { MouseEvent } from 'react'

import { keyDetailPath } from '../admin/routes'

interface JobKeyLinkProps {
  keyId: string | null
  keyGroup?: string | null
  ungroupedLabel: string
  detailLabel: string
  onOpenKey?: (id: string) => void
  showBubble?: boolean
}

function isPlainLeftClick(event: MouseEvent<HTMLAnchorElement>): boolean {
  return event.button === 0 && !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey
}

export default function JobKeyLink({
  keyId,
  keyGroup,
  ungroupedLabel,
  detailLabel,
  onOpenKey,
  showBubble = true,
}: JobKeyLinkProps): JSX.Element {
  if (!keyId) {
    return <>—</>
  }

  const groupLabel = keyGroup?.trim() ? keyGroup.trim() : ungroupedLabel
  const link = (
    <a
      href={keyDetailPath(keyId)}
      className="jobs-key-link"
      title={detailLabel}
      aria-label={`${detailLabel}: ${keyId}`}
      onClick={(event) => {
        if (!onOpenKey) return
        if (event.defaultPrevented || !isPlainLeftClick(event)) return
        event.preventDefault()
        onOpenKey(keyId)
      }}
    >
      <code>{keyId}</code>
    </a>
  )

  if (!showBubble) {
    return link
  }

  return (
    <span className="tooltip jobs-key-tooltip" data-tip={groupLabel}>
      {link}
    </span>
  )
}
