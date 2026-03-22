import { createPortal } from 'react-dom'
import { type CSSProperties, useCallback, useMemo, useState } from 'react'

import { Icon } from '../lib/icons'
import { useAnchoredFloatingLayer } from '../lib/useAnchoredFloatingLayer'
import { Tooltip, TooltipContent, TooltipTrigger } from './ui/tooltip'

export type ProbeButtonState = 'idle' | 'running' | 'success' | 'partial' | 'failed'
export type ProbeStepStatus = 'running' | 'success' | 'failed' | 'blocked' | 'skipped'
export type ProbeBubbleAnchor = 'mcp' | 'api'

export interface ProbeButtonModel {
  state: ProbeButtonState
  completed: number
  total: number
}

export interface ProbeBubbleItem {
  id: string
  label: string
  status: ProbeStepStatus
  detail?: string | null
}

export interface ProbeBubbleModel {
  visible: boolean
  anchor: ProbeBubbleAnchor
  items: ProbeBubbleItem[]
}

const MCP_TOOL_CALL_ID_PREFIX = 'mcp-tool-call:'

interface ConnectivityChecksPanelProps {
  title: string
  costHint: string
  costHintAria: string
  stepStatusText: Record<ProbeStepStatus, string>
  mcpButtonLabel: string
  apiButtonLabel: string
  mcpProbe: ProbeButtonModel
  apiProbe: ProbeButtonModel
  probeBubble?: ProbeBubbleModel | null
  anyProbeRunning?: boolean
  onMcpClick?: () => void
  onApiClick?: () => void
}

function probeButtonTone(state: ProbeButtonState): string {
  if (state === 'success') return 'user-console-probe-btn-success'
  if (state === 'partial') return 'user-console-probe-btn-partial'
  if (state === 'failed') return 'user-console-probe-btn-failed'
  if (state === 'running') return 'user-console-probe-btn-running'
  return 'user-console-probe-btn-idle'
}

function probeButtonIcon(state: ProbeButtonState): string {
  if (state === 'success') return 'mdi:check-circle-outline'
  if (state === 'partial') return 'mdi:alert-circle-outline'
  if (state === 'failed') return 'mdi:close-circle-outline'
  if (state === 'running') return 'mdi:loading'
  return 'mdi:play-circle-outline'
}

function probeBubbleItemIcon(status: ProbeStepStatus): string {
  if (status === 'success') return 'mdi:check-circle-outline'
  if (status === 'failed') return 'mdi:close-circle-outline'
  if (status === 'blocked') return 'mdi:alert-circle-outline'
  if (status === 'skipped') return 'mdi:minus-circle-outline'
  return 'mdi:loading'
}

function resolveStructuredMcpToolCallLabel(
  item: ProbeBubbleItem,
): { prefix: string, toolName: string, suffix: string } | null {
  if (!item.id.startsWith(MCP_TOOL_CALL_ID_PREFIX)) return null

  const toolName = item.id.slice(MCP_TOOL_CALL_ID_PREFIX.length).trim()
  if (toolName.length === 0) return null

  const toolIndex = item.label.indexOf(toolName)
  if (toolIndex === -1) return null

  return {
    prefix: item.label.slice(0, toolIndex).trimEnd(),
    toolName,
    suffix: item.label.slice(toolIndex + toolName.length).trimStart(),
  }
}

