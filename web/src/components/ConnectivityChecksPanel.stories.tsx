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
  { id: 'mcp-initialize', label: 'MCP session initialize', status: 'success' },
  { id: 'mcp-initialized', label: 'MCP initialized notification', status: 'success' },
  { id: 'mcp-ping', label: 'MCP service connectivity', status: 'success' },
  { id: 'mcp-tools-list', label: 'MCP tool discovery', status: 'success' },
  { id: 'mcp-tool-call:tavily_search', label: 'Call tavily_search tool', status: 'success' },
  { id: 'mcp-tool-call:tavily_extract', label: 'Call tavily_extract tool', status: 'success' },
  { id: 'mcp-tool-call:tavily_crawl', label: 'Call tavily_crawl tool', status: 'success' },
  { id: 'mcp-tool-call:tavily_map', label: 'Call tavily_map tool', status: 'success' },
  { id: 'mcp-tool-call:tavily_research', label: 'Call tavily_research tool', status: 'success' },
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
    mcpProbe: { state: 'success', completed: 9, total: 9 },
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
    mcpProbe: { state: 'partial', completed: 9, total: 9 },
    apiProbe: idleProbe,
    mcpButtonLabel: 'MCP Partial',
    apiButtonLabel: 'Test API',
    probeBubble: {
      visible: true,
      anchor: 'mcp',
      items: [
        { id: 'mcp-initialize', label: 'MCP session initialize', status: 'success' },
        { id: 'mcp-initialized', label: 'MCP initialized notification', status: 'success' },
        { id: 'mcp-ping', label: 'MCP service connectivity', status: 'success' },
        { id: 'mcp-tools-list', label: 'MCP tool discovery', status: 'success' },
        { id: 'mcp-tool-call:tavily_search', label: 'Call tavily_search tool', status: 'success' },
        { id: 'mcp-tool-call:tavily_extract', label: 'Call tavily_extract tool', status: 'success' },
        { id: 'mcp-tool-call:tavily_crawl', label: 'Call tavily_crawl tool', status: 'success' },
        { id: 'mcp-tool-call:tavily_map', label: 'Call tavily_map tool', status: 'failed', detail: '500 timeout from mock upstream' },
        { id: 'mcp-tool-call:tavily_research', label: 'Call tavily_research tool', status: 'success' },
      ],
    },
  },
  {
    title: 'Long Tool Names',
    description: 'Structured tool rows keep long tool identifiers readable without splitting the action copy into awkward fragments.',
    mcpProbe: { state: 'success', completed: 6, total: 6 },
    apiProbe: idleProbe,
    mcpButtonLabel: 'MCP Ready',
    apiButtonLabel: 'Test API',
    probeBubble: {
      visible: true,
      anchor: 'mcp',
      items: [
        { id: 'mcp-initialize', label: 'MCP session initialize', status: 'success' },
        { id: 'mcp-initialized', label: 'MCP initialized notification', status: 'success' },
        { id: 'mcp-ping', label: 'MCP service connectivity', status: 'success' },
        { id: 'mcp-tools-list', label: 'MCP tool discovery', status: 'success' },
        {
          id: 'mcp-tool-call:tavily_search_with_extended_probe_fixture_name',
          label: 'Call tavily_search_with_extended_probe_fixture_name tool',
          status: 'success',
        },
        { id: 'mcp-tool-call:tavily_extract', label: 'Call tavily_extract tool', status: 'success' },
      ],
    },
  },
  {
    title: 'Authentication Failed',
    description: 'The preflight token fetch succeeds, but MCP initialize rejects the user token immediately.',
    mcpProbe: { state: 'failed', completed: 0, total: 9 },
    apiProbe: idleProbe,
    mcpButtonLabel: 'MCP Failed',
    apiButtonLabel: 'Test API',
    probeBubble: {
      visible: true,
      anchor: 'mcp',
      items: [
        { id: 'mcp-initialize', label: 'MCP session initialize', status: 'failed', detail: '401 invalid or disabled token' },
      ],
    },
  },
  {
    title: 'Quota Blocked',
    description: 'Quota precheck blocks every billable MCP tool call while still surfacing discovery of the full advertised tool list.',
    mcpProbe: { state: 'partial', completed: 9, total: 9 },
    apiProbe: idleProbe,
    mcpButtonLabel: 'MCP Blocked',
    apiButtonLabel: 'Test API',
    probeBubble: {
      visible: true,
      anchor: 'mcp',
      items: [
        { id: 'mcp-initialize', label: 'MCP session initialize', status: 'success' },
        { id: 'mcp-initialized', label: 'MCP initialized notification', status: 'success' },
        { id: 'mcp-ping', label: 'MCP service connectivity', status: 'success' },
        { id: 'mcp-tools-list', label: 'MCP tool discovery', status: 'success' },
        { id: 'mcp-tool-call:tavily_search', label: 'Call tavily_search tool', status: 'blocked', detail: 'Skipped after quota precheck' },
        { id: 'mcp-tool-call:tavily_extract', label: 'Call tavily_extract tool', status: 'blocked', detail: 'Skipped after quota precheck' },
        { id: 'mcp-tool-call:tavily_crawl', label: 'Call tavily_crawl tool', status: 'blocked', detail: 'Skipped after quota precheck' },
        { id: 'mcp-tool-call:tavily_map', label: 'Call tavily_map tool', status: 'blocked', detail: 'Skipped after quota precheck' },
        { id: 'mcp-tool-call:tavily_research', label: 'Call tavily_research tool', status: 'blocked', detail: 'Skipped after quota precheck' },
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

function ConnectivityChecksBubbleProof(): JSX.Element {
  const bubbleProofScenario = scenarios.find((scenario) => scenario.title === 'MCP Full Sweep') ?? scenarios[2]

  return (
    <div
      style={{
        display: 'grid',
        gap: 20,
        maxWidth: 720,
        margin: '0 auto',
      }}
    >
      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>Clipped container bubble proof</h2>
            <p className="panel-description">
              The parent shell is intentionally clipped. The probe result bubble must still render above it through the
              shared portal layer.
            </p>
          </div>
        </div>
        <div
          style={{
            overflow: 'hidden',
            maxHeight: 220,
            borderRadius: 28,
            border: '1px dashed rgba(96, 165, 250, 0.4)',
            background:
              'radial-gradient(circle at top, rgba(59, 130, 246, 0.1), transparent 42%), linear-gradient(180deg, rgba(2, 6, 23, 0.98), rgba(15, 23, 42, 0.92))',
            padding: 24,
          }}
        >
          <div className="dark" style={{ paddingTop: 52 }}>
            <ConnectivityChecksPanel
              title="Connectivity Checks"
              costHint="Runs a small MCP handshake and the full Tavily API chain against the mock upstream."
              costHintAria="Connectivity check cost hint"
              stepStatusText={stepStatusText}
              mcpButtonLabel={bubbleProofScenario.mcpButtonLabel}
              apiButtonLabel={bubbleProofScenario.apiButtonLabel}
              mcpProbe={bubbleProofScenario.mcpProbe}
              apiProbe={bubbleProofScenario.apiProbe}
              probeBubble={bubbleProofScenario.probeBubble}
              anyProbeRunning={bubbleProofScenario.anyProbeRunning}
            />
          </div>
        </div>
      </section>
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

export const BubbleProof: Story = {
  name: 'Bubble Proof',
  render: () => <ConnectivityChecksBubbleProof />,
}

export const __testables = {
  galleryGridStyle,
  scenarios,
}
