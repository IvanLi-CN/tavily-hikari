async fn get_token_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<AuthTokenView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let tokens = state
        .proxy
        .list_access_tokens()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    match tokens.into_iter().find(|t| t.id == id) {
        Some(t) => {
            let owners = state
                .proxy
                .get_admin_token_owners(std::slice::from_ref(&t.id))
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let owner = owners.get(&t.id);
            Ok(Json(AuthTokenView::from_token_and_owner(t, owner)))
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}

#[derive(Debug, Serialize)]
struct TokenSnapshot {
    summary: TokenSummaryView,
    logs: Vec<TokenLogView>,
}

const MCP_SESSION_RETRY_AFTER_DEFAULT_SECS: i64 = 60;
const MCP_SESSION_RETRY_AFTER_MAX_SECS: i64 = 300;
const FAILURE_KIND_UPSTREAM_RATE_LIMITED_429_CODE: &str = "upstream_rate_limited_429";
const KEY_EFFECT_MCP_SESSION_RETRY_WAITED_CODE: &str = "mcp_session_retry_waited";
const KEY_EFFECT_MCP_SESSION_RETRY_SCHEDULED_CODE: &str = "mcp_session_retry_scheduled";
const KEY_EFFECT_MCP_SESSION_RETRY_WAITED_SUMMARY: &str =
    "This MCP session request waited for Retry-After before sending upstream";
const KEY_EFFECT_MCP_SESSION_RETRY_SCHEDULED_SUMMARY: &str =
    "This MCP session request hit upstream 429 and was retried once after Retry-After";

async fn sse_token(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, axum::http::Error>>>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let state = state.clone();
    let stream = stream! {
        let mut last_log_id: Option<i64> = None;
        if let Some(event) = build_token_snapshot_event(&state, &id).await { yield Ok(event); }
        if let Ok(logs) = state.proxy.token_recent_logs(&id, 1, None).await {
            last_log_id = logs.first().map(|l| l.id);
        }
        loop {
            match state.proxy.token_recent_logs(&id, 1, None).await {
                Ok(logs) => {
                    let latest = logs.first().map(|l| l.id);
                    if latest != last_log_id {
                        if let Some(event) = build_token_snapshot_event(&state, &id).await { yield Ok(event); }
                        last_log_id = latest;
                    } else {
                        let keep = Event::default().event("ping").data("{}");
                        yield Ok(keep);
                    }
                }
                Err(_) => {
                    let keep = Event::default().event("ping").data("{}");
                    yield Ok(keep);
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("")))
}

async fn build_token_snapshot_event(state: &Arc<AppState>, id: &str) -> Option<Event> {
    let now = Utc::now();
    let month_start = Utc
        .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()?
        .timestamp();
    let summary = state
        .proxy
        .token_summary_since(id, month_start, None)
        .await
        .ok()?;
    let logs = state
        .proxy
        .token_recent_logs(id, DEFAULT_LOG_LIMIT, None)
        .await
        .ok()?;
    let payload = TokenSnapshot {
        summary: summary.into(),
        logs: logs
            .into_iter()
            .map(TokenLogView::from)
            .map(|mut v| {
                if let Some(err) = v.error_message.as_ref() {
                    v.error_message = Some(redact_sensitive(err));
                }
                v
            })
            .collect(),
    };
    let json = serde_json::to_string(&payload).ok()?;
    Some(Event::default().event("snapshot").data(json))
}

fn mcp_session_retry_after_secs(headers: &HeaderMap, now_ts: i64) -> i64 {
    headers
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return None;
            }
            if let Ok(seconds) = trimmed.parse::<i64>() {
                return Some(seconds.max(0));
            }
            let parsed = httpdate::parse_http_date(trimmed).ok()?;
            let now_secs = now_ts.max(0) as u64;
            let now = std::time::UNIX_EPOCH.checked_add(Duration::from_secs(now_secs))?;
            match parsed.duration_since(now) {
                Ok(delta) => Some(delta.as_secs().min(i64::MAX as u64) as i64),
                Err(_) => Some(0),
            }
        })
        .unwrap_or(MCP_SESSION_RETRY_AFTER_DEFAULT_SECS)
        .clamp(0, MCP_SESSION_RETRY_AFTER_MAX_SECS)
}

async fn wait_for_mcp_session_retry_window(
    state: &Arc<AppState>,
    proxy_session_id: &str,
) -> Result<bool, ProxyError> {
    let mut waited = false;
    loop {
        let Some(session) = state.proxy.get_active_mcp_session(proxy_session_id).await? else {
            return Err(ProxyError::PinnedMcpSessionUnavailable);
        };
        let now = Utc::now().timestamp();
        let Some(rate_limited_until) = session.rate_limited_until else {
            return Ok(waited);
        };
        if rate_limited_until <= now {
            return Ok(waited);
        }

        waited = true;
        let wait_secs = (rate_limited_until - now).max(0) as u64;
        tokio::time::sleep(Duration::from_secs(wait_secs)).await;
    }
}

