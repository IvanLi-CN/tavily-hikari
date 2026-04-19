import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Icon, getGuideClientIconName } from '../lib/icons'
import CherryStudioMock from '../components/CherryStudioMock'
import ConnectivityChecksPanel, {
  type ProbeBubbleItem,
  type ProbeBubbleModel,
  type ProbeButtonModel,
  type ProbeButtonState,
  type ProbeStepStatus,
} from '../components/ConnectivityChecksPanel'
import TokenSecretField, { type TokenSecretCopyState } from '../components/TokenSecretField'
import ManualCopyBubble from '../components/ManualCopyBubble'
import UserConsoleHeader from '../components/UserConsoleHeader'

import {
  createBrowserTodayWindow,
  fetchVersion,
  fetchProfile,
  millisecondsUntilNextBrowserDayBoundary,
  probeApiTavilyCrawl,
  probeApiTavilyExtract,
  probeApiTavilyMap,
  probeApiTavilyResearch,
  probeApiTavilyResearchResult,
  probeApiTavilySearch,
  probeMcpInitialize,
  probeMcpInitialized,
  probeMcpPing,
  probeMcpToolsCall,
  probeMcpToolsList,
  fetchUserDashboard,
  fetchUserTokenDetail,
  buildUserTokenEventsUrl,
  fetchUserTokenLogs,
  fetchUserTokenSecret,
  fetchUserTokens,
  postUserLogout,
  parseUserTokenEventSnapshot,
  type Profile,
  type PublicTokenLog,
  type UserTokenEventSnapshot,
  type UserDashboard,
  type UserTokenSummary,
  type VersionInfo,
} from '../api'
import RollingNumber from '../components/RollingNumber'
import { StatusBadge, type StatusTone } from '../components/StatusBadge'
import UserConsoleFooter from '../components/UserConsoleFooter'
import { Button } from '../components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '../components/ui/dropdown-menu'
import { Tooltip, TooltipContent, TooltipTrigger } from '../components/ui/tooltip'
import { useLanguage, useTranslate, type Language } from '../i18n'
import { copyText, isCopyIntentKey, selectAllReadonlyText, shouldPrewarmSecretCopy } from '../lib/clipboard'
import {
  getMcpProbeResultError,
  type McpProbeStepState,
  type ProbeQuotaWindow,
  McpProbeRequestError,
  getProbeEnvelopeError,
  getQuotaExceededWindow,
  getTokenBusinessQuotaWindow,
  revalidateBlockedQuotaWindow,
  resolveMcpProbeButtonState,
} from '../lib/mcpProbe'
import { useResponsiveModes } from '../lib/responsive'
import {
  defaultRequestRateLabel,
  formatRequestRateScope,
  formatRequestRateSummary,
  resolveRequestRate,
} from '../requestRate'
import { getUserConsoleAdminHref } from '../lib/userConsoleAdminEntry'
import { resolveUserConsoleAvailability } from '../lib/userConsoleAvailability'
import {
  normalizeUserConsolePathname,
  parseUserConsolePath,
  userConsoleRouteToPath,
  type UserConsoleLandingSection,
  type UserConsoleRoute as ConsoleRoute,
} from '../lib/userConsoleRoutes'
import { MobileGuideDropdown, buildGuideContent, resolveGuideSamples } from './guide'
import { EN, ZH } from './text'


export const CODEX_DOC_URL = 'https://github.com/openai/codex/blob/main/docs/config.md'
export const CLAUDE_DOC_URL = 'https://code.claude.com/docs/en/mcp'
export const MCP_SPEC_URL = 'https://modelcontextprotocol.io/introduction'
const MCP_PROBE_PROTOCOL_VERSION = '2025-03-26'
export const TAVILY_SEARCH_DOC_URL = 'https://docs.tavily.com/documentation/api-reference/endpoint/search'
export const VSCODE_DOC_URL = 'https://code.visualstudio.com/docs/copilot/customization/mcp-servers'
export const NOCODB_DOC_URL = 'https://nocodb.com/docs/product-docs/mcp'
const USER_CONSOLE_SECRET_CACHE_TTL_MS = 2_000
const USER_CONSOLE_SECRET_PREWARM_DELAY_MS = 120
const BASE_MCP_PROBE_STEP_COUNT = 4
const BASE_API_PROBE_STEP_COUNT = 6

export type GuideLanguage = 'toml' | 'json' | 'bash'
export type GuideKey = 'codex' | 'claude' | 'vscode' | 'claudeDesktop' | 'cursor' | 'windsurf' | 'cherryStudio' | 'other'
type DetailLogsPushIssueCode = 'unsupported' | 'reconnecting' | 'closed'

export interface GuideReference {
  label: string
  url: string
}

export interface GuideSample {
  title: string
  language?: GuideLanguage
  snippet: string
  reference?: GuideReference
}

export interface GuideContent {
  title: string
  steps: ReactNode[]
  sampleTitle?: string
  snippetLanguage?: GuideLanguage
  snippet?: string
  reference?: GuideReference
  samples?: GuideSample[]
}

interface ManualCopyBubbleState {
  anchorEl: HTMLElement | null
  value: string
}

interface DetailLogsPushStatusText {
  ariaLabel: string
  browserUnsupported: string
  reconnecting: string
  closed: string
}

function resolveDetailLogsPushIssueMessage(
  issue: DetailLogsPushIssueCode,
  text: DetailLogsPushStatusText,
): string {
  switch (issue) {
    case 'unsupported':
      return text.browserUnsupported
    case 'reconnecting':
      return text.reconnecting
    case 'closed':
      return text.closed
  }
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

interface McpProbeStepDefinition {
  id: string
  label: string
  billable?: boolean
  run: (token: string, context: McpProbeRunContext) => Promise<McpProbeStepResult | null>
}

interface AdvertisedMcpTool {
  requestName: string
  displayName: string
  inputSchema: Record<string, unknown> | null
}

interface McpProbeStepResult {
  detail?: string | null
  discoveredTools?: AdvertisedMcpTool[]
  stepState?: Extract<McpProbeStepState, 'success' | 'skipped'>
}

interface McpProbeRunContext {
  protocolVersion: string
  sessionId: string | null
  clientVersion: string
  identity: McpProbeIdentityGenerator
  signal?: AbortSignal
}

interface McpProbeIdentityGenerator {
  runSignature: string
  nextRequestId: (kind: string, toolName?: string) => string
  nextIdentifier: (fieldName: string) => string
}

interface McpProbeIdentityGeneratorOptions {
  now?: number
  random?: () => number
}

interface ApiProbeStepDefinition {
  id: string
  label: string
  run: (
    token: string,
    context: { requestId: string | null, signal?: AbortSignal },
  ) => Promise<string | null>
}

interface McpProbeText {
  steps: {
    mcpInitialize: string
    mcpInitialized: string
    mcpPing: string
    mcpToolsList: string
    mcpToolCall: string
  }
  skippedProbeFixture: string
  errors: {
    missingAdvertisedTools: string
  }
}

interface ApiProbeText {
  steps: {
    apiSearch: string
    apiExtract: string
    apiCrawl: string
    apiMap: string
    apiResearch: string
    apiResearchResult: string
  }
  errors: {
    missingRequestId: string
    researchFailed: string
    researchUnexpectedStatus: string
  }
  researchPendingAccepted: string
  researchStatus: string
}

const numberFormatter = new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 })

function formatNumber(value: number): string {
  return numberFormatter.format(value)
}

