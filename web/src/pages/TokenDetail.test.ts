import { describe, expect, it } from 'bun:test'

import { __testables } from './TokenDetail'

describe('TokenDetail request log pagination helpers', () => {
  it('changes the list query key when the page size changes on the newest page', () => {
    expect(__testables.buildTokenLogsListQueryKey('token:a1b2', null, 'older', 20)).toBe(
      'token:a1b2:cursor=:direction=older:perPage=20',
    )
    expect(__testables.buildTokenLogsListQueryKey('token:a1b2', null, 'older', 50)).toBe(
      'token:a1b2:cursor=:direction=older:perPage=50',
    )
  })

  it('clears stale pagination controls with an empty page shell for the requested page size', () => {
    expect(__testables.createEmptyTokenLogsListPage(50)).toEqual({
      items: [],
      pageSize: 50,
      nextCursor: null,
      prevCursor: null,
      hasOlder: false,
      hasNewer: false,
    })
  })
})