fn set_proxy_response_key_effect_if_none(
    response: &mut ProxyResponse,
    key_effect_code: &str,
    key_effect_summary: Option<&str>,
) {
    if response.key_effect_code == "none" {
        response.key_effect_code = key_effect_code.to_string();
        response.key_effect_summary = key_effect_summary.map(str::to_string);
    }
}

async fn annotate_request_log_key_effect_if_none(
    state: &Arc<AppState>,
    request_log_id: Option<i64>,
    key_effect_code: &str,
    key_effect_summary: Option<&str>,
) {
    if let Some(request_log_id) = request_log_id {
        let _ = state
            .proxy
            .annotate_request_log_key_effect_if_none(
                request_log_id,
                key_effect_code,
                key_effect_summary,
            )
            .await;
    }
}

async fn proxy_mcp_follow_up_with_retry(
    state: &Arc<AppState>,
    proxy_session_id: &str,
    proxy_request: ProxyRequest,
) -> Result<ProxyResponse, ProxyError> {
    let waited_before_send = wait_for_mcp_session_retry_window(state, proxy_session_id).await?;
    let mut first_response = state.proxy.proxy_request(proxy_request.clone()).await?;
    let first_analysis = analyze_mcp_attempt(first_response.status, &first_response.body);
    let first_rate_limited =
        first_analysis.failure_kind.as_deref() == Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429_CODE);

    if !first_rate_limited {
        if waited_before_send {
            annotate_request_log_key_effect_if_none(
                state,
                first_response.request_log_id,
                KEY_EFFECT_MCP_SESSION_RETRY_WAITED_CODE,
                Some(KEY_EFFECT_MCP_SESSION_RETRY_WAITED_SUMMARY),
            )
            .await;
            set_proxy_response_key_effect_if_none(
                &mut first_response,
                KEY_EFFECT_MCP_SESSION_RETRY_WAITED_CODE,
                Some(KEY_EFFECT_MCP_SESSION_RETRY_WAITED_SUMMARY),
            );
        }
        let _ = state
            .proxy
            .clear_mcp_session_rate_limit(proxy_session_id)
            .await;
        return Ok(first_response);
    }

    let now = Utc::now().timestamp();
    let retry_after_secs = mcp_session_retry_after_secs(&first_response.headers, now);
    let rate_limited_until = now + retry_after_secs;
    state
        .proxy
        .mark_mcp_session_rate_limited(
            proxy_session_id,
            rate_limited_until,
            first_analysis.failure_kind.as_deref(),
        )
        .await?;
    annotate_request_log_key_effect_if_none(
        state,
        first_response.request_log_id,
        KEY_EFFECT_MCP_SESSION_RETRY_SCHEDULED_CODE,
        Some(KEY_EFFECT_MCP_SESSION_RETRY_SCHEDULED_SUMMARY),
    )
    .await;

    tokio::time::sleep(Duration::from_secs(retry_after_secs.max(0) as u64)).await;

    let mut retry_response = state.proxy.proxy_request(proxy_request).await?;
    annotate_request_log_key_effect_if_none(
        state,
        retry_response.request_log_id,
        KEY_EFFECT_MCP_SESSION_RETRY_WAITED_CODE,
        Some(KEY_EFFECT_MCP_SESSION_RETRY_WAITED_SUMMARY),
    )
    .await;
    set_proxy_response_key_effect_if_none(
        &mut retry_response,
        KEY_EFFECT_MCP_SESSION_RETRY_WAITED_CODE,
        Some(KEY_EFFECT_MCP_SESSION_RETRY_WAITED_SUMMARY),
    );

    let retry_analysis = analyze_mcp_attempt(retry_response.status, &retry_response.body);
    if retry_analysis.failure_kind.as_deref() == Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429_CODE) {
        let now = Utc::now().timestamp();
        let retry_after_secs = mcp_session_retry_after_secs(&retry_response.headers, now);
        let rate_limited_until = now + retry_after_secs;
        state
            .proxy
            .mark_mcp_session_rate_limited(
                proxy_session_id,
                rate_limited_until,
                retry_analysis.failure_kind.as_deref(),
            )
            .await?;
    } else {
        let _ = state
            .proxy
            .clear_mcp_session_rate_limit(proxy_session_id)
            .await;
    }

    Ok(retry_response)
}

