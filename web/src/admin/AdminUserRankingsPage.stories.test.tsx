import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import { ThemeProvider } from '../theme'
import { TooltipProvider } from '../components/ui/tooltip'
import * as stories from './AdminUserRankingsPage.stories'

describe('AdminUserRankingsPage Storybook proofs', () => {
  it('renders the rankings content module story with six tabs and three chart shells', () => {
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

    expect(markup).toContain('最近 24 小时')
    expect(markup).toContain('最近 7 天')
    expect(markup).toContain('最近 30 天')
    expect(markup).toContain('主要调用')
    expect(markup).toContain('积分')
    expect(markup).toContain('IP')
    expect(markup).not.toContain('<h1>用户排行</h1>')
    expect(markup.match(/role="radio"/g)?.length ?? 0).toBe(6)
    expect(markup.match(/admin-ranking-chart-shell/g)?.length ?? 0).toBe(3)
    expect(markup).toContain('1. Alice Chen: 184')
  })

  it('renders the metric-tab story inside the three-window card layout', () => {
    const renderStory = stories.DimensionView.render as ((args: Record<string, unknown>) => JSX.Element) | undefined
    expect(renderStory).toBeDefined()
    const args = {
      ...(stories.default.args ?? {}),
      ...(stories.DimensionView.args ?? {}),
    }
    const storyNode = renderStory!(args)

    const markup = renderToStaticMarkup(
      createElement(ThemeProvider, null, createElement(TooltipProvider, null, storyNode)),
    )

    expect(markup).toContain('最近 24 小时')
    expect(markup).toContain('最近 7 天')
    expect(markup).toContain('最近 30 天')
    expect(markup).toContain('按时间窗统计唯一 IP 数')
    expect(markup).toContain('1. Alice Chen: 42')
    expect(markup.match(/admin-ranking-chart-shell/g)?.length ?? 0).toBe(3)
    expect(markup).not.toContain('<h3>主要调用</h3>')
    expect(markup).not.toContain('<h3>积分</h3>')
    expect(markup).not.toContain('<h3>IP</h3>')
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

    expect(markup).toContain('当前分组暂无可展示的用户数据。')
    expect(markup).toContain('admin-ranking-empty-state')
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

    expect(markup).toContain('暂时无法加载用户排行。')
    expect(markup).not.toContain('立即重试')
    expect(markup).toContain('当前展示最近一次成功快照')
  })

  it('renders the loading story with skeleton cards while keeping live tabs', () => {
    const renderStory = stories.LoadingState.render as ((args: Record<string, unknown>) => JSX.Element) | undefined
    expect(renderStory).toBeDefined()
    const args = {
      ...(stories.default.args ?? {}),
      ...(stories.LoadingState.args ?? {}),
    }
    const storyNode = renderStory!(args)

    const markup = renderToStaticMarkup(
      createElement(ThemeProvider, null, createElement(TooltipProvider, null, storyNode)),
    )

    expect(markup).toContain('admin-ranking-skeleton-stage')
    expect(markup).toContain('最近 24 小时')
    expect(markup.match(/admin-ranking-card/g)?.length ?? 0).toBeGreaterThanOrEqual(3)
  })
})
