import { Description, Stories, Subtitle, Title } from '@storybook/addon-docs/blocks'
import type { Meta, StoryObj } from '@storybook/react-vite'
import type { ReactNode } from 'react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { themes } from 'storybook/theming'

import ForwardProxyEgressControl from './ForwardProxyEgressControl'
import ForwardProxyProgressBubble from './ForwardProxyProgressBubble'
import {
  createDialogProgressState,
  updateDialogProgressState,
  type ForwardProxyDialogProgressState,
} from './forwardProxyDialogProgress'
import { LanguageProvider, useTranslate } from '../i18n'

interface StoryArgs {
  enabled: boolean
  url: string
  loading: boolean
  controlsDisabled: boolean
  inputLocked: boolean
  errorMessage?: string | null
  errorPresentation?: 'hint' | 'alert'
  progress: ForwardProxyDialogProgressState | null
}

interface ScenarioState extends StoryArgs {}

function validateStoryEgressUrl(
  strings: ReturnType<typeof useTranslate>['admin']['proxySettings'],
  value: string,
): string | null {
  const trimmed = value.trim()
  if (trimmed.length === 0) return null

  let parsed: URL
  try {
    parsed = new URL(trimmed)
  } catch {
    return strings.config.egressInvalidUrlError
  }

  const scheme = parsed.protocol.replace(/:$/, '').toLowerCase()
  if ((scheme !== 'socks5' && scheme !== 'socks5h') || !parsed.hostname || !parsed.port) {
    return strings.config.egressInvalidUrlError
  }

  return null
}

function buildRunningProgress(strings: ReturnType<typeof useTranslate>['admin']['proxySettings']): ForwardProxyDialogProgressState {
  const state = createDialogProgressState(strings.progress, 'egress', 'save', {
    includeEgressValidation: true,
  })

  return {
    ...state,
    activeStepKey: 'validate_egress_socks5',
    message: '正在检测 SOCKS5 出口代理可用性…',
    steps: state.steps.map((step, index) =>
      index === 0
        ? {
            ...step,
            status: 'running',
            detail: '第 1 / 6 步 · 正在检测连通性',
          }
        : step,
    ),
  }
}

function buildFailedProgress(strings: ReturnType<typeof useTranslate>['admin']['proxySettings']): ForwardProxyDialogProgressState {
  const state = createDialogProgressState(strings.progress, 'egress', 'save', {
    includeEgressValidation: true,
  })

  return {
    ...state,
    activeStepKey: 'validate_egress_socks5',
    message: strings.config.egressInvalidUrlError,
    steps: state.steps.map((step, index) =>
      index === 0
        ? {
            ...step,
            status: 'error',
            detail: '地址格式不合法',
          }
        : step,
    ),
  }
}

function buildEditableScenario(): ScenarioState {
  return {
    enabled: false,
    url: 'socks5h://user:pass@127.0.0.1:1080',
    loading: false,
    controlsDisabled: false,
    inputLocked: false,
    errorMessage: null,
    errorPresentation: 'hint',
    progress: null,
  }
}

function buildEmptyScenario(): ScenarioState {
  return {
    enabled: false,
    url: '',
    loading: false,
    controlsDisabled: false,
    inputLocked: false,
    errorMessage: null,
    errorPresentation: 'hint',
    progress: null,
  }
}

function buildRequiredScenario(strings: ReturnType<typeof useTranslate>['admin']['proxySettings']): ScenarioState {
  return {
    ...buildEmptyScenario(),
    errorMessage: strings.config.egressRequiredError,
    errorPresentation: 'hint',
  }
}

function buildBlurInvalidScenario(strings: ReturnType<typeof useTranslate>['admin']['proxySettings']): ScenarioState {
  return {
    enabled: false,
    url: 'socks5h://user:pass@127',
    loading: false,
    controlsDisabled: false,
    inputLocked: false,
    errorMessage: strings.config.egressInvalidUrlError,
    errorPresentation: 'alert',
    progress: null,
  }
}

function buildSavingScenario(strings: ReturnType<typeof useTranslate>['admin']['proxySettings']): ScenarioState {
  return {
    enabled: true,
    url: 'socks5h://user:pass@127.0.0.1:1080',
    loading: true,
    controlsDisabled: true,
    inputLocked: false,
    errorMessage: null,
    errorPresentation: 'hint',
    progress: buildRunningProgress(strings),
  }
}

