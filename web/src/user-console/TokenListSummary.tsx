import type { EN } from './text'

type TokenText = typeof EN.tokens

interface TokenListSummaryProps {
  text: TokenText
  total: number
  enabled: number
  dailySuccess: number
  formatNumber: (value: number) => string
}

export default function TokenListSummary({
  text,
  total,
  enabled,
  dailySuccess,
  formatNumber,
}: TokenListSummaryProps): JSX.Element {
  return (
    <div className="user-console-section-meta user-console-md-up" aria-label={text.title}>
      <div className="user-console-inline-stat">
        <span className="user-console-inline-stat-label">{text.summary.total}</span>
        <strong className="user-console-inline-stat-value">{formatNumber(total)}</strong>
      </div>
      <div className="user-console-inline-stat">
        <span className="user-console-inline-stat-label">{text.summary.enabled}</span>
        <strong className="user-console-inline-stat-value">{formatNumber(enabled)}</strong>
      </div>
      <div className="user-console-inline-stat">
        <span className="user-console-inline-stat-label">{text.summary.dailySuccess}</span>
        <strong className="user-console-inline-stat-value">{formatNumber(dailySuccess)}</strong>
      </div>
    </div>
  )
}
