import { describe, expect, it } from 'bun:test'

import type { ForwardProxyProgressEvent } from '../api'
import { createDialogProgressState, updateDialogProgressState } from './forwardProxyDialogProgress'

const strings = {
  progress: {
    titleValidate: '验证进度',
    titleSave: '添加进度',
    badgeValidate: '验证',
    badgeSave: '添加',
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
})
