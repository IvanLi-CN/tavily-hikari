import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Icon, getGuideClientIconName } from './lib/icons'
import CherryStudioMock from './components/CherryStudioMock'
import ConnectivityChecksPanel, {
  type ProbeBubbleItem,
  type ProbeBubbleModel,
  type ProbeButtonModel,
  type ProbeButtonState,
  type ProbeStepStatus,
} from './components/ConnectivityChecksPanel'
import TokenSecretField, { type TokenSecretCopyState } from './components/TokenSecretField'
import ManualCopyBubble from './components/ManualCopyBubble'

import {
  fetchVersion,
  fetchProfile,
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
  fetchUserTokenLogs,
  fetchUserTokenSecret,
  fetchUserTokens,
  type Profile,
  type PublicTokenLog,
  type UserDashboard,
  type UserTokenSummary,
  type VersionInfo,
} from './api'
import LanguageSwitcher from './components/LanguageSwitcher'
import RollingNumber from './components/RollingNumber'
import { StatusBadge, type StatusTone } from './components/StatusBadge'
import ThemeToggle from './components/ThemeToggle'
import UserConsoleFooter from './components/UserConsoleFooter'
import { Button } from './components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from './components/ui/dropdown-menu'
import { useLanguage, useTranslate, type Language } from './i18n'
import { copyText, isCopyIntentKey, selectAllReadonlyText, shouldPrewarmSecretCopy } from './lib/clipboard'
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
} from './lib/mcpProbe'
import { useResponsiveModes } from './lib/responsive'
import { getUserConsoleAdminHref } from './lib/userConsoleAdminEntry'
import { resolveUserConsoleAvailability } from './lib/userConsoleAvailability'
import {
  parseUserConsoleHash,
  userConsoleRouteToHash,
  type UserConsoleLandingSection,
  type UserConsoleRoute as ConsoleRoute,
} from './lib/userConsoleRoutes'

const CODEX_DOC_URL = 'https://github.com/openai/codex/blob/main/docs/config.md'
const CLAUDE_DOC_URL = 'https://code.claude.com/docs/en/mcp'
const MCP_SPEC_URL = 'https://modelcontextprotocol.io/introduction'
const MCP_PROBE_PROTOCOL_VERSION = '2025-03-26'
const TAVILY_SEARCH_DOC_URL = 'https://docs.tavily.com/documentation/api-reference/endpoint/search'
const VSCODE_DOC_URL = 'https://code.visualstudio.com/docs/copilot/customization/mcp-servers'
const NOCODB_DOC_URL = 'https://nocodb.com/docs/product-docs/mcp'
const USER_CONSOLE_SECRET_CACHE_TTL_MS = 2_000
const USER_CONSOLE_SECRET_PREWARM_DELAY_MS = 120
const BASE_MCP_PROBE_STEP_COUNT = 4

type GuideLanguage = 'toml' | 'json' | 'bash'
type GuideKey = 'codex' | 'claude' | 'vscode' | 'claudeDesktop' | 'cursor' | 'windsurf' | 'cherryStudio' | 'other'

interface GuideReference {
  label: string
  url: string
}

interface GuideSample {
  title: string
  language?: GuideLanguage
  snippet: string
  reference?: GuideReference
}

