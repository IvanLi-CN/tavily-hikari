import { useMemo } from 'react'

import type { AdminUserDetail } from '../api'
import type { AdminRechargeTranslations } from '../i18n/adminRechargeTranslationTypes'

interface UserRechargeQuotaCalendarProps {
  detail: AdminUserDetail
  strings: AdminRechargeTranslations['userDetail']
  language: 'en' | 'zh'
  formatNumber: (value: number) => string
  embedded?: boolean
}

interface MonthQuotaRow {
  monthStart: number
  recharge: number
}

export function UserRechargeQuotaCalendar({
  detail,
  strings,
  language,
  formatNumber,
  embedded = false,
}: UserRechargeQuotaCalendarProps): JSX.Element {
  const entitlements = detail.recharge?.entitlements ?? []
  const rows = useMemo(() => buildRechargeMonthRows(entitlements), [entitlements])
  const tagDelta = detail.quotaBreakdown
    .filter((item) => item.kind === 'tag')
    .reduce((sum, item) => sum + item.monthlyDelta, 0)
  const locale = language === 'zh' ? 'zh-CN' : 'en-US'
  const currentRecharge = detail.recharge?.currentMonthEntitlementCredits
    ?? rows.find((row) => isSameLocalMonth(row.monthStart, Date.now() / 1000))?.recharge
    ?? 0
  const effectiveUntil = detail.recharge?.effectiveUntilMonthStart
  const tableFacts = [
    formatTemplate(strings.currentMonthRecharge, { value: formatNumber(currentRecharge) }),
    formatTemplate(strings.currentFinal, { value: formatNumber(detail.quotaMonthlyLimit) }),
    effectiveUntil
      ? formatTemplate(strings.effectiveUntil, { value: formatMonth(effectiveUntil, locale) })
      : strings.effectiveUntilEmpty,
  ]
  const Heading = embedded ? 'h3' : 'h2'

  return (
    <section className={embedded ? 'user-recharge-quota-calendar-panel user-recharge-quota-calendar-panel--embedded' : 'surface panel user-recharge-quota-calendar-panel'}>
      <div className="panel-header">
        <div>
          <Heading>{strings.title}</Heading>
          <p className="panel-description">{strings.description}</p>
        </div>
      </div>

      {rows.length === 0 ? (
        <div className="empty-state alert">{strings.empty}</div>
      ) : (
        <>
          <div className="admin-recharge-quota-table-facts" aria-label={strings.title}>
            {tableFacts.map((fact) => <span key={fact}>{fact}</span>)}
          </div>
          <div className="table-scroll-shell admin-recharge-quota-table-scroll" data-table-density="compact">
            <table className="admin-recharge-quota-table" data-table-density="compact">
            <thead>
              <tr>
                <th scope="col">{strings.monthColumn}</th>
                <th scope="col">{strings.baseColumn}</th>
                <th scope="col">{strings.tagColumn}</th>
                <th scope="col">{strings.rechargeColumn}</th>
                <th scope="col">{strings.finalColumn}</th>
                <th scope="col">{strings.usedColumn}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr key={row.monthStart}>
                  <th scope="row">{formatMonth(row.monthStart, locale)}</th>
                  <td>{formatNumber(detail.quotaBase.monthlyLimit)}</td>
                  <td>{formatNumber(tagDelta)}</td>
                  <td>{formatNumber(row.recharge)}</td>
                  <td>{formatNumber(detail.quotaBase.monthlyLimit + tagDelta + row.recharge)}</td>
                  <td>{formatNumber(detail.quotaMonthlyUsed)}</td>
                </tr>
              ))}
            </tbody>
            </table>
          </div>
        </>
      )}
    </section>
  )
}

function buildRechargeMonthRows(entitlements: AdminUserDetail['recharge']['entitlements']): MonthQuotaRow[] {
  if (entitlements.length === 0) return []
  const totals = new Map<number, number>()
  for (const entitlement of entitlements) {
    totals.set(entitlement.monthStart, (totals.get(entitlement.monthStart) ?? 0) + entitlement.credits)
  }
  const starts = [...totals.keys()].sort((a, b) => a - b)
  const first = addLocalMonths(starts[0], -1)
  const last = addLocalMonths(starts[starts.length - 1], 1)
  const rows: MonthQuotaRow[] = []
  for (let cursor = first; cursor <= last; cursor = addLocalMonths(cursor, 1)) {
    rows.push({ monthStart: cursor, recharge: totals.get(cursor) ?? 0 })
  }
  return rows
}

function addLocalMonths(ts: number, months: number): number {
  const date = new Date(ts * 1000)
  date.setMonth(date.getMonth() + months)
  return Math.floor(date.getTime() / 1000)
}

function formatMonth(ts: number, locale: string): string {
  return new Date(ts * 1000).toLocaleDateString(locale, { year: 'numeric', month: 'short' })
}

function isSameLocalMonth(leftTs: number, rightTs: number): boolean {
  const left = new Date(leftTs * 1000)
  const right = new Date(rightTs * 1000)
  return left.getFullYear() === right.getFullYear() && left.getMonth() === right.getMonth()
}

function formatTemplate(template: string, values: Record<string, string | number>): string {
  return Object.entries(values).reduce(
    (current, [key, value]) => current.replaceAll(`{${key}}`, String(value)),
    template,
  )
}
