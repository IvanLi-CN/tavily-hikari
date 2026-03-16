import type { ForwardProxyProgressEvent, ForwardProxyProgressPhaseKey } from '../api'

export type ForwardProxyDialogKind = 'subscription' | 'manual' | null
export type ForwardProxyDialogAction = 'validate' | 'save' | 'revalidate'
export type ForwardProxyDialogProgressStatus = 'pending' | 'running' | 'done' | 'error'

export interface ForwardProxyProgressStrings {
  running: string
  waiting: string
  done: string
  failed: string
  stepCounter: string
  steps: Record<ForwardProxyProgressPhaseKey, string>
}

export interface ForwardProxyDialogProgressStep {
  key: ForwardProxyProgressPhaseKey
  label: string
  status: ForwardProxyDialogProgressStatus
  detail: string | null
}

export interface ForwardProxyDialogProgressState {
  action: ForwardProxyDialogAction
  steps: ForwardProxyDialogProgressStep[]
  activeStepKey: ForwardProxyProgressPhaseKey | null
  message: string | null
}

function buildProgressDetail(
  strings: ForwardProxyProgressStrings,
  current?: number | null,
  total?: number | null,
  detail?: string | null,
): string | null {
  const parts: string[] = []
  if (typeof current === 'number' && typeof total === 'number' && total > 0) {
    parts.push(strings.stepCounter.replace('{current}', String(current)).replace('{total}', String(total)))
  }
  if (detail) {
    parts.push(detail)
  }
  return parts.length > 0 ? parts.join(' · ') : null
}

function buildProgressSteps(
  strings: ForwardProxyProgressStrings,
  kind: Exclude<ForwardProxyDialogKind, null>,
  action: ForwardProxyDialogAction,
): ForwardProxyDialogProgressStep[] {
  const stepKeys: ForwardProxyProgressPhaseKey[] =
    action === 'revalidate'
      ? ['refresh_subscription', 'probe_nodes', 'refresh_ui']
      : action === 'validate'
      ? kind === 'subscription'
        ? ['normalize_input', 'fetch_subscription', 'probe_nodes', 'generate_result']
        : ['parse_input', 'probe_nodes', 'generate_result']
      : kind === 'subscription'
        ? ['save_settings', 'refresh_subscription', 'bootstrap_probe', 'refresh_ui']
        : ['save_settings', 'bootstrap_probe', 'refresh_ui']

  return stepKeys.map((key) => ({
    key,
    label: strings.steps[key],
    status: 'pending',
    detail: null,
  }))
}

export function createDialogProgressState(
  strings: ForwardProxyProgressStrings,
  kind: Exclude<ForwardProxyDialogKind, null>,
  action: ForwardProxyDialogAction,
): ForwardProxyDialogProgressState {
  return {
    action,
    steps: buildProgressSteps(strings, kind, action),
    activeStepKey: null,
    message: null,
  }
}

export function updateDialogProgressState(
  current: ForwardProxyDialogProgressState,
  strings: ForwardProxyProgressStrings,
  event: ForwardProxyProgressEvent,
): ForwardProxyDialogProgressState {
  if (event.type === 'nodes' || event.type === 'node') {
    return current
  }

  if (event.type === 'phase') {
    const detail = buildProgressDetail(strings, event.current, event.total, event.detail)
    return {
      ...current,
      activeStepKey: event.phaseKey,
      steps: current.steps.map((step) => {
        if (step.key === event.phaseKey) {
          return {
            ...step,
            status: 'running',
            detail,
          }
        }
        if (step.status === 'running') {
          return {
            ...step,
            status: 'done',
          }
        }
        return step
      }),
      message: detail,
    }
  }

  if (event.type === 'error') {
    const failingStep = event.phaseKey ?? current.activeStepKey
    const detail = buildProgressDetail(strings, event.current, event.total, event.detail) ?? event.message
    return {
      ...current,
      steps: current.steps.map((step) => {
        if (step.key === failingStep) {
          return {
            ...step,
            status: 'error',
            detail,
          }
        }
        if (step.status === 'running') {
          return {
            ...step,
            status: 'done',
          }
        }
        return step
      }),
      message: event.message,
    }
  }

  if (current.steps.some((step) => step.status === 'error')) {
    return {
      ...current,
      message: current.message ?? strings.failed,
    }
  }

  return {
    ...current,
    steps: current.steps.map((step) =>
      step.status === 'running' || current.activeStepKey == null
        ? {
            ...step,
            status: 'done',
          }
        : step,
    ),
    message: strings.done,
  }
}
