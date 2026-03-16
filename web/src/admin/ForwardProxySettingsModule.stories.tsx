import type { Meta, StoryObj } from '@storybook/react-vite'
import { useEffect, useState } from 'react'

import ForwardProxySettingsModule, {
  type ForwardProxyDialogPreviewState,
  type ForwardProxyValidationEntry,
} from './ForwardProxySettingsModule'
import type { ForwardProxyDialogProgressState } from './forwardProxyDialogProgress'
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
      nodes: [
        {
          displayName: 'Tokyo-A',
          protocol: 'ss',
          ok: true,
          ip: '203.0.113.8',
          location: 'JP / NRT',
          latencyMs: 1135.57,
        },
        {
          displayName: 'Singapore-B',
          protocol: 'vless',
          ok: true,
          ip: '198.51.100.42',
          location: 'SG / SIN',
          latencyMs: 1248.31,
        },
        {
          displayName: 'Frankfurt-C',
          protocol: 'trojan',
          ok: false,
          latencyMs: null,
          location: null,
          ip: null,
          message: 'Bootstrap probe timed out before the node produced a trace response.',
        },
      ],
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
      nodes: [
        {
          displayName: '203.0.113.17:8080',
          protocol: 'http',
          ok: false,
          ip: null,
          location: null,
          latencyMs: null,
          message: 'Proxy timed out during bootstrap probe.',
        },
      ],
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
      nodes: [
        {
          displayName: '198.51.100.8:1080',
          protocol: 'socks5h',
          ok: true,
          ip: '198.51.100.8',
          location: 'US / SJC',
          latencyMs: 242.19,
        },
      ],
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
      nodes: [
        {
          displayName: `Overflow-${item}`,
          protocol: value.slice(0, value.indexOf(':')),
          ok,
          ip: ok ? `198.51.100.${item}` : null,
          location: ok ? `US / LAX` : null,
          latencyMs: ok ? 90 + item * 37.5 : null,
          message: ok ? undefined : `Proxy bootstrap probe failed on edge-${item}.example.com after repeated timeout and TLS handshake retries.`,
        },
      ],
    },
  }
})

const SUBSCRIPTION_VALIDATING_PROGRESS: ForwardProxyDialogProgressState = {
  action: 'validate',
  activeStepKey: 'probe_nodes',
  message: '2/3 · edge-2.example.com:443',
  steps: [
    { key: 'normalize_input', label: '规范化输入', status: 'done', detail: '已完成' },
    { key: 'fetch_subscription', label: '拉取订阅', status: 'done', detail: '已完成' },
    { key: 'probe_nodes', label: '探测节点', status: 'running', detail: '2/3 · edge-2.example.com:443' },
    { key: 'generate_result', label: '生成结果', status: 'pending', detail: null },
  ],
}

const SUBSCRIPTION_ADDING_PROGRESS: ForwardProxyDialogProgressState = {
  action: 'save',
  activeStepKey: 'bootstrap_probe',
  message: '3/8 · tokyo-03.example.com:443',
  steps: [
    { key: 'save_settings', label: '保存配置', status: 'done', detail: '已完成' },
    { key: 'refresh_subscription', label: '刷新订阅', status: 'done', detail: '已完成' },
    { key: 'bootstrap_probe', label: '引导探测节点', status: 'running', detail: '3/8 · tokyo-03.example.com:443' },
    { key: 'refresh_ui', label: '刷新列表与统计', status: 'pending', detail: null },
  ],
}

const MANUAL_VALIDATING_PROGRESS: ForwardProxyDialogProgressState = {
  action: 'validate',
  activeStepKey: 'probe_nodes',
  message: '2/4 · socks5h://198.51.100.8:1080',
  steps: [
    { key: 'parse_input', label: '解析输入', status: 'done', detail: '已完成' },
    { key: 'probe_nodes', label: '探测节点', status: 'running', detail: '2/4 · socks5h://198.51.100.8:1080' },
    { key: 'generate_result', label: '生成结果', status: 'pending', detail: null },
  ],
}

const PROGRESS_FAILURE: ForwardProxyDialogProgressState = {
  action: 'save',
  activeStepKey: 'refresh_subscription',
  message: 'Subscription unavailable: upstream returned 503 after 3 retries.',
  steps: [
    { key: 'save_settings', label: '保存配置', status: 'done', detail: '已完成' },
    {
      key: 'refresh_subscription',
      label: '刷新订阅',
      status: 'error',
      detail: 'Subscription unavailable: upstream returned 503 after 3 retries.',
    },
    { key: 'bootstrap_probe', label: '引导探测节点', status: 'pending', detail: null },
    { key: 'refresh_ui', label: '刷新列表与统计', status: 'pending', detail: null },
  ],
}

interface StoryCanvasProps {
  dialogPreview?: ForwardProxyDialogPreviewState | null
  revalidateProgress?: ForwardProxyDialogProgressState | null
  revalidateError?: string | null
  revalidating?: boolean
}

function wait(ms: number): Promise<void> {
  return new Promise((resolve) => {
    window.setTimeout(resolve, ms)
  })
}

const REVALIDATE_PROGRESS: ForwardProxyDialogProgressState = {
  action: 'revalidate',
  activeStepKey: 'probe_nodes',
  message: '4/11 · Singapore-B',
  steps: [
    { key: 'refresh_subscription', label: '刷新订阅', status: 'done', detail: '2 条订阅已同步' },
    { key: 'probe_nodes', label: '探测节点', status: 'running', detail: '4/11 · Singapore-B' },
    { key: 'refresh_ui', label: '刷新列表与统计', status: 'pending', detail: null },
  ],
}

