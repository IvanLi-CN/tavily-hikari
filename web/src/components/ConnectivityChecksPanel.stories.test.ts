import { describe, expect, it } from 'bun:test'

import meta, * as connectivityStories from './ConnectivityChecksPanel.stories'

describe('ConnectivityChecksPanel Storybook gallery', () => {
  it('publishes a single aggregated gallery story for probe states', () => {
    expect(meta).toMatchObject({
      title: 'User Console/Fragments/Connectivity Checks',
      tags: ['autodocs'],
      parameters: {
        layout: 'padded',
        controls: { disable: true },
      },
    })

    expect(connectivityStories.StateGallery).toMatchObject({
      name: 'State Gallery',
    })
    expect(connectivityStories).not.toHaveProperty('Idle')
    expect(connectivityStories).not.toHaveProperty('ApiCheckRunning')
    expect(connectivityStories).not.toHaveProperty('AllChecksPass')
    expect(connectivityStories).not.toHaveProperty('PartialAvailability')
    expect(connectivityStories).not.toHaveProperty('AuthenticationFailed')
    expect(connectivityStories).not.toHaveProperty('QuotaBlocked')
  })

  it('keeps the quota-blocked gallery aligned with runtime MCP ping behavior', () => {
    const quotaBlocked = connectivityStories.__testables.scenarios.find((scenario) => scenario.title === 'Quota Blocked')

    expect(quotaBlocked?.probeBubble?.items[0]).toEqual({
      id: 'mcp-ping',
      label: 'MCP service connectivity',
      status: 'success',
    })
  })

  it('adds extra vertical spacing so long probe bubbles do not overlap adjacent rows', () => {
    expect(connectivityStories.__testables.galleryGridStyle).toMatchObject({
      columnGap: 18,
      rowGap: 196,
      alignItems: 'start',
    })
  })
})
