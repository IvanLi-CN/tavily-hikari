import { useEffect, useState, type KeyboardEvent } from 'react'

import {
  type AdminUserListStats,
  fetchObservedClientIpRequests,
  type ObservedClientIpRequest,
  type RequestLogRetentionProfile,
  type RequestLogRetentionSettings,
  type SystemSettings,
  type UpstreamProjectIdMode,
} from '../api'
import type { QueryLoadState } from './queryLoadState'
import type { AdminTranslations } from '../i18n'
import type { AdminDisplayDensity } from './displayDensity'
import AdminLoadingRegion from '../components/AdminLoadingRegion'
import { Icon } from '../lib/icons'
import { Button } from '../components/ui/button'
import { Input } from '../components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../components/ui/select'
import { Switch } from '../components/ui/switch'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '../components/ui/tooltip'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../components/ui/dialog'

interface SystemSettingsModuleProps {
  strings: AdminTranslations['systemSettings']
  settings: SystemSettings | null
  loadState: QueryLoadState
  error: string | null
  saving: boolean
  helpBubbleOpen?: boolean
  displayDensity?: AdminDisplayDensity
  userListStats?: AdminUserListStats | null
  activeUpstreamMcpSessions?: number
  registrationPolicy?: {
    strings: AdminTranslations['users']['registration']
    checked: boolean | null
    disabled: boolean
    statusText: string
    error: string | null
    onToggle: () => Promise<void> | void
  }
  onDisplayDensityChange?: (density: AdminDisplayDensity) => void
  onOpenMcpSessionBindings?: () => void
  onApply: (settings: SystemSettings) => Promise<void> | void
}

type NormalSystemSettingsOverrides = Partial<
  Pick<
    SystemSettings,
    | 'requestRateLimit'
    | 'authTokenLogRetentionDays'
    | 'mcpSessionAffinityKeyCount'
    | 'rebalanceMcpEnabled'
    | 'apiRebalanceEnabled'
    | 'upstreamProjectIdMode'
    | 'upstreamProjectIdFixedValue'
    | 'upstreamMcpUserAgent'
    | 'upstreamPreciseReconciliationEnabled'
    | 'rechargeFeatureEnabled'
    | 'rechargeUserEnabled'
    | 'adminDefaultActiveUsersOnly'
    | 'userBlockedKeyBaseLimit'
    | 'globalIpLimit'
    | 'requestLogRetention'
  >
>

const requestLogRetentionDayStops = [0, 1, 2, 3, 7, 14, 32, 62, 92] as const
const authTokenLogRetentionDayStops = [1, 2, 3, 7, 14, 32, 62, 92] as const

const defaultRequestLogRetention: RequestLogRetentionSettings = {
  maxLogRetentionDays: 32,
  heavyUsageThresholdPercent: 80,
  global: { businessBodyDays: 7, nonBusinessBodyDays: 0, nonSuccessBodyDays: 3 },
  heavyUsage: { businessBodyDays: 3, nonBusinessBodyDays: 0, nonSuccessBodyDays: 1 },
  debugShared: { businessBodyDays: 14, nonBusinessBodyDays: 1, nonSuccessBodyDays: 7 },
}

function cloneRequestLogRetention(value?: RequestLogRetentionSettings): RequestLogRetentionSettings {
  const source = value ?? defaultRequestLogRetention
  return {
    maxLogRetentionDays: source.maxLogRetentionDays,
    heavyUsageThresholdPercent: source.heavyUsageThresholdPercent,
    global: { ...source.global },
    heavyUsage: { ...source.heavyUsage },
    debugShared: { ...source.debugShared },
  }
}

function dayStopIndex(value: number): number {
  const exact = requestLogRetentionDayStops.indexOf(value as (typeof requestLogRetentionDayStops)[number])
  if (exact >= 0) return exact
  let best = 0
  requestLogRetentionDayStops.forEach((stop, index) => {
    if (Math.abs(stop - value) < Math.abs(requestLogRetentionDayStops[best] - value)) best = index
  })
  return best
}

function authTokenLogRetentionDayStopIndex(value: number): number {
  const exact = authTokenLogRetentionDayStops.indexOf(
    value as (typeof authTokenLogRetentionDayStops)[number],
  )
  if (exact >= 0) return exact
  let best = 0
  authTokenLogRetentionDayStops.forEach((stop, index) => {
    if (Math.abs(stop - value) < Math.abs(authTokenLogRetentionDayStops[best] - value)) best = index
  })
  return best
}

function isValidCountDraft(value: string): value is `${number}` {
  if (!/^\d+$/.test(value)) return false
  const parsed = Number.parseInt(value, 10)
  return Number.isSafeInteger(parsed) && parsed >= 1 && parsed <= 1000
}

function isValidNonNegativeIntegerDraft(value: string): value is `${number}` {
  if (!/^\d+$/.test(value)) return false
  const parsed = Number.parseInt(value, 10)
  return Number.isSafeInteger(parsed) && parsed >= 0
}

function isValidRequestRateLimitDraft(value: string): value is `${number}` {
  if (!/^\d+$/.test(value)) return false
  const parsed = Number.parseInt(value, 10)
  return Number.isSafeInteger(parsed) && parsed >= 1
}

function isValidPercentDraft(value: string): value is `${number}` {
  if (!/^\d+$/.test(value)) return false
  const parsed = Number.parseInt(value, 10)
  return Number.isSafeInteger(parsed) && parsed >= 0 && parsed <= 100
}

function utf8ByteLength(value: string): number {
  return new TextEncoder().encode(value).length
}

function isValidUpstreamHeaderDraft(value: string, maxBytes: number, allowEmpty: boolean): boolean {
  if (!allowEmpty && value.length === 0) return false
  if (/\p{Cc}/u.test(value)) return false
  return utf8ByteLength(value) <= maxBytes
}

const clientIpHeaderPresets = [
  {
    group: '通用反代',
    headers: ['x-real-ip', 'x-forwarded-for', 'forwarded'],
  },
  {
    group: 'Cloudflare',
    headers: ['cf-connecting-ip', 'true-client-ip', 'cf-connecting-ipv6'],
    note: '通常与通用反代中的 x-forwarded-for 一起使用',
  },
  {
    group: 'EdgeOne',
    headers: ['eo-connecting-ip'],
    note: '通常与通用反代中的 x-forwarded-for 一起使用',
  },
] as const

function normalizedTrustedProxyCidrsFromDraft(current: string): string[] {
  return current
    .split(/\r?\n/)
    .map((value) => value.trim())
    .filter(Boolean)
}

export function parseTrustedClientIpHeaderDraft(current: string): {
  values: string[]
  duplicateError: string | null
} {
  const values: string[] = []
  const linesByValue = new Map<string, number[]>()
  current.split(/\r?\n/).forEach((rawValue, index) => {
    const value = rawValue.trim().toLowerCase()
    if (!value) return
    values.push(value)
    linesByValue.set(value, [...(linesByValue.get(value) ?? []), index + 1])
  })
  const duplicates = Array.from(linesByValue.entries()).filter(([, lines]) => lines.length > 1)
  return {
    values,
    duplicateError:
      duplicates.length > 0
        ? `客户端 IP 请求头重复：${duplicates
            .map(([value, lines]) => `${value} 出现在第 ${lines.join('、')} 行`)
            .join('；')}`
        : null,
  }
}

export function toggleOrderedHeaderDraft(current: string, header: string): string {
  const normalizedHeader = header.trim().toLowerCase()
  const values = current
    .split(/\r?\n/)
    .map((value) => value.trim().toLowerCase())
    .filter(Boolean)
  if (values.includes(normalizedHeader)) {
    return values.filter((value) => value !== normalizedHeader).join('\n')
  }
  return [...values, normalizedHeader].join('\n')
}

