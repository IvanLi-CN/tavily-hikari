import type { Meta, StoryObj } from '@storybook/react'

import type { AdminRechargeListResponse, AdminTotpStatus } from '../api'
import { LanguageProvider } from '../i18n'
import AdminRechargeRecordsModule from './AdminRechargeRecordsModule'

const now = Math.floor(Date.now() / 1000)

const rechargeData: AdminRechargeListResponse = {
  hasRechargeOrders: true,
  total: 3,
  page: 1,
  perPage: 25,
  items: [
    {
      outTradeNo: 'ldc_202605_0001',
      user: { id: 'usr_alice', displayName: 'Alice Chen', username: 'alice', avatarTemplate: null },
      status: 'paid',
      credits: 3000,
      months: 3,
      moneyCents: 45000,
      money: '450.00',
      quoteMonthStart: now - 86400 * 21,
      finalMoneyCents: 45000,
      finalHourlyDelta: 60,
      finalDailyDelta: 300,
      finalMonthlyDelta: 3000,
      monthEndClampApplied: false,
      tradeNo: 'linuxdo-trade-1001',
      paymentUrl: null,
      orderName: 'Tavily Hikari 3000 credits x 3 month(s)',
      createdAt: now - 86400 * 18,
      updatedAt: now - 86400 * 18 + 120,
      paidAt: now - 86400 * 18 + 120,
      refundedAt: null,
      refundActor: null,
      lastNotifyAt: now - 86400 * 18 + 120,
      lastError: null,
    },
    {
      outTradeNo: 'ldc_202605_0002',
      user: { id: 'usr_bob', displayName: 'Bob Lin', username: 'bob', avatarTemplate: null },
      status: 'refundOnly',
      credits: 1000,
      months: 1,
      moneyCents: 5000,
      money: '50.00',
      quoteMonthStart: now - 86400 * 9,
      finalMoneyCents: 5000,
      finalHourlyDelta: 20,
      finalDailyDelta: 100,
      finalMonthlyDelta: 1000,
      monthEndClampApplied: false,
      tradeNo: 'linuxdo-trade-1002',
      paymentUrl: null,
      orderName: 'Tavily Hikari 1000 credits x 1 month(s)',
      createdAt: now - 86400 * 8,
      updatedAt: now - 86400 * 7,
      paidAt: now - 86400 * 8 + 90,
      refundedAt: now - 86400 * 7,
      refundActor: 'builtin-admin',
      lastNotifyAt: now - 86400 * 8 + 90,
      lastError: null,
    },
    {
      outTradeNo: 'ldc_202605_0003',
      user: { id: 'usr_cora', displayName: 'Cora Wu', username: 'cora', avatarTemplate: null },
      status: 'expired',
      credits: 1000,
      months: 1,
      moneyCents: 5000,
      money: '30.00',
      quoteMonthStart: now - 86400 * 2,
      finalMoneyCents: 3000,
      finalHourlyDelta: 12,
      finalDailyDelta: 60,
      finalMonthlyDelta: 600,
      monthEndClampApplied: true,
      tradeNo: null,
      paymentUrl: null,
      orderName: 'Tavily Hikari 1000 credits x 1 month(s)',
      createdAt: now - 86400 * 2,
      updatedAt: now - 86400,
      paidAt: null,
      refundedAt: null,
      refundActor: null,
      lastNotifyAt: null,
      lastError: 'expired when month changed',
    },
  ],
  groups: [
    {
      user: { id: 'usr_alice', displayName: 'Alice Chen', username: 'alice', avatarTemplate: null },
      orderCount: 2,
      paidOrderCount: 2,
      refundedOrderCount: 0,
      totalCredits: 9000,
      totalMoneyCents: 45000,
      latestOrderCreatedAt: now - 86400 * 18,
      latestPaidAt: now - 86400 * 18 + 120,
      latestRefundedAt: null,
    },
    {
      user: { id: 'usr_bob', displayName: 'Bob Lin', username: 'bob', avatarTemplate: null },
      orderCount: 1,
      paidOrderCount: 0,
      refundedOrderCount: 1,
      totalCredits: 1000,
      totalMoneyCents: 5000,
      latestOrderCreatedAt: now - 86400 * 8,
      latestPaidAt: now - 86400 * 8 + 90,
      latestRefundedAt: now - 86400 * 7,
    },
    {
      user: { id: 'usr_cora', displayName: 'Cora Wu', username: 'cora', avatarTemplate: null },
      orderCount: 1,
      paidOrderCount: 0,
      refundedOrderCount: 0,
      totalCredits: 1000,
      totalMoneyCents: 3000,
      latestOrderCreatedAt: now - 86400 * 2,
      latestPaidAt: null,
      latestRefundedAt: null,
    },
  ],
}

const emptyData: AdminRechargeListResponse = {
  hasRechargeOrders: false,
  items: [],
  groups: [],
  total: 0,
  page: 1,
  perPage: 25,
}

const boundTotpStatus: AdminTotpStatus = {
  enabled: true,
  available: true,
  rechargeFeatureEnabled: true,
  missingCryptoKey: false,
  lockedUntil: null,
  issuer: 'Tavily Hikari',
  accountName: 'admin-recharge',
}

const unboundTotpStatus: AdminTotpStatus = {
  ...boundTotpStatus,
  enabled: false,
}

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
  render: () => <AdminRechargeRecordsModule initialData={rechargeData} initialTotpStatus={boundTotpStatus} disableAutoLoad />,
}

export const Grouped: Story = {
  render: () => <AdminRechargeRecordsModule initialData={{ ...rechargeData, items: [] }} initialTotpStatus={boundTotpStatus} disableAutoLoad />,
  play: async ({ canvasElement }) => {
    const groupedButton = Array.from(canvasElement.querySelectorAll<HTMLButtonElement>('button')).find(
      (button) => button.textContent === 'By user' || button.textContent === '按用户',
    )
    groupedButton?.click()
  },
}

export const UnboundTotpPrompt: Story = {
  render: () => <AdminRechargeRecordsModule initialData={rechargeData} initialTotpStatus={unboundTotpStatus} disableAutoLoad />,
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
  render: () => <AdminRechargeRecordsModule initialData={rechargeData} initialTotpStatus={boundTotpStatus} disableAutoLoad />,
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
  render: () => <AdminRechargeRecordsModule initialData={rechargeData} initialTotpStatus={boundTotpStatus} disableAutoLoad />,
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
  render: () => <AdminRechargeRecordsModule initialData={rechargeData} initialTotpStatus={boundTotpStatus} disableAutoLoad />,
  parameters: {
    docs: {
      description: {
        story: 'Expired unpaid orders stay visible with the expired status and final-value / clamp marker for month-end quote drift.',
      },
    },
  },
}

export const EmptyHiddenModule: Story = {
  render: () => <AdminRechargeRecordsModule initialData={emptyData} initialTotpStatus={boundTotpStatus} disableAutoLoad />,
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
