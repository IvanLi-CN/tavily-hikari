import { ReactNode, useCallback, useEffect, useMemo, useState } from 'react'
import {
  fetchPublicMetrics,
  fetchProfile,
  fetchSummary,
  type Profile,
  type PublicMetrics,
  type Summary,
} from './api'
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
const REPO_URL = 'https://github.com/IvanLi-CN/tavily-hikari'
const ICONIFY_ENDPOINT = 'https://api.iconify.design'
const STORAGE_LAST_TOKEN = 'tavily-hikari-last-token'
const STORAGE_TOKEN_MAP = 'tavily-hikari-token-map'

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
  const [summary, setSummary] = useState<Summary | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [profile, setProfile] = useState<Profile | null>(null)
  const [activeGuide, setActiveGuide] = useState<GuideKey>('codex')
  const updateBanner = useUpdateAvailable()
  const [copyState, setCopyState] = useState<'idle' | 'copied' | 'error'>('idle')

  useEffect(() => {
    const hash = window.location.hash.slice(1)
    const decodedHash = hash ? decodeURIComponent(hash) : null
    const tokenStore = loadTokenMap()
    const lastToken = loadLastToken()

    let initialToken: string | null = null
    if (decodedHash && isFullToken(decodedHash)) {
      initialToken = decodedHash
    } else if (decodedHash) {
      const id = extractTokenId(decodedHash)
      if (id && tokenStore[id]) {
        initialToken = tokenStore[id]
      }
    }

    if (!initialToken && lastToken) {
      initialToken = lastToken
    }

    if (!initialToken) {
      initialToken = DEFAULT_TOKEN
    }

    persistToken(initialToken)

    const controller = new AbortController()
    setLoading(true)
    Promise.allSettled([
      fetchPublicMetrics(controller.signal),
      fetchProfile(controller.signal),
      fetchSummary(controller.signal),
    ])
      .then(([metricsResult, profileResult, summaryResult]) => {
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

        if (summaryResult.status === 'fulfilled') {
          setSummary(summaryResult.value)
        } else {
          const reason = summaryResult.reason as Error
          if (reason?.name !== 'AbortError') {
            setError((prev) => prev ?? (reason instanceof Error ? reason.message : 'Unable to load summary data'))
          }
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
  const availableKeys = summary?.active_keys ?? null
  const exhaustedKeys = summary?.exhausted_keys ?? null
  const totalKeys = availableKeys != null && exhaustedKeys != null ? availableKeys + exhaustedKeys : null

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

  const versionTagUrl = updateBanner.currentVersion
    ? `${REPO_URL}/tree/v${encodeURIComponent(updateBanner.currentVersion)}`
    : null

  const handleCopyToken = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(token)
      setCopyState('copied')
      window.setTimeout(() => setCopyState('idle'), 2500)
    } catch {
      setCopyState('error')
      window.setTimeout(() => setCopyState('idle'), 2500)
    }
  }, [token])

  const persistToken = useCallback((next: string) => {
    setToken(next)
    const normalizedHash = normalizeTokenHash(next)
    window.location.hash = encodeURIComponent(normalizedHash)

    if (!isFullToken(next)) {
      return
    }

    const tokenId = extractTokenId(next)
    if (!tokenId) return

    const map = loadTokenMap()
    map[tokenId] = next
    saveTokenMap(map)
    try {
      localStorage.setItem(STORAGE_LAST_TOKEN, next)
    } catch {
      /* noop */
    }
  }, [])

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
        <h1 className="hero-title">Tavily Hikari Proxy</h1>
        <p className="public-home-tagline">Transparent request visibility for your Tavily integration.</p>
        <p className="public-home-description">
          Tavily Hikari 将多组 Tavily API Key 聚合为一个统一入口，自动均衡每个密钥的用量，避免本地频繁切换账户；并提供请求审计、速率监控与跨客户端分享的访问令牌管理。
        </p>
        {error && <div className="surface error-banner" role="status">{error}</div>}
        <div className="metrics-grid hero-metrics">
          <div className="metric-card">
            <h3>本月成功请求（UTC）</h3>
            <div className="metric-value">
              {loading ? '—' : formatNumber(metrics?.monthlySuccess ?? 0)}
            </div>
            <div className="metric-subtitle">Tavily 月额度按 UTC 月初自动重置</div>
          </div>
          <div className="metric-card">
            <h3>今日（服务器时区）</h3>
            <div className="metric-value">{loading ? '—' : formatNumber(metrics?.dailySuccess ?? 0)}</div>
            <div className="metric-subtitle">从服务器午夜起累计的成功请求</div>
          </div>
          <div className="metric-card">
            <h3>号池可用数</h3>
            <div className="metric-value">
              {loading ? '—' : availableKeys != null && totalKeys != null ? `${availableKeys}/${totalKeys}` : '—'}
            </div>
            <div className="metric-subtitle">活跃 Tavily API Key / 总密钥（含本月耗尽）</div>
          </div>
        </div>
        {isAdmin && (
          <button type="button" className="button button-primary" onClick={() => { window.location.href = '/admin' }}>
            Open Admin Dashboard
          </button>
        )}
      </section>
      <section className="surface panel access-panel">
        <div className="access-panel-grid">
          <div className="access-stats">
            <div className="access-stat">
              <h4>今日成功</h4>
              <p>{loading ? '—' : formatNumber(metrics?.dailySuccess ?? 0)}</p>
            </div>
            <div className="access-stat">
              <h4>今日失败</h4>
              <p>{loading ? '—' : formatNumber((summary?.error_count ?? 0) - (summary?.success_count ?? 0))}</p>
            </div>
            <div className="access-stat">
              <h4>本月成功</h4>
              <p>{loading ? '—' : formatNumber(metrics?.monthlySuccess ?? 0)}</p>
            </div>
          </div>
          <div className="access-token-box">
            <label htmlFor="access-token" className="token-label">
              Access Token
            </label>
            <div className="token-input-row">
              <div className="token-input-shell">
                <input
                  id="access-token"
                  className="token-input"
                  type={tokenVisible ? 'text' : 'password'}
                  value={token}
                  onChange={(event) => {
                    const value = event.target.value
                    setToken(value)
                  }}
                  onBlur={(event) => {
                    persistToken(event.target.value)
                  }}
                  placeholder="th-xxxx-xxxxxxxxxxxx"
                  autoComplete="off"
                />
                <button
                  type="button"
                  className="token-visibility-button"
                  onClick={() => setTokenVisible((prev) => !prev)}
                  aria-label={tokenVisible ? '隐藏 Access Token' : '显示 Access Token'}
                >
                  <img
                    src={`${ICONIFY_ENDPOINT}/mdi/${tokenVisible ? 'eye-off-outline' : 'eye-outline'}.svg?color=%236b7280`}
                    alt="toggle visibility"
                  />
                </button>
              </div>
              <button
                type="button"
                className={`button button-secondary token-copy-button${copyState === 'copied' ? ' success' : ''}`}
                onClick={handleCopyToken}
              >
                <img src={`${ICONIFY_ENDPOINT}/mdi/content-copy.svg?color=%23ffffff`} alt="复制" />
                <span>{copyState === 'copied' ? '已复制' : copyState === 'error' ? '复制失败' : '复制令牌'}</span>
              </button>
            </div>
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
        <a className="footer-gh" href={REPO_URL} target="_blank" rel="noreferrer">
          <img src={`${ICONIFY_ENDPOINT}/mdi/github.svg?color=%232563eb`} alt="GitHub" />
          <span>GitHub</span>
        </a>
        <div className="footer-version">
          <span>当前版本：</span>
          {versionTagUrl ? (
            <a href={versionTagUrl} target="_blank" rel="noreferrer">
              <code>v{updateBanner.currentVersion}</code>
            </a>
          ) : (
            <code>—</code>
          )}
        </div>
      </footer>
    </main>
  )
}