function StoryCanvas({
  dialogPreview = null,
  revalidateProgress = null,
  revalidateError = null,
  revalidating = false,
}: StoryCanvasProps): JSX.Element {
  const strings = useTranslate().admin.proxySettings
  const [previewOpen, setPreviewOpen] = useState(dialogPreview != null)
  const [storySavedAt, setStorySavedAt] = useState(forwardProxyStorySavedAt)
  const [storyRevalidating, setStoryRevalidating] = useState(revalidating)
  const [storyRevalidateProgress, setStoryRevalidateProgress] =
    useState<ForwardProxyDialogProgressState | null>(revalidateProgress)
  const [storyRevalidateError, setStoryRevalidateError] = useState<string | null>(revalidateError)

  useEffect(() => {
    setPreviewOpen(dialogPreview != null)
  }, [dialogPreview])

  useEffect(() => {
    setStorySavedAt(forwardProxyStorySavedAt)
  }, [])

  useEffect(() => {
    setStoryRevalidating(revalidating)
  }, [revalidating])

  useEffect(() => {
    setStoryRevalidateProgress(revalidateProgress)
  }, [revalidateProgress])

  useEffect(() => {
    setStoryRevalidateError(revalidateError)
  }, [revalidateError])

  async function handleRevalidate(): Promise<void> {
    if (storyRevalidating) {
      return
    }

    setStoryRevalidateError(null)
    setStoryRevalidating(true)
    setStoryRevalidateProgress({
      action: 'revalidate',
      activeStepKey: 'refresh_subscription',
      message: '1/2 · https://subscription.example.com',
      steps: [
        { key: 'refresh_subscription', label: '刷新订阅', status: 'running', detail: '1/2 · https://subscription.example.com' },
        { key: 'probe_nodes', label: '探测节点', status: 'pending', detail: null },
        { key: 'refresh_ui', label: '刷新列表与统计', status: 'pending', detail: null },
      ],
    })
    await wait(650)

    setStoryRevalidateProgress({
      action: 'revalidate',
      activeStepKey: 'probe_nodes',
      message: '4/11 · Singapore-B',
      steps: [
        { key: 'refresh_subscription', label: '刷新订阅', status: 'done', detail: '2 条订阅已同步' },
        { key: 'probe_nodes', label: '探测节点', status: 'running', detail: '4/11 · Singapore-B' },
        { key: 'refresh_ui', label: '刷新列表与统计', status: 'pending', detail: null },
      ],
    })
    await wait(850)

    setStoryRevalidateProgress({
      action: 'revalidate',
      activeStepKey: 'refresh_ui',
      message: '正在刷新列表与统计…',
      steps: [
        { key: 'refresh_subscription', label: '刷新订阅', status: 'done', detail: '2 条订阅已同步' },
        { key: 'probe_nodes', label: '探测节点', status: 'done', detail: '11 个节点已完成探测' },
        { key: 'refresh_ui', label: '刷新列表与统计', status: 'running', detail: '正在同步前端视图' },
      ],
    })
    await wait(550)

    setStorySavedAt(Date.now())
    setStoryRevalidating(false)
    setStoryRevalidateProgress(null)
  }

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
        revalidateError={storyRevalidateError}
        saving={false}
        revalidating={storyRevalidating}
        savedAt={storySavedAt}
        revalidateProgress={storyRevalidateProgress}
        settings={forwardProxyStorySettings}
        stats={forwardProxyStoryStats}
        onPersistDraft={async () => {}}
        onValidateCandidates={async () => []}
        onRefresh={() => {}}
        onRevalidate={handleRevalidate}
        dialogPreview={previewOpen ? dialogPreview : null}
        onDialogPreviewClose={() => setPreviewOpen(false)}
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

export const RevalidateProgressBubble: Story = {
  args: {
    revalidating: true,
    revalidateProgress: REVALIDATE_PROGRESS,
  },
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

export const ManualDialogReadyToImport: Story = {
  args: {
    dialogPreview: {
      kind: 'manual',
      input: MANUAL_MIXED_RESULTS.map((entry) => entry.value).join('\n'),
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

export const SubscriptionValidatingProgress: Story = {
  args: {
    dialogPreview: {
      kind: 'subscription',
      input: LONG_SUBSCRIPTION_URL,
      validating: true,
      progress: SUBSCRIPTION_VALIDATING_PROGRESS,
      results: [],
    },
  },
}

export const SubscriptionAddingProgress: Story = {
  args: {
    dialogPreview: {
      kind: 'subscription',
      input: LONG_SUBSCRIPTION_URL,
      progress: SUBSCRIPTION_ADDING_PROGRESS,
      results: SUBSCRIPTION_SUCCESS_RESULT,
    },
  },
}

export const ManualValidatingProgress: Story = {
  args: {
    dialogPreview: {
      kind: 'manual',
      input: MANUAL_MIXED_RESULTS.map((entry) => entry.value).join('\n'),
      validating: true,
      progress: MANUAL_VALIDATING_PROGRESS,
      results: [],
    },
  },
}

export const ProgressFailure: Story = {
  args: {
    dialogPreview: {
      kind: 'subscription',
      input: LONG_SUBSCRIPTION_URL,
      error: 'Subscription unavailable: upstream returned 503 after 3 retries.',
      progress: PROGRESS_FAILURE,
      results: [],
    },
  },
}