function SystemSettingsHelpBubble({
  strings,
  open,
}: {
  strings: AdminTranslations['systemSettings']
  open?: boolean
}): JSX.Element {
  return (
    <TooltipProvider>
      <Tooltip {...(open == null ? {} : { open })}>
        <TooltipTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            size="xs"
            className="h-7 w-7 rounded-full px-0 text-muted-foreground hover:text-foreground"
            aria-label={strings.helpLabel}
            data-testid="system-settings-help-trigger"
          >
            <Icon icon="mdi:help-circle-outline" width={16} height={16} aria-hidden="true" />
          </Button>
        </TooltipTrigger>
        <TooltipContent side="right" align="start" className="max-w-[min(24rem,calc(100vw-2rem))]">
          <div style={{ display: 'grid', gap: 8 }}>
            <p>{strings.description}</p>
            <p>{strings.form.description}</p>
            <p>{strings.form.requestRateLimitHint}</p>
            <p>{strings.form.countHint}</p>
            <p>{strings.form.rebalanceHint}</p>
            <p>{strings.form.apiRebalanceHint}</p>
            <p>{strings.form.blockedKeyBaseLimitHint}</p>
            <p>{strings.form.applyScopeHint}</p>
          </div>
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  )
}

export default function SystemSettingsModule({
  strings,
  settings,
  loadState,
  error,
  saving,
  helpBubbleOpen,
  displayDensity = 'comfortable',
  userListStats,
  activeUpstreamMcpSessions = 0,
  registrationPolicy,
  onDisplayDensityChange = () => {},
  onOpenMcpSessionBindings,
  onApply,
}: SystemSettingsModuleProps): JSX.Element {
  const [draftRequestRateLimit, setDraftRequestRateLimit] = useState(() =>
    settings ? String(settings.requestRateLimit) : '100',
  )
  const [draftAuthTokenLogRetentionDays, setDraftAuthTokenLogRetentionDays] = useState(
    settings?.authTokenLogRetentionDays ?? 92,
  )
  const [draftCount, setDraftCount] = useState(() => (settings ? String(settings.mcpSessionAffinityKeyCount) : ''))
  const [draftRebalanceEnabled, setDraftRebalanceEnabled] = useState(settings?.rebalanceMcpEnabled ?? false)
  const [draftApiRebalanceEnabled, setDraftApiRebalanceEnabled] = useState(settings?.apiRebalanceEnabled ?? false)
  const [draftUpstreamProjectIdMode, setDraftUpstreamProjectIdMode] = useState<UpstreamProjectIdMode>(
    settings?.upstreamProjectIdMode ?? 'accessToken',
  )
  const [draftUpstreamProjectIdFixedValue, setDraftUpstreamProjectIdFixedValue] = useState(
    settings?.upstreamProjectIdFixedValue ?? '',
  )
  const [draftUpstreamMcpUserAgent, setDraftUpstreamMcpUserAgent] = useState(
    settings?.upstreamMcpUserAgent ?? '',
  )
  const [draftUpstreamPreciseReconciliationEnabled, setDraftUpstreamPreciseReconciliationEnabled] = useState(
    settings?.upstreamPreciseReconciliationEnabled ?? false,
  )
  const [draftRechargeFeatureEnabled, setDraftRechargeFeatureEnabled] = useState(
    settings?.rechargeFeatureEnabled ?? true,
  )
  const [draftRechargeUserEnabled, setDraftRechargeUserEnabled] = useState(settings?.rechargeUserEnabled ?? true)
  const [draftAdminDefaultActiveUsersOnly, setDraftAdminDefaultActiveUsersOnly] = useState(
    settings?.adminDefaultActiveUsersOnly ?? false,
  )
  const [draftBlockedKeyBaseLimit, setDraftBlockedKeyBaseLimit] = useState(() =>
    settings ? String(settings.userBlockedKeyBaseLimit) : '5',
  )
  const [draftGlobalIpLimit, setDraftGlobalIpLimit] = useState(() => (settings ? String(settings.globalIpLimit) : '5'))
  const [draftRequestLogRetention, setDraftRequestLogRetention] = useState<RequestLogRetentionSettings>(() =>
    cloneRequestLogRetention(settings?.requestLogRetention),
  )
  const [clientIpDialogOpen, setClientIpDialogOpen] = useState(false)
  const [draftTrustedProxyCidrs, setDraftTrustedProxyCidrs] = useState(
    () => settings?.trustedProxyCidrs?.join('\n') ?? '',
  )
  const [draftTrustedClientIpHeaders, setDraftTrustedClientIpHeaders] = useState(
    () => settings?.trustedClientIpHeaders?.join('\n') ?? '',
  )
  const [observedClientIpRequests, setObservedClientIpRequests] = useState<ObservedClientIpRequest[]>([])
  const [observedClientIpRequestsError, setObservedClientIpRequestsError] = useState<string | null>(null)

  useEffect(() => {
    setDraftRequestRateLimit(settings ? String(settings.requestRateLimit) : '100')
    setDraftAuthTokenLogRetentionDays(settings?.authTokenLogRetentionDays ?? 92)
    setDraftCount(settings ? String(settings.mcpSessionAffinityKeyCount) : '')
    setDraftRebalanceEnabled(settings?.rebalanceMcpEnabled ?? false)
    setDraftApiRebalanceEnabled(settings?.apiRebalanceEnabled ?? false)
    setDraftUpstreamProjectIdMode(settings?.upstreamProjectIdMode ?? 'accessToken')
    setDraftUpstreamProjectIdFixedValue(settings?.upstreamProjectIdFixedValue ?? '')
    setDraftUpstreamMcpUserAgent(settings?.upstreamMcpUserAgent ?? '')
    setDraftUpstreamPreciseReconciliationEnabled(settings?.upstreamPreciseReconciliationEnabled ?? false)
    setDraftRechargeFeatureEnabled(settings?.rechargeFeatureEnabled ?? true)
    setDraftRechargeUserEnabled(settings?.rechargeUserEnabled ?? true)
    setDraftAdminDefaultActiveUsersOnly(settings?.adminDefaultActiveUsersOnly ?? false)
    setDraftBlockedKeyBaseLimit(settings ? String(settings.userBlockedKeyBaseLimit) : '5')
    setDraftGlobalIpLimit(settings ? String(settings.globalIpLimit) : '5')
    setDraftRequestLogRetention(cloneRequestLogRetention(settings?.requestLogRetention))
    if (!clientIpDialogOpen) {
      setDraftTrustedProxyCidrs(settings?.trustedProxyCidrs?.join('\n') ?? '')
      setDraftTrustedClientIpHeaders(settings?.trustedClientIpHeaders?.join('\n') ?? '')
    }
  }, [
    settings?.requestRateLimit,
    settings?.authTokenLogRetentionDays,
    settings?.mcpSessionAffinityKeyCount,
    settings?.rebalanceMcpEnabled,
    settings?.apiRebalanceEnabled,
    settings?.upstreamProjectIdMode,
    settings?.upstreamProjectIdFixedValue,
    settings?.upstreamMcpUserAgent,
    settings?.upstreamPreciseReconciliationEnabled,
    settings?.rechargeFeatureEnabled,
    settings?.rechargeUserEnabled,
    settings?.adminDefaultActiveUsersOnly,
    settings?.userBlockedKeyBaseLimit,
    settings?.globalIpLimit,
    settings?.requestLogRetention,
    settings?.trustedProxyCidrs,
    settings?.trustedClientIpHeaders,
    clientIpDialogOpen,
  ])

  useEffect(() => {
    if (!clientIpDialogOpen) return
    const controller = new AbortController()
    setObservedClientIpRequestsError(null)
    fetchObservedClientIpRequests(controller.signal)
      .then(setObservedClientIpRequests)
      .catch((err: unknown) => {
        if (!controller.signal.aborted) {
          setObservedClientIpRequestsError(err instanceof Error ? err.message : String(err))
        }
      })
    return () => controller.abort()
  }, [clientIpDialogOpen])

  const normalizedRequestRateLimit = draftRequestRateLimit.trim()
  const normalizedCount = draftCount.trim()
  const normalizedUpstreamProjectIdFixedValue = draftUpstreamProjectIdFixedValue.trim()
  const normalizedUpstreamMcpUserAgent = draftUpstreamMcpUserAgent.trim()
  const normalizedBlockedKeyBaseLimit = draftBlockedKeyBaseLimit.trim()
  const normalizedGlobalIpLimit = draftGlobalIpLimit.trim()
  const normalizedTrustedProxyCidrs = normalizedTrustedProxyCidrsFromDraft(draftTrustedProxyCidrs)
  const parsedTrustedClientIpHeaders = parseTrustedClientIpHeaderDraft(draftTrustedClientIpHeaders)
  const normalizedTrustedClientIpHeaders = parsedTrustedClientIpHeaders.values
  const observedHeaderColumns =
    normalizedTrustedClientIpHeaders.length > 0
      ? normalizedTrustedClientIpHeaders
      : (settings?.trustedClientIpHeaders ?? [])
  const parsedRequestRateLimit = isValidRequestRateLimitDraft(normalizedRequestRateLimit)
    ? Number.parseInt(normalizedRequestRateLimit, 10)
    : null
  const parsedCount = isValidCountDraft(normalizedCount) ? Number.parseInt(normalizedCount, 10) : null
  const parsedBlockedKeyBaseLimit = isValidNonNegativeIntegerDraft(normalizedBlockedKeyBaseLimit)
    ? Number.parseInt(normalizedBlockedKeyBaseLimit, 10)
    : null
  const parsedGlobalIpLimit = isValidNonNegativeIntegerDraft(normalizedGlobalIpLimit)
    ? Number.parseInt(normalizedGlobalIpLimit, 10)
    : null
  const effectiveDraftRebalancePercent = draftRebalanceEnabled ? 100 : 0
  const effectiveDraftApiRebalancePercent = draftApiRebalanceEnabled ? 100 : 0
  const upstreamProjectIdFixedValueValid = isValidUpstreamHeaderDraft(
    normalizedUpstreamProjectIdFixedValue,
    128,
    draftUpstreamProjectIdMode !== 'fixed',
  )
  const upstreamMcpUserAgentValid = isValidUpstreamHeaderDraft(
    normalizedUpstreamMcpUserAgent,
    256,
    true,
  )
  const changed =
    settings != null &&
    parsedRequestRateLimit != null &&
    parsedCount != null &&
    parsedBlockedKeyBaseLimit != null &&
    parsedGlobalIpLimit != null &&
    upstreamProjectIdFixedValueValid &&
    upstreamMcpUserAgentValid &&
    (parsedRequestRateLimit !== settings.requestRateLimit ||
      draftAuthTokenLogRetentionDays !== settings.authTokenLogRetentionDays ||
      parsedCount !== settings.mcpSessionAffinityKeyCount ||
      draftRebalanceEnabled !== settings.rebalanceMcpEnabled ||
      effectiveDraftRebalancePercent !== settings.rebalanceMcpSessionPercent ||
      draftApiRebalanceEnabled !== settings.apiRebalanceEnabled ||
      effectiveDraftApiRebalancePercent !== settings.apiRebalancePercent ||
      draftUpstreamProjectIdMode !== settings.upstreamProjectIdMode ||
      normalizedUpstreamProjectIdFixedValue !== settings.upstreamProjectIdFixedValue ||
      normalizedUpstreamMcpUserAgent !== settings.upstreamMcpUserAgent ||
      draftUpstreamPreciseReconciliationEnabled !== settings.upstreamPreciseReconciliationEnabled ||
      draftRechargeFeatureEnabled !== settings.rechargeFeatureEnabled ||
      draftRechargeUserEnabled !== settings.rechargeUserEnabled ||
      draftAdminDefaultActiveUsersOnly !== settings.adminDefaultActiveUsersOnly ||
      parsedBlockedKeyBaseLimit !== settings.userBlockedKeyBaseLimit ||
      parsedGlobalIpLimit !== settings.globalIpLimit ||
      JSON.stringify(draftRequestLogRetention) !== JSON.stringify(settings.requestLogRetention))
  const trustedClientIpChanged =
    settings != null &&
    parsedTrustedClientIpHeaders.duplicateError == null &&
    (normalizedTrustedProxyCidrs.join('\n') !== settings.trustedProxyCidrs.join('\n') ||
      normalizedTrustedClientIpHeaders.join('\n') !== settings.trustedClientIpHeaders.join('\n'))
  const fieldErrors = {
    requestRateLimit:
      normalizedRequestRateLimit.length > 0 && parsedRequestRateLimit == null
        ? strings.form.invalidRequestRateLimit
        : null,
    count: normalizedCount.length > 0 && parsedCount == null ? strings.form.invalidCount : null,
    upstreamProjectIdFixedValue:
      !upstreamProjectIdFixedValueValid ? strings.form.invalidUpstreamProjectIdFixedValue : null,
    upstreamMcpUserAgent:
      !upstreamMcpUserAgentValid ? strings.form.invalidUpstreamMcpUserAgent : null,
    blockedKeyBaseLimit:
      normalizedBlockedKeyBaseLimit.length > 0 && parsedBlockedKeyBaseLimit == null
        ? strings.form.invalidBlockedKeyBaseLimit
        : null,
    globalIpLimit:
      normalizedGlobalIpLimit.length > 0 && parsedGlobalIpLimit == null ? strings.form.invalidGlobalIpLimit : null,
  }
  const inlineError =
    fieldErrors.requestRateLimit ??
    fieldErrors.count ??
    fieldErrors.upstreamProjectIdFixedValue ??
    fieldErrors.upstreamMcpUserAgent ??
    fieldErrors.blockedKeyBaseLimit ??
    fieldErrors.globalIpLimit ??
    parsedTrustedClientIpHeaders.duplicateError ??
    error
  const requestRateLimitErrorId = 'system-settings-request-rate-limit-error'
  const blockedKeyBaseLimitErrorId = 'system-settings-blocked-key-base-limit-error'
  const globalIpLimitErrorId = 'system-settings-global-ip-limit-error'
  const affinityCountErrorId = 'system-settings-affinity-count-error'
  const upstreamProjectIdFixedValueErrorId = 'system-settings-upstream-project-id-fixed-value-error'
  const upstreamMcpUserAgentErrorId = 'system-settings-upstream-mcp-user-agent-error'

  const buildNormalSettingsPayload = (overrides: NormalSystemSettingsOverrides = {}): SystemSettings | null => {
    const nextRebalanceMcpEnabled = overrides.rebalanceMcpEnabled ?? draftRebalanceEnabled
    const nextApiRebalanceEnabled = overrides.apiRebalanceEnabled ?? draftApiRebalanceEnabled
    const nextUpstreamProjectIdMode = overrides.upstreamProjectIdMode ?? draftUpstreamProjectIdMode
    const nextUpstreamProjectIdFixedValue = (
      overrides.upstreamProjectIdFixedValue ?? normalizedUpstreamProjectIdFixedValue
    ).trim()
    const nextUpstreamMcpUserAgent = (overrides.upstreamMcpUserAgent ?? normalizedUpstreamMcpUserAgent).trim()
    const nextUpstreamProjectIdFixedValueValid = isValidUpstreamHeaderDraft(
      nextUpstreamProjectIdFixedValue,
      128,
      nextUpstreamProjectIdMode !== 'fixed',
    )
    const nextUpstreamMcpUserAgentValid = isValidUpstreamHeaderDraft(nextUpstreamMcpUserAgent, 256, true)
    if (
      settings == null ||
      parsedRequestRateLimit == null ||
      parsedCount == null ||
      parsedBlockedKeyBaseLimit == null ||
      parsedGlobalIpLimit == null ||
      !nextUpstreamProjectIdFixedValueValid ||
      !nextUpstreamMcpUserAgentValid
    )
      return null
    return {
      requestRateLimit: overrides.requestRateLimit ?? parsedRequestRateLimit,
      authTokenLogRetentionDays:
        overrides.authTokenLogRetentionDays ?? draftAuthTokenLogRetentionDays,
      mcpSessionAffinityKeyCount: overrides.mcpSessionAffinityKeyCount ?? parsedCount,
      rebalanceMcpEnabled: nextRebalanceMcpEnabled,
      rebalanceMcpSessionPercent: nextRebalanceMcpEnabled ? 100 : 0,
      apiRebalanceEnabled: nextApiRebalanceEnabled,
      apiRebalancePercent: nextApiRebalanceEnabled ? 100 : 0,
      upstreamProjectIdMode: nextUpstreamProjectIdMode,
      upstreamProjectIdFixedValue: nextUpstreamProjectIdFixedValue,
      upstreamMcpUserAgent: nextUpstreamMcpUserAgent,
      upstreamPreciseReconciliationEnabled:
        overrides.upstreamPreciseReconciliationEnabled ?? draftUpstreamPreciseReconciliationEnabled,
      rechargeFeatureEnabled: overrides.rechargeFeatureEnabled ?? draftRechargeFeatureEnabled,
      rechargeUserEnabled: overrides.rechargeUserEnabled ?? draftRechargeUserEnabled,
      adminDefaultActiveUsersOnly:
        overrides.adminDefaultActiveUsersOnly ?? draftAdminDefaultActiveUsersOnly,
      userBlockedKeyBaseLimit: overrides.userBlockedKeyBaseLimit ?? parsedBlockedKeyBaseLimit,
      globalIpLimit: overrides.globalIpLimit ?? parsedGlobalIpLimit,
      requestLogRetention: overrides.requestLogRetention ?? draftRequestLogRetention,
      trustedProxyCidrs: settings.trustedProxyCidrs,
      trustedClientIpHeaders: settings.trustedClientIpHeaders,
    }
  }

  const normalPayloadChanged = (payload: SystemSettings): boolean =>
    settings != null &&
    (payload.requestRateLimit !== settings.requestRateLimit ||
      payload.authTokenLogRetentionDays !== settings.authTokenLogRetentionDays ||
      payload.mcpSessionAffinityKeyCount !== settings.mcpSessionAffinityKeyCount ||
      payload.rebalanceMcpEnabled !== settings.rebalanceMcpEnabled ||
      payload.rebalanceMcpSessionPercent !== settings.rebalanceMcpSessionPercent ||
      payload.apiRebalanceEnabled !== settings.apiRebalanceEnabled ||
      payload.apiRebalancePercent !== settings.apiRebalancePercent ||
      payload.upstreamProjectIdMode !== settings.upstreamProjectIdMode ||
      payload.upstreamProjectIdFixedValue !== settings.upstreamProjectIdFixedValue ||
      payload.upstreamMcpUserAgent !== settings.upstreamMcpUserAgent ||
      payload.upstreamPreciseReconciliationEnabled !== settings.upstreamPreciseReconciliationEnabled ||
      payload.rechargeFeatureEnabled !== settings.rechargeFeatureEnabled ||
      payload.rechargeUserEnabled !== settings.rechargeUserEnabled ||
      payload.adminDefaultActiveUsersOnly !== settings.adminDefaultActiveUsersOnly ||
      payload.userBlockedKeyBaseLimit !== settings.userBlockedKeyBaseLimit ||
      payload.globalIpLimit !== settings.globalIpLimit ||
      JSON.stringify(payload.requestLogRetention) !== JSON.stringify(settings.requestLogRetention))

  const commitNormalSettings = (overrides: NormalSystemSettingsOverrides = {}): Promise<boolean> => {
    if (saving) return Promise.resolve(false)
    const payload = buildNormalSettingsPayload(overrides)
    if (!payload || !normalPayloadChanged(payload)) return Promise.resolve(false)
    return Promise.resolve()
      .then(() => onApply(payload))
      .then(
        () => true,
        () => false,
      )
  }

  const handleCommitKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== 'Enter') return
    event.preventDefault()
    event.currentTarget.blur()
  }

  const openClientIpDialog = () => {
    if (saving) return
    setDraftTrustedProxyCidrs(settings?.trustedProxyCidrs?.join('\n') ?? '')
    setDraftTrustedClientIpHeaders(settings?.trustedClientIpHeaders?.join('\n') ?? '')
    setClientIpDialogOpen(true)
  }

  const cancelClientIpDialog = () => {
    if (saving) return
    setDraftTrustedProxyCidrs(settings?.trustedProxyCidrs?.join('\n') ?? '')
    setDraftTrustedClientIpHeaders(settings?.trustedClientIpHeaders?.join('\n') ?? '')
    setClientIpDialogOpen(false)
  }

  const commitTrustedClientIpSettings = async () => {
    const normalPayload = buildNormalSettingsPayload()
    if (saving || !normalPayload || parsedTrustedClientIpHeaders.duplicateError != null) return
    const payload = {
      ...normalPayload,
      trustedProxyCidrs: normalizedTrustedProxyCidrs,
      trustedClientIpHeaders: normalizedTrustedClientIpHeaders,
    }
    if (!trustedClientIpChanged && !normalPayloadChanged(payload)) {
      setClientIpDialogOpen(false)
      return
    }
    try {
      await onApply(payload)
      setClientIpDialogOpen(false)
    } catch {
      // Parent state owns the visible save error; keep the dialog draft intact.
    }
  }
  const updateRetentionDraft = (
    updater: (current: RequestLogRetentionSettings) => RequestLogRetentionSettings,
  ): RequestLogRetentionSettings => {
    const next = updater(cloneRequestLogRetention(draftRequestLogRetention))
    setDraftRequestLogRetention(next)
    return next
  }
  const commitRetentionSettings = (next = draftRequestLogRetention): Promise<boolean> =>
    commitNormalSettings({ requestLogRetention: next })
  const setRetentionProfileField = (
    profileKey: 'global' | 'heavyUsage' | 'debugShared',
    field: keyof RequestLogRetentionProfile,
    value: number,
  ) =>
    updateRetentionDraft((current) => ({
      ...current,
      [profileKey]: {
        ...current[profileKey],
        [field]: value,
      },
    }))
  const retentionDaySlider = (
    label: string,
    profileKey: 'global' | 'heavyUsage' | 'debugShared',
    field: keyof RequestLogRetentionProfile,
  ) => {
    const value = draftRequestLogRetention[profileKey][field]
    return (
      <label className="grid gap-2 text-sm">
        <span className="font-medium">{label}</span>
        <div className="grid grid-cols-[minmax(0,1fr),64px] items-center gap-3">
          <input
            className="range"
            type="range"
            min={0}
            max={requestLogRetentionDayStops.length - 1}
            step={1}
            value={dayStopIndex(value)}
            disabled={saving}
            onChange={(event) => {
              const stop = requestLogRetentionDayStops[Number.parseInt(event.target.value, 10)] ?? 0
              setRetentionProfileField(profileKey, field, stop)
            }}
            onBlur={() => {
              void commitRetentionSettings()
            }}
          />
          <span className="text-right font-mono text-sm">{value}d</span>
        </div>
      </label>
    )
  }
  const observedClientIpRequestsSection = (
    <div className="grid gap-3 rounded-md border border-border/60 bg-muted/20 p-3 text-sm">
      <div className="grid gap-1">
        <span className="font-medium">{strings.form.observedClientIpTitle}</span>
        <p className="text-xs text-muted-foreground">{strings.form.observedClientIpDescription}</p>
      </div>
      {observedClientIpRequestsError ? (
        <p className="text-sm text-destructive">{observedClientIpRequestsError}</p>
      ) : observedHeaderColumns.length === 0 ? (
        <p className="text-sm text-muted-foreground">{strings.form.observedClientIpNoHeaders}</p>
      ) : observedClientIpRequests.length === 0 ? (
        <p className="text-sm text-muted-foreground">{strings.form.observedClientIpNoRequests}</p>
      ) : (
        <div className="max-h-[min(18rem,36dvh)] overflow-auto rounded-md border border-border bg-background">
          <table className="w-max min-w-full table-auto text-left text-sm">
            <thead className="bg-muted/50 text-sm text-muted-foreground">
              <tr>
                <th className="whitespace-nowrap px-4 py-3">{strings.form.observedClientIpRequestColumn}</th>
                {observedHeaderColumns.map((header) => (
                  <th key={header} className="whitespace-nowrap px-4 py-3 font-mono">
                    {header}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {observedClientIpRequests.map((item) => {
                const valuesByHeader = new Map(
                  item.ipHeaders.map((header) => [header.name.toLowerCase(), header.value]),
                )
                return (
                  <tr key={item.id} className="border-t border-border">
                    <td className="px-4 py-3 align-top whitespace-nowrap font-mono text-[13px] leading-6">
                      {new Date(item.createdAt * 1000).toLocaleString('zh-CN')}
                    </td>
                    {observedHeaderColumns.map((header) => (
                      <td
                        key={`${item.id}-${header}`}
                        className="whitespace-nowrap px-4 py-3 align-top font-mono text-[13px] leading-6"
                      >
                        {valuesByHeader.get(header) ?? '—'}
                      </td>
                    ))}
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
  const trustedClientIpPanel = (
    <section className="system-settings-trusted-panel">
      <div className="system-settings-trusted-copy">
        <h4>{strings.form.trustedClientIpTitle}</h4>
        <p>
          {settings?.trustedClientIpHeaders?.join(' -> ') ||
            'cf-connecting-ip -> true-client-ip -> x-real-ip -> x-forwarded-for -> forwarded'}
        </p>
      </div>
      <Dialog
        open={clientIpDialogOpen}
        onOpenChange={(open) => {
          if (open) openClientIpDialog()
        }}
      >
        <Button type="button" variant="outline" size="sm" disabled={saving} onClick={openClientIpDialog}>
          <Icon icon="mdi:shield-account-outline" width={16} height={16} aria-hidden="true" />
          {strings.form.trustedClientIpConfigure}
        </Button>
        <DialogContent
          hideCloseButton
          onEscapeKeyDown={(event) => event.preventDefault()}
          onPointerDownOutside={(event) => event.preventDefault()}
          className="grid max-h-[calc(100dvh-2rem)] w-[min(72rem,calc(100vw-2rem))] max-w-6xl grid-rows-[auto_minmax(0,1fr)_auto] gap-0 overflow-hidden p-0"
        >
          <DialogHeader className="px-6 pb-4 pr-12 pt-6">
            <DialogTitle>{strings.form.trustedClientIpTitle}</DialogTitle>
            <DialogDescription>
              {strings.form.trustedClientIpDialogDescription}
            </DialogDescription>
          </DialogHeader>
          <div className="grid min-h-0 gap-4 overflow-y-auto px-6 pb-4">
            <div className="grid gap-2 text-sm">
              <label className="flex flex-col gap-2">
                <span className="font-medium">{strings.form.trustedProxyCidrs}</span>
                <textarea
                  rows={4}
                  className="resize-y rounded-md border border-input bg-background px-3 py-2 text-sm leading-6"
                  value={draftTrustedProxyCidrs}
                  disabled={saving}
                  onChange={(event) => setDraftTrustedProxyCidrs(event.target.value)}
                />
              </label>

              <div className="grid gap-1">
                <span className="font-medium">{strings.form.trustedClientIpHeaderOrder}</span>
                <p className="text-xs text-muted-foreground">{strings.form.trustedClientIpHeaderOrderHint}</p>
              </div>
              <div className="grid gap-3">
                <div className="flex flex-wrap items-center gap-2 rounded-md border border-border/60 bg-muted/10 p-2">
                  {clientIpHeaderPresets.map((preset) => (
                    <div
                      key={preset.group}
                      className="inline-flex max-w-full flex-wrap items-center gap-1.5 rounded-md border border-border/50 bg-background/40 px-2 py-1.5"
                    >
                      <span
                        className="mr-0.5 shrink-0 text-xs font-medium text-muted-foreground"
                        title={'note' in preset ? preset.note : undefined}
                      >
                        {preset.group}
                      </span>
                      {preset.headers.map((header) =>
                        (() => {
                          const selected = normalizedTrustedClientIpHeaders.includes(header)
                          return (
                            <Button
                              key={`${preset.group}-${header}`}
                              type="button"
                              variant="outline"
                              size="xs"
                              aria-pressed={selected}
                              disabled={saving}
                              className={
                                selected
                                  ? 'border-primary/65 bg-primary/10 text-primary hover:bg-primary/15'
                                  : undefined
                              }
                              onClick={() =>
                                setDraftTrustedClientIpHeaders((current) => toggleOrderedHeaderDraft(current, header))
                              }
                            >
                              <Icon
                                icon={selected ? 'mdi:check' : 'mdi:plus'}
                                width={14}
                                height={14}
                                aria-hidden="true"
                              />
                              {header}
                            </Button>
                          )
                        })(),
                      )}
                    </div>
                  ))}
                </div>
              </div>
              <textarea
                className="h-28 resize-y rounded-md border border-input bg-background px-3 py-2 text-sm"
                value={draftTrustedClientIpHeaders}
                disabled={saving}
                onChange={(event) => setDraftTrustedClientIpHeaders(event.target.value)}
              />
            </div>
            {observedClientIpRequestsSection}
            {(parsedTrustedClientIpHeaders.duplicateError || (clientIpDialogOpen && error) || saving) && (
              <p
                className="text-sm font-medium"
                role="status"
                aria-live="polite"
                style={{
                  color: parsedTrustedClientIpHeaders.duplicateError || error ? 'hsl(var(--destructive))' : undefined,
                }}
              >
                {parsedTrustedClientIpHeaders.duplicateError ??
                  (clientIpDialogOpen ? error : null) ??
                  strings.actions.applying}
              </p>
            )}
          </div>
          <DialogFooter className="border-t border-border/60 px-6 py-4">
            <Button type="button" variant="outline" disabled={saving} onClick={cancelClientIpDialog}>
              {strings.actions.cancel}
            </Button>
            <Button
              type="button"
              disabled={
                saving ||
                parsedTrustedClientIpHeaders.duplicateError != null ||
                parsedRequestRateLimit == null ||
                parsedCount == null ||
                parsedBlockedKeyBaseLimit == null ||
                parsedGlobalIpLimit == null ||
                !trustedClientIpChanged
              }
              onClick={() => {
                void commitTrustedClientIpSettings()
              }}
            >
              <Icon
                icon={saving ? 'mdi:loading' : 'mdi:check-circle-outline'}
                width={16}
                height={16}
                className={saving ? 'icon-spin' : undefined}
                aria-hidden="true"
              />
              {saving ? strings.actions.applying : strings.actions.apply}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  )

  return (
    <section className="surface panel system-settings-shell">
      <AdminLoadingRegion
        loadState={loadState}
        loadingLabel={strings.description}
        errorLabel={error ?? undefined}
        minHeight={260}
      >
        <div className="system-settings-form-layout" aria-label={strings.form.title}>
          <section className="system-settings-config-section">
            <h4>{strings.form.accessDisplayTitle}</h4>
            <div className="system-settings-field-grid">
              {registrationPolicy && (
                <div className="system-settings-action-row" aria-labelledby="system-settings-registration-title">
                  <div className="system-settings-toggle-copy">
                    <span className="system-settings-setting-title" id="system-settings-registration-title">
                      {registrationPolicy.strings.title}
                    </span>
                    <p
                      role="status"
                      aria-live="polite"
                      style={{ color: registrationPolicy.error ? 'hsl(var(--destructive))' : undefined }}
                    >
                      {registrationPolicy.error ?? registrationPolicy.statusText}
                    </p>
                  </div>
                  <Switch
                    checked={registrationPolicy.checked ?? false}
                    disabled={registrationPolicy.disabled || registrationPolicy.checked === null}
                    aria-label={registrationPolicy.strings.title}
                    onCheckedChange={() => void registrationPolicy.onToggle()}
                  />
                </div>
              )}

              <div className="system-settings-action-row" aria-labelledby="system-settings-recharge-feature-title">
                <div className="system-settings-toggle-copy">
                  <span className="system-settings-setting-title" id="system-settings-recharge-feature-title">
                    {strings.form.rechargeFeatureLabel}
                  </span>
                  <p>{strings.form.rechargeFeatureHint}</p>
                </div>
                <Switch
                  checked={draftRechargeFeatureEnabled}
                  disabled={saving}
                  aria-label={strings.form.rechargeFeatureLabel}
                  onCheckedChange={(checked) => {
                    setDraftRechargeFeatureEnabled(checked)
                    void commitNormalSettings({
                      rechargeFeatureEnabled: checked,
                    }).then((saved) => {
                      if (!saved) setDraftRechargeFeatureEnabled(settings?.rechargeFeatureEnabled ?? true)
                    })
                  }}
                />
              </div>

              <div className="system-settings-action-row" aria-labelledby="system-settings-recharge-user-title">
                <div className="system-settings-toggle-copy">
                  <span className="system-settings-setting-title" id="system-settings-recharge-user-title">
                    {strings.form.rechargeUserLabel}
                  </span>
                  <p>{strings.form.rechargeUserHint}</p>
                </div>
                <Switch
                  checked={draftRechargeUserEnabled}
                  disabled={saving}
                  aria-label={strings.form.rechargeUserLabel}
                  onCheckedChange={(checked) => {
                    setDraftRechargeUserEnabled(checked)
                    void commitNormalSettings({
                      rechargeUserEnabled: checked,
                    }).then((saved) => {
                      if (!saved) setDraftRechargeUserEnabled(settings?.rechargeUserEnabled ?? true)
                    })
                  }}
                />
              </div>

              <div className="system-settings-action-row" aria-labelledby="system-settings-active-users-default-title">
                <div className="system-settings-toggle-copy">
                  <span
                    className="system-settings-setting-title"
                    id="system-settings-active-users-default-title"
                  >
                    {strings.form.activeUsersDefaultLabel}
                  </span>
                  <p>{strings.form.activeUsersDefaultHint}</p>
                  {userListStats && (
                    <p className="text-xs text-muted-foreground">
                      {strings.form.activeUsersDefaultCount
                        .replace('{active}', String(userListStats.activeUsers90d))
                        .replace('{total}', String(userListStats.totalUsers))}
                    </p>
                  )}
                  {userListStats && (
                    <p className="text-xs text-muted-foreground">
                      {strings.form.activeUsersDefinition.replace('{days}', String(userListStats.windowDays))}
                    </p>
                  )}
                </div>
                <Switch
                  checked={draftAdminDefaultActiveUsersOnly}
                  disabled={saving}
                  aria-label={strings.form.activeUsersDefaultLabel}
                  onCheckedChange={(checked) => {
                    setDraftAdminDefaultActiveUsersOnly(checked)
                    void commitNormalSettings({
                      adminDefaultActiveUsersOnly: checked,
                    }).then((saved) => {
                      if (!saved) {
                        setDraftAdminDefaultActiveUsersOnly(
                          settings?.adminDefaultActiveUsersOnly ?? false,
                        )
                      }
                    })
                  }}
                />
              </div>

              <div className="system-settings-action-row" aria-labelledby="system-settings-density-title">
                <div className="system-settings-toggle-copy">
                  <span className="system-settings-setting-title" id="system-settings-density-title">
                    {strings.form.displayDensityTitle}
                  </span>
                  <p>{strings.form.displayDensityStoredHint}</p>
                </div>
                <div className="system-settings-density-actions" role="group" aria-label={strings.form.displayDensityTitle}>
                  <Button
                    type="button"
                    variant={displayDensity === 'comfortable' ? 'default' : 'outline'}
                    size="sm"
                    aria-pressed={displayDensity === 'comfortable'}
                    onClick={() => onDisplayDensityChange('comfortable')}
                  >
                    {strings.form.displayDensityComfortable}
                  </Button>
                  <Button
                    type="button"
                    variant={displayDensity === 'compact' ? 'default' : 'outline'}
                    size="sm"
                    aria-pressed={displayDensity === 'compact'}
                    onClick={() => onDisplayDensityChange('compact')}
                  >
                    {strings.form.displayDensityCompact}
                  </Button>
                </div>
              </div>
            </div>
          </section>

          <section className="system-settings-config-section">
            <h4>{strings.form.limitsTitle}</h4>
            <div className="system-settings-field-grid system-settings-field-grid--limits">
              <div className="system-settings-field">
                <label className="text-sm font-medium" htmlFor="system-settings-request-rate-limit">
                  {strings.form.requestRateLimitLabel}
                </label>
                <Input
                  id="system-settings-request-rate-limit"
                  type="number"
                  inputMode="numeric"
                  min={1}
                  step={1}
                  value={draftRequestRateLimit}
                  disabled={saving}
                  onChange={(event) => setDraftRequestRateLimit(event.target.value)}
                  onBlur={() => {
                    void commitNormalSettings()
                  }}
                  onKeyDown={handleCommitKeyDown}
                  aria-invalid={fieldErrors.requestRateLimit ? true : undefined}
                  aria-describedby={fieldErrors.requestRateLimit ? requestRateLimitErrorId : undefined}
                />
                {fieldErrors.requestRateLimit && (
                  <p id={requestRateLimitErrorId} className="system-settings-field-error text-xs font-medium text-destructive">
                    {fieldErrors.requestRateLimit}
                  </p>
                )}
                <p className="system-settings-field-hint text-xs text-muted-foreground">
                  {strings.form.requestRateLimitHint}
                </p>
              </div>
              <div className="system-settings-field">
                <label className="text-sm font-medium" htmlFor="system-settings-blocked-key-base-limit">
                  {strings.form.blockedKeyBaseLimitLabel}
                </label>
                <Input
                  id="system-settings-blocked-key-base-limit"
                  type="number"
                  inputMode="numeric"
                  min={0}
                  step={1}
                  value={draftBlockedKeyBaseLimit}
                  disabled={saving}
                  onChange={(event) => setDraftBlockedKeyBaseLimit(event.target.value)}
                  onBlur={() => {
                    void commitNormalSettings()
                  }}
                  onKeyDown={handleCommitKeyDown}
                  aria-invalid={fieldErrors.blockedKeyBaseLimit ? true : undefined}
                  aria-describedby={fieldErrors.blockedKeyBaseLimit ? blockedKeyBaseLimitErrorId : undefined}
                />
                {fieldErrors.blockedKeyBaseLimit && (
                  <p id={blockedKeyBaseLimitErrorId} className="system-settings-field-error text-xs font-medium text-destructive">
                    {fieldErrors.blockedKeyBaseLimit}
                  </p>
                )}
                <p className="system-settings-field-hint text-xs text-muted-foreground">
                  {strings.form.blockedKeyBaseLimitHint}
                </p>
              </div>

              <div className="system-settings-field">
                <label className="text-sm font-medium" htmlFor="system-settings-auth-token-log-retention-days">
                  {strings.form.authTokenLogRetentionDaysLabel}
                </label>
                <div className="system-settings-range-control grid gap-3 md:grid-cols-[minmax(0,1fr),64px] md:items-center">
                  <input
                    id="system-settings-auth-token-log-retention-days"
                    className="range"
                    type="range"
                    min={0}
                    max={authTokenLogRetentionDayStops.length - 1}
                    step={1}
                    value={authTokenLogRetentionDayStopIndex(draftAuthTokenLogRetentionDays)}
                    disabled={saving}
                    onChange={(event) => {
                      const retentionDays =
                        authTokenLogRetentionDayStops[Number.parseInt(event.target.value, 10)] ?? 92
                      setDraftAuthTokenLogRetentionDays(retentionDays)
                    }}
                    onBlur={() => {
                      void commitNormalSettings({
                        authTokenLogRetentionDays: draftAuthTokenLogRetentionDays,
                      })
                    }}
                  />
                  <span className="text-right font-mono text-sm">{draftAuthTokenLogRetentionDays}d</span>
                </div>
                <p className="system-settings-field-hint text-xs text-muted-foreground">
                  {strings.form.authTokenLogRetentionDaysHint}
                </p>
              </div>

              <div className="system-settings-field">
                <label className="text-sm font-medium" htmlFor="system-settings-global-ip-limit">
                  {strings.form.globalIpLimitLabel}
                </label>
                <Input
                  id="system-settings-global-ip-limit"
                  type="number"
                  inputMode="numeric"
                  min={0}
                  step={1}
                  value={draftGlobalIpLimit}
                  disabled={saving}
                  onChange={(event) => setDraftGlobalIpLimit(event.target.value)}
                  onBlur={() => {
                    void commitNormalSettings()
                  }}
                  onKeyDown={handleCommitKeyDown}
                  aria-invalid={fieldErrors.globalIpLimit ? true : undefined}
                  aria-describedby={fieldErrors.globalIpLimit ? globalIpLimitErrorId : undefined}
                />
                {fieldErrors.globalIpLimit && (
                  <p id={globalIpLimitErrorId} className="system-settings-field-error text-xs font-medium text-destructive">
                    {fieldErrors.globalIpLimit}
                  </p>
                )}
                <p className="system-settings-field-hint text-xs text-muted-foreground">{strings.form.globalIpLimitHint}</p>
              </div>
            </div>
          </section>

          <section
            className="system-settings-config-section system-settings-retention-section"
            data-testid="request-log-retention-settings"
          >
            <h4>调用日志保留</h4>
            <div className="system-settings-retention-content">
              <div className="system-settings-field-grid system-settings-field-grid--limits">
                <div className="system-settings-field">
                  <label className="text-sm font-medium" htmlFor="system-settings-request-log-max-days">
                    最大日志行保留
                  </label>
                  <div className="system-settings-range-control grid gap-3 md:grid-cols-[minmax(0,1fr),64px] md:items-center">
                    <input
                      id="system-settings-request-log-max-days"
                      className="range"
                      type="range"
                      min={0}
                      max={requestLogRetentionDayStops.length - 1}
                      step={1}
                      value={dayStopIndex(draftRequestLogRetention.maxLogRetentionDays)}
                      disabled={saving}
                      onChange={(event) => {
                        const maxLogRetentionDays =
                          requestLogRetentionDayStops[Number.parseInt(event.target.value, 10)] ?? 32
                        const next = updateRetentionDraft((current) => ({
                          ...current,
                          maxLogRetentionDays,
                        }))
                        if (maxLogRetentionDays < draftRequestLogRetention.maxLogRetentionDays) {
                          setDraftRequestLogRetention(cloneRequestLogRetention(next))
                        }
                      }}
                      onBlur={() => {
                        void commitRetentionSettings()
                      }}
                    />
                    <span className="text-right font-mono text-sm">{draftRequestLogRetention.maxLogRetentionDays}d</span>
                  </div>
                  <p className="system-settings-field-hint text-xs text-muted-foreground">
                    到期日志行会由 request_logs_gc 分批删除，body 会先按下方策略清空。
                  </p>
                </div>

                <div className="system-settings-field">
                  <label className="text-sm font-medium" htmlFor="system-settings-heavy-usage-threshold">
                    高频用户阈值
                  </label>
                  <div className="system-settings-range-control grid gap-3 md:grid-cols-[minmax(0,1fr),64px] md:items-center">
                    <input
                      id="system-settings-heavy-usage-threshold"
                      className="range"
                      type="range"
                      min={50}
                      max={150}
                      step={10}
                      value={draftRequestLogRetention.heavyUsageThresholdPercent}
                      disabled={saving}
                      onChange={(event) => {
                        const heavyUsageThresholdPercent = Number.parseInt(event.target.value, 10)
                        updateRetentionDraft((current) => ({ ...current, heavyUsageThresholdPercent }))
                      }}
                      onBlur={() => {
                        void commitRetentionSettings()
                      }}
                    />
                    <span className="text-right font-mono text-sm">
                      {draftRequestLogRetention.heavyUsageThresholdPercent}%
                    </span>
                  </div>
                  <p className="system-settings-field-hint text-xs text-muted-foreground">
                    最近 24 小时额度使用比例达到该阈值时，使用高频用户 body 保留策略。
                  </p>
                </div>
              </div>

              <div className="system-settings-retention-profiles">
                <div className="system-settings-retention-card">
                  <h5 className="text-sm font-semibold">全局默认</h5>
                  {retentionDaySlider('业务 body', 'global', 'businessBodyDays')}
                  {retentionDaySlider('非业务 body', 'global', 'nonBusinessBodyDays')}
                  {retentionDaySlider('非成功 body', 'global', 'nonSuccessBodyDays')}
                </div>
                <div className="system-settings-retention-card">
                  <h5 className="text-sm font-semibold">高频调用用户</h5>
                  {retentionDaySlider('业务 body', 'heavyUsage', 'businessBodyDays')}
                  {retentionDaySlider('非业务 body', 'heavyUsage', 'nonBusinessBodyDays')}
                  {retentionDaySlider('非成功 body', 'heavyUsage', 'nonSuccessBodyDays')}
                </div>
                <div className="system-settings-retention-card">
                  <h5 className="text-sm font-semibold">共享调试用户</h5>
                  {retentionDaySlider('业务 body', 'debugShared', 'businessBodyDays')}
                  {retentionDaySlider('非业务 body', 'debugShared', 'nonBusinessBodyDays')}
                  {retentionDaySlider('非成功 body', 'debugShared', 'nonSuccessBodyDays')}
                </div>
              </div>
            </div>
          </section>

          <section className="system-settings-config-section">
            <h4>{strings.form.gatewaySectionTitle}</h4>
            <div className="system-settings-field-grid system-settings-field-grid--gateway">
              <div className="system-settings-field">
                <div className="system-settings-field-label-row">
                  <label className="text-sm font-medium" htmlFor="system-settings-affinity-count">
                    {strings.form.countLabel}
                  </label>
                  <SystemSettingsHelpBubble strings={strings} open={helpBubbleOpen} />
                </div>
                <Input
                  id="system-settings-affinity-count"
                  type="number"
                  inputMode="numeric"
                  min={1}
                  max={1000}
                  step={1}
                  value={draftCount}
                  disabled={saving}
                  onChange={(event) => setDraftCount(event.target.value)}
                  onBlur={() => {
                    void commitNormalSettings()
                  }}
                  onKeyDown={handleCommitKeyDown}
                  aria-invalid={fieldErrors.count ? true : undefined}
                  aria-describedby={fieldErrors.count ? affinityCountErrorId : undefined}
                />
                {fieldErrors.count && (
                  <p id={affinityCountErrorId} className="system-settings-field-error text-xs font-medium text-destructive">
                    {fieldErrors.count}
                  </p>
                )}
              </div>

              <div className="system-settings-toggle-row">
                <div className="system-settings-toggle-copy">
                  <div className="system-settings-field-label-row">
                    <label className="text-sm font-medium" htmlFor="system-settings-rebalance-switch">
                      {strings.form.rebalanceLabel}
                    </label>
                    {activeUpstreamMcpSessions > 0 && onOpenMcpSessionBindings ? (
                      <TooltipProvider>
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button
                              type="button"
                              variant="ghost"
                              size="xs"
                              className="h-7 w-7 rounded-full px-0 text-amber-600 hover:text-amber-700"
                              aria-label="upstream_mcp session"
                              onClick={onOpenMcpSessionBindings}
                            >
                              <Icon icon="mdi:alert-circle-outline" width={16} height={16} aria-hidden="true" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent side="top">
                            <span>upstream_mcp session · {activeUpstreamMcpSessions}</span>
                          </TooltipContent>
                        </Tooltip>
                      </TooltipProvider>
                    ) : null}
                  </div>
                  <p className="text-xs text-muted-foreground">{strings.form.rebalanceHint}</p>
                </div>
                <Switch
                  aria-label={strings.form.rebalanceLabel}
                  id="system-settings-rebalance-switch"
                  checked={draftRebalanceEnabled}
                  onCheckedChange={(checked) => {
                    setDraftRebalanceEnabled(checked)
                    void commitNormalSettings({
                      rebalanceMcpEnabled: checked,
                    }).then((saved) => {
                      if (!saved) setDraftRebalanceEnabled(settings?.rebalanceMcpEnabled ?? false)
                    })
                  }}
                  disabled={saving}
                />
              </div>

              <div className="system-settings-toggle-row">
                <div className="system-settings-toggle-copy">
                  <label className="text-sm font-medium" htmlFor="system-settings-api-rebalance-switch">
                    {strings.form.apiRebalanceLabel}
                  </label>
                  <p className="text-xs text-muted-foreground">{strings.form.apiRebalanceHint}</p>
                </div>
                <Switch
                  aria-label={strings.form.apiRebalanceLabel}
                  id="system-settings-api-rebalance-switch"
                  checked={draftApiRebalanceEnabled}
                  onCheckedChange={(checked) => {
                    setDraftApiRebalanceEnabled(checked)
                    void commitNormalSettings({
                      apiRebalanceEnabled: checked,
                    }).then((saved) => {
                      if (!saved) setDraftApiRebalanceEnabled(settings?.apiRebalanceEnabled ?? false)
                    })
                  }}
                  disabled={saving}
                />
              </div>
            </div>
          </section>

          <section className="system-settings-config-section">
            <h4>{strings.form.upstreamIdentityTitle}</h4>
            <div className="system-settings-field-grid system-settings-field-grid--api">
              <div className="system-settings-field">
                <label className="text-sm font-medium" htmlFor="system-settings-upstream-project-id-mode">
                  {strings.form.upstreamProjectIdModeLabel}
                </label>
                <Select
                  value={draftUpstreamProjectIdMode}
                  disabled={saving}
                  onValueChange={(value) => {
                    const nextMode = value as UpstreamProjectIdMode
                    setDraftUpstreamProjectIdMode(nextMode)
                    void commitNormalSettings({
                      upstreamProjectIdMode: nextMode,
                    }).then((saved) => {
                      if (!saved) {
                        setDraftUpstreamProjectIdMode(settings?.upstreamProjectIdMode ?? nextMode)
                      }
                    })
                  }}
                >
                  <SelectTrigger
                    id="system-settings-upstream-project-id-mode"
                    aria-label={strings.form.upstreamProjectIdModeLabel}
                    className="system-settings-select-trigger"
                  >
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent align="end" className="system-settings-select-content">
                    <SelectItem value="accessToken">{strings.form.upstreamProjectIdModeAccessToken}</SelectItem>
                    <SelectItem value="passthrough">{strings.form.upstreamProjectIdModePassthrough}</SelectItem>
                    <SelectItem value="fixed">{strings.form.upstreamProjectIdModeFixed}</SelectItem>
                  </SelectContent>
                </Select>
                <p className="system-settings-field-hint text-xs text-muted-foreground">
                  {strings.form.upstreamProjectIdModeHint}
                </p>
              </div>

              <div className="system-settings-field">
                <label className="text-sm font-medium" htmlFor="system-settings-upstream-project-id-fixed-value">
                  {strings.form.upstreamProjectIdFixedValueLabel}
                </label>
                <Input
                  id="system-settings-upstream-project-id-fixed-value"
                  type="text"
                  value={draftUpstreamProjectIdFixedValue}
                  placeholder={strings.form.upstreamProjectIdFixedValuePlaceholder}
                  disabled={saving}
                  onChange={(event) => setDraftUpstreamProjectIdFixedValue(event.target.value)}
                  onBlur={(event) => {
                    void commitNormalSettings({
                      upstreamProjectIdFixedValue: event.currentTarget.value.trim(),
                    })
                  }}
                  onKeyDown={handleCommitKeyDown}
                  aria-invalid={fieldErrors.upstreamProjectIdFixedValue ? true : undefined}
                  aria-describedby={
                    fieldErrors.upstreamProjectIdFixedValue ? upstreamProjectIdFixedValueErrorId : undefined
                  }
                />
                {fieldErrors.upstreamProjectIdFixedValue && (
                  <p
                    id={upstreamProjectIdFixedValueErrorId}
                    className="system-settings-field-error text-xs font-medium text-destructive"
                  >
                    {fieldErrors.upstreamProjectIdFixedValue}
                  </p>
                )}
                <p className="system-settings-field-hint text-xs text-muted-foreground">
                  {strings.form.upstreamProjectIdFixedValueHint}
                </p>
              </div>

              <div className="system-settings-field">
                <label className="text-sm font-medium" htmlFor="system-settings-upstream-mcp-user-agent">
                  {strings.form.upstreamMcpUserAgentLabel}
                </label>
                <Input
                  id="system-settings-upstream-mcp-user-agent"
                  type="text"
                  value={draftUpstreamMcpUserAgent}
                  placeholder={strings.form.upstreamMcpUserAgentPlaceholder}
                  disabled={saving}
                  onChange={(event) => setDraftUpstreamMcpUserAgent(event.target.value)}
                  onBlur={(event) => {
                    void commitNormalSettings({
                      upstreamMcpUserAgent: event.currentTarget.value.trim(),
                    })
                  }}
                  onKeyDown={handleCommitKeyDown}
                  aria-invalid={fieldErrors.upstreamMcpUserAgent ? true : undefined}
                  aria-describedby={fieldErrors.upstreamMcpUserAgent ? upstreamMcpUserAgentErrorId : undefined}
                />
                {fieldErrors.upstreamMcpUserAgent && (
                  <p
                    id={upstreamMcpUserAgentErrorId}
                    className="system-settings-field-error text-xs font-medium text-destructive"
                  >
                    {fieldErrors.upstreamMcpUserAgent}
                  </p>
                )}
                <p className="system-settings-field-hint text-xs text-muted-foreground">
                  {strings.form.upstreamMcpUserAgentHint}
                </p>
              </div>

              <div className="system-settings-field system-settings-field--notice">
                <div className="system-settings-field-copy">
                  <strong className="text-sm font-medium">{strings.form.upstreamHttpUserAgentNotice}</strong>
                </div>
              </div>
            </div>
          </section>

          <section className="system-settings-config-section">
            <h4>{strings.form.upstreamPreciseReconciliationTitle}</h4>
            <div className="system-settings-field-grid system-settings-field-grid--api">
              <div className="system-settings-toggle-row">
                <div className="system-settings-toggle-copy">
                  <label className="text-sm font-medium" htmlFor="system-settings-upstream-precise-reconciliation-switch">
                    {strings.form.upstreamPreciseReconciliationLabel}
                  </label>
                  <p className="text-xs text-muted-foreground">{strings.form.upstreamPreciseReconciliationHint}</p>
                </div>
                <Switch
                  aria-label={strings.form.upstreamPreciseReconciliationLabel}
                  id="system-settings-upstream-precise-reconciliation-switch"
                  checked={draftUpstreamPreciseReconciliationEnabled}
                  onCheckedChange={(checked) => {
                    setDraftUpstreamPreciseReconciliationEnabled(checked)
                    void commitNormalSettings({
                      upstreamPreciseReconciliationEnabled: checked,
                    }).then((saved) => {
                        if (!saved) {
                          setDraftUpstreamPreciseReconciliationEnabled(
                          settings?.upstreamPreciseReconciliationEnabled ?? false,
                          )
                        }
                      })
                  }}
                  disabled={saving}
                />
              </div>
            </div>
          </section>

          {trustedClientIpPanel}

          {(error || saving) && (
            <p
              className="system-settings-inline-status text-sm font-medium"
              role="status"
              aria-live="polite"
              style={{ color: error ? 'hsl(var(--destructive))' : undefined }}
            >
              {error ?? strings.actions.applying}
            </p>
          )}

          {changed && !inlineError && !saving && (
            <p className="system-settings-inline-status text-xs text-muted-foreground">{strings.form.autosaveHint}</p>
          )}
        </div>
      </AdminLoadingRegion>
    </section>
  )
}