fn extract_token_from_query(raw_query: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(raw) = raw_query else {
        return (None, None);
    };

    if raw.is_empty() {
        return (None, None);
    }

    let mut token: Option<String> = None;
    let mut serializer = form_urlencoded::Serializer::new(String::new());

    for (key, value) in form_urlencoded::parse(raw.as_bytes()) {
        if key.eq_ignore_ascii_case("tavilyApiKey") {
            // Capture the first non-empty token value and strip it from the forwarded query.
            if token.is_none() {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    token = Some(trimmed.to_string());
                }
            }
            continue;
        }

        serializer.append_pair(&key, &value);
    }

    let serialized = serializer.finish();
    let query = if serialized.is_empty() {
        None
    } else {
        Some(serialized)
    };

    (query, token)
}

struct AuthenticatedRequestToken {
    token_id: Option<String>,
    using_dev_open_admin_fallback: bool,
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_string())
        .and_then(|raw| raw.strip_prefix("Bearer ").map(str::to_string))
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
}

fn missing_token_response() -> Result<Response<Body>, StatusCode> {
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(CONTENT_TYPE, "application/json; charset=utf-8")
        .body(Body::from("{\"error\":\"missing token\"}"))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn invalid_token_response() -> Result<Response<Body>, StatusCode> {
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(CONTENT_TYPE, "application/json; charset=utf-8")
        .body(Body::from("{\"error\":\"invalid or disabled token\"}"))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn authenticate_request_token(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    query_token: Option<String>,
) -> Result<AuthenticatedRequestToken, Response<Body>> {
    let header_token = extract_bearer_token(headers);
    let Some(token_resolution) =
        resolve_request_token(state.dev_open_admin, vec![header_token, query_token])
    else {
        return Err(missing_token_response().unwrap_or_else(|status| {
            Response::builder()
                .status(status)
                .body(Body::empty())
                .unwrap()
        }));
    };

    let valid = if token_resolution.using_dev_open_admin_fallback {
        true
    } else {
        state
            .proxy
            .validate_access_token(&token_resolution.token)
            .await
            .map_err(|_| {
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::empty())
                    .unwrap()
            })?
    };

    if !valid {
        return Err(invalid_token_response().unwrap_or_else(|status| {
            Response::builder()
                .status(status)
                .body(Body::empty())
                .unwrap()
        }));
    }

    Ok(AuthenticatedRequestToken {
        token_id: token_resolution.auth_token_id,
        using_dev_open_admin_fallback: token_resolution.using_dev_open_admin_fallback,
    })
}

fn header_string(headers: &ReqHeaderMap, name: &'static str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum McpJsonRpcMessageKind {
    Request,
    Notification,
    Response,
    Invalid,
}

#[derive(Debug, Clone, Default)]
struct McpJsonRpcBodySummary {
    contains_initialize: bool,
    request_count: usize,
    notification_count: usize,
    response_count: usize,
    invalid_count: usize,
    explicit_jsonrpc_follow_up_count: usize,
    is_batch: bool,
    is_empty_batch: bool,
}

impl McpJsonRpcBodySummary {
    fn requires_session_header(&self) -> bool {
        self.explicit_jsonrpc_follow_up_count > 0
    }

    fn is_single_response(&self) -> bool {
        !self.is_batch
            && self.response_count == 1
            && self.request_count == 0
            && self.notification_count == 0
            && self.invalid_count == 0
    }

    fn is_response_only_batch(&self) -> bool {
        self.is_batch
            && self.response_count > 0
            && self.request_count == 0
            && self.notification_count == 0
            && self.invalid_count == 0
    }
}

fn classify_mcp_jsonrpc_message(value: &Value) -> McpJsonRpcMessageKind {
    let Some(map) = value.as_object() else {
        return McpJsonRpcMessageKind::Invalid;
    };

    let explicit_jsonrpc = match map.get("jsonrpc") {
        Some(Value::String(version)) if version == "2.0" => true,
        Some(_) => return McpJsonRpcMessageKind::Invalid,
        None => false,
    };
    let has_method = map
        .get("method")
        .and_then(Value::as_str)
        .is_some_and(|method| !method.trim().is_empty());
    let has_id = map.contains_key("id");
    let id_is_non_null = map.get("id").is_some_and(|id| !id.is_null());
    let has_result = map.contains_key("result");
    let has_error = map.contains_key("error");

    if has_method {
        return if explicit_jsonrpc {
            if has_id && id_is_non_null {
                McpJsonRpcMessageKind::Request
            } else if !has_id {
                McpJsonRpcMessageKind::Notification
            } else {
                McpJsonRpcMessageKind::Invalid
            }
        } else {
            McpJsonRpcMessageKind::Request
        };
    }

    if has_id && (has_result || has_error) {
        return McpJsonRpcMessageKind::Response;
    }

    McpJsonRpcMessageKind::Invalid
}

fn summarize_mcp_jsonrpc_body(body: &[u8]) -> Result<McpJsonRpcBodySummary, serde_json::Error> {
    let parsed = serde_json::from_slice::<Value>(body)?;
    let mut summary = McpJsonRpcBodySummary::default();

    let mut record = |item: &Value| {
        let explicit_jsonrpc = item
            .get("jsonrpc")
            .and_then(Value::as_str)
            .is_some_and(|version| version == "2.0");
        let method_name = item
            .get("method")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|method| !method.is_empty());
        let requires_session_header =
            explicit_jsonrpc && method_name.is_some_and(|method| method != "initialize");
        match classify_mcp_jsonrpc_message(item) {
            McpJsonRpcMessageKind::Request => {
                summary.request_count += 1;
                summary.contains_initialize |= method_name.is_some_and(|method| method == "initialize");
                if requires_session_header {
                    summary.explicit_jsonrpc_follow_up_count += 1;
                }
            }
            McpJsonRpcMessageKind::Notification => {
                summary.notification_count += 1;
                if requires_session_header {
                    summary.explicit_jsonrpc_follow_up_count += 1;
                }
            }
            McpJsonRpcMessageKind::Response => {
                summary.response_count += 1;
            }
            McpJsonRpcMessageKind::Invalid => summary.invalid_count += 1,
        }
    };

    match parsed {
        Value::Object(_) => record(&parsed),
        Value::Array(items) => {
            summary.is_batch = true;
            if items.is_empty() {
                summary.is_empty_batch = true;
            } else {
                for item in &items {
                    record(item);
                }
            }
        }
        _ => summary.invalid_count += 1,
    }

    Ok(summary)
}

