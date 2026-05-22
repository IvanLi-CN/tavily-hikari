import type { Meta, StoryObj } from '@storybook/react-vite'
import type { ReactNode } from 'react'
import { Activity, AlertTriangle, CheckCircle2, Database, KeyRound, Search } from 'lucide-react'

import { Badge } from './components/ui/badge'
import { Button } from './components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from './components/ui/card'
import { Input } from './components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from './components/ui/table'

const meta = {
  title: 'Design System/Claymorphism',
  tags: ['autodocs'],
  parameters: {
    layout: 'fullscreen',
    docs: {
      description: {
        component:
          'Claymorphism token and component review surface for the full Tavily Hikari web redesign.',
      },
    },
  },
} satisfies Meta

export default meta

type Story = StoryObj

function ClayPageFrame({ children }: { children: ReactNode }): JSX.Element {
  return (
    <main className="app-shell min-h-screen px-6 py-8 text-foreground md:px-10">
      <div className="mx-auto flex max-w-7xl flex-col gap-8">{children}</div>
    </main>
  )
}

export const Overview: Story = {
  render: () => (
    <ClayPageFrame>
      <section className="surface app-header flex flex-col gap-5 p-8 md:flex-row md:items-center md:justify-between">
        <div className="max-w-3xl">
          <p className="mb-3 text-sm font-bold uppercase tracking-wide text-primary">Tropical clay system</p>
          <h1 className="font-display text-4xl font-black leading-tight md:text-6xl">Soft control surfaces for Hikari operations</h1>
          <p className="mt-4 max-w-2xl text-base font-medium leading-relaxed text-muted-foreground">
            Light clay depth, saturated state color, and dense admin affordances share the same token set.
          </p>
        </div>
        <div className="flex flex-wrap gap-3">
          <Button>
            <KeyRound className="h-4 w-4" />
            Create token
          </Button>
          <Button variant="secondary">
            <Activity className="h-4 w-4" />
            View health
          </Button>
        </div>
      </section>

      <section className="grid gap-6 lg:grid-cols-[1.1fr_0.9fr]">
        <Card>
          <CardHeader>
            <CardTitle>Shared components</CardTitle>
            <CardDescription>Buttons, inputs, cards, and badges use the same clay depth vocabulary.</CardDescription>
          </CardHeader>
          <CardContent className="grid gap-5">
            <div className="flex flex-wrap gap-3">
              <Button>Default</Button>
              <Button variant="outline">Outline</Button>
              <Button variant="success">Healthy</Button>
              <Button variant="warning">Quota warning</Button>
              <Button variant="destructive">Disable key</Button>
            </div>
            <div className="grid gap-3 sm:grid-cols-[1fr_auto]">
              <Input placeholder="Paste th-... access token" />
              <Button variant="secondary">
                <Search className="h-4 w-4" />
                Inspect
              </Button>
            </div>
            <div className="flex flex-wrap gap-2">
              <Badge>active</Badge>
              <Badge variant="success">success</Badge>
              <Badge variant="warning">warning</Badge>
              <Badge variant="info">info</Badge>
              <Badge variant="destructive">blocked</Badge>
            </div>
          </CardContent>
        </Card>

        <Card className="animate-clay-breathe">
          <CardHeader>
            <CardTitle>Operational status</CardTitle>
            <CardDescription>Color reinforces status but labels remain explicit.</CardDescription>
          </CardHeader>
          <CardContent className="grid gap-4">
            {[
              { icon: CheckCircle2, label: 'Healthy keys', value: '18', tone: 'text-success' },
              { icon: AlertTriangle, label: 'Quota watch', value: '3', tone: 'text-warning' },
              { icon: Database, label: 'Requests today', value: '2,481', tone: 'text-primary' },
            ].map((item) => (
              <div key={item.label} className="flex items-center justify-between rounded-[24px] bg-muted/45 px-5 py-4 shadow-clayPressed">
                <span className="flex items-center gap-3 text-sm font-bold text-muted-foreground">
                  <item.icon className={`h-5 w-5 ${item.tone}`} />
                  {item.label}
                </span>
                <strong className="font-display text-2xl font-black">{item.value}</strong>
              </div>
            ))}
          </CardContent>
        </Card>
      </section>

      <Card>
        <CardHeader>
          <CardTitle>Admin density sample</CardTitle>
          <CardDescription>Tables keep compact scan rhythm while inheriting clay surfaces.</CardDescription>
        </CardHeader>
        <CardContent>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Key</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Requests</TableHead>
                <TableHead>Last used</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {[
                ['tvly-a7c2', 'active', '812', '2 min ago'],
                ['tvly-f94b', 'watch', '153', '18 min ago'],
                ['tvly-03ed', 'disabled', '0', 'yesterday'],
              ].map(([key, status, requests, lastUsed]) => (
                <TableRow key={key}>
                  <TableCell className="font-mono font-semibold">{key}</TableCell>
                  <TableCell>
                    <Badge variant={status === 'active' ? 'success' : status === 'watch' ? 'warning' : 'neutral'}>{status}</Badge>
                  </TableCell>
                  <TableCell>{requests}</TableCell>
                  <TableCell className="text-muted-foreground">{lastUsed}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </ClayPageFrame>
  ),
}
