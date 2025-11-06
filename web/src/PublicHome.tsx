import { useEffect, useState } from 'react'
import { fetchPublicMetrics, fetchProfile, type Profile, type PublicMetrics } from './api'

const numberFormatter = new Intl.NumberFormat('en-US', {
  maximumFractionDigits: 0,
})

function formatNumber(value: number): string {
  return numberFormatter.format(value)
}

function PublicHome(): JSX.Element {
  const [token, setToken] = useState('')
  const [tokenVisible, setTokenVisible] = useState(false)
  const [metrics, setMetrics] = useState<PublicMetrics | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [profile, setProfile] = useState<Profile | null>(null)

  useEffect(() => {
    const hash = window.location.hash.slice(1)
    if (hash) {
      setToken(decodeURIComponent(hash))
    }

    const controller = new AbortController()
    setLoading(true)
    Promise.allSettled([
      fetchPublicMetrics(controller.signal),
      fetchProfile(controller.signal),
    ])
      .then(([metricsResult, profileResult]) => {
        if (metricsResult.status === 'fulfilled') {
          setMetrics(metricsResult.value)
          setError(null)
        } else {
          const reason = metricsResult.reason as Error
          if (reason?.name !== 'AbortError') {
            setError(reason instanceof Error ? reason.message : 'Unable to load metrics right now')
          }
        }

        if (profileResult.status === 'fulfilled') {
          setProfile(profileResult.value)
        }
      })
      .finally(() => {
        if (!controller.signal.aborted) {
          setLoading(false)
        }
      })
    return () => controller.abort()
  }, [])

  const isAdmin = profile?.isAdmin ?? false

  return (
    <main className="app-shell public-home">
      <section className="surface public-home-hero">
        <h1>Tavily Hikari Proxy</h1>
        <p className="public-home-tagline">Transparent request visibility for your Tavily integration.</p>
        <div className="public-home-actions">
          <div className="token-input-wrapper">
            <label htmlFor="access-token" className="token-label">
              Access Token
            </label>
            <div className="token-input-row">
              <input
                id="access-token"
                className="token-input"
                type={tokenVisible ? 'text' : 'password'}
                value={token}
                onChange={(event) => {
                  const value = event.target.value
                  setToken(value)
                  window.location.hash = encodeURIComponent(value)
                }}
                placeholder="th-xxxx-xxxxxxxxxxxx"
                autoComplete="off"
              />
              <button
                type="button"
                className="button"
                onClick={() => setTokenVisible((prev) => !prev)}
                aria-label={tokenVisible ? 'Hide token' : 'Show token'}
              >
                {tokenVisible ? 'Hide' : 'Show'}
              </button>
            </div>
          </div>
          {isAdmin && (
            <button type="button" className="button button-primary" onClick={() => { window.location.href = '/admin' }}>
              Open Admin Dashboard
            </button>
          )}
        </div>
      </section>
      <section className="surface panel public-home-metrics">
        <header className="public-home-metrics-header">
          <h2>Successful Requests</h2>
          <p className="panel-description">Live counters across the entire proxy deployment.</p>
        </header>
        {error && <div className="surface error-banner" role="status">{error}</div>}
        <div className="metrics-grid">
          <div className="metric-card">
            <h3>This Month</h3>
            <div className="metric-value">
              {loading ? '—' : formatNumber(metrics?.monthlySuccess ?? 0)}
            </div>
            <div className="metric-subtitle">Completed since the start of the month</div>
          </div>
          <div className="metric-card">
            <h3>Today</h3>
            <div className="metric-value">{loading ? '—' : formatNumber(metrics?.dailySuccess ?? 0)}</div>
            <div className="metric-subtitle">Completed since midnight (UTC)</div>
          </div>
        </div>
      </section>
      <section className="surface panel public-home-guide">
        <h2>如何在常见客户端中使用 Tavily Hikari</h2>
        <ol className="guide-steps">
          <li>
            <strong>准备 Access Token：</strong> 在上方输入框粘贴管理员发放的 <code>th-xxxx-xxxxxxxxxxxx</code> 令牌，或直接访问带有 <code>#token</code> 的链接，页面会自动填充。
          </li>
          <li>
            <strong>Codex CLI：</strong> 将 <code>{window.location.origin}/mcp</code> 配置为自定义 MCP upstream，把 Access Token 写入 <code>~/.codex/credentials</code> 或对应 profile 的 <code>mcp_headers</code> 中，例如 <code>Authorization: Bearer th-xxxx-xxxxxxxxxxxx</code>。
          </li>
          <li>
            <strong>Claude Code：</strong> 在“自定义 MCP 服务器”面板里新增 Endpoint，URL 设置为 <code>{window.location.origin}/mcp</code>，并在 HTTP Headers 中添加 <code>Authorization: Bearer th-xxxx-xxxxxxxxxxxx</code>。保存后即可在 Claude Code 里使用 Tavily 搜索能力。
          </li>
          <li>
            <strong>其他 MCP 客户端：</strong> 只需将 upstream 指向 <code>{window.location.origin}/mcp</code> 并附带相同的 Bearer Token。所有请求会在此代理层记录，方便管理员在 `/admin` 后台追踪。
          </li>
        </ol>
        <p className="guide-note">
          如果令牌遗失，只要在网址后加上 <code>#token</code> 即可快速恢复，例如 <code>{window.location.origin}/#th-demo-1234567890</code>。需要新令牌或重置，请联系管理员。
        </p>
      </section>
    </main>
  )
}

export default PublicHome
