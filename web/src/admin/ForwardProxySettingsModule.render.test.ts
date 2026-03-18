import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import {
  buildValidationNodeRows,
  canImportSubscriptionDuringValidation,
  formatValidationMessage,
  resolveManualBatchButtonLabel,
  type ForwardProxyValidationEntry,
} from './ForwardProxySettingsModule'
import { translations } from '../i18n'
import ForwardProxySettingsModule from './ForwardProxySettingsModule'
import { forwardProxyStorySavedAt, forwardProxyStorySettings, forwardProxyStoryStats } from './forwardProxyStoryData'

const strings = translations.zh.admin.proxySettings

describe('ForwardProxySettingsModule dialog helpers', () => {
  it('uses the direct-import label before validation results exist', () => {
    expect(resolveManualBatchButtonLabel(strings, false, 0)).toBe('导入输入内容')
    expect(resolveManualBatchButtonLabel(strings, true, 0)).toBe('导入输入内容')
    expect(resolveManualBatchButtonLabel(strings, true, 2)).toBe('导入 2 个节点')
  })

  it('builds node rows from completed subscription validation results', () => {
    const rows = buildValidationNodeRows(strings, true, [
      {
        id: 'subscription-success',
        kind: 'subscriptionUrl',
        value: 'https://subscription.example.com/feed',
        result: {
          ok: true,
          message: 'subscription validation succeeded',
          normalizedValue: 'https://subscription.example.com/feed',
          discoveredNodes: 3,
          latencyMs: 128.45,
          nodes: [
            {
              displayName: 'Tokyo-A',
              protocol: 'ss',
              ok: true,
              ip: '203.0.113.8',
              location: 'JP / NRT',
              latencyMs: 128.45,
            },
          ],
        },
      } satisfies ForwardProxyValidationEntry,
    ])

    expect(rows).toEqual([
      {
        id: 'subscription-node-1',
        displayName: 'Tokyo-A',
        protocol: 'ss',
        ip: '203.0.113.8',
        location: 'JP / NRT',
        latencyMs: 128.45,
        status: 'ok',
        message: 'subscription validation succeeded',
      },
    ])
  })

  it('does not show latency for failed subscription rows', () => {
    const rows = buildValidationNodeRows(strings, true, [
      {
        id: 'subscription-failure-with-best-latency',
        kind: 'subscriptionUrl',
        value: 'https://subscription.example.com/feed',
        result: {
          ok: false,
          message: 'subscription validation failed',
          normalizedValue: 'https://subscription.example.com/feed',
          discoveredNodes: 3,
          latencyMs: 81.58,
          nodes: [
            {
              displayName: 'HongKong-A',
              protocol: 'ss',
              ok: false,
              latencyMs: 81.58,
              message: 'validation timed out after 5000ms',
            },
          ],
        },
      } satisfies ForwardProxyValidationEntry,
    ])

    expect(rows[0]).toMatchObject({
      protocol: 'ss',
      status: 'failed',
      latencyMs: null,
    })
  })

  it('does not fabricate a subscription node row when validation produced no nodes', () => {
    const rows = buildValidationNodeRows(strings, true, [
      {
        id: 'subscription-failure',
        kind: 'subscriptionUrl',
        value: 'https://subscription.example.com/feed',
        result: {
          ok: false,
          message: 'other error: subscription proxy probe failed: other error: validation timed out after 60000ms; no entry passed validation',
          normalizedValue: 'https://subscription.example.com/feed',
          discoveredNodes: 0,
          errorCode: 'subscription_unreachable',
          nodes: [],
        },
      } satisfies ForwardProxyValidationEntry,
    ])

    expect(rows).toEqual([])
  })

  it('maps subscription timeout failures to a product summary', () => {
    const message = formatValidationMessage(strings, {
      ok: false,
      message: 'other error: subscription proxy probe failed: other error: validation timed out after 60000ms; no entry passed validation',
      normalizedValue: 'https://subscription.example.com/feed',
      discoveredNodes: 0,
      errorCode: 'subscription_unreachable',
      nodes: [],
    })

    expect(message).toBe('订阅验证超时，未发现可用节点')
  })

  it('maps empty subscription results to a product summary', () => {
    const message = formatValidationMessage(strings, {
      ok: false,
      message: 'subscription fetch resolved zero proxy entries from the response body',
      normalizedValue: 'https://subscription.example.com/feed',
      discoveredNodes: 0,
      errorCode: 'subscription_invalid',
      nodes: [],
    })

    expect(message).toBe('订阅中没有解析出任何节点')
  })

  it('maps unsupported subscription formats to a product summary', () => {
    const message = formatValidationMessage(strings, {
      ok: false,
      message: 'subscription contains no supported proxy entries',
      normalizedValue: 'https://subscription.example.com/feed',
      discoveredNodes: 0,
      errorCode: 'subscription_invalid',
      nodes: [],
    })

    expect(message).toBe('订阅中的节点格式暂不支持')
  })

  it('allows importing a subscription as soon as the first live success appears', () => {
    expect(canImportSubscriptionDuringValidation([
      {
        id: 'edge-a',
        displayName: 'edge-a',
        protocol: 'ss',
        ip: '203.0.113.10',
        location: 'JP / NRT',
        latencyMs: 142.3,
        status: 'probing',
        message: '',
      },
    ])).toBe(true)

    expect(canImportSubscriptionDuringValidation([
      {
        id: 'edge-b',
        displayName: 'edge-b',
        protocol: 'vless',
        ip: null,
        location: null,
        latencyMs: null,
        status: 'probing',
        message: '',
      },
    ])).toBe(false)
  })

  it('renders the subscription action group before the manual action group', () => {
    const markup = renderToStaticMarkup(
      createElement(ForwardProxySettingsModule, {
        strings,
        settings: forwardProxyStorySettings,
        stats: forwardProxyStoryStats,
        settingsLoadState: 'ready',
        statsLoadState: 'ready',
        settingsError: null,
        statsError: null,
        saveError: null,
        revalidateError: null,
        saving: false,
        revalidating: false,
        savedAt: forwardProxyStorySavedAt,
        revalidateProgress: null,
        egressPreviewProgress: null,
        onPersistDraft: async () => {},
        onValidateCandidates: async () => [],
        onRefresh: () => {},
        onRevalidate: () => {},
        dialogPreview: null,
        onDialogPreviewClose: () => {},
      }),
    )

    expect(markup.indexOf(strings.config.addSubscription)).toBeGreaterThan(-1)
    expect(markup.indexOf(strings.config.addManual)).toBeGreaterThan(-1)
    expect(markup.indexOf(strings.config.subscriptionCount.replace('{count}', '2'))).toBeGreaterThan(-1)
    expect(markup.indexOf(strings.config.manualCount.replace('{count}', '3'))).toBeGreaterThan(-1)
    expect(markup.indexOf(strings.config.addSubscription)).toBeLessThan(markup.indexOf(strings.config.addManual))
    expect(markup.indexOf(strings.config.subscriptionCount.replace('{count}', '2'))).toBeLessThan(
      markup.indexOf(strings.config.manualCount.replace('{count}', '3')),
    )
  })
})
