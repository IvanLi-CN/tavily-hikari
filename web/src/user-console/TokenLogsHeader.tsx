import { Icon } from '../lib/icons'
import SegmentedTabs, { type SegmentedTabsOption } from '../components/ui/SegmentedTabs'
import { Tooltip, TooltipContent, TooltipTrigger } from '../components/ui/tooltip'

export type DetailLogsPushIssueCode = 'unsupported' | 'reconnecting' | 'closed'
export type UserTokenLogFilter = 'all' | 'billable'

export interface DetailLogsPushStatusText {
  ariaLabel: string
  browserUnsupported: string
  reconnecting: string
  closed: string
}

interface TokenLogsHeaderProps {
  title: string
  filter: UserTokenLogFilter
  filterOptions: ReadonlyArray<SegmentedTabsOption<UserTokenLogFilter>>
  filterAriaLabel: string
  filterDisabled: boolean
  pushIssue: DetailLogsPushIssueCode | null
  pushStatusText: DetailLogsPushStatusText
  onFilterChange: (filter: UserTokenLogFilter) => void
}

export function resolveDetailLogsPushIssueMessage(
  issue: DetailLogsPushIssueCode,
  text: DetailLogsPushStatusText,
): string {
  switch (issue) {
    case 'unsupported':
      return text.browserUnsupported
    case 'reconnecting':
      return text.reconnecting
    case 'closed':
      return text.closed
  }
}

export default function TokenLogsHeader({
  title,
  filter,
  filterOptions,
  filterAriaLabel,
  filterDisabled,
  pushIssue,
  pushStatusText,
  onFilterChange,
}: TokenLogsHeaderProps): JSX.Element {
  return (
    <div className="panel-header user-console-logs-header">
      <h2>{title}</h2>
      <div className="user-console-logs-header-actions">
        <SegmentedTabs<UserTokenLogFilter>
          value={filter}
          onChange={onFilterChange}
          options={filterOptions}
          ariaLabel={filterAriaLabel}
          className="user-console-log-filter-tabs"
          disabled={filterDisabled}
        />
        {pushIssue ? (
          <div className="user-console-push-status-slot is-active">
            <Tooltip>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  className="user-console-push-status-trigger"
                  aria-label={pushStatusText.ariaLabel}
                >
                  <Icon icon="mdi:alert-circle-outline" width={18} height={18} aria-hidden="true" />
                </button>
              </TooltipTrigger>
              <TooltipContent side="top" align="end" className="max-w-[min(20rem,calc(100vw-2rem))]">
                {resolveDetailLogsPushIssueMessage(pushIssue, pushStatusText)}
              </TooltipContent>
            </Tooltip>
          </div>
        ) : null}
      </div>
    </div>
  )
}
