import type { Meta, StoryObj } from '@storybook/react-vite'

import { Badge } from './badge'
import {
  Table,
  TableBody,
  TableCaption,
  TableCell,
  TableFooter,
  TableHead,
  TableHeader,
  TableRow,
} from './table'

const meta = {
  title: 'UI/Table',
  component: Table,
  subcomponents: {
    TableHeader,
    TableBody,
    TableFooter,
    TableRow,
    TableHead,
    TableCell,
    TableCaption,
  },
  tags: ['autodocs'],
  parameters: {
    layout: 'padded',
    docs: {
      description: {
        component:
          'Scrollable table shell with typed header/body/footer helpers. Keep sorting, pagination, and card fallbacks in parent wrappers so the primitive stays layout-focused.',
      },
    },
  },
} satisfies Meta<typeof Table>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {
  render: () => (
    <div className="w-full max-w-4xl rounded-xl border border-border/80 bg-card/60 p-4">
      <Table>
        <TableCaption>Recent sync activity</TableCaption>
        <TableHeader>
          <TableRow>
            <TableHead>Job</TableHead>
            <TableHead>Status</TableHead>
            <TableHead>Owner</TableHead>
            <TableHead className="text-right">Duration</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow>
            <TableCell className="font-medium">quota_sync_610</TableCell>
            <TableCell><Badge>Success</Badge></TableCell>
            <TableCell>ops-bot</TableCell>
            <TableCell className="text-right">18s</TableCell>
          </TableRow>
          <TableRow>
            <TableCell className="font-medium">usage_rollup_611</TableCell>
            <TableCell><Badge variant="secondary">Running</Badge></TableCell>
            <TableCell>scheduler</TableCell>
            <TableCell className="text-right">42s</TableCell>
          </TableRow>
          <TableRow>
            <TableCell className="font-medium">key_audit_612</TableCell>
            <TableCell><Badge variant="outline">Queued</Badge></TableCell>
            <TableCell>alice</TableCell>
            <TableCell className="text-right">-</TableCell>
          </TableRow>
        </TableBody>
        <TableFooter>
          <TableRow>
            <TableCell colSpan={3}>Visible jobs</TableCell>
            <TableCell className="text-right">3</TableCell>
          </TableRow>
        </TableFooter>
      </Table>
    </div>
  ),
}
