import type { Meta, StoryObj } from '@storybook/react-vite'
import { useEffect, useState } from 'react'

import RollingNumber from './RollingNumber'

const storyFormatter = new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 })

function finalDigitGroup(value: number): string {
  return String(Math.abs(value) % 1000).padStart(3, '0')
}

function RollingNumberDeltaDemo(props: { from: number; to: number; delay?: number; note: string }): JSX.Element {
  const [value, setValue] = useState<number>(props.from)

  useEffect(() => {
    const timer = window.setTimeout(() => setValue(props.to), props.delay ?? 600)
    return () => window.clearTimeout(timer)
  }, [props.delay, props.to])

  return (
    <div style={{
      minWidth: 420,
      borderRadius: 28,
      border: '1px solid hsl(var(--border) / 0.68)',
      background: 'hsl(var(--card) / 0.92)',
      boxShadow: '0 22px 54px hsl(var(--foreground) / 0.12), inset 0 1px 0 hsl(var(--background) / 0.9)',
      color: 'hsl(var(--foreground))',
      padding: '24px 28px 22px',
      textAlign: 'left',
    }}>
      <div style={{
        color: 'hsl(var(--muted-foreground))',
        fontSize: 13,
        fontWeight: 800,
        letterSpacing: '0.08em',
        textTransform: 'uppercase',
      }}>
        Suffix-only rolling proof
      </div>
      <div style={{
        marginTop: 12,
        color: 'hsl(var(--muted-foreground))',
        fontSize: 16,
        fontWeight: 700,
      }}>
        {storyFormatter.format(props.from)} {'->'} {storyFormatter.format(props.to)}
      </div>
      <div style={{
        marginTop: 8,
        fontFamily: 'Nunito, var(--font-sans, sans-serif)',
        fontSize: 58,
        fontWeight: 900,
        letterSpacing: '0.02em',
        lineHeight: 1,
      }}>
        <RollingNumber value={value} />
      </div>
      <div style={{
        marginTop: 14,
        color: 'hsl(var(--muted-foreground))',
        fontSize: 14,
        fontWeight: 700,
      }}>
        {props.note}: {finalDigitGroup(props.from)} {'->'} {finalDigitGroup(props.to)}
      </div>
    </div>
  )
}

const meta = {
  title: 'Components/RollingNumber',
  component: RollingNumber,
  tags: ['autodocs'],
  args: {
    value: 123_456,
    loading: false,
  },
  parameters: {
    layout: 'centered',
    docs: {
      description: {
        component:
          'Shared rolling-number primitive used across dashboard, public-home, and user-console metric surfaces. Only the rightmost three-digit group animates; higher groups update statically.',
      },
    },
  },
} satisfies Meta<typeof RollingNumber>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {}

export const Loading: Story = {
  args: { loading: true, value: null },
}

export const Empty: Story = {
  args: { value: null },
}

export const CarryChainSuffixOnly: Story = {
  render: () => <RollingNumberDeltaDemo from={65_777} to={66_876} note="Animated suffix" />,
  parameters: {
    docs: {
      description: {
        story:
          'Only the final three digits animate downward. The left group updates from 65 to 66 without rolling.',
      },
    },
  },
}

export const BorrowChainSuffixOnly: Story = {
  render: () => <RollingNumberDeltaDemo from={66_876} to={65_777} note="Animated suffix" />,
  parameters: {
    docs: {
      description: {
        story:
          'Only the final three digits animate upward. Equal higher-order digits remain static even when the left group changes.',
      },
    },
  },
}

export const EqualDigitFullCycle: Story = {
  render: () => <RollingNumberDeltaDemo from={129} to={130} note="Carry suffix" />,
  parameters: {
    docs: {
      description: {
        story:
          'The tens digit rolls a full cycle from 2 back to 3 only when it belongs to the animated suffix caused by the units carry.',
      },
    },
  },
}

export const GroupBoundaryJump: Story = {
  render: () => <RollingNumberDeltaDemo from={999} to={1_000} note="Rightmost group" />,
  parameters: {
    docs: {
      description: {
        story:
          'Crossing a comma boundary still keeps motion scoped to the rightmost group. The new thousands digit appears statically.',
      },
    },
  },
}
