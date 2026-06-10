import { type PublicTokenLog } from '../api'
import { StatusBadge, type StatusTone } from '../components/StatusBadge'
import TokenLogsHeader, { type DetailLogsPushIssueCode, type UserTokenLogFilter } from './TokenLogsHeader'
import { type SegmentedTabsOption } from '../components/ui/SegmentedTabs'

interface TokenLogsPanelText {
  logs: string
  emptyLogs: string
  noError: string
  mobileOpen: string
  mobileSummary: string
  mobileAction: string
  logFilters: {
    ariaLabel: string
  }
  pushStatus: {
    ariaLabel: string
    browserUnsupported: string
    reconnecting: string
    closed: string
  }
  table: {
    request: string
    transport: string
    credits: string
    result: string
  }
}

interface TokenLogsPanelProps {
  logs: PublicTokenLog[]
  text: TokenLogsPanelText
  filter: UserTokenLogFilter
  filterOptions: ReadonlyArray<SegmentedTabsOption<UserTokenLogFilter>>
  filterDisabled: boolean
  pushIssue: DetailLogsPushIssueCode | null
  mode: 'detail' | 'full'
  onFilterChange: (filter: UserTokenLogFilter) => void
  onOpenFull: () => void
  formatTimestamp: (timestamp: number) => string
  formatLogCredits: (value: number | null | undefined) => string
  statusTone: (status: string) => StatusTone
}

export default function TokenLogsPanel({
  logs,
  text,
  filter,
  filterOptions,
  filterDisabled,
  pushIssue,
  mode,
  onFilterChange,
  onOpenFull,
  formatTimestamp,
  formatLogCredits,
  statusTone,
}: TokenLogsPanelProps): JSX.Element {
  return (
    <section className={`surface panel user-console-detail-panel user-console-logs-panel is-${mode}`}>
      <TokenLogsHeader
        title={text.logs}
        filter={filter}
        filterOptions={filterOptions}
        filterAriaLabel={text.logFilters.ariaLabel}
        filterDisabled={filterDisabled}
        pushIssue={pushIssue}
        pushStatusText={text.pushStatus}
        onFilterChange={onFilterChange}
      />
      {mode === 'detail' ? (
        <button
          type="button"
          className="user-console-mobile-log-entry user-console-md-down"
          aria-label={`${text.mobileOpen}，${text.mobileSummary}`}
          onClick={onOpenFull}
        >
          <span className="user-console-mobile-log-entry-content">
            <strong>{text.mobileOpen}</strong>
            <span>{text.mobileSummary}</span>
          </span>
          <span className="user-console-mobile-log-entry-action" aria-hidden="true">
            {text.mobileAction}
          </span>
        </button>
      ) : null}
      <div className={`table-wrapper user-console-md-up ${mode === 'detail' ? 'user-console-logs-table-scroll' : ''}`}>
        {logs.length === 0 ? (
          <div className="empty-state alert">{text.emptyLogs}</div>
        ) : (
          <table className="token-detail-table user-console-logs-table">
            <thead>
              <tr>
                <th>{text.table.request}</th>
                <th>{text.table.transport}</th>
                <th>{text.table.credits}</th>
                <th>{text.table.result}</th>
              </tr>
            </thead>
            <tbody>
              {logs.map((log) => (
                <tr key={log.id}>
                  <td>
                    <div className="user-console-log-stack">
                      <strong className="user-console-log-main">{formatTimestamp(log.created_at)}</strong>
                      <span className="user-console-log-meta">
                        {log.method} {log.path}
                        {log.query ? ` · ${log.query}` : ''}
                      </span>
                    </div>
                  </td>
                  <td>
                    <div className="user-console-log-transport">
                      <span className="user-console-log-transport-item">
                        <em>H</em>
                        <strong>{log.http_status ?? '—'}</strong>
                      </span>
                      <span className="user-console-log-transport-item">
                        <em>T</em>
                        <strong>{log.mcp_status ?? '—'}</strong>
                      </span>
                    </div>
                  </td>
                  <td className="user-console-log-credits">
                    {formatLogCredits(log.business_credits)}
                  </td>
                  <td>
                    <div className="user-console-log-result-line">
                      <StatusBadge className="user-console-log-status" tone={statusTone(log.result_status)}>
                        {log.result_status}
                      </StatusBadge>
                      <span className="user-console-log-error">{log.error_message ?? '—'}</span>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
      {mode === 'full' ? (
        <div className="user-console-mobile-list user-console-md-down">
          {logs.length === 0 ? (
            <div className="empty-state alert">{text.emptyLogs}</div>
          ) : (
            logs.map((log) => (
              <article key={log.id} className="user-console-mobile-card user-console-log-card">
                <header className="user-console-log-card-head">
                  <div className="user-console-log-card-request">
                    <strong>{log.method} {log.path}</strong>
                    {log.query ? <span>{log.query}</span> : null}
                  </div>
                  <StatusBadge className="user-console-mobile-status" tone={statusTone(log.result_status)}>
                    {log.result_status}
                  </StatusBadge>
                </header>
                <div className="user-console-log-card-meta">
                  <time dateTime={new Date(log.created_at * 1000).toISOString()}>{formatTimestamp(log.created_at)}</time>
                  <span>H {log.http_status ?? '—'}</span>
                  <span>T {log.mcp_status ?? '—'}</span>
                  <span>{text.table.credits} {formatLogCredits(log.business_credits)}</span>
                </div>
                <p className="user-console-log-card-error">
                  {log.error_message ?? text.noError}
                </p>
              </article>
            ))
          )}
        </div>
      ) : null}
    </section>
  )
}
