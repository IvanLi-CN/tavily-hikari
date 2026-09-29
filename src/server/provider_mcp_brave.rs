//! Brave LOCAL MCP facade (`/brave/mcp`, plan todo 16; amendments M19/M22).
//!
//! Brave hosts no MCP server, so this surface is a Hikari-local facade built on
//! the provider-parameterized [`LocalMcpFacade`] framework (local_mcp_facade.rs):
//! initialize/tools-list/ping/notifications/prompts/resources are answered
//! locally (session-less, SSE-wrapped), and `tools/call` translates into a
//! pooled-key Brave REST GET executed by the lib seam
//! `TavilyProxy::proxy_provider_mcp_brave_facade_call`.
//!
//! Catalog provenance (M19): the three tool names, parameters, and inputSchema
//! mirror the official `@brave/brave-search-mcp-server` v2.1.4 (`brave_web_search`,
//! `brave_news_search`, `brave_llm_context`). Per M22 the local validation is the
//! minimal contract guard (required `query` only); the Brave upstream enforces the
//! advertised bounds (count/offset/token budgets). No summarizer/place/images/
//! videos/suggest/spellcheck tools, no streaming, no hosted passthrough.
//!
//! Billing (web=1, news=1, llm_context=2) resolves through the committed todo-2
//! ProviderSpec credits table and the SAME reserve→settle path as todos 12-15.

use super::local_mcp_facade::{
    LocalMcpFacade, LocalMcpToolDefinition, LocalMcpUpstreamRequest, handle_local_mcp_request_body,
};
use super::provider_mcp_serpapi::ProviderMcpFacadeContext;
use super::*;

/// serverInfo.name for the Brave facade (M19 parameterized identity).
const BRAVE_LOCAL_MCP_SERVER_NAME: &str = "brave-search-hikari";

/// Minimal contract guard (M22): the arguments must be an object carrying a
/// non-empty string `query`. Bounds and every other parameter are the Brave
/// upstream's to enforce — the facade only guards the required field.
fn brave_validate_required_query(
    tool: &LocalMcpToolDefinition,
    arguments: Option<&Value>,
) -> Result<Value, String> {
    let expected = format!(
        "Invalid arguments for {}: expected object with required '{}' field",
        tool.advertised_name, tool.required_field
    );
    let Some(Value::Object(map)) = arguments else {
        return Err(expected);
    };
    match map.get(tool.required_field) {
        Some(value)
            if value.is_string()
                && !value.as_str().unwrap_or_default().trim().is_empty() =>
        {
            // The guard matched a present object, so returning the cloned
            // arguments object is exact (avoids `.map(clone)` lint noise).
            Ok(Value::Object(map.clone()))
        }
        Some(_) => Err(format!(
            "Invalid arguments for {}: '{}' must be a non-empty string",
            tool.advertised_name, tool.required_field
        )),
        None => Err(format!(
            "Invalid arguments for {}: missing required '{}' field",
            tool.advertised_name, tool.required_field
        )),
    }
}

