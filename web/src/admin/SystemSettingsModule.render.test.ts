import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import SystemSettingsModule, {
  parseTrustedClientIpHeaderDraft,
  toggleOrderedHeaderDraft,
} from './SystemSettingsModule'
import { translations } from '../i18n'

const strings = translations.zh.admin.systemSettings

describe('SystemSettingsModule rendering', () => {
  it('toggles trusted client IP header presets at the end of the ordered draft', () => {
    expect(toggleOrderedHeaderDraft('cf-connecting-ip\nx-real-ip', 'x-forwarded-for')).toBe(
      'cf-connecting-ip\nx-real-ip\nx-forwarded-for',
    )
    expect(toggleOrderedHeaderDraft('cf-connecting-ip\nx-real-ip\nx-forwarded-for', 'x-real-ip')).toBe(
      'cf-connecting-ip\nx-forwarded-for',
    )
  })

  it('reports duplicated trusted client IP headers with exact line numbers', () => {
    expect(
      parseTrustedClientIpHeaderDraft('cf-connecting-ip\nx-forwarded-for\nCF-Connecting-IP').duplicateError,
    ).toBe('客户端 IP 请求头重复：cf-connecting-ip 出现在第 1、3 行')
    expect(
      parseTrustedClientIpHeaderDraft('cf-connecting-ip\nx-forwarded-for\nx-forwarded-for\ncf-connecting-ip')
        .duplicateError,
    ).toBe('客户端 IP 请求头重复：cf-connecting-ip 出现在第 1、4 行；x-forwarded-for 出现在第 2、3 行')
  })

  it('renders the help trigger while keeping explanatory copy inside the tooltip bubble', () => {
    const markup = renderToStaticMarkup(
      createElement(SystemSettingsModule, {
        strings,
        settings: {
          requestRateLimit: 100,
          mcpSessionAffinityKeyCount: 5,
          rebalanceMcpEnabled: false,
          rebalanceMcpSessionPercent: 100,
          apiRebalanceEnabled: false,
          apiRebalancePercent: 0,
          userBlockedKeyBaseLimit: 5,
          globalIpLimit: 5,
          trustedProxyCidrs: ["127.0.0.0/8", "::1/128"],
          trustedClientIpHeaders: ["cf-connecting-ip", "x-forwarded-for"],
        },
        loadState: 'ready',
        error: null,
        saving: false,
        onApply: () => {},
      }),
    )

    expect(markup).toContain(strings.title)
    expect(markup).toContain(strings.helpLabel)
    expect(markup.match(/system-settings-help-trigger/g)?.length).toBe(1)
    expect(markup).toContain(strings.form.currentRequestRateLimitValue.replace('{count}', '100'))
    expect(markup).toContain(strings.form.requestRateLimitHint)
    expect(markup).toContain(strings.form.currentValue.replace('{count}', '5'))
    expect(markup).toContain(strings.form.currentPercentValue.replace('{percent}', '100'))
    expect(markup).toContain(strings.form.currentApiRebalancePercentValue.replace('{percent}', '0'))
    expect(markup).toContain(strings.form.apiRebalancePercentDisabledHint)
    expect(markup).toContain(strings.form.currentBlockedKeyBaseLimitValue.replace('{count}', '5'))
    expect(markup).toContain(strings.form.blockedKeyBaseLimitHint)
    expect(markup).toContain(strings.form.currentGlobalIpLimitValue.replace('{count}', '5'))
    expect(markup).toContain(strings.form.globalIpLimitHint)
    expect(markup).toContain('配置可信 IP')
    expect(markup).not.toContain(strings.description)
    expect(markup).not.toContain(strings.form.description)
    expect(markup).not.toContain(strings.form.countHint)
    expect(markup).not.toContain(strings.form.percentHint)
    expect(markup).not.toContain(strings.form.applyScopeHint)
  })

  it('renders the saving state copy when apply is in progress', () => {
    const markup = renderToStaticMarkup(
      createElement(SystemSettingsModule, {
        strings,
        settings: {
          requestRateLimit: 100,
          mcpSessionAffinityKeyCount: 5,
          rebalanceMcpEnabled: true,
          rebalanceMcpSessionPercent: 35,
          apiRebalanceEnabled: true,
          apiRebalancePercent: 25,
          userBlockedKeyBaseLimit: 5,
          globalIpLimit: 5,
          trustedProxyCidrs: ["127.0.0.0/8", "::1/128"],
          trustedClientIpHeaders: ["cf-connecting-ip", "x-forwarded-for"],
        },
        loadState: 'ready',
        error: null,
        saving: true,
        onApply: () => {},
      }),
    )

    expect(markup).toContain(strings.actions.applying)
    expect(markup).toContain('icon-spin')
  })

  it('shows the locked hint when rebalance is disabled', () => {
    const markup = renderToStaticMarkup(
      createElement(SystemSettingsModule, {
        strings,
        settings: {
          requestRateLimit: 100,
          mcpSessionAffinityKeyCount: 5,
          rebalanceMcpEnabled: false,
          rebalanceMcpSessionPercent: 35,
          apiRebalanceEnabled: false,
          apiRebalancePercent: 25,
          userBlockedKeyBaseLimit: 5,
          globalIpLimit: 5,
          trustedProxyCidrs: ["127.0.0.0/8", "::1/128"],
          trustedClientIpHeaders: ["cf-connecting-ip", "x-forwarded-for"],
        },
        loadState: 'ready',
        error: null,
        saving: false,
        onApply: () => {},
      }),
    )

    expect(markup).toContain(strings.form.percentDisabledHint)
    expect(markup).toContain(strings.form.apiRebalancePercentDisabledHint)
  })
})