fn is_mcp_session_delete_request(method: &Method, path: &str) -> bool {
    *method == Method::DELETE && path == "/mcp"
}

fn is_mcp_session_delete_unsupported_response(
    method: &Method,
    path: &str,
    status: StatusCode,
    tavily_status_code: Option<i64>,
    failure_kind: Option<&str>,
    body: &[u8],
) -> bool {
    is_mcp_session_delete_request(method, path)
        && status == StatusCode::METHOD_NOT_ALLOWED
        && tavily_status_code == Some(StatusCode::METHOD_NOT_ALLOWED.as_u16() as i64)
        && failure_kind == Some("mcp_method_405")
        && String::from_utf8_lossy(body)
            .to_ascii_lowercase()
            .contains("session termination not supported")
}

fn mcp_session_response(
    status: StatusCode,
    error: &str,
    message: &str,
) -> Result<Response<Body>, StatusCode> {
    let payload = json!({
        "error": error,
        "message": message,
    });

    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "application/json; charset=utf-8")
        .body(Body::from(payload.to_string()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn mcp_session_body(error: &str, message: &str) -> Bytes {
    Bytes::from(
        json!({
            "error": error,
            "message": message,
        })
        .to_string(),
    )
}

fn mcp_response_requires_reconnect(status: StatusCode, body: &[u8]) -> bool {
    if !status.is_client_error() {
        return false;
    }

    let lower = String::from_utf8_lossy(body).to_ascii_lowercase();
    lower.contains("missing mcp-session-id header")
        || lower.contains("session not found")
        || lower.contains("unknown session")
        || lower.contains("invalid session")
        || lower.contains("session expired")
}

const REBALANCE_MCP_PROTOCOL_VERSION_DEFAULT: &str = "2025-03-26";
const REBALANCE_MCP_SERVER_NAME: &str = "tavily-mcp";
const REBALANCE_MCP_SERVER_VERSION: &str = "3.2.4";

#[derive(Clone, Copy)]
enum RebalanceMcpRequiredFieldType {
    String,
    StringOrStringArray,
}

#[derive(Clone, Copy)]
struct RebalanceMcpToolDefinition {
    advertised_name: &'static str,
    hyphen_name: &'static str,
    upstream_tool: &'static str,
    description: &'static str,
    required_field: &'static str,
    required_field_type: RebalanceMcpRequiredFieldType,
}

const REBALANCE_MCP_TOOL_DEFINITIONS: [RebalanceMcpToolDefinition; 5] = [
    RebalanceMcpToolDefinition {
        advertised_name: "tavily_search",
        hyphen_name: "tavily-search",
        upstream_tool: "search",
        description: "Search the web with Tavily",
        required_field: "query",
        required_field_type: RebalanceMcpRequiredFieldType::String,
    },
    RebalanceMcpToolDefinition {
        advertised_name: "tavily_extract",
        hyphen_name: "tavily-extract",
        upstream_tool: "extract",
        description: "Extract page content with Tavily",
        required_field: "urls",
        required_field_type: RebalanceMcpRequiredFieldType::StringOrStringArray,
    },
    RebalanceMcpToolDefinition {
        advertised_name: "tavily_crawl",
        hyphen_name: "tavily-crawl",
        upstream_tool: "crawl",
        description: "Crawl a site with Tavily",
        required_field: "url",
        required_field_type: RebalanceMcpRequiredFieldType::String,
    },
    RebalanceMcpToolDefinition {
        advertised_name: "tavily_map",
        hyphen_name: "tavily-map",
        upstream_tool: "map",
        description: "Map a site with Tavily",
        required_field: "url",
        required_field_type: RebalanceMcpRequiredFieldType::String,
    },
    RebalanceMcpToolDefinition {
        advertised_name: "tavily_research",
        hyphen_name: "tavily-research",
        upstream_tool: "research",
        description: "Run Tavily research",
        required_field: "input",
        required_field_type: RebalanceMcpRequiredFieldType::String,
    },
];

fn rebalance_mcp_tool_definition_by_name(tool: &str) -> Option<&'static RebalanceMcpToolDefinition> {
    let normalized = tool.trim().to_ascii_lowercase().replace('_', "-");
    REBALANCE_MCP_TOOL_DEFINITIONS
        .iter()
        .find(|definition| definition.hyphen_name == normalized)
}

fn schema_string(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

fn schema_integer(description: &str, default: i64) -> Value {
    json!({ "type": "integer", "description": description, "default": default })
}

fn schema_boolean(description: &str, default: bool) -> Value {
    json!({ "type": "boolean", "description": description, "default": default })
}

fn rebalance_mcp_required_field_schema(kind: RebalanceMcpRequiredFieldType) -> Value {
    match kind {
        RebalanceMcpRequiredFieldType::String => json!({ "type": "string" }),
        RebalanceMcpRequiredFieldType::StringOrStringArray => json!({
            "oneOf": [
                { "type": "string" },
                {
                    "type": "array",
                    "items": { "type": "string" }
                }
            ]
        }),
    }
}

fn rebalance_mcp_tool_input_schema(tool: &RebalanceMcpToolDefinition) -> Value {
    let properties = match tool.upstream_tool {
        "search" => json!({
            "query": schema_string("Search query"),
            "max_results": schema_integer("The maximum number of search results to return", 5),
            "search_depth": {
                "type": "string",
                "description": "The depth of the search. 'basic' for generic results, 'advanced' for more thorough search, 'fast' for optimized low latency with high relevance, 'ultra-fast' for prioritizing latency above all else",
                "enum": ["basic", "advanced", "fast", "ultra-fast"],
                "default": "basic"
            },
            "topic": {
                "type": "string",
                "description": "The category of the search.",
                "const": "general",
                "default": "general"
            },
            "include_domains": {
                "type": "array",
                "items": { "type": "string" },
                "description": "A list of domains to specifically include in the search results"
            },
            "exclude_domains": {
                "type": "array",
                "items": { "type": "string" },
                "description": "A list of domains to specifically exclude from the search results"
            },
            "time_range": schema_string("Time range filter for search results"),
            "start_date": schema_string("Start date for filtering search results"),
            "end_date": schema_string("End date for filtering search results"),
            "country": schema_string("Country filter for search results"),
            "include_images": schema_boolean("Include a list of query-related images in the response", false),
            "include_image_descriptions": schema_boolean("Include descriptions for returned images", false),
            "include_raw_content": schema_boolean("Include cleaned and parsed HTML content for each search result", false),
            "include_favicon": schema_boolean("Include favicon URLs for each result", true),
            "exact_match": schema_boolean("Only return results that exactly match the query", false)
        }),
        "extract" => json!({
            "urls": rebalance_mcp_required_field_schema(tool.required_field_type),
            "extract_depth": {
                "type": "string",
                "description": "The depth of the extraction process",
                "enum": ["basic", "advanced"],
                "default": "basic"
            },
            "format": {
                "type": "string",
                "description": "The format of the extracted content",
                "enum": ["markdown", "text"],
                "default": "markdown"
            },
            "query": schema_string("Optional query to guide extraction"),
            "include_images": schema_boolean("Include images extracted from the page", false),
            "include_favicon": schema_boolean("Include favicon URLs in the response", true)
        }),
        "crawl" => json!({
            "url": schema_string("The root URL to crawl"),
            "max_depth": schema_integer("Maximum crawl depth", 1),
            "max_breadth": schema_integer("Maximum number of links to follow per level", 20),
            "limit": schema_integer("Maximum number of pages to crawl", 50),
            "instructions": schema_string("Instructions to guide the crawl"),
            "select_paths": {
                "type": "array",
                "items": { "type": "string" },
                "description": "URL path patterns to include"
            },
            "select_domains": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Domains to include"
            },
            "allow_external": schema_boolean("Allow crawling external domains", false),
            "extract_depth": {
                "type": "string",
                "description": "The depth of the extraction process",
                "enum": ["basic", "advanced"],
                "default": "basic"
            },
            "format": {
                "type": "string",
                "description": "The format of the extracted content",
                "enum": ["markdown", "text"],
                "default": "markdown"
            },
            "include_favicon": schema_boolean("Include favicon URLs in the response", true)
        }),
        "map" => json!({
            "url": schema_string("The root URL to map"),
            "max_depth": schema_integer("Maximum crawl depth", 1),
            "max_breadth": schema_integer("Maximum number of links to follow per level", 20),
            "limit": schema_integer("Maximum number of URLs to return", 50),
            "instructions": schema_string("Instructions to guide site mapping"),
            "select_paths": {
                "type": "array",
                "items": { "type": "string" },
                "description": "URL path patterns to include"
            },
            "select_domains": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Domains to include"
            },
            "allow_external": schema_boolean("Allow mapping external domains", false)
        }),
        "research" => json!({
            "input": schema_string("Research task or question"),
            "model": schema_string("Research model to use")
        }),
        _ => json!({
            tool.required_field: rebalance_mcp_required_field_schema(tool.required_field_type)
        }),
    };

    json!({
        "type": "object",
        "properties": properties,
        "required": [tool.required_field],
        "additionalProperties": false,
    })
}

