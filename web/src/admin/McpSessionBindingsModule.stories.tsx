import type { Meta, StoryObj } from '@storybook/react-vite'
import { useMemo, useState, type ComponentProps } from 'react'
import { expect, userEvent, within } from 'storybook/test'

import McpSessionBindingsModule from './McpSessionBindingsModule'
import type {
  AdminMcpSessionBindingListItem,
  AdminMcpSessionBindingsPage,
  AdminMcpSessionBindingsQuery,
} from '../api'
import type { AdminMcpSessionBindingsPathContext } from './routes'

type StoryArgs = ComponentProps<typeof McpSessionBindingsModule>

const SAMPLE_ITEMS: AdminMcpSessionBindingListItem[] = [
  {
    proxySessionId: 'sess-upstream-001',
    authTokenId: 'tok_alpha',
    userId: 'usr_alice',
    upstreamKeyId: 'key-primary',
    createdAt: 1_783_940_400,
    updatedAt: 1_783_958_400,
    expiresAt: 1_783_962_000,
    status: 'active',
    revokedAt: null,
    revokeReason: null,
  },
  {
    proxySessionId: 'sess-upstream-002',
    authTokenId: 'tok_beta',
    userId: 'usr_bob',
    upstreamKeyId: 'key-backup',
    createdAt: 1_783_941_200,
    updatedAt: 1_783_957_500,
    expiresAt: 1_783_961_800,
    status: 'active',
    revokedAt: null,
    revokeReason: null,
  },
  {
    proxySessionId: 'sess-upstream-003',
    authTokenId: 'tok_gamma',
    userId: 'usr_carla',
    upstreamKeyId: 'key-eu',
    createdAt: 1_783_931_200,
    updatedAt: 1_783_950_200,
    expiresAt: 1_783_954_000,
    status: 'expired',
    revokedAt: null,
    revokeReason: null,
  },
  {
    proxySessionId: 'sess-upstream-004',
    authTokenId: 'tok_delta',
    userId: 'usr_derek',
    upstreamKeyId: 'key-quarantine',
    createdAt: 1_783_921_200,
    updatedAt: 1_783_948_200,
    expiresAt: 1_783_949_000,
    status: 'revoked',
    revokedAt: 1_783_948_400,
    revokeReason: 'admin_selected_revoke',
  },
]

function buildPage(
  items: AdminMcpSessionBindingListItem[],
  query: AdminMcpSessionBindingsPathContext,
): AdminMcpSessionBindingsPage {
  const status = query.status ?? 'active'
  const filtered = items.filter((item) => {
    if (status === 'all') return true
    if (status === 'revoked') return item.status === 'revoked'
    return item.status === 'active'
  })
  return {
    items: filtered,
    total: filtered.length,
    page: query.page ?? 1,
    perPage: 20,
    activeMatchingCount: filtered.filter((item) => item.status === 'active').length,
  }
}

function StoryCanvas(args: Partial<StoryArgs> & { initialItems?: AdminMcpSessionBindingListItem[] } = {}): JSX.Element {
  const [query, setQuery] = useState<AdminMcpSessionBindingsPathContext>(args.query ?? { status: 'active', page: 1 })
  const [items, setItems] = useState<AdminMcpSessionBindingListItem[]>(args.initialItems ?? SAMPLE_ITEMS)
  const page = useMemo(() => buildPage(items, query), [items, query])

  const revokeItems = (ids: Set<string>, reason: string) => {
    const revokedAt = 1_783_959_400
    setItems((current) =>
      current.map((item) =>
        ids.has(item.proxySessionId) && item.status === 'active'
          ? { ...item, status: 'revoked', revokedAt, revokeReason: reason }
          : item,
      ),
    )
  }

  const currentApiQuery: AdminMcpSessionBindingsQuery = {
    status: query.status ?? 'active',
    createdFrom: query.createdFrom ?? null,
    createdTo: query.createdTo ?? null,
    updatedFrom: query.updatedFrom ?? null,
    updatedTo: query.updatedTo ?? null,
    page: query.page ?? 1,
    perPage: 20,
  }

  return (
    <div style={{ maxWidth: 1360, margin: '0 auto', padding: 24 }}>
      <McpSessionBindingsModule
        language="zh"
        query={query}
        data={page}
        loadState={args.loadState ?? 'ready'}
        error={args.error ?? null}
        refreshing={args.refreshing ?? false}
        busy={args.busy ?? false}
        onRefresh={() => undefined}
        onNavigate={setQuery}
        onRevokeSelected={(proxySessionIds) => revokeItems(new Set(proxySessionIds), 'admin_selected_revoke')}
        onRevokeFiltered={() => {
          revokeItems(
            new Set(
              page.items
                .filter((item) => item.status === 'active')
                .map((item) => item.proxySessionId),
            ),
            'admin_filtered_revoke',
          )
        }}
        onOpenUser={() => undefined}
        onOpenToken={() => undefined}
        onOpenKey={() => undefined}
      />
      <div data-testid="mcp-session-bindings-active-count" style={{ marginTop: 12, fontSize: 13 }}>
        active={buildPage(items, currentApiQuery).activeMatchingCount}
      </div>
    </div>
  )
}

const meta = {
  title: 'Admin/Modules/McpSessionBindingsModule',
  component: McpSessionBindingsModule,
  tags: ['autodocs'],
  parameters: {
    layout: 'padded',
    docs: {
      description: {
        component:
          'Hidden admin route content for releasing legacy `upstream_mcp` session bindings without surfacing the route in system-settings navigation.',
      },
    },
  },
  args: {
    language: 'zh',
    query: { status: 'active', page: 1 },
    data: buildPage(SAMPLE_ITEMS, { status: 'active', page: 1 }),
    loadState: 'ready',
    error: null,
    refreshing: false,
    busy: false,
    onRefresh: () => undefined,
    onNavigate: () => undefined,
    onRevokeSelected: () => undefined,
    onRevokeFiltered: () => undefined,
    onOpenUser: () => undefined,
    onOpenToken: () => undefined,
    onOpenKey: () => undefined,
  },
  render: () => <StoryCanvas />,
} satisfies Meta<typeof McpSessionBindingsModule>

export default meta

type Story = StoryObj<typeof meta>

export const ActiveOnly: Story = {}

export const RevokedHistory: Story = {
  render: () => <StoryCanvas query={{ status: 'revoked', page: 1 }} />,
}

export const AllStates: Story = {
  render: () => <StoryCanvas query={{ status: 'all', page: 1 }} />,
}

export const EmptyState: Story = {
  render: () => (
    <StoryCanvas
      query={{ status: 'active', page: 1 }}
      initialItems={SAMPLE_ITEMS.filter((item) => item.status === 'revoked')}
    />
  ),
}

export const InteractionContract: Story = {
  render: () => <StoryCanvas />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const releaseAllButton = await canvas.findByRole('button', {
      name: /释放当前筛选结果全部活跃会话/,
    })
    await userEvent.click(releaseAllButton)

    const dialog = await canvas.findByRole('dialog')
    expect(dialog.textContent).toContain('本次将释放 2 个活跃会话')

    const confirmButton = within(dialog).getByRole('button', { name: '确认释放' })
    await userEvent.click(confirmButton)

    const activeCount = await canvas.findByTestId('mcp-session-bindings-active-count')
    expect(activeCount.textContent).toContain('active=0')
  },
}
