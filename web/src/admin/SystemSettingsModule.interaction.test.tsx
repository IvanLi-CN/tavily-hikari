import '../../test/happydom'

import { afterEach, describe, expect, it } from 'bun:test'
import { act, useState } from 'react'
import { createRoot, type Root } from 'react-dom/client'

import type { SystemSettings } from '../api'
import { translations } from '../i18n'
import SystemSettingsModule from './SystemSettingsModule'

const strings = translations.zh.admin.systemSettings

const initialSettings: SystemSettings = {
  requestRateLimit: 100,
  mcpSessionAffinityKeyCount: 5,
  rebalanceMcpEnabled: false,
  rebalanceMcpSessionPercent: 100,
  apiRebalanceEnabled: false,
  apiRebalancePercent: 0,
  userBlockedKeyBaseLimit: 5,
  globalIpLimit: 5,
  trustedProxyCidrs: ['127.0.0.0/8', '::1/128'],
  trustedClientIpHeaders: ['cf-connecting-ip', 'x-forwarded-for'],
}

function setNativeValue<T extends HTMLInputElement | HTMLTextAreaElement>(element: T, value: string): void {
  const descriptor = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(element), 'value')
  descriptor?.set?.call(element, value)
  element.dispatchEvent(new Event('input', { bubbles: true }))
}

async function flushEffects(): Promise<void> {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
}

function installObservedHeadersFetchMock(): () => void {
  const originalFetch = window.fetch.bind(window)

  window.fetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const request =
      input instanceof Request
        ? input
        : new Request(typeof input === 'string' && input.startsWith('/') ? `http://localhost${input}` : input, init)
    const url = new URL(request.url, 'http://localhost')

    if (url.pathname === '/api/settings/client-ip/observed-headers') {
      return new Response(
        JSON.stringify({
          items: [
            {
              id: 1042,
              createdAt: 1_774_693_640,
              remoteAddr: '10.0.0.4',
              clientIp: '203.0.113.7',
              clientIpSource: 'cf-connecting-ip',
              clientIpTrusted: true,
              ipHeaders: [
                { name: 'cf-connecting-ip', value: '203.0.113.7' },
                { name: 'x-forwarded-for', value: '198.51.100.10, 10.0.0.4' },
              ],
            },
          ],
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      )
    }

    return originalFetch(request, init)
  }

  return () => {
    window.fetch = originalFetch
  }
}

async function mountSystemSettingsModule(): Promise<{
  root: Root
  applied: SystemSettings[]
}> {
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

  return { root, applied }
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

  it('keeps trusted client IP changes inside the dialog until apply or cancel', async () => {
    const restoreFetch = installObservedHeadersFetchMock()
    const { root, applied } = await mountSystemSettingsModule()

    await act(async () => {
      Array.from(document.querySelectorAll('button'))
        .find((button) => button.textContent?.includes('配置可信 IP'))
        ?.click()
    })
    await flushEffects()

    expect(document.body.textContent).toContain('可信客户端 IP')
    expect(document.body.textContent).toContain('取消')
    expect(document.body.textContent).toContain('应用')
    expect(document.body.textContent).not.toContain('Close')

    await act(async () => {
      document.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, key: 'Escape' }))
    })
    await flushEffects()
    expect(document.body.textContent).toContain('可信客户端 IP')

    await act(async () => {
      document.querySelector<HTMLElement>('.layer-modal-overlay')?.dispatchEvent(
        new PointerEvent('pointerdown', { bubbles: true }),
      )
    })
    await flushEffects()
    expect(document.body.textContent).toContain('可信客户端 IP')

    const textareas = Array.from(document.querySelectorAll<HTMLTextAreaElement>('textarea'))
    expect(textareas.length).toBeGreaterThanOrEqual(2)

    await act(async () => {
      Array.from(document.querySelectorAll('button'))
        .find((button) => button.textContent?.trim() === 'x-real-ip')
        ?.click()
    })
    await flushEffects()
    expect(applied.length).toBe(0)

    await act(async () => {
      Array.from(document.querySelectorAll('button'))
        .find((button) => button.textContent?.includes('取消'))
        ?.click()
    })
    await flushEffects()
    expect(document.body.textContent).not.toContain('最近请求中的字段值')
    expect(applied.length).toBe(0)

    await act(async () => {
      Array.from(document.querySelectorAll('button'))
        .find((button) => button.textContent?.includes('配置可信 IP'))
        ?.click()
    })
    await flushEffects()

    const reopenedTextareas = Array.from(document.querySelectorAll<HTMLTextAreaElement>('textarea'))
    expect(reopenedTextareas.length).toBeGreaterThanOrEqual(2)
    await act(async () => {
      Array.from(document.querySelectorAll('button'))
        .find((button) => button.textContent?.trim() === 'x-real-ip')
        ?.click()
    })
    await act(async () => {
      Array.from(document.querySelectorAll('button'))
        .find((button) => button.textContent?.includes('应用'))
        ?.click()
    })
    await flushEffects()

    expect(applied.at(-1)?.trustedClientIpHeaders).toEqual(['cf-connecting-ip', 'x-forwarded-for', 'x-real-ip'])
    expect(document.body.textContent).not.toContain('最近请求中的字段值')

    await act(async () => root.unmount())
    restoreFetch()
  })
})
