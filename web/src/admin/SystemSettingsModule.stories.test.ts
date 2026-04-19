import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import meta, * as systemSettingsStories from './SystemSettingsModule.stories'

describe('SystemSettingsModule Storybook proofs', () => {
  it('keeps the default, request-rate, rebalance toggle, applying, error, and help-bubble stories available', () => {
    expect(meta).toMatchObject({
      title: 'Admin/SystemSettingsModule',
    })

    expect(systemSettingsStories.Default).toMatchObject({})
    expect(systemSettingsStories.RequestRateEdited).toMatchObject({})
    expect(systemSettingsStories.RebalanceEnabled).toMatchObject({})
    expect(systemSettingsStories.RebalanceDisabledSliderLocked).toMatchObject({})
    expect(systemSettingsStories.Applying).toMatchObject({})
    expect(systemSettingsStories.ErrorState).toMatchObject({})
    expect(systemSettingsStories.HelpBubbleOpen).toMatchObject({})
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

  it('renders the request-rate story with the current threshold copy', () => {
    const renderStory = systemSettingsStories.RequestRateEdited.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(createElement(renderStory!))
    expect(markup).toContain('5 分钟最大请求数')
    expect(markup).toContain('当前阈值：80')
  })
})
