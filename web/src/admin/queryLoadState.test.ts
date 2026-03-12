import { describe, expect, it } from 'bun:test'

import {
  getBlockingLoadState,
  getRefreshingLoadState,
  isBlockingLoadState,
  isRefreshingLoadState,
} from './queryLoadState'

describe('query load state helpers', () => {
  it('treats the first fetch as initial loading', () => {
    expect(getBlockingLoadState(false)).toBe('initial_loading')
    expect(getRefreshingLoadState(false)).toBe('initial_loading')
  })

  it('distinguishes query switches from same-query refreshes after data has resolved', () => {
    expect(getBlockingLoadState(true)).toBe('switch_loading')
    expect(getRefreshingLoadState(true)).toBe('refreshing')
  })

  it('classifies blocking vs non-blocking states', () => {
    expect(isBlockingLoadState('initial_loading')).toBe(true)
    expect(isBlockingLoadState('switch_loading')).toBe(true)
    expect(isBlockingLoadState('refreshing')).toBe(false)
    expect(isBlockingLoadState('ready')).toBe(false)
    expect(isBlockingLoadState('error')).toBe(false)

    expect(isRefreshingLoadState('refreshing')).toBe(true)
    expect(isRefreshingLoadState('ready')).toBe(false)
    expect(isRefreshingLoadState('switch_loading')).toBe(false)
  })
})
