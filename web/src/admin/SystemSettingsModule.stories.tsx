import { useLayoutEffect } from 'react'

import type { Meta, StoryObj } from '@storybook/react-vite'

import SystemSettingsModule from './SystemSettingsModule'
import { translations } from '../i18n'

function SystemSettingsCanvas(props: {
  requestRateLimit?: number
  count?: number
  blockedKeyBaseLimit?: number
  rebalanceEnabled?: boolean
  rebalancePercent?: number
  apiRebalanceEnabled?: boolean
  apiRebalancePercent?: number
  loadState?: 'initial_loading' | 'switch_loading' | 'refreshing' | 'ready' | 'error'
  error?: string | null
  saving?: boolean
  helpBubbleOpen?: boolean
}): JSX.Element {
  return (
    <div style={{ maxWidth: 960, margin: '0 auto' }}>
      <SystemSettingsModule
        strings={translations.zh.admin.systemSettings}
        settings={{
          requestRateLimit: props.requestRateLimit ?? 100,
          mcpSessionAffinityKeyCount: props.count ?? 5,
          rebalanceMcpEnabled: props.rebalanceEnabled ?? false,
          rebalanceMcpSessionPercent: props.rebalancePercent ?? 100,
          apiRebalanceEnabled: props.apiRebalanceEnabled ?? false,
          apiRebalancePercent: props.apiRebalancePercent ?? 0,
          userBlockedKeyBaseLimit: props.blockedKeyBaseLimit ?? 5,
          globalIpLimit: 5,
          trustedProxyCidrs: ['127.0.0.0/8', '::1/128'],
          trustedClientIpHeaders: [
            'cf-connecting-ip',
            'true-client-ip',
            'x-real-ip',
            'x-forwarded-for',
            'cf-connecting-ipv6',
            'eo-connecting-ip',
          ],
        }}
        loadState={props.loadState ?? 'ready'}
        error={props.error ?? null}
        saving={props.saving ?? false}
        helpBubbleOpen={props.helpBubbleOpen}
        onApply={() => {}}
      />
    </div>
  )
}

function ObservedClientIpRequestsMock(): null {
  useLayoutEffect(() => {
    const originalFetch = window.fetch.bind(window)

    window.fetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
      const request = input instanceof Request ? input : new Request(input, init)
      const url = new URL(request.url, window.location.origin)

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
              {
                id: 1041,
                createdAt: 1_774_693_580,
                remoteAddr: '10.0.0.4',
                clientIp: '198.51.100.10',
                clientIpSource: 'x-forwarded-for',
                clientIpTrusted: true,
                ipHeaders: [{ name: 'x-forwarded-for', value: '198.51.100.10, 10.0.0.4' }],
              },
              {
                id: 1040,
                createdAt: 1_774_693_520,
                remoteAddr: '127.0.0.1',
                clientIp: '198.51.100.33',
                clientIpSource: 'x-real-ip',
                clientIpTrusted: true,
                ipHeaders: [{ name: 'x-real-ip', value: '198.51.100.33' }],
              },
              {
                id: 1039,
                createdAt: 1_774_693_460,
                remoteAddr: '10.0.0.8',
                clientIp: '2001:db8::42',
                clientIpSource: 'cf-connecting-ipv6',
                clientIpTrusted: true,
                ipHeaders: [{ name: 'cf-connecting-ipv6', value: '2001:db8::42' }],
              },
              {
                id: 1038,
                createdAt: 1_774_693_400,
                remoteAddr: '10.0.0.9',
                clientIp: '203.0.113.40',
                clientIpSource: 'eo-connecting-ip',
                clientIpTrusted: true,
                ipHeaders: [{ name: 'eo-connecting-ip', value: '203.0.113.40' }],
              },
              {
                id: 1037,
                createdAt: 1_774_693_340,
                remoteAddr: '203.0.113.90',
                clientIp: '203.0.113.90',
                clientIpSource: 'remote_addr',
                clientIpTrusted: false,
                ipHeaders: [{ name: 'x-forwarded-for', value: '192.0.2.5' }],
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
  }, [])

  return null
}

const meta = {
  title: 'Admin/SystemSettingsModule',
  component: SystemSettingsModule,
  parameters: {
    layout: 'padded',
    docs: {
      description: {
        component: 'Admin-only MCP session affinity and Rebalance MCP controls with immediate apply feedback.',
      },
    },
  },
  args: {
    strings: translations.zh.admin.systemSettings,
    settings: {
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
    },
    loadState: 'ready',
    error: null,
    saving: false,
    helpBubbleOpen: undefined,
    onApply: () => {},
  },
  render: () => <SystemSettingsCanvas />,
} satisfies Meta<typeof SystemSettingsModule>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {}

export const RebalanceEnabled: Story = {
  render: () => <SystemSettingsCanvas rebalanceEnabled rebalancePercent={35} />,
}

export const RebalanceDisabledSliderLocked: Story = {
  render: () => <SystemSettingsCanvas rebalanceEnabled={false} rebalancePercent={35} />,
}

export const ApiRebalanceEnabled: Story = {
  render: () => <SystemSettingsCanvas apiRebalanceEnabled apiRebalancePercent={25} />,
}

export const ApiRebalanceDisabledSliderLocked: Story = {
  render: () => <SystemSettingsCanvas apiRebalanceEnabled={false} apiRebalancePercent={25} />,
}

export const Applying: Story = {
  render: () => (
    <SystemSettingsCanvas
      rebalanceEnabled
      rebalancePercent={35}
      apiRebalanceEnabled
      apiRebalancePercent={25}
      saving
    />
  ),
}

export const ErrorState: Story = {
  render: () => <SystemSettingsCanvas error="Failed to save system settings." rebalanceEnabled />,
}

export const HelpBubbleOpen: Story = {
  render: () => <SystemSettingsCanvas helpBubbleOpen />,
}

export const RequestRateEdited: Story = {
  render: () => (
    <SystemSettingsCanvas
      requestRateLimit={80}
      rebalanceEnabled
      rebalancePercent={35}
      apiRebalanceEnabled
      apiRebalancePercent={25}
    />
  ),
}

export const BlockedKeyBaseConfigured: Story = {
  render: () => (
    <SystemSettingsCanvas
      blockedKeyBaseLimit={8}
      rebalanceEnabled
      rebalancePercent={35}
      apiRebalanceEnabled
      apiRebalancePercent={25}
    />
  ),
}

export const ClientIpDialogWithObservedValues: Story = {
  render: () => (
    <>
      <ObservedClientIpRequestsMock />
      <SystemSettingsCanvas />
    </>
  ),
  parameters: {
    docs: {
      description: {
        story: '打开可信客户端 IP 设置弹窗，并展示最近请求中观测到的 IP 相关请求头值。',
      },
    },
  },
  play: async ({ canvasElement }) => {
    const button = Array.from(canvasElement.ownerDocument.querySelectorAll('button')).find((candidate) =>
      candidate.textContent?.includes('配置可信 IP'),
    )
    button?.click()
    await new Promise((resolve) => window.setTimeout(resolve, 250))
    const text = canvasElement.ownerDocument.body.textContent ?? ''
    for (const expected of [
      '可信客户端 IP',
      '请求',
      'cf-connecting-ip',
      'true-client-ip',
      'x-real-ip',
      'x-forwarded-for',
      'cf-connecting-ipv6',
      'eo-connecting-ip',
      '203.0.113.7',
      '2001:db8::42',
    ]) {
      if (!text.includes(expected)) {
        throw new Error(`Expected client IP dialog canvas to contain: ${expected}`)
      }
    }
  },
}
