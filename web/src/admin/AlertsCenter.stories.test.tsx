import { describe, expect, it } from 'bun:test'
import { renderToString } from 'react-dom/server'

import {
  AlertsCenterRefreshingStoryShell,
  AlertsCenterStoryShell,
  BackgroundRefreshKeepsRows,
  EventsDefault,
  GroupsView,
} from './AlertsCenter.stories'

describe('AlertsCenter Storybook proofs', () => {
  it('keeps event and grouped alert stories available', () => {
    expect(EventsDefault.args?.initialSearch).toContain('view=events')
    expect(GroupsView.args?.initialSearch).toContain('view=groups')
    expect(BackgroundRefreshKeepsRows.args?.initialSearch).toContain('requestKinds=tavily_search')
  })

  it('renders the grouped story with alert center chrome and shared filters', () => {
    const markup = renderToString(<AlertsCenterStoryShell {...(GroupsView.args ?? {})} />)
    expect(markup).toContain('告警中心')
    expect(markup).toContain('聚合告警')
    expect(markup).toContain('上游 Key 封禁')
    expect(markup).toContain('请求类型')
  })

  it('renders the background-refresh story with loaded rows as the initial canvas state', () => {
    const markup = renderToString(<AlertsCenterRefreshingStoryShell {...(BackgroundRefreshKeepsRows.args ?? {})} />)

    expect(markup).toContain('告警中心')
    expect(markup).toContain('用户额度耗尽')
    expect(markup).not.toContain('admin-loading-region-placeholder')
  })
})
