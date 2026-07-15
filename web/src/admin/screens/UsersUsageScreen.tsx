import type { ReactNode } from 'react'

import AdminLoadingRegion from '../../components/AdminLoadingRegion'
import AdminTableShell from '../../components/AdminTableShell'
import { StatusBadge } from '../../components/StatusBadge'
import type { AdminUserSummary, AdminUsersSortField, SortDirection } from '../../api'
import type { AdminTranslations } from '../../i18n'
import { formatRequestRateSummary, resolveRequestRate } from '../../requestRate'
import { buildShadowDailyUsageStack } from '../userUsageComparison'

import type { QueryLoadState } from '../queryLoadState'

import {
  AdminTableValueStack,
  AdminUsersSortableHeader,
  MonthlyBrokenCountTrigger,
  type StackedValue,
  UsagePageIntro,
} from './shared'

export interface UsersUsageScreenProps {
  users: AdminUserSummary[]
  language: 'en' | 'zh'
  usersStrings: AdminTranslations['users']
  showShadowDailyColumn: boolean
  searchControls: ReactNode
  filterStatusText?: string | null
  loadState: QueryLoadState
  loadingLabel: ReactNode
  errorLabel: ReactNode
  activeSortField: AdminUsersSortField | null
  activeSortOrder: SortDirection | null
  onToggleSort: (field: AdminUsersSortField) => void
  onOpenUser: (userId: string) => void
  onOpenMonthlyBrokenDrawer: (userId: string, label: string) => void
  formatNumber: (value: number) => string
  formatTimestamp: (value: number | null) => string
  formatQuotaUsagePair: (used: number, limit: number) => string
  formatQuotaStackValue: (used: number, limit: number) => StackedValue
  formatBusinessCalls1hStackValue: (
    success: number,
    failure: number,
    language: 'en' | 'zh',
  ) => StackedValue
  formatSuccessRateStackValue: (
    success: number,
    failure: number,
    language: 'en' | 'zh',
  ) => StackedValue
  formatCompactSuccessRateValue: (success: number, failure: number, language: 'en' | 'zh') => string
  formatStackedTimestamp: (value: number | null, language: 'en' | 'zh') => StackedValue
  formatAdminUserListPrimary: (
    user: Pick<AdminUserSummary, 'displayName' | 'username' | 'userId'>,
  ) => string
  formatAdminUserListMeta: (
    user: Pick<AdminUserSummary, 'displayName' | 'username'>,
  ) => string | null
  formatMonthlyBrokenStackValue: (count: number, limit: number) => StackedValue
  pagination?: ReactNode
}

