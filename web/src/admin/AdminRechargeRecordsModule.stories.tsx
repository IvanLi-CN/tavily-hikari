import type { Meta, StoryObj } from '@storybook/react'

import { LanguageProvider } from '../i18n'
import AdminRechargeRecordsModule from './AdminRechargeRecordsModule'
import {
  boundRechargeTotpStatus,
  emptyRechargeStoryData,
  rechargeStoryData,
  unboundRechargeTotpStatus,
} from './storySupport/rechargeStoryData'

const meta = {
  title: 'Admin/RechargeRecordsModule',
  component: AdminRechargeRecordsModule,
  tags: ['autodocs'],
  parameters: {
    layout: 'fullscreen',
    docs: {
      description: {
        component: 'Recharge records states cover bound TOTP, unbound setup guidance, grouped records, empty records, and refund failure feedback.',
      },
    },
  },
  decorators: [
    (Story) => (
      <LanguageProvider initialLanguage="zh">
        <Story />
      </LanguageProvider>
    ),
  ],
} satisfies Meta<typeof AdminRechargeRecordsModule>

export default meta

type Story = StoryObj<typeof meta>

export const Flat: Story = {
  render: () => <AdminRechargeRecordsModule initialData={rechargeStoryData} initialTotpStatus={boundRechargeTotpStatus} disableAutoLoad />,
}

export const LifecycleStates: Story = {
  render: () => <AdminRechargeRecordsModule initialData={rechargeStoryData} initialTotpStatus={boundRechargeTotpStatus} disableAutoLoad />,
}

export const Grouped: Story = {
  render: () => <AdminRechargeRecordsModule initialData={{ ...rechargeStoryData, items: [] }} initialTotpStatus={boundRechargeTotpStatus} disableAutoLoad />,
  play: async ({ canvasElement }) => {
    const groupedButton = Array.from(canvasElement.querySelectorAll<HTMLButtonElement>('button')).find(
      (button) => button.textContent === 'By user' || button.textContent === '按用户',
    )
    groupedButton?.click()
  },
}

export const UnboundTotpPrompt: Story = {
  render: () => <AdminRechargeRecordsModule initialData={rechargeStoryData} initialTotpStatus={unboundRechargeTotpStatus} disableAutoLoad />,
  play: async ({ canvasElement }) => {
    const refundButton = findButton(canvasElement.ownerDocument, ['Cancel order', '退单'])
    refundButton?.click()
    await waitForMicrotasks()
    const text = canvasElement.ownerDocument.body.textContent ?? ''
    if (!text.includes('Bind admin TOTP first') && !text.includes('请先绑定管理端 TOTP')) {
      throw new Error('Unbound TOTP prompt did not open')
    }
    if (canvasElement.ownerDocument.querySelector('#admin-recharge-refund-totp') != null) {
      throw new Error('Unbound TOTP prompt should not show the refund code input')
    }
  },
}

export const BoundTotpConfirmation: Story = {
  render: () => <AdminRechargeRecordsModule initialData={rechargeStoryData} initialTotpStatus={boundRechargeTotpStatus} disableAutoLoad />,
  play: async ({ canvasElement }) => {
    const refundButton = findButton(canvasElement.ownerDocument, ['Cancel order', '退单'])
    refundButton?.click()
    await waitForMicrotasks()
    const text = canvasElement.ownerDocument.body.textContent ?? ''
    if (!text.includes('确认退单') && !text.includes('Confirm order cancellation')) {
      throw new Error('Bound TOTP confirmation dialog did not open')
    }
    if (canvasElement.ownerDocument.querySelector('#admin-recharge-refund-totp') == null) {
      throw new Error('Bound TOTP confirmation should show the refund code input')
    }
  },
}

export const RefundFailureFeedback: Story = {
  render: () => <AdminRechargeRecordsModule initialData={rechargeStoryData} initialTotpStatus={boundRechargeTotpStatus} disableAutoLoad />,
  play: async ({ canvasElement }) => {
    const originalFetch = globalThis.fetch
    globalThis.fetch = ((input: RequestInfo | URL) => {
      const path = String(input)
      if (path.includes('/api/admin/recharges/') && path.endsWith('/refund')) {
        return Promise.resolve(new Response('admin TOTP is not bound', { status: 403 }))
      }
      return originalFetch(input)
    }) as typeof fetch
    try {
      const refundButton = findButton(canvasElement.ownerDocument, ['Cancel order', '退单'])
      refundButton?.click()
      await waitForMicrotasks()
      const codeInput = canvasElement.ownerDocument.querySelector<HTMLInputElement>('#admin-recharge-refund-totp')
      if (!codeInput) throw new Error('Refund TOTP input did not open')
      const valueSetter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set
      valueSetter?.call(codeInput, '123431')
      codeInput.dispatchEvent(new Event('input', { bubbles: true }))
      codeInput.dispatchEvent(new Event('change', { bubbles: true }))
      await waitForMicrotasks()
      findButton(canvasElement.ownerDocument, ['Confirm', '确认'])?.click()
      await waitForMicrotasks()
      const text = canvasElement.ownerDocument.body.textContent ?? ''
      if (!text.includes('Admin TOTP is not bound') && !text.includes('管理端 TOTP 尚未绑定')) {
        throw new Error('Refund failure feedback did not render')
      }
    } finally {
      globalThis.fetch = originalFetch
    }
  },
}

export const ExpiredClampVisible: Story = {
  render: () => <AdminRechargeRecordsModule initialData={rechargeStoryData} initialTotpStatus={boundRechargeTotpStatus} disableAutoLoad />,
  parameters: {
    docs: {
      description: {
        story: 'Expired unpaid orders stay visible with the expired status and final-value / clamp marker for month-end quote drift.',
      },
    },
  },
}

export const EmptyHiddenModule: Story = {
  render: () => <AdminRechargeRecordsModule initialData={emptyRechargeStoryData} initialTotpStatus={boundRechargeTotpStatus} disableAutoLoad />,
}

function findButton(root: Document | HTMLElement, labels: string[]): HTMLButtonElement | null {
  return Array.from(root.querySelectorAll<HTMLButtonElement>('button')).find((button) =>
    labels.some((label) => button.textContent?.includes(label)),
  ) ?? null
}

async function waitForMicrotasks(): Promise<void> {
  await Promise.resolve()
  await new Promise<void>((resolve) => setTimeout(resolve, 0))
}
