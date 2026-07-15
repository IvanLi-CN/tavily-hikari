import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import meta, * as systemSettingsStories from './SystemSettingsModule.stories'

describe('SystemSettingsModule Storybook proofs', () => {
  it('keeps the default, request-rate, rebalance, comparison-only, applying, error, and help-bubble stories available', () => {
    expect(meta).toMatchObject({
      title: 'Admin/SystemSettingsModule',
    })

    expect(systemSettingsStories.Default).toMatchObject({})
    expect(systemSettingsStories.RequestRateEdited).toMatchObject({})
    expect(systemSettingsStories.RebalanceEnabled).toMatchObject({})
    expect(systemSettingsStories.ApiRebalanceEnabled).toMatchObject({})
    expect(systemSettingsStories.FixedProjectIdAndControlUa).toMatchObject({})
    expect(systemSettingsStories.ComparisonOnlyReconciliation).toMatchObject({})
    expect(systemSettingsStories.Applying).toMatchObject({})
    expect(systemSettingsStories.ErrorState).toMatchObject({})
    expect(systemSettingsStories.HelpBubbleOpen).toMatchObject({})
    expect(systemSettingsStories.BlockedKeyBaseConfigured).toMatchObject({})
    expect(systemSettingsStories.AutosaveOnBlur).toMatchObject({})
    expect(systemSettingsStories.ClientIpDialogWithObservedValues).toMatchObject({})
  })

  it('renders the applying story without Storybook runtime helpers', () => {
    const renderStory = systemSettingsStories.Applying.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(createElement(renderStory!))
    expect(markup).toContain('应用中')
  })

  it('renders the help bubble story in the forced-open state', () => {
    const renderStory = systemSettingsStories.HelpBubbleOpen.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(createElement(renderStory!))
    expect(markup).toContain('显示系统设置说明')
    expect(markup).toContain('data-state="instant-open"')
  })

  it('renders the request-rate story without redundant current-value copy', () => {
    const renderStory = systemSettingsStories.RequestRateEdited.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(createElement(renderStory!))
    expect(markup).toContain('5 分钟最大请求数')
    expect(markup).not.toContain('当前阈值：80')
  })

  it('renders the blocked-key base limit story without redundant current-value copy', () => {
    const renderStory = systemSettingsStories.BlockedKeyBaseConfigured.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(createElement(renderStory!))
    expect(markup).toContain('封禁数基础值')
    expect(markup).not.toContain('当前基础值：8')
  })

  it('renders the API rebalance story without rollout controls', () => {
    const renderStory = systemSettingsStories.ApiRebalanceEnabled.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(createElement(renderStory!))
    expect(markup).toContain('启用 API Rebalance')
    expect(markup).not.toContain('Tavily API Rebalance')
    expect(markup).not.toContain('API 请求放量比例')
  })

  it('renders the fixed project id story with the configured Control MCP UA', () => {
    const renderStory = systemSettingsStories.FixedProjectIdAndControlUa.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(createElement(renderStory!))
    expect(markup).toContain('X-Project-ID 策略')
    expect(markup).toContain('team-search-prod')
    expect(markup).toContain('custom-control-mcp')
  })

  it('renders the comparison-only reconciliation story without redundant current-state copy', () => {
    const renderStory =
      systemSettingsStories.ComparisonOnlyReconciliation.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(createElement(renderStory!))
    expect(markup).not.toContain('当前：仅对比展示，不影响真实扣费。')
  })

  it('renders the default story without redundant current-state copy', () => {
    const renderStory = meta.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(createElement(renderStory!))
    expect(markup).not.toContain('当前：仅对比展示，不影响真实扣费。')
  })
})
