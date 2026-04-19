import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import SystemSettingsModule from './SystemSettingsModule'
import { translations } from '../i18n'

const strings = translations.zh.admin.systemSettings

describe('SystemSettingsModule rendering', () => {
  it('renders the help trigger while keeping explanatory copy inside the tooltip bubble', () => {
    const markup = renderToStaticMarkup(
      createElement(SystemSettingsModule, {
        strings,
        settings: {
          requestRateLimit: 100,
          mcpSessionAffinityKeyCount: 5,
          rebalanceMcpEnabled: false,
          rebalanceMcpSessionPercent: 100,
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
        },
        loadState: 'ready',
        error: null,
        saving: false,
        onApply: () => {},
      }),
    )

    expect(markup).toContain(strings.form.percentDisabledHint)
  })
})
