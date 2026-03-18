import type { Meta, StoryObj } from '@storybook/react-vite'

import RequestKindBadge from './RequestKindBadge'

const requestKindSections = [
  {
    title: 'API request kinds',
    description: 'HTTP routes, result fetches, and raw fallback endpoints.',
    items: [
      { requestKindKey: 'api:search', requestKindLabel: 'API | search' },
      { requestKindKey: 'api:extract', requestKindLabel: 'API | extract' },
      { requestKindKey: 'api:crawl', requestKindLabel: 'API | crawl' },
      { requestKindKey: 'api:map', requestKindLabel: 'API | map' },
      { requestKindKey: 'api:research', requestKindLabel: 'API | research' },
      { requestKindKey: 'api:research-result', requestKindLabel: 'API | research result' },
      { requestKindKey: 'api:usage', requestKindLabel: 'API | usage' },
      { requestKindKey: 'api:raw:/api/internal/report', requestKindLabel: 'API | /api/internal/report' },
    ],
  },
  {
    title: 'MCP request kinds',
    description: 'Tool calls, protocol methods, batched calls, and raw transport fallbacks.',
    items: [
      { requestKindKey: 'mcp:extract', requestKindLabel: 'MCP | extract' },
      { requestKindKey: 'mcp:search', requestKindLabel: 'MCP | search' },
      { requestKindKey: 'mcp:batch', requestKindLabel: 'MCP | batch' },
      { requestKindKey: 'mcp:tool:Acme Lookup', requestKindLabel: 'MCP | Acme Lookup' },
      { requestKindKey: 'mcp:initialize', requestKindLabel: 'MCP | initialize' },
      { requestKindKey: 'mcp:notifications/initialized', requestKindLabel: 'MCP | notifications/initialized' },
      { requestKindKey: 'mcp:ping', requestKindLabel: 'MCP | ping' },
      { requestKindKey: 'mcp:resources/list', requestKindLabel: 'MCP | resources/list' },
      { requestKindKey: 'mcp:resources/templates/list', requestKindLabel: 'MCP | resources/templates/list' },
      { requestKindKey: 'mcp:tools/list', requestKindLabel: 'MCP | tools/list' },
      { requestKindKey: 'mcp:resources/subscribe', requestKindLabel: 'MCP | resources/subscribe' },
      { requestKindKey: 'mcp:raw:/mcp/sse', requestKindLabel: 'MCP | /mcp/sse' },
    ],
  },
] as const

function RequestKindStoryCard(props: {
  requestKindKey: string
  requestKindLabel: string
}): JSX.Element {
  return (
    <div
      style={{
        display: 'grid',
        gap: 12,
        minWidth: 0,
        padding: '14px 16px',
        borderRadius: 16,
        border: '1px solid hsl(var(--border) / 0.72)',
        background: 'hsl(var(--background) / 0.56)',
      }}
    >
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, alignItems: 'center' }}>
        <span
          style={{
            fontSize: '0.72rem',
            fontWeight: 700,
            letterSpacing: '0.06em',
            textTransform: 'uppercase',
            color: 'hsl(var(--muted-foreground))',
          }}
        >
          Preview
        </span>
        <RequestKindBadge {...props} />
      </div>
      <div style={{ display: 'grid', gap: 6, minWidth: 0 }}>
        <span
          style={{
            fontSize: '0.72rem',
            fontWeight: 700,
            letterSpacing: '0.06em',
            textTransform: 'uppercase',
            color: 'hsl(var(--muted-foreground))',
          }}
        >
          Stable key
        </span>
        <code
          style={{
            display: 'block',
            minWidth: 0,
            padding: '10px 12px',
            borderRadius: 12,
            border: '1px solid hsl(var(--border) / 0.58)',
            background: 'hsl(var(--background) / 0.78)',
            color: 'hsl(var(--foreground) / 0.82)',
            fontSize: '0.84rem',
            lineHeight: 1.5,
            whiteSpace: 'normal',
            wordBreak: 'break-word',
          }}
        >
          {props.requestKindKey}
        </code>
      </div>
    </div>
  )
}

function ThemeCatalogPanel(): JSX.Element {
  return (
    <div
      style={{
        padding: 20,
        borderRadius: 20,
        border: '1px solid hsl(var(--border) / 0.82)',
        background: 'linear-gradient(180deg, hsl(var(--card) / 0.98), hsl(var(--card) / 0.92))',
        color: 'hsl(var(--foreground))',
        boxShadow: '0 18px 40px -28px hsl(var(--foreground) / 0.18)',
      }}
    >
      <div style={{ marginBottom: 14 }}>
        <div
          style={{
            fontSize: '0.78rem',
            fontWeight: 700,
            letterSpacing: '0.08em',
            textTransform: 'uppercase',
            color: 'hsl(var(--muted-foreground))',
          }}
        >
          Theme Catalog
        </div>
        <div style={{ fontSize: '0.92rem', color: 'hsl(var(--muted-foreground))' }}>
          Use the Storybook theme toolbar to preview the badge palette in light, dark, or system mode.
        </div>
      </div>
      <div style={{ display: 'grid', gap: 18 }}>
        {requestKindSections.map((section) => (
          <section
            key={section.title}
            style={{
              display: 'grid',
              gap: 12,
              padding: '16px 18px',
              borderRadius: 18,
              border: '1px solid hsl(var(--border) / 0.66)',
              background: 'hsl(var(--background) / 0.42)',
            }}
          >
            <div style={{ display: 'grid', gap: 4 }}>
              <div
                style={{
                  fontSize: '0.78rem',
                  fontWeight: 700,
                  letterSpacing: '0.08em',
                  textTransform: 'uppercase',
                  color: 'hsl(var(--muted-foreground))',
                }}
              >
                {section.title}
              </div>
              <div style={{ fontSize: '0.9rem', color: 'hsl(var(--muted-foreground))' }}>{section.description}</div>
            </div>
            <div
              style={{
                display: 'grid',
                gap: 12,
                gridTemplateColumns: 'repeat(auto-fit, minmax(260px, 1fr))',
              }}
            >
              {section.items.map((example) => (
                <RequestKindStoryCard key={example.requestKindKey} {...example} />
              ))}
            </div>
          </section>
        ))}
      </div>
    </div>
  )
}

const meta = {
  title: 'Components/RequestKindBadge',
  component: RequestKindBadge,
  tags: ['autodocs'],
  args: {
    requestKindKey: 'api:search',
    requestKindLabel: 'API | search',
    size: 'md',
  },
  argTypes: {
    size: {
      control: 'radio',
      options: ['sm', 'md'],
    },
  },
  parameters: {
    layout: 'padded',
    docs: {
      description: {
        component:
          'Theme-aware request type pill used by admin token logs. It maps stable request-kind keys to distinct API/MCP palettes, while unknown keys fall back to protocol-aware neutral tones. Use the Storybook toolbar theme switcher to inspect light and dark surfaces.',
      },
    },
  },
  render: (args) => (
    <div style={{ padding: 24, borderRadius: 20, border: '1px solid hsl(var(--border) / 0.82)', background: 'hsl(var(--card) / 0.88)' }}>
      <RequestKindBadge {...args} />
    </div>
  ),
} satisfies Meta<typeof RequestKindBadge>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {}

export const Compact: Story = {
  args: {
    requestKindKey: 'mcp:batch',
    requestKindLabel: 'MCP | batch',
    size: 'sm',
  },
}

export const ThemeCatalog: Story = {
  render: () => <ThemeCatalogPanel />,
}
