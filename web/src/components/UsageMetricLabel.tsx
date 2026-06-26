import { AnchoredInfoDisclosure } from './ui/anchored-info-disclosure'
import { cn } from '../lib/utils'

export type UsageMetricHelpKind = 'businessCalls1h' | 'dailyCredits' | 'monthlyCredits'

function usageMetricHelpText(kind: UsageMetricHelpKind, language: 'en' | 'zh'): string {
  if (language === 'zh') {
    switch (kind) {
      case 'businessCalls1h':
        return '滚动 1 小时内实际打到上游的业务请求次数。成功和失败都会计入；在本地提前拦截的请求不计入。'
      case 'dailyCredits':
        return '当前自然日内累计消耗的业务积分。'
      case 'monthlyCredits':
        return '当前自然月内累计消耗的业务积分。'
    }
  }

  switch (kind) {
    case 'businessCalls1h':
      return 'Rolling 1-hour count of business requests that actually reached the upstream. Both successes and failures count. Requests blocked locally do not count.'
    case 'dailyCredits':
      return 'Business credits consumed in the current calendar day.'
    case 'monthlyCredits':
      return 'Business credits consumed in the current calendar month.'
  }
}

export function UsageMetricLabel({
  label,
  kind,
  language,
  className,
}: {
  label: string
  kind: UsageMetricHelpKind
  language: 'en' | 'zh'
  className?: string
}): JSX.Element {
  if (typeof document === 'undefined') {
    return <span className={cn(className)}>{label}</span>
  }

  return (
    <AnchoredInfoDisclosure
      className={cn(className)}
      bubbleClassName="max-w-[min(18rem,calc(100vw-2rem))]"
      bubbleContent={<p style={{ margin: 0 }}>{usageMetricHelpText(kind, language)}</p>}
      aria-label={label}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 6,
        padding: 0,
        border: 0,
        background: 'none',
        color: 'inherit',
        font: 'inherit',
        cursor: 'help',
        textDecoration: 'underline dotted',
        textUnderlineOffset: 3,
      }}
    >
      <span>{label}</span>
    </AnchoredInfoDisclosure>
  )
}