function buildEnabledLockedScenario(): ScenarioState {
  return {
    enabled: true,
    url: 'socks5h://user:pass@127.0.0.1:1080',
    loading: false,
    controlsDisabled: false,
    inputLocked: true,
    errorMessage: null,
    errorPresentation: 'hint',
    progress: null,
  }
}

function buildFailedScenario(strings: ReturnType<typeof useTranslate>['admin']['proxySettings']): ScenarioState {
  return {
    enabled: false,
    url: 'not-a-socks5-url',
    loading: false,
    controlsDisabled: false,
    inputLocked: false,
    errorMessage: strings.config.egressInvalidUrlError,
    errorPresentation: 'alert',
    progress: buildFailedProgress(strings),
  }
}

function buildCompletedProgress(strings: ReturnType<typeof useTranslate>['admin']['proxySettings']): ForwardProxyDialogProgressState {
  let state = createDialogProgressState(strings.progress, 'egress', 'save', {
    includeEgressValidation: true,
  })

  const phases = [
    { key: 'validate_egress_socks5', detail: '第 1 / 6 步 · 已完成连通性检测' },
    { key: 'save_settings', detail: '第 2 / 6 步 · 已保存配置' },
    { key: 'apply_egress_socks5', detail: '第 3 / 6 步 · 已切换出口代理' },
    { key: 'refresh_subscription', detail: '第 4 / 6 步 · 已刷新订阅节点' },
    { key: 'bootstrap_probe', detail: '第 5 / 6 步 · 已完成引导探测' },
    { key: 'refresh_ui', detail: '第 6 / 6 步 · 列表与统计已刷新' },
  ] as const

  for (const phase of phases) {
    state = updateDialogProgressState(state, strings.progress, {
      type: 'phase',
      operation: 'save',
      phaseKey: phase.key,
      label: strings.progress.steps[phase.key],
      detail: phase.detail,
    })
  }

  return updateDialogProgressState(state, strings.progress, {
    type: 'complete',
    operation: 'save',
    payload: null,
  })
}

function buildCompletedScenario(strings: ReturnType<typeof useTranslate>['admin']['proxySettings']): ScenarioState {
  return {
    enabled: true,
    url: 'socks5h://user:pass@127.0.0.1:1080',
    loading: false,
    controlsDisabled: false,
    inputLocked: true,
    errorMessage: null,
    errorPresentation: 'hint',
    progress: buildCompletedProgress(strings),
  }
}

function StorySurface({
  children,
  compact = false,
  wide = false,
}: {
  children: ReactNode
  compact?: boolean
  wide?: boolean
}): JSX.Element {
  return (
    <div className="min-h-[360px] w-full bg-[radial-gradient(circle_at_top,hsl(var(--primary)/0.14),transparent_35%),linear-gradient(180deg,hsl(224_42%_13%),hsl(225_41%_9%))] px-6 py-8 text-foreground">
      <div
        className={`mx-auto rounded-3xl border border-border/70 bg-card/45 shadow-[0_24px_70px_-42px_hsl(var(--foreground)/0.45)] ${
          compact
            ? 'w-[min(46rem,calc(100vw-2rem))] p-6'
            : wide
              ? 'w-[min(84rem,calc(100vw-2rem))] p-8'
              : 'w-[min(52rem,calc(100vw-2rem))] p-6'
        }`}
      >
        {children}
      </div>
    </div>
  )
}

function ChineseStoryFrame({ children }: { children: ReactNode }): JSX.Element {
  return <LanguageProvider initialLanguage="zh">{children}</LanguageProvider>
}

function StaticScenario({
  state,
}: {
  state: ScenarioState
}): JSX.Element {
  const strings = useTranslate().admin.proxySettings

  return (
    <StorySurface compact>
      <ForwardProxyEgressControl
        strings={strings}
        enabled={state.enabled}
        url={state.url}
        loading={state.loading}
        controlsDisabled={state.controlsDisabled}
        inputLocked={state.inputLocked}
        errorMessage={state.errorMessage ?? null}
        errorPresentation={state.errorPresentation ?? 'hint'}
        progress={state.progress}
        onToggle={() => undefined}
        onUrlChange={() => undefined}
      />
    </StorySurface>
  )
}

