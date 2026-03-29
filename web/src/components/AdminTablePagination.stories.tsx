import { useState } from 'react'
import type { Meta, StoryObj } from '@storybook/react-vite'

import AdminTablePagination from './AdminTablePagination'

function PaginationStory(): JSX.Element {
  const [page, setPage] = useState(2)
  const [perPage, setPerPage] = useState(20)
  const totalPages = 6

  return (
    <div style={{ maxWidth: 720, margin: '0 auto' }}>
      <AdminTablePagination
        page={page}
        totalPages={totalPages}
        perPage={perPage}
        onPerPageChange={setPerPage}
        onPrevious={() => setPage((current) => Math.max(1, current - 1))}
        onNext={() => setPage((current) => Math.min(totalPages, current + 1))}
        previousDisabled={page <= 1}
        nextDisabled={page >= totalPages}
      />
    </div>
  )
}

const meta = {
  title: 'Admin/Wrappers/AdminTablePagination',
  component: AdminTablePagination,
  parameters: {
    docs: {
      description: {
        component: [
          'Shared admin pagination footer for list views that need page count, page size, and previous/next controls.',
          '',
          'Public docs: [Configuration & Access](../configuration-access.html) · [Storybook Guide](../storybook-guide.html)',
        ].join('\n'),
      },
    },
    layout: 'padded',
  },
  args: {
    page: 2,
    totalPages: 6,
    perPage: 20,
    onPerPageChange: () => undefined,
    onPrevious: () => undefined,
    onNext: () => undefined,
    previousDisabled: false,
    nextDisabled: false,
  },
  render: () => <PaginationStory />,
} satisfies Meta<typeof AdminTablePagination>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {}

export const SinglePage: Story = {
  render: () => (
    <div style={{ maxWidth: 720, margin: '0 auto' }}>
      <AdminTablePagination
        page={1}
        totalPages={1}
        pageSummary="Page 1 / 1"
        previousDisabled
        nextDisabled
        onPrevious={() => undefined}
        onNext={() => undefined}
      />
    </div>
  ),
}

export const MobileNoPerPage: Story = {
  parameters: {
    viewport: { defaultViewport: '0390-device-iphone-14' },
  },
  render: () => (
    <div style={{ maxWidth: 390, margin: '0 auto' }}>
      <AdminTablePagination
        page={2}
        totalPages={12}
        pageSummary="第 2/12 页"
        previousLabel="上一页"
        nextLabel="下一页"
        onPrevious={() => undefined}
        onNext={() => undefined}
      />
    </div>
  ),
}
