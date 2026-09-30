import type { ReactNode } from 'react'

export type ProviderGuideKey = 'serpapi' | 'exa' | 'brave'

type ProviderGuideLanguage = 'toml' | 'json' | 'bash'

export interface ProviderGuideSample {
  title: string
  language?: ProviderGuideLanguage
  snippet: string
}

export interface ProviderGuideContent {
  title: string
  steps: ReactNode[]
  samples: ProviderGuideSample[]
}

/**
 * Per-provider setup guide content (plan todo 24). Grounded in the committed
 * backend surfaces:
 * - `src/server/provider_facade.rs:163-167` MCP ingresses `/serpapi/mcp`,
 *   `/exa/mcp`, `/brave/mcp` (all accept `Authorization: Bearer th-...`).
 * - `src/server/provider_facade.rs:88-159` REST routes: SerpApi
 *   `GET|POST /api/serpapi/search` (client slot `api_key`), Exa
 *   `POST /api/exa/{search,contents,answer}` (client slot `x-api-key`), Brave
 *   `GET /api/brave/res/v1/{web/search,news/search,llm/context}` (client slot
 *   `X-Subscription-Token`); slot names pinned by `src/providers/spec.rs`.
 * - Tool catalogs: `src/server/provider_mcp_serpapi.rs:30`,
 *   `provider_mcp_exa.rs:211-233`, `provider_mcp_brave.rs:178-205`.
 * Boundaries: Exa streaming is a typed 400 before any upstream call
 * (`provider_facade.rs:435-445`); Brave has no URL-extraction route; SerpApi
 * mirrors the search family only.
 */
