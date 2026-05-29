import type { Meta, StoryObj } from '@storybook/react'

import type { AdminRechargeListResponse } from '../api'
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

const meta = {
  title: 'Admin/RechargeRecordsModule',
  component: AdminRechargeRecordsModule,
  parameters: {
    layout: 'fullscreen',
  },
} satisfies Meta<typeof AdminRechargeRecordsModule>

export default meta

type Story = StoryObj<typeof meta>

export const Flat: Story = {
  render: () => <AdminRechargeRecordsModule initialData={rechargeData} disableAutoLoad />,
}

export const Grouped: Story = {
  render: () => <AdminRechargeRecordsModule initialData={{ ...rechargeData, items: [] }} disableAutoLoad />,
  play: async ({ canvasElement }) => {
    const groupedButton = Array.from(canvasElement.querySelectorAll<HTMLButtonElement>('button')).find(
      (button) => button.textContent === 'By user' || button.textContent === '按用户',
    )
    groupedButton?.click()
  },
}

export const EmptyHiddenModule: Story = {
  render: () => <AdminRechargeRecordsModule initialData={emptyData} disableAutoLoad />,
}
