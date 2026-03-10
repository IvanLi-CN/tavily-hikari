import { useState } from 'react'
import type { Meta, StoryObj } from '@storybook/react-vite'

import { Button } from './button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from './dialog'
import { Input } from './input'

function DialogStory(props: {
  title: string
  description: string
  triggerLabel: string
  confirmLabel: string
}): JSX.Element {
  const [open, setOpen] = useState(false)

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant="outline">{props.triggerLabel}</Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{props.title}</DialogTitle>
          <DialogDescription>{props.description}</DialogDescription>
        </DialogHeader>
        <div className="grid gap-3">
          <label className="grid gap-2 text-sm font-medium">
            Display name
            <Input defaultValue="Tavily Hikari" />
          </label>
          <label className="grid gap-2 text-sm font-medium">
            Environment
            <Input defaultValue="Production" />
          </label>
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => setOpen(false)}>
            Cancel
          </Button>
          <Button onClick={() => setOpen(false)}>{props.confirmLabel}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

const meta = {
  title: 'UI/Dialog',
  component: Dialog,
  subcomponents: {
    DialogTrigger,
    DialogHeader,
    DialogFooter,
    DialogTitle,
    DialogDescription,
  },
  tags: ['autodocs'],
  parameters: {
    layout: 'centered',
    docs: {
      description: {
        component:
          'Desktop modal primitive built from Radix Dialog. Compose `DialogTrigger`, `DialogContent`, and header/footer slots for confirmation, form, or details overlays.',
      },
    },
  },
} satisfies Meta<typeof Dialog>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {
  render: () => (
    <DialogStory
      title="Update workspace settings"
      description="Use the dialog slots to group short forms and confirmation actions without leaving the current page."
      triggerLabel="Open dialog"
      confirmLabel="Save changes"
    />
  ),
}

export const Confirmation: Story = {
  parameters: {
    docs: {
      description: {
        story: 'A smaller confirmation flow that keeps the same trigger/content contract while swapping the copy and action tone.',
      },
    },
  },
  render: () => (
    <DialogStory
      title="Rotate token secret"
      description="This action will immediately invalidate the previous secret in connected tooling."
      triggerLabel="Rotate secret"
      confirmLabel="Rotate now"
    />
  ),
}
