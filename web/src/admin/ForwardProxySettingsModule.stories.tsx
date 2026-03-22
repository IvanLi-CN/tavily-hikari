import type { Meta, StoryObj } from '@storybook/react-vite'
import { useEffect, useRef, useState } from 'react'

import ForwardProxySettingsModule, {
  ForwardProxyCandidateDialog,
  type ForwardProxyDialogPreviewState,
  type ForwardProxyValidationEntry,
} from './ForwardProxySettingsModule'
import ForwardProxyProgressBubble from './ForwardProxyProgressBubble'
import type { ForwardProxyDialogProgressState } from './forwardProxyDialogProgress'
import type { ForwardProxySettings } from '../api'
import { Dialog, DialogContent } from '../components/ui/dialog'
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

const EGRESS_ENABLING_PROGRESS: ForwardProxyDialogProgressState = {
  action: 'save',
  activeStepKey: 'apply_egress_socks5',
  message: '正在切换新的全局 relay 出口…',
  steps: [
    { key: 'validate_egress_socks5', label: '校验 SOCKS5 relay', status: 'done', detail: '198.51.100.8:1080 可连通' },
    { key: 'save_settings', label: '保存配置', status: 'done', detail: '已完成' },
    { key: 'apply_egress_socks5', label: '切换 relay 出口', status: 'running', detail: '新请求会在完成后切换' },
    { key: 'refresh_subscription', label: '刷新订阅', status: 'pending', detail: null },
    { key: 'bootstrap_probe', label: '引导探测节点', status: 'pending', detail: null },
    { key: 'refresh_ui', label: '刷新列表与统计', status: 'pending', detail: null },
  ],
}

const EGRESS_ENABLE_FAILURE_PROGRESS: ForwardProxyDialogProgressState = {
  action: 'save',
  activeStepKey: 'validate_egress_socks5',
  message: 'SOCKS5 relay 握手失败：connection reset by peer',
  steps: [
    {
      key: 'validate_egress_socks5',
      label: '校验 SOCKS5 relay',
      status: 'error',
      detail: 'SOCKS5 relay 握手失败：connection reset by peer',
    },
    { key: 'save_settings', label: '保存配置', status: 'pending', detail: null },
    { key: 'apply_egress_socks5', label: '切换 relay 出口', status: 'pending', detail: null },
    { key: 'refresh_subscription', label: '刷新订阅', status: 'pending', detail: null },
    { key: 'bootstrap_probe', label: '引导探测节点', status: 'pending', detail: null },
    { key: 'refresh_ui', label: '刷新列表与统计', status: 'pending', detail: null },
  ],
}

const EGRESS_ENABLED_SETTINGS: ForwardProxySettings = {
  ...forwardProxyStorySettings,
  egressSocks5Enabled: true,
  egressSocks5Url: 'socks5h://relay.example.com:1080',
}

interface StoryCanvasProps {
  dialogPreview?: ForwardProxyDialogPreviewState | null
  egressPreviewProgress?: ForwardProxyDialogProgressState | null
  revalidateProgress?: ForwardProxyDialogProgressState | null
  revalidateError?: string | null
  revalidating?: boolean
  settings?: ForwardProxySettings
  saveError?: string | null
  saving?: boolean
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
  egressPreviewProgress = null,
  revalidateProgress = null,
  revalidateError = null,
  revalidating = false,
  settings = forwardProxyStorySettings,
  saveError = null,
  saving = false,
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
        saveError={saveError}
        revalidateError={storyRevalidateError}
        saving={saving}
        revalidating={storyRevalidating}
        savedAt={storySavedAt}
        revalidateProgress={storyRevalidateProgress}
        egressPreviewProgress={egressPreviewProgress}
        settings={settings}
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

function RevalidateProgressBubbleProof(): JSX.Element {
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
      <div
        style={{
          display: 'grid',
          gap: 20,
          maxWidth: 980,
          margin: '0 auto',
        }}
      >
        <section className="surface panel">
          <div className="panel-header">
            <div>
              <h2>Revalidate progress bubble proof</h2>
              <p className="panel-description">
                This state uses an inline progress card rather than a floating overlay. The progress details must stay obvious above
                the stats grid during a subscription revalidate run.
              </p>
            </div>
          </div>
          <div
            style={{
              display: 'grid',
              gap: 18,
              overflow: 'hidden',
              borderRadius: 28,
              border: '1px dashed hsl(var(--accent) / 0.42)',
              background: 'linear-gradient(180deg, hsl(var(--card) / 0.98), hsl(var(--muted) / 0.3))',
              padding: 18,
            }}
          >
            <div
              style={{
                display: 'flex',
                flexWrap: 'wrap',
                alignItems: 'center',
                justifyContent: 'space-between',
                gap: 12,
              }}
            >
              <div style={{ display: 'grid', gap: 4 }}>
                <strong style={{ fontSize: '1rem', color: 'hsl(var(--foreground))' }}>Node pool & live stats</strong>
                <span style={{ fontSize: '0.92rem', color: 'hsl(var(--muted-foreground))' }}>
                  Validate subscriptions now
                </span>
              </div>
              <div style={{ display: 'flex', gap: 10 }}>
                <button type="button" className="btn btn-outline btn-sm" disabled>
                  Refresh
                </button>
                <button type="button" className="btn btn-outline btn-sm" disabled>
                  Revalidating subscriptions
                </button>
              </div>
            </div>

            <ForwardProxyProgressBubble strings={strings} progress={REVALIDATE_PROGRESS} />

            <div
              style={{
                display: 'grid',
                gap: 12,
                opacity: 0.56,
              }}
            >
              <div
                style={{
                  height: 12,
                  width: '32%',
                  borderRadius: 999,
                  background: 'hsl(var(--muted) / 0.7)',
                }}
              />
              <div
                style={{
                  height: 148,
                  borderRadius: 22,
                  border: '1px solid hsl(var(--border) / 0.7)',
                  background:
                    'linear-gradient(180deg, hsl(var(--background) / 0.4), hsl(var(--background) / 0.22))',
                }}
              />
            </div>
          </div>
        </section>
      </div>
    </div>
  )
}