export default function ConnectivityChecksPanel({
  title,
  costHint,
  costHintAria,
  stepStatusText,
  mcpButtonLabel,
  apiButtonLabel,
  mcpProbe,
  apiProbe,
  probeBubble,
  anyProbeRunning = false,
  onMcpClick,
  onApiClick,
}: ConnectivityChecksPanelProps): JSX.Element {
  const [mcpButtonEl, setMcpButtonEl] = useState<HTMLButtonElement | null>(null)
  const [apiButtonEl, setApiButtonEl] = useState<HTMLButtonElement | null>(null)
  const handleMcpButtonRef = useCallback((node: HTMLButtonElement | null) => {
    setMcpButtonEl(node)
  }, [])
  const handleApiButtonRef = useCallback((node: HTMLButtonElement | null) => {
    setApiButtonEl(node)
  }, [])
  const activeProbeAnchor = probeBubble?.anchor === 'mcp' ? mcpButtonEl : apiButtonEl
  const isProbeBubbleOpen = Boolean(probeBubble?.visible && probeBubble.items.length > 0 && activeProbeAnchor)
  const probeBubbleAlign = probeBubble?.anchor === 'mcp' ? 'start' : 'end'
  const { layerRef: probeBubbleLayerRef, position: probeBubblePosition } = useAnchoredFloatingLayer<HTMLDivElement>({
    open: isProbeBubbleOpen,
    anchorEl: activeProbeAnchor,
    placement: 'top',
    align: probeBubbleAlign,
    offset: 10,
    viewportMargin: 12,
    arrowPadding: 24,
  })

  const probeBubbleNode = useMemo(() => {
    if (!probeBubble?.visible || probeBubble.items.length === 0) {
      return null
    }

    const bubbleStyle = {
      top: probeBubblePosition ? `${probeBubblePosition.top}px` : undefined,
      left: probeBubblePosition ? `${probeBubblePosition.left}px` : undefined,
      visibility: typeof document === 'undefined' || probeBubblePosition ? 'visible' : 'hidden',
      pointerEvents: typeof document === 'undefined' || probeBubblePosition ? 'auto' : 'none',
      ['--probe-bubble-arrow-offset' as string]: `${probeBubblePosition?.arrowOffset ?? 24}px`,
    } as CSSProperties

    return (
      <div
        ref={probeBubbleLayerRef}
        className="user-console-probe-bubble layer-popover"
        data-placement={probeBubblePosition?.placement ?? 'top'}
        role="status"
        aria-live="polite"
        style={bubbleStyle}
      >
        <ul className="user-console-probe-bubble-list">
          {probeBubble.items.map((item) => (
            (() => {
              const structuredLabel = resolveStructuredMcpToolCallLabel(item)
              return (
                <li
                  key={item.id}
                  className="user-console-probe-bubble-item"
                  aria-label={`${stepStatusText[item.status]} · ${item.label}${item.detail ? ` · ${item.detail}` : ''}`}
                >
                  <Icon
                    icon={probeBubbleItemIcon(item.status)}
                    className={
                      `user-console-probe-bubble-item-icon user-console-probe-bubble-item-icon-status-${item.status} `
                      + `${item.status === 'running' ? 'is-spinning' : ''}`
                    }
                  />
                  <div className="user-console-probe-bubble-item-copy">
                    {structuredLabel ? (
                      <strong className="user-console-probe-bubble-item-label user-console-probe-bubble-item-label-structured">
                        {structuredLabel.prefix ? (
                          <span className="user-console-probe-bubble-item-label-text">
                            {structuredLabel.prefix}
                          </span>
                        ) : null}
                        <code className="user-console-probe-bubble-item-tool">{structuredLabel.toolName}</code>
                        {structuredLabel.suffix ? (
                          <span className="user-console-probe-bubble-item-label-text">
                            {structuredLabel.suffix}
                          </span>
                        ) : null}
                      </strong>
                    ) : (
                      <strong className="user-console-probe-bubble-item-label">{item.label}</strong>
                    )}
                    {item.detail ? (
                      <span className="user-console-probe-bubble-item-detail">{item.detail}</span>
                    ) : null}
                  </div>
                </li>
              )
            })()
          ))}
        </ul>
      </div>
    )
  }, [probeBubble, probeBubbleLayerRef, probeBubblePosition, stepStatusText])

  const renderProbeBubble = (anchor: ProbeBubbleAnchor): JSX.Element | null => {
    if (!probeBubble?.visible || probeBubble.anchor !== anchor || !probeBubbleNode) {
      return null
    }

    if (typeof document === 'undefined') {
      return probeBubbleNode
    }

    return createPortal(probeBubbleNode, document.body)
  }

  return (
    <div className="user-console-probe-box">
      <div className="user-console-probe-label-row">
        <label className="token-label">{title}</label>
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              className="user-console-probe-hint-trigger"
              aria-label={costHintAria}
            >
              <Icon icon="mdi:help-circle-outline" />
            </button>
          </TooltipTrigger>
          <TooltipContent className="max-w-[min(20rem,calc(100vw-2rem))]" side="top">
            {costHint}
          </TooltipContent>
        </Tooltip>
      </div>
      <div className="user-console-probe-actions">
        <div className="user-console-probe-action">
          {renderProbeBubble('mcp')}
          <button
            ref={handleMcpButtonRef}
            type="button"
            data-probe-kind="mcp"
            className={`btn btn-sm user-console-probe-btn ${probeButtonTone(mcpProbe.state)}`}
            onClick={onMcpClick}
            disabled={anyProbeRunning}
          >
            <Icon
              icon={probeButtonIcon(mcpProbe.state)}
              className={`user-console-probe-btn-icon ${mcpProbe.state === 'running' ? 'is-spinning' : ''}`}
            />
            <span>{mcpButtonLabel}</span>
          </button>
        </div>
        <div className="user-console-probe-action">
          {renderProbeBubble('api')}
          <button
            ref={handleApiButtonRef}
            type="button"
            data-probe-kind="api"
            className={`btn btn-sm user-console-probe-btn ${probeButtonTone(apiProbe.state)}`}
            onClick={onApiClick}
            disabled={anyProbeRunning}
          >
            <Icon
              icon={probeButtonIcon(apiProbe.state)}
              className={`user-console-probe-btn-icon ${apiProbe.state === 'running' ? 'is-spinning' : ''}`}
            />
            <span>{apiButtonLabel}</span>
          </button>
        </div>
      </div>
    </div>
  )
}
