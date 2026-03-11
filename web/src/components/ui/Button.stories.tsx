import { ArrowRight, ExternalLink } from 'lucide-react'
import type { Meta, StoryObj } from '@storybook/react-vite'

import { Button } from './button'

const meta = {
  title: 'UI/Button',
  component: Button,
  tags: ['autodocs'],
  parameters: {
    layout: 'centered',
    docs: {
      description: {
        component:
          'Shared shadcn button primitive for primary, outline, tonal, and icon actions. Use `asChild` when the visual button should render an anchor or router link.',
      },
    },
  },
  args: {
    children: 'Continue',
    variant: 'default',
    size: 'default',
    disabled: false,
  },
} satisfies Meta<typeof Button>

export default meta

type Story = StoryObj<typeof meta>

export const Playground: Story = {}

export const Variants: Story = {
  render: () => (
    <div className="flex max-w-3xl flex-wrap items-center gap-3">
      <Button>Default</Button>
      <Button variant="secondary">Secondary</Button>
      <Button variant="outline">Outline</Button>
      <Button variant="ghost">Ghost</Button>
      <Button variant="link">Link style</Button>
      <Button variant="success">Success</Button>
      <Button variant="warning">Warning</Button>
      <Button variant="destructive">Destructive</Button>
    </div>
  ),
}

export const SizesAndIcon: Story = {
  render: () => (
    <div className="flex flex-wrap items-center gap-3">
      <Button size="xs">Tiny</Button>
      <Button size="sm">Small</Button>
      <Button>Default</Button>
      <Button size="lg">
        Primary action
        <ArrowRight className="h-4 w-4" />
      </Button>
      <Button size="icon" aria-label="Open details">
        <ArrowRight className="h-4 w-4" />
      </Button>
    </div>
  ),
}

export const AsChildLink: Story = {
  render: () => (
    <Button asChild variant="outline">
      <a href="https://example.com" target="_blank" rel="noreferrer" onClick={(event) => event.preventDefault()}>
        Open external guide
        <ExternalLink className="h-4 w-4" />
      </a>
    </Button>
  ),
}
