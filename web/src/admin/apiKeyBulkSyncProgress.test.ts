import { describe, expect, it } from 'bun:test'

import type { ApiKeyBulkActionResponse } from '../api'
import {
  createApiKeyBulkSyncProgressState,
  markApiKeyBulkSyncRefreshDone,
  markApiKeyBulkSyncRefreshError,
  markApiKeyBulkSyncRefreshRunning,
  updateApiKeyBulkSyncProgressState,
} from './apiKeyBulkSyncProgress'

describe('apiKeyBulkSyncProgress', () => {
  it('tracks prepare, per-key results, and refresh completion', () => {
    let state = createApiKeyBulkSyncProgressState(3)

    state = updateApiKeyBulkSyncProgressState(state, {
      type: 'phase',
      phaseKey: 'prepare_request',
      label: 'Preparing request',
      total: 3,
      detail: 'Queued 3 key(s) for manual quota sync',
    })
    expect(state.steps[0]).toMatchObject({ status: 'running' })

    state = updateApiKeyBulkSyncProgressState(state, {
      type: 'phase',
      phaseKey: 'sync_usage',
      label: 'Syncing selected keys',
      current: 0,
      total: 3,
      detail: 'Waiting for each manual quota sync result as keys finish',
    })
    expect(state.steps[0]).toMatchObject({ status: 'done' })
    expect(state.steps[1]).toMatchObject({ status: 'running' })
    expect(state.current).toBe(0)
    expect(state.total).toBe(3)

    state = updateApiKeyBulkSyncProgressState(state, {
      type: 'item',
      keyId: 'key-a',
      status: 'success',
      current: 1,
      total: 3,
      summary: { requested: 3, succeeded: 1, skipped: 0, failed: 0 },
      detail: 'limit=1000 remaining=900',
    })
    expect(state.steps[0]).toMatchObject({ status: 'done' })
    expect(state.steps[1]).toMatchObject({ status: 'running' })
    expect(state.lastResult).toMatchObject({ keyId: 'key-a', status: 'success' })

    state = updateApiKeyBulkSyncProgressState(state, {
      type: 'phase',
      phaseKey: 'refresh_ui',
      label: 'Refreshing list',
      current: 3,
      total: 3,
      detail: 'Server-side sync finished; refresh the admin keys list now',
    })
    expect(state.steps[1]).toMatchObject({ status: 'done' })
    expect(state.steps[2]).toMatchObject({ status: 'running' })

    const payload: ApiKeyBulkActionResponse = {
      summary: { requested: 3, succeeded: 2, skipped: 0, failed: 1 },
      results: [
        { key_id: 'key-a', status: 'success', detail: null },
        { key_id: 'key-b', status: 'success', detail: null },
        { key_id: 'key-c', status: 'failed', detail: 'unauthorized' },
      ],
    }
    state = updateApiKeyBulkSyncProgressState(state, { type: 'complete', payload })
    state = markApiKeyBulkSyncRefreshRunning(state, 'Refreshing the current keys list…')
    state = markApiKeyBulkSyncRefreshDone(state, payload)

    expect(state.completed).toBe(true)
    expect(state.response).toEqual(payload)
    expect(state.steps.map((step) => step.status)).toEqual(['done', 'done', 'done'])
  })

  it('marks refresh failures after the stream completed', () => {
    let state = createApiKeyBulkSyncProgressState(1)
    state = updateApiKeyBulkSyncProgressState(state, {
      type: 'complete',
      payload: {
        summary: { requested: 1, succeeded: 1, skipped: 0, failed: 0 },
        results: [{ key_id: 'key-a', status: 'success', detail: null }],
      },
    })

    state = markApiKeyBulkSyncRefreshError(state, 'Failed to refresh current list')
    expect(state.completed).toBe(true)
    expect(state.error).toBe('Failed to refresh current list')
    expect(state.steps[2]).toMatchObject({ status: 'error' })
  })

  it('hydrates final counters and latest result from a JSON fallback completion', () => {
    let state = createApiKeyBulkSyncProgressState(2)
    state = updateApiKeyBulkSyncProgressState(state, {
      type: 'complete',
      payload: {
        summary: { requested: 2, succeeded: 1, skipped: 0, failed: 1 },
        results: [
          { key_id: 'key-a', status: 'success', detail: null },
          { key_id: 'key-b', status: 'failed', detail: 'unauthorized' },
        ],
      },
    })

    expect(state.current).toBe(2)
    expect(state.total).toBe(2)
    expect(state.steps[0]).toMatchObject({ status: 'done' })
    expect(state.steps[1]).toMatchObject({ status: 'done' })
    expect(state.lastResult).toMatchObject({
      keyId: 'key-b',
      status: 'failed',
      detail: 'unauthorized',
    })
  })
})