fn rebalance_mcp_argument_matches_type(value: &Value, kind: RebalanceMcpRequiredFieldType) -> bool {
    match kind {
        RebalanceMcpRequiredFieldType::String => value.is_string(),
        RebalanceMcpRequiredFieldType::StringOrStringArray => {
            value.is_string()
                || value
                    .as_array()
                    .is_some_and(|items| items.iter().all(Value::is_string))
        }
    }
}

fn validate_rebalance_mcp_tool_arguments(
    tool: &RebalanceMcpToolDefinition,
    arguments: Option<&Value>,
) -> Result<Value, String> {
    let Some(arguments) = arguments else {
        return Err(format!(
            "Invalid arguments for {}: expected object with required '{}' field",
            tool.advertised_name, tool.required_field
        ));
    };
    let Value::Object(_) = arguments else {
        return Err(format!(
            "Invalid arguments for {}: expected object with required '{}' field",
            tool.advertised_name, tool.required_field
        ));
    };
    let Some(required_value) = arguments.get(tool.required_field) else {
        return Err(format!(
            "Invalid arguments for {}: missing required '{}' field",
            tool.advertised_name, tool.required_field
        ));
    };
    if !rebalance_mcp_argument_matches_type(required_value, tool.required_field_type) {
        return Err(format!(
            "Invalid arguments for {}: '{}' must match the advertised input schema",
            tool.advertised_name, tool.required_field
        ));
    }
    if tool.upstream_tool == "research"
        && let Some(message) = tavily_research_model_validation_message(arguments)
    {
        return Err(format!(
            "Invalid arguments for {}: {message}",
            tool.advertised_name
        ));
    }
    Ok(arguments.clone())
}

