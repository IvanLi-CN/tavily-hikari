import type { AdminUserQuotaBreakdownEntry } from '../api'
import type { AdminTranslations } from '../i18n'
import { StatusBadge, type StatusTone } from '../components/StatusBadge'

interface UserDetailQuotaBreakdownProps {
  entries: AdminUserQuotaBreakdownEntry[]
  usersStrings: AdminTranslations['users']
  formatQuotaLimitValue: (value: number) => string
  formatSignedQuotaDelta: (value: number) => string
}

interface BreakdownViewModel {
  breakdownLabel: string
  sourceLabel: string
  effectLabel: string
  effectTone: StatusTone
  isAbsoluteRow: boolean
}

function buildBreakdownViewModel(
  entry: AdminUserQuotaBreakdownEntry,
  usersStrings: AdminTranslations['users'],
): BreakdownViewModel {
  const isAbsoluteRow = entry.kind === 'base' || entry.kind === 'effective'
  const breakdownLabel =
    entry.kind === 'base'
      ? usersStrings.effectiveQuota.baseLabel
      : entry.kind === 'effective'
        ? usersStrings.effectiveQuota.effectiveLabel
        : entry.label
  const sourceLabel = entry.source
    ? entry.source === 'system_linuxdo'
      ? usersStrings.userTags.sourceSystem
      : usersStrings.userTags.sourceManual
    : '—'
  const effectLabel =
    entry.effectKind === 'block_all'
      ? usersStrings.catalog.effectKinds.blockAll
      : entry.effectKind === 'base'
        ? usersStrings.effectiveQuota.baseLabel
        : entry.kind === 'effective' || entry.effectKind === 'effective'
          ? usersStrings.effectiveQuota.effectiveLabel
          : usersStrings.catalog.effectKinds.quotaDelta

  return {
    breakdownLabel,
    sourceLabel,
    effectLabel,
    effectTone: entry.effectKind === 'block_all' ? 'error' : 'neutral',
    isAbsoluteRow,
  }
}

export function UserDetailQuotaBreakdown({
  entries,
  usersStrings,
  formatQuotaLimitValue,
  formatSignedQuotaDelta,
}: UserDetailQuotaBreakdownProps): JSX.Element {
  const formatBreakdownValue = (entry: AdminUserQuotaBreakdownEntry, isAbsoluteRow: boolean, value: number) =>
    isAbsoluteRow ? formatQuotaLimitValue(value) : formatSignedQuotaDelta(value)

  return (
    <>
      <div className="table-wrapper jobs-table-wrapper admin-responsive-up" style={{ marginTop: 12 }}>
        <table className="jobs-table admin-users-table user-tag-breakdown-table">
          <thead>
            <tr>
              <th>{usersStrings.effectiveQuota.columns.item}</th>
              <th>{usersStrings.effectiveQuota.columns.source}</th>
              <th>{usersStrings.effectiveQuota.columns.effect}</th>
              <th>{usersStrings.quota.hourly}</th>
              <th>{usersStrings.quota.daily}</th>
              <th>{usersStrings.quota.monthly}</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((entry, index) => {
              const view = buildBreakdownViewModel(entry, usersStrings)
              return (
                <tr key={`${entry.kind}:${entry.tagId ?? 'row'}:${index}`}>
                  <td>
                    <div className="token-compact-pair">
                      <div className="token-compact-field token-compact-field--wrap">
                        <span className="token-compact-value token-compact-value--wrap">{view.breakdownLabel}</span>
                      </div>
                      {entry.tagName && (
                        <div className="token-compact-field token-compact-field--wrap">
                          <code className="token-compact-value token-compact-value--wrap">{entry.tagName}</code>
                        </div>
                      )}
                    </div>
                  </td>
                  <td>{view.sourceLabel}</td>
                  <td>
                    <StatusBadge tone={view.effectTone}>{view.effectLabel}</StatusBadge>
                  </td>
                  <td>{formatBreakdownValue(entry, view.isAbsoluteRow, entry.hourlyDelta)}</td>
                  <td>{formatBreakdownValue(entry, view.isAbsoluteRow, entry.dailyDelta)}</td>
                  <td>{formatBreakdownValue(entry, view.isAbsoluteRow, entry.monthlyDelta)}</td>
                </tr>
              )
            })}
          </tbody>
        </table>
      </div>

      <div className="admin-mobile-list admin-responsive-down" style={{ marginTop: 12 }}>
        {entries.map((entry, index) => {
          const view = buildBreakdownViewModel(entry, usersStrings)
          return (
            <article className="admin-mobile-card admin-user-breakdown-card" key={`${entry.kind}:${entry.tagId ?? 'row'}:${index}`}>
              <div className="admin-user-mobile-card-head">
                <div className="admin-mobile-identity-block admin-user-mobile-identity">
                  <span className="admin-mobile-identity-label">{usersStrings.effectiveQuota.columns.item}</span>
                  <div className="panel-description admin-mobile-identity-meta admin-user-breakdown-meta">
                    <strong className="admin-user-breakdown-title">{view.breakdownLabel}</strong>
                    {entry.tagName ? <code className="admin-user-detail-mobile-code">{entry.tagName}</code> : null}
                  </div>
                </div>
                <StatusBadge tone={view.effectTone}>{view.effectLabel}</StatusBadge>
              </div>

              <div className="admin-user-mobile-chip-row">
                <div className="admin-user-mobile-chip admin-user-mobile-chip--wide">
                  <span className="admin-user-mobile-chip-label">{usersStrings.effectiveQuota.columns.source}</span>
                  <strong>{view.sourceLabel}</strong>
                </div>
              </div>

              <div className="admin-user-mobile-metric-grid admin-user-breakdown-metric-grid">
                <div className="admin-user-mobile-metric-card">
                  <span className="admin-user-mobile-metric-label">{usersStrings.quota.hourly}</span>
                  <strong>{formatBreakdownValue(entry, view.isAbsoluteRow, entry.hourlyDelta)}</strong>
                </div>
                <div className="admin-user-mobile-metric-card">
                  <span className="admin-user-mobile-metric-label">{usersStrings.quota.daily}</span>
                  <strong>{formatBreakdownValue(entry, view.isAbsoluteRow, entry.dailyDelta)}</strong>
                </div>
                <div className="admin-user-mobile-metric-card admin-user-mobile-metric-card--span-2">
                  <span className="admin-user-mobile-metric-label">{usersStrings.quota.monthly}</span>
                  <strong>{formatBreakdownValue(entry, view.isAbsoluteRow, entry.monthlyDelta)}</strong>
                </div>
              </div>
            </article>
          )
        })}
      </div>
    </>
  )
}
