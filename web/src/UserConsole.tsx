import React, { ReactNode, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { Icon } from '@iconify/react'
import CherryStudioMock from './components/CherryStudioMock'

import {
  fetchProfile,
  probeApiTavilyCrawl,
  probeApiTavilyExtract,
  probeApiTavilyMap,
  probeApiTavilyResearch,
  probeApiTavilyResearchResult,
  probeApiTavilySearch,
  probeMcpPing,
  probeMcpToolsList,
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
import { Button } from './components/ui/button'
import { useLanguage, useTranslate, type Language } from './i18n'
import {
  type McpProbeStepState,
  type ProbeQuotaWindow,
  McpProbeRequestError,
  getProbeEnvelopeError,
  getQuotaExceededWindow,
  getTokenBusinessQuotaWindow,
  revalidateBlockedQuotaWindow,
  resolveMcpProbeButtonState,
} from './lib/mcpProbe'
import { useResponsiveModes } from './lib/responsive'
import { getUserConsoleAdminHref } from './lib/userConsoleAdminEntry'
import { resolveUserConsoleAvailability } from './lib/userConsoleAvailability'

const REPO_URL = 'https://github.com/IvanLi-CN/tavily-hikari'
const CODEX_DOC_URL = 'https://github.com/openai/codex/blob/main/docs/config.md'
const CLAUDE_DOC_URL = 'https://code.claude.com/docs/en/mcp'
const VSCODE_DOC_URL = 'https://code.visualstudio.com/docs/copilot/customization/mcp-servers'
const NOCODB_DOC_URL = 'https://nocodb.com/docs/product-docs/mcp'
const MCP_SPEC_URL = 'https://modelcontextprotocol.io/introduction'
const ICONIFY_ENDPOINT = 'https://api.iconify.design'

type GuideLanguage = 'toml' | 'json' | 'bash'
type GuideKey = 'codex' | 'claude' | 'vscode' | 'claudeDesktop' | 'cursor' | 'windsurf' | 'cherryStudio' | 'other'

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

const GUIDE_KEY_ORDER: GuideKey[] = [
  'codex',
  'claude',
  'vscode',
  'claudeDesktop',
  'cursor',
  'windsurf',
  'cherryStudio',
  'other',
]

type ConsoleRoute =
  | { name: 'dashboard' }
  | { name: 'tokens' }
  | { name: 'token'; id: string }

type ProbeButtonState = 'idle' | 'running' | 'success' | 'partial' | 'failed'
type ProbeStepStatus = 'running' | 'success' | 'failed' | 'blocked'
type ProbeBubbleAnchor = 'mcp' | 'api'

interface ProbeButtonModel {
  state: ProbeButtonState
  completed: number
  total: number
}

interface ProbeBubbleItem {
  id: string
  label: string
  status: ProbeStepStatus
  detail?: string | null
}

interface ProbeBubbleModel {
  visible: boolean
  anchor: ProbeBubbleAnchor
  items: ProbeBubbleItem[]
}

interface McpProbeStepDefinition {
  id: string
  label: string
  run: (token: string) => Promise<string | null>
}

interface ApiProbeStepDefinition {
  id: string
  label: string
  run: (
    token: string,
    context: { requestId: string | null },
  ) => Promise<string | null>
}

const numberFormatter = new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 })

function formatNumber(value: number): string {
  return numberFormatter.format(value)
}

function formatQuotaPair(used: number, limit: number): string {
  return `${formatNumber(used)} / ${formatNumber(limit)}`
}