function StatusDetailBubbleProof(): JSX.Element {
  const strings = useTranslate().admin.proxySettings
  const rootRef = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    if (typeof window === 'undefined') return

    let timerId: number | null = null
    let frameId: number | null = null
    const openDetailsBubble = () => {
      const trigger = Array.from(rootRef.current?.querySelectorAll<HTMLButtonElement>('button[aria-label]') ?? []).find(
        (button) => button.getAttribute('aria-label') === strings.config.resultDetails,
      )

      if (trigger) {
        trigger.scrollIntoView({
          block: 'center',
          inline: 'nearest',
        })
        frameId = window.requestAnimationFrame(() => {
          trigger.click()
        })
        return
      }

      timerId = window.setTimeout(openDetailsBubble, 50)
    }

    openDetailsBubble()

    return () => {
      if (timerId != null) {
        window.clearTimeout(timerId)
      }
      if (frameId != null) {
        window.cancelAnimationFrame(frameId)
      }
    }
  }, [strings.config.resultDetails])

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
      <div
        style={{
          display: 'grid',
          gap: 20,
          maxWidth: 980,
          margin: '0 auto',
        }}
      >
        <section className="surface panel">
          <div className="panel-header">
            <div>
              <h2>Status detail bubble proof</h2>
              <p className="panel-description">
                This proof auto-opens the failed-row detail bubble. The floating panel must stay visible above the clipped shell
                instead of being cut off by the validation dialog container.
              </p>
            </div>
          </div>
          <div
            style={{
              overflow: 'hidden',
              borderRadius: 28,
              border: '1px dashed hsl(var(--accent) / 0.42)',
              background: 'linear-gradient(180deg, hsl(var(--card) / 0.98), hsl(var(--muted) / 0.3))',
              padding: 18,
            }}
          >
            <div style={{ minHeight: 420 }}>
              <Dialog open>
                <DialogContent
                  className="dark max-w-3xl overflow-hidden border-border/75 bg-background/94 p-0 shadow-[0_30px_70px_-42px_rgba(15,23,42,0.82)]"
                >
                  <div
                    ref={rootRef}
                    style={{
                      display: 'flex',
                      maxHeight: 520,
                      flexDirection: 'column',
                      overflow: 'hidden',
                    }}
                  >
                    <ForwardProxyCandidateDialog
                      strings={strings}
                      previewMode
                      dialogIsSubscription={false}
                      dialogInput={MANUAL_MIXED_RESULTS.map((entry) => entry.value).join('\n')}
                      dialogError={null}
                      dialogValidating={false}
                      dialogSaving={false}
                      dialogResults={MANUAL_MIXED_RESULTS}
                      liveRows={[]}
                      canAddSubscription={false}
                      canAddManualBatch
                      addManualBatchLabel={strings.config.add}
                      saving={false}
                      progress={null}
                      onClose={() => {}}
                      onCancelValidate={() => {}}
                      onInputChange={() => {}}
                      onValidate={() => {}}
                      onAddSubscription={() => {}}
                      onAddManualBatch={() => {}}
                      onAddManualEntry={() => {}}
                    />
                  </div>
                </DialogContent>
              </Dialog>
            </div>
          </div>
        </section>
      </div>
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

export const GlobalSocks5DisabledEditable: Story = {
  args: {
    settings: forwardProxyStorySettings,
  },
}

export const GlobalSocks5EnablingProgress: Story = {
  args: {
    settings: {
      ...forwardProxyStorySettings,
      egressSocks5Url: 'socks5h://relay.example.com:1080',
    },
    saving: true,
    egressPreviewProgress: EGRESS_ENABLING_PROGRESS,
  },
}

export const GlobalSocks5EnabledLocked: Story = {
  args: {
    settings: EGRESS_ENABLED_SETTINGS,
  },
}

export const GlobalSocks5EnableFailed: Story = {
  args: {
    settings: {
      ...forwardProxyStorySettings,
      egressSocks5Url: 'socks5h://relay.example.com:1080',
    },
    saving: true,
    saveError: 'SOCKS5 relay 握手失败：connection reset by peer',
    egressPreviewProgress: EGRESS_ENABLE_FAILURE_PROGRESS,
  },
}

export const RevalidateProgressBubble: Story = {
  name: 'Revalidate Progress Card',
  render: () => <RevalidateProgressBubbleProof />,
}

export const StatusDetailBubble: Story = {
  name: 'Status Detail Bubble Proof',
  render: () => <StatusDetailBubbleProof />,
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