function InteractiveControlPanel(): JSX.Element {
  const strings = useTranslate().admin.proxySettings
  const [state, setState] = useState<ScenarioState>(buildEditableScenario())
  const timerRefs = useRef<number[]>([])

  useEffect(() => {
    return () => {
      timerRefs.current.forEach((timer) => window.clearTimeout(timer))
      timerRefs.current = []
    }
  }, [])

  const clearTimers = () => {
    timerRefs.current.forEach((timer) => window.clearTimeout(timer))
    timerRefs.current = []
  }

  const scheduleStoryProgress = (url: string) => {
    clearTimers()

    const phases = [
      { key: 'save_settings', detail: '第 2 / 6 步 · 正在保存配置' },
      { key: 'apply_egress_socks5', detail: '第 3 / 6 步 · 正在切换出口代理' },
      { key: 'refresh_subscription', detail: '第 4 / 6 步 · 正在刷新订阅节点' },
      { key: 'bootstrap_probe', detail: '第 5 / 6 步 · 正在引导探测节点' },
      { key: 'refresh_ui', detail: '第 6 / 6 步 · 正在刷新列表与统计' },
    ] as const

    phases.forEach((phase, index) => {
      const timer = window.setTimeout(() => {
        setState((current) => {
          const progress = current.progress
          if (!progress) return current
          return {
            ...current,
            progress: updateDialogProgressState(progress, strings.progress, {
              type: 'phase',
              operation: 'save',
              phaseKey: phase.key,
              label: strings.progress.steps[phase.key],
              detail: phase.detail,
            }),
          }
        })
      }, (index + 1) * 850)
      timerRefs.current.push(timer)
    })

    const completeTimer = window.setTimeout(() => {
      setState((current) => {
        const progress = current.progress
        if (!progress) return current
        return {
          ...current,
          enabled: true,
          loading: false,
          controlsDisabled: false,
          inputLocked: true,
          url,
          progress: updateDialogProgressState(progress, strings.progress, {
            type: 'complete',
            operation: 'save',
            payload: null,
          }),
        }
      })
    }, (phases.length + 1) * 850)
    timerRefs.current.push(completeTimer)
  }

  const onToggle = (checked: boolean) => {
    clearTimers()
    if (!checked) {
      setState((current) => ({
        ...current,
        enabled: false,
        loading: false,
        controlsDisabled: false,
        inputLocked: false,
        errorMessage: null,
        errorPresentation: 'hint',
        progress: null,
      }))
      return
    }

    const trimmed = state.url.trim()
    if (trimmed.length === 0) {
      setState((current) => ({
        ...current,
        enabled: false,
        errorMessage: strings.config.egressRequiredError,
        errorPresentation: 'hint',
        progress: null,
      }))
      return
    }

    const validationError = validateStoryEgressUrl(strings, trimmed)
    if (validationError) {
      setState((current) => ({
        ...current,
        enabled: false,
        loading: false,
        controlsDisabled: false,
        inputLocked: false,
        errorMessage: validationError,
        errorPresentation: 'alert',
        progress: null,
      }))
      return
    }

    const runningScenario = buildSavingScenario(strings)
    setState((current) => ({
      ...current,
      enabled: true,
      loading: runningScenario.loading,
      controlsDisabled: runningScenario.controlsDisabled,
      inputLocked: false,
      errorMessage: null,
      errorPresentation: 'hint',
      progress: runningScenario.progress,
    }))
    scheduleStoryProgress(trimmed)
  }

  const onUrlChange = (value: string) => {
    setState((current) => ({
      ...current,
      url: value,
      errorMessage: null,
      errorPresentation: 'hint',
      progress: null,
      loading: false,
      controlsDisabled: false,
      enabled: current.inputLocked ? current.enabled : false,
    }))
  }

  const onRequireUrl = () => {
    setState((current) => ({
      ...current,
      errorMessage: strings.config.egressRequiredError,
      errorPresentation: 'hint',
      progress: null,
    }))
  }

  return (
    <ForwardProxyEgressControl
      strings={strings}
      enabled={state.enabled}
      url={state.url}
      loading={state.loading}
      controlsDisabled={state.controlsDisabled}
      inputLocked={state.inputLocked}
      errorMessage={state.errorMessage ?? null}
      errorPresentation={state.errorPresentation ?? 'hint'}
      progress={state.progress}
      onToggle={onToggle}
      onUrlChange={onUrlChange}
      onUrlBlur={() => {
        const validationError = validateStoryEgressUrl(strings, state.url)
        clearTimers()
        setState((current) => ({
          ...current,
          errorMessage: validationError,
          errorPresentation: validationError ? 'alert' : 'hint',
          progress: validationError ? null : current.progress,
        }))
      }}
      onRequireUrl={onRequireUrl}
    />
  )
}

