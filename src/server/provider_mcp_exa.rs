//! Exa LOCAL MCP facade (`/exa/mcp`, plan todo 17; amendments M12/M19/M22).
//!
//! Exa's hosted MCP answering surface is retired and its hosted registry tools
//! are frozen, so this surface is a Hikari-local facade built on the
//! provider-parameterized [`LocalMcpFacade`] framework (local_mcp_facade.rs):
//! initialize/tools-list/ping/notifications/prompts/resources are answered
//! locally (session-less, SSE-wrapped), and `tools/call` translates into a
//! pooled-key Exa REST POST executed by the lib seam
//! `TavilyProxy::proxy_provider_mcp_exa_facade_call` (x-api-key header
//! injection per the ProviderSpec; client key slots were already stripped at
//! the server boundary by todo 11 / M17).
//!
//! Catalog provenance (M19): the tool names, parameters, and inputSchema
//! mirror the official `exa-labs/exa-mcp-server` registry defaults
//! (`toolRegistry.ts`: `web_search_exa`, `web_fetch_exa`) so client prompts
//! port cleanly. `exa_answer` is a HIKARI-LOCAL name: Exa's hosted MCP
//! registry no longer offers an answering tool, but the Exa REST `/answer`
//! endpoint remains available through the pooled keys — this facade exposes it
//! under a local name (plan todo 17 documents the naming deliberately). Per
//! the plan's Must-NOT list the deprecated shims (`crawling_exa`,
//! `company_research_exa`, `deep_researcher_*`) and `agent_run` are NOT in the
//! catalog; there is no SSE/streaming, no hosted-MCP passthrough, and no
//! summarizer-style extra tools.
//!
//! M19 XOR variant: `web_fetch_exa` carries the catalog's validation-rule XOR
//! — exactly one of `urls` (1-100) / `ids` — expressed as the per-tool
//! `validate` fn (local typed invalid-arguments rejection, zero upstream);
//! no framework change is required. Per M22 every other parameter keeps the
//! minimal contract-guard shape (required `query` only) and the Exa upstream
//! enforces the advertised bounds.
//!
//! Stream-never semantics: no tool advertises `stream`, and the translation
//! runs every validated arguments object through the SAME T13 boundary gate as
//! the Exa REST facade (`prepare_exa_post_body`): a truthy `stream` smuggled
//! into tool arguments rejects locally with the documented boundary message
//! before any upstream call, and client api_key-like fields are stripped.
//! M12: Exa QPS is team-wide (10 QPS across ALL keys) — no new global limiter
//! lands in v1; per-key Retry-After backoff stays (armed by the T13 record
//! path the lib executor reuses).
//!
//! Billing (search=1, contents=1, answer=2) resolves through the committed
//! todo-2 ProviderSpec credits table and the SAME reserve→settle path as the
//! brave local facade (todo 16).

use super::local_mcp_facade::{
    LocalMcpFacade, LocalMcpToolDefinition, LocalMcpUpstreamRequest, handle_local_mcp_request_body,
};
use super::provider_mcp_serpapi::ProviderMcpFacadeContext;
use super::*;

/// serverInfo.name for the Exa facade (M19 parameterized identity).
const EXA_LOCAL_MCP_SERVER_NAME: &str = "exa-search-hikari";

/// Minimal contract guard (M22): the arguments must be an object carrying a
/// non-empty string `query`. Bounds and every other parameter are the Exa
/// upstream's to enforce — the facade only guards the required field.
fn exa_validate_required_query(
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
            if value.is_string() && !value.as_str().unwrap_or_default().trim().is_empty() =>
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

/// A supplied XOR side counts only as a NON-EMPTY array; empty/other-typed
/// values are treated as absent and fall through to the XOR error below.
fn exa_xor_side_supplied(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Array(items)) if !items.is_empty())
}

