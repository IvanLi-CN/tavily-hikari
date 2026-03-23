import type { HTMLAttributes } from 'react'

import { cn } from '../lib/utils'

import { Badge } from './ui/badge'

export type RequestKindBadgeTone =
  | 'neutral'
  | 'api'
  | 'api-search'
  | 'api-extract'
  | 'api-crawl'
  | 'api-map'
  | 'api-research'
  | 'api-research-result'
  | 'api-usage'
  | 'api-raw'
  | 'mcp'
  | 'mcp-extract'
  | 'mcp-search'
  | 'mcp-batch'
  | 'mcp-tool'
  | 'mcp-initialize'
  | 'mcp-notification'
  | 'mcp-ping'
  | 'mcp-resource'
  | 'mcp-resource-template'
  | 'mcp-tools-list'
  | 'mcp-system'
  | 'mcp-raw'

export interface RequestKindBadgeProps extends HTMLAttributes<HTMLSpanElement> {
  requestKindLabel: string
  requestKindKey?: string | null
  size?: 'sm' | 'md'
}

function splitRequestKindLabel(requestKindLabel: string): { source: string | null; detail: string | null } {
  const parts = requestKindLabel
    .split('|')
    .map((part) => part.trim())
    .filter(Boolean)

  if (parts.length >= 2) {
    return {
      source: parts[0],
      detail: parts.slice(1).join(' | '),
    }
  }

  return {
    source: null,
    detail: requestKindLabel.trim() || null,
  }
}

export function resolveRequestKindBadgeTone(
  requestKindKey?: string | null,
  requestKindLabel?: string | null,
): RequestKindBadgeTone {
  const key = requestKindKey?.trim().toLowerCase() ?? ''
  const label = requestKindLabel?.trim().toLowerCase() ?? ''

  if (key.startsWith('api:search') || label === 'api | search') return 'api-search'
  if (key.startsWith('api:extract') || label === 'api | extract') return 'api-extract'
  if (key.startsWith('api:crawl') || label === 'api | crawl') return 'api-crawl'
  if (key.startsWith('api:map') || label === 'api | map') return 'api-map'
  if (key.startsWith('api:research-result') || label === 'api | research result') return 'api-research-result'
  if (key.startsWith('api:research') || label === 'api | research') return 'api-research'
  if (key.startsWith('api:usage') || label === 'api | usage') return 'api-usage'
  if (key.startsWith('api:unknown-path') || label === 'api | unknown path' || key.startsWith('api:raw:')) {
    return 'api-raw'
  }

  if (key.startsWith('mcp:crawl') || label === 'mcp | crawl') return 'mcp'
  if (key.startsWith('mcp:extract') || label === 'mcp | extract') return 'mcp-extract'
  if (key.startsWith('mcp:map') || label === 'mcp | map') return 'mcp'
  if (key.startsWith('mcp:research') || label === 'mcp | research') return 'mcp'
  if (key.startsWith('mcp:search') || label === 'mcp | search') return 'mcp-search'
  if (key.startsWith('mcp:batch') || label === 'mcp | batch') return 'mcp-batch'
  if (key.startsWith('mcp:third-party-tool') || key.startsWith('mcp:tool:') || label === 'mcp | third-party tool') {
    return 'mcp-tool'
  }
  if (key.startsWith('mcp:initialize') || label === 'mcp | initialize') return 'mcp-initialize'
  if (key.startsWith('mcp:notifications/') || label.startsWith('mcp | notifications/')) return 'mcp-notification'
  if (key.startsWith('mcp:ping') || label === 'mcp | ping') return 'mcp-ping'
  if (key.startsWith('mcp:resources/templates/') || label.startsWith('mcp | resources/templates/')) {
    return 'mcp-resource-template'
  }
  if (key.startsWith('mcp:resources/') || label.startsWith('mcp | resources/')) return 'mcp-resource'
  if (key.startsWith('mcp:tools/list') || label === 'mcp | tools/list') return 'mcp-tools-list'
  if (
    key.startsWith('mcp:prompts/')
  ) {
    return 'mcp-system'
  }
  if (
    key.startsWith('mcp:unsupported-path') ||
    key.startsWith('mcp:unknown-payload') ||
    key.startsWith('mcp:unknown-method') ||
    key.startsWith('mcp:raw:') ||
    label === 'mcp | unsupported path' ||
    label === 'mcp | unknown payload' ||
    label === 'mcp | unknown method'
  ) {
    return 'mcp-raw'
  }

  if (key.startsWith('api:') || label.startsWith('api |')) return 'api'
  if (key.startsWith('mcp:') || label.startsWith('mcp |')) return 'mcp'
  return 'neutral'
}

export function RequestKindBadge({
  requestKindKey,
  requestKindLabel,
  size = 'md',
  className,
  title,
  ...props
}: RequestKindBadgeProps): JSX.Element {
  const safeLabel = requestKindLabel.trim() || requestKindKey?.trim() || '—'
  const tone = resolveRequestKindBadgeTone(requestKindKey, safeLabel)
  const { source, detail } = splitRequestKindLabel(safeLabel)

  return (
    <Badge
      variant="neutral"
      className={cn(
        'request-kind-badge',
        `request-kind-badge--${tone}`,
        size === 'sm' ? 'request-kind-badge--sm' : 'request-kind-badge--md',
        className,
      )}
      title={title ?? safeLabel}
      {...props}
    >
      {source && detail ? (
        <>
          <span className="request-kind-badge__source">{source}</span>
          <span className="request-kind-badge__separator" aria-hidden="true">
            |
          </span>
          <span className="request-kind-badge__detail">{detail}</span>
        </>
      ) : (
        <span className="request-kind-badge__detail">{safeLabel}</span>
      )}
    </Badge>
  )
}

export default RequestKindBadge
