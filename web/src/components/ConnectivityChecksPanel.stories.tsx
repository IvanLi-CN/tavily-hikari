import type { Meta, StoryObj } from '@storybook/react-vite'

import ConnectivityChecksPanel, {
  type ProbeBubbleModel,
  type ProbeButtonModel,
  type ProbeStepStatus,
} from './ConnectivityChecksPanel'

const stepStatusText: Record<ProbeStepStatus, string> = {
  running: 'Running',
  success: 'Connected',
  failed: 'Failed',
  blocked: 'Blocked',
  skipped: 'Skipped',
}

const idleProbe: ProbeButtonModel = {
  state: 'idle',
  completed: 0,
  total: 0,
}

interface ConnectivityScenario {
  title: string
  description: string
  mcpProbe: ProbeButtonModel
  apiProbe: ProbeButtonModel
  mcpButtonLabel: string
  apiButtonLabel: string
  probeBubble?: ProbeBubbleModel
  anyProbeRunning?: boolean
}

const allMcpToolSweepItems: ProbeBubbleModel['items'] = [
  { id: 'mcp-ping', label: 'MCP service connectivity', status: 'success' },
  { id: 'mcp-tools-list', label: 'MCP tool discovery', status: 'success' },
  { id: 'mcp-tool-call:tavily-search', label: 'MCP tool call · tavily-search', status: 'success' },
  { id: 'mcp-tool-call:tavily-extract', label: 'MCP tool call · tavily-extract', status: 'success' },
  { id: 'mcp-tool-call:tavily-crawl', label: 'MCP tool call · tavily-crawl', status: 'success' },
  { id: 'mcp-tool-call:tavily-map', label: 'MCP tool call · tavily-map', status: 'success' },
  { id: 'mcp-tool-call:tavily-research', label: 'MCP tool call · tavily-research', status: 'success' },
]

const galleryGridStyle = {
  display: 'grid',
  columnGap: 18,
  rowGap: 196,
  gridTemplateColumns: 'repeat(auto-fit, minmax(320px, 1fr))',
  alignItems: 'start',
} as const

const scenarios: ConnectivityScenario[] = [
  {
    title: 'Idle',
    description: 'Fresh token detail before the operator runs any connectivity checks.',
    mcpProbe: idleProbe,
    apiProbe: idleProbe,
    mcpButtonLabel: 'Test MCP',
    apiButtonLabel: 'Test API',
  },
  {
    title: 'API Running',
    description: 'API probe is in-flight and keeps the action group locked until all steps settle.',
    mcpProbe: idleProbe,
    apiProbe: { state: 'running', completed: 2, total: 6 },
    mcpButtonLabel: 'Test MCP',
    apiButtonLabel: 'Testing API (2/6)',
    anyProbeRunning: true,
    probeBubble: {
      visible: true,
      anchor: 'api',
      items: [
        { id: 'api-search', label: 'Search endpoint', status: 'success' },
        { id: 'api-extract', label: 'Extract endpoint', status: 'success' },
        { id: 'api-crawl', label: 'Crawl endpoint', status: 'running' },
      ],
    },
  },
  {
    title: 'MCP Full Sweep',
    description: 'The MCP probe discovers every advertised tool and executes a full tools/call sweep before settling.',
    mcpProbe: { state: 'success', completed: 7, total: 7 },
    apiProbe: { state: 'success', completed: 6, total: 6 },
    mcpButtonLabel: 'MCP Ready',
    apiButtonLabel: 'API Ready',
    probeBubble: {
      visible: true,
      anchor: 'mcp',
      items: allMcpToolSweepItems,
    },
  },
  {
    title: 'MCP Tool Failure',
    description: 'Discovery succeeds, but one advertised MCP tool still fails during the tools/call sweep and the rollup stays partial.',
    mcpProbe: { state: 'partial', completed: 7, total: 7 },
    apiProbe: idleProbe,
    mcpButtonLabel: 'MCP Partial',
    apiButtonLabel: 'Test API',
    probeBubble: {
      visible: true,
      anchor: 'mcp',
      items: [
        { id: 'mcp-ping', label: 'MCP service connectivity', status: 'success' },
        { id: 'mcp-tools-list', label: 'MCP tool discovery', status: 'success' },
        { id: 'mcp-tool-call:tavily-search', label: 'MCP tool call · tavily-search', status: 'success' },
        { id: 'mcp-tool-call:tavily-extract', label: 'MCP tool call · tavily-extract', status: 'success' },
        { id: 'mcp-tool-call:tavily-crawl', label: 'MCP tool call · tavily-crawl', status: 'success' },
        { id: 'mcp-tool-call:tavily-map', label: 'MCP tool call · tavily-map', status: 'failed', detail: '500 timeout from mock upstream' },
        { id: 'mcp-tool-call:tavily-research', label: 'MCP tool call · tavily-research', status: 'success' },
      ],
    },
  },
  {
    title: 'Authentication Failed',
    description: 'The preflight token fetch succeeds, but MCP handshake rejects the user token immediately.',
    mcpProbe: { state: 'failed', completed: 0, total: 7 },
    apiProbe: idleProbe,
    mcpButtonLabel: 'MCP Failed',
    apiButtonLabel: 'Test API',
    probeBubble: {
      visible: true,
      anchor: 'mcp',
      items: [
        { id: 'mcp-ping', label: 'MCP service reachable', status: 'failed', detail: '401 invalid or disabled token' },
      ],
    },
  },
  {
    title: 'Quota Blocked',
    description: 'Quota precheck blocks every billable MCP tool call while still surfacing discovery of the full advertised tool list.',
    mcpProbe: { state: 'partial', completed: 7, total: 7 },
    apiProbe: idleProbe,
    mcpButtonLabel: 'MCP Blocked',
    apiButtonLabel: 'Test API',
    probeBubble: {
      visible: true,
      anchor: 'mcp',
      items: [
        { id: 'mcp-ping', label: 'MCP service connectivity', status: 'success' },
        { id: 'mcp-tools-list', label: 'MCP tool discovery', status: 'success' },
        { id: 'mcp-tool-call:tavily-search', label: 'MCP tool call · tavily-search', status: 'blocked', detail: 'Skipped after quota precheck' },
        { id: 'mcp-tool-call:tavily-extract', label: 'MCP tool call · tavily-extract', status: 'blocked', detail: 'Skipped after quota precheck' },
        { id: 'mcp-tool-call:tavily-crawl', label: 'MCP tool call · tavily-crawl', status: 'blocked', detail: 'Skipped after quota precheck' },
        { id: 'mcp-tool-call:tavily-map', label: 'MCP tool call · tavily-map', status: 'blocked', detail: 'Skipped after quota precheck' },
        { id: 'mcp-tool-call:tavily-research', label: 'MCP tool call · tavily-research', status: 'blocked', detail: 'Skipped after quota precheck' },
      ],
    },
  },
]

