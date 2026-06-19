import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import { ThemeProvider } from '../theme'
import { TooltipProvider } from '../components/ui/tooltip'
import * as stories from './AdminUserRankingsPage.stories'

describe('AdminUserRankingsPage Storybook proofs', () => {
  it('renders the default rankings story with rolling windows and top-20 chart shells', () => {
    const renderStory = stories.Default.render as ((args: Record<string, unknown>) => JSX.Element) | undefined
    expect(renderStory).toBeDefined()
    const args = {
      ...(stories.default.args ?? {}),
      ...(stories.Default.args ?? {}),
    }
    const storyNode = renderStory!(args)

    const markup = renderToStaticMarkup(
      createElement(ThemeProvider, null, createElement(TooltipProvider, null, storyNode)),
    )

    expect(markup).toContain('用户排行')
    expect(markup).toContain('最近 24 小时')
    expect(markup).toContain('最近 7 天')
    expect(markup).toContain('最近 30 天')
    expect(markup).toContain('TOP20 用户')
    expect(markup).toContain('admin-ranking-chart-shell')
    expect(markup.match(/<canvas/g)?.length ?? 0).toBeGreaterThanOrEqual(6)
  })

  it('renders the empty story with the shared empty state copy', () => {
    const renderStory = stories.EmptyState.render as ((args: Record<string, unknown>) => JSX.Element) | undefined
    expect(renderStory).toBeDefined()
    const args = {
      ...(stories.default.args ?? {}),
      ...(stories.EmptyState.args ?? {}),
    }
    const storyNode = renderStory!(args)

    const markup = renderToStaticMarkup(
      createElement(ThemeProvider, null, createElement(TooltipProvider, null, storyNode)),
    )

    expect(markup).toContain('当前时间窗暂无可展示的用户数据。')
  })
})
