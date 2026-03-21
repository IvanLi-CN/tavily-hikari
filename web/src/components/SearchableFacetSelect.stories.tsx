import { type ReactNode, useState } from 'react'
import type { Meta, StoryObj } from '@storybook/react-vite'

import SearchableFacetSelect, { type SearchableFacetSelectOption } from './SearchableFacetSelect'

const keyOptions: SearchableFacetSelectOption[] = [
  { value: 'asR8', count: 1 },
  { value: 'J1nW', count: 3 },
  { value: 'MZli', count: 7 },
  { value: 'Qn8R', count: 2 },
  { value: 'U2vK', count: 5 },
  { value: 'Vn7D', count: 4 },
]

function StorySurface(props: { children: ReactNode }): JSX.Element {
  return (
    <div
      style={{
        width: 'min(380px, 100%)',
        padding: 24,
        borderRadius: 24,
        border: '1px solid hsl(var(--border) / 0.78)',
        background: 'linear-gradient(180deg, hsl(var(--card) / 0.98), hsl(var(--card) / 0.92))',
        boxShadow: '0 24px 48px -36px hsl(var(--foreground) / 0.24)',
      }}
    >
      {props.children}
    </div>
  )
}

function SearchableFacetSelectDemo(props: {
  options?: SearchableFacetSelectOption[]
  initialValue?: string | null
  allLabel?: string
  searchPlaceholder?: string
}): JSX.Element {
  const { options = keyOptions, initialValue = null, allLabel = 'All', searchPlaceholder = 'Filter keys' } = props
  const [value, setValue] = useState<string | null>(initialValue)
  const summary = value ? options.find((option) => option.value === value)?.value ?? value : allLabel

  return (
    <StorySurface>
      <div style={{ display: 'grid', gap: 8 }}>
        <span
          style={{
            paddingInline: 2,
            fontSize: '0.72rem',
            fontWeight: 700,
            letterSpacing: '0.08em',
            textTransform: 'uppercase',
            color: 'hsl(var(--muted-foreground))',
          }}
        >
          Key
        </span>
        <SearchableFacetSelect
          value={value}
          options={options}
          summary={summary}
          allLabel={allLabel}
          emptyLabel="No matching keys"
          searchPlaceholder={searchPlaceholder}
          searchAriaLabel="Filter keys"
          triggerAriaLabel={`Key: ${summary}`}
          listAriaLabel="Key"
          onChange={setValue}
          labelVariant="mono"
        />
      </div>
    </StorySurface>
  )
}

function GalleryStory(): JSX.Element {
  const longList = Array.from({ length: 14 }, (_, index) => ({
    value: `k${String(index + 1).padStart(2, '0')}X`,
    count: (index % 5) + 1,
  }))

  return (
    <div style={{ display: 'grid', gap: 16, gridTemplateColumns: 'repeat(auto-fit, minmax(320px, 1fr))' }}>
      <SearchableFacetSelectDemo />
      <SearchableFacetSelectDemo initialValue="MZli" />
      <SearchableFacetSelectDemo options={longList} initialValue="k09X" />
    </div>
  )
}

const meta = {
  title: 'Components/SearchableFacetSelect',
  component: SearchableFacetSelect,
  tags: ['autodocs'],
  args: {
    value: null,
    options: keyOptions,
    summary: 'All',
    allLabel: 'All',
    emptyLabel: 'No matching keys',
    searchPlaceholder: 'Filter keys',
    searchAriaLabel: 'Filter keys',
    triggerAriaLabel: 'Key: All',
    listAriaLabel: 'Key',
    onChange: () => undefined,
  },
  parameters: {
    layout: 'padded',
    docs: {
      description: {
        component:
          'Shared searchable single-select dropdown for compact admin filters. It owns the trigger, search box, option typography, and right-aligned facet counts so pages do not hand-roll their own menu rows.',
      },
    },
  },
} satisfies Meta<typeof SearchableFacetSelect>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {
  render: () => <SearchableFacetSelectDemo />,
  play: async ({ canvasElement }) => {
    const trigger = canvasElement.querySelector('button[aria-label^="Key:"]') as HTMLButtonElement | null
    trigger?.click()
    await new Promise((resolve) => window.setTimeout(resolve, 50))

    const doc = canvasElement.ownerDocument
    const input = doc.querySelector('input[aria-label="Filter keys"]') as HTMLInputElement | null
    if (!input) {
      throw new Error('Expected searchable facet select to render its filter input.')
    }

    const valueSetter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set
    valueSetter?.call(input, 'J1')
    input.dispatchEvent(new Event('input', { bubbles: true }))
    await new Promise((resolve) => window.setTimeout(resolve, 50))

    const listbox = doc.querySelector('[role="listbox"][aria-label="Key"]')
    const text = listbox?.textContent ?? ''
    if (!text.includes('J1nW')) {
      throw new Error('Expected searchable facet select to keep matching options visible.')
    }
    if (text.includes('MZli')) {
      throw new Error('Expected searchable facet select to filter out non-matching options.')
    }
  },
}

export const SelectedValue: Story = {
  render: () => <SearchableFacetSelectDemo initialValue="MZli" />,
}

export const StateGallery: Story = {
  parameters: {
    docs: {
      disable: true,
    },
  },
  render: () => <GalleryStory />,
}
