import { describe, expect, it } from 'bun:test'

import type { ForwardProxyProgressEvent } from '../api'
import { createDialogProgressState, updateDialogProgressState } from './forwardProxyDialogProgress'

const strings = {
  progress: {
    titleValidate: '验证进度',
    titleSave: '添加进度',
    titleRevalidate: '全量验证进度',
    badgeValidate: '验证',
    badgeSave: '添加',
    badgeRevalidate: '全量验证',
    buttonValidatingSubscription: '正在验证订阅…',
    buttonValidatingManual: '正在验证节点…',
    buttonAddingSubscription: '正在添加订阅…',
    buttonAddingManual: '正在导入节点…',
    running: '进行中…',
    waiting: '等待中…',
    done: '已完成',
    failed: '失败',
    stepCounter: '{current}/{total}',
    steps: {
      save_settings: '保存配置',
      refresh_subscription: '刷新订阅',
      bootstrap_probe: '引导探测节点',
      normalize_input: '规范化输入',
      parse_input: '解析输入',
      fetch_subscription: '拉取订阅',
      probe_nodes: '探测节点',
      generate_result: '生成结果',
      refresh_ui: '刷新列表与统计',
    },
  },
} as any

describe('ForwardProxySettingsModule progress state helpers', () => {
  it('advances manual validation from parsing to aggregated probe progress', () => {
    let state = createDialogProgressState(strings.progress, 'manual', 'validate')
    state = updateDialogProgressState(state, strings.progress, {
      type: 'phase',
      operation: 'validate',
      phaseKey: 'parse_input',
      label: 'Parse input',
    } satisfies ForwardProxyProgressEvent)
    state = updateDialogProgressState(state, strings.progress, {
      type: 'phase',
      operation: 'validate',
      phaseKey: 'probe_nodes',
      label: 'Probe nodes',
      current: 2,
      total: 4,
      detail: 'socks5h://198.51.100.8:1080',
    } satisfies ForwardProxyProgressEvent)

    expect(state.steps.find((step) => step.key === 'parse_input')?.status).toBe('done')
    expect(state.steps.find((step) => step.key === 'probe_nodes')).toMatchObject({
      status: 'running',
      detail: '2/4 · socks5h://198.51.100.8:1080',
    })
  })

  it('marks the active save step as failed when an error arrives', () => {
    let state = createDialogProgressState(strings.progress, 'subscription', 'save')
    state = updateDialogProgressState(state, strings.progress, {
      type: 'phase',
      operation: 'save',
      phaseKey: 'refresh_subscription',
      label: 'Refresh subscription',
      current: 1,
      total: 1,
      detail: 'https://example.com/subscription',
    } satisfies ForwardProxyProgressEvent)
    state = updateDialogProgressState(state, strings.progress, {
      type: 'error',
      operation: 'save',
      message: 'Subscription unavailable',
    } satisfies ForwardProxyProgressEvent)

    expect(state.steps.find((step) => step.key === 'refresh_subscription')).toMatchObject({
      status: 'error',
      detail: 'Subscription unavailable',
    })
    expect(state.message).toBe('Subscription unavailable')
  })

  it('does not overwrite a failure banner when a late complete event arrives', () => {
    let state = createDialogProgressState(strings.progress, 'subscription', 'validate')
    state = updateDialogProgressState(state, strings.progress, {
      type: 'phase',
      operation: 'validate',
      phaseKey: 'fetch_subscription',
      label: 'Fetch subscription',
    } satisfies ForwardProxyProgressEvent)
    state = updateDialogProgressState(state, strings.progress, {
      type: 'error',
      operation: 'validate',
      message: 'Validation failed',
    } satisfies ForwardProxyProgressEvent)
    state = updateDialogProgressState(state, strings.progress, {
      type: 'complete',
      operation: 'validate',
      payload: null,
    } satisfies ForwardProxyProgressEvent)

    expect(state.steps.find((step) => step.key === 'fetch_subscription')?.status).toBe('error')
    expect(state.message).toBe('Validation failed')
  })

  it('ignores subscription node streaming events for the step bubble state', () => {
    let state = createDialogProgressState(strings.progress, 'subscription', 'validate')
    state = updateDialogProgressState(state, strings.progress, {
      type: 'nodes',
      operation: 'validate',
      nodes: [
        {
          nodeKey: 'edge-a',
          displayName: 'edge-a',
          protocol: 'ss',
          status: 'pending',
        },
      ],
    } satisfies ForwardProxyProgressEvent)
    state = updateDialogProgressState(state, strings.progress, {
      type: 'node',
      operation: 'validate',
      node: {
        nodeKey: 'edge-a',
        displayName: 'edge-a',
        protocol: 'ss',
        status: 'probing',
      },
    } satisfies ForwardProxyProgressEvent)

    expect(state.activeStepKey).toBeNull()
    expect(state.message).toBeNull()
    expect(state.steps.every((step) => step.status === 'pending')).toBe(true)
  })

  it('builds revalidate progress with refresh, probe, and ui steps', () => {
    let state = createDialogProgressState(strings.progress, 'subscription', 'revalidate')
    state = updateDialogProgressState(state, strings.progress, {
      type: 'phase',
      operation: 'revalidate',
      phaseKey: 'refresh_subscription',
      label: 'Refresh subscription',
      current: 1,
      total: 2,
      detail: 'https://example.com/subscription',
    } satisfies ForwardProxyProgressEvent)

    expect(state.steps.map((step) => step.key)).toEqual([
      'refresh_subscription',
      'probe_nodes',
      'refresh_ui',
    ])
    expect(state.steps.find((step) => step.key === 'refresh_subscription')).toMatchObject({
      status: 'running',
      detail: '1/2 · https://example.com/subscription',
    })
  })
})
