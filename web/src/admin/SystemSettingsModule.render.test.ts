import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import SystemSettingsModule, {
  parseTrustedClientIpHeaderDraft,
  toggleOrderedHeaderDraft,
} from './SystemSettingsModule'
import { translations } from '../i18n'

const zhStrings = translations.zh.admin.systemSettings
const enStrings = translations.en.admin.systemSettings

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
        strings: zhStrings,
        settings: {
          requestRateLimit: 100,
          authTokenLogRetentionDays: 92,
          mcpSessionAffinityKeyCount: 5,
          rebalanceMcpEnabled: false,
          rebalanceMcpSessionPercent: 0,
          apiRebalanceEnabled: false,
          apiRebalancePercent: 0,
          upstreamProjectIdMode: 'accessToken',
          upstreamProjectIdFixedValue: '',
          upstreamMcpUserAgent: '',
          upstreamPreciseReconciliationEnabled: false,
          rechargeFeatureEnabled: true,
          rechargeUserEnabled: true,
          adminDefaultActiveUsersOnly: false,
          userBlockedKeyBaseLimit: 5,
          globalIpLimit: 5,
          trustedProxyCidrs: ["127.0.0.0/8", "::1/128"],
          trustedClientIpHeaders: ["cf-connecting-ip", "x-forwarded-for"],
          requestLogRetention: {
            maxLogRetentionDays: 32,
            heavyUsageThresholdPercent: 80,
            global: { businessBodyDays: 7, nonBusinessBodyDays: 0, nonSuccessBodyDays: 3 },
            heavyUsage: { businessBodyDays: 3, nonBusinessBodyDays: 0, nonSuccessBodyDays: 1 },
            debugShared: { businessBodyDays: 14, nonBusinessBodyDays: 1, nonSuccessBodyDays: 7 },
          },
        },
        loadState: 'ready',
        error: null,
        saving: false,
        userListStats: { activeUsers90d: 128, totalUsers: 346, windowDays: 90 },
        onApply: () => {},
      }),
    )

    expect(markup).toContain(zhStrings.title)
    expect(markup).toContain(zhStrings.helpLabel)
    expect(markup).toContain(zhStrings.form.displayDensityTitle)
    expect(markup).toContain(zhStrings.form.displayDensityComfortable)
    expect(markup).toContain(zhStrings.form.displayDensityCompact)
    expect(markup.match(/system-settings-help-trigger/g)?.length).toBe(1)
    expect(markup).not.toContain('当前阈值：100')
    expect(markup).toContain(zhStrings.form.requestRateLimitHint)
    expect(markup).not.toContain('当前值：5')
    expect(markup).toContain(zhStrings.form.rebalanceLabel)
    expect(markup).toContain(zhStrings.form.apiRebalanceLabel)
    expect(markup).not.toContain('Tavily API Rebalance')
    expect(markup).toContain(zhStrings.form.upstreamProjectIdModeLabel)
    expect(markup).toContain(zhStrings.form.upstreamMcpUserAgentLabel)
    expect(markup).toContain('system-settings-select-trigger')
    expect(markup).not.toContain('<select id="system-settings-upstream-project-id-mode"')
    expect(markup).toContain(zhStrings.form.upstreamPreciseReconciliationTitle)
    expect(markup).not.toContain('当前：仅对比展示，不影响真实扣费。')
    expect(markup).toContain(zhStrings.form.rechargeFeatureLabel)
    expect(markup).toContain(zhStrings.form.rechargeUserLabel)
    expect(markup).toContain(zhStrings.form.activeUsersDefaultLabel)
    expect(markup).toContain(zhStrings.form.activeUsersDefaultCount.replace('{active}', '128').replace('{total}', '346'))
    expect(markup).not.toContain('当前基础值：5')
    expect(markup).toContain(zhStrings.form.blockedKeyBaseLimitHint)
    expect(markup).not.toContain('当前限制：5')
    expect(markup).toContain(zhStrings.form.globalIpLimitHint)
    expect(markup).toContain('配置可信 IP')
    expect(markup).not.toContain('system-settings-apply')
    expect(markup).not.toContain(zhStrings.description)
    expect(markup).not.toContain(zhStrings.form.description)
    expect(markup).not.toContain(zhStrings.form.countHint)
    expect(markup).not.toContain(zhStrings.form.percentHint)
    expect(markup).not.toContain(zhStrings.form.percentLabel)
    expect(markup).not.toContain(zhStrings.form.apiRebalancePercentLabel)
    expect(markup).not.toContain(zhStrings.form.applyScopeHint)
  })

  it('renders the auth token retention copy from the provided translation set', () => {
    const markup = renderToStaticMarkup(
      createElement(SystemSettingsModule, {
        strings: enStrings,
        settings: {
          requestRateLimit: 100,
          authTokenLogRetentionDays: 14,
          mcpSessionAffinityKeyCount: 5,
          rebalanceMcpEnabled: false,
          rebalanceMcpSessionPercent: 0,
          apiRebalanceEnabled: false,
          apiRebalancePercent: 0,
          upstreamProjectIdMode: 'accessToken',
          upstreamProjectIdFixedValue: '',
          upstreamMcpUserAgent: '',
  upstreamPreciseReconciliationEnabled: true,
          rechargeFeatureEnabled: true,
          rechargeUserEnabled: true,
          adminDefaultActiveUsersOnly: false,
          userBlockedKeyBaseLimit: 5,
          globalIpLimit: 5,
          trustedProxyCidrs: ["127.0.0.0/8", "::1/128"],
          trustedClientIpHeaders: ["cf-connecting-ip", "x-forwarded-for"],
          requestLogRetention: {
            maxLogRetentionDays: 32,
            heavyUsageThresholdPercent: 80,
            global: { businessBodyDays: 7, nonBusinessBodyDays: 0, nonSuccessBodyDays: 3 },
            heavyUsage: { businessBodyDays: 3, nonBusinessBodyDays: 0, nonSuccessBodyDays: 1 },
            debugShared: { businessBodyDays: 14, nonBusinessBodyDays: 1, nonSuccessBodyDays: 7 },
          },
        },
        loadState: 'ready',
        error: null,
        saving: false,
        onApply: () => {},
      }),
    )

    expect(markup).toContain(enStrings.form.authTokenLogRetentionDaysLabel)
    expect(markup).toContain(enStrings.form.authTokenLogRetentionDaysHint)
  })

  it('renders the saving state copy when apply is in progress', () => {
    const markup = renderToStaticMarkup(
      createElement(SystemSettingsModule, {
        strings: zhStrings,
        settings: {
          requestRateLimit: 100,
          authTokenLogRetentionDays: 92,
          mcpSessionAffinityKeyCount: 5,
          rebalanceMcpEnabled: true,
          rebalanceMcpSessionPercent: 100,
          apiRebalanceEnabled: true,
          apiRebalancePercent: 100,
          upstreamProjectIdMode: 'accessToken',
          upstreamProjectIdFixedValue: '',
          upstreamMcpUserAgent: '',
  upstreamPreciseReconciliationEnabled: true,
          rechargeFeatureEnabled: true,
          rechargeUserEnabled: true,
          adminDefaultActiveUsersOnly: false,
          userBlockedKeyBaseLimit: 5,
          globalIpLimit: 5,
          trustedProxyCidrs: ["127.0.0.0/8", "::1/128"],
          trustedClientIpHeaders: ["cf-connecting-ip", "x-forwarded-for"],
          requestLogRetention: {
            maxLogRetentionDays: 32,
            heavyUsageThresholdPercent: 80,
            global: { businessBodyDays: 7, nonBusinessBodyDays: 0, nonSuccessBodyDays: 3 },
            heavyUsage: { businessBodyDays: 3, nonBusinessBodyDays: 0, nonSuccessBodyDays: 1 },
            debugShared: { businessBodyDays: 14, nonBusinessBodyDays: 1, nonSuccessBodyDays: 7 },
          },
        },
        loadState: 'ready',
        error: null,
        saving: true,
        onApply: () => {},
      }),
    )

    expect(markup).toContain(zhStrings.actions.applying)
  })

  it('does not render the removed rebalance rollout controls', () => {
    const markup = renderToStaticMarkup(
      createElement(SystemSettingsModule, {
        strings: zhStrings,
        settings: {
          requestRateLimit: 100,
          authTokenLogRetentionDays: 92,
          mcpSessionAffinityKeyCount: 5,
          rebalanceMcpEnabled: false,
          rebalanceMcpSessionPercent: 0,
          apiRebalanceEnabled: false,
          apiRebalancePercent: 0,
          upstreamProjectIdMode: 'accessToken',
          upstreamProjectIdFixedValue: '',
          upstreamMcpUserAgent: '',
  upstreamPreciseReconciliationEnabled: true,
          rechargeFeatureEnabled: true,
          rechargeUserEnabled: true,
          adminDefaultActiveUsersOnly: false,
          userBlockedKeyBaseLimit: 5,
          globalIpLimit: 5,
          trustedProxyCidrs: ["127.0.0.0/8", "::1/128"],
          trustedClientIpHeaders: ["cf-connecting-ip", "x-forwarded-for"],
          requestLogRetention: {
            maxLogRetentionDays: 32,
            heavyUsageThresholdPercent: 80,
            global: { businessBodyDays: 7, nonBusinessBodyDays: 0, nonSuccessBodyDays: 3 },
            heavyUsage: { businessBodyDays: 3, nonBusinessBodyDays: 0, nonSuccessBodyDays: 1 },
            debugShared: { businessBodyDays: 14, nonBusinessBodyDays: 1, nonSuccessBodyDays: 7 },
          },
        },
        loadState: 'ready',
        error: null,
        saving: false,
        onApply: () => {},
      }),
    )

    expect(markup).not.toContain(zhStrings.form.percentLabel)
    expect(markup).not.toContain(zhStrings.form.apiRebalancePercentLabel)
    expect(markup).not.toContain('system-settings-rebalance-percent')
    expect(markup).not.toContain('system-settings-api-rebalance-percent')
  })
})