/// M19 XOR variant: `web_fetch_exa` must carry EXACTLY ONE of `urls` (1-100
/// documents) or `ids` — both sides or neither side is a local typed
/// invalid-arguments rejection (zero upstream calls, zero billing). Element
/// shapes and the 100-item cap beyond non-emptiness stay the Exa upstream's
/// to enforce (M22 minimal contract guard).
fn exa_validate_contents_xor(
    tool: &LocalMcpToolDefinition,
    arguments: Option<&Value>,
) -> Result<Value, String> {
    let expected = format!(
        "Invalid arguments for {}: provide exactly one of 'urls' or 'ids' as a non-empty array",
        tool.advertised_name
    );
    let Some(Value::Object(map)) = arguments else {
        return Err(expected);
    };
    let urls = exa_xor_side_supplied(map.get("urls"));
    let ids = exa_xor_side_supplied(map.get("ids"));
    if urls == ids {
        // Both sides supplied, or neither: XOR violated.
        return Err(expected);
    }
    Ok(Value::Object(map.clone()))
}

/// Translate validated tools/call arguments into an Exa REST POST body (M19:
/// a NEW provider translation module — the Tavily translation is never
/// touched). The MCP tool argument names ARE the Exa REST body field names
/// (registry mirror), so the arguments object serializes verbatim and then
/// runs through the SAME T13 boundary gate as the REST facade
/// (`prepare_exa_post_body`): truthy `stream` rejects locally with the
/// documented streaming-boundary message (the MCP tools never stream — `/answer`
/// is called with stream OMITTED, never true), client api_key-like fields are
/// stripped (the pooled key is injected by the lib executor, never the client
/// slot), and every other byte forwards unchanged — Exa owns request
/// validation beyond that gate (M22).
fn exa_translate(
    tool: &LocalMcpToolDefinition,
    arguments: &Value,
) -> Result<LocalMcpUpstreamRequest, String> {
    let body = serde_json::to_vec(arguments).map_err(|_| {
        format!(
            "Invalid arguments for {}: not JSON-serializable",
            tool.advertised_name
        )
    })?;
    let prepared = prepare_exa_post_body(&body)
        .map_err(|message| format!("Invalid arguments for {}: {message}", tool.advertised_name))?;
    Ok(LocalMcpUpstreamRequest::Post { body: prepared })
}

// --- inputSchema builders (mirror exa-labs/exa-mcp-server toolRegistry.ts) ---

/// web_search_exa — Exa REST `POST /search`. Registry-mirror parameters per
/// M19; `contents` requests page text/highlight snippets alongside results.
fn exa_web_search_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": "The search query (required)." },
            "numResults": { "type": "integer", "description": "Number of results to return (1-100).", "minimum": 1, "maximum": 100 },
            "type": { "type": "string", "description": "Search type.", "enum": ["neural", "keyword", "auto"] },
            "category": { "type": "string", "description": "Optional category filter for results.", "enum": ["company", "research paper", "news", "github", "tweet", "financial report", "linkedin profile"] },
            "includeDomains": { "type": "array", "items": { "type": "string" }, "description": "Only include results from these domains." },
            "excludeDomains": { "type": "array", "items": { "type": "string" }, "description": "Exclude results from these domains." },
            "startPublishedDate": { "type": "string", "description": "ISO 8601 date; only results published at/after this date." },
            "endPublishedDate": { "type": "string", "description": "ISO 8601 date; only results published at/before this date." },
            "contents": {
                "type": "object",
                "description": "Request page contents alongside the search results.",
                "properties": {
                    "text": { "type": ["boolean", "object"], "description": "Include page text content (boolean or a text-options object)." },
                    "highlights": { "type": ["boolean", "object"], "description": "Include highlight snippets (boolean or a highlights-options object)." }
                }
            }
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

