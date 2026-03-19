import { useState } from 'react'
import type { Meta, StoryObj } from '@storybook/react-vite'

import KeyStickyPanels from './KeyStickyPanels'
import { stickyNodesStoryData, stickyUsersStoryData, stickyUsersStoryPerPage, stickyUsersStoryTotal } from './keyStickyStoryData'

function StickyPanelsCanvas(props: {
  stickyUsersTotal?: number
  stickyUsersPage?: number
  stickyUsersLoadState?: 'initial_loading' | 'switch_loading' | 'refreshing' | 'ready' | 'error'
  stickyUsersError?: string | null
  stickyNodesLoadState?: 'initial_loading' | 'switch_loading' | 'refreshing' | 'ready' | 'error'
  stickyNodesError?: string | null
  stickyUsers?: typeof stickyUsersStoryData
  stickyNodes?: typeof stickyNodesStoryData
  maxWidth?: number
}): JSX.Element {
  const [page, setPage] = useState(props.stickyUsersPage ?? 1)

  return (
    <div style={{ maxWidth: props.maxWidth ?? 1280, margin: '0 auto', display: 'grid', gap: 16 }}>
      <KeyStickyPanels
        stickyUsers={props.stickyUsers ?? stickyUsersStoryData}
        stickyUsersLoadState={props.stickyUsersLoadState ?? 'ready'}
        stickyUsersError={props.stickyUsersError ?? null}
        stickyUsersPage={page}
        stickyUsersTotal={props.stickyUsersTotal ?? stickyUsersStoryTotal}
        stickyUsersPerPage={stickyUsersStoryPerPage}
        onStickyUsersPrevious={() => setPage((current) => Math.max(1, current - 1))}
        onStickyUsersNext={() => setPage((current) => current + 1)}
        stickyNodes={props.stickyNodes ?? stickyNodesStoryData}
        stickyNodesLoadState={props.stickyNodesLoadState ?? 'ready'}
        stickyNodesError={props.stickyNodesError ?? null}
      />
    </div>
  )
}

const meta = {
  title: 'Admin/Fragments/KeyStickyPanels',
  component: KeyStickyPanels,
  parameters: {
    layout: 'padded',
  },
  args: {
    stickyUsers: stickyUsersStoryData,
    stickyUsersLoadState: 'ready',
    stickyUsersError: null,
    stickyUsersPage: 1,
    stickyUsersTotal: stickyUsersStoryTotal,
    stickyUsersPerPage: stickyUsersStoryPerPage,
    onStickyUsersPrevious: () => undefined,
    onStickyUsersNext: () => undefined,
    stickyNodes: stickyNodesStoryData,
    stickyNodesLoadState: 'ready',
    stickyNodesError: null,
    onOpenUser: () => undefined,
  },
  render: () => <StickyPanelsCanvas />,
} satisfies Meta<typeof KeyStickyPanels>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {}

export const Empty: Story = {
  render: () => <StickyPanelsCanvas stickyUsers={[]} stickyUsersTotal={0} stickyNodes={[]} />,
}

export const Loading: Story = {
  render: () => <StickyPanelsCanvas stickyUsers={[]} stickyNodes={[]} stickyUsersLoadState="initial_loading" stickyNodesLoadState="initial_loading" />,
}

export const ErrorState: Story = {
  render: () => (
    <StickyPanelsCanvas
      stickyUsers={[]}
      stickyUsersTotal={0}
      stickyNodes={[]}
      stickyUsersLoadState="error"
      stickyUsersError="Failed to load sticky users"
      stickyNodesLoadState="error"
      stickyNodesError="Failed to load sticky nodes"
    />
  ),
}

export const Paginated: Story = {
  render: () => <StickyPanelsCanvas stickyUsersTotal={43} stickyUsersPage={2} />,
}

export const NarrowLayout: Story = {
  render: () => <StickyPanelsCanvas maxWidth={960} />,
}

export const Gallery: Story = {
  render: () => (
    <div style={{ display: 'grid', gap: 24 }}>
      <div>
        <h3 style={{ marginBottom: 12, fontWeight: 700 }}>Healthy snapshot</h3>
        <StickyPanelsCanvas />
      </div>
      <div>
        <h3 style={{ marginBottom: 12, fontWeight: 700 }}>Empty snapshot</h3>
        <StickyPanelsCanvas stickyUsers={[]} stickyUsersTotal={0} stickyNodes={[]} />
      </div>
    </div>
  ),
}
