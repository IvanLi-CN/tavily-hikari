import { ReactNode, useEffect, useMemo, useState } from 'react'
import { fetchPublicMetrics, fetchProfile, type Profile, type PublicMetrics } from './api'
import useUpdateAvailable from './hooks/useUpdateAvailable'

type GuideLanguage = 'toml' | 'json' | 'bash'

type GuideKey = 'codex' | 'claude' | 'vscode' | 'claudeDesktop' | 'cursor' | 'windsurf' | 'other'

interface GuideReference {
  label: string
  url: string
}

interface GuideContent {
  title: string
  steps: ReactNode[]
  sampleTitle?: string
  snippetLanguage?: GuideLanguage
  snippet?: string
  reference?: GuideReference
}

const CODEX_DOC_URL = 'https://github.com/openai/codex/blob/main/docs/config.md'
const CLAUDE_DOC_URL = 'https://code.claude.com/docs/en/mcp'
const VSCODE_DOC_URL = 'https://code.visualstudio.com/docs/copilot/customization/mcp-servers'
const NOCODB_DOC_URL = 'https://nocodb.com/docs/product-docs/mcp'
const MCP_SPEC_URL = 'https://modelcontextprotocol.io/introduction'

const GUIDE_TABS: Array<{ id: GuideKey; label: string }> = [
  { id: 'codex', label: 'Codex CLI' },
  { id: 'claude', label: 'Claude Code CLI' },
  { id: 'vscode', label: 'VS Code / Copilot' },
  { id: 'claudeDesktop', label: 'Claude Desktop' },
  { id: 'cursor', label: 'Cursor' },
  { id: 'windsurf', label: 'Windsurf' },
  { id: 'other', label: '其他客户端' },
]

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
  const [activeGuide, setActiveGuide] = useState<GuideKey>('codex')
  const updateBanner = useUpdateAvailable()

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

  const guideDescription = useMemo<GuideContent>(() => {
    const baseUrl = window.location.origin
    const prettyToken = token || DEFAULT_TOKEN

    const guides: Record<GuideKey, GuideContent> = {
      codex: (() => {
        const snippet = [
          '<span class="hl-comment"># ~/.codex/config.toml</span>',
          'experimental_use_rmcp_client = true',
          '',
          '[mcp_servers.tavily_hikari]',
          `url = "<span class=\"hl-string\">${baseUrl}/mcp</span>"`,
          'bearer_token_env_var = "TAVILY_HIKARI_TOKEN"',
        ].join('\n')

        return {
          title: 'Codex CLI',
          steps: [
            <>在 <code>~/.codex/config.toml</code> 设定 <code>experimental_use_rmcp_client = true</code>。</>,
            <>添加 <code>[mcp_servers.tavily_hikari]</code>，将 <code>url</code> 指向 <code>{baseUrl}/mcp</code> 并声明 <code>bearer_token_env_var = TAVILY_HIKARI_TOKEN</code>。</>,
            <>运行 <code>export TAVILY_HIKARI_TOKEN="{prettyToken}"</code> 后，执行 <code>codex mcp list</code> 或 <code>codex mcp get tavily_hikari</code> 验证。</>,
          ],
          sampleTitle: '示例：~/.codex/config.toml',
          snippetLanguage: 'toml',
          snippet,
          reference: {
            label: 'OpenAI Codex docs',
            url: CODEX_DOC_URL,
          },
        }
      })(),
      claude: (() => {
        const snippet = [
          '<span class="hl-comment"># claude mcp add-json</span>',
          `claude mcp add-json tavily-hikari '{`,
          `  <span class=\"hl-key\">"type"</span>: <span class=\"hl-string\">"http"</span>,`,
          `  <span class=\"hl-key\">"url"</span>: <span class=\"hl-string\">"${baseUrl}/mcp"</span>,`,
          '  <span class="hl-key">"headers"</span>: {',
          `    <span class=\"hl-key\">"Authorization"</span>: <span class=\"hl-string\">"Bearer ${prettyToken}"</span>`,
          '  }',
          "}'",
          '',
          '# 验证',
          'claude mcp get tavily-hikari',
        ].join('\n')

        return {
          title: 'Claude Code',
          steps: [
            <>参考下方命令，使用 <code>claude mcp add-json</code> 注册 Tavily Hikari HTTP MCP。</>,
            <>运行 <code>claude mcp get tavily-hikari</code> 查看状态或排查错误。</>,
          ],
          sampleTitle: '示例：claude mcp add-json',
          snippetLanguage: 'bash',
          snippet,
          reference: {
            label: 'Claude Code MCP docs',
            url: CLAUDE_DOC_URL,
          },
        }
      })(),
      vscode: (() => {
        const snippet = [
          '{',
          '  <span class="hl-key">"servers"</span>: {',
          '    <span class="hl-key">"tavily-hikari"</span>: {',
          '      <span class="hl-key">"type"</span>: <span class="hl-string">"http"</span>,',
          `      <span class=\"hl-key\">"url"</span>: <span class=\"hl-string\">"${baseUrl}/mcp"</span>,`,
          '      <span class="hl-key">"headers"</span>: {',
          `        <span class=\"hl-key\">"Authorization"</span>: <span class=\"hl-string\">"Bearer ${prettyToken}"</span>`,
          '      }',
          '    }',
          '  }',
          '}',
        ].join('\n')

        return {
          title: '其他 MCP 客户端',
          steps: [
            <>在 VS Code Copilot <code>mcp.json</code>（或 <code>.code-workspace</code>/<code>devcontainer.json</code> 的 <code>customizations.vscode.mcp</code>）添加服务器节点。</>,
            <>设置 <code>type</code> 为 <code>"http"</code>、<code>url</code> 为 <code>{baseUrl}/mcp</code>，并在 <code>headers.Authorization</code> 写入 <code>Bearer {prettyToken}</code>。</>,
            <>保存后重新打开 Copilot Chat，使配置与 <a href={VSCODE_DOC_URL} rel="noreferrer" target="_blank">官方指南</a> 保持一致。</>,
          ],
          sampleTitle: '示例：mcp.json',
          snippetLanguage: 'json',
          snippet,
          reference: {
            label: 'VS Code Copilot MCP 文档',
            url: VSCODE_DOC_URL,
          },
        }
      })(),
      claudeDesktop: {
        title: 'Claude Desktop',
        steps: [
          <>打开 <code>⌘+,</code> → <strong>Develop</strong> → <code>Edit Config</code>，按照官方文档将 MCP JSON 写入本地 <code>claude_desktop_config.json</code>。</>,
          <>在 JSON 中保留我们提供的 endpoint，保存后重启 Claude Desktop 以载入新的工具列表。</>,
        ],
        sampleTitle: '示例：claude_desktop_config.json',
        snippetLanguage: 'json',
        snippet: `{
  <span class="hl-key">"mcpServers"</span>: {
    <span class="hl-key">"tavily-hikari"</span>: {
      <span class="hl-key">"type"</span>: <span class="hl-string">"http"</span>,
      <span class="hl-key">"url"</span>: <span class="hl-string">"${baseUrl}/mcp"</span>,
      <span class="hl-key">"headers"</span>: {
        <span class="hl-key">"Authorization"</span>: <span class="hl-string">"Bearer ${prettyToken}"</span>
      }
    }
  }
}`,
        reference: {
          label: 'NocoDB MCP docs',
          url: NOCODB_DOC_URL,
        },
      },
      cursor: {
        title: 'Cursor',
        steps: [
          <>在 Cursor 设置（<code>⇧+⌘+J</code>）中打开 <strong>MCP → Add Custom MCP</strong>，按照官方指南编辑全局 <code>mcp.json</code>。</>,
          <>粘贴下方配置并保存，回到 MCP 面板确认条目显示 “tools enabled”。</>,
        ],
        sampleTitle: '示例：~/.cursor/mcp.json',
        snippetLanguage: 'json',
        snippet: `{
  <span class="hl-key">"mcpServers"</span>: {
    <span class="hl-key">"tavily-hikari"</span>: {
      <span class="hl-key">"type"</span>: <span class="hl-string">"http"</span>,
      <span class="hl-key">"url"</span>: <span class="hl-string">"${baseUrl}/mcp"</span>,
      <span class="hl-key">"headers"</span>: {
        <span class="hl-key">"Authorization"</span>: <span class="hl-string">"Bearer ${prettyToken}"</span>
      }
    }
  }
}`,
        reference: {
          label: 'NocoDB MCP docs',
          url: NOCODB_DOC_URL,
        },
      },
      windsurf: {
        title: 'Windsurf',
        steps: [
          <>在 Windsurf 中点击 MCP 侧边栏的锤子图标 → <strong>Configure</strong>，再选择 <strong>View raw config</strong> 打开 <code>mcp_config.json</code>。</>,
          <>将下方片段写入 <code>mcpServers</code>，保存后在 Manage Plugins 页点击 <strong>Refresh</strong> 以加载新工具。</>,
        ],
        sampleTitle: '示例：~/.codeium/windsurf/mcp_config.json',
        snippetLanguage: 'json',
        snippet: `{
  <span class="hl-key">"mcpServers"</span>: {
    <span class="hl-key">"tavily-hikari"</span>: {
      <span class="hl-key">"type"</span>: <span class="hl-string">"http"</span>,
      <span class="hl-key">"url"</span>: <span class="hl-string">"${baseUrl}/mcp"</span>,
      <span class="hl-key">"headers"</span>: {
        <span class="hl-key">"Authorization"</span>: <span class="hl-string">"Bearer ${prettyToken}"</span>
      }
    }
  }
}`,
        reference: {
          label: 'NocoDB MCP docs',
          url: NOCODB_DOC_URL,
        },
      },
      other: {
        title: '其他 MCP 客户端',
        steps: [
          <>端点：<code>{baseUrl}/mcp</code>（Streamable HTTP）。</>,
          <>认证：HTTP Header <code>Authorization: Bearer {prettyToken}</code>。</>,
          <>适用于任意兼容客户端，直接指向该 URL 并附带上述头部即可。</>,
        ],
        sampleTitle: '示例：通用请求',
        snippetLanguage: 'bash',
        snippet: `curl -X POST \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${prettyToken}" \
  ${baseUrl}/mcp`,
        reference: {
          label: 'Model Context Protocol spec',
          url: MCP_SPEC_URL,
        },
      },
    }

    return guides[activeGuide]
  }, [activeGuide, token])

  return (
    <main className="app-shell public-home">
      {updateBanner.visible && (
        <section className="surface update-banner" role="status" aria-live="polite">
          <div className="update-banner-text">
            <strong>有新版本上线</strong>
            <span>
              当前 {updateBanner.currentVersion ?? 'unknown'} → 可用 {updateBanner.availableVersion ?? 'latest'}
            </span>
          </div>
          <div className="update-banner-actions">
            <button type="button" className="button button-primary" onClick={updateBanner.reload}>
              刷新以更新
            </button>
            <button type="button" className="button" onClick={updateBanner.dismiss}>
              暂不提醒
            </button>
          </div>
        </section>
      )}
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
          {GUIDE_TABS.map((tab) => (
            <button
              key={tab.id}
              type="button"
              className={`guide-tab${activeGuide === tab.id ? ' active' : ''}`}
              onClick={() => setActiveGuide(tab.id)}
            >
              {tab.label}
            </button>
          ))}
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
                <code dangerouslySetInnerHTML={{ __html: guideDescription.snippet }} />
              </pre>
            </div>
          )}
          {guideDescription.reference && (
            <p className="guide-reference">
              数据来源：
              <a href={guideDescription.reference.url} target="_blank" rel="noreferrer">
                {guideDescription.reference.label}
              </a>
            </p>
          )}
        </div>
      </section>
      <footer className="surface public-home-footer">
        <div className="footer-links">
          <span>开源仓库：</span>
          <a href="https://github.com/IvanLi-CN/tavily-hikari" target="_blank" rel="noreferrer">
            github.com/IvanLi-CN/tavily-hikari
          </a>
        </div>
        <div className="footer-version">
          <span>当前版本：</span>
          <code>{updateBanner.currentVersion ?? '—'}</code>
        </div>
      </footer>
    </main>
  )
}

export default PublicHome
