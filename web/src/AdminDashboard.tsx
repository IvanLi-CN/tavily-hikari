import { Icon } from '@iconify/react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import LanguageSwitcher from './components/LanguageSwitcher'
import TokenDetail from './pages/TokenDetail'
import { useTranslate, type AdminTranslations } from './i18n'
import {
  fetchApiKeys,
  fetchApiKeySecret,
  addApiKey,
  deleteApiKey,
  setKeyStatus,
  fetchProfile,
  fetchRequestLogs,
  fetchSummary,
  fetchVersion,
  type ApiKeyStats,
  type Profile,
  type RequestLog,
  type Summary,
  fetchTokens,
  type AuthToken,
  fetchTokenSecret,
  createToken,
  deleteToken,
  setTokenEnabled,
  updateTokenNote,
  createTokensBatch,
  type Paginated,
  fetchKeyMetrics,
  fetchKeyLogs,
  type KeySummary,
  fetchApiKeyDetail,
  syncApiKeyUsage,
  fetchJobs,
  fetchTokenGroups,
  type TokenGroup,
} from './api'

function parseHashForKeyId(): string | null {
  const hash = location.hash || ''
  const m = hash.match(/^#\/keys\/([^\/?#]+)/)
  return m ? decodeURIComponent(m[1]) : null
}

function parseHashForTokenId(): string | null {
  const hash = location.hash || ''
  const m = hash.match(/^#\/tokens\/([^\/?#]+)/)
  return m ? decodeURIComponent(m[1]) : null
}

const REFRESH_INTERVAL_MS = 30_000

const numberFormatter = new Intl.NumberFormat('en-US', {
  maximumFractionDigits: 0,
})

const percentageFormatter = new Intl.NumberFormat('en-US', {
  style: 'percent',
  minimumFractionDigits: 0,
  maximumFractionDigits: 1,
})

const dateTimeFormatter = new Intl.DateTimeFormat(undefined, {
  dateStyle: 'medium',
  timeStyle: 'medium',
})

// Date/time without year for compact "Last Used" rendering
const dateTimeNoYearFormatter = new Intl.DateTimeFormat(undefined, {
  month: 'short',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit',
  second: '2-digit',
  hour12: false,
})

// Time-only formatter for compact "Updated HH:MM:SS"
const timeOnlyFormatter = new Intl.DateTimeFormat(undefined, {
  hour: '2-digit',
  minute: '2-digit',
  second: '2-digit',
  hour12: false,
})

const tooltipTimeFormatter = new Intl.DateTimeFormat(undefined, {
  year: 'numeric',
  month: 'short',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit',
  second: '2-digit',
  hour12: false,
  fractionalSecondDigits: 3,
})

const relativeTimeFormatter = new Intl.RelativeTimeFormat(undefined, {
  numeric: 'auto',
})

function formatClockTime(value: number | null): string {
  if (!value) return '—'
  return timeOnlyFormatter.format(new Date(value * 1000))
}

function formatTimestampWithMs(value: number | null): string {
  if (!value) return '—'
  return tooltipTimeFormatter.format(new Date(value * 1000))
}

function formatRelativeTime(value: number | null): string {
  if (!value) return '—'
  const nowSeconds = Date.now() / 1000
  const diffSeconds = value - nowSeconds
  const divisions: Array<{ amount: number; unit: Intl.RelativeTimeFormatUnit }> = [
    { amount: 60, unit: 'second' },
    { amount: 60, unit: 'minute' },
    { amount: 24, unit: 'hour' },
    { amount: 7, unit: 'day' },
    { amount: 4.34524, unit: 'week' },
    { amount: 12, unit: 'month' },
    { amount: Number.POSITIVE_INFINITY, unit: 'year' },
  ]

  let duration = diffSeconds
  for (const division of divisions) {
    if (Math.abs(duration) < division.amount) {
      return relativeTimeFormatter.format(Math.round(duration), division.unit)
    }
    duration /= division.amount
  }
  return relativeTimeFormatter.format(Math.round(duration), 'year')
}

function formatNumber(value: number): string {
  return numberFormatter.format(value)
}

function formatPercent(numerator: number, denominator: number): string {
  if (denominator === 0) return '—'
  return percentageFormatter.format(numerator / denominator)
}

function formatTimestamp(value: number | null): string {
  if (!value) {
    return '—'
  }
  return dateTimeFormatter.format(new Date(value * 1000))
}

function formatTimestampNoYear(value: number | null): string {
  if (!value) return '—'
  return dateTimeNoYearFormatter.format(new Date(value * 1000))
}

function statusClass(status: string): string {
  const normalized = status.toLowerCase()
  if (normalized === 'active' || normalized === 'success') {
    return 'status-badge status-active'
  }
  if (normalized === 'exhausted' || normalized === 'quota_exhausted') {
    return 'status-badge status-exhausted'
  }
  // 'deleted' 由 deleted_at 字段控制，这里仅兜底
  if (normalized === 'deleted') return 'status-badge status-unknown'
  if (normalized === 'error') {
    return 'status-badge status-error'
  }
  return 'status-badge status-unknown'
}

function statusLabel(status: string, strings: AdminTranslations): string {
  const normalized = status.toLowerCase()
  return strings.statuses[normalized] ?? status
}

function formatErrorMessage(log: RequestLog, errorsStrings: AdminTranslations['logs']['errors']): string {
  const message = log.error_message?.trim()
  if (message) {
    return message
  }

  const status = log.result_status.toLowerCase()
  if (status === 'quota_exhausted') {
    if (log.http_status != null) {
      return errorsStrings.quotaExhaustedHttp.replace('{http}', String(log.http_status))
    }
    return errorsStrings.quotaExhausted
  }

  if (status === 'error') {
    if (log.http_status != null && log.mcp_status != null) {
      return errorsStrings.requestFailedHttpMcp
        .replace('{http}', String(log.http_status))
        .replace('{mcp}', String(log.mcp_status))
    }
    if (log.http_status != null) {
      return errorsStrings.requestFailedHttp.replace('{http}', String(log.http_status))
    }
    if (log.mcp_status != null) {
      return errorsStrings.requestFailedMcp.replace('{mcp}', String(log.mcp_status))
    }
    return errorsStrings.requestFailedGeneric
  }

  if (status === 'success') {
    return errorsStrings.none
  }

  if (log.http_status != null) {
    return errorsStrings.httpStatus.replace('{http}', String(log.http_status))
  }

  return errorsStrings.none
}

function AdminDashboard(): JSX.Element {
  const [route, setRoute] = useState<{ name: 'home' } | { name: 'key'; id: string } | { name: 'token'; id: string }>(() => {
    const keyId = parseHashForKeyId()
    if (keyId) return { name: 'key', id: keyId }
    const tokenId = parseHashForTokenId()
    return tokenId ? { name: 'token', id: tokenId } : { name: 'home' }
  })
  const translations = useTranslate()
  const adminStrings = translations.admin
  const headerStrings = adminStrings.header
  const tokenStrings = adminStrings.tokens
  const quotaLabels = tokenStrings.quotaStates ?? {
    normal: 'Normal',
    hour: '1 hour limit',
    day: '24 hour limit',
    month: 'Monthly limit',
  }
  const metricsStrings = adminStrings.metrics
  const keyStrings = adminStrings.keys
  const logStrings = adminStrings.logs
  const jobsStrings = adminStrings.jobs
  const footerStrings = adminStrings.footer
  const errorStrings = adminStrings.errors
  const [summary, setSummary] = useState<Summary | null>(null)
  const [keys, setKeys] = useState<ApiKeyStats[]>([])
  const [tokens, setTokens] = useState<AuthToken[]>([])
  const [tokensPage, setTokensPage] = useState(1)
  const tokensPerPage = 10
  const [tokensTotal, setTokensTotal] = useState(0)
  const [tokenGroups, setTokenGroups] = useState<TokenGroup[]>([])
  const [selectedTokenGroupName, setSelectedTokenGroupName] = useState<string | null>(null)
  const [selectedTokenUngrouped, setSelectedTokenUngrouped] = useState(false)
  const [tokenGroupsExpanded, setTokenGroupsExpanded] = useState(false)
  const [tokenGroupsCollapsedOverflowing, setTokenGroupsCollapsedOverflowing] = useState(false)
  const [logs, setLogs] = useState<RequestLog[]>([])
  const [jobs, setJobs] = useState<import('./api').JobLogView[]>([])
  const [jobFilter, setJobFilter] = useState<'all' | 'quota' | 'usage' | 'logs'>('all')
  const [jobsPage, setJobsPage] = useState(1)
  const jobsPerPage = 10
  const [jobsTotal, setJobsTotal] = useState(0)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const pollingTimerRef = useRef<number | null>(null)
  const [lastUpdated, setLastUpdated] = useState<Date | null>(null)
  const [version, setVersion] = useState<{ backend: string; frontend: string } | null>(null)
  const [profile, setProfile] = useState<Profile | null>(null)
  const secretCacheRef = useRef<Map<string, string>>(new Map())
  const tokenSecretCacheRef = useRef<Map<string, string>>(new Map())
  const tokenGroupsListRef = useRef<HTMLDivElement | null>(null)
  const [copyState, setCopyState] = useState<Map<string, 'loading' | 'copied'>>(() => new Map())
  const [expandedLogs, setExpandedLogs] = useState<Set<number>>(() => new Set())
  const [newKey, setNewKey] = useState('')
  const [newTokenNote, setNewTokenNote] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const [deletingId, setDeletingId] = useState<string | null>(null)
  const [togglingId, setTogglingId] = useState<string | null>(null)
  const deleteDialogRef = useRef<HTMLDialogElement | null>(null)
  const [pendingDeleteId, setPendingDeleteId] = useState<string | null>(null)
  const disableDialogRef = useRef<HTMLDialogElement | null>(null)
  const [pendingDisableId, setPendingDisableId] = useState<string | null>(null)
  const tokenDeleteDialogRef = useRef<HTMLDialogElement | null>(null)
  const [pendingTokenDeleteId, setPendingTokenDeleteId] = useState<string | null>(null)
  const tokenNoteDialogRef = useRef<HTMLDialogElement | null>(null)
  const [editingTokenId, setEditingTokenId] = useState<string | null>(null)
  const [editingTokenNote, setEditingTokenNote] = useState('')
  const [savingTokenNote, setSavingTokenNote] = useState(false)
  const [sseConnected, setSseConnected] = useState(false)
  const [expandedJobs, setExpandedJobs] = useState<Set<number>>(() => new Set())
  // Batch dialog state
  const batchDialogRef = useRef<HTMLDialogElement | null>(null)
  const [batchGroup, setBatchGroup] = useState('')
  const [batchCount, setBatchCount] = useState(10)
  const [batchCreating, setBatchCreating] = useState(false)
  const [batchShareText, setBatchShareText] = useState<string | null>(null)
  const isAdmin = profile?.isAdmin ?? false

  const copyStateKey = useCallback((scope: 'keys' | 'logs' | 'tokens', identifier: string | number) => {
    return `${scope}:${identifier}`
  }, [])

  const updateCopyState = useCallback((key: string, next: 'loading' | 'copied' | null) => {
    setCopyState((previous) => {
      const clone = new Map(previous)
      if (next === null) {
        clone.delete(key)
      } else {
        clone.set(key, next)
      }
      return clone
    })
  }, [])

  const copyToClipboard = useCallback(async (value: string) => {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(value)
      return
    }

    const textarea = document.createElement('textarea')
    textarea.value = value
    textarea.style.position = 'fixed'
    textarea.style.opacity = '0'
    textarea.style.left = '-9999px'
    document.body.appendChild(textarea)
    textarea.focus()
    textarea.select()
    document.execCommand('copy')
    document.body.removeChild(textarea)
  }, [])

  const resolveTokenSecret = useCallback(async (id: string) => {
    let secret = tokenSecretCacheRef.current.get(id)
    if (!secret) {
      const result = await fetchTokenSecret(id)
      secret = result.token
      tokenSecretCacheRef.current.set(id, secret)
    }
    return secret
  }, [])

  const handleCopySecret = useCallback(
    async (id: string, stateKey: string) => {
      updateCopyState(stateKey, 'loading')
      try {
        let secret = secretCacheRef.current.get(id)
        if (!secret) {
          const result = await fetchApiKeySecret(id)
          secret = result.api_key
          secretCacheRef.current.set(id, secret)
        }

        await copyToClipboard(secret)
        updateCopyState(stateKey, 'copied')
        window.setTimeout(() => updateCopyState(stateKey, null), 2000)
      } catch (err) {
        console.error(err)
        setError(err instanceof Error ? err.message : errorStrings.copyKey)
        updateCopyState(stateKey, null)
      }
    },
    [copyToClipboard, setError, updateCopyState],
  )

  const loadData = useCallback(
    async (signal?: AbortSignal) => {
      try {
        const [summaryData, keyData, logData, ver, profileData, tokenData, tokenGroupsData] = await Promise.all([
          fetchSummary(signal),
          fetchApiKeys(signal),
          fetchRequestLogs(50, signal),
          fetchVersion(signal).catch(() => null),
          fetchProfile(signal).catch(() => null),
          fetchTokens(
            tokensPage,
            tokensPerPage,
            { group: selectedTokenGroupName, ungrouped: selectedTokenUngrouped },
            signal,
          ).catch(
            () =>
              ({
                items: [],
                total: 0,
                page: tokensPage,
                perPage: tokensPerPage,
              }) as Paginated<AuthToken>,
          ),
          fetchTokenGroups(signal).catch(() => [] as TokenGroup[]),
        ])

        if (signal?.aborted) {
          return
        }

        setProfile(profileData ?? null)
        setSummary(summaryData)
        setKeys(keyData)
        setLogs(logData)
        setTokens(tokenData.items)
        setTokensTotal(tokenData.total)
        setTokenGroups(tokenGroupsData)
        setExpandedLogs((previous) => {
          if (previous.size === 0) {
            return new Set()
          }
          const validIds = new Set(logData.map((item) => item.id))
          const next = new Set<number>()
          for (const id of previous) {
            if (validIds.has(id)) {
              next.add(id)
            }
          }
          return next
        })
        setVersion(ver ?? null)
        setLastUpdated(new Date())
        setError(null)
      } catch (err) {
        if ((err as Error).name === 'AbortError') {
          return
        }
        setError(err instanceof Error ? err.message : 'Unexpected error occurred')
      } finally {
        if (!(signal?.aborted ?? false)) {
          setLoading(false)
        }
      }
  },
    [tokensPage, selectedTokenGroupName, selectedTokenUngrouped],
  )

  useEffect(() => {
    const controller = new AbortController()
    setLoading(true)
    void loadData(controller.signal)
    return () => controller.abort()
  }, [loadData])

  // Jobs list: refetch when filter or page changes
  useEffect(() => {
    const controller = new AbortController()
    fetchJobs(jobsPage, jobsPerPage, jobFilter, controller.signal)
      .then((result) => {
        if (!controller.signal.aborted) {
          setJobs(result.items)
          setJobsTotal(result.total)
          setExpandedJobs((previous) => {
            if (previous.size === 0) return new Set()
            const visibleIds = new Set(result.items.map((item) => item.id))
            const next = new Set<number>()
            for (const id of previous) {
              if (visibleIds.has(id)) next.add(id)
            }
            return next
          })
        }
      })
      .catch(() => {
        if (!controller.signal.aborted) {
          setJobs([])
          setJobsTotal(0)
        }
      })
    return () => controller.abort()
  }, [jobFilter, jobsPage])

  // Automatic fallback polling when SSE is not connected
  useEffect(() => {
    if (sseConnected) {
      if (pollingTimerRef.current != null) {
        window.clearInterval(pollingTimerRef.current)
        pollingTimerRef.current = null
      }
      return
    }

    if (pollingTimerRef.current == null) {
      pollingTimerRef.current = window.setInterval(() => {
        const controller = new AbortController()
        void loadData(controller.signal).finally(() => controller.abort())
      }, REFRESH_INTERVAL_MS) as unknown as number
    }

    return () => {
      if (pollingTimerRef.current != null) {
        window.clearInterval(pollingTimerRef.current)
        pollingTimerRef.current = null
      }
    }
  }, [sseConnected, loadData])

  // Detect whether the collapsed token groups row overflows horizontally.
  // If everything fits in a single line, we hide the "more" toggle button.
  useEffect(() => {
    if (!Array.isArray(tokenGroups) || tokenGroups.length === 0 || tokenGroupsExpanded) {
      setTokenGroupsCollapsedOverflowing(false)
      return
    }
    const el = tokenGroupsListRef.current
    if (!el) return

    const measure = () => {
      const overflowing = el.scrollWidth > el.clientWidth
      setTokenGroupsCollapsedOverflowing(overflowing)
    }

    measure()
    window.addEventListener('resize', measure)
    return () => window.removeEventListener('resize', measure)
  }, [tokenGroups, tokenGroupsExpanded, selectedTokenGroupName, selectedTokenUngrouped])

  // Establish SSE connection to receive live dashboard updates
  useEffect(() => {
    let es: EventSource | null = null

    const connect = () => {
      if (es) {
        try { es.close() } catch {}
        es = null
      }
      es = new EventSource('/api/events')
      es.onopen = () => { setSseConnected(true) }
      es.onerror = () => {
        // Trigger fallback polling; attempt reconnect automatically
        setSseConnected(false)
      }
      es.addEventListener('snapshot', (ev: MessageEvent) => {
        try {
          const data = JSON.parse(ev.data) as { summary: Summary; keys: ApiKeyStats[]; logs: RequestLog[] }
          setSummary(data.summary)
          setKeys(data.keys)
          setLogs(data.logs)
          setExpandedLogs((previous) => {
            // keep expansion only for visible ids
            const valid = new Set(data.logs.map((l) => l.id))
            const next = new Set<number>()
            for (const id of previous) if (valid.has(id)) next.add(id)
            return next
          })
          setLastUpdated(new Date())
          setError(null)
          setLoading(false)
        } catch (e) {
          console.error('SSE parse error', e)
        }
      })
    }

    connect()
    return () => {
      if (es) {
        try { es.close() } catch {}
      }
      setSseConnected(false)
    }
  }, [])

  useEffect(() => {
    const onHash = () => {
      const keyId = parseHashForKeyId()
      if (keyId) {
        setRoute({ name: 'key', id: keyId })
        return
      }
      const tokenId = parseHashForTokenId()
      setRoute(tokenId ? { name: 'token', id: tokenId } : { name: 'home' })
    }
    window.addEventListener('hashchange', onHash)
    return () => window.removeEventListener('hashchange', onHash)
  }, [])

  const navigateHome = () => {
    if (window.location.pathname !== '/admin') {
      window.history.pushState(null, '', '/admin')
    }
    location.hash = ''
    setRoute({ name: 'home' })
  }

  const navigateKey = (id: string) => {
    location.hash = `#/keys/${encodeURIComponent(id)}`
    setRoute({ name: 'key', id })
  }

  const navigateToken = (id: string) => {
    location.hash = `#/tokens/${encodeURIComponent(id)}`
    setRoute({ name: 'token', id })
  }

  const handleManualRefresh = () => {
    const controller = new AbortController()
    setLoading(true)
    void loadData(controller.signal).finally(() => controller.abort())
  }

  const metrics = useMemo(() => {
    if (!summary) {
      return []
    }

    const total = summary.total_requests
    return [
      {
        id: 'total',
        label: metricsStrings.labels.total,
        value: formatNumber(summary.total_requests),
        subtitle: '—',
      },
      {
        id: 'success',
        label: metricsStrings.labels.success,
        value: formatNumber(summary.success_count),
        subtitle: formatPercent(summary.success_count, total),
      },
      {
        id: 'errors',
        label: metricsStrings.labels.errors,
        value: formatNumber(summary.error_count),
        subtitle: formatPercent(summary.error_count, total),
      },
      {
        id: 'quota',
        label: metricsStrings.labels.quota,
        value: formatNumber(summary.quota_exhausted_count),
        subtitle: formatPercent(summary.quota_exhausted_count, total),
      },
      {
        id: 'remaining',
        label: metricsStrings.labels.remaining,
        value: `${formatNumber(summary.total_quota_remaining)} / ${formatNumber(summary.total_quota_limit)}`,
        subtitle:
          summary.total_quota_limit > 0
            ? formatPercent(summary.total_quota_remaining, summary.total_quota_limit)
            : '—',
      },
      {
        id: 'keys',
        label: metricsStrings.labels.keys,
        value: `${formatNumber(summary.active_keys)} / ${formatNumber(summary.active_keys + summary.exhausted_keys)}`,
        subtitle:
          summary.exhausted_keys === 0
            ? metricsStrings.subtitles.keysAll
            : metricsStrings.subtitles.keysExhausted.replace('{count}', formatNumber(summary.exhausted_keys)),
      },
    ]
  }, [summary, metricsStrings])

  const dedupedKeys = useMemo(() => {
    const map = new Map<string, ApiKeyStats>()
    for (const item of keys) {
      if (item.deleted_at) continue // hide soft-deleted keys
      map.set(item.id, item)
    }
    return Array.from(map.values())
  }, [keys])

  const sortedKeys = useMemo(() => {
    return [...dedupedKeys].sort((a, b) => {
      if (a.status !== b.status) {
        return a.status === 'active' ? -1 : 1
      }
      const left = a.last_used_at ?? 0
      const right = b.last_used_at ?? 0
      return right - left
    })
  }, [dedupedKeys])

  const displayName = profile?.displayName ?? null

  const toggleLogExpansion = useCallback((id: number) => {
    setExpandedLogs((previous) => {
      const next = new Set(previous)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }, [])

  const toggleJobExpansion = useCallback((id: number) => {
    setExpandedJobs((previous) => {
      const next = new Set(previous)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }, [])

  const handleAddKey = async () => {
    const value = newKey.trim()
    if (!value) return
    setSubmitting(true)
    try {
      await addApiKey(value)
      setNewKey('')
      const controller = new AbortController()
      setLoading(true)
      await loadData(controller.signal)
      controller.abort()
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : errorStrings.addKey)
    } finally {
      setSubmitting(false)
    }
  }

  const handleAddToken = async () => {
    const note = newTokenNote.trim()
    setSubmitting(true)
    try {
      const { token } = await createToken(note || undefined)
      setNewTokenNote('')
      try { await navigator.clipboard?.writeText(token) } catch {}
      const controller = new AbortController()
      setLoading(true)
      await loadData(controller.signal)
      controller.abort()
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : errorStrings.createToken)
    } finally {
      setSubmitting(false)
    }
  }

  const totalPages = useMemo(() => Math.max(1, Math.ceil(tokensTotal / tokensPerPage)), [tokensTotal])

  const goPrevPage = () => {
    setTokensPage((p) => Math.max(1, p - 1))
  }
  const goNextPage = () => {
    setTokensPage((p) => Math.min(totalPages, p + 1))
  }

  const handleSelectTokenGroupAll = () => {
    setSelectedTokenGroupName(null)
    setSelectedTokenUngrouped(false)
    setTokensPage(1)
  }

  const handleSelectTokenGroupUngrouped = () => {
    setSelectedTokenGroupName(null)
    setSelectedTokenUngrouped(true)
    setTokensPage(1)
  }

  const handleSelectTokenGroupNamed = (group: string) => {
    setSelectedTokenGroupName(group)
    setSelectedTokenUngrouped(false)
    setTokensPage(1)
  }

  const toggleTokenGroupsExpanded = () => {
    setTokenGroupsExpanded((previous) => !previous)
  }

  const openBatchDialog = () => {
    setBatchGroup('')
    setBatchCount(10)
    setBatchShareText(null)
    window.requestAnimationFrame(() => batchDialogRef.current?.showModal())
  }
  const submitBatchCreate = async () => {
    const group = batchGroup.trim()
    if (!group) return
    setBatchCreating(true)
    try {
      const res = await createTokensBatch(group, Math.max(1, Math.min(1000, batchCount)), newTokenNote.trim() || undefined)
      const links = res.tokens.map((t) => `${window.location.origin}/#${encodeURIComponent(t)}`).join('\n')
      setBatchShareText(links)
      // refresh list to first page
      setTokensPage(1)
      const controller = new AbortController()
      setLoading(true)
      await loadData(controller.signal)
      controller.abort()
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : errorStrings.createToken)
    } finally {
      setBatchCreating(false)
    }
  }
  const closeBatchDialog = () => {
    batchDialogRef.current?.close()
  }

  const handleCopyToken = async (id: string, stateKey: string) => {
    updateCopyState(stateKey, 'loading')
    try {
      const token = await resolveTokenSecret(id)
      await copyToClipboard(token)
      updateCopyState(stateKey, 'copied')
      window.setTimeout(() => updateCopyState(stateKey, null), 2000)
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : errorStrings.copyToken)
      updateCopyState(stateKey, null)
    }
  }

  const handleShareToken = async (id: string, stateKey: string) => {
    updateCopyState(stateKey, 'loading')
    try {
      const token = await resolveTokenSecret(id)
      const shareUrl = `${window.location.origin}/#${encodeURIComponent(token)}`
      await copyToClipboard(shareUrl)
      updateCopyState(stateKey, 'copied')
      window.setTimeout(() => updateCopyState(stateKey, null), 2000)
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : errorStrings.copyToken)
      updateCopyState(stateKey, null)
    }
  }

  const toggleToken = async (id: string, enabled: boolean) => {
    setTogglingId(id)
    try {
      await setTokenEnabled(id, !enabled)
      const controller = new AbortController()
      setLoading(true)
      await loadData(controller.signal)
      controller.abort()
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : errorStrings.toggleToken)
    } finally {
      setTogglingId(null)
    }
  }

  const openTokenDeleteConfirm = (id: string) => {
    if (!id) return
    setPendingTokenDeleteId(id)
    window.requestAnimationFrame(() => tokenDeleteDialogRef.current?.showModal())
  }

  const confirmTokenDelete = async () => {
    if (!pendingTokenDeleteId) return
    const id = pendingTokenDeleteId
    setDeletingId(id)
    try {
      await deleteToken(id)
      tokenDeleteDialogRef.current?.close()
      setPendingTokenDeleteId(null)
      const controller = new AbortController()
      setLoading(true)
      await loadData(controller.signal)
      controller.abort()
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : errorStrings.deleteToken)
    } finally {
      setDeletingId(null)
    }
  }

  const cancelTokenDelete = () => {
    tokenDeleteDialogRef.current?.close()
    setPendingTokenDeleteId(null)
  }

  const openTokenNoteEdit = (id: string, current: string | null) => {
    setEditingTokenId(id)
    setEditingTokenNote(current ?? '')
    window.requestAnimationFrame(() => tokenNoteDialogRef.current?.showModal())
  }

  const saveTokenNote = async () => {
    if (!editingTokenId) return
    setSavingTokenNote(true)
    try {
      await updateTokenNote(editingTokenId, editingTokenNote)
      tokenNoteDialogRef.current?.close()
      setEditingTokenId(null)
      setEditingTokenNote('')
      const controller = new AbortController()
      setLoading(true)
      await loadData(controller.signal)
      controller.abort()
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : errorStrings.updateTokenNote)
    } finally {
      setSavingTokenNote(false)
    }
  }

  const cancelTokenNote = () => {
    tokenNoteDialogRef.current?.close()
    setEditingTokenId(null)
    setEditingTokenNote('')
  }

  const openDeleteConfirm = (id: string) => {
    if (!id) return
    setPendingDeleteId(id)
    window.requestAnimationFrame(() => deleteDialogRef.current?.showModal())
  }

  const confirmDelete = async () => {
    if (!pendingDeleteId) return
    const id = pendingDeleteId
    setDeletingId(id)
    try {
      await deleteApiKey(id)
      deleteDialogRef.current?.close()
      setPendingDeleteId(null)
      const controller = new AbortController()
      setLoading(true)
      await loadData(controller.signal)
      controller.abort()
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : errorStrings.deleteKey)
    } finally {
      setDeletingId(null)
    }
  }

  const cancelDelete = () => {
    deleteDialogRef.current?.close()
    setPendingDeleteId(null)
  }

  const handleToggleDisable = async (id: string, toDisabled: boolean) => {
    if (!id) return
    setTogglingId(id)
    try {
      await setKeyStatus(id, toDisabled ? 'disabled' : 'active')
      const controller = new AbortController()
      setLoading(true)
      await loadData(controller.signal)
      controller.abort()
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : errorStrings.toggleKey)
    } finally {
      setTogglingId(null)
    }
  }

  // DaisyUI disable confirm flow
  const openDisableConfirm = (id: string) => {
    if (!id) return
    setPendingDisableId(id)
    window.requestAnimationFrame(() => disableDialogRef.current?.showModal())
  }

  const confirmDisable = async () => {
    if (!pendingDisableId) return
    const id = pendingDisableId
    await handleToggleDisable(id, true)
    disableDialogRef.current?.close()
    setPendingDisableId(null)
  }

  const cancelDisable = () => {
    disableDialogRef.current?.close()
    setPendingDisableId(null)
  }

  if (route.name === 'key') {
    return <KeyDetails id={route.id} onBack={navigateHome} />
  }
  if (route.name === 'token') {
    return <TokenDetail id={route.id} onBack={navigateHome} />
  }

  const tokenList = Array.isArray(tokens) ? tokens : []
  const tokenGroupList = Array.isArray(tokenGroups) ? tokenGroups : []
  const ungroupedGroup = tokenGroupList.find((group) => !group.name || group.name.trim().length === 0)
  const namedTokenGroups = tokenGroupList.filter((group) => group.name && group.name.trim().length > 0)
  const hasTokenGroups = tokenGroupList.length > 0
  return (
    <>
    <main className="app-shell">
      <section className="surface app-header">
        <div className="title-group">
          <h1>{headerStrings.title}</h1>
          <p>{headerStrings.subtitle}</p>
        </div>
        <div className="header-right">
          <div className="admin-language-switcher">
            <LanguageSwitcher />
          </div>
          {displayName && (
            <div className={`user-badge${isAdmin ? ' user-badge-admin' : ''}`}>
              {isAdmin && <Icon icon="mdi:crown-outline" className="user-badge-icon" aria-hidden="true" />}
              <span>{displayName}</span>
            </div>
          )}
          <div className="controls">
            {lastUpdated && (
              <span className="panel-description updated-time" style={{ marginRight: 8 }}>
                {headerStrings.updatedPrefix} {timeOnlyFormatter.format(lastUpdated)}
              </span>
            )}
            <button
              type="button"
              className="button button-primary"
              onClick={handleManualRefresh}
              disabled={loading}
            >
              {loading ? headerStrings.refreshing : headerStrings.refreshNow}
            </button>
          </div>
        </div>
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{tokenStrings.title}</h2>
            <p className="panel-description">{tokenStrings.description}</p>
          </div>
          {isAdmin && (
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <input
                type="text"
                placeholder={tokenStrings.notePlaceholder}
                value={newTokenNote}
                onChange={(e) => setNewTokenNote(e.target.value)}
                style={{ padding: '8px 12px', borderRadius: 10, border: '1px solid rgba(15, 23, 42, 0.16)', minWidth: 240 }}
                aria-label={tokenStrings.notePlaceholder}
              />
              <button type="button" className="button button-primary" onClick={() => void handleAddToken()} disabled={submitting}>
                {submitting ? tokenStrings.creating : tokenStrings.newToken}
              </button>
              <button type="button" className="button" onClick={openBatchDialog} disabled={submitting}>
                {tokenStrings.batchCreate}
              </button>
            </div>
          )}
        </div>
        {hasTokenGroups && (
          <div className="token-groups-container">
            <div className="token-groups-label">
              <span>{tokenStrings.groups.label}</span>
            </div>
            <div className="token-groups-row">
              <div
                ref={tokenGroupsListRef}
                className={`token-groups-list${tokenGroupsExpanded ? ' token-groups-list-expanded' : ''}`}
              >
                <button
                  type="button"
                  className={`token-group-chip${
                    !selectedTokenUngrouped && selectedTokenGroupName == null ? ' token-group-chip-active' : ''
                  }`}
                  onClick={handleSelectTokenGroupAll}
                >
                  <span className="token-group-name">{tokenStrings.groups.all}</span>
                </button>
                {ungroupedGroup && (
                  <button
                    type="button"
                    className={`token-group-chip${selectedTokenUngrouped ? ' token-group-chip-active' : ''}`}
                    onClick={handleSelectTokenGroupUngrouped}
                  >
                    <span className="token-group-name">{tokenStrings.groups.ungrouped}</span>
                    {tokenGroupsExpanded && (
                      <span className="token-group-count">
                        {ungroupedGroup.tokenCount}
                      </span>
                    )}
                  </button>
                )}
                {namedTokenGroups.map((group) => (
                  <button
                    key={group.name}
                    type="button"
                    className={`token-group-chip${
                      !selectedTokenUngrouped && selectedTokenGroupName === group.name ? ' token-group-chip-active' : ''
                    }`}
                    onClick={() => handleSelectTokenGroupNamed(group.name)}
                  >
                    <span className="token-group-name">{group.name}</span>
                    {tokenGroupsExpanded && (
                      <span className="token-group-count">
                        {group.tokenCount}
                      </span>
                    )}
                  </button>
                ))}
              </div>
              {(tokenGroupsCollapsedOverflowing || tokenGroupsExpanded) && (
                <button
                  type="button"
                  className={`token-group-chip token-group-toggle${tokenGroupsExpanded ? ' token-group-toggle-active' : ''}`}
                  onClick={toggleTokenGroupsExpanded}
                  aria-label={tokenGroupsExpanded ? tokenStrings.groups.moreHide : tokenStrings.groups.moreShow}
                >
                  <Icon icon={tokenGroupsExpanded ? 'mdi:chevron-up' : 'mdi:chevron-down'} width={18} height={18} />
                </button>
              )}
            </div>
          </div>
        )}
        <div className="table-wrapper jobs-table-wrapper">
          {tokenList.length === 0 ? (
            <div className="empty-state">{loading ? tokenStrings.empty.loading : tokenStrings.empty.none}</div>
          ) : (
            <table className="jobs-table">
              <thead>
                <tr>
                  <th>{tokenStrings.table.id}</th>
                  <th>{tokenStrings.table.note}</th>
                  <th>{tokenStrings.table.usage}</th>
                  <th>{tokenStrings.table.quota}</th>
                  <th>{tokenStrings.table.lastUsed}</th>
                  {isAdmin && <th>{tokenStrings.table.actions}</th>}
                </tr>
              </thead>
              <tbody>
                {tokenList.map((t) => {
                  const stateKey = copyStateKey('tokens', t.id)
                  const state = copyState.get(stateKey)
                  const shareStateKey = copyStateKey('tokens', `${t.id}:share`)
                  const shareState = copyState.get(shareStateKey)
                  const quotaStateKey = t.quota_state ?? 'normal'
                  const quotaLabel = quotaLabels[quotaStateKey] ?? quotaLabels.normal
                  const quotaTitle = `${t.quota_hourly_used}/${t.quota_hourly_limit} · ${t.quota_daily_used}/${t.quota_daily_limit} · ${t.quota_monthly_used}/${t.quota_monthly_limit}`
                  return (
                    <tr key={t.id}>
                      <td>
                        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                          <button
                            type="button"
                            title={tokenStrings.table.id}
                            className="link-button"
                            onClick={() => navigateToken(t.id)}
                          >
                            <code>{t.id}</code>
                          </button>
                          <span
                            className="token-status-slot"
                            aria-hidden={t.enabled ? true : undefined}
                            title={t.enabled ? undefined : tokenStrings.statusBadges.disabled}
                          >
                            {!t.enabled && (
                              <Icon
                                className="token-status-icon"
                                icon="mdi:pause-circle-outline"
                                width={14}
                                height={14}
                                aria-label={tokenStrings.statusBadges.disabled}
                              />
                            )}
                          </span>
                        </div>
                      </td>
                      <td>{t.note || '—'}</td>
                      <td>{formatNumber(t.total_requests)}</td>
                      <td>
                        <span
                          className={`token-quota-pill token-quota-pill-${quotaStateKey}`}
                          title={quotaTitle}
                        >
                          {quotaLabel}
                        </span>
                      </td>
                      <td>{formatTimestamp(t.last_used_at)}</td>
                      {isAdmin && (
                        <td className="jobs-message-cell">
                          <div style={{ display: 'flex', gap: 8 }}>
                            <button
                              type="button"
                              className={`icon-button${state === 'copied' ? ' icon-button-success' : ''}${state === 'loading' ? ' icon-button-loading' : ''}`}
                              title={tokenStrings.actions.copy}
                              aria-label={tokenStrings.actions.copy}
                              onClick={() => void handleCopyToken(t.id, stateKey)}
                              disabled={state === 'loading'}
                            >
                              <Icon icon={state === 'copied' ? 'mdi:check' : 'mdi:content-copy'} width={18} height={18} />
                            </button>
                          <button
                              type="button"
                              className={`icon-button${shareState === 'copied' ? ' icon-button-success' : ''}${shareState === 'loading' ? ' icon-button-loading' : ''}`}
                              title={tokenStrings.actions.share}
                              aria-label={tokenStrings.actions.share}
                              onClick={() => void handleShareToken(t.id, shareStateKey)}
                              disabled={shareState === 'loading'}
                            >
                              <Icon icon={shareState === 'copied' ? 'mdi:check' : 'mdi:share-variant'} width={18} height={18} />
                            </button>
                            <button
                              type="button"
                              className="icon-button"
                              title={keyStrings.actions.details}
                              aria-label={keyStrings.actions.details}
                              onClick={() => navigateToken(t.id)}
                            >
                              <Icon icon="mdi:eye-outline" width={18} height={18} />
                            </button>
                            <button
                              type="button"
                              className="icon-button"
                              title={t.enabled ? tokenStrings.actions.disable : tokenStrings.actions.enable}
                              aria-label={t.enabled ? tokenStrings.actions.disable : tokenStrings.actions.enable}
                              onClick={() => void toggleToken(t.id, t.enabled)}
                              disabled={togglingId === t.id}
                            >
                              <Icon icon={t.enabled ? 'mdi:pause-circle-outline' : 'mdi:play-circle-outline'} width={18} height={18} />
                            </button>
                            <button
                              type="button"
                              className="icon-button"
                              title={tokenStrings.actions.edit}
                              aria-label={tokenStrings.actions.edit}
                              onClick={() => openTokenNoteEdit(t.id, t.note)}
                            >
                              <Icon icon="mdi:pencil-outline" width={18} height={18} />
                            </button>
                            <button
                              type="button"
                              className="icon-button icon-button-danger"
                              title={tokenStrings.actions.delete}
                              aria-label={tokenStrings.actions.delete}
                              onClick={() => openTokenDeleteConfirm(t.id)}
                              disabled={deletingId === t.id}
                            >
                              <Icon icon={deletingId === t.id ? 'mdi:progress-helper' : 'mdi:trash-outline'} width={18} height={18} />
                            </button>
                          </div>
                        </td>
                      )}
                    </tr>
                  )
                })}
              </tbody>
            </table>
          )}
        </div>
        {tokensTotal > tokensPerPage && (
          <div className="table-pagination">
            <span className="panel-description">
              {tokenStrings.pagination.page
                .replace('{page}', String(tokensPage))
                .replace('{total}', String(totalPages))}
            </span>
            <div style={{ display: 'inline-flex', gap: 8 }}>
              <button className="button" onClick={goPrevPage} disabled={tokensPage <= 1}>
                {tokenStrings.pagination.prev}
              </button>
              <button className="button" onClick={goNextPage} disabled={tokensPage >= totalPages}>
                {tokenStrings.pagination.next}
              </button>
            </div>
          </div>
        )}
      </section>
      {error && <div className="surface error-banner">{error}</div>}

      <section className="surface quick-stats-grid">
        {metrics.length === 0 && loading ? (
          <div className="empty-state" style={{ gridColumn: '1 / -1' }}>
            {metricsStrings.loading}
          </div>
        ) : (
          metrics.map((metric) => (
            <div key={metric.id} className="metric-card quick-stats-card">
              <h3>{metric.label}</h3>
              <div className="metric-value">{metric.value}</div>
              <div className="metric-subtitle">{metric.subtitle}</div>
            </div>
          ))
        )}
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{keyStrings.title}</h2>
            <p className="panel-description">{keyStrings.description}</p>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
            {isAdmin && (
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <input
                  type="text"
                  placeholder={keyStrings.placeholder}
                  aria-label={keyStrings.placeholder}
                  value={newKey}
                  onChange={(e) => setNewKey(e.target.value)}
                  style={{
                    padding: '8px 12px',
                    borderRadius: 10,
                    border: '1px solid rgba(15, 23, 42, 0.16)',
                    minWidth: 240,
                  }}
                />
                <button
                  type="button"
                  className="button button-primary"
                  onClick={() => void handleAddKey()}
                  disabled={submitting || !newKey.trim()}
                >
                  {submitting ? keyStrings.adding : keyStrings.addButton}
                </button>
              </div>
            )}
          </div>
        </div>
        <div className="table-wrapper jobs-table-wrapper">
          {sortedKeys.length === 0 ? (
            <div className="empty-state">{loading ? keyStrings.empty.loading : keyStrings.empty.none}</div>
          ) : (
            <table>
              <thead>
                <tr>
                  <th>{keyStrings.table.keyId}</th>
                  <th>{keyStrings.table.status}</th>
                  <th>{keyStrings.table.total}</th>
                  <th>{keyStrings.table.success}</th>
                  <th>{keyStrings.table.errors}</th>
                  <th>{keyStrings.table.quotaLeft}</th>
                  <th>{keyStrings.table.lastUsed}</th>
                  <th>{keyStrings.table.statusChanged}</th>
                  {isAdmin && <th>{keyStrings.table.actions}</th>}
                </tr>
              </thead>
              <tbody>
                {sortedKeys.map((item) => {
                  const total = item.total_requests || 0
                  const stateKey = copyStateKey('keys', item.id)
                  const state = copyState.get(stateKey)
                  return (
                    <tr key={item.id}>
                      <td>
                        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                          <button
                            type="button"
                            className="link-button"
                            onClick={() => navigateKey(item.id)}
                            title={keyStrings.actions.details}
                            aria-label={keyStrings.actions.details}
                          >
                            <code>{item.id}</code>
                          </button>
                          {isAdmin && (
                            <button
                              type="button"
                              className={`icon-button${state === 'copied' ? ' icon-button-success' : ''}${state === 'loading' ? ' icon-button-loading' : ''}`}
                              title={keyStrings.actions.copy}
                              aria-label={keyStrings.actions.copy}
                              onClick={() => void handleCopySecret(item.id, stateKey)}
                              disabled={state === 'loading'}
                            >
                              <Icon icon={state === 'copied' ? 'mdi:check' : 'mdi:content-copy'} width={18} height={18} />
                            </button>
                          )}
                        </div>
                      </td>
                      <td>
                        <span className={statusClass(item.status)}>{statusLabel(item.status, adminStrings)}</span>
                      </td>
                      <td>{formatNumber(total)}</td>
                      <td>{formatNumber(item.success_count)}</td>
                      <td>{formatNumber(item.error_count)}</td>
                      <td>
                        {item.quota_remaining != null && item.quota_limit != null
                          ? `${formatNumber(item.quota_remaining)} / ${formatNumber(item.quota_limit)}`
                          : '—'}
                      </td>
                      <td>{formatTimestampNoYear(item.last_used_at)}</td>
                      <td>{formatTimestamp(item.status_changed_at)}</td>
                      {isAdmin && (
                        <td>
                          <div style={{ display: 'flex', gap: 8 }}>
                            {item.status === 'disabled' ? (
                              <button
                                type="button"
                                className="icon-button"
                                title={keyStrings.actions.enable}
                                aria-label={keyStrings.actions.enable}
                                onClick={() => void handleToggleDisable(item.id, false)}
                                disabled={togglingId === item.id}
                              >
                                <Icon icon={togglingId === item.id ? 'mdi:progress-helper' : 'mdi:play-circle-outline'} width={18} height={18} />
                              </button>
                            ) : (
                              <button
                                type="button"
                                className="icon-button"
                                title={keyStrings.actions.disable}
                                aria-label={keyStrings.actions.disable}
                                onClick={() => openDisableConfirm(item.id)}
                                disabled={togglingId === item.id}
                              >
                                <Icon icon={togglingId === item.id ? 'mdi:progress-helper' : 'mdi:pause-circle-outline'} width={18} height={18} />
                              </button>
                            )}
                            <button
                              type="button"
                              className="icon-button icon-button-danger"
                              title={keyStrings.actions.delete}
                              aria-label={keyStrings.actions.delete}
                              onClick={() => openDeleteConfirm(item.id)}
                              disabled={deletingId === item.id}
                            >
                              <Icon icon={deletingId === item.id ? 'mdi:progress-helper' : 'mdi:trash-outline'} width={18} height={18} />
                            </button>
                            <button
                              type="button"
                              className="icon-button"
                              title={keyStrings.actions.details}
                              aria-label={keyStrings.actions.details}
                              onClick={() => navigateKey(item.id)}
                            >
                              <Icon icon="mdi:eye-outline" width={18} height={18} />
                            </button>
                          </div>
                        </td>
                      )}
                    </tr>
                  )
                })}
              </tbody>
            </table>
          )}
        </div>
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{logStrings.title}</h2>
            <p className="panel-description">{logStrings.description}</p>
          </div>
        </div>
        <div className="table-wrapper jobs-table-wrapper">
          {logs.length === 0 ? (
            <div className="empty-state">{loading ? logStrings.empty.loading : logStrings.empty.none}</div>
          ) : (
            <table className="admin-logs-table">
              <thead>
                <tr>
                  <th>{logStrings.table.time}</th>
                  <th>{logStrings.table.key}</th>
                  <th>{logStrings.table.token}</th>
                  <th>{logStrings.table.httpStatus}</th>
                  <th>{logStrings.table.mcpStatus}</th>
                  <th>{logStrings.table.result}</th>
                  <th>{logStrings.table.error}</th>
                </tr>
              </thead>
              <tbody>
                {logs.map((log) => (
                  <LogRow
                    key={log.id}
                    log={log}
                    expanded={expandedLogs.has(log.id)}
                    onToggle={toggleLogExpansion}
                    strings={adminStrings}
                  />
                ))}
              </tbody>
            </table>
          )}
        </div>
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{jobsStrings.title}</h2>
            <p className="panel-description">{jobsStrings.description}</p>
          </div>
          <div className="panel-actions">
            <div className="segmented-control">
              <button
                type="button"
                className={jobFilter === 'all' ? 'active' : ''}
                onClick={() => setJobFilter('all')}
              >
                {jobsStrings.filters.all}
              </button>
              <button
                type="button"
                className={jobFilter === 'quota' ? 'active' : ''}
                onClick={() => setJobFilter('quota')}
              >
                {jobsStrings.filters.quota}
              </button>
              <button
                type="button"
                className={jobFilter === 'usage' ? 'active' : ''}
                onClick={() => setJobFilter('usage')}
              >
                {jobsStrings.filters.usage}
              </button>
              <button
                type="button"
                className={jobFilter === 'logs' ? 'active' : ''}
                onClick={() => setJobFilter('logs')}
              >
                {jobsStrings.filters.logs}
              </button>
            </div>
          </div>
        </div>
        <div className="table-wrapper jobs-table-wrapper">
          {jobs.length === 0 ? (
            <div className="empty-state">
              {loading ? jobsStrings.empty.loading : jobsStrings.empty.none}
            </div>
          ) : (
            <table className="jobs-table">
              <thead>
                <tr>
                  <th>{jobsStrings.table.id}</th>
                  <th>{jobsStrings.table.type}</th>
                  <th>{jobsStrings.table.key}</th>
                  <th>{jobsStrings.table.status}</th>
                  <th>{jobsStrings.table.attempt}</th>
                  <th>{jobsStrings.table.started}</th>
                  <th>{jobsStrings.table.message}</th>
                </tr>
              </thead>
              <tbody>
                {jobs.map((j) => {
                  const job: any = j as any
                  const jt = job.job_type ?? job.jobType ?? ''
                  const jobTypeLabel = jobsStrings.types?.[jt] ?? jt
                  const keyId = job.key_id ?? job.keyId ?? '—'
                  const started: number | null = job.started_at ?? job.startedAt ?? null
                  const finished: number | null = job.finished_at ?? job.finishedAt ?? null
                  const startedTimeLabel = formatTimestamp(started)
                  const startedDetail =
                    started != null
                      ? `${formatTimestampWithMs(started)} · ${formatRelativeTime(started)}`
                      : jobsStrings.empty.none
                  const isExpanded = expandedJobs.has(j.id)
                  const jobMessage: string | null = j.message ?? null
                  const messageLabel = isExpanded
                    ? jobsStrings.toggles?.hide ?? jobsStrings.table.message
                    : jobsStrings.toggles?.show ?? jobsStrings.table.message
                  const duration =
                    started != null && finished != null
                      ? (() => {
                          const seconds = Math.max(0, finished - started)
                          if (seconds < 60) return `${seconds}s`
                          const minutes = Math.round(seconds / 60)
                          return `${minutes}m`
                        })()
                      : null
                  const startedSummary =
                    started != null ? `${formatTimestampWithMs(started)} · ${formatRelativeTime(started)}` : null
                  const finishedSummary =
                    finished != null ? `${formatTimestampWithMs(finished)} · ${formatRelativeTime(finished)}` : null
                  const rows: JSX.Element[] = []

                  rows.push(
                    <tr key={j.id}>
                        <td>{j.id}</td>
                        <td>{jobTypeLabel}</td>
                        <td>{keyId ?? '—'}</td>
                        <td>
                          <span className={statusClass(j.status)}>{j.status}</span>
                        </td>
                        <td>{j.attempt}</td>
                        <td>{started ? startedTimeLabel : '—'}</td>
                        <td>
                          {jobMessage ? (
                            <button
                              type="button"
                              className={`jobs-message-button${isExpanded ? ' jobs-message-button-active' : ''}`}
                              onClick={() => toggleJobExpansion(j.id)}
                              aria-expanded={isExpanded}
                              aria-controls={`job-details-${j.id}`}
                              aria-label={messageLabel}
                              title={jobMessage}
                            >
                              <span className="jobs-message-text">{jobMessage}</span>
                              <Icon
                                icon={isExpanded ? 'mdi:chevron-up' : 'mdi:chevron-down'}
                                width={16}
                                height={16}
                                className="jobs-message-icon"
                                aria-hidden="true"
                              />
                            </button>
                          ) : (
                            '—'
                          )}
                        </td>
                      </tr>,
                  )

                  if (isExpanded) {
                    rows.push(
                      <tr key={`${j.id}-details`} className="log-details-row">
                        <td colSpan={7} id={`job-details-${j.id}`}>
                          <div className="log-details-panel">
                            <div className="log-details-summary">
                              <div>
                                <div className="log-details-label">{jobsStrings.table.id}</div>
                                <div className="log-details-value">{j.id}</div>
                              </div>
                              <div>
                                <div className="log-details-label">{jobsStrings.table.type}</div>
                                <div className="log-details-value">
                                  {jt ? (
                                    <span className="job-type-pill">
                                      <button
                                        type="button"
                                        className="job-type-trigger"
                                        aria-label={jt}
                                      >
                                        <span className="job-type-main">{jobTypeLabel}</span>
                                      </button>
                                      <div className="job-type-bubble">{jt}</div>
                                    </span>
                                  ) : (
                                    '—'
                                  )}
                                </div>
                              </div>
                              <div>
                                <div className="log-details-label">{jobsStrings.table.key}</div>
                                <div className="log-details-value">{keyId ?? '—'}</div>
                              </div>
                              <div>
                                <div className="log-details-label">{jobsStrings.table.status}</div>
                                <div className="log-details-value">{j.status}</div>
                              </div>
                              <div>
                                <div className="log-details-label">{jobsStrings.table.attempt}</div>
                                <div className="log-details-value">{j.attempt}</div>
                              </div>
                              <div>
                                <div className="log-details-label">{jobsStrings.table.started}</div>
                                <div className="log-details-value">
                                  {startedSummary ?? jobsStrings.empty.none}
                                </div>
                              </div>
                              {finishedSummary && (
                                <div>
                                  <div className="log-details-label">Finished</div>
                                  <div className="log-details-value">
                                    {finishedSummary}
                                  </div>
                                </div>
                              )}
                              {duration && (
                                <div>
                                  <div className="log-details-label">DURATION</div>
                                  <div className="log-details-value">{duration}</div>
                                </div>
                              )}
                            </div>
                            {jobMessage && (
                              <div className="log-details-body">
                                <section className="log-details-section">
                                  <header>{jobsStrings.table.message}</header>
                                  <pre>{jobMessage}</pre>
                                </section>
                              </div>
                            )}
                          </div>
                        </td>
                      </tr>,
                    )
                  }

                  return rows
                })}
              </tbody>
            </table>
          )}
        </div>
        {jobsTotal > jobsPerPage && (
          <div className="table-pagination">
            <span className="panel-description">
              {jobsStrings.description} ({jobsPage} / {Math.max(1, Math.ceil(jobsTotal / jobsPerPage))})
            </span>
            <div style={{ display: 'inline-flex', gap: 8 }}>
              <button
                className="button"
                onClick={() => setJobsPage((p) => Math.max(1, p - 1))}
                disabled={jobsPage <= 1}
              >
                {tokenStrings.pagination.prev}
              </button>
              <button
                className="button"
                onClick={() => setJobsPage((p) => p + 1)}
                disabled={jobsPage >= Math.ceil(jobsTotal / jobsPerPage)}
              >
                {tokenStrings.pagination.next}
              </button>
            </div>
          </div>
        )}
      </section>

      <div className="app-footer">
        <span>{footerStrings.title}</span>
        <span className="footer-meta">
          {/* GitHub repository link with Iconify icon */}
          <a
            href="https://github.com/IvanLi-CN/tavily-hikari"
            className="footer-link"
            target="_blank"
            rel="noreferrer"
            aria-label={footerStrings.githubAria}
          >
            <Icon icon="mdi:github" width={18} height={18} className="footer-link-icon" />
            <span>{footerStrings.githubLabel}</span>
          </a>
        </span>
        <span className="footer-meta">
          {version ? (
            (() => {
              const raw = version.backend || ''
              const clean = raw.replace(/-.+$/, '')
              const tag = clean.startsWith('v') ? clean : `v${clean}`
              const href = `https://github.com/IvanLi-CN/tavily-hikari/releases/tag/${tag}`
              return (
                <>
                  {footerStrings.tagPrefix}
                  <a href={href} className="footer-link" target="_blank" rel="noreferrer">
                    {`v${raw}`}
                  </a>
                </>
              )
            })()
          ) : (
            footerStrings.loadingVersion
          )}
        </span>
      </div>
    </main>
    {/* Batch Create Tokens (DaisyUI modal) */}
    <dialog id="batch_create_tokens_modal" ref={batchDialogRef} className="modal">
      <div className="modal-box">
        <h3 className="font-bold text-lg" style={{ marginTop: 0 }}>{tokenStrings.batchDialog.title}</h3>
        {batchShareText == null ? (
          <>
            <div className="py-2" style={{ display: 'flex', gap: 8 }}>
              <input
                type="text"
                className="input"
                placeholder={tokenStrings.batchDialog.groupPlaceholder}
                value={batchGroup}
                onChange={(e) => setBatchGroup(e.target.value)}
                style={{ flex: 1 }}
              />
              <input
                type="number"
                className="input"
                min={1}
                max={1000}
                value={batchCount}
                onChange={(e) => setBatchCount(Number(e.target.value) || 1)}
                style={{ width: 120 }}
              />
            </div>
            <div className="modal-action">
              <form method="dialog" onSubmit={(e) => e.preventDefault()} style={{ display: 'flex', gap: 8 }}>
                <button type="button" className="btn" onClick={closeBatchDialog}>{tokenStrings.batchDialog.cancel}</button>
                <button type="button" className="btn btn-primary" onClick={() => void submitBatchCreate()} disabled={batchCreating}>
                  {batchCreating ? tokenStrings.batchDialog.creating : tokenStrings.batchDialog.confirm}
                </button>
              </form>
            </div>
          </>
        ) : (
          <>
            <div className="batch-dialog-body">
              <p className="py-2">
                {tokenStrings.batchDialog.createdN.replace(
                  '{n}',
                  String((batchShareText ?? '').split('\n').filter((line) => line.length > 0).length),
                )}
              </p>
              <textarea
                className="textarea"
                readOnly
                wrap="off"
                rows={6}
                style={{
                  width: '100%',
                  fontFamily:
                    'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
                  whiteSpace: 'pre',
                  overflowX: 'auto',
                  overflowY: 'auto',
                  resize: 'none',
                }}
                value={batchShareText ?? ''}
              />
            </div>
            <div className="modal-action">
              <form method="dialog" onSubmit={(e) => e.preventDefault()} style={{ display: 'flex', gap: 8 }}>
                <button
                  type="button"
                  className="btn"
                  onClick={() => {
                    if (!batchShareText) return
                    void copyToClipboard(batchShareText)
                  }}
                >
                  {tokenStrings.batchDialog.copyAll}
                </button>
                <button type="button" className="btn" onClick={closeBatchDialog}>
                  {tokenStrings.batchDialog.done}
                </button>
              </form>
            </div>
          </>
        )}
      </div>
    </dialog>
    {/* Disable Confirmation (daisyUI modal) */}
    <dialog id="confirm_disable_modal" ref={disableDialogRef} className="modal">
      <div className="modal-box">
        <h3 className="font-bold text-lg" style={{ marginTop: 0 }}>{keyStrings.dialogs.disable.title}</h3>
        <p className="py-2">{keyStrings.dialogs.disable.description}</p>
        <div className="modal-action">
          <form method="dialog" onSubmit={(e) => e.preventDefault()} style={{ display: 'flex', gap: 8 }}>
            <button type="button" className="btn" onClick={cancelDisable}>{keyStrings.dialogs.disable.cancel}</button>
            <button type="button" className="btn" onClick={() => void confirmDisable()} disabled={!!togglingId}>
              {keyStrings.dialogs.disable.confirm}
            </button>
          </form>
        </div>
      </div>
    </dialog>

    {/* Delete Confirmation (daisyUI modal) */}
    <dialog id="confirm_delete_modal" ref={deleteDialogRef} className="modal">
      <div className="modal-box">
        <h3 className="font-bold text-lg" style={{ marginTop: 0 }}>{keyStrings.dialogs.delete.title}</h3>
        <p className="py-2">{keyStrings.dialogs.delete.description}</p>
        <div className="modal-action">
          <form method="dialog" onSubmit={(e) => e.preventDefault()} style={{ display: 'flex', gap: 8 }}>
            <button type="button" className="btn" onClick={cancelDelete}>{keyStrings.dialogs.delete.cancel}</button>
            <button type="button" className="btn btn-error" onClick={() => void confirmDelete()} disabled={!!deletingId}>
              {keyStrings.dialogs.delete.confirm}
            </button>
          </form>
        </div>
      </div>
    </dialog>
    {/* Token Delete Confirmation */}
    <dialog id="confirm_token_delete_modal" ref={tokenDeleteDialogRef} className="modal">
      <div className="modal-box">
        <h3 className="font-bold text-lg" style={{ marginTop: 0 }}>{tokenStrings.dialogs.delete.title}</h3>
        <p className="py-2">{tokenStrings.dialogs.delete.description}</p>
        <div className="modal-action">
          <form method="dialog" onSubmit={(e) => e.preventDefault()} style={{ display: 'flex', gap: 8 }}>
            <button type="button" className="btn" onClick={cancelTokenDelete}>{tokenStrings.dialogs.delete.cancel}</button>
            <button type="button" className="btn btn-error" onClick={() => void confirmTokenDelete()} disabled={!!deletingId}>
              {tokenStrings.dialogs.delete.confirm}
            </button>
          </form>
        </div>
      </div>
    </dialog>

    {/* Token Edit Note (DaisyUI modal) */}
    <dialog id="edit_token_note_modal" ref={tokenNoteDialogRef} className="modal">
      <div className="modal-box">
        <h3 className="font-bold text-lg" style={{ marginTop: 0 }}>{tokenStrings.dialogs.note.title}</h3>
        <div className="py-2" style={{ display: 'flex', gap: 8 }}>
          <input
            type="text"
            className="input"
            placeholder={tokenStrings.dialogs.note.placeholder}
            value={editingTokenNote}
            onChange={(e) => setEditingTokenNote(e.target.value)}
            style={{ flex: 1 }}
          />
        </div>
        <div className="modal-action">
          <form method="dialog" onSubmit={(e) => e.preventDefault()} style={{ display: 'flex', gap: 8 }}>
            <button type="button" className="btn" onClick={cancelTokenNote}>{tokenStrings.dialogs.note.cancel}</button>
            <button type="button" className="btn btn-primary" onClick={() => void saveTokenNote()} disabled={savingTokenNote}>
              {savingTokenNote ? tokenStrings.dialogs.note.saving : tokenStrings.dialogs.note.confirm}
            </button>
          </form>
        </div>
      </div>
    </dialog>
    </>
  )
}