function InteractiveDefaultStory(): JSX.Element {
  return (
    <StorySurface compact>
      <InteractiveControlPanel />
    </StorySurface>
  )
}

function GalleryStory(): JSX.Element {
  const strings = useTranslate().admin.proxySettings
  const fieldStates = useMemo(
    () => [
      {
        title: '默认可编辑',
        note: '关闭状态，地址可直接录入或修改。',
        state: buildEditableScenario(),
      },
      {
        title: '空值待填写',
        note: '还没输入地址时的初始状态。',
        state: buildEmptyScenario(),
      },
      {
        title: '必填提醒',
        note: '点击开启但未填写地址时，原位给出轻提示。',
        state: buildRequiredScenario(strings),
      },
      {
        title: '失焦格式错误',
        note: '输入框失焦后立即前端校验，不等后端返回。',
        state: buildBlurInvalidScenario(strings),
      },
    ],
    [strings],
  )
  const flowStates = useMemo(
    () => [
      {
        title: '开启中气泡',
        note: '自动出现进度气泡，串行展示 6 个步骤。',
        state: buildSavingScenario(strings),
        previewProgress: buildSavingScenario(strings).progress,
      },
      {
        title: '完成后可回看',
        note: '完成后仍可通过悬浮开关回看最后一次步骤状态。',
        state: buildCompletedScenario(strings),
        previewProgress: buildCompletedScenario(strings).progress,
      },
      {
        title: '后端校验失败',
        note: '后端拒绝非法地址时，字段错误与气泡结果同时可见。',
        state: buildFailedScenario(strings),
        previewProgress: buildFailedScenario(strings).progress,
      },
      {
        title: '已开启锁定',
        note: '启用成功后输入框锁定，需先关闭再编辑。',
        state: buildEnabledLockedScenario(),
        previewProgress: null,
      },
    ],
    [strings],
  )

  const renderScenarioCard = (scenario: {
    title: string
    note: string
    state: ScenarioState
    previewProgress?: ForwardProxyDialogProgressState | null
    layout?: 'field' | 'flow'
  }) => (
    <div key={scenario.title} className="space-y-3">
      <div className="space-y-1">
        <p className="text-sm font-semibold text-foreground">{scenario.title}</p>
        <p className="text-sm text-muted-foreground">{scenario.note}</p>
      </div>
      <div className="rounded-3xl border border-border/70 bg-card/45 p-6 shadow-[0_24px_70px_-42px_hsl(var(--foreground)/0.45)]">
        <div
          className={`grid gap-6 ${
            scenario.layout === 'flow' && scenario.previewProgress
              ? 'xl:grid-cols-[minmax(0,1fr)_22rem] xl:items-start xl:gap-8'
              : ''
          }`}
        >
          <ForwardProxyEgressControl
            strings={strings}
            enabled={scenario.state.enabled}
            url={scenario.state.url}
            loading={scenario.state.loading}
            controlsDisabled={scenario.state.controlsDisabled}
            inputLocked={scenario.state.inputLocked}
            errorMessage={scenario.state.errorMessage ?? null}
            errorPresentation={scenario.state.errorPresentation ?? 'hint'}
            progress={null}
            onToggle={() => undefined}
            onUrlChange={() => undefined}
          />
          {scenario.previewProgress ? (
            <div className="space-y-3 border-t border-border/50 pt-4 xl:border-t-0 xl:border-l xl:border-border/50 xl:pt-0 xl:pl-6">
              <p className="text-xs font-medium uppercase tracking-[0.18em] text-muted-foreground">气泡预览</p>
              <div className="rounded-2xl border border-border/60 bg-background/45 p-3">
                <ForwardProxyProgressBubble strings={strings} progress={scenario.previewProgress} />
              </div>
            </div>
          ) : null}
        </div>
      </div>
    </div>
  )

  return (
    <StorySurface wide>
      <div className="space-y-10">
        <section className="space-y-4">
          <div className="space-y-1">
            <h2 className="text-lg font-semibold text-foreground">字段状态总览</h2>
            <p className="text-sm text-muted-foreground">聚合展示输入框从初始到前端校验失败的全部关键状态。</p>
          </div>
          <div className="grid gap-6 xl:grid-cols-2">
            {fieldStates.map((scenario) => renderScenarioCard({ ...scenario, layout: 'field' }))}
          </div>
        </section>

        <section className="space-y-4">
          <div className="space-y-1">
            <h2 className="text-lg font-semibold text-foreground">流程状态总览</h2>
            <p className="text-sm text-muted-foreground">覆盖开启中、完成回看、后端失败和已开启锁定等完整流程场景。</p>
          </div>
          <div className="grid gap-6">
            {flowStates.map((scenario) => renderScenarioCard({ ...scenario, layout: 'flow' }))}
          </div>
        </section>

        <section className="space-y-4">
          <div className="space-y-1">
            <h2 className="text-lg font-semibold text-foreground">交互验收面板</h2>
            <p className="text-sm text-muted-foreground">
              这里保留可实际操作的示例：开启时气泡自动出现，点击其他区域隐藏，悬浮开关区域可以再次显示。
            </p>
          </div>
          <div className="rounded-3xl border border-border/70 bg-card/45 p-6 shadow-[0_24px_70px_-42px_hsl(var(--foreground)/0.45)]">
            <InteractiveControlPanel />
          </div>
        </section>
      </div>
      <p className="sr-only">{strings.config.egressTitle}</p>
    </StorySurface>
  )
}

