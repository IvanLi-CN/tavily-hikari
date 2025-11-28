import React, { ReactNode, useCallback, useEffect, useMemo, useState } from 'react'
import { Icon } from '@iconify/react'
import { StatusBadge, type StatusTone } from './components/StatusBadge'
import {
  fetchPublicMetrics,
  fetchProfile,
  fetchSummary,
  fetchTokenMetrics,
  fetchPublicLogs,
  type Profile,
  type PublicMetrics,
  type Summary,
  type TokenMetrics,
  type PublicTokenLog,
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
// Keep in sync with backend constants in src/lib.rs
const TOKEN_HOURLY_LIMIT = 100
const TOKEN_DAILY_LIMIT = 500
const TOKEN_MONTHLY_LIMIT = 5000

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
  // No default token on public page. Start empty.
  const strings = useTranslate()
  const publicStrings = strings.public
  const { language } = useLanguage()
  const [token, setToken] = useState('')
  const [tokenVisible, setTokenVisible] = useState(false)
  const [metrics, setMetrics] = useState<PublicMetrics | null>(null)
  const [tokenMetrics, setTokenMetrics] = useState<TokenMetrics | null>(null)
  const [publicLogs, setPublicLogs] = useState<PublicTokenLog[]>([])
  const [expandedPublicLogs, setExpandedPublicLogs] = useState<Set<number>>(() => new Set())
  const [publicLogsLoading, setPublicLogsLoading] = useState(false)
  const [invalidToken, setInvalidToken] = useState(false)
  const [summary, setSummary] = useState<Summary | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [profile, setProfile] = useState<Profile | null>(null)
  const [activeGuide, setActiveGuide] = useState<GuideKey>('codex')
  const updateBanner = useUpdateAvailable()
  const [copyState, setCopyState] = useState<'idle' | 'copied' | 'error'>('idle')
  const [isMobileGuide, setIsMobileGuide] = useState(false)
  const [recentTokenUsage, setRecentTokenUsage] = useState<TokenMetrics | null>(null)

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

    // Do not set any default token when none is provided
    if (initialToken) {
      persistToken(initialToken)
    }

    const controller = new AbortController()
    setLoading(true)
    Promise.allSettled([
      fetchPublicMetrics(controller.signal),
      fetchProfile(controller.signal),
      fetchSummary(controller.signal),
      initialToken && isFullToken(initialToken) ? fetchTokenMetrics(initialToken, controller.signal) : Promise.resolve(null),
    ])
      .then(([metricsResult, profileResult, summaryResult, tokenMetricsResult]) => {
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
          setInvalidToken(false)
          if (tokenMetricsResult && tokenMetricsResult.status === 'fulfilled') {
            setTokenMetrics(tokenMetricsResult.value)
            setRecentTokenUsage(tokenMetricsResult.value)
          }
          setPublicLogsLoading(true)
          fetchPublicLogs(initialToken, 20, controller.signal)
            .then((ls) => {
              setPublicLogs(ls)
              setInvalidToken(false)
            })
            .catch((err: any) => {
              setPublicLogs([])
              setInvalidToken(Boolean(err?.status) && err.status >= 400 && err.status < 500)
            })
            .finally(() => setPublicLogsLoading(false))
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
          const next: TokenMetrics = {
            monthlySuccess: data.token.monthlySuccess,
            dailySuccess: data.token.dailySuccess,
            dailyFailure: data.token.dailyFailure,
            quotaHourlyUsed: data.token.quotaHourlyUsed ?? 0,
            quotaHourlyLimit: data.token.quotaHourlyLimit ?? TOKEN_HOURLY_LIMIT,
            quotaDailyUsed: data.token.quotaDailyUsed ?? 0,
            quotaDailyLimit: data.token.quotaDailyLimit ?? TOKEN_DAILY_LIMIT,
            quotaMonthlyUsed: data.token.quotaMonthlyUsed ?? 0,
            quotaMonthlyLimit: data.token.quotaMonthlyLimit ?? TOKEN_MONTHLY_LIMIT,
          }
          setTokenMetrics(next)
          setRecentTokenUsage(next)
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

  // Watch viewport for small-screen guide dropdown
  useEffect(() => {
    const mq = window.matchMedia('(max-width: 640px)')
    const apply = () => setIsMobileGuide(mq.matches)
    apply()
    mq.addEventListener('change', apply)
    return () => mq.removeEventListener('change', apply)
  }, [])

  // Fallback polling: if token metrics未就绪或 SSE 不返回 token 段，定期补一次拉取
  useEffect(() => {
    if (!token || !isFullToken(token)) return
    let active = true
    const tick = async () => {
      try {
        const tm = await fetchTokenMetrics(token)
        if (!active) return
        setTokenMetrics(tm)
        setRecentTokenUsage(tm)
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
    // Show placeholder in guides when no valid token is present to avoid confusion
    const prettyToken = isFullToken(token) ? token : publicStrings.accessToken.placeholder
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
      setTokenMetrics(null)
      setPublicLogs([])
      setInvalidToken(true)
      return
    }
    setInvalidToken(false)

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
    // Fetch token-scoped metrics and recent logs
    void fetchTokenMetrics(next)
      .then((tm) => {
        setTokenMetrics(tm)
        setRecentTokenUsage(tm)
      })
      .catch(() => {
        setTokenMetrics(null)
        setRecentTokenUsage(null)
      })
    setPublicLogsLoading(true)
    void fetchPublicLogs(next, 20)
      .then((ls) => { setPublicLogs(ls); setInvalidToken(false) })
      .catch((err: any) => { setPublicLogs([]); setInvalidToken(Boolean(err?.status) && err.status >= 400 && err.status < 500) })
      .finally(() => setPublicLogsLoading(false))
  }, [])

  const togglePublicLog = useCallback((id: number) => {
    setExpandedPublicLogs((prev) => {
      const copy = new Set(prev)
      if (copy.has(id)) copy.delete(id)
      else copy.add(id)
      return copy
    })
  }, [])

  const formatTimestamp = (ts: number): string => {
    try {
      const d = new Date(ts * 1000)
      return d.toLocaleString()
    } catch {
      return String(ts)
    }
  }

  const statusTone = (status: string): StatusTone => {
    const normalized = status.toLowerCase()
    if (normalized === 'active' || normalized === 'success') return 'success'
    if (normalized === 'exhausted' || normalized === 'quota_exhausted') return 'warning'
    if (normalized === 'error') return 'error'
    return 'neutral'
  }

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
            <button type="button" className="btn btn-primary" onClick={updateBanner.reload}>
              {publicStrings.updateBanner.refresh}
            </button>
            <button type="button" className="btn btn-ghost" onClick={updateBanner.dismiss}>
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
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => { window.location.href = '/admin' }}
            >
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
            {/* Group 1: usage counts */}
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
          <div className="access-stats">
            {/* Group 2: rolling quota limits, styled similar to admin quick stats */}
            <div className="access-stat quota-stat-card">
              <div className="quota-stat-label">{publicStrings.accessPanel.stats.hourlyLimit}</div>
              <div className="quota-stat-value">
                {formatNumber(recentTokenUsage?.quotaHourlyUsed ?? 0)}
                <span>/ {formatNumber(recentTokenUsage?.quotaHourlyLimit ?? TOKEN_HOURLY_LIMIT)}</span>
              </div>
              <div className="quota-stat-description">Rolling 1-hour window</div>
            </div>
            <div className="access-stat quota-stat-card">
              <div className="quota-stat-label">{publicStrings.accessPanel.stats.dailyLimit}</div>
              <div className="quota-stat-value">
                {formatNumber(recentTokenUsage?.quotaDailyUsed ?? 0)}
                <span>/ {formatNumber(recentTokenUsage?.quotaDailyLimit ?? TOKEN_DAILY_LIMIT)}</span>
              </div>
              <div className="quota-stat-description">Rolling 24-hour window</div>
            </div>
            <div className="access-stat quota-stat-card">
              <div className="quota-stat-label">{publicStrings.accessPanel.stats.monthlyLimit}</div>
              <div className="quota-stat-value">
                {formatNumber(recentTokenUsage?.quotaMonthlyUsed ?? 0)}
                <span>/ {formatNumber(recentTokenUsage?.quotaMonthlyLimit ?? TOKEN_MONTHLY_LIMIT)}</span>
              </div>
              <div className="quota-stat-description">Calendar month</div>
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
                  name="not-a-login-field"
                  className={`token-input${tokenVisible ? '' : ' masked'}`}
                  // Always use text input to avoid triggering password managers
                  type="text"
                  value={token}
                  onChange={(event) => {
                    const value = event.target.value
                    setToken(value)
                  }}
                  onBlur={(event) => {
                    const next = event.target.value
                    persistToken(next)
                    if (isFullToken(next)) {
                      fetchTokenMetrics(next)
                        .then((tm) => {
                          setTokenMetrics(tm)
                          setRecentTokenUsage(tm)
                        })
                        .catch(() => {
                          setTokenMetrics(null)
                          setRecentTokenUsage(null)
                        })
                    }
                  }}
                  placeholder={publicStrings.accessToken.placeholder}
                  autoComplete="off"
                  autoCorrect="off"
                  autoCapitalize="off"
                  spellCheck={false}
                  aria-autocomplete="none"
                  inputMode="text"
                  data-1p-ignore="true"
                  data-lpignore="true"
                  data-form-type="other"
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
                className={`btn token-copy-button${
                  copyState === 'copied' ? ' btn-success' : ' btn-secondary'
                }`}
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
      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{publicStrings.logs.title}</h2>
            <p className="panel-description">{publicStrings.logs.description}</p>
          </div>
        </div>
        <div className="table-wrapper">
          {(!isFullToken(token) || invalidToken) ? (
            <div className="empty-state alert">
              <p style={{ margin: 0 }}>
                {publicStrings.logs.empty.noToken}{' '}
                <span style={{ opacity: 0.9 }}>{publicStrings.logs.empty.hint}</span>
              </p>
            </div>
          ) : publicLogsLoading ? (
            <div className="empty-state alert">{publicStrings.logs.empty.loading}</div>
          ) : publicLogs.length === 0 ? (
            <div className="empty-state alert">{publicStrings.logs.empty.none}</div>
          ) : (
            <table className="token-detail-table">
              <thead>
                <tr>
                  <th>{publicStrings.logs.table.time}</th>
                  <th>{publicStrings.logs.table.httpStatus}</th>
                  <th>{publicStrings.logs.table.mcpStatus}</th>
                  <th>{publicStrings.logs.table.result}</th>
                </tr>
              </thead>
              <tbody>
                {publicLogs.map((log) => (
                  <React.Fragment key={log.id}>
                    <tr>
                      <td>{formatTimestamp(log.created_at)}</td>
                      <td>{log.http_status ?? '—'}</td>
                      <td>{log.mcp_status ?? '—'}</td>
                      <td>
                        <button
                          type="button"
                          className={`log-result-button${expandedPublicLogs.has(log.id) ? ' log-result-button-active' : ''}`}
                          onClick={() => togglePublicLog(log.id)}
                          aria-expanded={expandedPublicLogs.has(log.id)}
                          aria-controls={`plog-${log.id}`}
                          aria-label={expandedPublicLogs.has(log.id) ? publicStrings.logs.toggles.hide : publicStrings.logs.toggles.show}
                          title={expandedPublicLogs.has(log.id) ? publicStrings.logs.toggles.hide : publicStrings.logs.toggles.show}
                        >
                          <StatusBadge tone={statusTone(log.result_status)}>
                            {log.result_status}
                          </StatusBadge>
                          <Icon
                            icon={expandedPublicLogs.has(log.id) ? 'mdi:chevron-up' : 'mdi:chevron-down'}
                            width={18}
                            height={18}
                            className="log-result-icon"
                          />
                        </button>
                      </td>
                    </tr>
                    {expandedPublicLogs.has(log.id) && (
                      <tr className="log-details-row">
                        <td colSpan={4} id={`plog-${log.id}`}>
                          <div className="log-details-panel">
                            <div className="log-details-summary">
                              <div>
                                <span className="log-details-label">Request</span>
                                <span className="log-details-value">{`${log.method} ${log.path}${log.query ? `?${log.query}` : ''}`}</span>
                              </div>
                              <div>
                                <span className="log-details-label">Response</span>
                                <span className="log-details-value">{`${publicStrings.logs.table.httpStatus}: ${log.http_status ?? '—'} · ${publicStrings.logs.table.mcpStatus}: ${log.mcp_status ?? '—'}`}</span>
                              </div>
                              <div>
                                <span className="log-details-label">Outcome</span>
                                <span className="log-details-value">{log.result_status}</span>
                              </div>
                              {log.error_message && (
                                <div>
                                  <span className="log-details-label">Error</span>
                                  <span className="log-details-value">{log.error_message}</span>
                                </div>
                              )}
                            </div>
                          </div>
                        </td>
                      </tr>
                    )}
                  </React.Fragment>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </section>
      <section className="surface panel public-home-guide">
        <h2>{publicStrings.guide.title}</h2>
        {/* Mobile: DaisyUI dropdown menu with icons */}
        {isMobileGuide && (
          <div className="guide-select" aria-label="Client selector (mobile)">
            <MobileGuideDropdown
              active={activeGuide}
              onChange={(id) => setActiveGuide(id)}
              labels={guideTabs}
            />
          </div>
        )}
        {!isMobileGuide && (
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
        )}
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
              <div className="mockup-code relative guide-code-shell">
                <span className="guide-lang-badge badge badge-outline badge-sm">
                  {(guideDescription.snippetLanguage ?? 'code').toUpperCase()}
                </span>
                <pre>
                  <code dangerouslySetInnerHTML={{ __html: guideDescription.snippet }} />
                </pre>
              </div>
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

function MobileGuideDropdown({
  active,
  onChange,
  labels,
}: {
  active: GuideKey
  onChange: (id: GuideKey) => void
  labels: { id: GuideKey, label: string }[]
}): JSX.Element {
  const icon = (id: GuideKey) => {
    // Prefer brand logos from simple-icons when available; fallback to MDI
    const map: Record<GuideKey, string> = {
      codex: 'simple-icons/openai',
      claude: 'simple-icons/anthropic',
      vscode: 'simple-icons/visualstudiocode',
      claudeDesktop: 'simple-icons/anthropic',
      cursor: 'simple-icons/cursor', // if missing, Iconify returns 404; UI will still show label
      windsurf: 'simple-icons/codeium',
      other: 'mdi/dots-horizontal',
    }
    const key = map[id] ?? 'mdi/dots-horizontal'
    return `${ICONIFY_ENDPOINT}/${key}.svg?color=%23475569`
  }

  const current = labels.find((l) => l.id === active)
  return (
    <div className="dropdown w-full">
      <div tabIndex={0} role="button" className="btn btn-outline w-full justify-between btn-sm md:btn-md">
        <span className="inline-flex items-center gap-2">
          <img src={icon(active)} alt="client" width={18} height={18} />
          {current?.label ?? active}
        </span>
        <img src={`${ICONIFY_ENDPOINT}/mdi/chevron-down.svg?color=%23647589`} alt="open" width={16} height={16} />
      </div>
      <ul tabIndex={0} className="menu dropdown-content bg-base-100 rounded-box z-[1] w-60 p-2 shadow mt-2">
        {labels.map((tab) => (
          <li key={tab.id}>
            <button type="button" onClick={() => onChange(tab.id)} className="flex items-center gap-2">
              <img src={icon(tab.id)} alt="" width={16} height={16} />
              <span className="truncate">{tab.label}</span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  )
}
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