export function UsersUsageScreen({
  users,
  language,
  usersStrings,
  showShadowDailyColumn,
  searchControls,
  filterStatusText,
  loadState,
  loadingLabel,
  errorLabel,
  activeSortField,
  activeSortOrder,
  onToggleSort,
  onOpenUser,
  onOpenMonthlyBrokenDrawer,
  formatNumber,
  formatTimestamp,
  formatQuotaUsagePair,
  formatQuotaStackValue,
  formatBusinessCalls1hStackValue,
  formatSuccessRateStackValue,
  formatCompactSuccessRateValue,
  formatStackedTimestamp,
  formatAdminUserListPrimary,
  formatAdminUserListMeta,
  formatMonthlyBrokenStackValue,
  pagination,
}: UsersUsageScreenProps): JSX.Element {
  const usageDailyRateLabel = language === 'zh' ? usersStrings.usage.table.dailySuccessRate : 'Daily'
  const usageMonthlyRateLabel = language === 'zh' ? usersStrings.usage.table.monthlySuccessRate : 'Monthly'

  return (
    <>
      <UsagePageIntro
        title={usersStrings.usage.title}
        description={usersStrings.usage.description}
        searchControls={searchControls}
        filterStatusText={filterStatusText}
      />

      <section className="surface panel">
        <AdminTableShell
          className="jobs-table-wrapper admin-users-usage-table-wrapper admin-responsive-up"
          tableClassName={`jobs-table admin-users-table admin-users-usage-table${showShadowDailyColumn ? ' admin-users-usage-table--shadow-compare' : ''}`}
          loadState={loadState}
          loadingLabel={loadingLabel}
          errorLabel={errorLabel}
          minHeight={360}
        >
          {users.length === 0 ? (
            <tbody>
              <tr>
                <td colSpan={showShadowDailyColumn ? 12 : 11}>
                  <div className="empty-state alert">{usersStrings.empty.none}</div>
                </td>
              </tr>
            </tbody>
          ) : (
            <>
              <thead>
                <tr>
                  <th>{usersStrings.usage.table.user}</th>
                  <th>{usersStrings.usage.table.status}</th>
                  <AdminUsersSortableHeader
                    label={usersStrings.usage.table.hourlyAny}
                    field="requestRateUsed"
                    activeField={activeSortField}
                    activeOrder={activeSortOrder}
                    onToggle={onToggleSort}
                  />
                  <AdminUsersSortableHeader
                    label={usersStrings.usage.table.businessOneHour}
                    field="businessCalls1hUsed"
                    activeField={activeSortField}
                    activeOrder={activeSortOrder}
                    onToggle={onToggleSort}
                  />
                  <AdminUsersSortableHeader
                    label={usersStrings.usage.table.daily}
                    field="dailyCreditsUsed"
                    activeField={activeSortField}
                    activeOrder={activeSortOrder}
                    onToggle={onToggleSort}
                  />
                  {showShadowDailyColumn ? <th>{usersStrings.usage.table.shadowDaily}</th> : null}
                  <AdminUsersSortableHeader
                    label={usersStrings.usage.table.monthly}
                    field="monthlyCreditsUsed"
                    activeField={activeSortField}
                    activeOrder={activeSortOrder}
                    onToggle={onToggleSort}
                  />
                  <AdminUsersSortableHeader
                    label={usersStrings.usage.table.monthlyBroken}
                    field="monthlyBrokenCount"
                    activeField={activeSortField}
                    activeOrder={activeSortOrder}
                    onToggle={onToggleSort}
                  />
                  <AdminUsersSortableHeader
                    label={usersStrings.usage.table.ipCount}
                    field="recentIpCount7d"
                    activeField={activeSortField}
                    activeOrder={activeSortOrder}
                    onToggle={onToggleSort}
                  />
                  <AdminUsersSortableHeader
                    label={usersStrings.usage.table.dailySuccessRate}
                    displayLabel={usageDailyRateLabel}
                    field="dailySuccessRate"
                    activeField={activeSortField}
                    activeOrder={activeSortOrder}
                    onToggle={onToggleSort}
                  />
                  <AdminUsersSortableHeader
                    label={usersStrings.usage.table.monthlySuccessRate}
                    displayLabel={usageMonthlyRateLabel}
                    field="monthlySuccessRate"
                    activeField={activeSortField}
                    activeOrder={activeSortOrder}
                    onToggle={onToggleSort}
                  />
                  <AdminUsersSortableHeader
                    label={usersStrings.usage.table.lastUsed}
                    field="lastActivity"
                    activeField={activeSortField}
                    activeOrder={activeSortOrder}
                    onToggle={onToggleSort}
                  />
                </tr>
              </thead>
              <tbody>
                {users.map((item) => {
                  const requestRate = resolveRequestRate(item, 'user')
                  const userLabel = item.displayName || item.username || item.userId
                  const userMeta = formatAdminUserListMeta(item)
                  const shadowDailyUsage = buildShadowDailyUsageStack({
                    actualUsed: item.dailyCreditsUsed,
                    shadowUsed: item.shadowDailyCreditsUsed,
                    limit: item.dailyCreditsLimit,
                    usersStrings,
                    formatNumber,
                    formatQuotaStackValue,
                  })
                  return (
                    <tr key={item.userId}>
                      <td className="admin-users-identity-cell">
                        <button
                          type="button"
                          className="link-button admin-users-identity-button"
                          aria-label={usersStrings.actions.view}
                          onClick={() => onOpenUser(item.userId)}
                        >
                          <strong>{formatAdminUserListPrimary(item)}</strong>
                        </button>
                        {userMeta ? (
                          <div className="panel-description admin-users-identity-meta">{userMeta}</div>
                        ) : null}
                      </td>
                      <td>
                        <StatusBadge tone={item.active ? 'success' : 'neutral'}>
                          {item.active ? usersStrings.status.active : usersStrings.status.inactive}
                        </StatusBadge>
                      </td>
                      <td className="admin-users-compact-cell">
                        <AdminTableValueStack {...formatQuotaStackValue(requestRate.used, requestRate.limit)} />
                      </td>
                      <td className="admin-users-compact-cell">
                        <AdminTableValueStack
                          primary={formatQuotaUsagePair(
                            item.businessCalls1h.totalCount,
                            item.businessCalls1h.limit,
                          )}
                          secondary={
                            language === 'zh'
                              ? `成 ${formatNumber(item.businessCalls1h.successCount)} / 败 ${formatNumber(item.businessCalls1h.failureCount)}`
                              : `S ${formatNumber(item.businessCalls1h.successCount)} / F ${formatNumber(item.businessCalls1h.failureCount)}`
                          }
                        />
                      </td>
                      <td className="admin-users-compact-cell">
                        <AdminTableValueStack {...formatQuotaStackValue(item.dailyCreditsUsed, item.dailyCreditsLimit)} />
                      </td>
                      {showShadowDailyColumn ? (
                        <td className="admin-users-compact-cell">
                          <AdminTableValueStack {...shadowDailyUsage} />
                        </td>
                      ) : null}
                      <td className="admin-users-compact-cell">
                        <AdminTableValueStack
                          {...formatQuotaStackValue(item.monthlyCreditsUsed, item.monthlyCreditsLimit)}
                        />
                      </td>
                      <td className="admin-users-compact-cell">
                        {(() => {
                          const metric = formatMonthlyBrokenStackValue(
                            item.monthlyBrokenCount,
                            item.monthlyBrokenLimit,
                          )
                          return (
                            <div className="admin-table-value-stack">
                              <MonthlyBrokenCountTrigger
                                count={item.monthlyBrokenCount}
                                onOpen={() => onOpenMonthlyBrokenDrawer(item.userId, userLabel)}
                                ariaLabel={usersStrings.brokenKeys.openDetails.replace('{label}', userLabel)}
                                className={metric.primaryClassName}
                              />
                              <span className="admin-table-value-secondary">{metric.secondary}</span>
                            </div>
                          )
                        })()}
                      </td>
                      <td className="admin-users-compact-cell">
                        <strong>{formatNumber(item.recentIpCount7d)}</strong>
                      </td>
                      <td className="admin-users-compact-cell">
                        <AdminTableValueStack
                          {...formatSuccessRateStackValue(item.dailySuccess, item.dailyFailure, language)}
                        />
                      </td>
                      <td className="admin-users-compact-cell">
                        <AdminTableValueStack
                          {...formatSuccessRateStackValue(item.monthlySuccess, item.monthlyFailure, language)}
                        />
                      </td>
                      <td className="admin-users-compact-cell">
                        <AdminTableValueStack {...formatStackedTimestamp(item.lastActivity, language)} />
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </>
          )}
        </AdminTableShell>

        <AdminLoadingRegion
          className="admin-mobile-list admin-responsive-down"
          loadState={loadState}
          loadingLabel={loadingLabel}
          errorLabel={errorLabel}
          minHeight={260}
        >
          {users.length === 0 ? (
            <div className="empty-state alert">{usersStrings.empty.none}</div>
          ) : (
            users.map((item) => {
              const requestRate = resolveRequestRate(item, 'user')
              const shadowDailyUsage = buildShadowDailyUsageStack({
                actualUsed: item.dailyCreditsUsed,
                shadowUsed: item.shadowDailyCreditsUsed,
                limit: item.dailyCreditsLimit,
                usersStrings,
                formatNumber,
                formatQuotaStackValue,
              })
              return (
                <article key={item.userId} className="admin-mobile-card">
                  <div className="admin-mobile-kv">
                    <span>{usersStrings.usage.table.user}</span>
                    <button
                      type="button"
                      className="link-button admin-users-mobile-link"
                      aria-label={usersStrings.actions.view}
                      onClick={() => onOpenUser(item.userId)}
                    >
                      <strong>{formatAdminUserListPrimary(item)}</strong>
                    </button>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{usersStrings.usage.table.status}</span>
                    <StatusBadge tone={item.active ? 'success' : 'neutral'}>
                      {item.active ? usersStrings.status.active : usersStrings.status.inactive}
                    </StatusBadge>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{formatRequestRateSummary(requestRate, language)}</span>
                    <strong>{formatQuotaUsagePair(requestRate.used, requestRate.limit)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{usersStrings.usage.table.businessOneHour}</span>
                    <strong>
                      {formatQuotaUsagePair(item.businessCalls1h.totalCount, item.businessCalls1h.limit)}
                      {' · '}
                      {language === 'zh'
                        ? `成 ${formatNumber(item.businessCalls1h.successCount)} / 败 ${formatNumber(item.businessCalls1h.failureCount)}`
                        : `S ${formatNumber(item.businessCalls1h.successCount)} / F ${formatNumber(item.businessCalls1h.failureCount)}`}
                    </strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{usersStrings.usage.table.daily}</span>
                    <AdminTableValueStack {...formatQuotaStackValue(item.dailyCreditsUsed, item.dailyCreditsLimit)} />
                  </div>
                  {showShadowDailyColumn ? (
                    <div className="admin-mobile-kv">
                      <span>{usersStrings.usage.table.shadowDaily}</span>
                      <AdminTableValueStack {...shadowDailyUsage} />
                    </div>
                  ) : null}
                  <div className="admin-mobile-kv">
                    <span>{usersStrings.usage.table.monthly}</span>
                    <strong>{formatQuotaUsagePair(item.monthlyCreditsUsed, item.monthlyCreditsLimit)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{usersStrings.usage.table.monthlyBroken}</span>
                    {item.monthlyBrokenCount > 0 ? (
                      <button
                        type="button"
                        className="link-button"
                        onClick={() =>
                          onOpenMonthlyBrokenDrawer(
                            item.userId,
                            item.displayName || item.username || item.userId,
                          )}
                      >
                        <strong>{formatQuotaUsagePair(item.monthlyBrokenCount, item.monthlyBrokenLimit)}</strong>
                      </button>
                    ) : (
                      <strong>{formatQuotaUsagePair(item.monthlyBrokenCount, item.monthlyBrokenLimit)}</strong>
                    )}
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{usersStrings.usage.table.ipCount}</span>
                    <strong>{formatNumber(item.recentIpCount7d)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{usersStrings.usage.table.dailySuccessRate}</span>
                    <strong>{formatCompactSuccessRateValue(item.dailySuccess, item.dailyFailure, language)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{usersStrings.usage.table.monthlySuccessRate}</span>
                    <strong>{formatCompactSuccessRateValue(item.monthlySuccess, item.monthlyFailure, language)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{usersStrings.usage.table.lastUsed}</span>
                    <strong>{formatTimestamp(item.lastActivity)}</strong>
                  </div>
                </article>
              )
            })
          )}
        </AdminLoadingRegion>

        {pagination}
      </section>
    </>
  )
}