function parseRouteFromHash(): ConsoleRoute {
  const hash = window.location.hash || ''
  const tokenMatch = hash.match(/^#\/tokens\/([^/?#]+)/)
  if (tokenMatch) {
    try {
      return { name: 'token', id: decodeURIComponent(tokenMatch[1]) }
    } catch {
      return { name: 'tokens' }
    }
  }
  if (hash.startsWith('#/tokens')) {
    return { name: 'tokens' }
  }
  return { name: 'dashboard' }
}

function errorStatus(err: unknown): number | undefined {
  if (!err || typeof err !== 'object' || !('status' in err)) {
    return undefined
  }
  const value = (err as { status?: unknown }).status
  return typeof value === 'number' ? value : undefined
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

function createProbeButtonModel(total: number): ProbeButtonModel {
  return {
    state: 'idle',
    completed: 0,
    total,
  }
}

function getProbeErrorMessage(err: unknown): string {
  if (err instanceof Error && err.message.trim().length > 0) {
    return err.message
  }
  return 'Request failed'
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' ? value as Record<string, unknown> : null
}

function envelopeError(payload: unknown): string | null {
  return getProbeEnvelopeError(payload)
}

function getResearchRequestId(payload: unknown): string | null {
  const map = asRecord(payload)
  if (!map) return null
  const snake = map.request_id
  if (typeof snake === 'string' && snake.trim().length > 0) return snake
  const camel = map.requestId
  if (typeof camel === 'string' && camel.trim().length > 0) return camel
  return null
}

function quotaWindowLabel(
  probeText: typeof EN.detail.probe,
  window: ProbeQuotaWindow,
): string {
  return probeText.quotaWindows[window]
}

function quotaBlockedDetail(
  probeText: typeof EN.detail.probe,
  window: ProbeQuotaWindow,
): string {
  return formatTemplate(probeText.quotaBlocked, {
    window: quotaWindowLabel(probeText, window),
  })
}

function formatTemplate(
  template: string,
  values: Record<string, string | number>,
): string {
  return Object.entries(values).reduce(
    (current, [key, value]) => current.replace(new RegExp(`\\{${key}\\}`, 'g'), String(value)),
    template,
  )
}

export default function UserConsole(): JSX.Element {
  const language = useLanguage().language
  const publicStrings = useTranslate().public
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
  const [activeGuide, setActiveGuide] = useState<GuideKey>('codex')
  const [isMobileGuide, setIsMobileGuide] = useState(false)
  const [mcpProbe, setMcpProbe] = useState<ProbeButtonModel>(() => createProbeButtonModel(2))
  const [apiProbe, setApiProbe] = useState<ProbeButtonModel>(() => createProbeButtonModel(6))
  const [probeBubble, setProbeBubble] = useState<ProbeBubbleModel | null>(null)
  const [probeBubbleShift, setProbeBubbleShift] = useState(0)
  const probeBubbleRef = useRef<HTMLDivElement | null>(null)
  const probeRunIdRef = useRef(0)
  const pageRef = useRef<HTMLElement>(null)
  const { viewportMode, contentMode, isCompactLayout } = useResponsiveModes(pageRef)

  useEffect(() => {
    const onHash = () => setRoute(parseRouteFromHash())
    window.addEventListener('hashchange', onHash)
    return () => window.removeEventListener('hashchange', onHash)
  }, [])

  const reloadBase = useCallback(async (signal: AbortSignal) => {
    try {
      const nextProfile = await fetchProfile(signal)
      setProfile(nextProfile)

      const availability = resolveUserConsoleAvailability(nextProfile)
      if (availability === 'logged_out') {
        window.location.href = '/'
        return
      }
      if (availability === 'disabled') {
        setDashboard(null)
        setTokens([])
        setDetail(null)
        setDetailLogs([])
        setError(null)
        return
      }

      const [nextDashboard, nextTokens] = await Promise.all([
        fetchUserDashboard(signal),
        fetchUserTokens(signal),
      ])
      setDashboard(nextDashboard)
      setTokens(nextTokens)
      setError(null)
    } catch (err) {
      const message = err instanceof Error ? err.message : text.errors.load
      setError(message)
      if (errorStatus(err) === 401) {
        window.location.href = '/'
      }
    } finally {
      setLoading(false)
    }
  }, [text.errors.load])

  useEffect(() => {
    const controller = new AbortController()
    void reloadBase(controller.signal)
    return () => controller.abort()
  }, [reloadBase])

  const consoleAvailability = resolveUserConsoleAvailability(profile)

  useEffect(() => {
    if (consoleAvailability !== 'enabled' || route.name !== 'token') {
      setDetail(null)
      setDetailLogs([])
      setDetailLoading(false)
      return
    }
    setDetail(null)
    setDetailLogs([])
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
        if (errorStatus(err) === 401) {
          window.location.href = '/'
        }
      })
      .finally(() => setDetailLoading(false))
    return () => controller.abort()
  }, [consoleAvailability, route, text.errors.detail])

  useEffect(() => {
    probeRunIdRef.current += 1
    setMcpProbe(createProbeButtonModel(2))
    setApiProbe(createProbeButtonModel(6))
    setProbeBubble(null)
    setProbeBubbleShift(0)
  }, [route.name === 'token' ? route.id : route.name])

  useLayoutEffect(() => {
    if (!probeBubble?.visible || probeBubble.items.length === 0) {
      setProbeBubbleShift(0)
      return
    }

    let frame = 0
    const updateShift = () => {
      const bubble = probeBubbleRef.current
      if (!bubble) return
      const rect = bubble.getBoundingClientRect()
      const viewportWidth = window.innerWidth
      const margin = 10

      let nextShift = 0
      if (rect.left < margin) {
        nextShift = margin - rect.left
      } else if (rect.right > viewportWidth - margin) {
        nextShift = (viewportWidth - margin) - rect.right
      }
      setProbeBubbleShift(Math.round(nextShift))
    }

    frame = window.requestAnimationFrame(updateShift)
    window.addEventListener('resize', updateShift)
    return () => {
      window.cancelAnimationFrame(frame)
      window.removeEventListener('resize', updateShift)
    }
  }, [probeBubble])

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

  const guideToken = useMemo(() => {
    if (route.name === 'token') {
      return tokenLabel(route.id)
    }
    return 'th-xxxx-xxxxxxxxxxxx'
  }, [route])

  const guideDescription = useMemo<GuideContent>(() => {
    const baseUrl = window.location.origin
    const guides = buildGuideContent(language, baseUrl, guideToken)
    return guides[activeGuide]
  }, [activeGuide, guideToken, language])

  const guideTabs = useMemo(
    () => GUIDE_KEY_ORDER.map((id) => ({ id, label: publicStrings.guide.tabs[id] ?? id })),
    [publicStrings.guide.tabs],
  )

  const anyProbeRunning = mcpProbe.state === 'running' || apiProbe.state === 'running'
  const adminHref = getUserConsoleAdminHref(profile)
  const consoleUnavailable = consoleAvailability === 'disabled'

  const runMcpProbe = useCallback(async () => {
    if (route.name !== 'token' || anyProbeRunning) return
    const runId = probeRunIdRef.current + 1
    probeRunIdRef.current = runId
    const isActiveRun = () => probeRunIdRef.current === runId
    const probeText = text.detail.probe

    const stepDefinitions: McpProbeStepDefinition[] = [
      {
        id: 'mcp-ping',
        label: probeText.steps.mcpPing,
        run: async (token: string): Promise<string | null> => {
          const payload = await probeMcpPing(token)
          const error = envelopeError(payload)
          if (error) throw new Error(error)
          return null
        },
      },
      {
        id: 'mcp-tools-list',
        label: probeText.steps.mcpToolsList,
        run: async (token: string): Promise<string | null> => {
          const payload = await probeMcpToolsList(token)
          const error = envelopeError(payload)
          if (error) throw new Error(error)
          return null
        },
      },
    ]

    setMcpProbe({
      state: 'running',
      completed: 0,
      total: stepDefinitions.length,
    })
    setProbeBubble({ visible: true, anchor: 'mcp', items: [] })

    let token = ''
    try {
      const secret = await fetchUserTokenSecret(route.id)
      if (!isActiveRun()) return
      token = secret.token
    } catch (err) {
      if (!isActiveRun()) return
      setMcpProbe({
        state: 'failed',
        completed: 0,
        total: stepDefinitions.length,
      })
      setProbeBubble({
        visible: true,
        anchor: 'mcp',
        items: [{
          id: stepDefinitions[0].id,
          label: stepDefinitions[0].label,
          status: 'failed',
          detail: formatTemplate(probeText.preflightFailed, { message: getProbeErrorMessage(err) }),
        }],
      })
      return
    }

    let quotaBlockedWindow = getTokenBusinessQuotaWindow(detail)
    if (quotaBlockedWindow) {
      try {
        const revalidatedQuota = await revalidateBlockedQuotaWindow(detail, async () => {
          return await fetchUserTokenDetail(route.id)
        })
        if (!isActiveRun()) return
        quotaBlockedWindow = revalidatedQuota.window
        if (revalidatedQuota.token) {
          setDetail(revalidatedQuota.token)
        }
      } catch {
        if (!isActiveRun()) return
      }
    }

    const completedItems: ProbeBubbleItem[] = []
    const stepStates: McpProbeStepState[] = []

    for (let index = 0; index < stepDefinitions.length; index += 1) {
      if (!isActiveRun()) return
      const current = stepDefinitions[index]
      const runningItem: ProbeBubbleItem = {
        id: current.id,
        label: current.label,
        status: 'running',
      }
      setProbeBubble({
        visible: true,
        anchor: 'mcp',
        items: [...completedItems, runningItem],
      })

      if (current.id === 'mcp-ping' && quotaBlockedWindow) {
        completedItems.push({
          ...runningItem,
          status: 'blocked',
          detail: quotaBlockedDetail(probeText, quotaBlockedWindow),
        })
        stepStates.push('blocked')
      } else {
        try {
          await current.run(token)
          if (!isActiveRun()) return
          completedItems.push({
            ...runningItem,
            status: 'success',
          })
          stepStates.push('success')
        } catch (err) {
          if (!isActiveRun()) return
          const quotaWindow = current.id === 'mcp-ping' && err instanceof McpProbeRequestError
            ? getQuotaExceededWindow(err.payload)
            : null
          if (quotaWindow) {
            try {
              const refreshedDetail = await fetchUserTokenDetail(route.id)
              if (!isActiveRun()) return
              setDetail(refreshedDetail)
            } catch {
              if (!isActiveRun()) return
            }

            completedItems.push({
              ...runningItem,
              status: 'blocked',
              detail: quotaBlockedDetail(probeText, quotaWindow),
            })
            stepStates.push('blocked')
          } else {
            completedItems.push({
              ...runningItem,
              status: 'failed',
              detail: getProbeErrorMessage(err),
            })
            stepStates.push('failed')
          }
        }
      }

      setMcpProbe((prev) => ({
        ...prev,
        state: 'running',
        completed: index + 1,
      }))
      setProbeBubble({
        visible: true,
        anchor: 'mcp',
        items: [...completedItems],
      })
    }
    if (!isActiveRun()) return

    const finalState = resolveMcpProbeButtonState(stepStates)
    setMcpProbe({
      state: finalState,
      completed: stepDefinitions.length,
      total: stepDefinitions.length,
    })
    setProbeBubble({ visible: true, anchor: 'mcp', items: [...completedItems] })
  }, [anyProbeRunning, detail, route, text.detail.probe])

  const runApiProbe = useCallback(async () => {
    if (route.name !== 'token' || anyProbeRunning) return
    const runId = probeRunIdRef.current + 1
    probeRunIdRef.current = runId
    const isActiveRun = () => probeRunIdRef.current === runId

    const stepDefinitions: ApiProbeStepDefinition[] = [
      {
        id: 'api-search',
        label: text.detail.probe.steps.apiSearch,
        run: async (token: string): Promise<string | null> => {
          const payload = await probeApiTavilySearch(token, {
            query: 'health check',
            max_results: 1,
            search_depth: 'basic',
            include_answer: false,
            include_raw_content: false,
            include_images: false,
          })
          const error = envelopeError(payload)
          if (error) throw new Error(error)
          return null
        },
      },
      {
        id: 'api-extract',
        label: text.detail.probe.steps.apiExtract,
        run: async (token: string): Promise<string | null> => {
          const payload = await probeApiTavilyExtract(token, {
            urls: ['https://example.com'],
            include_images: false,
          })
          const error = envelopeError(payload)
          if (error) throw new Error(error)
          return null
        },
      },
      {
        id: 'api-crawl',
        label: text.detail.probe.steps.apiCrawl,
        run: async (token: string): Promise<string | null> => {
          const payload = await probeApiTavilyCrawl(token, {
            url: 'https://example.com',
            max_depth: 1,
            limit: 1,
          })
          const error = envelopeError(payload)
          if (error) throw new Error(error)
          return null
        },
      },
      {
        id: 'api-map',
        label: text.detail.probe.steps.apiMap,
        run: async (token: string): Promise<string | null> => {
          const payload = await probeApiTavilyMap(token, {
            url: 'https://example.com',
            max_depth: 1,
            limit: 1,
          })
          const error = envelopeError(payload)
          if (error) throw new Error(error)
          return null
        },
      },
      {
        id: 'api-research',
        label: text.detail.probe.steps.apiResearch,
        run: async (token: string): Promise<string | null> => {
          const payload = await probeApiTavilyResearch(token, {
            input: 'health check',
            model: 'mini',
            citation_format: 'numbered',
          })
          const error = envelopeError(payload)
          if (error) throw new Error(error)
          const requestId = getResearchRequestId(payload)
          if (!requestId) {
            throw new Error(text.detail.probe.errors.missingRequestId)
          }
          return requestId
        },
      },
      {
        id: 'api-research-result',
        label: text.detail.probe.steps.apiResearchResult,
        run: async (token: string, context: { requestId: string | null }): Promise<string | null> => {
          if (!context.requestId) {
            throw new Error(text.detail.probe.errors.missingRequestId)
          }
          const payload = await probeApiTavilyResearchResult(token, context.requestId)
          const error = envelopeError(payload)
          if (error) throw new Error(error)
          const status = payload.status
          if (typeof status === 'string' && status.trim().length > 0) {
            const normalized = status.trim().toLowerCase()
            if (
              normalized === 'failed'
              || normalized === 'failure'
              || normalized === 'error'
              || normalized === 'errored'
              || normalized === 'cancelled'
              || normalized === 'canceled'
            ) {
              throw new Error(text.detail.probe.errors.researchFailed)
            }
            if (
              normalized === 'pending'
              || normalized === 'processing'
              || normalized === 'running'
              || normalized === 'in_progress'
              || normalized === 'queued'
            ) {
              return text.detail.probe.researchPendingAccepted
            }
            if (
              normalized === 'completed'
              || normalized === 'success'
              || normalized === 'succeeded'
              || normalized === 'done'
            ) {
              return formatTemplate(text.detail.probe.researchStatus, { status: normalized })
            }
            throw new Error(
              formatTemplate(text.detail.probe.errors.researchUnexpectedStatus, {
                status: normalized,
              }),
            )
          }
          return null
        },
      },
    ]

    setApiProbe({
      state: 'running',
      completed: 0,
      total: stepDefinitions.length,
    })
    setProbeBubble({ visible: true, anchor: 'api', items: [] })

    let token = ''
    try {
      const secret = await fetchUserTokenSecret(route.id)
      if (!isActiveRun()) return
      token = secret.token
    } catch (err) {
      if (!isActiveRun()) return
      setApiProbe({
        state: 'failed',
        completed: 0,
        total: stepDefinitions.length,
      })
      setProbeBubble({
        visible: true,
        anchor: 'api',
        items: [{
          id: stepDefinitions[0].id,
          label: stepDefinitions[0].label,
          status: 'failed',
        }],
      })
      return
    }

    const completedItems: ProbeBubbleItem[] = []
    let passed = 0
    let researchRequestId: string | null = null
    for (let index = 0; index < stepDefinitions.length; index += 1) {
      if (!isActiveRun()) return
      const current = stepDefinitions[index]
      const runningItem: ProbeBubbleItem = {
        id: current.id,
        label: current.label,
        status: 'running',
      }
      setProbeBubble({
        visible: true,
        anchor: 'api',
        items: [...completedItems, runningItem],
      })

      try {
        const detail = await current.run(token, { requestId: researchRequestId })
        if (!isActiveRun()) return
        if (current.id === 'api-research' && detail) {
          researchRequestId = detail
        }
        passed += 1
        completedItems.push({
          ...runningItem,
          status: 'success',
        })
      } catch (err) {
        if (!isActiveRun()) return
        completedItems.push({
          ...runningItem,
          status: 'failed',
        })
      }
      setApiProbe((prev) => ({
        ...prev,
        state: 'running',
        completed: index + 1,
      }))
      setProbeBubble({
        visible: true,
        anchor: 'api',
        items: [...completedItems],
      })
    }
    if (!isActiveRun()) return

    const failed = stepDefinitions.length - passed
    const finalState: ProbeButtonState = failed === 0
      ? 'success'
      : passed === 0
        ? 'failed'
        : 'partial'
    setApiProbe({
      state: finalState,
      completed: stepDefinitions.length,
      total: stepDefinitions.length,
    })
    setProbeBubble({ visible: true, anchor: 'api', items: [...completedItems] })
  }, [anyProbeRunning, route, text.detail.probe])

  const buttonMeta = useMemo(() => {
    const tone = (state: ProbeButtonState): string => {
      if (state === 'success') return 'user-console-probe-btn-success'
      if (state === 'partial') return 'user-console-probe-btn-partial'
      if (state === 'failed') return 'user-console-probe-btn-failed'
      if (state === 'running') return 'user-console-probe-btn-running'
      return 'user-console-probe-btn-idle'
    }
    const icon = (state: ProbeButtonState): string => {
      if (state === 'success') return 'mdi:check-circle-outline'
      if (state === 'partial') return 'mdi:alert-circle-outline'
      if (state === 'failed') return 'mdi:close-circle-outline'
      if (state === 'running') return 'mdi:loading'
      return 'mdi:play-circle-outline'
    }
    return {
      tone,
      icon,
    }
  }, [])

  const probeItemMeta = useMemo(() => {
    const icon = (status: ProbeStepStatus): string => {
      if (status === 'success') return 'mdi:check-circle-outline'
      if (status === 'failed') return 'mdi:close-circle-outline'
      if (status === 'blocked') return 'mdi:alert-circle-outline'
      return 'mdi:loading'
    }
    const textFor = (status: ProbeStepStatus): string => text.detail.probe.stepStatus[status]
    return {
      icon,
      textFor,
    }
  }, [text.detail.probe.stepStatus])

  const renderProbeBubble = (): JSX.Element | null => {
    if (!probeBubble?.visible || probeBubble.items.length === 0) return null
    const bubbleStyle = {
      '--probe-bubble-shift': `${probeBubbleShift}px`,
    } as React.CSSProperties
    return (
      <div
        ref={probeBubbleRef}
        className={`user-console-probe-bubble user-console-probe-bubble-anchor-${probeBubble.anchor}`}
        style={bubbleStyle}
        role="status"
        aria-live="polite"
      >
        <ul className="user-console-probe-bubble-list">
          {probeBubble.items.map((item) => (
            <li
              key={item.id}
              className="user-console-probe-bubble-item"
              aria-label={`${probeItemMeta.textFor(item.status)} · ${item.label}${item.detail ? ` · ${item.detail}` : ''}`}
            >
              <Icon
                icon={probeItemMeta.icon(item.status)}
                className={
                  `user-console-probe-bubble-item-icon user-console-probe-bubble-item-icon-status-${item.status} `
                  + `${item.status === 'running' ? 'is-spinning' : ''}`
                }
              />
              <div className="user-console-probe-bubble-item-copy">
                <strong className="user-console-probe-bubble-item-label">{item.label}</strong>
                {item.detail ? (
                  <span className="user-console-probe-bubble-item-detail">{item.detail}</span>
                ) : null}
              </div>
            </li>
          ))}
        </ul>
      </div>
    )
  }

  const goDashboard = () => {
    window.location.hash = '#/dashboard'
  }
  const goHome = () => {
    window.location.href = '/'
  }
  const goTokens = () => {
    window.location.hash = '#/tokens'
  }
  const goTokenDetail = (tokenId: string) => {
    window.location.hash = `#/tokens/${encodeURIComponent(tokenId)}`
  }

  const probeButtonLabel = useCallback((
    kind: 'mcp' | 'api',
    model: ProbeButtonModel,
  ): string => {
    const titles = kind === 'mcp' ? text.detail.probe.mcpButton : text.detail.probe.apiButton
    if (model.state === 'running') {
      return formatTemplate(text.detail.probe.runningButton, {
        label: titles.idle,
        done: model.completed,
        total: model.total,
      })
    }
    if (model.state === 'success') return titles.success
    if (model.state === 'partial') return titles.partial
    if (model.state === 'failed') return titles.failed
    return titles.idle
  }, [text.detail.probe])

  return (
    <main
      ref={pageRef}
      className={`app-shell public-home viewport-${viewportMode} content-${contentMode}${
        isCompactLayout ? ' is-compact-layout' : ''
      }`}
    >
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
          {adminHref && (
            <div className="admin-panel-header-actions">
              <Button asChild variant="outline" size="sm" className="user-console-admin-entry">
                <a href={adminHref}>
                  <Icon icon="mdi:crown-outline" width={16} height={16} aria-hidden="true" />
                  <span>{publicStrings.adminButton}</span>
                </a>
              </Button>
            </div>
          )}
        </div>
      </section>

      {!consoleUnavailable && (
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
      )}

      {consoleUnavailable && (
        <section className="surface panel access-panel">
          <div className="console-unavailable-state">
            <div className="console-unavailable-icon" aria-hidden="true">
              <Icon icon="mdi:account-off-outline" width={22} height={22} />
            </div>
            <div className="console-unavailable-copy">
              <h2>{text.unavailable.title}</h2>
              <p>{text.unavailable.description}</p>
            </div>
            <div className="table-actions console-unavailable-actions">
              <button type="button" className="btn btn-primary" onClick={goHome}>
                {text.unavailable.home}
              </button>
            </div>
          </div>
        </section>
      )}

      {!consoleUnavailable && error && <section className="surface error-banner">{error}</section>}

      {!consoleUnavailable && route.name === 'dashboard' && (
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

      {!consoleUnavailable && route.name === 'tokens' && (
        <section className="surface panel">
          <div className="panel-header">
            <h2>{text.tokens.title}</h2>
          </div>
          <div className="table-wrapper jobs-table-wrapper user-console-md-up">
            {tokens.length === 0 ? (
              <div className="empty-state alert">{text.tokens.empty}</div>
            ) : (
              <table className="user-console-tokens-table">
                <thead>
                  <tr>
                    <th>{text.tokens.table.id}</th>
                    <th>{text.tokens.table.quotas}</th>
                    <th>{text.tokens.table.stats}</th>
                    <th>{text.tokens.table.actions}</th>
                  </tr>
                </thead>
                <tbody>
                  {tokens.map((item) => {
                    const state = copyState[item.tokenId] ?? 'idle'
                    return (
                      <tr key={item.tokenId}>
                        <td>
                          <code>{item.tokenId}</code>
                        </td>
                        <td>
                          <div className="user-console-cell-stack">
                            <div className="user-console-cell-item">
                              <span>{text.tokens.table.any}</span>
                              <strong>{formatQuotaPair(item.hourlyAnyUsed, item.hourlyAnyLimit)}</strong>
                            </div>
                            <div className="user-console-cell-item">
                              <span>{text.tokens.table.hourly}</span>
                              <strong>{formatQuotaPair(item.quotaHourlyUsed, item.quotaHourlyLimit)}</strong>
                            </div>
                            <div className="user-console-cell-item">
                              <span>{text.tokens.table.daily}</span>
                              <strong>{formatQuotaPair(item.quotaDailyUsed, item.quotaDailyLimit)}</strong>
                            </div>
                            <div className="user-console-cell-item">
                              <span>{text.tokens.table.monthly}</span>
                              <strong>{formatQuotaPair(item.quotaMonthlyUsed, item.quotaMonthlyLimit)}</strong>
                            </div>
                          </div>
                        </td>
                        <td>
                          <div className="user-console-cell-stack">
                            <div className="user-console-cell-item">
                              <span>{text.tokens.table.dailySuccess}</span>
                              <strong>{formatNumber(item.dailySuccess)}</strong>
                            </div>
                            <div className="user-console-cell-item">
                              <span>{text.tokens.table.dailyFailure}</span>
                              <strong>{formatNumber(item.dailyFailure)}</strong>
                            </div>
                            <div className="user-console-cell-item">
                              <span>{text.dashboard.monthlySuccess}</span>
                              <strong>{formatNumber(item.monthlySuccess)}</strong>
                            </div>
                          </div>
                        </td>
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
          <div className="user-console-mobile-list user-console-md-down">
            {tokens.length === 0 ? (
              <div className="empty-state alert">{text.tokens.empty}</div>
            ) : (
              tokens.map((item) => {
                const state = copyState[item.tokenId] ?? 'idle'
                return (
                  <article key={item.tokenId} className="user-console-mobile-card">
                    <header className="user-console-mobile-card-header">
                      <strong>{text.tokens.table.id}</strong>
                      <code>{item.tokenId}</code>
                    </header>
                    <div className="user-console-mobile-kv">
                      <span>{text.tokens.table.any}</span>
                      <strong>{formatQuotaPair(item.hourlyAnyUsed, item.hourlyAnyLimit)}</strong>
                    </div>
                    <div className="user-console-mobile-kv">
                      <span>{text.tokens.table.hourly}</span>
                      <strong>{formatQuotaPair(item.quotaHourlyUsed, item.quotaHourlyLimit)}</strong>
                    </div>
                    <div className="user-console-mobile-kv">
                      <span>{text.tokens.table.daily}</span>
                      <strong>{formatQuotaPair(item.quotaDailyUsed, item.quotaDailyLimit)}</strong>
                    </div>
                    <div className="user-console-mobile-kv">
                      <span>{text.tokens.table.monthly}</span>
                      <strong>{formatQuotaPair(item.quotaMonthlyUsed, item.quotaMonthlyLimit)}</strong>
                    </div>
                    <div className="user-console-mobile-kv">
                      <span>{text.tokens.table.dailySuccess}</span>
                      <strong>{formatNumber(item.dailySuccess)}</strong>
                    </div>
                    <div className="user-console-mobile-kv">
                      <span>{text.tokens.table.dailyFailure}</span>
                      <strong>{formatNumber(item.dailyFailure)}</strong>
                    </div>
                    <div className="user-console-mobile-kv">
                      <span>{text.dashboard.monthlySuccess}</span>
                      <strong>{formatNumber(item.monthlySuccess)}</strong>
                    </div>
                    <div className="table-actions user-console-mobile-actions">
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
                  </article>
                )
              })
            )}
          </div>
        </section>
      )}

      {!consoleUnavailable && route.name === 'token' && (
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

            <div className="access-token-box user-console-token-box">
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

            <div className="user-console-probe-box">
              <div className="user-console-probe-label-row">
                <label className="token-label">{text.detail.probe.title}</label>
                <span className="user-console-probe-hint">
                  <button
                    type="button"
                    className="user-console-probe-hint-trigger"
                    aria-label={text.detail.probe.costHintAria}
                  >
                    <Icon icon="mdi:help-circle-outline" />
                  </button>
                  <span className="user-console-probe-hint-bubble" role="tooltip">
                    {text.detail.probe.costHint}
                  </span>
                </span>
              </div>
              <div className="user-console-probe-actions">
                <div className="user-console-probe-action">
                  {probeBubble?.anchor === 'mcp' && renderProbeBubble()}
                  <button
                    type="button"
                    data-probe-kind="mcp"
                    className={`btn btn-sm user-console-probe-btn ${buttonMeta.tone(mcpProbe.state)}`}
                    onClick={() => void runMcpProbe()}
                    disabled={anyProbeRunning}
                  >
                    <Icon
                      icon={buttonMeta.icon(mcpProbe.state)}
                      className={`user-console-probe-btn-icon ${mcpProbe.state === 'running' ? 'is-spinning' : ''}`}
                    />
                    <span>{probeButtonLabel('mcp', mcpProbe)}</span>
                  </button>
                </div>
                <div className="user-console-probe-action">
                  {probeBubble?.anchor === 'api' && renderProbeBubble()}
                  <button
                    type="button"
                    data-probe-kind="api"
                    className={`btn btn-sm user-console-probe-btn ${buttonMeta.tone(apiProbe.state)}`}
                    onClick={() => void runApiProbe()}
                    disabled={anyProbeRunning}
                  >
                    <Icon
                      icon={buttonMeta.icon(apiProbe.state)}
                      className={`user-console-probe-btn-icon ${apiProbe.state === 'running' ? 'is-spinning' : ''}`}
                    />
                    <span>{probeButtonLabel('api', apiProbe)}</span>
                  </button>
                </div>
              </div>
            </div>
          </section>

          <section className="surface panel user-console-detail-panel">
            <div className="panel-header">
              <h2>{text.detail.logs}</h2>
            </div>
            <div className="table-wrapper user-console-md-up">
              {detailLogs.length === 0 ? (
                <div className="empty-state alert">{text.detail.emptyLogs}</div>
              ) : (
                <table className="token-detail-table user-console-logs-table">
                  <thead>
                    <tr>
                      <th>{text.detail.table.request}</th>
                      <th>{text.detail.table.transport}</th>
                      <th>{text.detail.table.result}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {detailLogs.map((log) => (
                      <tr key={log.id}>
                        <td>
                          <div className="user-console-log-stack">
                            <strong className="user-console-log-main">{formatTimestamp(log.created_at)}</strong>
                            <span className="user-console-log-meta">
                              {log.method} {log.path}
                              {log.query ? ` · ${log.query}` : ''}
                            </span>
                          </div>
                        </td>
                        <td>
                          <div className="user-console-log-transport">
                            <span className="user-console-log-transport-item">
                              <em>H</em>
                              <strong>{log.http_status ?? '—'}</strong>
                            </span>
                            <span className="user-console-log-transport-item">
                              <em>T</em>
                              <strong>{log.mcp_status ?? '—'}</strong>
                            </span>
                          </div>
                        </td>
                        <td>
                          <div className="user-console-log-result-line">
                            <StatusBadge className="user-console-log-status" tone={statusTone(log.result_status)}>
                              {log.result_status}
                            </StatusBadge>
                            <span className="user-console-log-error">{log.error_message ?? '—'}</span>
                          </div>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
            <div className="user-console-mobile-list user-console-md-down">
              {detailLogs.length === 0 ? (
                <div className="empty-state alert">{text.detail.emptyLogs}</div>
              ) : (
                detailLogs.map((log) => (
                  <article key={log.id} className="user-console-mobile-card">
                    <div className="user-console-mobile-kv">
                      <span>{text.detail.table.request}</span>
                      <strong>{formatTimestamp(log.created_at)}</strong>
                    </div>
                    <div className="user-console-mobile-kv">
                      <span>{text.detail.table.path}</span>
                      <strong>{log.method} {log.path}</strong>
                    </div>
                    <div className="user-console-mobile-kv">
                      <span>{text.detail.table.http}</span>
                      <strong>{log.http_status ?? '—'}</strong>
                    </div>
                    <div className="user-console-mobile-kv">
                      <span>{text.detail.table.mcp}</span>
                      <strong>{log.mcp_status ?? '—'}</strong>
                    </div>
                    <div className="user-console-mobile-kv">
                      <span>{text.detail.table.result}</span>
                      <StatusBadge className="user-console-mobile-status" tone={statusTone(log.result_status)}>
                        {log.result_status}
                      </StatusBadge>
                    </div>
                    <div className="user-console-mobile-kv">
                      <span>{text.detail.table.error}</span>
                      <strong>{log.error_message ?? text.detail.noError}</strong>
                    </div>
                  </article>
                ))
              )}
            </div>
          </section>

          <section className="surface panel public-home-guide">
            <h2>{publicStrings.guide.title}</h2>
            {isCompactLayout && (
              <div className="guide-select" aria-label="Client selector (mobile)">
                <MobileGuideDropdown active={activeGuide} onChange={setActiveGuide} labels={guideTabs} />
              </div>
            )}
            {!isCompactLayout && (
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
            {activeGuide === 'cherryStudio' && <CherryStudioMock apiKeyExample={guideToken} />}
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
    const map: Record<GuideKey, string> = {
      codex: 'simple-icons/openai',
      claude: 'simple-icons/anthropic',
      vscode: 'simple-icons/visualstudiocode',
      claudeDesktop: 'simple-icons/anthropic',
      cursor: 'simple-icons/cursor',
      windsurf: 'simple-icons/codeium',
      cherryStudio: 'mdi/cherry',
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
    cherryStudio: {
      title: isEnglish ? 'Cherry Studio' : 'Cherry Studio 桌面客户端',
      steps: isEnglish
        ? [
            <>1. Copy your Tavily Hikari access token (for example <code>{prettyToken}</code>) for this client.</>,
            <>2. In Cherry Studio, open <strong>Settings → Web Search</strong>.</>,
            <>3. Choose the search provider <strong>Tavily (API key)</strong>.</>,
            <>
              4. Set <strong>API URL</strong> to <code>{baseUrl}/api/tavily</code>.
            </>,
            <>
              5. Set <strong>API key</strong> to the Hikari access token from step 1 (the full <code>{prettyToken}</code> value),{' '}
              <strong>not</strong> your Tavily official API key.
            </>,
            <>
              6. Optionally tweak result count, answer/date options, etc. Cherry Studio will send these fields through to
              Tavily, while Hikari rotates Tavily keys and enforces per-token quotas.
            </>,
          ]
        : [
            <>1）准备好当前客户端要使用的 Tavily Hikari 访问令牌（例如 <code>{prettyToken}</code>）。</>,
            <>2）在 Cherry Studio 中打开 <strong>设置 → 网络搜索（Web Search）</strong>。</>,
            <>3）将搜索服务商设置为 <strong>Tavily (API key)</strong>。</>,
            <>
              4）将 <strong>API 地址 / API URL</strong> 设置为 <code>{baseUrl}/api/tavily</code>。
            </>,
            <>
              5）将 <strong>API 密钥 / API key</strong> 填写为步骤 1 中复制的 Hikari 访问令牌（完整的 <code>{prettyToken}</code>），而不是
              Tavily 官方 API key。
            </>,
            <>6）可按需在 Cherry 中调整返回条数、是否附带答案/日期等选项。</>,
          ],
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
    '<span class="hl-key">experimental_use_rmcp_client</span> = <span class="hl-boolean">true</span>',
    '',
    '[<span class="hl-section">mcp_servers.tavily_hikari</span>]',
    `<span class="hl-key">url</span> = <span class="hl-string">"${baseUrl}/mcp"</span>`,
    '<span class="hl-key">bearer_token_env_var</span> = <span class="hl-string">"TAVILY_HIKARI_TOKEN"</span>',
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
      quotas: 'Quota Windows',
      stats: 'Usage Stats',
      any: 'Any Req (1h)',
      hourly: 'Hourly',
      daily: 'Daily',
      monthly: 'Monthly',
      dailySuccess: 'Daily Success',
      dailyFailure: 'Daily Failure',
      actions: 'Actions',
    },
  },
  unavailable: {
    title: 'User console unavailable',
    description: 'This server has not enabled user OAuth login, so `/console` cannot load dashboard or token data right now.',
    home: 'Back to Home',
  },
  detail: {
    title: 'Token Detail',
    subtitle: 'Same token-level modules as home page (without global site card).',
    back: 'Back',
    tokenLabel: 'Token',
    probe: {
      title: 'Connectivity Checks',
      costHint: "This check uses this token's own quota/credits.",
      costHintAria: 'Quota usage hint',
      mcpButton: {
        idle: 'Test MCP',
        success: 'MCP Ready',
        partial: 'MCP Partial',
        failed: 'MCP Failed',
      },
      apiButton: {
        idle: 'Test API',
        success: 'API Ready',
        partial: 'API Partial',
        failed: 'API Failed',
      },
      runningButton: '{label} {done}/{total}',
      running: 'Checking: {step}',
      preflightFailed: 'Cannot load token secret: {message}',
      summarySuccess: '{passed}/{total} checks passed',
      summaryPartial: '{passed}/{total} checks passed, {failed} failed',
      stepOk: 'OK',
      stepStatus: {
        running: 'Running',
        success: 'Success',
        failed: 'Failed',
        blocked: 'Blocked',
      },
      quotaBlocked: '{window} quota exhausted, skipping billable MCP ping.',
      quotaWindows: {
        hour: 'Hourly',
        day: 'Daily',
        month: 'Monthly',
      },
      bubbles: {
        mcpTitle: 'MCP Probe',
        apiTitle: 'API Probe',
        current: 'Current',
        result: 'Result',
        preflight: 'Token Secret',
        done: 'Summary',
      },
      steps: {
        mcpPing: 'MCP service connectivity',
        mcpToolsList: 'MCP tool discovery',
        apiSearch: 'Web search capability',
        apiExtract: 'Page extract capability',
        apiCrawl: 'Site crawl capability',
        apiMap: 'Site map capability',
        apiResearch: 'Research task creation',
        apiResearchResult: 'Research result query',
      },
      errors: {
        missingRequestId: 'Research request_id is missing',
        researchFailed: 'Research task failed',
        researchUnexpectedStatus: 'Research returned unsupported status: {status}',
      },
      researchPendingAccepted: 'pending (accepted)',
      researchStatus: 'status={status}',
    },
    logs: 'Recent Requests (20)',
    emptyLogs: 'No recent requests.',
    guideTitle: 'Client Setup',
    guideDescription: 'Use the same MCP configuration as the public homepage.',
    table: {
      request: 'Request',
      transport: 'HTTP / Tavily',
      path: 'Path',
      time: 'Time',
      http: 'HTTP',
      mcp: 'Tavily',
      result: 'Result',
      error: 'Error',
    },
    noQuery: 'No query string',
    noError: 'No error message',
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
      quotas: '配额窗口',
      stats: '用量统计',
      any: '任意请求(1h)',
      hourly: '小时',
      daily: '日',
      monthly: '月',
      dailySuccess: '今日成功',
      dailyFailure: '今日失败',
      actions: '操作',
    },
  },
  unavailable: {
    title: '用户控制台暂不可用',
    description: '当前服务未启用用户 OAuth 登录，因此 `/console` 暂时无法加载账户仪表盘与 Token 数据。',
    home: '返回首页',
  },
  detail: {
    title: 'Token 详情',
    subtitle: '保留首页 token 相关模块（不展示首个站点全局卡片）。',
    back: '返回',
    tokenLabel: 'Token',
    probe: {
      title: '连通性检测',
      costHint: '该检测会消耗当前 Token 自身额度。',
      costHintAria: '额度消耗提示',
      mcpButton: {
        idle: '检测 MCP',
        success: 'MCP 就绪',
        partial: 'MCP 部分通过',
        failed: 'MCP 失败',
      },
      apiButton: {
        idle: '检测 API',
        success: 'API 就绪',
        partial: 'API 部分通过',
        failed: 'API 失败',
      },
      runningButton: '{label} {done}/{total}',
      running: '检测中：{step}',
      preflightFailed: '读取 Token 失败：{message}',
      summarySuccess: '{passed}/{total} 项通过',
      summaryPartial: '{passed}/{total} 项通过，{failed} 项失败',
      stepOk: '通过',
      stepStatus: {
        running: '进行中',
        success: '成功',
        failed: '失败',
        blocked: '受阻',
      },
      quotaBlocked: '{window}配额已耗尽，已跳过会消耗额度的 MCP 连通检测。',
      quotaWindows: {
        hour: '小时',
        day: '日',
        month: '月',
      },
      bubbles: {
        mcpTitle: 'MCP 检测',
        apiTitle: 'API 检测',
        current: '当前检测',
        result: '结果',
        preflight: '读取 Token',
        done: '汇总',
      },
      steps: {
        mcpPing: 'MCP 服务连通',
        mcpToolsList: 'MCP 工具发现',
        apiSearch: '网页搜索能力',
        apiExtract: '页面抽取能力',
        apiCrawl: '站点抓取能力',
        apiMap: '站点映射能力',
        apiResearch: '研究任务创建',
        apiResearchResult: '研究结果查询',
      },
      errors: {
        missingRequestId: 'research 响应缺少 request_id',
        researchFailed: 'research 任务失败',
        researchUnexpectedStatus: 'research 返回了不支持的状态：{status}',
      },
      researchPendingAccepted: 'pending（已受理）',
      researchStatus: '状态={status}',
    },
    logs: '近期请求（20 条）',
    emptyLogs: '暂无请求记录。',
    guideTitle: '客户端接入',
    guideDescription: '沿用首页的 MCP 配置方式即可接入。',
    table: {
      request: '请求',
      transport: 'HTTP / Tavily',
      path: '路径',
      time: '时间',
      http: 'HTTP',
      mcp: 'Tavily',
      result: '结果',
      error: '错误',
    },
    noQuery: '无查询参数',
    noError: '无错误信息',
  },
  errors: {
    load: '加载控制台数据失败',
    detail: '加载 Token 详情失败',
  },
}
