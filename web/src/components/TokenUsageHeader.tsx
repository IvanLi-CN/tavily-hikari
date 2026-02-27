import { Icon } from '@iconify/react'

import ThemeToggle from './ThemeToggle'
import { Button } from './ui/button'
import SegmentedTabs, { type SegmentedTabsOption } from './ui/SegmentedTabs'

type TokenLeaderboardPeriod = 'day' | 'month' | 'all'
type TokenLeaderboardFocus = 'usage' | 'errors' | 'other'
type TokenUsageHeaderVisualPreset = 'panel' | 'inline' | 'accent'

interface TokenUsageHeaderProps {
  title: string
  subtitle: string
  backLabel: string
  refreshLabel: string
  refreshingLabel: string
  isRefreshing: boolean
  period: TokenLeaderboardPeriod
  focus: TokenLeaderboardFocus
  periodOptions: ReadonlyArray<SegmentedTabsOption<TokenLeaderboardPeriod>>
  focusOptions: ReadonlyArray<SegmentedTabsOption<TokenLeaderboardFocus>>
  visualPreset?: TokenUsageHeaderVisualPreset
  onBack: () => void
  onRefresh: () => void
  onPeriodChange: (value: TokenLeaderboardPeriod) => void
  onFocusChange: (value: TokenLeaderboardFocus) => void
}

export default function TokenUsageHeader(props: TokenUsageHeaderProps): JSX.Element {
  const visualPreset = props.visualPreset ?? 'panel'
  const activePeriodLabel = props.periodOptions.find((option) => option.value === props.period)?.label
  const activeFocusLabel = props.focusOptions.find((option) => option.value === props.focus)?.label

  return (
    <section className={`surface app-header token-usage-header token-usage-header--${visualPreset}`}>
      <div className="token-usage-header-top">
        <div className="token-usage-header-main">
          <h1>{props.title}</h1>
          <p>{props.subtitle}</p>
        </div>

        <div className="token-usage-header-side">
          {visualPreset === 'inline' ? (
            <p className="token-usage-header-context-line" aria-live="polite">
              <Icon icon="mdi:tune" width={14} height={14} aria-hidden="true" />
              <span>当前筛选</span>
              <strong>{activePeriodLabel}</strong>
              <span>·</span>
              <strong>{activeFocusLabel}</strong>
            </p>
          ) : (
            <div className="token-usage-header-context" aria-live="polite">
              <span className="token-usage-header-context-chip">
                <Icon icon="mdi:calendar-today-outline" width={14} height={14} aria-hidden="true" />
                <span>{activePeriodLabel}</span>
              </span>
              <span className="token-usage-header-context-chip">
                <Icon icon="mdi:tune-variant" width={14} height={14} aria-hidden="true" />
                <span>{activeFocusLabel}</span>
              </span>
            </div>
          )}

          <div className="token-usage-header-utility">
            <ThemeToggle />
            <Button type="button" variant="ghost" size="sm" className="token-usage-back-button" onClick={props.onBack}>
              <Icon icon="mdi:arrow-left" width={16} height={16} />
              <span>{props.backLabel}</span>
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="token-usage-refresh-button"
              onClick={props.onRefresh}
              disabled={props.isRefreshing}
            >
              <Icon
                icon={props.isRefreshing ? 'mdi:loading' : 'mdi:refresh'}
                width={16}
                height={16}
                className={props.isRefreshing ? 'icon-spin' : undefined}
              />
              <span>{props.isRefreshing ? props.refreshingLabel : props.refreshLabel}</span>
            </Button>
          </div>
        </div>
      </div>

      <div className="token-usage-header-filters">
        <SegmentedTabs<'day' | 'month' | 'all'>
          className="token-usage-segmented"
          value={props.period}
          onChange={props.onPeriodChange}
          options={props.periodOptions}
          ariaLabel={props.title}
        />
        <SegmentedTabs<'usage' | 'errors' | 'other'>
          className="token-usage-segmented"
          value={props.focus}
          onChange={props.onFocusChange}
          options={props.focusOptions}
          ariaLabel={props.subtitle}
        />
      </div>
    </section>
  )
}
