import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import { ThemeProvider } from '../theme'
import { TooltipProvider } from '../components/ui/tooltip'
import * as stories from './AdminUserRankingsPage.stories'

describe('AdminUserRankingsPage Storybook proofs', () => {
  it('renders the default rankings story with a single active rolling window and two chart shells', () => {
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
    expect(markup).toContain('时间范围')
    expect(markup).toContain('最后更新')
    expect(markup).toContain('实时连接正常')
    expect(markup).toContain('admin-ranking-chart-shell')
    expect(markup.match(/echarts-for-react/g)?.length ?? 0).toBeGreaterThanOrEqual(2)
    expect(markup).toContain('1. Alice Chen: 184')
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

  it('renders the degraded state with retry affordance and stale hint', () => {
    const renderStory = stories.ErrorState.render as ((args: Record<string, unknown>) => JSX.Element) | undefined
    expect(renderStory).toBeDefined()
    const args = {
      ...(stories.default.args ?? {}),
      ...(stories.ErrorState.args ?? {}),
    }
    const storyNode = renderStory!(args)

    const markup = renderToStaticMarkup(
      createElement(ThemeProvider, null, createElement(TooltipProvider, null, storyNode)),
    )

    expect(markup).toContain('实时更新异常')
    expect(markup).toContain('立即重试')
    expect(markup).toContain('当前展示最近一次成功快照')
  })
})