function ConnectivityScenarioCard({
  title,
  description,
  mcpProbe,
  apiProbe,
  mcpButtonLabel,
  apiButtonLabel,
  probeBubble,
  anyProbeRunning,
}: ConnectivityScenario): JSX.Element {
  return (
    <article
      style={{
        display: 'grid',
        gap: 18,
        minWidth: 0,
        padding: '20px 22px',
        borderRadius: 24,
        border: '1px solid rgba(148, 163, 184, 0.2)',
        background: 'linear-gradient(180deg, rgba(15, 23, 42, 0.96), rgba(15, 23, 42, 0.88))',
        color: '#e5edf8',
        boxShadow: '0 26px 60px -36px rgba(15, 23, 42, 0.75)',
      }}
    >
      <div style={{ display: 'grid', gap: 6 }}>
        <div
          style={{
            fontSize: '0.76rem',
            fontWeight: 700,
            letterSpacing: '0.08em',
            textTransform: 'uppercase',
            color: 'rgba(191, 219, 254, 0.72)',
          }}
        >
          {title}
        </div>
        <div style={{ fontSize: '0.92rem', lineHeight: 1.55, color: 'rgba(226, 232, 240, 0.82)' }}>
          {description}
        </div>
      </div>
      <div className="dark" style={{ minWidth: 0 }}>
        <ConnectivityChecksPanel
          title="Connectivity Checks"
          costHint="Runs a small MCP handshake and the full Tavily API chain against the mock upstream."
          costHintAria="Connectivity check cost hint"
          stepStatusText={stepStatusText}
          mcpButtonLabel={mcpButtonLabel}
          apiButtonLabel={apiButtonLabel}
          mcpProbe={mcpProbe}
          apiProbe={apiProbe}
          probeBubble={probeBubble}
          anyProbeRunning={anyProbeRunning}
        />
      </div>
    </article>
  )
}

function ConnectivityChecksGallery(): JSX.Element {
  return (
    <div
      style={{
        display: 'grid',
        gap: 24,
        padding: 28,
        borderRadius: 28,
        background:
          'radial-gradient(circle at top, rgba(59, 130, 246, 0.12), transparent 32%), linear-gradient(180deg, #020617 0%, #0f172a 100%)',
      }}
    >
      <section style={{ display: 'grid', gap: 8, maxWidth: 760 }}>
        <div
          style={{
            fontSize: '0.78rem',
            fontWeight: 700,
            letterSpacing: '0.1em',
            textTransform: 'uppercase',
            color: 'rgba(148, 163, 184, 0.92)',
          }}
        >
          Token Detail Fragment
        </div>
        <h2 style={{ margin: 0, fontSize: '1.8rem', lineHeight: 1.12, color: '#f8fafc' }}>
          Connectivity Checks Gallery
        </h2>
        <p style={{ margin: 0, fontSize: '1rem', lineHeight: 1.6, color: 'rgba(226, 232, 240, 0.78)' }}>
          Dedicated Storybook surface for the MCP and API probe controls. It now shows the full MCP tools/list plus tools/call
          sweep for every advertised tool in one review board, without relying on separate full-page User Console stories.
        </p>
      </section>
      <div
        style={galleryGridStyle}
      >
        {scenarios.map((scenario) => (
          <ConnectivityScenarioCard key={scenario.title} {...scenario} />
        ))}
      </div>
    </div>
  )
}

const meta = {
  title: 'User Console/Fragments/Connectivity Checks',
  component: ConnectivityChecksPanel,
  tags: ['autodocs'],
  args: {
    title: 'Connectivity Checks',
    costHint: 'Runs a small MCP handshake and the full Tavily API chain against the mock upstream.',
    costHintAria: 'Connectivity check cost hint',
    stepStatusText,
    mcpButtonLabel: 'Test MCP',
    apiButtonLabel: 'Test API',
    mcpProbe: idleProbe,
    apiProbe: idleProbe,
  },
  parameters: {
    layout: 'padded',
    controls: { disable: true },
    docs: {
      description: {
        component:
          'Standalone Token Detail connectivity-check fragment. Use this isolated gallery to compare MCP/API probe states without loading the full User Console page shell.',
      },
    },
  },
} satisfies Meta<typeof ConnectivityChecksPanel>

export default meta

type Story = StoryObj<typeof meta>

export const StateGallery: Story = {
  name: 'State Gallery',
  render: () => <ConnectivityChecksGallery />,
}

export const __testables = {
  galleryGridStyle,
  scenarios,
}
