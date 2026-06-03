type Language = 'en' | 'zh'

export type RequestLogBodySummarySource = {
  request_body_bytes?: number | null
  response_body_bytes?: number | null
  request_body_sha256?: string | null
  response_body_sha256?: string | null
  body_cleaned_reason?: string | null
  body_cleaned_at?: number | null
}

function cleanedReasonLabel(reason: string | null | undefined, language: Language): string {
  if (language === 'zh') {
    if (reason === 'policy_zero_days') return '策略设置为 0 天'
    if (reason === 'retention_expired') return 'body 保留到期'
    return reason || '已清理'
  }
  if (reason === 'policy_zero_days') return 'zero-day policy'
  if (reason === 'retention_expired') return 'body retention expired'
  return reason || 'cleaned'
}

export function cleanedRequestLogBodySummary({
  source,
  kind,
  language,
  noBodyLabel,
  emptyValueLabel,
  formatTime,
}: {
  source: RequestLogBodySummarySource
  kind: 'request' | 'response'
  language: Language
  noBodyLabel: string
  emptyValueLabel: string
  formatTime: (ts: number) => string
}): string {
  if (!source.body_cleaned_reason) return noBodyLabel
  const bytes = kind === 'request' ? source.request_body_bytes : source.response_body_bytes
  if (bytes == null || bytes <= 0) return noBodyLabel
  const sha256 = kind === 'request' ? source.request_body_sha256 : source.response_body_sha256
  const lines = [
    language === 'zh' ? '完整 body 已清理' : 'Full body was cleaned',
    `${language === 'zh' ? '原因' : 'Reason'}: ${cleanedReasonLabel(source.body_cleaned_reason, language)}`,
    `${language === 'zh' ? '原始字节数' : 'Original bytes'}: ${bytes ?? emptyValueLabel}`,
    `SHA-256: ${sha256 ?? emptyValueLabel}`,
  ]
  if (source.body_cleaned_at != null) {
    lines.push(`${language === 'zh' ? '清理时间' : 'Cleaned at'}: ${formatTime(source.body_cleaned_at)}`)
  }
  return lines.join('\n')
}