export default PublicHome
function normalizeTokenHash(value: string): string {
  const maybeId = extractTokenId(value)
  return maybeId ?? value
}

function extractTokenId(value: string): string | null {
  const fullTokenMatch = /^th-([a-zA-Z0-9]{4})-[a-zA-Z0-9]+$/.exec(value)
  if (fullTokenMatch) return fullTokenMatch[1]
  if (/^[a-zA-Z0-9]{4}$/.test(value)) return value
  return null
}

function isFullToken(value: string): boolean {
  return /^th-[a-zA-Z0-9]{4}-[a-zA-Z0-9]+$/.test(value)
}

function loadTokenMap(): Record<string, string> {
  try {
    const raw = localStorage.getItem(STORAGE_TOKEN_MAP)
    if (!raw) return {}
    const parsed = JSON.parse(raw)
    return typeof parsed === 'object' && parsed ? parsed : {}
  } catch {
    return {}
  }
}

function saveTokenMap(map: Record<string, string>): void {
  try {
    localStorage.setItem(STORAGE_TOKEN_MAP, JSON.stringify(map))
  } catch {
    /* ignore */
  }
}

function loadLastToken(): string | null {
  try {
    return localStorage.getItem(STORAGE_LAST_TOKEN)
  } catch {
    return null
  }
}
