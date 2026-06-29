import '../../test/happydom'

import { afterEach, describe, expect, it, mock } from 'bun:test'
import { act } from 'react'
import { createRoot } from 'react-dom/client'

import type { AdminRechargeListResponse, AdminTotpStatus } from '../api'
import { LanguageProvider, translations } from '../i18n'
import {
  AdminRechargeRefundDialogBody,
  refundErrorMessage,
  type RefundKind,
} from './AdminRechargeRecordsModule'

const now = Math.floor(Date.now() / 1000)

const rechargeData: AdminRechargeListResponse = {
  hasRechargeOrders: true,
  total: 1,
  page: 1,
  perPage: 25,
  items: [
    {
      outTradeNo: 'ldc_test_paid_001',
      user: { id: 'usr_ivan', displayName: 'Ivan Li', username: 'ivan', avatarTemplate: null },
      status: 'paid',
      credits: 1000,
      months: 1,
      moneyCents: 5000,
      money: '50.00',
      quoteMonthStart: now - 120,
      finalMoneyCents: 5000,
      finalHourlyDelta: 20,
      finalDailyDelta: 100,
      finalMonthlyDelta: 1000,
      monthEndClampApplied: false,
      tradeNo: 'linuxdo-trade-001',
      paymentUrl: null,
      orderName: 'Tavily Hikari 1000 credits x 1 month(s)',
      createdAt: now - 120,
      updatedAt: now - 60,
      paidAt: now - 60,
      refundedAt: null,
      refundActor: null,
      lastNotifyAt: now - 60,
      lastError: null,
    },
  ],
  groups: [],
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

function findButtonWithin(root: ParentNode, labels: string[]): HTMLButtonElement {
  const button = Array.from(root.querySelectorAll<HTMLButtonElement>('button')).find((candidate) =>
    labels.some((label) => candidate.textContent?.includes(label)),
  )
  expect(button).not.toBeNull()
  return button!
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('AdminRechargeRecordsModule refund TOTP feedback', () => {
  function renderRefundDialog(options: {
    kind?: RefundKind
    totpStatus: AdminTotpStatus | null
    totpCode?: string
    refundBusy?: boolean
    refundError?: string | null
    totpStatusLoading?: boolean
    totpStatusError?: string | null
    onOpenSystemSettings?: () => void
  }) {
    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)
    act(() => {
      root.render(
        <LanguageProvider initialLanguage="zh">
          <AdminRechargeRefundDialogBody
            refundTarget={{ order: rechargeData.items[0], kind: options.kind ?? 'refund' }}
            totpStatus={options.totpStatus}
            totpCode={options.totpCode ?? ''}
            refundBusy={options.refundBusy ?? false}
            refundError={options.refundError ?? null}
            totpStatusLoading={options.totpStatusLoading ?? false}
            totpStatusError={options.totpStatusError ?? null}
            onTotpCodeChange={() => {}}
            onClose={() => {}}
            onExecuteRefund={() => {}}
            onOpenSystemSettings={options.onOpenSystemSettings ?? (() => {})}
            chrome="plain"
          />
        </LanguageProvider>,
      )
    })
    return { container, root }
  }

  it('shows setup guidance instead of a TOTP input when admin TOTP is unbound', async () => {
    const onOpenSystemSettings = mock(() => {})
    const { container, root } = renderRefundDialog({
      totpStatus: { ...boundTotpStatus, enabled: false },
      onOpenSystemSettings,
    })

    expect(container.textContent).toContain('请先绑定管理端 TOTP')
    expect(container.querySelector('#admin-recharge-refund-totp')).toBeNull()

    await act(async () => {
      findButtonWithin(container, ['去系统设置', 'Open settings']).click()
    })
    expect(onOpenSystemSettings).toHaveBeenCalledTimes(1)

    await act(async () => root.unmount())
  })

  it('blocks refund submission while admin TOTP status is unknown', async () => {
    const { container, root } = renderRefundDialog({
      totpStatus: null,
      totpStatusLoading: true,
    })

    expect(container.textContent).toContain('正在确认管理端 TOTP')
    expect(container.textContent).toContain('正在读取管理端 TOTP 状态')
    expect(container.querySelector('#admin-recharge-refund-totp')).toBeNull()
    const confirmButton = Array.from(container.querySelectorAll<HTMLButtonElement>('button')).find((button) =>
      ['确认', 'Confirm'].some((label) => button.textContent?.includes(label)),
    )
    expect(confirmButton).toBeUndefined()

    await act(async () => root.unmount())
  })

  it('blocks refund submission when admin TOTP is unavailable', async () => {
    const { container, root } = renderRefundDialog({
      totpStatus: { ...boundTotpStatus, available: false },
      totpCode: '123456',
    })

    expect(container.textContent).toContain('管理端 TOTP 当前不可用')
    expect(container.textContent).toContain('请先恢复服务端 TOTP 配置')
    expect(container.querySelector('#admin-recharge-refund-totp')).toBeNull()
    const confirmButton = Array.from(container.querySelectorAll<HTMLButtonElement>('button')).find((button) =>
      ['确认', 'Confirm'].some((label) => button.textContent?.includes(label)),
    )
    expect(confirmButton).toBeUndefined()

    await act(async () => root.unmount())
  })

  it('keeps the refund dialog open and renders backend failure text', async () => {
    const { container, root } = renderRefundDialog({
      totpStatus: boundTotpStatus,
      totpCode: '123431',
      refundError: refundErrorMessage(
        'admin TOTP is not bound',
        translations.zh.admin.recharges,
      ),
    })

    const codeInput = container.querySelector<HTMLInputElement>('#admin-recharge-refund-totp')
    expect(codeInput).not.toBeNull()
    expect(codeInput!.value).toBe('123431')
    expect(container.textContent).toContain('管理端 TOTP 尚未绑定')
    expect(findButtonWithin(container, ['确认', 'Confirm']).disabled).toBe(false)

    await act(async () => root.unmount())
  })
})
