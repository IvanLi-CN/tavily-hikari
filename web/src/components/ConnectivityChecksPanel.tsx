import type { CSSProperties, Ref } from 'react'

import { Icon } from '../lib/icons'

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
  probeBubbleShift?: number
  probeBubbleRef?: Ref<HTMLDivElement>
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
  probeBubbleShift = 0,
  probeBubbleRef,
  anyProbeRunning = false,
  onMcpClick,
  onApiClick,
}: ConnectivityChecksPanelProps): JSX.Element {
  const renderProbeBubble = (anchor: ProbeBubbleAnchor): JSX.Element | null => {
    if (!probeBubble?.visible || probeBubble.anchor !== anchor || probeBubble.items.length === 0) {
      return null
    }

    const bubbleStyle = {
      '--probe-bubble-shift': `${probeBubbleShift}px`,
    } as CSSProperties

    return (
      <div
        ref={probeBubbleRef}
        className={`user-console-probe-bubble user-console-probe-bubble-anchor-${anchor}`}
        style={bubbleStyle}
        role="status"
        aria-live="polite"
      >
        <ul className="user-console-probe-bubble-list">
          {probeBubble.items.map((item) => (
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
                <strong className="user-console-probe-bubble-item-label">{item.label}</strong>
                {item.detail ? (
                  <span className="user-console-probe-bubble-item-detail">{item.detail}</span>
                ) : null}
              </div>
            </li>
          ))}
        </ul>
      </div>
    )
  }

  return (
    <div className="user-console-probe-box">
      <div className="user-console-probe-label-row">
        <label className="token-label">{title}</label>
        <span className="user-console-probe-hint">
          <button
            type="button"
            className="user-console-probe-hint-trigger"
            aria-label={costHintAria}
          >
            <Icon icon="mdi:help-circle-outline" />
          </button>
          <span className="user-console-probe-hint-bubble" role="tooltip">
            {costHint}
          </span>
        </span>
      </div>
      <div className="user-console-probe-actions">
        <div className="user-console-probe-action">
          {renderProbeBubble('mcp')}
          <button
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