/// Render one JSON scalar into a Brave REST query-parameter value.
fn brave_scalar_to_param(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Bool(flag) => Some(if *flag { "true".to_string() } else { "false".to_string() }),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// Translate validated tools/call arguments into a Brave REST GET query string
/// (M19: a NEW provider translation module — the Tavily translation is never
/// touched). The official MCP server names the query field `query`; the Brave
/// REST API uses `q`. Every other schema parameter passes through under its own
/// name; array parameters (e.g. `result_filter`) expand into repeated params.
/// Unknown/extra params forward verbatim so the upstream owns bound enforcement.
fn brave_translate(
    tool: &LocalMcpToolDefinition,
    arguments: &Value,
) -> Result<LocalMcpUpstreamRequest, String> {
    let map = arguments.as_object().ok_or_else(|| {
        format!(
            "Invalid arguments for {}: expected an object",
            tool.advertised_name
        )
    })?;
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (key, value) in map {
        let rest_key = if key == "query" { "q" } else { key.as_str() };
        match value {
            Value::Null => {}
            Value::Array(items) => {
                for item in items {
                    if let Some(text) = brave_scalar_to_param(item) {
                        serializer.append_pair(rest_key, &text);
                    }
                }
            }
            scalar => {
                if let Some(text) = brave_scalar_to_param(scalar) {
                    serializer.append_pair(rest_key, &text);
                }
            }
        }
    }
    let query = serializer.finish();
    Ok(LocalMcpUpstreamRequest::Get {
        query: (!query.is_empty()).then_some(query),
    })
}

// --- inputSchema builders (mirror @brave/brave-search-mcp-server v2.1.4) -----

/// brave_web_search — Web Search API (`GET /res/v1/web/search`).
fn brave_web_search_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": "The search query (required)." },
            "country": { "type": "string", "description": "An ISO 3166-1 alpha-2 country code for market localization." },
            "search_lang": { "type": "string", "description": "An ISO 639-1 language code to prefer for search results." },
            "ui_lang": { "type": "string", "description": "An ISO 639-1 language code for UI elements." },
            "count": { "type": "integer", "description": "Number of results to return (1-20).", "minimum": 1, "maximum": 20, "default": 10 },
            "offset": { "type": "integer", "description": "Pagination offset (0-9).", "minimum": 0, "maximum": 9, "default": 0 },
            "safesearch": { "type": "string", "description": "Safe-search filtering level.", "enum": ["off", "moderate", "strict"], "default": "moderate" },
            "freshness": { "type": "string", "description": "Filter results by a discovery-time window (e.g. pd, pw, pm, or a date range)." },
            "result_filter": { "type": "array", "items": { "type": "string" }, "description": "Comma/repeat content filters (web, news, videos, faq, discussions, infobox)." },
            "extra_snippets": { "type": "boolean", "description": "Include extra result snippets in the response.", "default": false },
            "summary": { "type": "boolean", "description": "Include a summary of the result content.", "default": false }
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

/// brave_news_search — News Search API (`GET /res/v1/news/search`).
fn brave_news_search_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": "The news search query (required)." },
            "count": { "type": "integer", "description": "Number of results to return (1-50).", "minimum": 1, "maximum": 50, "default": 20 },
            "offset": { "type": "integer", "description": "Pagination offset (0-9).", "minimum": 0, "maximum": 9, "default": 0 },
            "freshness": { "type": "string", "description": "Filter results by a discovery-time window." },
            "country": { "type": "string", "description": "An ISO 3166-1 alpha-2 country code for market localization." },
            "search_lang": { "type": "string", "description": "An ISO 639-1 language code to prefer for results." },
            "safesearch": { "type": "string", "description": "Safe-search filtering level.", "enum": ["off", "moderate", "strict"], "default": "moderate" }
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

/// brave_llm_context — LLM Context API (`GET /res/v1/llm/context`).
fn brave_llm_context_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": "The query to build LLM context for (required)." },
            "count": { "type": "integer", "description": "Number of source results to consider (1-50).", "minimum": 1, "maximum": 50, "default": 20 },
            "maximum_number_of_urls": { "type": "integer", "description": "Maximum distinct URLs to include (1-50).", "minimum": 1, "maximum": 50 },
            "maximum_number_of_tokens": { "type": "integer", "description": "Token budget for the assembled context (1024-32768).", "minimum": 1024, "maximum": 32768 },
            "maximum_number_of_snippets": { "type": "integer", "description": "Maximum number of snippets to include per source." },
            "context_threshold_mode": { "type": "string", "description": "Relevance-threshold mode used to select context snippets.", "enum": ["low", "medium", "high"] },
            "freshness": { "type": "string", "description": "Filter results by a discovery-time window." }
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

/// The Brave local-facade tool catalog (DATA). Credits resolve through the
/// ProviderSpec table via `credits_op` (web=1, news=1, llm_context=2).
static BRAVE_LOCAL_MCP_TOOLS: &[LocalMcpToolDefinition] = &[
    LocalMcpToolDefinition {
        advertised_name: "brave_web_search",
        description: "Brave Search API — Web search",
        input_schema: brave_web_search_schema,
        validate: brave_validate_required_query,
        required_field: "query",
        credits_op: "web",
        upstream_path: "/res/v1/web/search",
        upstream_operation: "brave_http_web_search",
    },
    LocalMcpToolDefinition {
        advertised_name: "brave_news_search",
        description: "Brave Search API — News search",
        input_schema: brave_news_search_schema,
        validate: brave_validate_required_query,
        required_field: "query",
        credits_op: "news",
        upstream_path: "/res/v1/news/search",
        upstream_operation: "brave_http_news_search",
    },
    LocalMcpToolDefinition {
        advertised_name: "brave_llm_context",
        description: "Brave Search API — LLM context",
        input_schema: brave_llm_context_schema,
        validate: brave_validate_required_query,
        required_field: "query",
        credits_op: "llm_context",
        upstream_path: "/res/v1/llm/context",
        upstream_operation: "brave_http_llm_context",
    },
];

/// The Brave local MCP facade descriptor (M19 provider-parameterized identity +
/// catalog DATA + translate hook). Todo-17 adds an analogous Exa facade.
static BRAVE_LOCAL_MCP_FACADE: LocalMcpFacade = LocalMcpFacade {
    provider: ProviderId::Brave,
    server_name: BRAVE_LOCAL_MCP_SERVER_NAME,
    ingress_path: "/brave/mcp",
    tools: BRAVE_LOCAL_MCP_TOOLS,
    translate: brave_translate,
};

/// Brave local-facade body (replaces the todo-11 typed `provider_mcp_pending`
/// boundary for the brave surface only; Exa keeps that boundary until todo 17).
pub(super) async fn provider_mcp_brave_local_facade(
    state: Arc<AppState>,
    ctx: ProviderMcpFacadeContext<'_>,
) -> Result<Response<Body>, StatusCode> {
    // Tavily Control-mode / serpapi-passthrough parity: a GET opening an event
    // stream is rejected locally — the facade buffers responses, so a
    // stream-open GET would hang instead of serving.
    if *ctx.method == Method::GET && accepts_event_stream(ctx.original_headers) {
        return Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .body(Body::empty())
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
    }
    let system_settings = state.proxy.get_system_settings().await.map_err(|err| {
        eprintln!("get system settings failed for {}: {err}", ctx.path);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let client_ip = resolve_client_ip_info(
        ctx.remote_addr,
        ctx.original_headers,
        &TrustedClientIpSettings {
            trusted_proxy_cidrs: system_settings.trusted_proxy_cidrs.clone(),
            trusted_client_ip_headers: system_settings.trusted_client_ip_headers.clone(),
        },
    );
    let incoming_protocol_version = header_string(&ctx.auth.headers, "mcp-protocol-version");

    handle_local_mcp_request_body(
        &BRAVE_LOCAL_MCP_FACADE,
        &state,
        ctx.method,
        ctx.path,
        ctx.request_kind,
        ctx.body,
        &ctx.auth.headers,
        ctx.auth.token_id.as_deref(),
        ctx.auth.query.as_deref(),
        &client_ip,
        incoming_protocol_version.as_deref(),
    )
    .await
}