export function buildProviderGuideContent(
  language: 'en' | 'zh',
  baseUrl: string,
  prettyToken: string,
): Record<ProviderGuideKey, ProviderGuideContent> {
  const isEnglish = language === 'en'
  return {
    serpapi: {
      title: 'SerpApi',
      steps: isEnglish
        ? [
            <>Point any HTTP MCP client at <code>{baseUrl}/serpapi/mcp</code> with <code>Authorization: Bearer {prettyToken}</code>; this is the hosted SerpApi MCP passthrough.</>,
            <>The façade advertises <code>search</code>, <code>search_table</code>, and <code>search_dashboard</code>.</>,
            <>HTTP clients can call <code>GET {baseUrl}/api/serpapi/search?q=...&api_key=th-...</code>, sending the Hikari token in the <code>api_key</code> query slot; a JSON <code>POST</code> convenience is also accepted.</>,
            <>Boundary: only the SerpApi search family is mirrored here — <code>engine</code> and other search parameters pass through as-is, and plan/quota state syncs from SerpApi&apos;s <code>/account</code> surface.</>,
          ]
        : [
            <>将任意 HTTP MCP 客户端指向 <code>{baseUrl}/serpapi/mcp</code>，并携带 <code>Authorization: Bearer {prettyToken}</code>；这是 SerpApi 托管 MCP 的透传入口。</>,
            <>该入口会通告 <code>search</code>、<code>search_table</code>、<code>search_dashboard</code> 三个工具。</>,
            <>HTTP 客户端可调用 <code>GET {baseUrl}/api/serpapi/search?q=...&api_key=th-...</code>，把 Hikari token 放进 <code>api_key</code> 查询参数；同时也接受 JSON <code>POST</code> 便捷写法。</>,
            <>边界：这里只镜像 SerpApi 搜索能力，<code>engine</code> 等搜索参数原样透传；套餐与额度状态从 SerpApi 的 <code>/account</code> 同步。</>,
          ],
      samples: [
        {
          title: isEnglish ? 'Example: ~/.codex/config.toml' : '示例：~/.codex/config.toml',
          language: 'toml',
          snippet: buildProviderCodexSnippet('serpapi_hikari', baseUrl, '/serpapi/mcp', 'SERPAPI_HIKARI_TOKEN'),
        },
        {
          title: isEnglish ? 'Example: claude mcp add-json' : '示例：claude mcp add-json',
          language: 'bash',
          snippet: buildProviderClaudeSnippet('serpapi-hikari', baseUrl, '/serpapi/mcp', prettyToken),
        },
        {
          title: isEnglish ? 'Example: VS Code / generic mcp.json' : '示例：VS Code / 通用 mcp.json',
          language: 'json',
          snippet: buildProviderVscodeSnippet('serpapi-hikari', baseUrl, '/serpapi/mcp', prettyToken),
        },
        {
          title: isEnglish ? 'Example: GET /api/serpapi/search' : '示例：GET /api/serpapi/search',
          language: 'bash',
          snippet: buildSerpapiHttpSnippet(baseUrl, prettyToken),
        },
      ],
    },
    exa: {
      title: 'Exa',
      steps: isEnglish
        ? [
            <>Point any HTTP MCP client at <code>{baseUrl}/exa/mcp</code> with <code>Authorization: Bearer {prettyToken}</code>; Hikari runs this facade locally.</>,
            <>The façade advertises <code>web_search_exa</code>, <code>web_fetch_exa</code>, and <code>exa_answer</code>.</>,
            <>HTTP clients can call <code>POST {baseUrl}/api/exa/search</code> (plus <code>/contents</code> and <code>/answer</code>) with a JSON body and the Hikari token in the <code>x-api-key</code> header.</>,
            <>Boundary: no streaming — omit <code>stream</code> or set it to <code>false</code>; a truthy <code>stream</code> value is rejected with a typed 400. Only search, contents, and answer are exposed.</>,
          ]
        : [
            <>将任意 HTTP MCP 客户端指向 <code>{baseUrl}/exa/mcp</code>，并携带 <code>Authorization: Bearer {prettyToken}</code>；该入口由 Hikari 本地提供。</>,
            <>该入口会通告 <code>web_search_exa</code>、<code>web_fetch_exa</code>、<code>exa_answer</code> 三个工具。</>,
            <>HTTP 客户端可调用 <code>POST {baseUrl}/api/exa/search</code>（以及 <code>/contents</code>、<code>/answer</code>），使用 JSON 请求体，并把 Hikari token 放进 <code>x-api-key</code> 请求头。</>,
            <>边界：不支持流式输出——请省略 <code>stream</code> 或设为 <code>false</code>；传入真值会收到类型化 400。仅开放 search、contents、answer 三类能力。</>,
          ],
      samples: [
        {
          title: isEnglish ? 'Example: ~/.codex/config.toml' : '示例：~/.codex/config.toml',
          language: 'toml',
          snippet: buildProviderCodexSnippet('exa_hikari', baseUrl, '/exa/mcp', 'EXA_HIKARI_TOKEN'),
        },
        {
          title: isEnglish ? 'Example: claude mcp add-json' : '示例：claude mcp add-json',
          language: 'bash',
          snippet: buildProviderClaudeSnippet('exa-hikari', baseUrl, '/exa/mcp', prettyToken),
        },
        {
          title: isEnglish ? 'Example: VS Code / generic mcp.json' : '示例：VS Code / 通用 mcp.json',
          language: 'json',
          snippet: buildProviderVscodeSnippet('exa-hikari', baseUrl, '/exa/mcp', prettyToken),
        },
        {
          title: isEnglish ? 'Example: POST /api/exa/search' : '示例：POST /api/exa/search',
          language: 'bash',
          snippet: buildExaHttpSnippet(baseUrl, prettyToken),
        },
      ],
    },
    brave: {
      title: 'Brave',
      steps: isEnglish
        ? [
            <>Point any HTTP MCP client at <code>{baseUrl}/brave/mcp</code> with <code>Authorization: Bearer {prettyToken}</code>; Hikari runs this facade locally.</>,
            <>The façade advertises <code>brave_web_search</code>, <code>brave_news_search</code>, and <code>brave_llm_context</code>.</>,
            <>HTTP clients can call <code>GET {baseUrl}/api/brave/res/v1/web/search?q=...</code> with the Hikari token in the <code>X-Subscription-Token</code> header; the sibling native paths <code>/news/search</code> and <code>/llm/context</code> are mirrored too.</>,
            <>Cherry Studio: choose the <strong>Brave Search</strong> provider and set its base URL to <code>{baseUrl}/api/brave</code>; the native <code>/res/v1</code> paths are mirrored, so base-URL-override clients work unchanged.</>,
            <>Boundary: web search, news search, and LLM context only — there is no URL extraction endpoint. Generic OpenAI-style chat clients are not applicable; use the MCP facade or the native Brave paths.</>,
          ]
        : [
            <>将任意 HTTP MCP 客户端指向 <code>{baseUrl}/brave/mcp</code>，并携带 <code>Authorization: Bearer {prettyToken}</code>；该入口由 Hikari 本地提供。</>,
            <>该入口会通告 <code>brave_web_search</code>、<code>brave_news_search</code>、<code>brave_llm_context</code> 三个工具。</>,
            <>HTTP 客户端可调用 <code>GET {baseUrl}/api/brave/res/v1/web/search?q=...</code>，把 Hikari token 放进 <code>X-Subscription-Token</code> 请求头；同样镜像了 <code>/news/search</code> 与 <code>/llm/context</code> 原生路径。</>,
            <>Cherry Studio：选择 <strong>Brave Search</strong> 服务商，将 Base URL 设为 <code>{baseUrl}/api/brave</code>；原生 <code>/res/v1</code> 路径已镜像，支持 Base URL 覆盖的客户端无需额外映射。</>,
            <>边界：仅镜像网页搜索、新闻搜索与 LLM 上下文，没有 URL 抽取端点。通用 OpenAI 风格聊天客户端不适用；请使用 MCP 入口或 Brave 原生路径。</>,
          ],
      samples: [
        {
          title: isEnglish ? 'Example: ~/.codex/config.toml' : '示例：~/.codex/config.toml',
          language: 'toml',
          snippet: buildProviderCodexSnippet('brave_hikari', baseUrl, '/brave/mcp', 'BRAVE_HIKARI_TOKEN'),
        },
        {
          title: isEnglish ? 'Example: claude mcp add-json' : '示例：claude mcp add-json',
          language: 'bash',
          snippet: buildProviderClaudeSnippet('brave-hikari', baseUrl, '/brave/mcp', prettyToken),
        },
        {
          title: isEnglish ? 'Example: VS Code / generic mcp.json' : '示例：VS Code / 通用 mcp.json',
          language: 'json',
          snippet: buildProviderVscodeSnippet('brave-hikari', baseUrl, '/brave/mcp', prettyToken),
        },
        {
          title: isEnglish ? 'Example: GET /api/brave/res/v1/web/search' : '示例：GET /api/brave/res/v1/web/search',
          language: 'bash',
          snippet: buildBraveHttpSnippet(baseUrl, prettyToken),
        },
      ],
    },
  }
}

