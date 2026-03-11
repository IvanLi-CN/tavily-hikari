import type { Meta, StoryObj } from '@storybook/react-vite'

import AdminTableShell from './AdminTableShell'
import { TableBody, TableCell, TableHead, TableHeader, TableRow } from './ui/table'

const meta = {
  title: 'Admin/Wrappers/AdminTableShell',
  component: AdminTableShell,
  parameters: {
    layout: 'padded',
  },
  args: {
    children: null,
  },
  render: () => (
    <div style={{ maxWidth: 920, margin: '0 auto' }}>
      <AdminTableShell tableClassName="admin-users-table">
        <TableHeader>
          <TableRow>
            <TableHead>User</TableHead>
            <TableHead>Status</TableHead>
            <TableHead>Quota</TableHead>
            <TableHead>Last Activity</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow>
            <TableCell>Alice Wang</TableCell>
            <TableCell>Active</TableCell>
            <TableCell>298 / 1,000</TableCell>
            <TableCell>11:42:10</TableCell>
          </TableRow>
          <TableRow>
            <TableCell>Bob Chen</TableCell>
            <TableCell>Active</TableCell>
            <TableCell>602 / 1,000</TableCell>
            <TableCell>11:36:44</TableCell>
          </TableRow>
          <TableRow>
            <TableCell>Charlie Li</TableCell>
            <TableCell>Inactive</TableCell>
            <TableCell>0 / 500</TableCell>
            <TableCell>--</TableCell>
          </TableRow>
        </TableBody>
      </AdminTableShell>
    </div>
  ),
} satisfies Meta<typeof AdminTableShell>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {}