function formatQuotaPair(used: number, limit: number): string {
  return `${formatNumber(used)} / ${formatNumber(limit)}`
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

type UserConsoleViewKey = 'dashboard' | 'tokens' | 'tokenDetail'

function resolveUserConsoleView(route: ConsoleRoute): UserConsoleViewKey {
  if (route.name === 'token') return 'tokenDetail'
  if (route.section === 'tokens') return 'tokens'
  return 'dashboard'
}

function resolveUserConsoleIdentityName(profile: Profile | null): string | null {
  const primary = profile?.userDisplayName?.trim()
  if (primary) return primary
  const fallback = profile?.displayName?.trim()
  if (fallback) return fallback
  return null
}

function resolveUserConsoleProviderLabel(
  provider: Profile['userProvider'] | undefined,
  providers: { linuxdo: string },
): string | null {
  if (provider === 'linuxdo') return providers.linuxdo
  return null
}

function resolveFallbackLogoutTarget(
  locationLike: Pick<Location, 'pathname' | 'search'>,
): string {
  const pathname = locationLike.pathname?.trim() || '/'
  const normalizedPath = pathname.startsWith('/') ? pathname : `/${pathname}`
  return `${normalizedPath}${locationLike.search || ''}`
}

type LogoutTargetProbe = (
  input: RequestInfo | URL,
  init?: RequestInit,
) => Promise<Pick<Response, 'ok' | 'status' | 'redirected' | 'url'>>

function isConsoleEntryPath(pathname: string): boolean {
  const normalizedPath = normalizeUserConsolePathname(pathname)
  return normalizedPath === '/console'
    || normalizedPath === '/console/dashboard'
    || normalizedPath === '/console/tokens'
    || normalizedPath.startsWith('/console/tokens/')
}

function isPublicHomePath(pathname: string): boolean {
  return pathname === '/' || pathname === '/index.html'
}

function resolveLogoutTargetFromProbeResponse(
  response: Pick<Response, 'ok' | 'status' | 'redirected' | 'url'>,
  fallbackTarget: string,
): string | null {
  if (response.redirected && typeof response.url === 'string' && response.url.length > 0) {
    const redirectedPath = new URL(response.url, 'https://codex.invalid').pathname
    return isPublicHomePath(redirectedPath) ? '/' : fallbackTarget
  }

  if (response.ok || (response.status >= 300 && response.status < 400)) {
    return '/'
  }

  return null
}

async function resolvePostLogoutTarget(
  locationLike: Pick<Location, 'pathname' | 'search'>,
  probeHome: LogoutTargetProbe = fetch,
): Promise<string> {
  const fallbackTarget = resolveFallbackLogoutTarget(locationLike)

  try {
    const response = await probeHome('/', {
      method: 'HEAD',
      credentials: 'same-origin',
    })
    const headTarget = resolveLogoutTargetFromProbeResponse(response, fallbackTarget)
    if (headTarget != null) {
      return headTarget
    }

    if (!response.ok) {
      const getResponse = await probeHome('/', {
        method: 'GET',
        credentials: 'same-origin',
      })
      const getTarget = resolveLogoutTargetFromProbeResponse(getResponse, fallbackTarget)
      if (getTarget != null) {
        return getTarget
      }
    }
  } catch {
    // Fall back to the current console entry when the public home is unavailable.
  }

  return fallbackTarget
}

function shouldRedirectToLogoutTarget(
  locationLike: Pick<Location, 'pathname' | 'search'>,
  logoutTarget: string,
): boolean {
  return resolveFallbackLogoutTarget(locationLike) !== logoutTarget
}

async function performUserLogoutFlow({
  logoutRequest,
  abortActiveConsoleLoads,
  redirectAfterLogout,
}: {
  logoutRequest: () => Promise<void>
  abortActiveConsoleLoads: () => void
  redirectAfterLogout: () => Promise<boolean> | Promise<void> | boolean | void
}): Promise<void> {
  await logoutRequest()
  abortActiveConsoleLoads()
  await redirectAfterLogout()
}

function applyLoggedOutConsoleReset({
  clearSensitiveConsoleState,
  setShowLoggedOutState,
}: {
  clearSensitiveConsoleState: () => void
  setShowLoggedOutState: (value: boolean) => void
}): void {
  setShowLoggedOutState(false)
  clearSensitiveConsoleState()
}

function resetActiveProbeUiState(
  currentRunId: number,
  abortActiveProbeRun: () => void,
): {
  nextRunId: number
  mcpProbe: ProbeButtonModel
  apiProbe: ProbeButtonModel
} {
  abortActiveProbeRun()
  return createClearedProbeUiState(currentRunId)
}

function createClearedProbeUiState(currentRunId: number): {
  nextRunId: number
  mcpProbe: ProbeButtonModel
  apiProbe: ProbeButtonModel
} {
  return {
    nextRunId: currentRunId + 1,
    mcpProbe: createProbeButtonModel(BASE_MCP_PROBE_STEP_COUNT),
    apiProbe: createProbeButtonModel(BASE_API_PROBE_STEP_COUNT),
  }
}

function toLoggedOutConsoleProfile(profile: Profile | null | undefined): Profile {
  return {
    displayName: profile?.isAdmin === true ? profile.displayName ?? null : null,
    isAdmin: profile?.isAdmin === true,
    forwardAuthEnabled: profile?.forwardAuthEnabled ?? false,
    builtinAuthEnabled: profile?.builtinAuthEnabled ?? false,
    allowRegistration: profile?.allowRegistration ?? false,
    userLoggedIn: false,
    userProvider: null,
    userDisplayName: null,
    userAvatarUrl: null,
  }
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

function shouldRenderLandingGuide(route: ConsoleRoute, tokenCount: number): boolean {
  return route.name === 'landing' && tokenCount === 1
}

function resolveGuideTokenId(route: ConsoleRoute, tokens: UserTokenSummary[]): string | null {
  if (route.name === 'token') {
    return route.id
  }
  if (tokens.length === 1) {
    return tokens[0].tokenId
  }
  return null
}

function resolveGuideToken(route: ConsoleRoute, tokens: UserTokenSummary[]): string {
  const guideTokenId = resolveGuideTokenId(route, tokens)
  return guideTokenId ? tokenLabel(guideTokenId) : 'th-xxxx-xxxxxxxxxxxx'
}

function resolveGuideRevealContextKey(route: ConsoleRoute, tokens: UserTokenSummary[]): string | null {
  const guideTokenId = resolveGuideTokenId(route, tokens)
  if (!guideTokenId) return null
  if (route.name === 'token') {
    return `token:${route.id}`
  }
  return `landing:${route.section ?? 'landing'}:${tokens.map((token) => token.tokenId).join(',')}`
}

function isActiveGuideRevealContext(revealedContextKey: string | null, currentContextKey: string | null): boolean {
  return revealedContextKey != null && currentContextKey != null && revealedContextKey === currentContextKey
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

function compactUtcTimestamp(timestamp: number): string {
  return new Date(timestamp)
    .toISOString()
    .replace(/\.\d{3}Z$/, 'z')
    .replace(/[-:]/g, '')
    .toLowerCase()
}

function randomBase36Fragment(random: () => number, length: number): string {
  let fragment = ''
  while (fragment.length < length) {
    fragment += Math.floor(random() * 36).toString(36)
  }
  return fragment.slice(0, length)
}

function splitProbeIdentityWords(value: string): string[] {
  return value
    .trim()
    .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
    .split(/[^A-Za-z0-9]+/)
    .filter(Boolean)
    .map((word) => word.toLowerCase())
}

function slugifyProbeIdentityPart(value: string): string {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
}

function hashProbeIdentityHex(value: string): string {
  let hash = 0x811c9dc5
  for (const ch of value) {
    hash ^= ch.charCodeAt(0)
    hash = Math.imul(hash, 0x01000193) >>> 0
  }
  return hash.toString(16).padStart(8, '0')
}

function buildPseudoUuid(runSignature: string, fieldName: string, counter: number): string {
  const hex = [
    hashProbeIdentityHex(`${runSignature}:${fieldName}:a:${counter}`),
    hashProbeIdentityHex(`${runSignature}:${fieldName}:b:${counter}`),
    hashProbeIdentityHex(`${runSignature}:${fieldName}:c:${counter}`),
    hashProbeIdentityHex(`${runSignature}:${fieldName}:d:${counter}`),
  ].join('')

  return [
    hex.slice(0, 8),
    hex.slice(8, 12),
    `4${hex.slice(13, 16)}`,
    `a${hex.slice(17, 20)}`,
    hex.slice(20, 32),
  ].join('-')
}

function createMcpProbeIdentityGenerator(
  options: McpProbeIdentityGeneratorOptions = {},
): McpProbeIdentityGenerator {
  const now = options.now ?? Date.now()
  const random = options.random ?? Math.random
  const runSignature = `ucp-${compactUtcTimestamp(now)}-${randomBase36Fragment(random, 6)}`
  let requestCounter = 0
  let identifierCounter = 0

  return {
    runSignature,
    nextRequestId: (kind: string, toolName?: string): string => {
      requestCounter += 1
      const requestParts = [
        'req',
        slugifyProbeIdentityPart(kind) || 'step',
        toolName ? slugifyProbeIdentityPart(toolName) : '',
        runSignature,
        requestCounter.toString(36).padStart(2, '0'),
      ].filter(Boolean)
      return requestParts.join('-')
    },
    nextIdentifier: (fieldName: string): string => {
      identifierCounter += 1
      const normalized = fieldName.trim()
      const slug = slugifyProbeIdentityPart(normalized) || 'id'
      const serial = identifierCounter.toString(36).padStart(2, '0')
      if (normalized.toLowerCase().includes('uuid')) {
        return buildPseudoUuid(runSignature, normalized, identifierCounter)
      }
      if (normalized.toLowerCase().includes('session')) {
        return `sess_${runSignature}_${serial}`
      }
      if (normalized.toLowerCase().includes('request')) {
        return `req_${runSignature}_${serial}`
      }
      if (normalized.toLowerCase().includes('trace')) {
        return `trace_${runSignature}_${serial}`
      }
      if (normalized.toLowerCase().includes('cursor')) {
        return `cursor_${runSignature}_${serial}`
      }
      return `${slug}_${runSignature}_${serial}`
    },
  }
}

function isIdentifierLikePropertyName(propertyName: string): boolean {
  const words = splitProbeIdentityWords(propertyName)
  if (words.length === 0) return false

  if (words.includes('uuid')) {
    return true
  }

  const lastWord = words[words.length - 1]
  return lastWord === 'id' || lastWord === 'request' || lastWord === 'session' || lastWord === 'trace' || lastWord === 'cursor'
}

function canonicalMcpProbeToolName(toolName: string): string {
  const trimmed = toolName.trim()
  const normalized = trimmed.toLowerCase().replaceAll('_', '-')

  if (normalized.startsWith('tavily-')) {
    return normalized
  }

  return trimmed
}

function isBillableMcpProbeTool(toolName: string): boolean {
  return canonicalMcpProbeToolName(toolName).startsWith('tavily-')
}

function firstSchemaRecord(value: unknown): Record<string, unknown> | null {
  if (Array.isArray(value)) {
    for (const item of value) {
      const record = asRecord(item)
      if (record) return record
    }
    return null
  }
  return asRecord(value)
}

function extractAdvertisedMcpToolSchema(tool: Record<string, unknown>): Record<string, unknown> | null {
  return firstSchemaRecord(tool.inputSchema)
    ?? firstSchemaRecord(tool.input_schema)
    ?? firstSchemaRecord(tool.parameters)
    ?? firstSchemaRecord(tool.schema)
}

function mcpToolProbeArguments(toolName: string): Record<string, unknown> | null {
  switch (canonicalMcpProbeToolName(toolName)) {
    case 'tavily-search':
      return {
        query: 'health check',
        search_depth: 'basic',
      }
    case 'tavily-extract':
      return {
        urls: ['https://example.com'],
      }
    case 'tavily-crawl':
    case 'tavily-map':
      return {
        url: 'https://example.com',
        max_depth: 1,
        limit: 1,
      }
    case 'tavily-research':
      return {
        input: 'health check',
        model: 'mini',
      }
    default:
      return null
  }
}

function schemaType(schema: Record<string, unknown>): string | null {
  const directType = schema.type
  if (typeof directType === 'string' && directType.length > 0) return directType
  if (Array.isArray(directType)) {
    for (const item of directType) {
      if (typeof item === 'string' && item !== 'null' && item.length > 0) return item
    }
  }
  if (schema.properties || schema.required) return 'object'
  if (schema.items) return 'array'
  return null
}

function schemaExampleValue(
  schema: Record<string, unknown>,
  propertyName: string,
  identity: McpProbeIdentityGenerator | null,
  depth = 0,
): unknown | undefined {
  if (depth > 4) return undefined
  if ('const' in schema) return schema.const
  if ('default' in schema) return schema.default

  const examples = Array.isArray(schema.examples) ? schema.examples : []
  for (const example of examples) {
    if (example !== undefined) return example
  }

  const enumValues = Array.isArray(schema.enum) ? schema.enum : []
  for (const value of enumValues) {
    if (value !== undefined) return value
  }

  for (const key of ['oneOf', 'anyOf', 'allOf'] as const) {
    const variants = Array.isArray(schema[key]) ? schema[key] : []
    for (const variant of variants) {
      const variantSchema = asRecord(variant)
      if (!variantSchema) continue
      const synthesized = schemaExampleValue(variantSchema, propertyName, identity, depth + 1)
      if (synthesized !== undefined) return synthesized
    }
  }

  const lowerName = propertyName.toLowerCase()
  switch (schemaType(schema)) {
    case 'boolean':
      return false
    case 'integer':
    case 'number':
      if (
        lowerName.includes('limit')
        || lowerName.includes('depth')
        || lowerName.includes('breadth')
        || lowerName.includes('count')
        || lowerName.includes('page')
        || lowerName.includes('max')
      ) {
        return 1
      }
      return typeof schema.minimum === 'number' ? schema.minimum : 0
    case 'string':
      if ((schema.format === 'uuid' || isIdentifierLikePropertyName(propertyName)) && identity) {
        return identity.nextIdentifier(propertyName)
      }
      if (
        schema.format === 'uri'
        || schema.format === 'url'
        || lowerName.includes('url')
        || lowerName.includes('uri')
      ) {
        return 'https://example.com'
      }
      if (lowerName.includes('country')) return 'United States'
      if (lowerName.includes('id')) return 'probe-id'
      return 'health check'
    case 'array': {
      const itemSchema = asRecord(schema.items)
      const itemValue = itemSchema ? schemaExampleValue(itemSchema, propertyName, identity, depth + 1) : undefined
      return itemValue === undefined ? [] : [itemValue]
    }
    case 'object': {
      const properties = asRecord(schema.properties)
      const required = Array.isArray(schema.required)
        ? schema.required.filter((value): value is string => typeof value === 'string' && value.length > 0)
        : []
      const value: Record<string, unknown> = {}
      for (const key of required) {
        const childSchema = properties ? asRecord(properties[key]) : null
        if (!childSchema) return undefined
        const childValue = schemaExampleValue(childSchema, key, identity, depth + 1)
        if (childValue === undefined) return undefined
        value[key] = childValue
      }
      return value
    }
    default:
      return undefined
  }
}

function synthesizeMcpToolProbeArguments(
  inputSchema: Record<string, unknown> | null,
  identity: McpProbeIdentityGenerator,
): unknown | null {
  if (!inputSchema) return null
  const synthesized = schemaExampleValue(inputSchema, 'arguments', identity)
  return synthesized === undefined ? null : synthesized
}

function extractAdvertisedMcpTools(payload: unknown): AdvertisedMcpTool[] {
  const result = asRecord(asRecord(payload)?.result)
  const tools = Array.isArray(result?.tools) ? result.tools : []
  const uniqueByRequestName = new Set<string>()
  const discoveredTools: AdvertisedMcpTool[] = []

  for (const tool of tools) {
    const toolRecord = asRecord(tool)
    const rawName = typeof toolRecord?.name === 'string' ? toolRecord.name : null
    if (!rawName || rawName.trim().length === 0) continue
    const trimmedName = rawName.trim()
    const canonicalName = canonicalMcpProbeToolName(trimmedName)
    if (canonicalName.length === 0 || uniqueByRequestName.has(trimmedName)) continue
    uniqueByRequestName.add(trimmedName)
    discoveredTools.push({
      requestName: trimmedName,
      displayName: canonicalName,
      inputSchema: toolRecord ? extractAdvertisedMcpToolSchema(toolRecord) : null,
    })
  }

  return discoveredTools
}

function buildMcpProbeStepDefinitions(
  probeText: McpProbeText,
): McpProbeStepDefinition[] {
  return [
    {
      id: 'mcp-initialize',
      label: probeText.steps.mcpInitialize,
      billable: false,
      run: async (token: string, context: McpProbeRunContext): Promise<McpProbeStepResult | null> => {
        const response = await probeMcpInitialize(token, {
          requestId: context.identity.nextRequestId('initialize'),
          protocolVersion: context.protocolVersion,
          clientVersion: context.clientVersion,
          signal: context.signal,
        })
        const error = envelopeError(response.payload)
        if (error) throw new Error(error)
        context.protocolVersion = response.negotiatedProtocolVersion ?? context.protocolVersion
        context.sessionId = response.sessionId ?? context.sessionId
        return null
      },
    },
    {
      id: 'mcp-initialized',
      label: probeText.steps.mcpInitialized,
      billable: false,
      run: async (token: string, context: McpProbeRunContext): Promise<McpProbeStepResult | null> => {
        const response = await probeMcpInitialized(token, {
          protocolVersion: context.protocolVersion,
          sessionId: context.sessionId,
          signal: context.signal,
        })
        context.sessionId = response.sessionId ?? context.sessionId
        return null
      },
    },
    {
      id: 'mcp-ping',
      label: probeText.steps.mcpPing,
      billable: false,
      run: async (token: string, context: McpProbeRunContext): Promise<McpProbeStepResult | null> => {
        const response = await probeMcpPing(token, {
          requestId: context.identity.nextRequestId('ping'),
          protocolVersion: context.protocolVersion,
          sessionId: context.sessionId,
          signal: context.signal,
        })
        const error = envelopeError(response.payload)
        if (error) throw new Error(error)
        context.sessionId = response.sessionId ?? context.sessionId
        context.protocolVersion = response.negotiatedProtocolVersion ?? context.protocolVersion
        return null
      },
    },
    {
      id: 'mcp-tools-list',
      label: probeText.steps.mcpToolsList,
      run: async (token: string, context: McpProbeRunContext): Promise<McpProbeStepResult | null> => {
        const response = await probeMcpToolsList(token, {
          requestId: context.identity.nextRequestId('tools-list'),
          protocolVersion: context.protocolVersion,
          sessionId: context.sessionId,
          signal: context.signal,
        })
        const error = envelopeError(response.payload)
        if (error) throw new Error(error)
        context.sessionId = response.sessionId ?? context.sessionId
        context.protocolVersion = response.negotiatedProtocolVersion ?? context.protocolVersion
        const discoveredTools = extractAdvertisedMcpTools(response.payload)
        if (discoveredTools.length === 0) {
          throw new Error(probeText.errors.missingAdvertisedTools)
        }
        return { discoveredTools }
      },
    },
  ]
}

function buildMcpToolCallProbeStepDefinitions(
  probeText: McpProbeText,
  tools: Array<string | AdvertisedMcpTool>,
): McpProbeStepDefinition[] {
  const toolEntries: AdvertisedMcpTool[] = []
  const seenRequestNames = new Set<string>()

  for (const tool of tools) {
    const requestName = typeof tool === 'string' ? tool.trim() : tool.requestName.trim()
    const displayName = typeof tool === 'string'
      ? canonicalMcpProbeToolName(requestName)
      : canonicalMcpProbeToolName(tool.displayName)
    if (displayName.length === 0 || seenRequestNames.has(requestName)) continue
    seenRequestNames.add(requestName)
    toolEntries.push({
      requestName,
      displayName,
      inputSchema: typeof tool === 'string' ? null : tool.inputSchema,
    })
  }

  return toolEntries.flatMap(({ requestName, displayName, inputSchema }) => {
    return [{
      id: `mcp-tool-call:${requestName}`,
      label: formatTemplate(probeText.steps.mcpToolCall, { tool: requestName }),
      billable: isBillableMcpProbeTool(displayName),
      run: async (token: string, context: McpProbeRunContext): Promise<McpProbeStepResult | null> => {
        const safeProbeTarget = isBillableMcpProbeTool(displayName)
        const probeArguments = mcpToolProbeArguments(displayName)
          ?? (safeProbeTarget ? synthesizeMcpToolProbeArguments(inputSchema, context.identity) : null)

        if (probeArguments == null) {
          return {
            detail: formatTemplate(probeText.skippedProbeFixture, { tool: requestName }),
            stepState: 'skipped',
          }
        }

        const response = await probeMcpToolsCall(token, requestName, probeArguments, {
          requestId: context.identity.nextRequestId('tools-call', requestName),
          protocolVersion: context.protocolVersion,
          sessionId: context.sessionId,
          signal: context.signal,
        })
        const error = envelopeError(response.payload) ?? getMcpProbeResultError(response.payload)
        if (error) throw new Error(error)
        context.sessionId = response.sessionId ?? context.sessionId
        context.protocolVersion = response.negotiatedProtocolVersion ?? context.protocolVersion
        return null
      },
    }]
  })
}

function buildApiProbeStepDefinitions(
  probeText: ApiProbeText,
): ApiProbeStepDefinition[] {
  return [
    {
      id: 'api-search',
      label: probeText.steps.apiSearch,
      run: async (token: string, context: { requestId: string | null, signal?: AbortSignal }): Promise<string | null> => {
        const payload = await probeApiTavilySearch(token, {
          query: 'health check',
          max_results: 1,
          search_depth: 'basic',
          include_answer: false,
          include_raw_content: false,
          include_images: false,
        }, context.signal)
        const error = envelopeError(payload)
        if (error) throw new Error(error)
        return null
      },
    },
    {
      id: 'api-extract',
      label: probeText.steps.apiExtract,
      run: async (token: string, context: { requestId: string | null, signal?: AbortSignal }): Promise<string | null> => {
        const payload = await probeApiTavilyExtract(token, {
          urls: ['https://example.com'],
          include_images: false,
        }, context.signal)
        const error = envelopeError(payload)
        if (error) throw new Error(error)
        return null
      },
    },
    {
      id: 'api-crawl',
      label: probeText.steps.apiCrawl,
      run: async (token: string, context: { requestId: string | null, signal?: AbortSignal }): Promise<string | null> => {
        const payload = await probeApiTavilyCrawl(token, {
          url: 'https://example.com',
          max_depth: 1,
          limit: 1,
        }, context.signal)
        const error = envelopeError(payload)
        if (error) throw new Error(error)
        return null
      },
    },
    {
      id: 'api-map',
      label: probeText.steps.apiMap,
      run: async (token: string, context: { requestId: string | null, signal?: AbortSignal }): Promise<string | null> => {
        const payload = await probeApiTavilyMap(token, {
          url: 'https://example.com',
          max_depth: 1,
          limit: 1,
        }, context.signal)
        const error = envelopeError(payload)
        if (error) throw new Error(error)
        return null
      },
    },
    {
      id: 'api-research',
      label: probeText.steps.apiResearch,
      run: async (token: string, context: { requestId: string | null, signal?: AbortSignal }): Promise<string | null> => {
        const payload = await probeApiTavilyResearch(token, {
          input: 'health check',
          model: 'mini',
          citation_format: 'numbered',
        }, context.signal)
        const error = envelopeError(payload)
        if (error) throw new Error(error)
        const requestId = getResearchRequestId(payload)
        if (!requestId) {
          throw new Error(probeText.errors.missingRequestId)
        }
        return requestId
      },
    },
    {
      id: 'api-research-result',
      label: probeText.steps.apiResearchResult,
      run: async (token: string, context: { requestId: string | null, signal?: AbortSignal }): Promise<string | null> => {
        if (!context.requestId) {
          throw new Error(probeText.errors.missingRequestId)
        }
        const payload = await probeApiTavilyResearchResult(token, context.requestId, context.signal)
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
            throw new Error(probeText.errors.researchFailed)
          }
          if (
            normalized === 'pending'
            || normalized === 'processing'
            || normalized === 'running'
            || normalized === 'in_progress'
            || normalized === 'queued'
          ) {
            return probeText.researchPendingAccepted
          }
          if (
            normalized === 'completed'
            || normalized === 'success'
            || normalized === 'succeeded'
            || normalized === 'done'
          ) {
            return formatTemplate(probeText.researchStatus, { status: normalized })
          }
          throw new Error(
            formatTemplate(probeText.errors.researchUnexpectedStatus, {
              status: normalized,
            }),
          )
        }
        return null
      },
    },
  ]
}

function nextRunningMcpProbeModel(
  previous: ProbeButtonModel,
  stepDefinitions: readonly McpProbeStepDefinition[],
  completed: number,
): ProbeButtonModel {
  return {
    ...previous,
    state: 'running',
    completed,
    total: stepDefinitions.length,
  }
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
  const [versionState, setVersionState] = useState<
    { status: 'loading' } | { status: 'error' } | { status: 'ready'; value: VersionInfo | null }
  >({ status: 'loading' })
  const [route, setRoute] = useState<ConsoleRoute>(() => parseUserConsolePath(window.location.pathname || ''))
  const [detail, setDetail] = useState<UserTokenSummary | null>(null)
  const [detailLogs, setDetailLogs] = useState<PublicTokenLog[]>([])
  const [detailLogsPushIssue, setDetailLogsPushIssue] = useState<DetailLogsPushIssueCode | null>(null)
  const [loading, setLoading] = useState(true)
  const [detailLoading, setDetailLoading] = useState(false)
  const [todayWindow, setTodayWindow] = useState(() => createBrowserTodayWindow())

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setTodayWindow(createBrowserTodayWindow())
    }, millisecondsUntilNextBrowserDayBoundary())
    return () => window.clearTimeout(timer)
  }, [todayWindow.todayEnd])
  const [error, setError] = useState<string | null>(null)
  const [isLoggingOut, setIsLoggingOut] = useState(false)
  const [copyState, setCopyState] = useState<Record<string, TokenSecretCopyState>>({})
  const [tokenSecretTokenId, setTokenSecretTokenId] = useState<string | null>(null)
  const [tokenSecretVisible, setTokenSecretVisible] = useState(false)
  const [tokenSecretValue, setTokenSecretValue] = useState<string | null>(null)
  const [tokenSecretLoading, setTokenSecretLoading] = useState(false)
  const [tokenSecretError, setTokenSecretError] = useState<string | null>(null)
  const [activeGuide, setActiveGuide] = useState<GuideKey>('codex')
  const [isMobileGuide, setIsMobileGuide] = useState(false)
  const [mcpProbe, setMcpProbe] = useState<ProbeButtonModel>(() => createProbeButtonModel(BASE_MCP_PROBE_STEP_COUNT))
  const [apiProbe, setApiProbe] = useState<ProbeButtonModel>(() => createProbeButtonModel(BASE_API_PROBE_STEP_COUNT))
  const [probeBubble, setProbeBubble] = useState<ProbeBubbleModel | null>(null)
  const [manualCopyBubble, setManualCopyBubble] = useState<ManualCopyBubbleState | null>(null)
  const [showLoggedOutState, setShowLoggedOutState] = useState(false)
  const [revealedGuideContextKey, setRevealedGuideContextKey] = useState<string | null>(null)
  const [guideTokenValue, setGuideTokenValue] = useState<string | null>(null)
  const [guideTokenLoading, setGuideTokenLoading] = useState(false)
  const [guideTokenError, setGuideTokenError] = useState<string | null>(null)
  const tokenSecretCacheRef = useRef<Map<string, string>>(new Map())
  const tokenSecretCacheTimerRef = useRef<Map<string, number>>(new Map())
  const tokenSecretWarmTimerRef = useRef<Map<string, number>>(new Map())
  const tokenSecretWarmAbortRef = useRef<Map<string, AbortController>>(new Map())
  const tokenSecretRequestRef = useRef<Map<string, Promise<string>>>(new Map())
  const tokenSecretRequestAbortRef = useRef<Map<string, AbortController>>(new Map())
  const probeRunIdRef = useRef(0)
  const tokenSecretRunIdRef = useRef(0)
  const guideTokenRunIdRef = useRef(0)
  const baseLoadRunIdRef = useRef(0)
  const detailLoadRunIdRef = useRef(0)
  const baseLoadAbortRef = useRef<AbortController | null>(null)
  const detailLoadAbortRef = useRef<AbortController | null>(null)
  const probeAbortRef = useRef<AbortController | null>(null)
  const detailEventsRef = useRef<EventSource | null>(null)
  const pageRef = useRef<HTMLElement>(null)
  const dashboardSectionRef = useRef<HTMLElement | null>(null)
  const tokensSectionRef = useRef<HTMLElement | null>(null)
  const detailHeadingRef = useRef<HTMLHeadingElement | null>(null)
  const detailTokenFieldRef = useRef<HTMLInputElement | null>(null)
  const historyTraversalRef = useRef(false)
  const landingScrollBehaviorRef = useRef<ScrollBehavior>('auto')
  const shouldScrollLandingSectionRef = useRef(route.name === 'landing' && route.section !== null)
  const { viewportMode, contentMode, isCompactLayout } = useResponsiveModes(pageRef)

  const clearConsoleData = useCallback(() => {
    setDashboard(null)
    setTokens([])
    setDetail(null)
    setDetailLogs([])
    setDetailLogsPushIssue(null)
    setError(null)
  }, [])

  const clearProbeUi = useCallback(() => {
    const clearedProbeState = createClearedProbeUiState(probeRunIdRef.current)
    probeRunIdRef.current = clearedProbeState.nextRunId
    setMcpProbe(clearedProbeState.mcpProbe)
    setApiProbe(clearedProbeState.apiProbe)
    setProbeBubble(null)
    setManualCopyBubble(null)
  }, [])

  const abortActiveConsoleLoads = useCallback(() => {
    baseLoadRunIdRef.current += 1
    detailLoadRunIdRef.current += 1
    baseLoadAbortRef.current?.abort()
    detailLoadAbortRef.current?.abort()
    baseLoadAbortRef.current = null
    detailLoadAbortRef.current = null
    detailEventsRef.current?.close()
    detailEventsRef.current = null
    setDetailLogsPushIssue(null)
  }, [])

  const abortActiveProbeRun = useCallback(() => {
    probeAbortRef.current?.abort()
    probeAbortRef.current = null
  }, [])

  const abortAllPendingTokenSecretRequests = useCallback(() => {
    for (const controller of tokenSecretRequestAbortRef.current.values()) {
      controller.abort()
    }
    tokenSecretRequestAbortRef.current.clear()
    tokenSecretRequestRef.current.clear()
  }, [])

  const clearTokenSecretState = useCallback(() => {
    tokenSecretRunIdRef.current += 1
    setTokenSecretTokenId(null)
    setTokenSecretVisible(false)
    setTokenSecretValue(null)
    setTokenSecretLoading(false)
    setTokenSecretError(null)
    for (const timer of tokenSecretWarmTimerRef.current.values()) {
      window.clearTimeout(timer)
    }
    for (const timer of tokenSecretCacheTimerRef.current.values()) {
      window.clearTimeout(timer)
    }
    for (const controller of tokenSecretWarmAbortRef.current.values()) {
      controller.abort()
    }
    abortAllPendingTokenSecretRequests()
    tokenSecretWarmTimerRef.current.clear()
    tokenSecretCacheTimerRef.current.clear()
    tokenSecretWarmAbortRef.current.clear()
    tokenSecretCacheRef.current.clear()
  }, [abortAllPendingTokenSecretRequests])

  const clearGuideTokenState = useCallback(() => {
    guideTokenRunIdRef.current += 1
    setGuideTokenValue(null)
    setGuideTokenLoading(false)
    setGuideTokenError(null)
    setRevealedGuideContextKey(null)
  }, [])

  const clearSensitiveConsoleState = useCallback(() => {
    detailLoadRunIdRef.current += 1
    detailLoadAbortRef.current?.abort()
    detailLoadAbortRef.current = null
    detailEventsRef.current?.close()
    detailEventsRef.current = null
    setDetailLogsPushIssue(null)
    clearConsoleData()
    abortActiveProbeRun()
    clearProbeUi()
    clearTokenSecretState()
    clearGuideTokenState()
    setCopyState({})
  }, [abortActiveProbeRun, clearConsoleData, clearGuideTokenState, clearProbeUi, clearTokenSecretState])

  const redirectAfterLogoutIfNeeded = useCallback(async (
    locationLike: Pick<Location, 'pathname' | 'search'>,
    options: { showLoggedOutState?: boolean } = {},
  ) => {
    const { showLoggedOutState: nextShowLoggedOutState = true } = options
    setProfile((prev) => toLoggedOutConsoleProfile(prev))
    setShowLoggedOutState(nextShowLoggedOutState)
    clearSensitiveConsoleState()

    const logoutTarget = await resolvePostLogoutTarget(locationLike)
    if (shouldRedirectToLogoutTarget(locationLike, logoutTarget)) {
      window.location.href = logoutTarget
      return true
    }
    return false
  }, [clearSensitiveConsoleState])

  useEffect(() => {
    const syncRoute = () => {
      const nextPathname = window.location.pathname || ''
      const nextRoute = parseUserConsolePath(nextPathname)
      const normalizedPath = normalizeUserConsolePathname(nextPathname)
      const canonicalPath = userConsoleRouteToPath(nextRoute)
      if (normalizedPath !== canonicalPath && isConsoleEntryPath(nextPathname)) {
        window.history.replaceState(null, '', `${canonicalPath}${window.location.search}${window.location.hash}`)
      }
      if (nextRoute.name === 'landing' && nextRoute.section && !historyTraversalRef.current) {
        shouldScrollLandingSectionRef.current = true
      }
      setRoute(nextRoute)
      historyTraversalRef.current = false
    }

    const handlePopState = () => {
      historyTraversalRef.current = true
      syncRoute()
    }

    syncRoute()
    window.addEventListener('popstate', handlePopState)
    return () => {
      window.removeEventListener('popstate', handlePopState)
    }
  }, [])

  const reloadBase = useCallback(async (signal: AbortSignal, runId: number) => {
    try {
      const nextProfile = await fetchProfile(signal)
      if (signal.aborted || baseLoadRunIdRef.current !== runId) return
      setProfile(nextProfile)

      const availability = resolveUserConsoleAvailability(nextProfile)
      if (availability === 'logged_out') {
        applyLoggedOutConsoleReset({
          clearSensitiveConsoleState,
          setShowLoggedOutState,
        })
        return
      }
      setShowLoggedOutState(false)
      if (availability === 'disabled') {
        clearConsoleData()
        return
      }

      const [nextDashboard, nextTokens] = await Promise.all([
        fetchUserDashboard(todayWindow, signal),
        fetchUserTokens(todayWindow, signal),
      ])
      if (signal.aborted || baseLoadRunIdRef.current !== runId) return
      setDashboard(nextDashboard)
      setTokens(nextTokens)
      setError(null)
    } catch (err) {
      if (signal.aborted || baseLoadRunIdRef.current !== runId) return
      const message = err instanceof Error ? err.message : text.errors.load
      setError(message)
      if (errorStatus(err) === 401) {
        abortActiveConsoleLoads()
        await redirectAfterLogoutIfNeeded(window.location)
      }
    } finally {
      if (!signal.aborted && baseLoadRunIdRef.current === runId) {
        setLoading(false)
      }
    }
  }, [abortActiveConsoleLoads, clearConsoleData, clearSensitiveConsoleState, redirectAfterLogoutIfNeeded, text.errors.load, todayWindow])

  useEffect(() => {
    const controller = new AbortController()
    const runId = baseLoadRunIdRef.current + 1
    baseLoadRunIdRef.current = runId
    baseLoadAbortRef.current?.abort()
    baseLoadAbortRef.current = controller
    void reloadBase(controller.signal, runId)
    return () => {
      controller.abort()
      if (baseLoadAbortRef.current === controller) {
        baseLoadAbortRef.current = null
      }
    }
  }, [reloadBase])

  useEffect(() => {
    const controller = new AbortController()
    fetchVersion(controller.signal)
      .then((nextVersion) => {
        setVersionState({ status: 'ready', value: nextVersion })
      })
      .catch(() => {
        setVersionState({ status: 'error' })
      })
    return () => controller.abort()
  }, [])

  const consoleAvailability = resolveUserConsoleAvailability(profile)

  useEffect(() => {
    if (consoleAvailability !== 'enabled' || route.name !== 'token') {
      detailLoadRunIdRef.current += 1
      detailLoadAbortRef.current?.abort()
      detailLoadAbortRef.current = null
      setDetail(null)
      setDetailLogs([])
      setDetailLoading(false)
      return
    }
    setDetail(null)
    setDetailLogs([])
    setDetailLoading(true)
    const controller = new AbortController()
    const runId = detailLoadRunIdRef.current + 1
    detailLoadRunIdRef.current = runId
    detailLoadAbortRef.current?.abort()
    detailLoadAbortRef.current = controller
    Promise.all([
      fetchUserTokenDetail(route.id, todayWindow, controller.signal),
      fetchUserTokenLogs(route.id, 20, controller.signal),
    ])
      .then(([nextDetail, nextLogs]) => {
        if (controller.signal.aborted || detailLoadRunIdRef.current !== runId) return
        setDetail(nextDetail)
        setDetailLogs(nextLogs)
        setError(null)
      })
      .catch((err) => {
        if (controller.signal.aborted || detailLoadRunIdRef.current !== runId) return
        setDetail(null)
        setDetailLogs([])
        setError(err instanceof Error ? err.message : text.errors.detail)
        if (errorStatus(err) === 401) {
          abortActiveConsoleLoads()
          void redirectAfterLogoutIfNeeded(window.location)
        }
      })
      .finally(() => {
        if (!controller.signal.aborted && detailLoadRunIdRef.current === runId) {
          setDetailLoading(false)
        }
      })
    return () => {
      controller.abort()
      if (detailLoadAbortRef.current === controller) {
        detailLoadAbortRef.current = null
      }
    }
  }, [abortActiveConsoleLoads, consoleAvailability, redirectAfterLogoutIfNeeded, route, text.errors.detail, todayWindow])

  useEffect(() => {
    detailEventsRef.current?.close()
    detailEventsRef.current = null
    setDetailLogsPushIssue(null)

    if (consoleAvailability !== 'enabled' || route.name !== 'token' || typeof EventSource === 'undefined') {
      if (consoleAvailability === 'enabled' && route.name === 'token' && typeof EventSource === 'undefined') {
        setDetailLogsPushIssue('unsupported')
      }
      return
    }

    const url = buildUserTokenEventsUrl(route.id, todayWindow)
    const source = new EventSource(url)
    detailEventsRef.current = source

    const handleSnapshot = (event: MessageEvent<string>) => {
      try {
        const snapshot: UserTokenEventSnapshot = parseUserTokenEventSnapshot(event.data)
        setDetail(snapshot.token)
        setDetailLogs(snapshot.logs)
        setDetailLoading(false)
        setError(null)
        setDetailLogsPushIssue(null)
      } catch (err) {
        console.error('failed to parse user token SSE snapshot', err)
      }
    }

    const handleOpen = () => {
      setDetailLogsPushIssue(null)
    }

    const handleError = () => {
      const eventSourceCtor = window.EventSource
      const closedState = typeof eventSourceCtor === 'function' ? eventSourceCtor.CLOSED : 2
      if (source.readyState === closedState) {
        setDetailLogsPushIssue('closed')
        return
      }
      setDetailLogsPushIssue('reconnecting')
    }

    source.addEventListener('snapshot', handleSnapshot as EventListener)
    source.addEventListener('open', handleOpen as EventListener)
    source.onerror = handleError

    return () => {
      source.removeEventListener('snapshot', handleSnapshot as EventListener)
      source.removeEventListener('open', handleOpen as EventListener)
      source.close()
      source.onerror = null
      if (detailEventsRef.current === source) {
        detailEventsRef.current = null
      }
    }
  }, [consoleAvailability, route, todayWindow])

  useEffect(() => {
    const clearedProbeState = resetActiveProbeUiState(probeRunIdRef.current, abortActiveProbeRun)
    probeRunIdRef.current = clearedProbeState.nextRunId
    setMcpProbe(clearedProbeState.mcpProbe)
    setApiProbe(clearedProbeState.apiProbe)
    setProbeBubble(null)
    setManualCopyBubble(null)
  }, [abortActiveProbeRun, route.name === 'token' ? route.id : route.section ?? 'landing'])

  const abortPendingTokenSecretRequest = useCallback((tokenId: string) => {
    const controller = tokenSecretRequestAbortRef.current.get(tokenId)
    if (controller) {
      controller.abort()
      tokenSecretRequestAbortRef.current.delete(tokenId)
    }
    tokenSecretRequestRef.current.delete(tokenId)
  }, [])

  const handleLogout = useCallback(async () => {
    if (isLoggingOut || profile?.userLoggedIn !== true) return
    setIsLoggingOut(true)
    setError(null)

    try {
      await performUserLogoutFlow({
        logoutRequest: postUserLogout,
        abortActiveConsoleLoads,
        redirectAfterLogout: () => redirectAfterLogoutIfNeeded(window.location),
      })
      return
    } catch (err) {
      setError(formatTemplate(text.header.logoutFailed, {
        message: getProbeErrorMessage(err),
      }))
    } finally {
      setIsLoggingOut(false)
    }
  }, [
    abortActiveConsoleLoads,
    isLoggingOut,
    profile?.userLoggedIn,
    redirectAfterLogoutIfNeeded,
    text.header.logoutFailed,
  ])

  useEffect(() => {
    clearTokenSecretState()
  }, [clearTokenSecretState, consoleAvailability, route.name === 'token' ? route.id : route.name])

  useEffect(() => {
    return () => {
      for (const timer of tokenSecretWarmTimerRef.current.values()) {
        window.clearTimeout(timer)
      }
      for (const timer of tokenSecretCacheTimerRef.current.values()) {
        window.clearTimeout(timer)
      }
      for (const controller of tokenSecretWarmAbortRef.current.values()) {
        controller.abort()
      }
      abortAllPendingTokenSecretRequests()
    }
  }, [abortAllPendingTokenSecretRequests])

  useEffect(() => {
    guideTokenRunIdRef.current += 1
    setRevealedGuideContextKey(null)
    setGuideTokenValue(null)
    setGuideTokenLoading(false)
    setGuideTokenError(null)
  }, [
    consoleAvailability,
    route.name === 'token' ? route.id : `${route.section ?? 'landing'}:${tokens.map((token) => token.tokenId).join(',')}`,
  ])

  const clearCachedTokenSecret = useCallback((tokenId: string) => {
    const cacheTimer = tokenSecretCacheTimerRef.current.get(tokenId)
    if (cacheTimer != null) {
      window.clearTimeout(cacheTimer)
      tokenSecretCacheTimerRef.current.delete(tokenId)
    }
    tokenSecretCacheRef.current.delete(tokenId)
  }, [])

  const cacheTokenSecret = useCallback((tokenId: string, token: string) => {
    clearCachedTokenSecret(tokenId)
    tokenSecretCacheRef.current.set(tokenId, token)
    const timer = window.setTimeout(() => {
      tokenSecretCacheTimerRef.current.delete(tokenId)
      tokenSecretCacheRef.current.delete(tokenId)
    }, USER_CONSOLE_SECRET_CACHE_TTL_MS)
    tokenSecretCacheTimerRef.current.set(tokenId, timer)
  }, [clearCachedTokenSecret])

  const clearWarmTokenSecretTimer = useCallback((tokenId: string) => {
    const timer = tokenSecretWarmTimerRef.current.get(tokenId)
    if (timer != null) {
      window.clearTimeout(timer)
      tokenSecretWarmTimerRef.current.delete(tokenId)
    }
  }, [])

  const cancelWarmTokenSecret = useCallback((tokenId: string) => {
    clearWarmTokenSecretTimer(tokenId)
    const controller = tokenSecretWarmAbortRef.current.get(tokenId)
    if (controller) {
      tokenSecretWarmAbortRef.current.delete(tokenId)
      abortPendingTokenSecretRequest(tokenId)
    }
  }, [abortPendingTokenSecretRequest, clearWarmTokenSecretTimer])

  const commitWarmTokenSecret = useCallback((tokenId: string) => {
    clearWarmTokenSecretTimer(tokenId)
    tokenSecretWarmAbortRef.current.delete(tokenId)
  }, [clearWarmTokenSecretTimer])

  const resolveTokenSecret = useCallback(async (tokenId: string, signal?: AbortSignal) => {
    const revealedToken =
      route.name === 'token' && route.id === tokenId && tokenSecretTokenId === tokenId
        ? tokenSecretValue
        : null
    if (revealedToken) {
      return revealedToken
    }
    const cachedToken = tokenSecretCacheRef.current.get(tokenId)
    if (cachedToken) {
      return cachedToken
    }
    const pending = tokenSecretRequestRef.current.get(tokenId)
    if (pending) {
      return await pending
    }

    const requestController = new AbortController()
    tokenSecretRequestAbortRef.current.set(tokenId, requestController)
    const forwardAbort = () => requestController.abort()
    if (signal) {
      if (signal.aborted) {
        requestController.abort()
      } else {
        signal.addEventListener('abort', forwardAbort, { once: true })
      }
    }
    const requestRunId = tokenSecretRunIdRef.current
    const request = fetchUserTokenSecret(tokenId, requestController.signal)
      .then(({ token }) => {
        if (!requestController.signal.aborted && requestRunId === tokenSecretRunIdRef.current) {
          cacheTokenSecret(tokenId, token)
        }
        return token
      })
      .finally(() => {
        if (signal) {
          signal.removeEventListener('abort', forwardAbort)
        }
        if (tokenSecretRequestRef.current.get(tokenId) === request) {
          tokenSecretRequestRef.current.delete(tokenId)
        }
        if (tokenSecretRequestAbortRef.current.get(tokenId) === requestController) {
          tokenSecretRequestAbortRef.current.delete(tokenId)
        }
      })

    tokenSecretRequestRef.current.set(tokenId, request)
    return await request
  }, [cacheTokenSecret, route, tokenSecretTokenId, tokenSecretValue])

  const shouldPrewarmTokenCopy = useMemo(() => shouldPrewarmSecretCopy(), [])

  const warmTokenSecret = useCallback((tokenId: string) => {
    if (consoleAvailability !== 'enabled' || !shouldPrewarmTokenCopy) return
    clearWarmTokenSecretTimer(tokenId)
    if (tokenSecretCacheRef.current.has(tokenId) || tokenSecretRequestRef.current.has(tokenId)) return
    const controller = new AbortController()
    tokenSecretWarmAbortRef.current.set(tokenId, controller)
    void resolveTokenSecret(tokenId, controller.signal)
      .then((token) => {
        if (tokenSecretWarmAbortRef.current.get(tokenId) !== controller) return
        cacheTokenSecret(tokenId, token)
      })
      .catch(() => undefined)
      .finally(() => {
        if (tokenSecretWarmAbortRef.current.get(tokenId) === controller) {
          tokenSecretWarmAbortRef.current.delete(tokenId)
        }
      })
  }, [cacheTokenSecret, clearWarmTokenSecretTimer, consoleAvailability, resolveTokenSecret, shouldPrewarmTokenCopy])

  const scheduleWarmTokenSecret = useCallback((tokenId: string) => {
    if (consoleAvailability !== 'enabled' || !shouldPrewarmTokenCopy) return
    if (tokenSecretCacheRef.current.has(tokenId) || tokenSecretRequestRef.current.has(tokenId)) return
    clearWarmTokenSecretTimer(tokenId)
    const timer = window.setTimeout(() => {
      tokenSecretWarmTimerRef.current.delete(tokenId)
      void warmTokenSecret(tokenId)
    }, USER_CONSOLE_SECRET_PREWARM_DELAY_MS)
    tokenSecretWarmTimerRef.current.set(tokenId, timer)
  }, [clearWarmTokenSecretTimer, consoleAvailability, shouldPrewarmTokenCopy, warmTokenSecret])

  const revealDetailTokenForManualCopy = useCallback((tokenId: string, token: string) => {
    if (route.name !== 'token' || route.id !== tokenId) return false
    setTokenSecretTokenId(tokenId)
    setTokenSecretValue(token)
    setTokenSecretVisible(true)
    setTokenSecretLoading(false)
    setTokenSecretError(null)
    window.requestAnimationFrame(() => {
      selectAllReadonlyText(detailTokenFieldRef.current)
    })
    return true
  }, [route])

  const copyToken = useCallback(async (tokenId: string, anchorEl?: HTMLElement | null) => {
    setManualCopyBubble(null)
    commitWarmTokenSecret(tokenId)
    try {
      const inlineToken =
        route.name === 'token' && route.id === tokenId && tokenSecretTokenId === tokenId && tokenSecretValue != null
          ? tokenSecretValue
          : null
      const cachedToken = inlineToken ?? tokenSecretCacheRef.current.get(tokenId)
      const token = cachedToken ?? await resolveTokenSecret(tokenId)
      const result = await copyText(token, cachedToken ? { preferExecCommand: true } : undefined)
      if (cachedToken && tokenId !== tokenSecretTokenId) {
        clearCachedTokenSecret(tokenId)
      }
      if (!result.ok) {
        if (!revealDetailTokenForManualCopy(tokenId, token) && anchorEl) {
          setManualCopyBubble({ anchorEl, value: token })
        }
        setCopyState((prev) => ({ ...prev, [tokenId]: 'error' }))
        window.setTimeout(() => {
          setCopyState((prev) => ({ ...prev, [tokenId]: 'idle' }))
        }, 1800)
        return
      }
      setManualCopyBubble(null)
      setCopyState((prev) => ({ ...prev, [tokenId]: 'copied' }))
    } catch {
      setCopyState((prev) => ({ ...prev, [tokenId]: 'error' }))
    }
    window.setTimeout(() => {
      setCopyState((prev) => ({ ...prev, [tokenId]: 'idle' }))
    }, 1800)
  }, [clearCachedTokenSecret, commitWarmTokenSecret, resolveTokenSecret, revealDetailTokenForManualCopy, route, tokenSecretTokenId, tokenSecretValue])

  const toggleTokenSecretVisibility = useCallback(async () => {
    if (route.name !== 'token') return
    if (tokenSecretVisible) {
      tokenSecretRunIdRef.current += 1
      setTokenSecretTokenId(null)
      setTokenSecretVisible(false)
      setTokenSecretValue(null)
      setTokenSecretLoading(false)
      setTokenSecretError(null)
      return
    }
    if (tokenSecretLoading) return

    const runId = tokenSecretRunIdRef.current + 1
    tokenSecretRunIdRef.current = runId
    setTokenSecretTokenId(route.id)
    setTokenSecretVisible(false)
    setTokenSecretValue(null)
    setTokenSecretLoading(true)
    setTokenSecretError(null)

    try {
      const secret = await fetchUserTokenSecret(route.id)
      if (tokenSecretRunIdRef.current !== runId) return
      setTokenSecretTokenId(route.id)
      setTokenSecretValue(secret.token)
      cacheTokenSecret(route.id, secret.token)
      setTokenSecretVisible(true)
    } catch (err) {
      if (tokenSecretRunIdRef.current !== runId) return
      setTokenSecretTokenId(route.id)
      setTokenSecretVisible(false)
      setTokenSecretValue(null)
      setTokenSecretError(formatTemplate(text.detail.tokenSecret.revealFailed, {
        message: getProbeErrorMessage(err),
      }))
    } finally {
      if (tokenSecretRunIdRef.current === runId) {
        setTokenSecretLoading(false)
      }
    }
  }, [route, text.detail.tokenSecret.revealFailed, tokenSecretLoading, tokenSecretVisible])

  const guideTokenId = useMemo(() => resolveGuideTokenId(route, tokens), [route, tokens])
  const maskedGuideToken = useMemo(() => resolveGuideToken(route, tokens), [route, tokens])
  const guideRevealContextKey = useMemo(() => resolveGuideRevealContextKey(route, tokens), [route, tokens])
  const guideTokenVisible =
    consoleAvailability === 'enabled'
    && guideTokenValue != null
    && isActiveGuideRevealContext(revealedGuideContextKey, guideRevealContextKey)

  const toggleGuideTokenVisibility = useCallback(async () => {
    if (!guideTokenId) return
    if (guideTokenVisible) {
      guideTokenRunIdRef.current += 1
      setRevealedGuideContextKey(null)
      setGuideTokenValue(null)
      setGuideTokenLoading(false)
      setGuideTokenError(null)
      return
    }
    if (guideTokenLoading) return

    const runId = guideTokenRunIdRef.current + 1
    guideTokenRunIdRef.current = runId
    setRevealedGuideContextKey(null)
    setGuideTokenValue(null)
    setGuideTokenLoading(true)
    setGuideTokenError(null)

    try {
      const secret = await resolveTokenSecret(guideTokenId)
      if (guideTokenRunIdRef.current !== runId) return
      setGuideTokenValue(secret)
      setRevealedGuideContextKey(guideRevealContextKey)
    } catch (err) {
      if (guideTokenRunIdRef.current !== runId) return
      setRevealedGuideContextKey(null)
      setGuideTokenValue(null)
      setGuideTokenError(formatTemplate(text.detail.guideToken.revealFailed, {
        message: getProbeErrorMessage(err),
      }))
    } finally {
      if (guideTokenRunIdRef.current === runId) {
        setGuideTokenLoading(false)
      }
    }
  }, [
    guideRevealContextKey,
    guideTokenId,
    guideTokenLoading,
    guideTokenVisible,
    resolveTokenSecret,
    text.detail.guideToken.revealFailed,
  ])

  const subtitle = text.subtitle
  const currentView = useMemo(() => resolveUserConsoleView(route), [route])
  const currentViewTitle = text.header.views[currentView]
  const currentViewDescription = currentView === 'dashboard'
    ? text.dashboard.description
    : currentView === 'tokens'
      ? text.tokens.description
      : text.detail.subtitle
  const sessionDisplayName = useMemo(() => {
    const name = resolveUserConsoleIdentityName(profile)
    if (name) return name
    if (profile?.userLoggedIn === true) return text.header.unknownUser
    if (profile?.isAdmin) return text.header.adminLabel
    return null
  }, [profile, text.header.adminLabel, text.header.unknownUser])
  const sessionProviderLabel = useMemo(
    () => resolveUserConsoleProviderLabel(profile?.userProvider, text.header.providers),
    [profile?.userProvider, text.header.providers],
  )

  const guideToken = guideTokenVisible ? guideTokenValue ?? maskedGuideToken : maskedGuideToken

  const detailTokenCopyState = route.name === 'token' ? copyState[route.id] ?? 'idle' : 'idle'
  const detailTokenMatchesRoute = route.name === 'token' && tokenSecretTokenId === route.id
  const detailTokenVisible = detailTokenMatchesRoute && tokenSecretVisible && tokenSecretValue != null
  const detailTokenValue = detailTokenVisible ? tokenSecretValue ?? '' : ''
  const detailTokenLoading = detailTokenMatchesRoute && tokenSecretLoading
  const detailTokenError = detailTokenMatchesRoute ? tokenSecretError : null

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
  const consoleLoggedOut = consoleAvailability === 'logged_out' && showLoggedOutState
  const consoleNeedsLogin = consoleAvailability === 'logged_out' && !showLoggedOutState
  const consoleEmptyState = consoleUnavailable || consoleLoggedOut || consoleNeedsLogin
  const logoutVisible = profile?.userLoggedIn === true
  const showTokenListLoading = loading && tokens.length === 0
  const showEmptyTokens = !loading && tokens.length === 0
  const showLandingGuide = shouldRenderLandingGuide(route, tokens.length)

  const scrollToLandingSection = useCallback((section: UserConsoleLandingSection, behavior: ScrollBehavior = 'auto') => {
    const target = section === 'dashboard' ? dashboardSectionRef.current : tokensSectionRef.current
    if (!target) return
    const finalBehavior = behavior === 'smooth' && window.matchMedia('(prefers-reduced-motion: reduce)').matches
      ? 'auto'
      : behavior
    target.scrollIntoView({ behavior: finalBehavior, block: 'start' })
  }, [])

  useEffect(() => {
    if (consoleEmptyState || route.name !== 'landing' || !route.section) return
    if (!shouldScrollLandingSectionRef.current) {
      landingScrollBehaviorRef.current = 'auto'
      return
    }
    const section = route.section
    const behavior = landingScrollBehaviorRef.current
    const frame = window.requestAnimationFrame(() => {
      scrollToLandingSection(section, behavior)
      shouldScrollLandingSectionRef.current = false
      landingScrollBehaviorRef.current = 'auto'
    })
    return () => window.cancelAnimationFrame(frame)
  }, [consoleEmptyState, route, scrollToLandingSection])

  useEffect(() => {
    if (consoleEmptyState || route.name !== 'token') return
    const frame = window.requestAnimationFrame(() => {
      window.scrollTo({ top: 0, behavior: 'auto' })
      detailHeadingRef.current?.focus({ preventScroll: true })
    })
    return () => window.cancelAnimationFrame(frame)
  }, [consoleEmptyState, route])

  const navigateToRoute = useCallback((nextRoute: ConsoleRoute) => {
    const nextPath = userConsoleRouteToPath(nextRoute)
    const currentPath = normalizeUserConsolePathname(window.location.pathname || '')
    if (currentPath !== nextPath || window.location.hash) {
      window.history.pushState(null, '', `${nextPath}${window.location.search}`)
    }
    setRoute(nextRoute)
  }, [])

  const runMcpProbe = useCallback(async () => {
    if (route.name !== 'token' || anyProbeRunning) return
    const runId = probeRunIdRef.current + 1
    probeRunIdRef.current = runId
    probeAbortRef.current?.abort()
    const controller = new AbortController()
    probeAbortRef.current = controller
    const isActiveRun = () => probeRunIdRef.current === runId && !controller.signal.aborted
    const probeText = text.detail.probe
    const probeContext: McpProbeRunContext = {
      protocolVersion: MCP_PROBE_PROTOCOL_VERSION,
      sessionId: null,
      clientVersion: versionState.status === 'ready' ? versionState.value?.frontend ?? 'dev' : 'dev',
      identity: createMcpProbeIdentityGenerator(),
      signal: controller.signal,
    }

    const stepDefinitions = [...buildMcpProbeStepDefinitions(probeText)]

    setMcpProbe({
      state: 'running',
      completed: 0,
      total: stepDefinitions.length,
    })
    setProbeBubble({ visible: true, anchor: 'mcp', items: [] })

    let token = ''
    try {
      const secret = await fetchUserTokenSecret(route.id, controller.signal)
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
          return await fetchUserTokenDetail(route.id, todayWindow, controller.signal)
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

      if (current.billable && quotaBlockedWindow) {
        completedItems.push({
          ...runningItem,
          status: 'blocked',
          detail: quotaBlockedDetail(probeText, quotaBlockedWindow),
        })
        stepStates.push('blocked')
      } else {
        try {
          const result = await current.run(token, probeContext)
          if (!isActiveRun()) return
          if (result?.discoveredTools?.length) {
            stepDefinitions.push(...buildMcpToolCallProbeStepDefinitions(probeText, result.discoveredTools))
          }
          const stepState = result?.stepState ?? 'success'
          completedItems.push({
            ...runningItem,
            status: stepState,
            detail: result?.detail ?? undefined,
          })
          stepStates.push(stepState)
        } catch (err) {
          if (!isActiveRun()) return
          const quotaWindow = current.billable && err instanceof McpProbeRequestError
            ? getQuotaExceededWindow(err.payload)
            : null
          if (quotaWindow) {
            quotaBlockedWindow = quotaWindow
            try {
              const refreshedDetail = await fetchUserTokenDetail(route.id, todayWindow, controller.signal)
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

      setMcpProbe((prev) => nextRunningMcpProbeModel(prev, stepDefinitions, index + 1))
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
    if (probeAbortRef.current === controller) {
      probeAbortRef.current = null
    }
  }, [anyProbeRunning, detail, route, text.detail.probe, todayWindow, versionState])

  const runApiProbe = useCallback(async () => {
    if (route.name !== 'token' || anyProbeRunning) return
    const runId = probeRunIdRef.current + 1
    probeRunIdRef.current = runId
    probeAbortRef.current?.abort()
    const controller = new AbortController()
    probeAbortRef.current = controller
    const isActiveRun = () => probeRunIdRef.current === runId && !controller.signal.aborted

    const stepDefinitions = buildApiProbeStepDefinitions(text.detail.probe)

    setApiProbe({
      state: 'running',
      completed: 0,
      total: stepDefinitions.length,
    })
    setProbeBubble({ visible: true, anchor: 'api', items: [] })

    let token = ''
    try {
      const secret = await fetchUserTokenSecret(route.id, controller.signal)
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
        const detail = await current.run(token, { requestId: researchRequestId, signal: controller.signal })
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
    if (probeAbortRef.current === controller) {
      probeAbortRef.current = null
    }
  }, [anyProbeRunning, route, text.detail.probe])

  const goHome = () => {
    window.location.href = '/'
  }
  const goTokens = (behavior: ScrollBehavior = 'auto') => {
    shouldScrollLandingSectionRef.current = true
    landingScrollBehaviorRef.current = behavior
    navigateToRoute({ name: 'landing', section: 'tokens' })
  }
  const goTokenDetail = (tokenId: string) => {
    navigateToRoute({ name: 'token', id: tokenId })
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

  const renderGuideSection = useCallback((options?: {
    sectionTitle?: string
    sectionDescription?: string
  }): JSX.Element => (
    <section className="surface panel public-home-guide">
      {options?.sectionTitle ? (
        <div className="panel-header user-console-section-header">
          <div>
            <h2>{options.sectionTitle}</h2>
            {options.sectionDescription ? (
              <p className="panel-description">{options.sectionDescription}</p>
            ) : null}
          </div>
        </div>
      ) : (
        <h2>{publicStrings.guide.title}</h2>
      )}
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
        <div className="guide-panel-header">
          <h3>{guideDescription.title}</h3>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="guide-token-toggle"
            disabled={!guideTokenId || guideTokenLoading}
            aria-pressed={guideTokenVisible}
            aria-busy={guideTokenLoading}
            onClick={() => void toggleGuideTokenVisibility()}
          >
            <Icon
              icon={guideTokenLoading ? 'mdi:loading' : guideTokenVisible ? 'mdi:eye-off-outline' : 'mdi:eye-outline'}
              width={16}
              height={16}
              aria-hidden="true"
              className={guideTokenLoading ? 'guide-token-toggle-icon-spin' : undefined}
            />
            <span>
              {guideTokenLoading
                ? text.detail.guideToken.loading
                : guideTokenVisible
                  ? text.detail.guideToken.hide
                  : text.detail.guideToken.show}
            </span>
          </Button>
        </div>
        {guideTokenError ? (
          <p className="guide-token-error" role="status" aria-live="polite">{guideTokenError}</p>
        ) : null}
        <ol>
          {guideDescription.steps.map((step, index) => (
            <li key={index}>{step}</li>
          ))}
        </ol>
        {resolveGuideSamples(guideDescription).map((sample) => (
          <div className="guide-sample" key={`${guideDescription.title}-${sample.title}`}>
            <p className="guide-sample-title">{sample.title}</p>
            <div className="mockup-code relative guide-code-shell">
              <span className="guide-lang-badge badge badge-outline badge-sm">
                {(sample.language ?? 'code').toUpperCase()}
              </span>
              <pre>
                <code dangerouslySetInnerHTML={{ __html: sample.snippet }} />
              </pre>
            </div>
            {sample.reference ? (
              <p className="guide-reference">
                {publicStrings.guide.dataSourceLabel}
                <a href={sample.reference.url} target="_blank" rel="noreferrer">
                  {sample.reference.label}
                </a>
              </p>
            ) : null}
          </div>
        ))}
      </div>
      {activeGuide === 'cherryStudio' && <CherryStudioMock apiKeyExample={guideToken} />}
    </section>
  ), [
    activeGuide,
    guideDescription,
    guideTabs,
    guideToken,
    guideTokenError,
    guideTokenId,
    guideTokenLoading,
    guideTokenVisible,
    isCompactLayout,
    publicStrings.guide.dataSourceLabel,
    publicStrings.guide.title,
    text.detail.guideToken.hide,
    text.detail.guideToken.loading,
    text.detail.guideToken.show,
    toggleGuideTokenVisibility,
  ])

  return (
    <main
      ref={pageRef}
      className={`app-shell public-home viewport-${viewportMode} content-${contentMode}${
        isCompactLayout ? ' is-compact-layout' : ''
      }`}
    >
      <UserConsoleHeader
        title={text.title}
        subtitle={subtitle}
        eyebrow={text.header.eyebrow}
        currentViewLabel={text.header.currentView}
        currentViewTitle={currentViewTitle}
        currentViewDescription={currentViewDescription}
        sessionLabel={text.header.session}
        sessionDisplayName={sessionDisplayName}
        sessionProviderLabel={sessionProviderLabel}
        sessionAvatarUrl={profile?.userAvatarUrl}
        adminLabel={text.header.adminLabel}
        isAdmin={profile?.isAdmin === true}
        adminHref={adminHref}
        adminActionLabel={publicStrings.adminButton}
        adminMenuLabel={text.header.adminMenuAction}
        logoutVisible={logoutVisible}
        isLoggingOut={isLoggingOut}
        logoutLabel={text.header.logout}
        loggingOutLabel={text.header.loggingOut}
        onLogout={handleLogout}
      />

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

      {consoleLoggedOut && (
        <section className="surface panel access-panel">
          <div className="console-unavailable-state">
            <div className="console-unavailable-icon" aria-hidden="true">
              <Icon icon="mdi:logout-variant" width={22} height={22} />
            </div>
            <div className="console-unavailable-copy">
              <h2>{text.loggedOut.title}</h2>
              <p>{text.loggedOut.description}</p>
            </div>
            <div className="table-actions console-unavailable-actions">
              <button type="button" className="btn btn-primary" onClick={() => { window.location.href = '/auth/linuxdo' }}>
                {text.loggedOut.action}
              </button>
            </div>
          </div>
        </section>
      )}

      {consoleNeedsLogin && (
        <section className="surface panel access-panel">
          <div className="console-unavailable-state">
            <div className="console-unavailable-icon" aria-hidden="true">
              <Icon icon="mdi:account-arrow-right-outline" width={22} height={22} />
            </div>
            <div className="console-unavailable-copy">
              <h2>{text.loginRequired.title}</h2>
              <p>{text.loginRequired.description}</p>
            </div>
            <div className="table-actions console-unavailable-actions">
              <button type="button" className="btn btn-primary" onClick={() => { window.location.href = '/auth/linuxdo' }}>
                {text.loginRequired.action}
              </button>
            </div>
          </div>
        </section>
      )}

      {!consoleEmptyState && error && <section className="surface error-banner">{error}</section>}

      {!consoleEmptyState && route.name === 'landing' && (
        <div className="user-console-landing-stack">
          <section
            ref={dashboardSectionRef}
            id="console-dashboard-section"
            className="surface panel user-console-section"
            data-console-section="dashboard"
          >
            <header className="panel-header user-console-section-header">
              <div>
                <h2>{text.dashboard.usage}</h2>
                <p className="panel-description">{text.dashboard.description}</p>
              </div>
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
                <h4>{text.dashboard.monthlySuccessUtc}</h4>
                <p><RollingNumber value={loading ? null : dashboard?.monthlySuccess ?? 0} /></p>
              </div>
            </div>
            <div className="access-stats">
              <div className="access-stat quota-stat-card">
                <div className="quota-stat-label">
                  {formatRequestRateSummary(resolveRequestRate(dashboard, 'user'), language)}
                </div>
                <div className="quota-stat-value">
                  {formatNumber(resolveRequestRate(dashboard, 'user').used)}
                  <span>/ {formatNumber(resolveRequestRate(dashboard, 'user').limit)}</span>
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

          <section
            ref={tokensSectionRef}
            id="console-tokens-section"
            className="surface panel user-console-section"
            data-console-section="tokens"
          >
            <div className="panel-header user-console-section-header">
              <div>
                <h2>{text.tokens.title}</h2>
                <p className="panel-description">{text.tokens.description}</p>
              </div>
            </div>
            <div className="table-wrapper jobs-table-wrapper user-console-md-up">
              {showTokenListLoading ? (
                <div className="empty-state">{text.tokens.loading}</div>
              ) : showEmptyTokens ? (
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
                                <span>{formatRequestRateSummary(resolveRequestRate(item, 'token'), language)}</span>
                                <strong>
                                  {formatQuotaPair(
                                    resolveRequestRate(item, 'token').used,
                                    resolveRequestRate(item, 'token').limit,
                                  )}
                                </strong>
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
                                onPointerEnter={() => scheduleWarmTokenSecret(item.tokenId)}
                                onPointerLeave={() => cancelWarmTokenSecret(item.tokenId)}
                                onBlur={() => cancelWarmTokenSecret(item.tokenId)}
                                onPointerDown={() => warmTokenSecret(item.tokenId)}
                                onKeyDown={(event) => {
                                  if (!isCopyIntentKey(event.key)) return
                                  warmTokenSecret(item.tokenId)
                                }}
                                onClick={(event) => void copyToken(item.tokenId, event.currentTarget)}
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
              {showTokenListLoading ? (
                <div className="empty-state">{text.tokens.loading}</div>
              ) : showEmptyTokens ? (
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
                        <span>{formatRequestRateSummary(resolveRequestRate(item, 'token'), language)}</span>
                        <strong>
                          {formatQuotaPair(
                            resolveRequestRate(item, 'token').used,
                            resolveRequestRate(item, 'token').limit,
                          )}
                        </strong>
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
                          onPointerEnter={() => scheduleWarmTokenSecret(item.tokenId)}
                          onPointerLeave={() => cancelWarmTokenSecret(item.tokenId)}
                          onBlur={() => cancelWarmTokenSecret(item.tokenId)}
                          onPointerDown={() => warmTokenSecret(item.tokenId)}
                          onKeyDown={(event) => {
                            if (!isCopyIntentKey(event.key)) return
                            warmTokenSecret(item.tokenId)
                          }}
                          onClick={(event) => void copyToken(item.tokenId, event.currentTarget)}
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
          {showLandingGuide && renderGuideSection({
            sectionTitle: text.detail.guideTitle,
            sectionDescription: text.detail.guideDescription,
          })}
        </div>
      )}

      {!consoleEmptyState && route.name === 'token' && (
        <>
          <section className="surface panel access-panel">
            <header className="panel-header" style={{ marginBottom: 8 }}>
              <div>
                <h2 ref={detailHeadingRef} tabIndex={-1}>{text.detail.title} <code>{route.id}</code></h2>
                <p className="panel-description">{text.detail.subtitle}</p>
              </div>
              <button type="button" className="btn btn-outline" onClick={() => goTokens()}>{text.detail.back}</button>
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
                <h4>{text.dashboard.monthlySuccessUtc}</h4>
                <p><RollingNumber value={detailLoading ? null : detail?.monthlySuccess ?? 0} /></p>
              </div>
            </div>
            <div className="access-stats">
              <div className="access-stat quota-stat-card">
                <div className="quota-stat-label">
                  {formatRequestRateSummary(resolveRequestRate(detail, 'token'), language)}
                </div>
                <div className="quota-stat-value">
                  {formatNumber(resolveRequestRate(detail, 'token').used)}
                  <span>/ {formatNumber(resolveRequestRate(detail, 'token').limit)}</span>
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

            <TokenSecretField
              inputId={`user-console-token-${route.id}`}
              inputRef={detailTokenFieldRef}
              value={detailTokenValue}
              visible={detailTokenVisible}
              hiddenDisplayValue={tokenLabel(route.id)}
              visibilityBusy={detailTokenLoading}
              copyState={detailTokenCopyState}
              onValueChange={() => undefined}
              onToggleVisibility={() => void toggleTokenSecretVisibility()}
              onCopyIntent={() => scheduleWarmTokenSecret(route.id)}
              onCopyIntentCancel={() => cancelWarmTokenSecret(route.id)}
              onCopy={(anchorEl) => copyToken(route.id, anchorEl)}
              label={text.detail.tokenLabel}
              visibilityShowLabel={text.detail.tokenSecret.show}
              visibilityHideLabel={text.detail.tokenSecret.hide}
              visibilityIconAlt={text.detail.tokenSecret.iconAlt}
              copyAriaLabel={text.tokens.copy}
              copyLabel={text.tokens.copy}
              copiedLabel={text.tokens.copied}
              copyErrorLabel={text.tokens.copyFailed}
              wrapperClassName="access-token-box user-console-token-box"
              readOnly
            />
            {detailTokenLoading ? (
              <p className="sr-only" role="status" aria-live="polite">
                {text.detail.tokenSecret.loading}
              </p>
            ) : null}
            {detailTokenError ? (
              <p className="user-console-token-error" role="status" aria-live="polite">{detailTokenError}</p>
            ) : null}

            <ConnectivityChecksPanel
              title={text.detail.probe.title}
              costHint={text.detail.probe.costHint}
              costHintAria={text.detail.probe.costHintAria}
              stepStatusText={text.detail.probe.stepStatus}
              mcpButtonLabel={probeButtonLabel('mcp', mcpProbe)}
              apiButtonLabel={probeButtonLabel('api', apiProbe)}
              mcpProbe={mcpProbe}
              apiProbe={apiProbe}
              probeBubble={probeBubble}
              anyProbeRunning={anyProbeRunning}
              onMcpClick={() => void runMcpProbe()}
              onApiClick={() => void runApiProbe()}
            />
          </section>

          <section className="surface panel user-console-detail-panel">
            <div className="panel-header">
              <h2>{text.detail.logs}</h2>
              <div className="user-console-push-status-slot">
                {detailLogsPushIssue ? (
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <button
                        type="button"
                        className="user-console-push-status-trigger"
                        aria-label={text.detail.pushStatus.ariaLabel}
                      >
                        <Icon icon="mdi:alert-circle-outline" width={18} height={18} aria-hidden="true" />
                      </button>
                    </TooltipTrigger>
                    <TooltipContent side="top" align="end" className="max-w-[min(20rem,calc(100vw-2rem))]">
                      {resolveDetailLogsPushIssueMessage(detailLogsPushIssue, text.detail.pushStatus)}
                    </TooltipContent>
                  </Tooltip>
                ) : (
                  <span className="user-console-push-status-spacer" aria-hidden="true" />
                )}
              </div>
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

          {renderGuideSection()}

        </>
      )}
      <UserConsoleFooter strings={text.footer} versionState={versionState} />
      <ManualCopyBubble
        open={manualCopyBubble != null}
        anchorEl={manualCopyBubble?.anchorEl ?? null}
        title={text.tokens.manualCopy.title}
        description={text.tokens.manualCopy.description}
        fieldLabel={text.tokens.manualCopy.fieldLabel}
        value={manualCopyBubble?.value ?? ''}
        closeLabel={text.tokens.manualCopy.close}
        onClose={() => setManualCopyBubble(null)}
      />
    </main>
  )
}

export {
  applyLoggedOutConsoleReset,
  buildApiProbeStepDefinitions,
  buildMcpProbeStepDefinitions,
  buildMcpToolCallProbeStepDefinitions,
  createClearedProbeUiState,
  createMcpProbeIdentityGenerator,
  extractAdvertisedMcpTools,
  isActiveGuideRevealContext,
  isIdentifierLikePropertyName,
  nextRunningMcpProbeModel,
  performUserLogoutFlow,
  resetActiveProbeUiState,
  resolveDetailLogsPushIssueMessage,
  resolveGuideRevealContextKey,
  resolveGuideToken,
  resolveGuideTokenId,
  resolveFallbackLogoutTarget,
  resolvePostLogoutTarget,
  resolveUserConsoleIdentityName,
  resolveUserConsoleProviderLabel,
  resolveUserConsoleView,
  shouldRedirectToLogoutTarget,
  shouldRenderLandingGuide,
  toLoggedOutConsoleProfile,
}

