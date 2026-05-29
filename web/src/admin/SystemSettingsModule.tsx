import { useEffect, useState, type KeyboardEvent } from 'react'

import {
  confirmAdminTotp,
  createAdminTotpSetup,
  disableAdminTotp,
  fetchAdminTotpStatus,
  resetAdminTotp,
  fetchObservedClientIpRequests,
  type AdminTotpSetup,
  type AdminTotpStatus,
  type ObservedClientIpRequest,
  type SystemSettings,
} from '../api'
import type { QueryLoadState } from './queryLoadState'
import type { AdminTranslations } from '../i18n'
import type { AdminDisplayDensity } from './displayDensity'
import AdminLoadingRegion from '../components/AdminLoadingRegion'
import { Icon } from '../lib/icons'
import { Button } from '../components/ui/button'
import { Input } from '../components/ui/input'
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
  registrationPolicy?: {
    strings: AdminTranslations['users']['registration']
    checked: boolean | null
    disabled: boolean
    statusText: string
    error: string | null
    onToggle: () => Promise<void> | void
  }
  onDisplayDensityChange?: (density: AdminDisplayDensity) => void
  onApply: (settings: SystemSettings) => Promise<void> | void
}

type NormalSystemSettingsOverrides = Partial<
  Pick<
    SystemSettings,
    | 'requestRateLimit'
    | 'mcpSessionAffinityKeyCount'
    | 'rebalanceMcpEnabled'
    | 'rebalanceMcpSessionPercent'
    | 'apiRebalanceEnabled'
    | 'apiRebalancePercent'
    | 'rechargeFeatureEnabled'
    | 'rechargeUserEnabled'
    | 'userBlockedKeyBaseLimit'
    | 'globalIpLimit'
  >
