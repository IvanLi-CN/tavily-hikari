import { describe, expect, it } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'

import ConnectivityChecksPanel, { type ProbeButtonModel, type ProbeStepStatus } from './ConnectivityChecksPanel'
import { TooltipProvider } from './ui/tooltip'

const stepStatusText: Record<ProbeStepStatus, string> = {
  running: '进行中',
  success: '成功',
  failed: '失败',
  blocked: '受阻',
  skipped: '已跳过',
}

const idleProbe: ProbeButtonModel = {
  state: 'idle',
  completed: 0,
  total: 0,
}

describe('ConnectivityChecksPanel', () => {
  it('renders MCP tool-call rows with a separate monospace tool chip', () => {
    const html = renderToStaticMarkup(
      <TooltipProvider>
        <ConnectivityChecksPanel
          title="连通性检测"
          costHint="Runs probe checks."
          costHintAria="Probe cost hint"
          stepStatusText={stepStatusText}
          mcpButtonLabel="检测 MCP"
          apiButtonLabel="检测 API"
          mcpProbe={idleProbe}
          apiProbe={idleProbe}
          probeBubble={{
            visible: true,
            anchor: 'mcp',
            items: [{
              id: 'mcp-tool-call:tavily_search',
              label: '调用 tavily_search 工具',
              status: 'success',
              detail: 'mock upstream replied in 42ms',
            }],
          }}
        />
      </TooltipProvider>,
    )

    expect(html).toContain('user-console-probe-bubble-item-label-structured')
    expect(html).toContain('<span class="user-console-probe-bubble-item-label-text">调用</span>')
    expect(html).toContain('<code class="user-console-probe-bubble-item-tool">tavily_search</code>')
    expect(html).toContain('<span class="user-console-probe-bubble-item-label-text">工具</span>')
    expect(html).toContain('mock upstream replied in 42ms')
  })

  it('falls back to the plain label when the rendered copy cannot be split around the tool name', () => {
    const html = renderToStaticMarkup(
      <TooltipProvider>
        <ConnectivityChecksPanel
          title="连通性检测"
          costHint="Runs probe checks."
          costHintAria="Probe cost hint"
          stepStatusText={stepStatusText}
          mcpButtonLabel="检测 MCP"
          apiButtonLabel="检测 API"
          mcpProbe={idleProbe}
          apiProbe={idleProbe}
          probeBubble={{
            visible: true,
            anchor: 'mcp',
            items: [{
              id: 'mcp-tool-call:tavily_search',
              label: '自定义调用文案',
              status: 'success',
            }],
          }}
        />
      </TooltipProvider>,
    )

    expect(html).not.toContain('user-console-probe-bubble-item-label-structured')
    expect(html).toContain('<strong class="user-console-probe-bubble-item-label">自定义调用文案</strong>')
  })
})
