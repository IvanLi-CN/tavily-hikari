export type QueryLoadState = 'initial_loading' | 'switch_loading' | 'refreshing' | 'ready' | 'error'

export function getBlockingLoadState(hasLoadedOnce: boolean): QueryLoadState {
  return hasLoadedOnce ? 'switch_loading' : 'initial_loading'
}

export function getRefreshingLoadState(hasLoadedOnce: boolean): QueryLoadState {
  return hasLoadedOnce ? 'refreshing' : 'initial_loading'
}

export function isBlockingLoadState(state: QueryLoadState): boolean {
  return state === 'initial_loading' || state === 'switch_loading'
}

export function isRefreshingLoadState(state: QueryLoadState): boolean {
  return state === 'refreshing'
}