interface GuideContent {
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
    context: { requestId: string | null },
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
      label: probeText.steps.apiExtract,
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
      label: probeText.steps.apiCrawl,
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
      label: probeText.steps.apiMap,
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
      label: probeText.steps.apiResearch,
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
          throw new Error(probeText.errors.missingRequestId)
        }
        return requestId
      },
    },
    {
      id: 'api-research-result',
      label: probeText.steps.apiResearchResult,
      run: async (token: string, context: { requestId: string | null }): Promise<string | null> => {
        if (!context.requestId) {
          throw new Error(probeText.errors.missingRequestId)
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
  const [route, setRoute] = useState<ConsoleRoute>(() => parseUserConsoleHash(window.location.hash || ''))
  const [detail, setDetail] = useState<UserTokenSummary | null>(null)
  const [detailLogs, setDetailLogs] = useState<PublicTokenLog[]>([])
  const [loading, setLoading] = useState(true)
  const [detailLoading, setDetailLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [copyState, setCopyState] = useState<Record<string, TokenSecretCopyState>>({})
  const [tokenSecretTokenId, setTokenSecretTokenId] = useState<string | null>(null)
  const [tokenSecretVisible, setTokenSecretVisible] = useState(false)
  const [tokenSecretValue, setTokenSecretValue] = useState<string | null>(null)
  const [tokenSecretLoading, setTokenSecretLoading] = useState(false)
  const [tokenSecretError, setTokenSecretError] = useState<string | null>(null)
  const [activeGuide, setActiveGuide] = useState<GuideKey>('codex')
  const [isMobileGuide, setIsMobileGuide] = useState(false)
  const [mcpProbe, setMcpProbe] = useState<ProbeButtonModel>(() => createProbeButtonModel(BASE_MCP_PROBE_STEP_COUNT))
  const [apiProbe, setApiProbe] = useState<ProbeButtonModel>(() => createProbeButtonModel(6))
  const [probeBubble, setProbeBubble] = useState<ProbeBubbleModel | null>(null)
  const [manualCopyBubble, setManualCopyBubble] = useState<ManualCopyBubbleState | null>(null)
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
  const pageRef = useRef<HTMLElement>(null)
  const dashboardSectionRef = useRef<HTMLElement | null>(null)
  const tokensSectionRef = useRef<HTMLElement | null>(null)
  const detailHeadingRef = useRef<HTMLHeadingElement | null>(null)
  const detailTokenFieldRef = useRef<HTMLInputElement | null>(null)
  const historyTraversalRef = useRef(false)
  const landingScrollBehaviorRef = useRef<ScrollBehavior>('auto')
  const shouldScrollLandingSectionRef = useRef(route.name === 'landing' && route.section !== null)
  const { viewportMode, contentMode, isCompactLayout } = useResponsiveModes(pageRef)

  useEffect(() => {
    const handlePopState = () => {
      historyTraversalRef.current = true
    }
    const syncRoute = () => {
      const nextRoute = parseUserConsoleHash(window.location.hash || '')
      if (nextRoute.name === 'landing' && nextRoute.section && !historyTraversalRef.current) {
        shouldScrollLandingSectionRef.current = true
      }
      setRoute(nextRoute)
      historyTraversalRef.current = false
    }
    window.addEventListener('popstate', handlePopState)
    window.addEventListener('hashchange', syncRoute)
    return () => {
      window.removeEventListener('popstate', handlePopState)
      window.removeEventListener('hashchange', syncRoute)
    }
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
    setMcpProbe(createProbeButtonModel(BASE_MCP_PROBE_STEP_COUNT))
    setApiProbe(createProbeButtonModel(6))
    setProbeBubble(null)
    setManualCopyBubble(null)
  }, [route.name === 'token' ? route.id : route.section ?? 'landing'])

  const abortPendingTokenSecretRequest = useCallback((tokenId: string) => {
    const controller = tokenSecretRequestAbortRef.current.get(tokenId)
    if (controller) {
      controller.abort()
      tokenSecretRequestAbortRef.current.delete(tokenId)
    }
    tokenSecretRequestRef.current.delete(tokenId)
  }, [])

  const abortAllPendingTokenSecretRequests = useCallback(() => {
    for (const controller of tokenSecretRequestAbortRef.current.values()) {
      controller.abort()
    }
    tokenSecretRequestAbortRef.current.clear()
    tokenSecretRequestRef.current.clear()
  }, [])

  useEffect(() => {
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
  }, [abortAllPendingTokenSecretRequests, consoleAvailability, route.name === 'token' ? route.id : route.name])

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

  const subtitle = useMemo(() => {
    const user = profile?.userDisplayName?.trim()
    if (user && user.length > 0) {
      return `${text.subtitle} · ${user}`
    }
    return text.subtitle
  }, [profile?.userDisplayName, text.subtitle])

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
    if (consoleUnavailable || route.name !== 'landing' || !route.section) return
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
  }, [consoleUnavailable, route, scrollToLandingSection])

  useEffect(() => {
    if (consoleUnavailable || route.name !== 'token') return
    const frame = window.requestAnimationFrame(() => {
      window.scrollTo({ top: 0, behavior: 'auto' })
      detailHeadingRef.current?.focus({ preventScroll: true })
    })
    return () => window.cancelAnimationFrame(frame)
  }, [consoleUnavailable, route])

  const navigateToRoute = useCallback((nextRoute: ConsoleRoute) => {
    const nextHash = userConsoleRouteToHash(nextRoute)
    if (window.location.hash !== nextHash) {
      if (nextHash) {
        window.location.hash = nextHash
        return
      }
      window.history.replaceState(null, '', `${window.location.pathname}${window.location.search}`)
    }
    setRoute(nextRoute)
  }, [])

  const runMcpProbe = useCallback(async () => {
    if (route.name !== 'token' || anyProbeRunning) return
    const runId = probeRunIdRef.current + 1
    probeRunIdRef.current = runId
    const isActiveRun = () => probeRunIdRef.current === runId
    const probeText = text.detail.probe
    const probeContext: McpProbeRunContext = {
      protocolVersion: MCP_PROBE_PROTOCOL_VERSION,
      sessionId: null,
      clientVersion: versionState.status === 'ready' ? versionState.value?.frontend ?? 'dev' : 'dev',
      identity: createMcpProbeIdentityGenerator(),
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
  }, [anyProbeRunning, detail, route, text.detail.probe, versionState])

  const runApiProbe = useCallback(async () => {
    if (route.name !== 'token' || anyProbeRunning) return
    const runId = probeRunIdRef.current + 1
    probeRunIdRef.current = runId
    const isActiveRun = () => probeRunIdRef.current === runId

    const stepDefinitions = buildApiProbeStepDefinitions(text.detail.probe)

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

      {!consoleUnavailable && route.name === 'landing' && (
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

      {!consoleUnavailable && route.name === 'token' && (
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

export const __testables = {
  buildApiProbeStepDefinitions,
  buildMcpProbeStepDefinitions,
  buildMcpToolCallProbeStepDefinitions,
  canonicalMcpProbeToolName,
  createMcpProbeIdentityGenerator,
  extractAdvertisedMcpTools,
  isActiveGuideRevealContext,
  isBillableMcpProbeTool,
  isIdentifierLikePropertyName,
  nextRunningMcpProbeModel,
  resolveGuideSamples,
  resolveGuideRevealContextKey,
  resolveGuideToken,
  resolveGuideTokenId,
  shouldRenderLandingGuide,
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
  const current = labels.find((l) => l.id === active)
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button type="button" className="btn btn-outline w-full justify-between btn-sm md:btn-md">
          <span className="inline-flex items-center gap-2">
            <Icon
              icon={getGuideClientIconName(active)}
              width={18}
              height={18}
              aria-hidden="true"
              style={{ color: '#475569' }}
            />
            {current?.label ?? active}
          </span>
          <Icon icon="mdi:chevron-down" width={16} height={16} aria-hidden="true" style={{ color: '#647589' }} />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="guide-select-menu p-1">
        {labels.map((tab) => (
          <DropdownMenuItem
            key={tab.id}
            className={`flex items-center gap-2 ${tab.id === active ? 'bg-accent/45 text-accent-foreground' : ''}`}
            onSelect={() => onChange(tab.id)}
          >
              <Icon
                icon={getGuideClientIconName(tab.id)}
                width={16}
                height={16}
                aria-hidden="true"
                style={{ color: '#475569' }}
              />
              <span className="truncate">{tab.label}</span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function buildGuideContent(language: Language, baseUrl: string, prettyToken: string): Record<GuideKey, GuideContent> {
  const isEnglish = language === 'en'
  const codexSnippet = buildCodexSnippet(baseUrl)
  const claudeSnippet = buildClaudeSnippet(baseUrl, prettyToken, language)
  const genericJsonSnippet = buildGenericJsonSnippet(baseUrl, prettyToken)
  const genericMcpSnippet = buildGenericMcpSnippet(baseUrl, prettyToken)
  const apiSearchSnippet = buildApiSearchSnippet(baseUrl, prettyToken)
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
      title: isEnglish ? 'Other clients' : '其他客户端',
      steps: isEnglish
        ? [
            <>If your client supports remote MCP, point it to <code>{baseUrl}/mcp</code> and attach <code>Authorization: Bearer {prettyToken}</code>.</>,
            <>If your client talks to Tavily's HTTP API instead of MCP, use the façade base URL <code>{baseUrl}/api/tavily</code> and call endpoints such as <code>/search</code>, <code>/extract</code>, <code>/crawl</code>, <code>/map</code>, or <code>/research</code>.</>,
            <>For HTTP API clients, prefer the same bearer token in the header; if headers are unavailable, send it as JSON field <code>api_key</code>.</>,
          ]
        : [
            <>如果客户端支持远程 MCP，就把地址指向 <code>{baseUrl}/mcp</code>，并附带 <code>Authorization: Bearer {prettyToken}</code>。</>,
            <>如果客户端走的是 Tavily 风格 HTTP API，而不是 MCP，就使用基础地址 <code>{baseUrl}/api/tavily</code>，再继续调用 <code>/search</code>、<code>/extract</code>、<code>/crawl</code>、<code>/map</code>、<code>/research</code> 等端点。</>,
            <>对于 HTTP API 客户端，推荐继续使用同一个 Bearer Token；如果没法自定义 Header，也可以把令牌写入 JSON 请求体字段 <code>api_key</code>。</>,
          ],
      samples: [
        {
          title: isEnglish ? 'Example 1: generic MCP client config' : '示例 1：通用 MCP 客户端配置',
          language: 'json',
          snippet: genericMcpSnippet,
          reference: {
            label: 'Model Context Protocol spec',
            url: MCP_SPEC_URL,
          },
        },
        {
          title: isEnglish ? 'Example 2: POST /api/tavily/search' : '示例 2：POST /api/tavily/search',
          language: 'bash',
          snippet: apiSearchSnippet,
          reference: {
            label: 'Tavily Search API docs',
            url: TAVILY_SEARCH_DOC_URL,
          },
        },
      ],
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

function buildGenericMcpSnippet(baseUrl: string, prettyToken: string): string {
  return `{
  <span class="hl-key">"type"</span>: <span class="hl-string">"http"</span>,
  <span class="hl-key">"url"</span>: <span class="hl-string">"${baseUrl}/mcp"</span>,
  <span class="hl-key">"headers"</span>: {
    <span class="hl-key">"Authorization"</span>: <span class="hl-string">"Bearer ${prettyToken}"</span>
  }
}`
}

function buildApiSearchSnippet(baseUrl: string, prettyToken: string): string {
  return `curl -X POST "${baseUrl}/api/tavily/search" \\
  -H "Content-Type: application/json" \\
  -H "Authorization: Bearer ${prettyToken}" \\
  -d '{
    "query": "latest AI agent news",
    "topic": "general",
    "search_depth": "basic",
    "include_answer": true,
    "max_results": 5
  }'`
}

function resolveGuideSamples(content: GuideContent): GuideSample[] {
  if (content.samples && content.samples.length > 0) return content.samples
  if (content.sampleTitle && content.snippet) {
    return [{
      title: content.sampleTitle,
      language: content.snippetLanguage,
      snippet: content.snippet,
      reference: content.reference,
    }]
  }
  return []
}

const EN = {
  title: 'User Console',
  subtitle: 'Your account dashboard and token management',
  dashboard: {
    usage: 'Account Usage Overview',
    description: 'Track account-level throughput, failures, and quota windows without switching pages.',
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
    description: 'Copy any token or jump into its detail view from the same landing page.',
    loading: 'Loading token list…',
    empty: 'No token available for this account.',
    copy: 'Copy',
    copied: 'Copied',
    copyFailed: 'Copy failed',
    detail: 'Details',
    manualCopy: {
      title: 'Manual copy required',
      description: 'This browser blocked automatic copy. The full token is selected below for manual copy.',
      fieldLabel: 'Full Token',
      close: 'Close',
    },
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
    back: 'Back to Token List',
    tokenLabel: 'Token',
    tokenSecret: {
      show: 'Show full token',
      hide: 'Hide full token',
      iconAlt: 'token visibility toggle',
      loading: 'Loading full token…',
      revealFailed: 'Failed to reveal token: {message}',
    },
    guideToken: {
      show: 'Show token',
      hide: 'Hide token',
      loading: 'Loading token…',
      revealFailed: 'Failed to reveal guide token: {message}',
    },
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
        skipped: 'Skipped',
      },
      quotaBlocked: '{window} quota exhausted, skipping billable MCP tool calls.',
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
        mcpInitialize: 'MCP session initialize',
        mcpInitialized: 'MCP initialized notification',
        mcpPing: 'MCP service connectivity',
        mcpToolsList: 'MCP tool discovery',
        mcpToolCall: 'Call {tool} tool',
        apiSearch: 'Web search capability',
        apiExtract: 'Page extract capability',
        apiCrawl: 'Site crawl capability',
        apiMap: 'Site map capability',
        apiResearch: 'Research task creation',
        apiResearchResult: 'Research result query',
      },
      skippedProbeFixture: 'No local probe fixture for {tool}; skipped.',
      errors: {
        missingAdvertisedTools: 'MCP tools/list returned no tools',
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
  footer: {
    title: 'Tavily Hikari User Console',
    githubAria: 'Open GitHub repository',
    githubLabel: 'GitHub',
    loadingVersion: '· Loading version…',
    errorVersion: '· Version unavailable',
    tagPrefix: '· ',
  },
}

const ZH = {
  title: '用户控制台',
  subtitle: '账户仪表盘与 Token 管理',
  dashboard: {
    usage: '账户用量概览',
    description: '在同一页集中查看账户级成功率、失败量与各配额窗口。',
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
    description: '在同一落地页里完成复制 Token 与进入详情，不再切换独立列表页。',
    loading: 'Token 列表加载中…',
    empty: '当前账户暂无 Token。',
    copy: '复制',
    copied: '已复制',
    copyFailed: '复制失败',
    detail: '详情',
    manualCopy: {
      title: '请手动复制',
      description: '当前浏览器拦截了自动复制，下面已选中完整 Token，可直接手动复制。',
      fieldLabel: '完整 Token',
      close: '关闭',
    },
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
    back: '返回 Token 列表',
    tokenLabel: 'Token',
    tokenSecret: {
      show: '显示完整 Token',
      hide: '隐藏完整 Token',
      iconAlt: 'Token 显隐切换',
      loading: '正在读取完整 Token…',
      revealFailed: '读取完整 Token 失败：{message}',
    },
    guideToken: {
      show: '显示密钥',
      hide: '隐藏密钥',
      loading: '正在读取密钥…',
      revealFailed: '读取导览密钥失败：{message}',
    },
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
        skipped: '已跳过',
      },
      quotaBlocked: '{window}配额已耗尽，已跳过会消耗额度的 MCP 工具调用。',
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
        mcpInitialize: 'MCP 会话初始化',
        mcpInitialized: 'MCP initialized 通知',
        mcpPing: 'MCP 服务连通',
        mcpToolsList: 'MCP 工具发现',
        mcpToolCall: '调用 {tool} 工具',
        apiSearch: '网页搜索能力',
        apiExtract: '页面抽取能力',
        apiCrawl: '站点抓取能力',
        apiMap: '站点映射能力',
        apiResearch: '研究任务创建',
        apiResearchResult: '研究结果查询',
      },
      skippedProbeFixture: '当前本地没有 {tool} 的检测夹具，已跳过。',
      errors: {
        missingAdvertisedTools: 'MCP tools/list 没有返回任何工具',
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
  footer: {
    title: 'Tavily Hikari 用户控制台',
    githubAria: '打开 GitHub 仓库',
    githubLabel: 'GitHub',
    loadingVersion: '· 正在读取版本…',
    errorVersion: '· 版本不可用',
    tagPrefix: '· ',
  },
}
