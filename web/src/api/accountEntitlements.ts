import { normalizeAdminUserEntitlement } from './adminUserNormalization'
import { requestJson } from './runtime'

export type AccountEntitlementScopeKind = 'month' | 'permanent'

export interface AdminUserEntitlementDelta {
  businessCalls1hDelta: number
  dailyCreditsDelta: number
  monthlyCreditsDelta: number
}

export interface AdminUserEntitlement {
  id: number
  userId: string
  scopeKind: AccountEntitlementScopeKind
  monthStart: number
  businessCalls1hDelta: number
  dailyCreditsDelta: number
  monthlyCreditsDelta: number
  backendNote: string
  frontendNote: string
  sourceKind: string
  sourceId: string
  actorUserId: string | null
  actorDisplayName: string | null
  createdAt: number
}

export interface AdminUserEntitlements {
  currentMonthStart: number
  currentMonthDelta: AdminUserEntitlementDelta
  currentPermanentDelta: AdminUserEntitlementDelta
  items: AdminUserEntitlement[]
}

export interface CreateAdminUserEntitlementPayload {
  scopeKind: AccountEntitlementScopeKind
  monthStart?: number | null
  businessCalls1hDelta: number
  dailyCreditsDelta: number
  monthlyCreditsDelta: number
  backendNote: string
  frontendNote: string
}

export function fetchAdminUserEntitlements(
  id: string,
  filters: { scopeKind?: AccountEntitlementScopeKind | 'all'; startMonth?: number | null; endMonthBefore?: number | null } = {},
  signal?: AbortSignal,
): Promise<AdminUserEntitlement[]> {
  const encoded = encodeURIComponent(id)
  const params = new URLSearchParams()
  if (filters.scopeKind && filters.scopeKind !== 'all') params.set('scopeKind', filters.scopeKind)
  if (typeof filters.startMonth === 'number') params.set('startMonth', String(filters.startMonth))
  if (typeof filters.endMonthBefore === 'number') params.set('endMonthBefore', String(filters.endMonthBefore))
  const suffix = params.toString() ? `?${params.toString()}` : ''
  return requestJson<{ items?: unknown[] }>(`/api/users/${encoded}/entitlements${suffix}`, { signal })
    .then((payload) => Array.isArray(payload.items) ? payload.items.map(normalizeAdminUserEntitlement) : [])
}

export function createAdminUserEntitlement(
  id: string,
  payload: CreateAdminUserEntitlementPayload,
): Promise<AdminUserEntitlement> {
  const encoded = encodeURIComponent(id)
  return requestJson<unknown>(`/api/users/${encoded}/entitlements`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  }).then(normalizeAdminUserEntitlement)
}
