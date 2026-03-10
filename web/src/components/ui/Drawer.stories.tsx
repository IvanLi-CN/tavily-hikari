import { useState } from 'react'
import type { Meta, StoryObj } from '@storybook/react-vite'

import { Button } from './button'
import {
  Drawer,
  DrawerClose,
  DrawerContent,
  DrawerDescription,
  DrawerFooter,
  DrawerHeader,
  DrawerTitle,
  DrawerTrigger,
} from './drawer'
import { Input } from './input'

function DrawerStory(): JSX.Element {
  const [open, setOpen] = useState(false)

  return (
    <Drawer open={open} onOpenChange={setOpen}>
      <DrawerTrigger asChild>
        <Button variant="outline">Open drawer</Button>
      </DrawerTrigger>
      <DrawerContent>
        <DrawerHeader>
          <DrawerTitle>Quick filter sheet</DrawerTitle>
          <DrawerDescription>
            The mobile drawer keeps short forms and action groups attached to the bottom edge of the viewport.
          </DrawerDescription>
        </DrawerHeader>
        <div className="grid gap-3 px-4 pb-2">
          <label className="grid gap-2 text-sm font-medium">
            Query
            <Input defaultValue="status:quota_exhausted" />
          </label>
          <label className="grid gap-2 text-sm font-medium">
            Owner
            <Input defaultValue="ops-team" />
          </label>
        </div>
        <DrawerFooter>
          <Button>Apply filters</Button>
          <DrawerClose asChild>
            <Button variant="ghost">Dismiss</Button>
          </DrawerClose>
        </DrawerFooter>
      </DrawerContent>
    </Drawer>
  )
}

const meta = {
  title: 'UI/Drawer',
  component: Drawer,
  subcomponents: {
    DrawerTrigger,
    DrawerHeader,
    DrawerFooter,
    DrawerTitle,
    DrawerDescription,
  },
  tags: ['autodocs'],
  parameters: {
    layout: 'centered',
    viewport: { defaultViewport: '0390-device-iphone-14' },
    docs: {
      description: {
        component:
          'Bottom-sheet primitive powered by Vaul. Use it for compact mobile flows that should preserve context while exposing short forms or confirmations.',
      },
    },
  },
} satisfies Meta<typeof Drawer>

export default meta

type Story = StoryObj<typeof meta>

export const MobileSheet: Story = {
  render: () => <DrawerStory />,
}
