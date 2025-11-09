import { ReactNode, useCallback, useEffect, useMemo, useState } from 'react'
import {
  fetchPublicMetrics,
  fetchProfile,
  fetchSummary,
  fetchTokenMetrics,
  type Profile,
  type PublicMetrics,
  type Summary,
  type TokenMetrics,
} from './api'
import LanguageSwitcher from './components/LanguageSwitcher'
import useUpdateAvailable from './hooks/useUpdateAvailable'
import RollingNumber from './components/RollingNumber'
import { useLanguage, useTranslate, type Language } from './i18n'

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

const GUIDE_KEY_ORDER: GuideKey[] = [
  'codex',
  'claude',
  'vscode',
  'claudeDesktop',
  'cursor',
  'windsurf',
  'other',
]

const numberFormatter = new Intl.NumberFormat('en-US', {
  maximumFractionDigits: 0,
})

function formatNumber(value: number): string {
  return numberFormatter.format(value)
}

function PublicHome(): JSX.Element {
  const DEFAULT_TOKEN = 'th-demo-123456789012'
  const strings = useTranslate()
  const publicStrings = strings.public
  const { language } = useLanguage()
  const [token, setToken] = useState(DEFAULT_TOKEN)
  const [tokenVisible, setTokenVisible] = useState(false)
  const [metrics, setMetrics] = useState<PublicMetrics | null>(null)
  const [tokenMetrics, setTokenMetrics] = useState<TokenMetrics | null>(null)
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
            setError(reason instanceof Error ? reason.message : publicStrings.errors.metrics)
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
            setError((prev) => prev ?? (reason instanceof Error ? reason.message : publicStrings.errors.summary))
          }
        }
        if (initialToken && isFullToken(initialToken)) {
          fetchTokenMetrics(initialToken, controller.signal)
            .then((tm) => setTokenMetrics(tm))
            .catch(() => setTokenMetrics(null))
        }
      })
      .finally(() => {
        if (!controller.signal.aborted) {
          setLoading(false)
        }
      })
  return () => controller.abort()
  }, [])

  // Realtime metrics via public SSE
  useEffect(() => {
    // build URL with optional token
    const params = new URLSearchParams()
    if (token && isFullToken(token)) params.set('token', token)
    const url = `/api/public/events${params.toString() ? `?${params.toString()}` : ''}`
    const es = new EventSource(url)
    const onMetrics = (ev: MessageEvent) => {
      try {
        const data = JSON.parse(ev.data)
        if (data?.public) {
          setMetrics({ monthlySuccess: data.public.monthlySuccess, dailySuccess: data.public.dailySuccess })
        }
        if (data?.token) {
          setTokenMetrics({
            monthlySuccess: data.token.monthlySuccess,
            dailySuccess: data.token.dailySuccess,
            dailyFailure: data.token.dailyFailure,
          })
        }
      } catch {
        // ignore parse errors
      }
    }
    es.addEventListener('metrics', onMetrics as unknown as EventListener)
    return () => {
      es.removeEventListener('metrics', onMetrics as unknown as EventListener)
      es.close()
    }
  }, [token])

  // Fallback polling: if token metrics未就绪或 SSE 不返回 token 段，定期补一次拉取
  useEffect(() => {
    if (!token || !isFullToken(token)) return
    let active = true
    const tick = async () => {
      try {
        const tm = await fetchTokenMetrics(token)
        if (!active) return
        setTokenMetrics(tm)
      } catch {
        // ignore
      }
    }
    // 先补一次
    tick()
    const id = window.setInterval(tick, 6000)
    return () => {
      active = false
      window.clearInterval(id)
    }
  }, [token])

  const isAdmin = profile?.isAdmin ?? false
  const availableKeys = summary?.active_keys ?? null
  const exhaustedKeys = summary?.exhausted_keys ?? null
  const totalKeys = availableKeys != null && exhaustedKeys != null ? availableKeys + exhaustedKeys : null

  const guideDescription = useMemo<GuideContent>(() => {
    const baseUrl = window.location.origin
    const prettyToken = token || DEFAULT_TOKEN
    const guides = buildGuideContent(language, baseUrl, prettyToken)
    return guides[activeGuide]
  }, [activeGuide, token, language])

  const guideTabs = useMemo(
    () => GUIDE_KEY_ORDER.map((id) => ({ id, label: publicStrings.guide.tabs[id] ?? id })),
    [publicStrings.guide.tabs],
  )

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
            <strong>{publicStrings.updateBanner.title}</strong>
            <span>
              {publicStrings.updateBanner.description(
                updateBanner.currentVersion ?? 'unknown',
                updateBanner.availableVersion ?? 'latest',
              )}
            </span>
          </div>
          <div className="update-banner-actions">
            <button type="button" className="button button-primary" onClick={updateBanner.reload}>
              {publicStrings.updateBanner.refresh}
            </button>
            <button type="button" className="button" onClick={updateBanner.dismiss}>
              {publicStrings.updateBanner.dismiss}
            </button>
          </div>
        </section>
      )}
      <section className="surface public-home-hero">
        <div className="language-switcher-row">
          <LanguageSwitcher />
        </div>
        <h1 className="hero-title">{publicStrings.heroTitle}</h1>
        <p className="public-home-description">{publicStrings.heroDescription}</p>
        {error && <div className="surface error-banner" role="status">{error}</div>}
        <div className="metrics-grid hero-metrics">
          <div className="metric-card">
            <h3>{publicStrings.metrics.monthly.title}</h3>
            <div className="metric-value"><RollingNumber value={loading ? null : metrics?.monthlySuccess ?? 0} /></div>
            <div className="metric-subtitle">{publicStrings.metrics.monthly.subtitle}</div>
          </div>
          <div className="metric-card">
            <h3>{publicStrings.metrics.daily.title}</h3>
            <div className="metric-value"><RollingNumber value={loading ? null : metrics?.dailySuccess ?? 0} /></div>
            <div className="metric-subtitle">{publicStrings.metrics.daily.subtitle}</div>
          </div>
          <div className="metric-card">
            <h3>{publicStrings.metrics.pool.title}</h3>
            <div className="metric-value">
              {loading ? '—' : availableKeys != null && totalKeys != null ? `${availableKeys}/${totalKeys}` : '—'}
            </div>
            <div className="metric-subtitle">{publicStrings.metrics.pool.subtitle}</div>
          </div>
        </div>
        {isAdmin && (
          <div className="public-home-actions">
            <button type="button" className="button button-primary" onClick={() => { window.location.href = '/admin' }}>
              {publicStrings.adminButton}
            </button>
          </div>
        )}
      </section>
      <section className="surface panel access-panel">
        <div className="access-panel-grid">
          <header className="panel-header" style={{ marginBottom: 8 }}>
            <h2>{publicStrings.accessPanel.title}</h2>
          </header>
          <div className="access-stats">
            <div className="access-stat">
              <h4>{publicStrings.accessPanel.stats.dailySuccess}</h4>
              <p><RollingNumber value={loading ? null : tokenMetrics?.dailySuccess ?? 0} /></p>
            </div>
            <div className="access-stat">
              <h4>{publicStrings.accessPanel.stats.dailyFailure}</h4>
              <p><RollingNumber value={loading ? null : tokenMetrics?.dailyFailure ?? 0} /></p>
            </div>
            <div className="access-stat">
              <h4>{publicStrings.accessPanel.stats.monthlySuccess}</h4>
              <p><RollingNumber value={loading ? null : tokenMetrics?.monthlySuccess ?? 0} /></p>
            </div>
          </div>
          <div className="access-token-box">
            <label htmlFor="access-token" className="token-label">
              {publicStrings.accessToken.label}
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
                    const next = event.target.value
                    persistToken(next)
                    if (isFullToken(next)) {
                      fetchTokenMetrics(next).then(setTokenMetrics).catch(() => setTokenMetrics(null))
                    }
                  }}
                  placeholder={publicStrings.accessToken.placeholder}
                  autoComplete="off"
                />
                <button
                  type="button"
                  className="token-visibility-button"
                  onClick={() => setTokenVisible((prev) => !prev)}
                  aria-label={tokenVisible ? publicStrings.accessToken.toggle.hide : publicStrings.accessToken.toggle.show}
                >
                  <img
                    src={`${ICONIFY_ENDPOINT}/mdi/${tokenVisible ? 'eye-off-outline' : 'eye-outline'}.svg?color=%236b7280`}
                    alt={publicStrings.accessToken.toggle.iconAlt}
                  />
                </button>
              </div>
              <button
                type="button"
                className={`button button-secondary token-copy-button${copyState === 'copied' ? ' success' : ''}`}
                onClick={handleCopyToken}
                aria-label={publicStrings.copyToken.iconAlt}
              >
                <img src={`${ICONIFY_ENDPOINT}/mdi/content-copy.svg?color=%23ffffff`} alt={publicStrings.copyToken.iconAlt} />
                <span>
                  {copyState === 'copied'
                    ? publicStrings.copyToken.copied
                    : copyState === 'error'
                      ? publicStrings.copyToken.error
                      : publicStrings.copyToken.copy}
                </span>
              </button>
            </div>
          </div>
        </div>
      </section>
      <section className="surface panel public-home-guide">
        <h2>{publicStrings.guide.title}</h2>
        <div className="guide-tabs">
          {guideTabs.map((tab) => (
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
              {publicStrings.guide.dataSourceLabel}
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
          <span>{publicStrings.footer.version}</span>
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
function buildGuideContent(language: Language, baseUrl: string, prettyToken: string): Record<GuideKey, GuideContent> {
  const isEnglish = language === 'en'
  const codexSnippet = buildCodexSnippet(baseUrl)
  const claudeSnippet = buildClaudeSnippet(baseUrl, prettyToken, language)
  const genericJsonSnippet = buildGenericJsonSnippet(baseUrl, prettyToken)
  const curlSnippet = buildCurlSnippet(baseUrl, prettyToken)

  return {
    codex: {
      title: 'Codex CLI',
      steps: isEnglish
        ? [
            <>Set <code>experimental_use_rmcp_client = true</code> inside <code>~/.codex/config.toml</code>.</>,
            <>Add <code>[mcp_servers.tavily_hikari]</code>, point <code>url</code> to <code>{baseUrl}/mcp</code>, and set <code>bearer_token_env_var = TAVILY_HIKARI_TOKEN</code>.</>,
            <>Run <code>export TAVILY_HIKARI_TOKEN="{prettyToken}"</code>, then verify with <code>codex mcp list</code> or <code>codex mcp get tavily_hikari</code>.</>,
          ]
        : [
            <>在 <code>~/.codex/config.toml</code> 设定 <code>experimental_use_rmcp_client = true</code>。</>,
            <>添加 <code>[mcp_servers.tavily_hikari]</code>，将 <code>url</code> 指向 <code>{baseUrl}/mcp</code> 并声明 <code>bearer_token_env_var = TAVILY_HIKARI_TOKEN</code>。</>,
            <>运行 <code>export TAVILY_HIKARI_TOKEN="{prettyToken}"</code> 后，执行 <code>codex mcp list</code> 或 <code>codex mcp get tavily_hikari</code> 验证。</>,
          ],
      sampleTitle: isEnglish ? 'Example: ~/.codex/config.toml' : '示例：~/.codex/config.toml',
      snippetLanguage: 'toml',
      snippet: codexSnippet,
      reference: {
        label: 'OpenAI Codex docs',
        url: CODEX_DOC_URL,
      },
    },
    claude: {
      title: 'Claude Code CLI',
      steps: isEnglish
        ? [
            <>Use <code>claude mcp add-json</code> to register Tavily Hikari as an HTTP MCP endpoint.</>,
            <>Run <code>claude mcp get tavily-hikari</code> to confirm the connection or troubleshoot errors.</>,
          ]
        : [
            <>参考下方命令，使用 <code>claude mcp add-json</code> 注册 Tavily Hikari HTTP MCP。</>,
            <>运行 <code>claude mcp get tavily-hikari</code> 查看状态或排查错误。</>,
          ],
      sampleTitle: isEnglish ? 'Example: claude mcp add-json' : '示例：claude mcp add-json',
      snippetLanguage: 'bash',
      snippet: claudeSnippet,
      reference: {
        label: 'Claude Code MCP docs',
        url: CLAUDE_DOC_URL,
      },
    },
    vscode: {
      title: 'VS Code / Copilot',
      steps: isEnglish
        ? [
            <>Add Tavily Hikari to VS Code Copilot <code>mcp.json</code> (or <code>.code-workspace</code>/<code>devcontainer.json</code> under <code>customizations.vscode.mcp</code>).</>,
            <>Set <code>type</code> to <code>"http"</code>, <code>url</code> to <code>{baseUrl}/mcp</code>, and place <code>Bearer {prettyToken}</code> in <code>headers.Authorization</code>.</>,
            <>Reload Copilot Chat to apply changes, keeping it aligned with the <a href={VSCODE_DOC_URL} rel="noreferrer" target="_blank">official guide</a>.</>,
          ]
        : [
            <>在 VS Code Copilot <code>mcp.json</code>（或 <code>.code-workspace</code>/<code>devcontainer.json</code> 的 <code>customizations.vscode.mcp</code>）添加服务器节点。</>,
            <>设置 <code>type</code> 为 <code>"http"</code>、<code>url</code> 为 <code>{baseUrl}/mcp</code>，并在 <code>headers.Authorization</code> 写入 <code>Bearer {prettyToken}</code>。</>,
            <>保存后重新打开 Copilot Chat，使配置与 <a href={VSCODE_DOC_URL} rel="noreferrer" target="_blank">官方指南</a> 保持一致。</>,
          ],
      sampleTitle: isEnglish ? 'Example: mcp.json' : '示例：mcp.json',
      snippetLanguage: 'json',
      snippet: buildVscodeSnippet(baseUrl, prettyToken),
      reference: {
        label: 'VS Code Copilot MCP docs',
        url: VSCODE_DOC_URL,
      },
    },
    claudeDesktop: {
      title: 'Claude Desktop',
      steps: isEnglish
        ? [
            <>Open <code>⌘+,</code> → <strong>Develop</strong> → <code>Edit Config</code>, then update <code>claude_desktop_config.json</code> following the official docs.</>,
            <>Keep the endpoint defined below, save the file, and restart Claude Desktop to load the new tool list.</>,
          ]
        : [
            <>打开 <code>⌘+,</code> → <strong>Develop</strong> → <code>Edit Config</code>，按照官方文档将 MCP JSON 写入本地 <code>claude_desktop_config.json</code>。</>,
            <>在 JSON 中保留我们提供的 endpoint，保存后重启 Claude Desktop 以载入新的工具列表。</>,
          ],
      sampleTitle: isEnglish ? 'Example: claude_desktop_config.json' : '示例：claude_desktop_config.json',
      snippetLanguage: 'json',
      snippet: genericJsonSnippet,
      reference: {
        label: 'NocoDB MCP docs',
        url: NOCODB_DOC_URL,
      },
    },
    cursor: {
      title: 'Cursor',
      steps: isEnglish
        ? [
            <>Open Cursor Settings (<code>⇧+⌘+J</code>) → <strong>MCP → Add Custom MCP</strong> and edit the global <code>mcp.json</code>.</>,
            <>Paste the configuration below, save it, and confirm “tools enabled” inside the MCP panel.</>,
          ]
        : [
            <>在 Cursor 设置（<code>⇧+⌘+J</code>）中打开 <strong>MCP → Add Custom MCP</strong>，按照官方指南编辑全局 <code>mcp.json</code>。</>,
            <>粘贴下方配置并保存，回到 MCP 面板确认条目显示 “tools enabled”。</>,
          ],
      sampleTitle: isEnglish ? 'Example: ~/.cursor/mcp.json' : '示例：~/.cursor/mcp.json',
      snippetLanguage: 'json',
      snippet: genericJsonSnippet,
      reference: {
        label: 'NocoDB MCP docs',
        url: NOCODB_DOC_URL,
      },
    },
    windsurf: {
      title: 'Windsurf',
      steps: isEnglish
        ? [
            <>In Windsurf, click the hammer icon in the MCP sidebar → <strong>Configure</strong>, then choose <strong>View raw config</strong> to open <code>mcp_config.json</code>.</>,
            <>Insert the snippet under <code>mcpServers</code>, save, and click <strong>Refresh</strong> on Manage Plugins to reload tools.</>,
          ]
        : [
            <>在 Windsurf 中点击 MCP 侧边栏的锤子图标 → <strong>Configure</strong>，再选择 <strong>View raw config</strong> 打开 <code>mcp_config.json</code>。</>,
            <>将下方片段写入 <code>mcpServers</code>，保存后在 Manage Plugins 页点击 <strong>Refresh</strong> 以加载新工具。</>,
          ],
      sampleTitle: isEnglish ? 'Example: ~/.codeium/windsurf/mcp_config.json' : '示例：~/.codeium/windsurf/mcp_config.json',
      snippetLanguage: 'json',
      snippet: genericJsonSnippet,
      reference: {
        label: 'NocoDB MCP docs',
        url: NOCODB_DOC_URL,
      },
    },
    other: {
      title: isEnglish ? 'Other clients' : '其他 MCP 客户端',
      steps: isEnglish
        ? [
            <>Endpoint: <code>{baseUrl}/mcp</code> (Streamable HTTP).</>,
            <>Auth: HTTP header <code>Authorization: Bearer {prettyToken}</code>.</>,
            <>Any MCP-compatible client can target this URL with the header attached.</>,
          ]
        : [
            <>端点：<code>{baseUrl}/mcp</code>（Streamable HTTP）。</>,
            <>认证：HTTP Header <code>Authorization: Bearer {prettyToken}</code>。</>,
            <>适用于任意兼容客户端，直接指向该 URL 并附带上述头部即可。</>,
          ],
      sampleTitle: isEnglish ? 'Example: generic request' : '示例：通用请求',
      snippetLanguage: 'bash',
      snippet: curlSnippet,
      reference: {
        label: 'Model Context Protocol spec',
        url: MCP_SPEC_URL,
      },
    },
  }
}

function buildCodexSnippet(baseUrl: string): string {
  return [
    '<span class="hl-comment"># ~/.codex/config.toml</span>',
    'experimental_use_rmcp_client = true',
    '',
    '[mcp_servers.tavily_hikari]',
    `url = "<span class="hl-string">${baseUrl}/mcp</span>"`,
    'bearer_token_env_var = "TAVILY_HIKARI_TOKEN"',
  ].join('\n')
}

function buildClaudeSnippet(baseUrl: string, prettyToken: string, language: Language): string {
  const verifyLabel = language === 'en' ? '# Verify' : '# 验证'
  return [
    '<span class="hl-comment"># claude mcp add-json</span>',
    `claude mcp add-json tavily-hikari '{`,
    `  <span class="hl-key">"type"</span>: <span class="hl-string">"http"</span>,`,
    `  <span class="hl-key">"url"</span>: <span class="hl-string">"${baseUrl}/mcp"</span>,`,
    '  <span class="hl-key">"headers"</span>: {',
    `    <span class="hl-key">"Authorization"</span>: <span class="hl-string">"Bearer ${prettyToken}"</span>`,
    '  }',
    "}'",
    '',
    verifyLabel,
    'claude mcp get tavily-hikari',
  ].join('\n')
}

function buildVscodeSnippet(baseUrl: string, prettyToken: string): string {
  return [
    '{',
    '  <span class="hl-key">"servers"</span>: {',
    '    <span class="hl-key">"tavily-hikari"</span>: {',
    '      <span class="hl-key">"type"</span>: <span class="hl-string">"http"</span>,',
    `      <span class="hl-key">"url"</span>: <span class="hl-string">"${baseUrl}/mcp"</span>,`,
    '      <span class="hl-key">"headers"</span>: {',
    `        <span class="hl-key">"Authorization"</span>: <span class="hl-string">"Bearer ${prettyToken}"</span>`,
    '      }',
    '    }',
    '  }',
    '}',
  ].join('\n')
}

function buildGenericJsonSnippet(baseUrl: string, prettyToken: string): string {
  return `{
  <span class="hl-key">"mcpServers"</span>: {
    <span class="hl-key">"tavily-hikari"</span>: {
      <span class="hl-key">"type"</span>: <span class="hl-string">"http"</span>,
      <span class="hl-key">"url"</span>: <span class="hl-string">"${baseUrl}/mcp"</span>,
      <span class="hl-key">"headers"</span>: {
        <span class="hl-key">"Authorization"</span>: <span class="hl-string">"Bearer ${prettyToken}"</span>
      }
    }
  }
}`
}

function buildCurlSnippet(baseUrl: string, prettyToken: string): string {
  return `curl -X POST \\
  -H "Content-Type: application/json" \\
  -H "Authorization: Bearer ${prettyToken}" \\
  ${baseUrl}/mcp`
}

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