interface LogRowProps {
  log: RequestLog
  expanded: boolean
  onToggle: (id: number) => void
  strings: AdminTranslations
}

function LogRow({ log, expanded, onToggle, strings }: LogRowProps): JSX.Element {
  const requestButtonLabel = expanded ? strings.logs.toggles.hide : strings.logs.toggles.show
  const tokenId = log.auth_token_id ?? null
  const timeLabel = formatClockTime(log.created_at)
  const timeDetail =
    log.created_at != null
      ? `${formatTimestampWithMs(log.created_at)} · ${formatRelativeTime(log.created_at)}`
      : strings.logs.errors.none

  return (
    <>
      <tr>
        <td>
          <div className="log-time-cell">
            <button
              type="button"
              className="log-time-trigger"
              aria-label={timeDetail}
            >
              <span className="log-time-main">{timeLabel}</span>
            </button>
            <div className="log-time-bubble">{timeDetail}</div>
          </div>
        </td>
        <td>
          <a
            href={`#/keys/${encodeURIComponent(log.key_id)}`}
            className="log-key-pill"
            title={strings.keys.actions.details}
            aria-label={strings.keys.actions.details}
          >
            <code>{log.key_id}</code>
          </a>
        </td>
        <td>
          {tokenId ? (
            <a
              href={`#/tokens/${encodeURIComponent(tokenId)}`}
              className="link-button log-token-link"
              title={strings.tokens.table.id}
              aria-label={strings.tokens.table.id}
            >
              <code>{tokenId}</code>
            </a>
          ) : (
            '—'
          )}
        </td>
        <td>{log.http_status ?? '—'}</td>
        <td>{log.mcp_status ?? '—'}</td>
        <td>
          <button
            type="button"
            className={`log-result-button${expanded ? ' log-result-button-active' : ''}`}
            onClick={() => onToggle(log.id)}
            aria-expanded={expanded}
            aria-controls={`log-details-${log.id}`}
            aria-label={requestButtonLabel}
            title={requestButtonLabel}
          >
            <span className={statusClass(log.result_status)}>{statusLabel(log.result_status, strings)}</span>
            <Icon icon={expanded ? 'mdi:chevron-up' : 'mdi:chevron-down'} width={18} height={18} className="log-result-icon" />
          </button>
        </td>
        <td>{formatErrorMessage(log, strings.logs.errors)}</td>
      </tr>
      {expanded && (
        <tr className="log-details-row">
          <td colSpan={7} id={`log-details-${log.id}`}>
            <LogDetails log={log} strings={strings} />
          </td>
        </tr>
      )}
    </>
  )
}

