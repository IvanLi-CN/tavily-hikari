import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '../components/ui/dropdown-menu'
import { Icon, getGuideClientIconName } from '../lib/icons'
import type { Language } from '../i18n'
import {
  type GuideContent,
  type GuideKey,
  type GuideSample,
  CLAUDE_DOC_URL,
  CODEX_DOC_URL,
  MCP_SPEC_URL,
  NOCODB_DOC_URL,
  TAVILY_SEARCH_DOC_URL,
  VSCODE_DOC_URL,
} from './runtime'
export function MobileGuideDropdown({
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

export function buildGuideContent(language: Language, baseUrl: string, prettyToken: string): Record<GuideKey, GuideContent> {
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

export function buildCodexSnippet(baseUrl: string): string {
  return [
    '<span class="hl-comment"># ~/.codex/config.toml</span>',
    '<span class="hl-key">experimental_use_rmcp_client</span> = <span class="hl-boolean">true</span>',
    '',
    '[<span class="hl-section">mcp_servers.tavily_hikari</span>]',
    `<span class="hl-key">url</span> = <span class="hl-string">"${baseUrl}/mcp"</span>`,
    '<span class="hl-key">bearer_token_env_var</span> = <span class="hl-string">"TAVILY_HIKARI_TOKEN"</span>',
  ].join('\n')
}

export function buildClaudeSnippet(baseUrl: string, prettyToken: string, language: Language): string {
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

export function buildVscodeSnippet(baseUrl: string, prettyToken: string): string {
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

export function buildGenericJsonSnippet(baseUrl: string, prettyToken: string): string {
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

export function buildGenericMcpSnippet(baseUrl: string, prettyToken: string): string {
  return `{
  <span class="hl-key">"type"</span>: <span class="hl-string">"http"</span>,
  <span class="hl-key">"url"</span>: <span class="hl-string">"${baseUrl}/mcp"</span>,
  <span class="hl-key">"headers"</span>: {
    <span class="hl-key">"Authorization"</span>: <span class="hl-string">"Bearer ${prettyToken}"</span>
  }
}`
}

export function buildApiSearchSnippet(baseUrl: string, prettyToken: string): string {
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

export function resolveGuideSamples(content: GuideContent): GuideSample[] {
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

