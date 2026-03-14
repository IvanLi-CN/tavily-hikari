import type { Meta, StoryObj } from '@storybook/react-vite'

import ForwardProxySettingsModule, {
  type ForwardProxyDialogPreviewState,
  type ForwardProxyValidationEntry,
} from './ForwardProxySettingsModule'
import {
  forwardProxyStorySavedAt,
  forwardProxyStorySettings,
  forwardProxyStoryStats,
} from './forwardProxyStoryData'
import { LanguageProvider, useTranslate } from '../i18n'

const LONG_SUBSCRIPTION_URL =
  'https://subscription.example.com/api/v1/client/subscribe?token=demo_1234567890abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ&format=raw'

const SUBSCRIPTION_SUCCESS_RESULT: ForwardProxyValidationEntry[] = [
  {
    id: 'subscription-success',
    kind: 'subscriptionUrl',
    value: LONG_SUBSCRIPTION_URL,
    result: {
      ok: true,
      message: 'subscription validation succeeded',
      normalizedValue: LONG_SUBSCRIPTION_URL,
      discoveredNodes: 8,
      latencyMs: 1135.57,
    },
  },
]

const SUBSCRIPTION_FAILURE_RESULT: ForwardProxyValidationEntry[] = [
  {
    id: 'subscription-failure',
    kind: 'subscriptionUrl',
    value: LONG_SUBSCRIPTION_URL,
    result: {
      ok: false,
      message: 'Subscription unavailable: upstream returned 503 after 3 retries.',
      normalizedValue: LONG_SUBSCRIPTION_URL,
      discoveredNodes: 0,
      latencyMs: 1840.12,
      errorCode: 'subscription_unreachable',
    },
  },
]

const MANUAL_MIXED_RESULTS: ForwardProxyValidationEntry[] = [
  {
    id: 'manual-ok-1',
    kind: 'proxyUrl',
    value: 'ss://YWVzLTI1Ni1nY206cGFzc3dvcmQ@example.com:443#Tokyo-A',
    result: {
      ok: true,
      message: 'proxy validation succeeded',
      normalizedValue: 'ss://YWVzLTI1Ni1nY206cGFzc3dvcmQ@example.com:443#Tokyo-A',
      latencyMs: 128.45,
    },
  },
  {
    id: 'manual-bad-1',
    kind: 'proxyUrl',
    value: 'http://203.0.113.17:8080',
    result: {
      ok: false,
      message: 'Proxy timed out during bootstrap probe.',
      normalizedValue: 'http://203.0.113.17:8080',
      latencyMs: 2100,
      errorCode: 'proxy_timeout',
    },
  },
  {
    id: 'manual-ok-2',
    kind: 'proxyUrl',
    value: 'socks5h://198.51.100.8:1080',
    result: {
      ok: true,
      message: 'proxy validation succeeded',
      normalizedValue: 'socks5h://198.51.100.8:1080',
      latencyMs: 242.19,
    },
  },
]

const MANUAL_OVERFLOW_RESULTS: ForwardProxyValidationEntry[] = Array.from({ length: 14 }, (_, index) => {
  const item = index + 1
  const value =
    item % 4 === 0
      ? `trojan://demo-password-${item}@edge-${item}.example.com:443?security=tls&type=ws#Overflow-${item}`
      : item % 3 === 0
        ? `socks5h://198.51.100.${item}:1080`
        : `ss://YWVzLTI1Ni1nY206cGFzc3dvcmQ${item}@edge-${item}.example.com:443#Overflow-${item}`

  const ok = item % 5 !== 0

  return {
    id: `manual-overflow-${item}`,
    kind: 'proxyUrl',
    value,
    result: {
      ok,
      message: ok
        ? `proxy validation succeeded via edge-${item}.example.com after a longer bootstrap handshake to simulate a tall scrollable result list`
        : `Proxy bootstrap probe failed on edge-${item}.example.com after repeated timeout and TLS handshake retries.`,
      normalizedValue: value,
      latencyMs: 90 + item * 37.5,
      errorCode: ok ? undefined : 'proxy_timeout',
    },
  }
})

interface StoryCanvasProps {
  dialogPreview?: ForwardProxyDialogPreviewState | null
}

function StoryCanvas({ dialogPreview = null }: StoryCanvasProps): JSX.Element {
  const strings = useTranslate().admin.proxySettings

  return (
    <div
      style={{
        minHeight: '100vh',
        padding: 24,
        color: 'hsl(var(--foreground))',
        background: [
          'radial-gradient(1000px 520px at 6% -8%, hsl(var(--primary) / 0.14), transparent 62%)',
          'radial-gradient(900px 460px at 95% -14%, hsl(var(--accent) / 0.12), transparent 64%)',
          'linear-gradient(180deg, hsl(var(--background)) 0%, hsl(var(--background)) 62%, hsl(var(--muted) / 0.58) 100%)',
          'hsl(var(--background))',
        ].join(', '),
      }}
    >
      <ForwardProxySettingsModule
        strings={strings}
        settingsLoadState="ready"
        statsLoadState="ready"
        settingsError={null}
        statsError={null}
        saveError={null}
        saving={false}
        savedAt={forwardProxyStorySavedAt}
        settings={forwardProxyStorySettings}
        stats={forwardProxyStoryStats}
        onPersistDraft={async () => {}}
        onValidateCandidates={async () => []}
        onRefresh={() => {}}
        dialogPreview={dialogPreview}
      />
    </div>
  )
}

const meta = {
  title: 'Admin/ForwardProxySettingsModule',
  component: StoryCanvas,
  parameters: {
    layout: 'fullscreen',
  },
  decorators: [
    (Story) => (
      <LanguageProvider>
        <Story />
      </LanguageProvider>
    ),
  ],
} satisfies Meta<typeof StoryCanvas>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {
  args: {},
}

export const SubscriptionDialogEmpty: Story = {
  args: {
    dialogPreview: {
      kind: 'subscription',
      input: LONG_SUBSCRIPTION_URL,
      results: [],
    },
  },
}

export const SubscriptionValidationSuccess: Story = {
  args: {
    dialogPreview: {
      kind: 'subscription',
      input: LONG_SUBSCRIPTION_URL,
      results: SUBSCRIPTION_SUCCESS_RESULT,
    },
  },
}

export const SubscriptionValidationFailure: Story = {
  args: {
    dialogPreview: {
      kind: 'subscription',
      input: LONG_SUBSCRIPTION_URL,
      results: SUBSCRIPTION_FAILURE_RESULT,
    },
  },
}

export const ManualValidationMixed: Story = {
  args: {
    dialogPreview: {
      kind: 'manual',
      input: MANUAL_MIXED_RESULTS.map((entry) => entry.value).join('\n'),
      results: MANUAL_MIXED_RESULTS,
    },
  },
}

export const ManualValidationOverflow: Story = {
  args: {
    dialogPreview: {
      kind: 'manual',
      input: MANUAL_OVERFLOW_RESULTS.map((entry) => entry.value).join('\n'),
      results: MANUAL_OVERFLOW_RESULTS,
    },
  },
}