fn rebalance_mcp_tool_usage_metered(tool: &RebalanceMcpToolDefinition) -> bool {
    matches!(tool.upstream_tool, "search" | "extract" | "crawl" | "map")
}

fn stable_rebalance_bucket(proxy_session_id: &str) -> i64 {
    let digest = Sha256::digest(proxy_session_id.as_bytes());
    let bucket_seed = u64::from_be_bytes([
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
    ]);
    (bucket_seed % 100) as i64
}

fn hash_routing_subject(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let digest = Sha256::digest(trimmed.as_bytes());
    Some(
        digest[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    )
}

fn build_rebalance_mcp_success_body(response_id: Option<&Value>, result: Value) -> Vec<u8> {
    let mut envelope = serde_json::Map::new();
    envelope.insert("jsonrpc".to_string(), Value::String("2.0".to_string()));
    if let Some(id) = response_id {
        envelope.insert("id".to_string(), id.clone());
    }
    envelope.insert("result".to_string(), result);
    serde_json::to_vec(&Value::Object(envelope)).unwrap_or_else(|_| {
        br#"{"jsonrpc":"2.0","result":{"content":[],"isError":true,"structuredContent":{"status":500}}}"#
            .to_vec()
    })
}

fn build_rebalance_mcp_error_body(
    response_id: Option<&Value>,
    code: i64,
    message: &str,
) -> Vec<u8> {
    let mut envelope = serde_json::Map::new();
    envelope.insert("jsonrpc".to_string(), Value::String("2.0".to_string()));
    if let Some(id) = response_id {
        envelope.insert("id".to_string(), id.clone());
    } else {
        envelope.insert("id".to_string(), Value::Null);
    }
    envelope.insert(
        "error".to_string(),
        json!({
            "code": code,
            "message": message,
        }),
    );
    serde_json::to_vec(&Value::Object(envelope)).unwrap_or_else(|_| {
        br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"internal error"}}"#
            .to_vec()
    })
}