/// web_fetch_exa — Exa REST `POST /contents`. The M19 XOR variant: exactly one
/// of `urls` / `ids` (documented in the descriptions; enforced by the per-tool
/// validate fn because plain JSON Schema `required` cannot express XOR).
fn exa_web_fetch_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "urls": { "type": "array", "items": { "type": "string" }, "minItems": 1, "maxItems": 100, "description": "URLs to fetch contents for. Provide exactly one of urls/ids (XOR — validated locally)." },
            "ids": { "type": "array", "items": { "type": "string" }, "minItems": 1, "description": "Exa document ids (from earlier search results) to fetch. Provide exactly one of urls/ids (XOR — validated locally)." }
        },
        "additionalProperties": false
    })
}

/// exa_answer — Exa REST `POST /answer`. HIKARI-LOCAL tool name: Exa's hosted
/// answering MCP tool no longer exists in the upstream registry, so this name
/// exists only on this facade (plan todo 17). `stream` is deliberately NOT
/// advertised and never reaches the wire (the plan's stream OMITTED rule —
/// never true); a smuggled truthy value rejects locally via the T13 gate.
fn exa_answer_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": "The question to answer from live web data (required)." },
            "model": { "type": "string", "description": "Answer model.", "enum": ["exa", "exa-pro", "exa-research", "exa-fast"] }
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

/// The Exa local-facade tool catalog (DATA). Credits resolve through the
/// ProviderSpec table via `credits_op` (search=1, contents=1, answer=2).
/// upstream_operation labels reuse the ratified REST vocabulary — the MCP call
/// IS that REST operation (T16 precedent; todo 27 owns the closed enum).
static EXA_LOCAL_MCP_TOOLS: &[LocalMcpToolDefinition] = &[
    LocalMcpToolDefinition {
        advertised_name: "web_search_exa",
        description: "Search the web with Exa — live web search returning links, metadata, and optional page contents",
        input_schema: exa_web_search_schema,
        validate: exa_validate_required_query,
        required_field: "query",
        credits_op: "search",
        upstream_path: "/search",
        upstream_operation: "exa_http_search",
    },
    LocalMcpToolDefinition {
        advertised_name: "web_fetch_exa",
        description: "Fetch clean contents of web pages by URL or Exa document id (exactly one of urls/ids)",
        input_schema: exa_web_fetch_schema,
        validate: exa_validate_contents_xor,
        // XOR guard owns the real contract; the field name only surfaces in
        // the shared validator vocabulary.
        required_field: "urls",
        credits_op: "contents",
        upstream_path: "/contents",
        upstream_operation: "exa_http_contents",
    },
    LocalMcpToolDefinition {
        advertised_name: "exa_answer",
        description: "Answer a question from live web data with citations (Hikari-local tool name; the hosted Exa answering tool is retired)",
        input_schema: exa_answer_schema,
        validate: exa_validate_required_query,
        required_field: "query",
        credits_op: "answer",
        upstream_path: "/answer",
        upstream_operation: "exa_http_answer",
    },
];

/// The Exa local MCP facade descriptor (M19 provider-parameterized identity +
/// catalog DATA + translate hook), completing the todo-16 framework's provider
/// set.
static EXA_LOCAL_MCP_FACADE: LocalMcpFacade = LocalMcpFacade {
    provider: ProviderId::Exa,
    server_name: EXA_LOCAL_MCP_SERVER_NAME,
    ingress_path: "/exa/mcp",
    tools: EXA_LOCAL_MCP_TOOLS,
    translate: exa_translate,
};

/// Exa local-facade body (replaces the todo-11 typed `provider_mcp_pending`
/// boundary for the exa surface — the last of the three provider MCP ingresses
/// to land on the local facade framework).
pub(super) async fn provider_mcp_exa_local_facade(
    state: Arc<AppState>,
    ctx: ProviderMcpFacadeContext<'_>,
) -> Result<Response<Body>, StatusCode> {
    // Tavily Control-mode / serpapi-passthrough / brave parity: a GET opening
    // an event stream is rejected locally — the facade buffers responses, so a
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
        &EXA_LOCAL_MCP_FACADE,
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
