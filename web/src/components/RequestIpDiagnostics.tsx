import type { RequestLog } from '../api'

type Language = 'en' | 'zh'

export default function RequestIpDiagnostics({
  log,
  language,
}: {
  log: RequestLog
  language: Language
}): JSX.Element | null {
  const ipHeaders = (log.ip_headers ?? []).filter(
    (item) => item.name.trim().length > 0 || item.value.trim().length > 0,
  )
  if (!log.remote_addr && !log.client_ip && !log.client_ip_source && ipHeaders.length === 0) {
    return null
  }
  return (
    <div className="log-details-headers">
      <div className="log-details-section">
        <header>{language === 'zh' ? 'IP 诊断' : 'IP diagnostics'}</header>
        <ul>
          <li>
            remoteAddr: <code>{log.remote_addr ?? '-'}</code>
          </li>
          <li>
            clientIp: <code>{log.client_ip ?? '-'}</code>
          </li>
          <li>
            source: <code>{log.client_ip_source ?? '-'}</code>
          </li>
          <li>{language === 'zh' ? '可信代理' : 'trusted proxy'}: {log.client_ip_trusted ? 'yes' : 'no'}</li>
        </ul>
      </div>
      {ipHeaders.length > 0 ? (
        <div className="log-details-section">
          <header>{language === 'zh' ? 'IP 头值快照' : 'IP header values'}</header>
          <ul>
            {ipHeaders.map((header, index) => (
              <li key={`ip-header-${index}-${header.name}-${header.value}`}>
                <code>{header.name}</code>: <code>{header.value}</code>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </div>
  )
}
