import { useEffect, useState } from 'react'

import { fetchObservedClientIpRequests, type ObservedClientIpRequest, type SystemSettings } from '../api'
import type { QueryLoadState } from './queryLoadState'
import type { AdminTranslations } from '../i18n'
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
  DialogTrigger,
} from '../components/ui/dialog'

interface SystemSettingsModuleProps {
  strings: AdminTranslations['systemSettings']
  settings: SystemSettings | null
  loadState: QueryLoadState
  error: string | null
  saving: boolean
  helpBubbleOpen?: boolean
  onApply: (settings: SystemSettings) => Promise<void> | void
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
  onApply,
}: SystemSettingsModuleProps): JSX.Element {
  const [draftRequestRateLimit, setDraftRequestRateLimit] = useState(() =>
    settings ? String(settings.requestRateLimit) : '100',
  )
  const [draftCount, setDraftCount] = useState(() =>
    settings ? String(settings.mcpSessionAffinityKeyCount) : '',
  )
  const [draftRebalanceEnabled, setDraftRebalanceEnabled] = useState(
    settings?.rebalanceMcpEnabled ?? false,
  )
  const [draftPercent, setDraftPercent] = useState(() =>
    settings ? String(settings.rebalanceMcpSessionPercent) : '100',
  )
  const [draftApiRebalanceEnabled, setDraftApiRebalanceEnabled] = useState(
    settings?.apiRebalanceEnabled ?? false,
  )
  const [draftApiRebalancePercent, setDraftApiRebalancePercent] = useState(() =>
    settings ? String(settings.apiRebalancePercent) : '0',
  )
  const [draftBlockedKeyBaseLimit, setDraftBlockedKeyBaseLimit] = useState(() =>
    settings ? String(settings.userBlockedKeyBaseLimit) : '5',
  )
  const [clientIpDialogOpen, setClientIpDialogOpen] = useState(false)
  const [draftTrustedProxyCidrs, setDraftTrustedProxyCidrs] = useState(() =>
    settings?.trustedProxyCidrs?.join('\n') ?? '',
  )
  const [draftTrustedClientIpHeaders, setDraftTrustedClientIpHeaders] = useState(() =>
    settings?.trustedClientIpHeaders?.join('\n') ?? '',
  )
  const [observedClientIpRequests, setObservedClientIpRequests] = useState<ObservedClientIpRequest[]>([])
  const [observedClientIpRequestsError, setObservedClientIpRequestsError] = useState<string | null>(null)

  useEffect(() => {
    setDraftRequestRateLimit(settings ? String(settings.requestRateLimit) : '100')
    setDraftCount(settings ? String(settings.mcpSessionAffinityKeyCount) : '')
    setDraftRebalanceEnabled(settings?.rebalanceMcpEnabled ?? false)
    setDraftPercent(settings ? String(settings.rebalanceMcpSessionPercent) : '100')
    setDraftApiRebalanceEnabled(settings?.apiRebalanceEnabled ?? false)
    setDraftApiRebalancePercent(settings ? String(settings.apiRebalancePercent) : '0')
    setDraftBlockedKeyBaseLimit(settings ? String(settings.userBlockedKeyBaseLimit) : '5')
    setDraftTrustedProxyCidrs(settings?.trustedProxyCidrs?.join('\n') ?? '')
    setDraftTrustedClientIpHeaders(settings?.trustedClientIpHeaders?.join('\n') ?? '')
  }, [
    settings?.requestRateLimit,
    settings?.mcpSessionAffinityKeyCount,
    settings?.rebalanceMcpEnabled,
    settings?.rebalanceMcpSessionPercent,
    settings?.apiRebalanceEnabled,
    settings?.apiRebalancePercent,
    settings?.userBlockedKeyBaseLimit,
    settings?.trustedProxyCidrs,
    settings?.trustedClientIpHeaders,
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
  const normalizedPercent = draftPercent.trim()
  const normalizedApiRebalancePercent = draftApiRebalancePercent.trim()
  const normalizedBlockedKeyBaseLimit = draftBlockedKeyBaseLimit.trim()
  const normalizedTrustedProxyCidrs = draftTrustedProxyCidrs
    .split(/\r?\n/)
    .map((value) => value.trim())
    .filter(Boolean)
  const parsedTrustedClientIpHeaders = parseTrustedClientIpHeaderDraft(draftTrustedClientIpHeaders)
  const normalizedTrustedClientIpHeaders = parsedTrustedClientIpHeaders.values
  const observedHeaderColumns =
    normalizedTrustedClientIpHeaders.length > 0
      ? normalizedTrustedClientIpHeaders
      : settings?.trustedClientIpHeaders ?? []
  const parsedRequestRateLimit = isValidRequestRateLimitDraft(normalizedRequestRateLimit)
    ? Number.parseInt(normalizedRequestRateLimit, 10)
    : null
  const parsedCount = isValidCountDraft(normalizedCount) ? Number.parseInt(normalizedCount, 10) : null
  const parsedPercent = isValidPercentDraft(normalizedPercent)
    ? Number.parseInt(normalizedPercent, 10)
    : null
  const parsedApiRebalancePercent = isValidPercentDraft(normalizedApiRebalancePercent)
    ? Number.parseInt(normalizedApiRebalancePercent, 10)
    : null
  const parsedBlockedKeyBaseLimit = isValidNonNegativeIntegerDraft(normalizedBlockedKeyBaseLimit)
    ? Number.parseInt(normalizedBlockedKeyBaseLimit, 10)
    : null
  const changed =
    settings != null &&
    parsedRequestRateLimit != null &&
    parsedCount != null &&
    parsedPercent != null &&
    parsedApiRebalancePercent != null &&
    parsedBlockedKeyBaseLimit != null &&
    (parsedRequestRateLimit !== settings.requestRateLimit ||
      parsedCount !== settings.mcpSessionAffinityKeyCount ||
      draftRebalanceEnabled !== settings.rebalanceMcpEnabled ||
      parsedPercent !== settings.rebalanceMcpSessionPercent ||
      draftApiRebalanceEnabled !== settings.apiRebalanceEnabled ||
      parsedApiRebalancePercent !== settings.apiRebalancePercent ||
      parsedBlockedKeyBaseLimit !== settings.userBlockedKeyBaseLimit ||
      normalizedTrustedProxyCidrs.join('\n') !== settings.trustedProxyCidrs.join('\n') ||
      normalizedTrustedClientIpHeaders.join('\n') !== settings.trustedClientIpHeaders.join('\n'))
  const inlineError =
    normalizedRequestRateLimit.length > 0 && parsedRequestRateLimit == null
      ? strings.form.invalidRequestRateLimit
      : normalizedCount.length > 0 && parsedCount == null
      ? strings.form.invalidCount
      : normalizedPercent.length > 0 && parsedPercent == null
        ? strings.form.invalidPercent
        : normalizedApiRebalancePercent.length > 0 && parsedApiRebalancePercent == null
          ? strings.form.invalidPercent
          : normalizedBlockedKeyBaseLimit.length > 0 && parsedBlockedKeyBaseLimit == null
            ? strings.form.invalidBlockedKeyBaseLimit
            : parsedTrustedClientIpHeaders.duplicateError ?? error
  const observedClientIpRequestsSection = (
    <div className="grid gap-3 rounded-md border border-border/60 bg-muted/20 p-3 text-sm">
      <div className="grid gap-1">
        <span className="font-medium">最近请求中的字段值</span>
        <p className="text-xs text-muted-foreground">最近 50 条可见请求，按时间倒序。</p>
      </div>
      {observedClientIpRequestsError ? (
        <p className="text-sm text-destructive">{observedClientIpRequestsError}</p>
      ) : observedHeaderColumns.length === 0 ? (
        <p className="text-sm text-muted-foreground">先在上方添加要核对的请求头字段。</p>
      ) : observedClientIpRequests.length === 0 ? (
        <p className="text-sm text-muted-foreground">暂无近期请求。</p>
      ) : (
        <div className="max-h-[min(18rem,36dvh)] overflow-auto rounded-md border border-border bg-background">
          <table className="w-max min-w-full table-auto text-left text-sm">
            <thead className="bg-muted/50 text-sm text-muted-foreground">
              <tr>
                <th className="whitespace-nowrap px-4 py-3">请求</th>
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

  return (
    <section className="surface panel">
      <div className="panel-header">
        <div>
          <h2>{strings.title}</h2>
        </div>
      </div>

      <AdminLoadingRegion
        loadState={loadState}
        loadingLabel={strings.description}
        errorLabel={error ?? undefined}
        minHeight={260}
      >
        <div
          className="rounded-2xl border border-border/60 bg-background/55 p-5 shadow-sm backdrop-blur"
          style={{ display: 'grid', gap: 20 }}
        >
          <div>
            <h3 className="text-base font-semibold">{strings.form.title}</h3>
          </div>

          <div style={{ display: 'grid', gap: 12, gridTemplateColumns: 'minmax(220px, 420px)' }}>
            <div className="rounded-lg border border-border/70 bg-muted/20 p-4" style={{ display: 'grid', gap: 10 }}>
              <div>
                <h4 className="text-sm font-semibold">可信客户端 IP</h4>
                <p className="text-xs text-muted-foreground">
                  {settings?.trustedClientIpHeaders?.join(' -> ') || 'cf-connecting-ip -> true-client-ip -> x-real-ip -> x-forwarded-for -> forwarded'}
                </p>
              </div>
              <Dialog open={clientIpDialogOpen} onOpenChange={setClientIpDialogOpen}>
                <DialogTrigger asChild>
                  <Button type="button" variant="outline" size="sm" disabled={saving}>
                    <Icon icon="mdi:shield-account-outline" width={16} height={16} aria-hidden="true" />
                    配置可信 IP
                  </Button>
                </DialogTrigger>
                <DialogContent className="grid max-h-[calc(100dvh-2rem)] w-[min(72rem,calc(100vw-2rem))] max-w-6xl grid-rows-[auto_minmax(0,1fr)_auto] gap-0 overflow-hidden p-0">
                  <DialogHeader className="px-6 pb-4 pr-12 pt-6">
                    <DialogTitle>可信客户端 IP</DialogTitle>
                    <DialogDescription>
                      先核对最近请求中的真实值，再用快捷按钮切换下方请求头顺序。
                    </DialogDescription>
                  </DialogHeader>
                  <div className="grid min-h-0 gap-4 overflow-y-auto px-6 pb-4">
                    <div className="grid gap-2 text-sm">
                      <label className="flex flex-col gap-2">
                        <span className="font-medium">可信代理 CIDR</span>
                        <textarea
                          rows={4}
                          className="resize-y rounded-md border border-input bg-background px-3 py-2 text-sm leading-6"
                          value={draftTrustedProxyCidrs}
                          disabled={saving}
                          onChange={(event) => setDraftTrustedProxyCidrs(event.target.value)}
                        />
                      </label>

                      <div className="grid gap-1">
                        <span className="font-medium">客户端 IP 请求头顺序</span>
                        <p className="text-xs text-muted-foreground">
                          点击切换。选中会出现在列表末尾，取消会从列表删除。
                        </p>
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
                              {preset.headers.map((header) => (
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
                                        setDraftTrustedClientIpHeaders((current) =>
                                          toggleOrderedHeaderDraft(current, header),
                                        )
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
                                })()
                              ))}
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
                  </div>
                  <DialogFooter className="border-t border-border/60 px-6 py-4">
                    <Button type="button" variant="outline" onClick={() => setClientIpDialogOpen(false)}>
                      完成
                    </Button>
                  </DialogFooter>
                </DialogContent>
              </Dialog>
            </div>

            <div style={{ display: 'grid', gap: 8 }}>
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
                aria-invalid={inlineError ? true : undefined}
              />
              {settings && (
                <p className="text-xs text-muted-foreground">
                  {strings.form.currentRequestRateLimitValue.replace(
                    '{count}',
                    String(settings.requestRateLimit),
                  )}
                </p>
              )}
              <p className="text-xs text-muted-foreground">{strings.form.requestRateLimitHint}</p>
            </div>

            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
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
              aria-invalid={inlineError ? true : undefined}
            />
            {settings && (
              <p className="text-xs text-muted-foreground">
                {strings.form.currentValue.replace('{count}', String(settings.mcpSessionAffinityKeyCount))}
              </p>
            )}

            <div className="mt-2 flex items-start justify-between gap-4 rounded-2xl border border-border/60 bg-background/70 px-4 py-3">
              <div style={{ display: 'grid', gap: 4 }}>
                <label className="text-sm font-medium" htmlFor="system-settings-rebalance-switch">
                  {strings.form.rebalanceLabel}
                </label>
                <p className="text-xs text-muted-foreground">{strings.form.rebalanceHint}</p>
              </div>
              <Switch
                aria-label={strings.form.rebalanceLabel}
                id="system-settings-rebalance-switch"
                checked={draftRebalanceEnabled}
                onCheckedChange={setDraftRebalanceEnabled}
                disabled={saving}
              />
            </div>

            <div style={{ display: 'grid', gap: 8 }}>
              <label className="text-sm font-medium" htmlFor="system-settings-rebalance-percent">
                {strings.form.percentLabel}
              </label>
              <div className="grid gap-3 md:grid-cols-[minmax(0,1fr),96px] md:items-center">
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
                  aria-label={strings.form.percentLabel}
                />
                <Input
                  type="number"
                  inputMode="numeric"
                  min={0}
                  max={100}
                  step={1}
                  value={draftPercent}
                  disabled={saving || !draftRebalanceEnabled}
                  onChange={(event) => setDraftPercent(event.target.value)}
                  aria-invalid={inlineError ? true : undefined}
                />
              </div>
              {settings && (
                <p className="text-xs text-muted-foreground">
                  {strings.form.currentPercentValue.replace(
                    '{percent}',
                    String(settings.rebalanceMcpSessionPercent),
                  )}
                </p>
              )}
              <p className="text-xs text-muted-foreground">
                {draftRebalanceEnabled ? strings.form.percentHint : strings.form.percentDisabledHint}
              </p>
            </div>

            <div className="mt-2 flex items-start justify-between gap-4 rounded-2xl border border-border/60 bg-background/70 px-4 py-3">
              <div style={{ display: 'grid', gap: 4 }}>
                <label className="text-sm font-medium" htmlFor="system-settings-api-rebalance-switch">
                  {strings.form.apiRebalanceLabel}
                </label>
                <p className="text-xs text-muted-foreground">{strings.form.apiRebalanceHint}</p>
              </div>
              <Switch
                aria-label={strings.form.apiRebalanceLabel}
                id="system-settings-api-rebalance-switch"
                checked={draftApiRebalanceEnabled}
                onCheckedChange={setDraftApiRebalanceEnabled}
                disabled={saving}
              />
            </div>

            <div style={{ display: 'grid', gap: 8 }}>
              <label className="text-sm font-medium" htmlFor="system-settings-api-rebalance-percent">
                {strings.form.apiRebalancePercentLabel}
              </label>
              <div className="grid gap-3 md:grid-cols-[minmax(0,1fr),96px] md:items-center">
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
                  aria-label={strings.form.apiRebalancePercentLabel}
                />
                <Input
                  type="number"
                  inputMode="numeric"
                  min={0}
                  max={100}
                  step={1}
                  value={draftApiRebalancePercent}
                  disabled={saving || !draftApiRebalanceEnabled}
                  onChange={(event) => setDraftApiRebalancePercent(event.target.value)}
                  aria-invalid={inlineError ? true : undefined}
                />
              </div>
              {settings && (
                <p className="text-xs text-muted-foreground">
                  {strings.form.currentApiRebalancePercentValue.replace(
                    '{percent}',
                    String(settings.apiRebalancePercent),
                  )}
                </p>
              )}
              <p className="text-xs text-muted-foreground">
                {draftApiRebalanceEnabled
                  ? strings.form.apiRebalancePercentHint
                  : strings.form.apiRebalancePercentDisabledHint}
              </p>
            </div>

            <div style={{ display: 'grid', gap: 8 }}>
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
                aria-invalid={inlineError ? true : undefined}
              />
              {settings && (
                <p className="text-xs text-muted-foreground">
                  {strings.form.currentBlockedKeyBaseLimitValue.replace(
                    '{count}',
                    String(settings.userBlockedKeyBaseLimit),
                  )}
                </p>
              )}
              <p className="text-xs text-muted-foreground">{strings.form.blockedKeyBaseLimitHint}</p>
            </div>
          </div>

          {(inlineError || saving) && (
            <p
              className="text-sm font-medium"
              role="status"
              aria-live="polite"
              style={{ color: inlineError ? 'hsl(var(--destructive))' : undefined }}
            >
              {inlineError ?? strings.actions.applying}
            </p>
          )}

          <div style={{ display: 'flex', justifyContent: 'flex-start' }}>
            <Button
              type="button"
              onClick={() => {
                if (
                  parsedRequestRateLimit == null ||
                  parsedCount == null ||
                  parsedPercent == null ||
                  parsedApiRebalancePercent == null ||
                  parsedBlockedKeyBaseLimit == null ||
                  parsedTrustedClientIpHeaders.duplicateError != null ||
                  saving ||
                  !changed
                ) return
                void onApply({
                  requestRateLimit: parsedRequestRateLimit,
                  mcpSessionAffinityKeyCount: parsedCount,
                  rebalanceMcpEnabled: draftRebalanceEnabled,
                  rebalanceMcpSessionPercent: parsedPercent,
                  apiRebalanceEnabled: draftApiRebalanceEnabled,
                  apiRebalancePercent: parsedApiRebalancePercent,
                  userBlockedKeyBaseLimit: parsedBlockedKeyBaseLimit,
                  trustedProxyCidrs: normalizedTrustedProxyCidrs,
                  trustedClientIpHeaders: normalizedTrustedClientIpHeaders,
                })
              }}
              disabled={
                saving ||
                !changed ||
                parsedRequestRateLimit == null ||
                parsedCount == null ||
                parsedPercent == null ||
                parsedApiRebalancePercent == null ||
                parsedBlockedKeyBaseLimit == null ||
                parsedTrustedClientIpHeaders.duplicateError != null
              }
              data-testid="system-settings-apply"
            >
              <Icon
                icon={saving ? 'mdi:loading' : 'mdi:check-circle-outline'}
                width={16}
                height={16}
                className={saving ? 'icon-spin' : undefined}
                aria-hidden="true"
              />
              <span>{saving ? strings.actions.applying : strings.actions.apply}</span>
            </Button>
          </div>
        </div>
      </AdminLoadingRegion>
    </section>
  )
}
