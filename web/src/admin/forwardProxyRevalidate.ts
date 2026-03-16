export interface ForwardProxyStatsRefreshOptions {
  reason: 'refresh'
}

export async function finalizeForwardProxyRevalidate(
  loadForwardProxyStatsData: (options: ForwardProxyStatsRefreshOptions) => Promise<void>,
  markRefreshUi: () => void,
  markComplete: () => void,
): Promise<void> {
  markRefreshUi()
  await loadForwardProxyStatsData({ reason: 'refresh' })
  markComplete()
}