fn build_rebalance_mcp_tool_error_result_body(
    response_id: Option<&Value>,
    message: &str,
) -> Vec<u8> {
    build_rebalance_mcp_success_body(
        response_id,
        json!({
            "content": [
                {
                    "type": "text",
                    "text": message
                }
            ],
            "isError": true
        }),
    )
}

fn wrap_mcp_sse_message_body(body: &[u8]) -> Vec<u8> {
    if body.is_empty() {
        return Vec::new();
    }
    let payload = String::from_utf8_lossy(body);
    format!("event: message\ndata: {payload}\n\n").into_bytes()
}

fn parse_mcp_sse_message_body(body: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(body).ok()?;
    let data = text
        .lines()
        .find_map(|line| line.strip_prefix("data:").map(str::trim))?;
    serde_json::from_str(data).ok()
}

#[allow(clippy::too_many_arguments)]
async fn build_and_log_local_mcp_protocol_response(
    state: &Arc<AppState>,
    token_id: Option<&str>,
    method: &Method,
    path: &str,
    query: Option<&str>,
    request_body: &[u8],
    response_status: StatusCode,
    response_body: &[u8],
    failure_kind: Option<&str>,
    gateway_mode: Option<&str>,
    experiment_variant: Option<&str>,
    proxy_session_id: Option<&str>,
    routing_subject_hash: Option<&str>,
    upstream_operation: Option<&str>,
    fallback_reason: Option<&str>,
    sse_transport: bool,
) -> Result<Response<Body>, StatusCode> {
    let analysis = analyze_mcp_attempt(response_status, response_body);
    let request_kind = classify_token_request_kind(path, Some(request_body));
    let empty_headers: [String; 0] = [];
    let request_log_id = match state
        .proxy
        .record_local_request_log_without_key_with_diagnostics(
            token_id,
            method,
            path,
            query,
            response_status,
            analysis.tavily_status_code,
            request_body,
            response_body,
            analysis.status,
            failure_kind.or(analysis.failure_kind.as_deref()),
            gateway_mode,
            experiment_variant,
            proxy_session_id,
            routing_subject_hash,
            upstream_operation,
            fallback_reason,
            &empty_headers,
            &empty_headers,
        )
        .await
    {
        Ok(log_id) => Some(log_id),
        Err(err) => {
            eprintln!("local MCP protocol request_log failed for {path}: {err}");
            None
        }
    };

    if let Some(token_id) = token_id {
        let _ = state
            .proxy
            .record_token_attempt_with_kind_request_log_metadata(
                token_id,
                method,
                path,
                query,
                Some(response_status.as_u16() as i64),
                analysis.tavily_status_code,
                false,
                analysis.status,
                None,
                &request_kind,
                failure_kind.or(analysis.failure_kind.as_deref()),
                Some("none"),
                None,
                None,
                None,
                None,
                None,
                request_log_id,
            )
            .await;
    }

    let (content_type, body) = if sse_transport && !response_body.is_empty() {
        (
            "text/event-stream",
            wrap_mcp_sse_message_body(response_body),
        )
    } else {
        (
            "application/json; charset=utf-8",
            response_body.to_vec(),
        )
    };

    Response::builder()
        .status(response_status)
        .header(CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn build_rebalance_mcp_initialize_body(
    response_id: Option<&Value>,
    protocol_version: &str,
) -> Vec<u8> {
    build_rebalance_mcp_success_body(
        response_id,
        json!({
            "protocolVersion": protocol_version,
            "capabilities": {
                "tools": {
                    "listChanged": false
                },
                "prompts": {
                    "listChanged": false
                },
                "resources": {}
            },
            "serverInfo": {
                "name": REBALANCE_MCP_SERVER_NAME,
                "version": REBALANCE_MCP_SERVER_VERSION
            }
        }),
    )
}

fn rebalance_mcp_tools_descriptor() -> Vec<Value> {
    REBALANCE_MCP_TOOL_DEFINITIONS
        .iter()
        .map(|tool| {
            json!({
                "name": tool.advertised_name,
                "description": tool.description,
                "inputSchema": rebalance_mcp_tool_input_schema(tool),
            })
        })
        .collect()
}

fn build_rebalance_mcp_tools_list_body(response_id: Option<&Value>) -> Vec<u8> {
    build_rebalance_mcp_success_body(
        response_id,
        json!({
            "tools": rebalance_mcp_tools_descriptor(),
        }),
    )
}

fn build_rebalance_mcp_prompts_list_body(response_id: Option<&Value>) -> Vec<u8> {
    build_rebalance_mcp_success_body(
        response_id,
        json!({
            "prompts": [],
        }),
    )
}

fn build_rebalance_mcp_resources_list_body(response_id: Option<&Value>) -> Vec<u8> {
    build_rebalance_mcp_success_body(
        response_id,
        json!({
            "resources": [],
        }),
    )
}

fn build_rebalance_mcp_resource_templates_list_body(response_id: Option<&Value>) -> Vec<u8> {
    build_rebalance_mcp_success_body(
        response_id,
        json!({
            "resourceTemplates": [],
        }),
    )
}

fn build_rebalance_mcp_ping_body(response_id: Option<&Value>) -> Vec<u8> {
    build_rebalance_mcp_success_body(response_id, json!({}))
}

fn rebalance_initialize_protocol_version(
    request: &Value,
    incoming_protocol_version: Option<&str>,
) -> String {
    request
        .get("params")
        .and_then(|params| params.get("protocolVersion"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or(incoming_protocol_version)
        .unwrap_or(REBALANCE_MCP_PROTOCOL_VERSION_DEFAULT)
        .to_string()
}

fn proxy_response_with_json_body(
    status: StatusCode,
    body: Vec<u8>,
    request_log_id: Option<i64>,
) -> ProxyResponse {
    let mut headers = ReqHeaderMap::new();
    if !body.is_empty() {
        headers.insert(
            CONTENT_TYPE,
            ReqHeaderValue::from_static("application/json; charset=utf-8"),
        );
    }
    ProxyResponse {
        status,
        headers: headers.clone(),
        body: Bytes::from(body),
        api_key_id: None,
        request_log_id,
        key_effect_code: "none".to_string(),
        key_effect_summary: None,
        binding_effect_code: "none".to_string(),
        binding_effect_summary: None,
        selection_effect_code: "none".to_string(),
        selection_effect_summary: None,
    }
}

fn proxy_response_with_mcp_body(
    status: StatusCode,
    body: Vec<u8>,
    request_log_id: Option<i64>,
) -> ProxyResponse {
    let sse_body = wrap_mcp_sse_message_body(&body);
    let mut response = proxy_response_with_json_body(status, sse_body, request_log_id);
    if !response.body.is_empty() {
        response.headers.insert(
            CONTENT_TYPE,
            ReqHeaderValue::from_static("text/event-stream"),
        );
    }
    response
}

#[allow(clippy::too_many_arguments)]
async fn log_rebalance_local_control_plane_response(
    state: &Arc<AppState>,
    token_id: Option<&str>,
    method: &Method,
    path: &str,
    request_body: &[u8],
    response_status: StatusCode,
    response_body: &[u8],
    proxy_session_id: Option<&str>,
    routing_subject_hash: Option<&str>,
    fallback_reason: Option<&str>,
) -> Option<i64> {
    let analysis = analyze_mcp_attempt(response_status, response_body);
    let failure_kind = match fallback_reason {
        Some("unknown_tool") => Some("unknown_tool_name"),
        Some("invalid_tool_arguments") | Some("invalid_tool_params") => {
            match analysis.failure_kind.as_deref() {
                Some("invalid_search_depth")
                | Some("invalid_country_search_depth_combo")
                | Some("tool_argument_validation") => analysis.failure_kind.as_deref(),
                _ => Some("tool_argument_validation"),
            }
        }
        _ => analysis.failure_kind.as_deref(),
    };
    let empty_headers: [String; 0] = [];
    match state
        .proxy
        .record_local_request_log_without_key_with_diagnostics(
            token_id,
            method,
            path,
            None,
            response_status,
            analysis.tavily_status_code,
            request_body,
            response_body,
            analysis.status,
            failure_kind,
            Some(tavily_hikari::MCP_GATEWAY_MODE_REBALANCE),
            Some(tavily_hikari::MCP_EXPERIMENT_VARIANT_REBALANCE),
            proxy_session_id,
            routing_subject_hash,
            Some("mcp"),
            fallback_reason,
            &empty_headers,
            &empty_headers,
        )
        .await
    {
        Ok(log_id) => Some(log_id),
        Err(err) => {
            eprintln!("local rebalance MCP request_log failed for {path}: {err}");
            None
        }
    }
}
