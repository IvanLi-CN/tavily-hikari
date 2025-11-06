import { useEffect, useMemo, useState } from 'react'
import { fetchPublicMetrics, fetchProfile, type Profile, type PublicMetrics } from './api'

const numberFormatter = new Intl.NumberFormat('en-US', {
  maximumFractionDigits: 0,
})

function formatNumber(value: number): string {
  return numberFormatter.format(value)
}

function PublicHome(): JSX.Element {
  const DEFAULT_TOKEN = 'th-demo-1234567890'
  const [token, setToken] = useState(DEFAULT_TOKEN)
  const [tokenVisible, setTokenVisible] = useState(false)
  const [metrics, setMetrics] = useState<PublicMetrics | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [profile, setProfile] = useState<Profile | null>(null)
  const [activeGuide, setActiveGuide] = useState<'codex' | 'claude' | 'other'>('codex')

  useEffect(() => {
    const hash = window.location.hash.slice(1)
    if (hash) {
      setToken(decodeURIComponent(hash))
    } else {
      window.location.hash = encodeURIComponent(DEFAULT_TOKEN)
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

  const guideDescription = useMemo(() => {
    const baseUrl = window.location.origin
    switch (activeGuide) {
      case 'codex':
        return {
          title: 'Codex CLI',
          steps: [
            '编辑配置文件 `~/.codex/credentials`（或当前 profile 的 `mcp_headers`）',
            `在 headers 中加入 \`Authorization: Bearer ${token || DEFAULT_TOKEN}\``,
            `将自定义 MCP upstream 指向 \`${baseUrl}/mcp\``,
            '保存后执行 `codex mcp connect tavily-hikari` 或直接在 CLI 中调用。',
          ],
          sampleTitle: '示例：~/.codex/credentials',
          snippetLanguage: 'toml',
          snippet: `[mcp_servers.tavily-hikari]\nurl = \"${baseUrl}/mcp\"\nheaders = { Authorization = \"Bearer ${token || DEFAULT_TOKEN}\" }`,
        }
      case 'claude':
        return {
          title: 'Claude Code',
          steps: [
            '打开设置 → Custom MCP Servers → Add Endpoint',
            `URL 填写 \`${baseUrl}/mcp\``,
            `在 HTTP Headers 增加 \`Authorization: Bearer ${token || DEFAULT_TOKEN}\``,
            '保存后在 Claude Code 侧边栏启用该 MCP，即可使用 Tavily 功能。',
          ],
          sampleTitle: '示例：claude_desktop_config.json',
          snippetLanguage: 'json',
          snippet: `{
  "mcpServers": [
    {
      "name": "tavily-hikari",
      "baseUrl": "${baseUrl}/mcp",
      "auth": {
        "type": "bearer",
        "token": "${token || DEFAULT_TOKEN}"
      }
    }
  ]
}`,
        }
      default:
        return {
          title: '其他 MCP 客户端',
          steps: [
            `确保请求地址指向 \`${baseUrl}/mcp\``,
            `向请求添加 Authorization Header：\`Bearer ${token || DEFAULT_TOKEN}\``,
            '参考下方 cURL 调用示例，确认代理能正常响应。',
            '遇到认证失败，请联系管理员更新 Access Token。',
          ],
          sampleTitle: '示例：curl 测试请求',
          snippetLanguage: 'bash',
          snippet: `curl -H "Authorization: Bearer ${token || DEFAULT_TOKEN}" \\\n+     -H "Content-Type: application/json" \\\n+     ${baseUrl}/mcp/health`,
        }
    }
  }, [activeGuide, token])

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
        <h2>如何在常见 MCP 客户端中接入 Tavily Hikari</h2>
        <div className="guide-tabs">
          <button
            type="button"
            className={`guide-tab${activeGuide === 'codex' ? ' active' : ''}`}
            onClick={() => setActiveGuide('codex')}
          >
            Codex CLI
          </button>
          <button
            type="button"
            className={`guide-tab${activeGuide === 'claude' ? ' active' : ''}`}
            onClick={() => setActiveGuide('claude')}
          >
            Claude Code
          </button>
          <button
            type="button"
            className={`guide-tab${activeGuide === 'other' ? ' active' : ''}`}
            onClick={() => setActiveGuide('other')}
          >
            其他 MCP 客户端
          </button>
        </div>
        <div className="guide-panel">
          <h3>{guideDescription.title}</h3>
          <ol>
            {guideDescription.steps.map((step, index) => (
              <li key={index}>{step}</li>
            ))}
          </ol>
          {guideDescription.sampleTitle && guideDescription.snippet && (
            <div className="guide-sample">
              <p className="guide-sample-title">{guideDescription.sampleTitle}</p>
              <pre className="guide-code" data-lang={guideDescription.snippetLanguage}>
                <code>{guideDescription.snippet}</code>
              </pre>
            </div>
          )}
          <p className="guide-note">
            想快速分享访问？复制当前链接，我们会把 Access Token 放在 URL Hash 中，例如{' '}
            <code>{window.location.origin}/#{token || DEFAULT_TOKEN}</code>。
          </p>
        </div>
      </section>
    </main>
  )
}

export default PublicHome