function DocsPage(): JSX.Element {
  return (
    <ChineseStoryFrame>
      <div className="sb-unstyled">
        <Title />
        <Subtitle>聚合展示全局 SOCKS5 出口代理控件的字段状态、流程状态与交互验收面板。</Subtitle>
        <Description />
        <GalleryStory />
        <Stories includePrimary={false} title="Standalone Stories" />
      </div>
    </ChineseStoryFrame>
  )
}

const meta = {
  title: 'Admin/ForwardProxyEgressControl',
  component: InteractiveDefaultStory,
  tags: ['autodocs'],
  parameters: {
    layout: 'fullscreen',
    docs: {
      theme: themes.dark,
      page: DocsPage,
      description: {
        component:
          '用于管理全局 SOCKS5 出口代理的紧凑设置控件。Storybook 中所有状态都与真实页面的交互语义保持一致，并提供可直接验收的 Default 交互故事。',
      },
    },
  },
} satisfies Meta<typeof InteractiveDefaultStory>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {
  render: () => (
    <ChineseStoryFrame>
      <InteractiveDefaultStory />
    </ChineseStoryFrame>
  ),
}

export const Editable: Story = {
  render: () => (
    <ChineseStoryFrame>
      <StaticScenario state={buildEditableScenario()} />
    </ChineseStoryFrame>
  ),
}

export const SavingWithBubble: Story = {
  render: () => (
    <ChineseStoryFrame>
      <SavingScenarioWrapper />
    </ChineseStoryFrame>
  ),
  play: async ({ canvasElement }) => {
    await new Promise((resolve) => window.setTimeout(resolve, 50))
    const text = canvasElement.ownerDocument.body.textContent ?? ''
    if (!text.includes('校验 SOCKS5 出口代理')) {
      throw new Error('Expected the anchored progress bubble to render the validation step.')
    }
  },
}

function SavingScenarioWrapper(): JSX.Element {
  return <StaticScenario state={buildSavingScenario(useTranslate().admin.proxySettings)} />
}

function FailedScenarioWrapper(): JSX.Element {
  return <StaticScenario state={buildFailedScenario(useTranslate().admin.proxySettings)} />
}

export const EnabledLocked: Story = {
  render: () => (
    <ChineseStoryFrame>
      <StaticScenario state={buildEnabledLockedScenario()} />
    </ChineseStoryFrame>
  ),
}

export const FailedValidation: Story = {
  render: () => (
    <ChineseStoryFrame>
      <FailedScenarioWrapper />
    </ChineseStoryFrame>
  ),
}

export const StateGallery: Story = {
  parameters: {
    docs: {
      disable: true,
    },
  },
  render: () => (
    <ChineseStoryFrame>
      <GalleryStory />
    </ChineseStoryFrame>
  ),
}
