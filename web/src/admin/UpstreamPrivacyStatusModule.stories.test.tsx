import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import meta, * as systemStatusStories from './UpstreamPrivacyStatusModule.stories'
import UpstreamPrivacyStatusModule from './UpstreamPrivacyStatusModule'
import { translations } from '../i18n'

describe('SystemStatusModule Storybook proofs', () => {
  it('keeps the reconciliation state matrix and fallback stories available', () => {
    expect(meta).toMatchObject({
      title: 'Admin/Modules/SystemStatusModule',
    })

    expect(systemStatusStories.Pending).toMatchObject({})
    expect(systemStatusStories.BlockedBySessions).toMatchObject({})
    expect(systemStatusStories.CompareOnly).toMatchObject({})
    expect(systemStatusStories.Active).toMatchObject({})
    expect(systemStatusStories.ActivePaused).toMatchObject({})
    expect(systemStatusStories.Healthy).toMatchObject({})
    expect(systemStatusStories.Degraded).toMatchObject({})
    expect(systemStatusStories.LocalBackoff).toMatchObject({})
    expect(systemStatusStories.EvidenceLocalBackoff).toMatchObject({})
    expect(systemStatusStories.UpstreamBackoff).toMatchObject({})
    expect(systemStatusStories.GlobalBackoff).toMatchObject({})
    expect(systemStatusStories.UnknownObservation).toMatchObject({})
    expect(systemStatusStories.BudgetExhausted).toMatchObject({})
    expect(systemStatusStories.NoAdjustment).toMatchObject({})
    expect(systemStatusStories.MissingEligibleUpstreamKey).toMatchObject({})
    expect(systemStatusStories.EmptyState).toMatchObject({})
    expect(systemStatusStories.ErrorState).toMatchObject({})
    expect(systemStatusStories.LoadingState).toMatchObject({})
    expect(systemStatusStories.Mobile393x852).toMatchObject({})
    expect(systemStatusStories.TransportFailureMobile393x852).toMatchObject({})
    expect(systemStatusStories.EvidenceLocalBackoffMobile393x852).toMatchObject({})
    expect(systemStatusStories.MobileStateGallery).toMatchObject({})
    expect(systemStatusStories.Mobile).toMatchObject({})
    expect(systemStatusStories.Gallery).toMatchObject({})
    expect(systemStatusStories.InteractionContract).toMatchObject({})
  })

  it('renders the base module without a duplicate route title and keeps the auto-refresh label wiring', () => {
    const markup = renderToStaticMarkup(createElement(UpstreamPrivacyStatusModule, meta.args))

    expect(markup).not.toContain('<h2>系统状态</h2>')
    expect(markup).toContain('自动刷新')
    expect(markup).toContain('aria-labelledby')
    expect(markup).toContain('需要关注')
    expect(markup).toContain('对账落账模式')
    expect(markup).toContain('活跃 upstream_mcp session')
    expect(markup).toContain('最近对账运行')
    expect(markup).toContain('最近对账结果')
    expect(markup).toContain('最近 shadow 调整')
    expect(markup).toContain('最近入队失败')
    expect(markup).toContain('重试原因分布')
    expect(markup).toContain('对账控制器')
    expect(markup).toContain('告警投影覆盖')
    expect(markup).toContain('告警投影状态')
    expect(markup).toContain('429 上游限流')
    expect(markup).toContain('本地 usage 限流')
    expect(markup).toContain('缺少可用上游 Key')
    expect(markup).toContain('当前时段 Key 活动')
    expect(markup).toContain('待查询 Project ID 数')
    expect(markup).not.toContain('立即刷新')
  })

  it('renders the gallery story with the state matrix and error fallback', () => {
    const renderStory = systemStatusStories.Gallery.render as (() => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(createElement(renderStory!))
    expect(markup).toContain('Pending')
    expect(markup).toContain('Compare')
    expect(markup).toContain('Degraded')
    expect(markup).toContain('Active paused')
    expect(markup).toContain('其余 2 个 Key')
    expect(markup).toContain('其余 3 个 Key')
    expect(markup).toContain(translations.zh.admin.systemSettings.privacy.loadFailed)
  })

  it('renders local and upstream reconciliation backoff as distinct actionable states', () => {
    const localArgs = { ...meta.args, ...systemStatusStories.LocalBackoff.args }
    const upstreamArgs = { ...meta.args, ...systemStatusStories.UpstreamBackoff.args }
    const localMarkup = renderToStaticMarkup(createElement(UpstreamPrivacyStatusModule, localArgs))
    const upstreamMarkup = renderToStaticMarkup(createElement(UpstreamPrivacyStatusModule, upstreamArgs))

    expect(localMarkup).toContain('对账本地退避')
    expect(upstreamMarkup).toContain('对账上游退避')
    expect(upstreamMarkup).toContain('级别 1')
  })

  it('renders reconciliation budget exhaustion with the last round aggregate', () => {
    const args = { ...meta.args, ...systemStatusStories.BudgetExhausted.args }
    const markup = renderToStaticMarkup(createElement(UpstreamPrivacyStatusModule, args))

    expect(markup).toContain('20,000 ms')
    expect(markup).toContain('20 / 0 / 0 / 16')
    expect(markup).toContain('预算耗尽')
    expect(markup).toContain('已耗尽')
  })

  it('renders no adjustment as a healthy non-error outcome', () => {
    const args = { ...meta.args, ...systemStatusStories.NoAdjustment.args }
    const markup = renderToStaticMarkup(createElement(UpstreamPrivacyStatusModule, args))

    expect(markup).toContain('无需调整')
    expect(markup).toContain('6 / 6 / 6 / 0')
  })
})
