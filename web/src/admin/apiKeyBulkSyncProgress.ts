import type {
  ApiKeyBulkActionResponse,
  ApiKeyBulkActionResult,
  ApiKeyBulkActionSummary,
  ApiKeyBulkSyncProgressEvent,
} from '../api'

export type ApiKeyBulkSyncPhaseKey = 'prepare_request' | 'sync_usage' | 'refresh_ui'
export type ApiKeyBulkSyncStepStatus = 'pending' | 'running' | 'done' | 'error'

export interface ApiKeyBulkSyncStepState {
  key: ApiKeyBulkSyncPhaseKey
  status: ApiKeyBulkSyncStepStatus
  detail: string | null
}

export interface ApiKeyBulkSyncLastResult {
  keyId: string
  status: ApiKeyBulkActionResult['status']
  detail: string | null
}

export interface ApiKeyBulkSyncProgressState {
  steps: ApiKeyBulkSyncStepState[]
  summary: ApiKeyBulkActionSummary
  current: number
  total: number
  lastResult: ApiKeyBulkSyncLastResult | null
  message: string | null
  response: ApiKeyBulkActionResponse | null
  completed: boolean
  error: string | null
}

function lastResultFromResponse(
  response: ApiKeyBulkActionResponse,
): ApiKeyBulkSyncLastResult | null {
  const item = response.results.at(-1)
  if (!item) return null
  return {
    keyId: item.key_id,
    status: item.status,
    detail: item.detail ?? null,
  }
}

function cloneSummary(summary: ApiKeyBulkActionSummary): ApiKeyBulkActionSummary {
  return {
    requested: summary.requested,
    succeeded: summary.succeeded,
    skipped: summary.skipped,
    failed: summary.failed,
  }
}

function getStepDetail(
  steps: ApiKeyBulkSyncStepState[],
  key: ApiKeyBulkSyncPhaseKey,
): string | null {
  return steps.find((step) => step.key === key)?.detail ?? null
}

function updateStep(
  steps: ApiKeyBulkSyncStepState[],
  key: ApiKeyBulkSyncPhaseKey,
  status: ApiKeyBulkSyncStepStatus,
  detail: string | null,
): ApiKeyBulkSyncStepState[] {
  return steps.map((step) => {
    if (step.key === key) {
      return { ...step, status, detail }
    }
    return step
  })
}

function baseSteps(): ApiKeyBulkSyncStepState[] {
  return [
    { key: 'prepare_request', status: 'pending', detail: null },
    { key: 'sync_usage', status: 'pending', detail: null },
    { key: 'refresh_ui', status: 'pending', detail: null },
  ]
}

function markPrepareDone(steps: ApiKeyBulkSyncStepState[]): ApiKeyBulkSyncStepState[] {
  return updateStep(steps, 'prepare_request', 'done', getStepDetail(steps, 'prepare_request'))
}

function markSyncDone(steps: ApiKeyBulkSyncStepState[]): ApiKeyBulkSyncStepState[] {
  return updateStep(steps, 'sync_usage', 'done', getStepDetail(steps, 'sync_usage'))
}

export function createApiKeyBulkSyncProgressState(total: number): ApiKeyBulkSyncProgressState {
  return {
    steps: baseSteps(),
    summary: {
      requested: total,
      succeeded: 0,
      skipped: 0,
      failed: 0,
    },
    current: 0,
    total,
    lastResult: null,
    message: null,
    response: null,
    completed: false,
    error: null,
  }
}

export function updateApiKeyBulkSyncProgressState(
  current: ApiKeyBulkSyncProgressState,
  event: ApiKeyBulkSyncProgressEvent,
): ApiKeyBulkSyncProgressState {
  if (event.type === 'phase') {
    if (event.phaseKey === 'prepare_request') {
      return {
        ...current,
        steps: updateStep(current.steps, 'prepare_request', 'running', event.detail ?? null),
        total: event.total ?? current.total,
        summary: {
          ...current.summary,
          requested: event.total ?? current.summary.requested,
        },
        message: event.detail ?? event.label,
        error: null,
      }
    }

    if (event.phaseKey === 'sync_usage') {
      return {
        ...current,
        steps: updateStep(
          markPrepareDone(current.steps),
          'sync_usage',
          'running',
          event.detail ?? null,
        ),
        current: event.current ?? current.current,
        total: event.total ?? current.total,
        message: event.detail ?? event.label,
        error: null,
      }
    }

    return {
      ...current,
      steps: updateStep(
        markSyncDone(markPrepareDone(current.steps)),
        'refresh_ui',
        'running',
        event.detail ?? null,
      ),
      message: event.detail ?? event.label,
      error: null,
    }
  }

  if (event.type === 'item') {
    return {
      ...current,
      steps: updateStep(markPrepareDone(current.steps), 'sync_usage', 'running', event.detail ?? null),
      summary: cloneSummary(event.summary),
      current: event.current,
      total: event.total,
      lastResult: {
        keyId: event.keyId,
        status: event.status,
        detail: event.detail ?? null,
      },
      message: event.detail ?? null,
      error: null,
    }
  }

  if (event.type === 'complete') {
    const fallbackCurrent = Math.max(
      current.current,
      event.payload.summary.requested,
      event.payload.results.length,
    )
    const fallbackTotal = Math.max(
      current.total,
      event.payload.summary.requested,
      event.payload.results.length,
    )
    return {
      ...current,
      steps: markSyncDone(markPrepareDone(current.steps)),
      summary: cloneSummary(event.payload.summary),
      current: fallbackCurrent,
      total: fallbackTotal,
      lastResult: current.lastResult ?? lastResultFromResponse(event.payload),
      response: event.payload,
      completed: false,
      error: null,
    }
  }

  return {
    ...current,
    steps: updateStep(
      current.steps,
      event.phaseKey === 'refresh_ui'
        ? 'refresh_ui'
        : event.phaseKey === 'prepare_request'
          ? 'prepare_request'
          : 'sync_usage',
      'error',
      event.detail ?? event.message,
    ),
    message: event.detail ?? event.message,
    error: event.message,
    completed: true,
  }
}

export function markApiKeyBulkSyncRefreshRunning(
  current: ApiKeyBulkSyncProgressState,
  detail: string | null,
): ApiKeyBulkSyncProgressState {
  return {
    ...current,
    steps: updateStep(markSyncDone(markPrepareDone(current.steps)), 'refresh_ui', 'running', detail),
  }
}

export function markApiKeyBulkSyncRefreshDone(
  current: ApiKeyBulkSyncProgressState,
  response?: ApiKeyBulkActionResponse | null,
): ApiKeyBulkSyncProgressState {
  return {
    ...current,
    steps: updateStep(
      markSyncDone(markPrepareDone(current.steps)),
      'refresh_ui',
      'done',
      getStepDetail(current.steps, 'refresh_ui'),
    ),
    response: response ?? current.response,
    completed: true,
    error: null,
  }
}

export function markApiKeyBulkSyncRefreshError(
  current: ApiKeyBulkSyncProgressState,
  message: string,
): ApiKeyBulkSyncProgressState {
  return {
    ...current,
    steps: updateStep(current.steps, 'refresh_ui', 'error', message),
    completed: true,
    error: message,
  }
}