function LogDetails({ log, strings }: { log: RequestLog; strings: AdminTranslations }): JSX.Element {
  const query = log.query ? `?${log.query}` : ''
  const requestLine = `${log.method} ${log.path}${query}`
  const forwarded = log.forwarded_headers.filter((value) => value.trim().length > 0)
  const dropped = log.dropped_headers.filter((value) => value.trim().length > 0)
  const httpLabel = `${strings.logs.table.httpStatus}: ${log.http_status ?? strings.logs.errors.none}`
  const mcpLabel = `${strings.logs.table.mcpStatus}: ${log.mcp_status ?? strings.logs.errors.none}`
  const requestBody = log.request_body ?? strings.logDetails.noBody
  const responseBody = log.response_body ?? strings.logDetails.noBody

  return (
    <div className="log-details-panel">
      <div className="log-details-summary">
        <div>
          <span className="log-details-label">{strings.logDetails.request}</span>
          <span className="log-details-value">{requestLine}</span>
        </div>
        <div>
          <span className="log-details-label">{strings.logDetails.response}</span>
          <span className="log-details-value">
            {httpLabel}
            {` · ${mcpLabel}`}
          </span>
        </div>
        <div>
          <span className="log-details-label">{strings.logDetails.outcome}</span>
          <span className="log-details-value">{statusLabel(log.result_status, strings)}</span>
        </div>
      </div>
      <div className="log-details-body">
        <div className="log-details-section">
          <header>{strings.logDetails.requestBody}</header>
          <pre>{requestBody}</pre>
        </div>
        <div className="log-details-section">
          <header>{strings.logDetails.responseBody}</header>
          <pre>{responseBody}</pre>
        </div>
      </div>
      {(forwarded.length > 0 || dropped.length > 0) && (
        <div className="log-details-headers">
          {forwarded.length > 0 && (
            <div className="log-details-section">
              <header>{strings.logDetails.forwardedHeaders}</header>
              <ul>
                {forwarded.map((header, index) => (
                  <li key={`forwarded-${index}-${header}`}>{header}</li>
                ))}
              </ul>
            </div>
          )}
          {dropped.length > 0 && (
            <div className="log-details-section">
              <header>{strings.logDetails.droppedHeaders}</header>
              <ul>
                {dropped.map((header, index) => (
                  <li key={`dropped-${index}-${header}`}>{header}</li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
    </div>
  )
}

function KeyDetails({ id, onBack }: { id: string; onBack: () => void }): JSX.Element {
  const translations = useTranslate()
  const adminStrings = translations.admin
  const keyDetailsStrings = adminStrings.keyDetails
  const logsTableStrings = adminStrings.logs.table
  const [detail, setDetail] = useState<ApiKeyStats | null>(null)
  const [period, setPeriod] = useState<'day' | 'week' | 'month'>('month')
  const [startDate, setStartDate] = useState<string>(() => new Date().toISOString().slice(0, 10))
  const [summary, setSummary] = useState<KeySummary | null>(null)
  const [logs, setLogs] = useState<RequestLog[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [syncState, setSyncState] = useState<'idle' | 'syncing' | 'success'>('idle')
  const syncInFlightRef = useRef(false)
  const syncFeedbackTimerRef = useRef<number | null>(null)

  const computeSince = useCallback((): number => {
    const base = new Date(startDate + 'T00:00:00Z')
    if (Number.isNaN(base.getTime())) return Math.floor(Date.now() / 1000)
    const d = new Date(base)
    if (period === 'day') return Math.floor(d.getTime() / 1000)
    if (period === 'week') {
      const day = d.getUTCDay() // 0..6 (Sun..Sat)
      const diff = (day + 6) % 7 // days since Monday
      d.setUTCDate(d.getUTCDate() - diff)
      return Math.floor(d.getTime() / 1000)
    }
    // month
    d.setUTCDate(1)
    return Math.floor(d.getTime() / 1000)
  }, [period, startDate])

  const load = useCallback(async () => {
    try {
      setLoading(true)
      setError(null)
      const since = computeSince()
      const [s, ls, d] = await Promise.all([
        fetchKeyMetrics(id, period, since),
        fetchKeyLogs(id, 50, since),
        fetchApiKeyDetail(id).catch(() => null),
      ])
      setSummary(s)
      setLogs(ls)
      setDetail(d)
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : adminStrings.errors.loadKeyDetails)
    } finally {
      setLoading(false)
    }
  }, [id, period, computeSince])

  useEffect(() => {
    void load()
  }, [load])

  useEffect(() => () => {
    if (syncFeedbackTimerRef.current != null) {
      window.clearTimeout(syncFeedbackTimerRef.current)
    }
  }, [])

  const syncUsage = useCallback(async () => {
    if (syncInFlightRef.current) return
    syncInFlightRef.current = true
    try {
      setSyncState('syncing')
      setError(null)
      await syncApiKeyUsage(id)
      await load()
      setSyncState('success')
      if (syncFeedbackTimerRef.current != null) {
        window.clearTimeout(syncFeedbackTimerRef.current)
      }
      syncFeedbackTimerRef.current = window.setTimeout(() => {
        setSyncState('idle')
        syncFeedbackTimerRef.current = null
      }, 2500)
    } catch (err) {
      console.error(err)
      setError(err instanceof Error ? err.message : adminStrings.errors.syncUsage)
      setSyncState('idle')
    } finally {
      syncInFlightRef.current = false
    }
  }, [adminStrings.errors.syncUsage, id, load])

  const metricCards = useMemo(() => {
    if (!summary) return []
    const total = summary.total_requests
    const lastActivitySubtitle = summary.last_activity
      ? `${keyDetailsStrings.metrics.lastActivityPrefix} ${formatTimestamp(summary.last_activity)}`
      : keyDetailsStrings.metrics.noActivity
    return [
      { id: 'total', label: keyDetailsStrings.metrics.total, value: formatNumber(summary.total_requests), subtitle: lastActivitySubtitle },
      { id: 'success', label: keyDetailsStrings.metrics.success, value: formatNumber(summary.success_count), subtitle: formatPercent(summary.success_count, total) },
      { id: 'errors', label: keyDetailsStrings.metrics.errors, value: formatNumber(summary.error_count), subtitle: formatPercent(summary.error_count, total) },
      { id: 'quota', label: keyDetailsStrings.metrics.quota, value: formatNumber(summary.quota_exhausted_count), subtitle: formatPercent(summary.quota_exhausted_count, total) },
    ]
  }, [summary, keyDetailsStrings])

  return (
    <main className="app-shell">
      <section className="surface app-header">
        <div className="title-group">
          <h1>{keyDetailsStrings.title}</h1>
          <p>
            {keyDetailsStrings.descriptionPrefix}{' '}
            <code>{id}</code>
          </p>
        </div>
        <div className="controls">
          <button
            type="button"
            className={`button${syncState === 'success' ? ' button-success' : ''}`}
            onClick={() => void syncUsage()}
            disabled={syncState === 'syncing'}
            aria-busy={syncState === 'syncing'}
          >
            <Icon
              icon={syncState === 'syncing' ? 'mdi:loading' : syncState === 'success' ? 'mdi:check-bold' : 'mdi:refresh'}
              width={18}
              height={18}
              className={syncState === 'syncing' ? 'icon-spin' : undefined}
            />
            &nbsp;
            {syncState === 'syncing'
              ? keyDetailsStrings.syncing
              : syncState === 'success'
                ? keyDetailsStrings.syncSuccess
                : keyDetailsStrings.syncAction}
          </button>
          <button type="button" className="button" onClick={onBack}>
            <Icon icon="mdi:arrow-left" width={18} height={18} />
            &nbsp;{keyDetailsStrings.back}
          </button>
        </div>
      </section>

      {error && <div className="surface error-banner" style={{ marginTop: 8, marginBottom: 0 }}>{error}</div>}

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>Quota</h2>
            <p className="panel-description">Tavily Usage for this key</p>
          </div>
        </div>
        <section className="metrics-grid">
          {(!detail || loading) ? (
            <div className="empty-state" style={{ gridColumn: '1 / -1' }}>{keyDetailsStrings.loading}</div>
          ) : (
            (() => {
              const limit = detail?.quota_limit ?? null
              const remaining = detail?.quota_remaining ?? null
              const used = (limit != null && remaining != null) ? Math.max(limit - remaining, 0) : null
              const percent = (limit && remaining != null && limit > 0) ? formatPercent(remaining, limit) : '—'
              return [
                { id: 'used', label: 'Used', value: used != null ? formatNumber(used) : '—', subtitle: limit != null ? `of ${formatNumber(limit)}` : '—' },
                { id: 'remaining', label: 'Remaining', value: remaining != null ? formatNumber(remaining) : '—', subtitle: percent },
                { id: 'synced', label: 'Synced', value: detail?.quota_synced_at ? formatTimestamp(detail.quota_synced_at) : '—', subtitle: '' },
              ].map((m) => (
                <div key={m.id} className="metric-card">
                  <h3>{m.label}</h3>
                  <div className="metric-value">{m.value}</div>
                  <div className="metric-subtitle">{m.subtitle}</div>
                </div>
              ))
            })()
          )}
        </section>
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{keyDetailsStrings.usageTitle}</h2>
            <p className="panel-description">{keyDetailsStrings.usageDescription}</p>
          </div>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <select value={period} onChange={(e) => setPeriod(e.target.value as any)} className="input" aria-label={keyDetailsStrings.usageTitle}>
              <option value="day">{keyDetailsStrings.periodOptions.day}</option>
              <option value="week">{keyDetailsStrings.periodOptions.week}</option>
              <option value="month">{keyDetailsStrings.periodOptions.month}</option>
            </select>
            <input type="date" value={startDate} onChange={(e) => setStartDate(e.target.value)} className="input" />
            <button type="button" className="button button-primary" onClick={() => void load()} disabled={loading}>
              {keyDetailsStrings.apply}
            </button>
          </div>
        </div>
        <section className="metrics-grid">
          {(!summary || loading) ? (
            <div className="empty-state" style={{ gridColumn: '1 / -1' }}>{keyDetailsStrings.loading}</div>
          ) : (
            metricCards.map((m) => (
              <div key={m.id} className="metric-card">
                <h3>{m.label}</h3>
                <div className="metric-value">{m.value}</div>
                <div className="metric-subtitle">{m.subtitle}</div>
              </div>
            ))
          )}
        </section>
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>{keyDetailsStrings.logsTitle}</h2>
            <p className="panel-description">{keyDetailsStrings.logsDescription}</p>
          </div>
        </div>
        <div className="table-wrapper">
          {logs.length === 0 ? (
            <div className="empty-state">{loading ? keyDetailsStrings.loading : keyDetailsStrings.logsEmpty}</div>
          ) : (
            <table className="admin-logs-table">
              <thead>
                <tr>
                  <th>{logsTableStrings.time}</th>
                  <th>{logsTableStrings.httpStatus}</th>
                  <th>{logsTableStrings.mcpStatus}</th>
                  <th>{logsTableStrings.result}</th>
                  <th>{logsTableStrings.error}</th>
                </tr>
              </thead>
              <tbody>
                {logs.map((log) => (
                  <tr key={log.id}>
                    <td>{formatTimestamp(log.created_at)}</td>
                    <td>{log.http_status ?? '—'}</td>
                    <td>{log.mcp_status ?? '—'}</td>
                    <td><span className={statusClass(log.result_status)}>{statusLabel(log.result_status, adminStrings)}</span></td>
                    <td>{formatErrorMessage(log, adminStrings.logs.errors)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </section>
    </main>
  )
}

export default AdminDashboard
