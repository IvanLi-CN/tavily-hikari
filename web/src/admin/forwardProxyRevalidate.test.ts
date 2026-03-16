import { describe, expect, it } from 'bun:test'

import { finalizeForwardProxyRevalidate } from './forwardProxyRevalidate'

describe('forwardProxyRevalidate', () => {
  it('completes after stats refresh succeeds', async () => {
    const steps: string[] = []

    await finalizeForwardProxyRevalidate(
      async ({ reason }) => {
        steps.push(`load:${reason}`)
      },
      () => {
        steps.push('phase')
      },
      () => {
        steps.push('complete')
      },
    )

    expect(steps).toEqual(['phase', 'load:refresh', 'complete'])
  })

  it('does not mark completion when stats refresh fails', async () => {
    let completed = false

    await expect(
      finalizeForwardProxyRevalidate(
        async () => {
          throw new Error('stats unavailable')
        },
        () => {},
        () => {
          completed = true
        },
      ),
    ).rejects.toThrow('stats unavailable')
    expect(completed).toBe(false)
  })
})
