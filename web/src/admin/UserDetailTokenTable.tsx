import { Icon } from '../lib/icons'
import type { AdminUserTokenSummary } from '../api'
import type { AdminTranslations } from '../i18n'
import { StatusBadge } from '../components/StatusBadge'
import { Button } from '../components/ui/button'
import { Table } from '../components/ui/table'

interface UserDetailTokenTableProps {
  tokens: AdminUserTokenSummary[]
  usersStrings: AdminTranslations['users']
  formatNumber: (value: number) => string
  formatTimestamp: (value: number | null | undefined) => string
  onViewToken: (tokenId: string) => void
}

export function UserDetailTokenTable({
  tokens,
  usersStrings,
  formatNumber,
  formatTimestamp,
  onViewToken,
}: UserDetailTokenTableProps): JSX.Element {
  if (tokens.length === 0) {
    return <div className="empty-state alert">{usersStrings.empty.noTokens}</div>
  }

  return (
    <>
      <div className="admin-responsive-up">
        <Table className="jobs-table admin-users-table admin-user-tokens-table">
          <thead>
            <tr>
              <th>{`${usersStrings.tokens.table.id} · ${usersStrings.tokens.table.note}`}</th>
              <th>{`${usersStrings.tokens.table.status} · ${usersStrings.tokens.table.lastUsed}`}</th>
              <th>{`${usersStrings.tokens.table.totalRequests} · ${usersStrings.tokens.table.createdAt}`}</th>
              <th>{`${usersStrings.tokens.table.successDaily} · ${usersStrings.tokens.table.successMonthly}`}</th>
              <th>{usersStrings.tokens.table.actions}</th>
            </tr>
          </thead>
          <tbody>
            {tokens.map((token) => {
              const successDailyText = `${formatNumber(token.dailySuccess)} / ${formatNumber(token.dailyFailure)}`
              return (
                <tr key={token.tokenId}>
                  <td>
                    <div className="token-compact-pair">
                      <div className="token-compact-field">
                        <code className="token-compact-value">{token.tokenId}</code>
                      </div>
                      <div className="token-compact-field">
                        <span className="token-compact-value">{token.note || '—'}</span>
                      </div>
                    </div>
                  </td>
                  <td>
                    <div className="token-compact-pair">
                      <div className="token-compact-field">
                        <StatusBadge tone={token.enabled ? 'success' : 'neutral'}>
                          {token.enabled ? usersStrings.status.enabled : usersStrings.status.disabled}
                        </StatusBadge>
                      </div>
                      <div className="token-compact-field">
                        <span className="token-compact-value">{formatTimestamp(token.lastUsedAt)}</span>
                      </div>
                    </div>
                  </td>
                  <td>
                    <div className="token-compact-pair">
                      <div className="token-compact-field">
                        <span className="token-compact-label">{usersStrings.tokens.table.totalRequests}</span>
                        <span className="token-compact-value">{formatNumber(token.totalRequests)}</span>
                      </div>
                      <div className="token-compact-field">
                        <span className="token-compact-label">{usersStrings.tokens.table.createdAt}</span>
                        <span className="token-compact-value">{formatTimestamp(token.createdAt)}</span>
                      </div>
                    </div>
                  </td>
                  <td>
                    <div className="token-compact-pair">
                      <div className="token-compact-field">
                        <span className="token-compact-label">{usersStrings.tokens.table.successDaily}</span>
                        <span className="token-compact-value">{successDailyText}</span>
                      </div>
                      <div className="token-compact-field">
                        <span className="token-compact-label">{usersStrings.tokens.table.successMonthly}</span>
                        <span className="token-compact-value">{formatNumber(token.monthlySuccess)}</span>
                      </div>
                    </div>
                  </td>
                  <td>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 rounded-full p-0 shadow-none"
                      title={usersStrings.tokens.actions.view}
                      aria-label={usersStrings.tokens.actions.view}
                      onClick={() => onViewToken(token.tokenId)}
                    >
                      <Icon icon="mdi:eye-outline" width={16} height={16} />
                    </Button>
                  </td>
                </tr>
              )
            })}
          </tbody>
        </Table>
      </div>

      <div className="admin-mobile-list admin-responsive-down">
        {tokens.map((token) => {
          const successDailyText = `${formatNumber(token.dailySuccess)} / ${formatNumber(token.dailyFailure)}`
          return (
            <article key={token.tokenId} className="admin-mobile-card admin-user-token-card">
              <div className="admin-user-mobile-card-head">
                <div className="admin-mobile-identity-block admin-user-mobile-identity">
                  <span className="admin-mobile-identity-label">{usersStrings.tokens.table.id}</span>
                  <div className="panel-description admin-mobile-identity-meta admin-user-token-meta">
                    <code className="admin-user-detail-mobile-code">{token.tokenId}</code>
                    <div className="admin-user-token-summary-row">
                      <span className="admin-user-mobile-note">{token.note || '—'}</span>
                      <StatusBadge
                        tone={token.enabled ? 'success' : 'neutral'}
                        className="admin-user-token-status-badge"
                      >
                        {token.enabled ? usersStrings.status.enabled : usersStrings.status.disabled}
                      </StatusBadge>
                    </div>
                  </div>
                </div>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 rounded-full p-0 shadow-none admin-user-mobile-card-action"
                  title={usersStrings.tokens.actions.view}
                  aria-label={usersStrings.tokens.actions.view}
                  onClick={() => onViewToken(token.tokenId)}
                >
                  <Icon icon="mdi:eye-outline" width={16} height={16} />
                </Button>
              </div>

              <div className="admin-user-mobile-metric-grid admin-user-token-metric-grid">
                <div className="admin-user-mobile-metric-card admin-user-mobile-metric-card--span-2">
                  <span className="admin-user-mobile-metric-label">{usersStrings.tokens.table.lastUsed}</span>
                  <strong>{formatTimestamp(token.lastUsedAt)}</strong>
                </div>
                <div className="admin-user-mobile-metric-card">
                  <span className="admin-user-mobile-metric-label">{usersStrings.tokens.table.totalRequests}</span>
                  <strong>{formatNumber(token.totalRequests)}</strong>
                </div>
                <div className="admin-user-mobile-metric-card">
                  <span className="admin-user-mobile-metric-label">{usersStrings.tokens.table.successMonthly}</span>
                  <strong>{formatNumber(token.monthlySuccess)}</strong>
                </div>
                <div className="admin-user-mobile-metric-card admin-user-mobile-metric-card--span-2">
                  <span className="admin-user-mobile-metric-label">{usersStrings.tokens.table.successDaily}</span>
                  <strong>{successDailyText}</strong>
                </div>
                <div className="admin-user-mobile-metric-card admin-user-mobile-metric-card--span-2">
                  <span className="admin-user-mobile-metric-label">{usersStrings.tokens.table.createdAt}</span>
                  <strong>{formatTimestamp(token.createdAt)}</strong>
                </div>
              </div>
            </article>
          )
        })}
      </div>
    </>
  )
}