export function buildProviderCodexSnippet(
  serverName: string,
  baseUrl: string,
  mcpPath: string,
  tokenEnvVar: string,
): string {
  return [
    '<span class="hl-comment"># ~/.codex/config.toml</span>',
    '<span class="hl-key">experimental_use_rmcp_client</span> = <span class="hl-boolean">true</span>',
    '',
    `[<span class="hl-section">mcp_servers.${serverName}</span>]`,
    `<span class="hl-key">url</span> = <span class="hl-string">"${baseUrl}${mcpPath}"</span>`,
    `<span class="hl-key">bearer_token_env_var</span> = <span class="hl-string">"${tokenEnvVar}"</span>`,
  ].join('\n')
}

export function buildProviderClaudeSnippet(
  serverName: string,
  baseUrl: string,
  mcpPath: string,
  prettyToken: string,
): string {
  return [
    '<span class="hl-comment"># claude mcp add-json</span>',
    `claude mcp add-json ${serverName} '{`,
    '  <span class="hl-key">"type"</span>: <span class="hl-string">"http"</span>,',
    `  <span class="hl-key">"url"</span>: <span class="hl-string">"${baseUrl}${mcpPath}"</span>,`,
    '  <span class="hl-key">"headers"</span>: {',
    `    <span class="hl-key">"Authorization"</span>: <span class="hl-string">"Bearer ${prettyToken}"</span>`,
    '  }',
    "}'",
  ].join('\n')
}

export function buildProviderVscodeSnippet(
  serverName: string,
  baseUrl: string,
  mcpPath: string,
  prettyToken: string,
): string {
  return [
    '{',
    '  <span class="hl-key">"servers"</span>: {',
    `    <span class="hl-key">"${serverName}"</span>: {`,
    '      <span class="hl-key">"type"</span>: <span class="hl-string">"http"</span>,',
    `      <span class="hl-key">"url"</span>: <span class="hl-string">"${baseUrl}${mcpPath}"</span>,`,
    '      <span class="hl-key">"headers"</span>: {',
    `        <span class="hl-key">"Authorization"</span>: <span class="hl-string">"Bearer ${prettyToken}"</span>`,
    '      }',
    '    }',
    '  }',
    '}',
  ].join('\n')
}

export function buildSerpapiHttpSnippet(baseUrl: string, prettyToken: string): string {
  return `curl "${baseUrl}/api/serpapi/search?q=coffee&engine=google&api_key=${prettyToken}"`
}

export function buildExaHttpSnippet(baseUrl: string, prettyToken: string): string {
  return `curl -X POST "${baseUrl}/api/exa/search" \\
  -H "Content-Type: application/json" \\
  -H "x-api-key: ${prettyToken}" \\
  -d '{
    "query": "latest AI agent news",
    "numResults": 5
  }'`
}

export function buildBraveHttpSnippet(baseUrl: string, prettyToken: string): string {
  return `curl "${baseUrl}/api/brave/res/v1/web/search?q=coffee&count=5" \\
  -H "X-Subscription-Token: ${prettyToken}"`
}