>

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
            <p>{strings.form.percentHint}</p>
            <p>{strings.form.apiRebalanceHint}</p>
            <p>{strings.form.apiRebalancePercentHint}</p>
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
  registrationPolicy,
  onDisplayDensityChange = () => {},
  onApply,
}: SystemSettingsModuleProps): JSX.Element {
  const [draftRequestRateLimit, setDraftRequestRateLimit] = useState(() =>
    settings ? String(settings.requestRateLimit) : '100',
  )
  const [draftCount, setDraftCount] = useState(() => (settings ? String(settings.mcpSessionAffinityKeyCount) : ''))
  const [draftRebalanceEnabled, setDraftRebalanceEnabled] = useState(settings?.rebalanceMcpEnabled ?? false)
  const [draftPercent, setDraftPercent] = useState(() =>
    settings ? String(settings.rebalanceMcpSessionPercent) : '100',
  )
  const [draftApiRebalanceEnabled, setDraftApiRebalanceEnabled] = useState(settings?.apiRebalanceEnabled ?? false)
  const [draftApiRebalancePercent, setDraftApiRebalancePercent] = useState(() =>
    settings ? String(settings.apiRebalancePercent) : '0',
  )
  const [draftRechargeFeatureEnabled, setDraftRechargeFeatureEnabled] = useState(
    settings?.rechargeFeatureEnabled ?? true,
  )
  const [draftRechargeUserEnabled, setDraftRechargeUserEnabled] = useState(settings?.rechargeUserEnabled ?? true)
  const [draftBlockedKeyBaseLimit, setDraftBlockedKeyBaseLimit] = useState(() =>
    settings ? String(settings.userBlockedKeyBaseLimit) : '5',
  )
  const [draftGlobalIpLimit, setDraftGlobalIpLimit] = useState(() => (settings ? String(settings.globalIpLimit) : '5'))
  const [clientIpDialogOpen, setClientIpDialogOpen] = useState(false)
  const [draftTrustedProxyCidrs, setDraftTrustedProxyCidrs] = useState(
    () => settings?.trustedProxyCidrs?.join('\n') ?? '',
  )
  const [draftTrustedClientIpHeaders, setDraftTrustedClientIpHeaders] = useState(
    () => settings?.trustedClientIpHeaders?.join('\n') ?? '',
  )
  const [observedClientIpRequests, setObservedClientIpRequests] = useState<ObservedClientIpRequest[]>([])
  const [observedClientIpRequestsError, setObservedClientIpRequestsError] = useState<string | null>(null)
  const [totpStatus, setTotpStatus] = useState<AdminTotpStatus | null>(null)
  const [totpSetup, setTotpSetup] = useState<AdminTotpSetup | null>(null)
  const [totpCode, setTotpCode] = useState('')
  const [totpCurrentCode, setTotpCurrentCode] = useState('')
  const [totpBusy, setTotpBusy] = useState(false)
  const [totpError, setTotpError] = useState<string | null>(null)

  useEffect(() => {
    setDraftRequestRateLimit(settings ? String(settings.requestRateLimit) : '100')
    setDraftCount(settings ? String(settings.mcpSessionAffinityKeyCount) : '')
    setDraftRebalanceEnabled(settings?.rebalanceMcpEnabled ?? false)
    setDraftPercent(settings ? String(settings.rebalanceMcpSessionPercent) : '100')
    setDraftApiRebalanceEnabled(settings?.apiRebalanceEnabled ?? false)
    setDraftApiRebalancePercent(settings ? String(settings.apiRebalancePercent) : '0')
    setDraftRechargeFeatureEnabled(settings?.rechargeFeatureEnabled ?? true)
    setDraftRechargeUserEnabled(settings?.rechargeUserEnabled ?? true)
    setDraftBlockedKeyBaseLimit(settings ? String(settings.userBlockedKeyBaseLimit) : '5')
    setDraftGlobalIpLimit(settings ? String(settings.globalIpLimit) : '5')
    if (!clientIpDialogOpen) {
      setDraftTrustedProxyCidrs(settings?.trustedProxyCidrs?.join('\n') ?? '')
      setDraftTrustedClientIpHeaders(settings?.trustedClientIpHeaders?.join('\n') ?? '')
    }
  }, [
    settings?.requestRateLimit,
    settings?.mcpSessionAffinityKeyCount,
    settings?.rebalanceMcpEnabled,
    settings?.rebalanceMcpSessionPercent,
    settings?.apiRebalanceEnabled,
    settings?.apiRebalancePercent,
    settings?.rechargeFeatureEnabled,
    settings?.rechargeUserEnabled,
    settings?.userBlockedKeyBaseLimit,
    settings?.globalIpLimit,
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

  useEffect(() => {
    if (!settings?.rechargeFeatureEnabled) {
      setTotpStatus(null)
      setTotpSetup(null)
      setTotpError(null)
      return
    }
    const controller = new AbortController()
    fetchAdminTotpStatus(controller.signal)
      .then(setTotpStatus)
      .catch((err: unknown) => {
        if (!controller.signal.aborted) setTotpError(err instanceof Error ? err.message : String(err))
      })
    return () => controller.abort()
  }, [settings?.rechargeFeatureEnabled])

  const beginTotpSetup = async () => {
    setTotpBusy(true)
    setTotpError(null)
    try {
      const setup = await createAdminTotpSetup()
      setTotpSetup(setup)
      setTotpCode('')
      setTotpCurrentCode('')
    } catch (err) {
      setTotpError(err instanceof Error ? err.message : String(err))
    } finally {
      setTotpBusy(false)
    }
  }

  const confirmTotpSetup = async () => {
    if (!totpSetup) return
    setTotpBusy(true)
    setTotpError(null)
    try {
      const status = totpStatus?.enabled
        ? await resetAdminTotp(totpCurrentCode, totpSetup.secret, totpCode)
        : await confirmAdminTotp(totpSetup.secret, totpCode)
      setTotpStatus(status)
      setTotpSetup(null)
      setTotpCode('')
      setTotpCurrentCode('')
    } catch (err) {
      setTotpError(err instanceof Error ? err.message : String(err))
    } finally {
      setTotpBusy(false)
    }
  }

  const disableTotpBinding = async () => {
    setTotpBusy(true)
    setTotpError(null)
    try {
      const status = await disableAdminTotp(totpCurrentCode || totpCode)
      setTotpStatus(status)
      setTotpSetup(null)
      setTotpCode('')
      setTotpCurrentCode('')
    } catch (err) {
      setTotpError(err instanceof Error ? err.message : String(err))
    } finally {
      setTotpBusy(false)
    }
  }

  const normalizedRequestRateLimit = draftRequestRateLimit.trim()
  const normalizedCount = draftCount.trim()
  const normalizedPercent = draftPercent.trim()
  const normalizedApiRebalancePercent = draftApiRebalancePercent.trim()
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
  const parsedPercent = isValidPercentDraft(normalizedPercent) ? Number.parseInt(normalizedPercent, 10) : null
  const parsedApiRebalancePercent = isValidPercentDraft(normalizedApiRebalancePercent)
    ? Number.parseInt(normalizedApiRebalancePercent, 10)
    : null
  const parsedBlockedKeyBaseLimit = isValidNonNegativeIntegerDraft(normalizedBlockedKeyBaseLimit)
    ? Number.parseInt(normalizedBlockedKeyBaseLimit, 10)
    : null
  const parsedGlobalIpLimit = isValidNonNegativeIntegerDraft(normalizedGlobalIpLimit)
    ? Number.parseInt(normalizedGlobalIpLimit, 10)
    : null
  const changed =
    settings != null &&
    parsedRequestRateLimit != null &&
    parsedCount != null &&
    parsedPercent != null &&
    parsedApiRebalancePercent != null &&
    parsedBlockedKeyBaseLimit != null &&
    parsedGlobalIpLimit != null &&
    (parsedRequestRateLimit !== settings.requestRateLimit ||
      parsedCount !== settings.mcpSessionAffinityKeyCount ||
      draftRebalanceEnabled !== settings.rebalanceMcpEnabled ||
      parsedPercent !== settings.rebalanceMcpSessionPercent ||
      draftApiRebalanceEnabled !== settings.apiRebalanceEnabled ||
      parsedApiRebalancePercent !== settings.apiRebalancePercent ||
      draftRechargeFeatureEnabled !== settings.rechargeFeatureEnabled ||
      draftRechargeUserEnabled !== settings.rechargeUserEnabled ||
      parsedBlockedKeyBaseLimit !== settings.userBlockedKeyBaseLimit ||
      parsedGlobalIpLimit !== settings.globalIpLimit)
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
    percent: normalizedPercent.length > 0 && parsedPercent == null ? strings.form.invalidPercent : null,
    apiRebalancePercent:
      normalizedApiRebalancePercent.length > 0 && parsedApiRebalancePercent == null
        ? strings.form.invalidPercent
        : null,
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
    fieldErrors.percent ??
    fieldErrors.apiRebalancePercent ??
    fieldErrors.blockedKeyBaseLimit ??
    fieldErrors.globalIpLimit ??
    parsedTrustedClientIpHeaders.duplicateError ??
    error
  const requestRateLimitErrorId = 'system-settings-request-rate-limit-error'
  const blockedKeyBaseLimitErrorId = 'system-settings-blocked-key-base-limit-error'
  const globalIpLimitErrorId = 'system-settings-global-ip-limit-error'
  const affinityCountErrorId = 'system-settings-affinity-count-error'
  const rebalancePercentErrorId = 'system-settings-rebalance-percent-error'
  const apiRebalancePercentErrorId = 'system-settings-api-rebalance-percent-error'

  const buildNormalSettingsPayload = (overrides: NormalSystemSettingsOverrides = {}): SystemSettings | null => {
    if (
      settings == null ||
      parsedRequestRateLimit == null ||
      parsedCount == null ||
      parsedPercent == null ||
      parsedApiRebalancePercent == null ||
      parsedBlockedKeyBaseLimit == null ||
      parsedGlobalIpLimit == null
    )
      return null
    return {
      requestRateLimit: overrides.requestRateLimit ?? parsedRequestRateLimit,
      mcpSessionAffinityKeyCount: overrides.mcpSessionAffinityKeyCount ?? parsedCount,
      rebalanceMcpEnabled: overrides.rebalanceMcpEnabled ?? draftRebalanceEnabled,
      rebalanceMcpSessionPercent: overrides.rebalanceMcpSessionPercent ?? parsedPercent,
      apiRebalanceEnabled: overrides.apiRebalanceEnabled ?? draftApiRebalanceEnabled,
      apiRebalancePercent: overrides.apiRebalancePercent ?? parsedApiRebalancePercent,
      rechargeFeatureEnabled: overrides.rechargeFeatureEnabled ?? draftRechargeFeatureEnabled,
      rechargeUserEnabled: overrides.rechargeUserEnabled ?? draftRechargeUserEnabled,
      userBlockedKeyBaseLimit: overrides.userBlockedKeyBaseLimit ?? parsedBlockedKeyBaseLimit,
      globalIpLimit: overrides.globalIpLimit ?? parsedGlobalIpLimit,
      trustedProxyCidrs: settings.trustedProxyCidrs,
      trustedClientIpHeaders: settings.trustedClientIpHeaders,
    }
  }

  const normalPayloadChanged = (payload: SystemSettings): boolean =>
    settings != null &&
    (payload.requestRateLimit !== settings.requestRateLimit ||
      payload.mcpSessionAffinityKeyCount !== settings.mcpSessionAffinityKeyCount ||
      payload.rebalanceMcpEnabled !== settings.rebalanceMcpEnabled ||
      payload.rebalanceMcpSessionPercent !== settings.rebalanceMcpSessionPercent ||
      payload.apiRebalanceEnabled !== settings.apiRebalanceEnabled ||
      payload.apiRebalancePercent !== settings.apiRebalancePercent ||
      payload.rechargeFeatureEnabled !== settings.rechargeFeatureEnabled ||
      payload.rechargeUserEnabled !== settings.rechargeUserEnabled ||
      payload.userBlockedKeyBaseLimit !== settings.userBlockedKeyBaseLimit ||
      payload.globalIpLimit !== settings.globalIpLimit)

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
                parsedPercent == null ||
                parsedApiRebalancePercent == null ||
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

              {draftRechargeFeatureEnabled && (
                <div className="system-settings-action-row system-settings-totp-row" aria-labelledby="system-settings-totp-title">
                  <div className="system-settings-toggle-copy">
                    <span className="system-settings-setting-title" id="system-settings-totp-title">
                      {strings.form.totpTitle}
                    </span>
                    <p>
                      {totpStatus?.enabled
                        ? strings.form.totpBoundHint
                        : strings.form.totpUnboundHint}
                    </p>
                    {totpStatus?.missingCryptoKey && <p className="form-error">{strings.form.totpMissingCryptoKey}</p>}
                    {totpError && <p className="form-error">{totpError}</p>}
                    {totpSetup && (
                      <div className="system-settings-totp-setup">
                        <img
                          src={`data:image/png;base64,${totpSetup.qrPngBase64}`}
                          alt={strings.form.totpQrAlt}
                          className="system-settings-totp-qr"
                        />
                        <Input value={totpSetup.secret} readOnly aria-label={strings.form.totpSetupSecretLabel} />
                        {totpStatus?.enabled && (
                          <Input
                            id="admin-totp-current-code"
                            name="admin_totp_current_code"
                            value={totpCurrentCode}
                            onChange={(event) => setTotpCurrentCode(event.target.value.replace(/\D/g, '').slice(0, 6))}
                            placeholder={strings.form.totpCurrentCodePlaceholder}
                            autoComplete="one-time-code"
                            inputMode="numeric"
                            pattern="[0-9]*"
                          />
                        )}
                        <Input
                          id="admin-totp-bind-code"
                          name="admin_totp_bind_code"
                          value={totpCode}
                          onChange={(event) => setTotpCode(event.target.value.replace(/\D/g, '').slice(0, 6))}
                          placeholder={strings.form.totpConfirmCodePlaceholder}
                          autoComplete="one-time-code"
                          inputMode="numeric"
                          pattern="[0-9]*"
                        />
                      </div>
                    )}
                  </div>
                  <div className="system-settings-totp-actions">
                    {!totpSetup && (
                      <Button
                        type="button"
                        variant="outline"
                        disabled={totpBusy || !totpStatus?.available}
                        onClick={() => void beginTotpSetup()}
                      >
                        {totpStatus?.enabled ? strings.form.totpResetAction : strings.form.totpBindAction}
                      </Button>
                    )}
                    {totpSetup && (
                      <Button
                        type="button"
                        disabled={totpBusy || totpCode.length !== 6 || (totpStatus?.enabled && totpCurrentCode.length !== 6)}
                        onClick={() => void confirmTotpSetup()}
                      >
                        {strings.form.totpConfirmAction}
                      </Button>
                    )}
                    {totpStatus?.enabled && !totpSetup && (
                      <>
                        <Input
                          id="admin-totp-disable-code"
                          name="admin_totp_disable_code"
                          value={totpCurrentCode}
                          onChange={(event) => setTotpCurrentCode(event.target.value.replace(/\D/g, '').slice(0, 6))}
                          placeholder={strings.form.totpCurrentCodePlaceholder}
                          autoComplete="one-time-code"
                          inputMode="numeric"
                          pattern="[0-9]*"
                        />
                        <Button
                          type="button"
                          variant="outline"
                          disabled={totpBusy || totpCurrentCode.length !== 6}
                          onClick={() => void disableTotpBinding()}
                        >
                          {strings.form.totpDisableAction}
                        </Button>
                      </>
                    )}
                  </div>
                </div>
              )}

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
                {settings && (
                  <p className="system-settings-field-current text-xs text-muted-foreground">
                    {strings.form.currentRequestRateLimitValue.replace('{count}', String(settings.requestRateLimit))}
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
                {settings && (
                  <p className="system-settings-field-current text-xs text-muted-foreground">
                    {strings.form.currentBlockedKeyBaseLimitValue.replace(
                      '{count}',
                      String(settings.userBlockedKeyBaseLimit),
                    )}
                  </p>
                )}
                <p className="system-settings-field-hint text-xs text-muted-foreground">
                  {strings.form.blockedKeyBaseLimitHint}
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
                {settings && (
                  <p className="system-settings-field-current text-xs text-muted-foreground">
                    {strings.form.currentGlobalIpLimitValue.replace('{count}', String(settings.globalIpLimit))}
                  </p>
                )}
                <p className="system-settings-field-hint text-xs text-muted-foreground">{strings.form.globalIpLimitHint}</p>
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
                {settings && (
                  <p className="system-settings-field-current text-xs text-muted-foreground">
                    {strings.form.currentValue.replace('{count}', String(settings.mcpSessionAffinityKeyCount))}
                  </p>
                )}
              </div>

              <div className="system-settings-toggle-row">
                <div className="system-settings-toggle-copy">
                  <label className="text-sm font-medium" htmlFor="system-settings-rebalance-switch">
                    {strings.form.rebalanceLabel}
                  </label>
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

              <div className="system-settings-field">
                <label className="text-sm font-medium" htmlFor="system-settings-rebalance-percent">
                  {strings.form.percentLabel}
                </label>
                <div className="system-settings-range-control grid gap-3 md:grid-cols-[minmax(0,1fr),96px] md:items-center">
                  <input
                    id="system-settings-rebalance-percent"
                    className="range"
                    type="range"
                    min={0}
                    max={100}
                    step={1}
                    value={parsedPercent ?? 0}
                    disabled={saving || !draftRebalanceEnabled}
                    onChange={(event) => setDraftPercent(event.target.value)}
                    onBlur={() => {
                      void commitNormalSettings()
                    }}
                    aria-label={strings.form.percentLabel}
                  />
                  <Input
                    id="system-settings-rebalance-percent-input"
                    type="number"
                    inputMode="numeric"
                    min={0}
                    max={100}
                    step={1}
                    value={draftPercent}
                    disabled={saving || !draftRebalanceEnabled}
                    onChange={(event) => setDraftPercent(event.target.value)}
                    onBlur={() => {
                      void commitNormalSettings()
                    }}
                    onKeyDown={handleCommitKeyDown}
                    aria-label={strings.form.percentLabel}
                    aria-invalid={fieldErrors.percent ? true : undefined}
                    aria-describedby={fieldErrors.percent ? rebalancePercentErrorId : undefined}
                  />
                </div>
                {fieldErrors.percent && (
                  <p id={rebalancePercentErrorId} className="system-settings-field-error text-xs font-medium text-destructive">
                    {fieldErrors.percent}
                  </p>
                )}
                {settings && (
                  <p className="system-settings-field-current text-xs text-muted-foreground">
                    {strings.form.currentPercentValue.replace('{percent}', String(settings.rebalanceMcpSessionPercent))}
                  </p>
                )}
                <p className="system-settings-field-hint text-xs text-muted-foreground">
                  {draftRebalanceEnabled ? strings.form.percentHint : strings.form.percentDisabledHint}
                </p>
              </div>
            </div>
          </section>

          <section className="system-settings-config-section">
            <h4>{strings.form.apiRebalanceTitle}</h4>
            <div className="system-settings-field-grid system-settings-field-grid--api">
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

              <div className="system-settings-field">
                <label className="text-sm font-medium" htmlFor="system-settings-api-rebalance-percent">
                  {strings.form.apiRebalancePercentLabel}
                </label>
                <div className="system-settings-range-control grid gap-3 md:grid-cols-[minmax(0,1fr),96px] md:items-center">
                  <input
                    id="system-settings-api-rebalance-percent"
                    className="range"
                    type="range"
                    min={0}
                    max={100}
                    step={1}
                    value={parsedApiRebalancePercent ?? 0}
                    disabled={saving || !draftApiRebalanceEnabled}
                    onChange={(event) => setDraftApiRebalancePercent(event.target.value)}
                    onBlur={() => {
                      void commitNormalSettings()
                    }}
                    aria-label={strings.form.apiRebalancePercentLabel}
                  />
                  <Input
                    id="system-settings-api-rebalance-percent-input"
                    type="number"
                    inputMode="numeric"
                    min={0}
                    max={100}
                    step={1}
                    value={draftApiRebalancePercent}
                    disabled={saving || !draftApiRebalanceEnabled}
                    onChange={(event) => setDraftApiRebalancePercent(event.target.value)}
                    onBlur={() => {
                      void commitNormalSettings()
                    }}
                    onKeyDown={handleCommitKeyDown}
                    aria-label={strings.form.apiRebalancePercentLabel}
                    aria-invalid={fieldErrors.apiRebalancePercent ? true : undefined}
                    aria-describedby={fieldErrors.apiRebalancePercent ? apiRebalancePercentErrorId : undefined}
                  />
                </div>
                {fieldErrors.apiRebalancePercent && (
                  <p id={apiRebalancePercentErrorId} className="system-settings-field-error text-xs font-medium text-destructive">
                    {fieldErrors.apiRebalancePercent}
                  </p>
                )}
                {settings && (
                  <p className="system-settings-field-current text-xs text-muted-foreground">
                    {strings.form.currentApiRebalancePercentValue.replace(
                      '{percent}',
                      String(settings.apiRebalancePercent),
                    )}
                  </p>
                )}
                <p className="system-settings-field-hint text-xs text-muted-foreground">
                  {draftApiRebalanceEnabled
                    ? strings.form.apiRebalancePercentHint
                    : strings.form.apiRebalancePercentDisabledHint}
                </p>
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
