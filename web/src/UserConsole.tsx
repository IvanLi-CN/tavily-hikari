import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { Icon } from '@iconify/react'

import {
  fetchProfile,
  fetchUserDashboard,
  fetchUserTokenDetail,
  fetchUserTokenLogs,
  fetchUserTokenSecret,
  fetchUserTokens,
  type Profile,
  type PublicTokenLog,
  type UserDashboard,
  type UserTokenSummary,
} from './api'
import LanguageSwitcher from './components/LanguageSwitcher'
import RollingNumber from './components/RollingNumber'
import { StatusBadge, type StatusTone } from './components/StatusBadge'
import ThemeToggle from './components/ThemeToggle'
import { useLanguage } from './i18n'

const REPO_URL = 'https://github.com/IvanLi-CN/tavily-hikari'

type ConsoleRoute =
  | { name: 'dashboard' }
  | { name: 'tokens' }
  | { name: 'token'; id: string }

const numberFormatter = new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 })

function formatNumber(value: number): string {
  return numberFormatter.format(value)
}

function parseRouteFromHash(): ConsoleRoute {
  const hash = window.location.hash || ''
  const tokenMatch = hash.match(/^#\/tokens\/([^/?#]+)/)
  if (tokenMatch) {
    return { name: 'token', id: decodeURIComponent(tokenMatch[1]) }
  }
  if (hash.startsWith('#/tokens')) {
    return { name: 'tokens' }
  }
  return { name: 'dashboard' }
}

function statusTone(status: string): StatusTone {
  if (status === 'success') return 'success'
  if (status === 'error') return 'error'
  if (status === 'quota_exhausted') return 'warning'
  return 'neutral'
}

function formatTimestamp(ts: number): string {
  try {
    return new Date(ts * 1000).toLocaleString()
  } catch {
    return String(ts)
  }
}

function tokenLabel(tokenId: string): string {
  return `th-${tokenId}-************************`
}

export default function UserConsole(): JSX.Element {
  const language = useLanguage().language
  const text = language === 'zh' ? ZH : EN

  const [profile, setProfile] = useState<Profile | null>(null)
  const [dashboard, setDashboard] = useState<UserDashboard | null>(null)
  const [tokens, setTokens] = useState<UserTokenSummary[]>([])
  const [route, setRoute] = useState<ConsoleRoute>(() => parseRouteFromHash())
  const [detail, setDetail] = useState<UserTokenSummary | null>(null)
  const [detailLogs, setDetailLogs] = useState<PublicTokenLog[]>([])
  const [loading, setLoading] = useState(true)
  const [detailLoading, setDetailLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [copyState, setCopyState] = useState<Record<string, 'idle' | 'copied' | 'error'>>({})

  useEffect(() => {
    const onHash = () => setRoute(parseRouteFromHash())
    window.addEventListener('hashchange', onHash)
    return () => window.removeEventListener('hashchange', onHash)
  }, [])

  const reloadBase = useCallback(async () => {
    const controller = new AbortController()
    try {
      const [nextProfile, nextDashboard, nextTokens] = await Promise.all([
        fetchProfile(controller.signal),
        fetchUserDashboard(controller.signal),
        fetchUserTokens(controller.signal),
      ])
      setProfile(nextProfile)
      if (nextProfile.userLoggedIn === false) {
        window.location.href = '/'
        return
      }
      setDashboard(nextDashboard)
      setTokens(nextTokens)
      setError(null)
    } catch (err) {
      const message = err instanceof Error ? err.message : text.errors.load
      setError(message)
      if (/401/.test(message)) {
        window.location.href = '/'
      }
    } finally {
      setLoading(false)
    }
    return () => controller.abort()
  }, [text.errors.load])

  useEffect(() => {
    void reloadBase()
  }, [reloadBase])

  useEffect(() => {
    if (route.name !== 'token') {
      setDetail(null)
      setDetailLogs([])
      return
    }
    setDetailLoading(true)
    const controller = new AbortController()
    Promise.all([
      fetchUserTokenDetail(route.id, controller.signal),
      fetchUserTokenLogs(route.id, 20, controller.signal),
    ])
      .then(([nextDetail, nextLogs]) => {
        setDetail(nextDetail)
        setDetailLogs(nextLogs)
        setError(null)
      })
      .catch((err) => {
        setDetail(null)
        setDetailLogs([])
        setError(err instanceof Error ? err.message : text.errors.detail)
      })
      .finally(() => setDetailLoading(false))
    return () => controller.abort()
  }, [route, text.errors.detail])

  const copyToken = useCallback(async (tokenId: string) => {
    try {
      const { token } = await fetchUserTokenSecret(tokenId)
      await navigator.clipboard.writeText(token)
      setCopyState((prev) => ({ ...prev, [tokenId]: 'copied' }))
    } catch {
      setCopyState((prev) => ({ ...prev, [tokenId]: 'error' }))
    }
    window.setTimeout(() => {
      setCopyState((prev) => ({ ...prev, [tokenId]: 'idle' }))
    }, 1800)
  }, [])

  const subtitle = useMemo(() => {
    const user = profile?.userDisplayName?.trim()
    if (user && user.length > 0) {
      return `${text.subtitle} · ${user}`
    }
    return text.subtitle
  }, [profile?.userDisplayName, text.subtitle])

  const goDashboard = () => {
    window.location.hash = '#/dashboard'
  }
  const goTokens = () => {
    window.location.hash = '#/tokens'
  }
  const goTokenDetail = (tokenId: string) => {
    window.location.hash = `#/tokens/${encodeURIComponent(tokenId)}`
  }

  return (
    <main className="app-shell public-home">
      <section className="surface app-header admin-panel-header">
        <div className="admin-panel-header-main">
          <h1>{text.title}</h1>
          <p className="admin-panel-header-subtitle">{subtitle}</p>
        </div>
        <div className="admin-panel-header-side">
          <div className="admin-panel-header-tools">
            <div className="admin-language-switcher">
              <ThemeToggle />
              <LanguageSwitcher />
            </div>
          </div>
        </div>
      </section>

      <section className="surface panel" style={{ marginBottom: 16 }}>
        <div className="table-actions">
          <button type="button" className={`btn ${route.name === 'dashboard' ? 'btn-primary' : 'btn-outline'}`} onClick={goDashboard}>
            {text.nav.dashboard}
          </button>
          <button type="button" className={`btn ${route.name !== 'dashboard' ? 'btn-primary' : 'btn-outline'}`} onClick={goTokens}>
            {text.nav.tokens}
          </button>
        </div>
      </section>

      {error && <section className="surface error-banner">{error}</section>}

      {route.name === 'dashboard' && (
        <>
          <section className="surface panel access-panel">
            <header className="panel-header">
              <h2>{text.dashboard.usage}</h2>
            </header>
            <div className="access-stats">
              <div className="access-stat">
                <h4>{text.dashboard.dailySuccess}</h4>
                <p><RollingNumber value={loading ? null : dashboard?.dailySuccess ?? 0} /></p>
              </div>
              <div className="access-stat">
                <h4>{text.dashboard.dailyFailure}</h4>
                <p><RollingNumber value={loading ? null : dashboard?.dailyFailure ?? 0} /></p>
              </div>
              <div className="access-stat">
                <h4>{text.dashboard.monthlySuccess}</h4>
                <p><RollingNumber value={loading ? null : dashboard?.monthlySuccess ?? 0} /></p>
              </div>
            </div>
            <div className="access-stats">
              <div className="access-stat quota-stat-card">
                <div className="quota-stat-label">{text.dashboard.hourlyAny}</div>
                <div className="quota-stat-value">
                  {formatNumber(dashboard?.hourlyAnyUsed ?? 0)}
                  <span>/ {formatNumber(dashboard?.hourlyAnyLimit ?? 0)}</span>
                </div>
              </div>
              <div className="access-stat quota-stat-card">
                <div className="quota-stat-label">{text.dashboard.hourly}</div>
                <div className="quota-stat-value">
                  {formatNumber(dashboard?.quotaHourlyUsed ?? 0)}
                  <span>/ {formatNumber(dashboard?.quotaHourlyLimit ?? 0)}</span>
                </div>
              </div>
              <div className="access-stat quota-stat-card">
                <div className="quota-stat-label">{text.dashboard.daily}</div>
                <div className="quota-stat-value">
                  {formatNumber(dashboard?.quotaDailyUsed ?? 0)}
                  <span>/ {formatNumber(dashboard?.quotaDailyLimit ?? 0)}</span>
                </div>
              </div>
              <div className="access-stat quota-stat-card">
                <div className="quota-stat-label">{text.dashboard.monthly}</div>
                <div className="quota-stat-value">
                  {formatNumber(dashboard?.quotaMonthlyUsed ?? 0)}
                  <span>/ {formatNumber(dashboard?.quotaMonthlyLimit ?? 0)}</span>
                </div>
              </div>
            </div>
          </section>
        </>
      )}

      {route.name === 'tokens' && (
        <section className="surface panel">
          <div className="panel-header">
            <h2>{text.tokens.title}</h2>
          </div>
          <div className="table-wrapper jobs-table-wrapper">
            {tokens.length === 0 ? (
              <div className="empty-state alert">{text.tokens.empty}</div>
            ) : (
              <table className="jobs-table tokens-table">
                <thead>
                  <tr>
                    <th>{text.tokens.table.id}</th>
                    <th>{text.tokens.table.any}</th>
                    <th>{text.tokens.table.hourly}</th>
                    <th>{text.tokens.table.daily}</th>
                    <th>{text.tokens.table.monthly}</th>
                    <th>{text.tokens.table.dailySuccess}</th>
                    <th>{text.tokens.table.dailyFailure}</th>
                    <th>{text.tokens.table.actions}</th>
                  </tr>
                </thead>
                <tbody>
                  {tokens.map((item) => {
                    const state = copyState[item.tokenId] ?? 'idle'
                    return (
                      <tr key={item.tokenId}>
                        <td><code>{item.tokenId}</code></td>
                        <td>{formatNumber(item.hourlyAnyUsed)} / {formatNumber(item.hourlyAnyLimit)}</td>
                        <td>{formatNumber(item.quotaHourlyUsed)} / {formatNumber(item.quotaHourlyLimit)}</td>
                        <td>{formatNumber(item.quotaDailyUsed)} / {formatNumber(item.quotaDailyLimit)}</td>
                        <td>{formatNumber(item.quotaMonthlyUsed)} / {formatNumber(item.quotaMonthlyLimit)}</td>
                        <td>{formatNumber(item.dailySuccess)}</td>
                        <td>{formatNumber(item.dailyFailure)}</td>
                        <td>
                          <div className="table-actions">
                            <button
                              type="button"
                              className={`btn btn-outline btn-sm ${state === 'copied' ? 'btn-success' : state === 'error' ? 'btn-warning' : ''}`}
                              onClick={() => void copyToken(item.tokenId)}
                            >
                              {state === 'copied' ? text.tokens.copied : state === 'error' ? text.tokens.copyFailed : text.tokens.copy}
                            </button>
                            <button type="button" className="btn btn-primary btn-sm" onClick={() => goTokenDetail(item.tokenId)}>
                              {text.tokens.detail}
                            </button>
                          </div>
                        </td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            )}
          </div>
        </section>
      )}

      {route.name === 'token' && (
        <>
          <section className="surface panel access-panel">
            <header className="panel-header" style={{ marginBottom: 8 }}>
              <div>
                <h2>{text.detail.title} <code>{route.id}</code></h2>
                <p className="panel-description">{text.detail.subtitle}</p>
              </div>
              <button type="button" className="btn btn-outline" onClick={goTokens}>{text.detail.back}</button>
            </header>

            <div className="access-stats">
              <div className="access-stat">
                <h4>{text.dashboard.dailySuccess}</h4>
                <p><RollingNumber value={detailLoading ? null : detail?.dailySuccess ?? 0} /></p>
              </div>
              <div className="access-stat">
                <h4>{text.dashboard.dailyFailure}</h4>
                <p><RollingNumber value={detailLoading ? null : detail?.dailyFailure ?? 0} /></p>
              </div>
              <div className="access-stat">
                <h4>{text.dashboard.monthlySuccess}</h4>
                <p><RollingNumber value={detailLoading ? null : detail?.monthlySuccess ?? 0} /></p>
              </div>
            </div>
            <div className="access-stats">
              <div className="access-stat quota-stat-card">
                <div className="quota-stat-label">{text.dashboard.hourlyAny}</div>
                <div className="quota-stat-value">
                  {formatNumber(detail?.hourlyAnyUsed ?? 0)}
                  <span>/ {formatNumber(detail?.hourlyAnyLimit ?? 0)}</span>
                </div>
              </div>
              <div className="access-stat quota-stat-card">
                <div className="quota-stat-label">{text.dashboard.hourly}</div>
                <div className="quota-stat-value">
                  {formatNumber(detail?.quotaHourlyUsed ?? 0)}
                  <span>/ {formatNumber(detail?.quotaHourlyLimit ?? 0)}</span>
                </div>
              </div>
              <div className="access-stat quota-stat-card">
                <div className="quota-stat-label">{text.dashboard.daily}</div>
                <div className="quota-stat-value">
                  {formatNumber(detail?.quotaDailyUsed ?? 0)}
                  <span>/ {formatNumber(detail?.quotaDailyLimit ?? 0)}</span>
                </div>
              </div>
              <div className="access-stat quota-stat-card">
                <div className="quota-stat-label">{text.dashboard.monthly}</div>
                <div className="quota-stat-value">
                  {formatNumber(detail?.quotaMonthlyUsed ?? 0)}
                  <span>/ {formatNumber(detail?.quotaMonthlyLimit ?? 0)}</span>
                </div>
              </div>
            </div>

            <div className="access-token-box">
              <label className="token-label">{text.detail.tokenLabel}</label>
              <div className="token-input-row">
                <div className="token-input-shell">
                  <input className="token-input" type="text" value={tokenLabel(route.id)} readOnly />
                </div>
                <button type="button" className="btn token-copy-button btn-outline" onClick={() => void copyToken(route.id)}>
                  <Icon icon="mdi:content-copy" className="token-copy-icon" />
                  <span>{text.tokens.copy}</span>
                </button>
              </div>
            </div>
          </section>

          <section className="surface panel">
            <div className="panel-header">
              <h2>{text.detail.logs}</h2>
            </div>
            <div className="table-wrapper">
              {detailLogs.length === 0 ? (
                <div className="empty-state alert">{text.detail.emptyLogs}</div>
              ) : (
                <table className="token-detail-table">
                  <thead>
                    <tr>
                      <th>{text.detail.table.time}</th>
                      <th>{text.detail.table.http}</th>
                      <th>{text.detail.table.mcp}</th>
                      <th>{text.detail.table.result}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {detailLogs.map((log) => (
                      <tr key={log.id}>
                        <td>{formatTimestamp(log.created_at)}</td>
                        <td>{log.http_status ?? '—'}</td>
                        <td>{log.mcp_status ?? '—'}</td>
                        <td>
                          <StatusBadge tone={statusTone(log.result_status)}>{log.result_status}</StatusBadge>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
          </section>

          <section className="surface panel public-home-guide">
            <h2>{text.detail.guideTitle}</h2>
            <p>{text.detail.guideDescription}</p>
            <div className="mockup-code relative guide-code-shell">
              <pre>
                <code>{`[mcp_servers.tavily_hikari]\nurl = "${window.location.origin}/mcp"\nbearer_token_env_var = "TAVILY_HIKARI_TOKEN"`}</code>
              </pre>
            </div>
          </section>

          <footer className="surface public-home-footer">
            <a className="footer-gh" href={REPO_URL} target="_blank" rel="noreferrer">
              <img src="https://api.iconify.design/mdi/github.svg?color=%232563eb" alt="GitHub" />
              <span>GitHub</span>
            </a>
          </footer>
        </>
      )}
    </main>
  )
}

const EN = {
  title: 'User Console',
  subtitle: 'Your account dashboard and token management',
  nav: {
    dashboard: 'Dashboard',
    tokens: 'Token Management',
  },
  dashboard: {
    usage: 'Account Usage Overview',
    dailySuccess: 'Daily Success',
    dailyFailure: 'Daily Failure',
    monthlySuccess: 'Monthly Success',
    hourlyAny: 'Hourly Any Requests',
    hourly: 'Hourly Quota',
    daily: 'Daily Quota',
    monthly: 'Monthly Quota',
  },
  tokens: {
    title: 'Token List',
    empty: 'No token available for this account.',
    copy: 'Copy',
    copied: 'Copied',
    copyFailed: 'Copy failed',
    detail: 'Details',
    table: {
      id: 'Token ID',
      any: 'Any Req (1h)',
      hourly: 'Hourly',
      daily: 'Daily',
      monthly: 'Monthly',
      dailySuccess: 'Daily Success',
      dailyFailure: 'Daily Failure',
      actions: 'Actions',
    },
  },
  detail: {
    title: 'Token Detail',
    subtitle: 'Same token-level modules as home page (without global site card).',
    back: 'Back',
    tokenLabel: 'Token',
    logs: 'Recent Requests (20)',
    emptyLogs: 'No recent requests.',
    guideTitle: 'Client Setup',
    guideDescription: 'Use the same MCP configuration as the public homepage.',
    table: {
      time: 'Time',
      http: 'HTTP',
      mcp: 'MCP',
      result: 'Result',
    },
  },
  errors: {
    load: 'Failed to load console data',
    detail: 'Failed to load token detail',
  },
}

const ZH = {
  title: '用户控制台',
  subtitle: '账户仪表盘与 Token 管理',
  nav: {
    dashboard: '控制台仪表盘',
    tokens: 'Token 管理',
  },
  dashboard: {
    usage: '账户用量概览',
    dailySuccess: '今日成功',
    dailyFailure: '今日失败',
    monthlySuccess: '本月成功',
    hourlyAny: '每小时任意请求',
    hourly: '小时配额',
    daily: '日配额',
    monthly: '月配额',
  },
  tokens: {
    title: 'Token 列表',
    empty: '当前账户暂无 Token。',
    copy: '复制',
    copied: '已复制',
    copyFailed: '复制失败',
    detail: '详情',
    table: {
      id: 'Token ID',
      any: '任意请求(1h)',
      hourly: '小时',
      daily: '日',
      monthly: '月',
      dailySuccess: '今日成功',
      dailyFailure: '今日失败',
      actions: '操作',
    },
  },
  detail: {
    title: 'Token 详情',
    subtitle: '保留首页 token 相关模块（不展示首个站点全局卡片）。',
    back: '返回',
    tokenLabel: 'Token',
    logs: '近期请求（20 条）',
    emptyLogs: '暂无请求记录。',
    guideTitle: '客户端接入',
    guideDescription: '沿用首页的 MCP 配置方式即可接入。',
    table: {
      time: '时间',
      http: 'HTTP',
      mcp: 'MCP',
      result: '结果',
    },
  },
  errors: {
    load: '加载控制台数据失败',
    detail: '加载 Token 详情失败',
  },
}
