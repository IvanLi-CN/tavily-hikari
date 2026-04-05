import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import SystemSettingsModule from './SystemSettingsModule'
import { translations } from '../i18n'

const strings = translations.zh.admin.systemSettings

describe('SystemSettingsModule rendering', () => {
  it('renders the current affinity count and apply scope hint', () => {
    const markup = renderToStaticMarkup(
      createElement(SystemSettingsModule, {
        strings,
        settings: { mcpSessionAffinityKeyCount: 5 },
        loadState: 'ready',
        error: null,
        saving: false,
        onApply: () => {},
      }),
    )

    expect(markup).toContain(strings.title)
    expect(markup).toContain(strings.form.currentValue.replace('{count}', '5'))
    expect(markup).toContain(strings.form.applyScopeHint)
  })

  it('renders the saving state copy when apply is in progress', () => {
    const markup = renderToStaticMarkup(
      createElement(SystemSettingsModule, {
        strings,
        settings: { mcpSessionAffinityKeyCount: 5 },
        loadState: 'ready',
        error: null,
        saving: true,
        onApply: () => {},
      }),
    )

    expect(markup).toContain(strings.actions.applying)
    expect(markup).toContain('icon-spin')
  })
})
