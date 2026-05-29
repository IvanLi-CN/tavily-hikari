import { requestJson, type SortDirection } from './runtime'

export type AdminRechargeStatus = 'pending' | 'paid' | 'failed' | 'refunding' | 'refunded' | 'refundOnly'
export type AdminRechargeSort = 'createdAt' | 'paidAt' | 'refundedAt' | 'status'
export type AdminRechargeViewMode = 'flat' | 'user'

export interface AdminRechargeUser {
  id: string
  displayName: string | null
  username: string | null
  avatarTemplate: string | null
}

export interface AdminRechargeOrder {
  outTradeNo: string
  user: AdminRechargeUser
  status: AdminRechargeStatus
  credits: number
  months: number
  moneyCents: number
  money: string
  tradeNo: string | null
  paymentUrl: string | null
  orderName: string
  createdAt: number
  updatedAt: number
  paidAt: number | null
  refundedAt: number | null
  refundActor: string | null
  lastNotifyAt: number | null
  lastError: string | null
}

export interface AdminRechargeUserGroup {
  user: AdminRechargeUser
  orderCount: number
  paidOrderCount: number
  refundedOrderCount: number
  totalCredits: number
  totalMoneyCents: number
  latestOrderCreatedAt: number
  latestPaidAt: number | null
  latestRefundedAt: number | null
}

export interface AdminRechargeListResponse {
  hasRechargeOrders: boolean
  items: AdminRechargeOrder[]
  groups: AdminRechargeUserGroup[]
  total: number
  page: number
  perPage: number
}

export interface AdminRechargeListParams {
  user?: string
  status?: AdminRechargeStatus | 'all'
  startAt?: number
  endAt?: number
  sort?: AdminRechargeSort
  order?: SortDirection
  view?: AdminRechargeViewMode
  page?: number
  perPage?: number
}

export interface AdminTotpStatus {
  enabled: boolean
  available: boolean
  rechargeFeatureEnabled: boolean
  missingCryptoKey: boolean
  lockedUntil: number | null
  issuer: string
  accountName: string
}

export interface AdminTotpSetup {
  secret: string
  otpAuthUrl: string
  qrPngBase64: string
}

export function fetchAdminRecharges(params: AdminRechargeListParams = {}, signal?: AbortSignal): Promise<AdminRechargeListResponse> {
  const query = new URLSearchParams()
  if (params.user) query.set('user', params.user)
  if (params.status) query.set('status', params.status)
  if (params.startAt != null) query.set('startAt', String(params.startAt))
  if (params.endAt != null) query.set('endAt', String(params.endAt))
  if (params.sort) query.set('sort', params.sort)
  if (params.order) query.set('order', params.order)
  if (params.view) query.set('view', params.view)
  if (params.page != null) query.set('page', String(params.page))
  if (params.perPage != null) query.set('perPage', String(params.perPage))
  const suffix = query.toString() ? `?${query.toString()}` : ''
  return requestJson(`/api/admin/recharges${suffix}`, { signal })
}

export function refundAdminRecharge(outTradeNo: string, totpCode: string): Promise<AdminRechargeOrder> {
  return requestJson(`/api/admin/recharges/${encodeURIComponent(outTradeNo)}/refund`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ totpCode }),
  })
}

export function refundOnlyAdminRecharge(outTradeNo: string, totpCode: string): Promise<AdminRechargeOrder> {
  return requestJson(`/api/admin/recharges/${encodeURIComponent(outTradeNo)}/refund-only`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ totpCode }),
  })
}

export function fetchAdminTotpStatus(signal?: AbortSignal): Promise<AdminTotpStatus> {
  return requestJson('/api/admin/totp', { signal })
}

export function createAdminTotpSetup(): Promise<AdminTotpSetup> {
  return requestJson('/api/admin/totp/setup', { method: 'POST' })
}

export function confirmAdminTotp(secret: string, code: string): Promise<AdminTotpStatus> {
  return requestJson('/api/admin/totp/confirm', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ secret, code }),
  })
}

export function resetAdminTotp(currentCode: string, secret: string, code: string): Promise<AdminTotpStatus> {
  return requestJson('/api/admin/totp/reset', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ currentCode, secret, code }),
  })
}

export function disableAdminTotp(totpCode: string): Promise<AdminTotpStatus> {
  return requestJson('/api/admin/totp/disable', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ totpCode }),
  })
}
