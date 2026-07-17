import { describe, expect, it } from 'bun:test'

import meta, * as sessionBindingStories from './McpSessionBindingsModule.stories'

describe('McpSessionBindingsModule Storybook proofs', () => {
  it('keeps the hidden-route session binding stories available', () => {
    expect(meta).toMatchObject({
      title: 'Admin/Modules/McpSessionBindingsModule',
    })
    expect(sessionBindingStories.ActiveOnly).toMatchObject({})
    expect(sessionBindingStories.RevokedHistory).toMatchObject({})
    expect(sessionBindingStories.AllStates).toMatchObject({})
    expect(sessionBindingStories.EmptyState).toMatchObject({})
    expect(sessionBindingStories.InteractionContract).toMatchObject({})
  })
})
