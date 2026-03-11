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
