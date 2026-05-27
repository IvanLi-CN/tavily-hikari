import '../../test/happydom'

import { afterEach, describe, expect, it } from 'bun:test'
import { act, useState } from 'react'
import { createRoot } from 'react-dom/client'

import type { SystemSettings } from '../api'
import { translations } from '../i18n'
import type { AdminDisplayDensity } from './displayDensity'
import SystemSettingsModule from './SystemSettingsModule'

const strings = translations.zh.admin.systemSettings

const initialSettings: SystemSettings = {
  requestRateLimit: 100,
  mcpSessionAffinityKeyCount: 5,
  rebalanceMcpEnabled: false,
  rebalanceMcpSessionPercent: 100,
  apiRebalanceEnabled: false,
  apiRebalancePercent: 0,
  rechargeFeatureEnabled: true,
  rechargeUserEnabled: true,
  userBlockedKeyBaseLimit: 5,
  globalIpLimit: 5,
  trustedProxyCidrs: ['127.0.0.0/8', '::1/128'],
  trustedClientIpHeaders: ['cf-connecting-ip', 'x-forwarded-for'],
}

async function flushEffects(): Promise<void> {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
    await new Promise<void>((resolve) => setTimeout(resolve, 0))
  })
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('SystemSettingsModule interactions', () => {
  it('saves switches immediately and keeps the previous value when save fails', async () => {
    const applied: SystemSettings[] = []
    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)

    function Harness(): JSX.Element {
      return (
        <SystemSettingsModule
          strings={strings}
          settings={initialSettings}
          loadState="ready"
          error="保存系统设置失败。"
          saving={false}
          onApply={(nextSettings) => {
            applied.push(nextSettings)
            throw new Error('save failed')
          }}
        />
      )
    }

    await act(async () => {
      root.render(<Harness />)
    })
    await flushEffects()

    const switchButton = document.querySelector<HTMLButtonElement>('#system-settings-rebalance-switch')
    expect(switchButton).not.toBeNull()

    await act(async () => {
      switchButton!.click()
    })
    await flushEffects()

    expect(applied.at(-1)?.rebalanceMcpEnabled).toBe(true)
    expect(switchButton!.getAttribute('aria-checked')).toBe('false')

    await act(async () => root.unmount())
  })

  it('switches the browser-local list density without saving system settings', async () => {
    const applied: SystemSettings[] = []
    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)

    function Harness(): JSX.Element {
      const [displayDensity, setDisplayDensity] = useState<AdminDisplayDensity>('comfortable')
      return (
        <SystemSettingsModule
          strings={strings}
          settings={initialSettings}
          loadState="ready"
          error={null}
          saving={false}
          displayDensity={displayDensity}
          onDisplayDensityChange={setDisplayDensity}
          onApply={(nextSettings) => {
            applied.push(nextSettings)
          }}
        />
      )
    }

    await act(async () => {
      root.render(<Harness />)
    })
    await flushEffects()

    const compactButton = Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find(
      (button) => button.textContent === strings.form.displayDensityCompact,
    )
    expect(compactButton).not.toBeNull()

    await act(async () => {
      compactButton!.click()
    })
    await flushEffects()

    expect(compactButton!.getAttribute('aria-pressed')).toBe('true')
    expect(applied).toHaveLength(0)

    await act(async () => root.unmount())
  })

  it('saves recharge switches immediately', async () => {
    const applied: SystemSettings[] = []
    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)

    function Harness(): JSX.Element {
      const [settings, setSettings] = useState<SystemSettings>(initialSettings)
      return (
        <SystemSettingsModule
          strings={strings}
          settings={settings}
          loadState="ready"
          error={null}
          saving={false}
          onApply={(nextSettings) => {
            applied.push(nextSettings)
            setSettings(nextSettings)
          }}
        />
      )
    }

    await act(async () => {
      root.render(<Harness />)
    })
    await flushEffects()

    const featureSwitch = Array.from(document.querySelectorAll<HTMLButtonElement>('[role="switch"]')).find(
      (button) => button.getAttribute('aria-label') === strings.form.rechargeFeatureLabel,
    )
    const userSwitch = Array.from(document.querySelectorAll<HTMLButtonElement>('[role="switch"]')).find(
      (button) => button.getAttribute('aria-label') === strings.form.rechargeUserLabel,
    )
    expect(featureSwitch).not.toBeNull()
    expect(userSwitch).not.toBeNull()

    await act(async () => {
      featureSwitch!.click()
    })
    await flushEffects()
    await act(async () => {
      userSwitch!.click()
    })
    await flushEffects()

    expect(applied[0]?.rechargeFeatureEnabled).toBe(false)
    expect(applied[1]?.rechargeUserEnabled).toBe(false)

    await act(async () => root.unmount())
  })
})
