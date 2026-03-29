use crate::models::*;
use crate::*;

pub(crate) fn analyze_attempt(status: StatusCode, body: &[u8]) -> AttemptAnalysis {
    if !status.is_success() {
        let tavily_status_code = Some(status.as_u16() as i64);
        return AttemptAnalysis {
            status: OUTCOME_ERROR,
            tavily_status_code,
            key_health_action: classify_quarantine_reason(tavily_status_code, body)
                .map(KeyHealthAction::Quarantine)
                .unwrap_or(KeyHealthAction::None),
            failure_kind: classify_failure_kind(
                "/mcp",
                tavily_status_code,
                tavily_status_code,
                None,
                body,
            ),
            key_effect: KeyEffect::none(),
            api_key_id: None,
        };
    }

    let text = match std::str::from_utf8(body) {
        Ok(text) => text,
        Err(_) => {
            return AttemptAnalysis {
                status: OUTCOME_UNKNOWN,
                tavily_status_code: None,
                key_health_action: KeyHealthAction::None,
                failure_kind: None,
                key_effect: KeyEffect::none(),
                api_key_id: None,
            };
        }
    };

    let mut any_success = false;
    let mut any_error = false;
    let mut detected_code = None;
    let mut messages = extract_sse_json_messages(text);
    if messages.is_empty()
        && let Ok(value) = serde_json::from_str::<Value>(text)
    {
        match value {
            // JSON-RPC batch responses return an array of message envelopes. Treat each element
            // as its own message so we can correctly detect success/error and enforce billing.
            Value::Array(items) => messages.extend(items),
            other => messages.push(other),
        }
    }

    for message in messages {
        if let Some((outcome, code)) = analyze_json_message(&message) {
            if detected_code.is_none() {
                detected_code = code;
            }
            match outcome {
                MessageOutcome::QuotaExhausted => {
                    return AttemptAnalysis {
                        status: OUTCOME_QUOTA_EXHAUSTED,
                        tavily_status_code: code.or(detected_code),
                        key_health_action: KeyHealthAction::MarkExhausted,
                        failure_kind: None,
                        key_effect: KeyEffect::new(
                            KEY_EFFECT_MARKED_EXHAUSTED,
                            "The system automatically marked this key as exhausted",
                        ),
                        api_key_id: None,
                    };
                }
                MessageOutcome::Error => {
                    any_error = true;
                }
                MessageOutcome::Success => any_success = true,
            }
        }
    }

    if any_error {
        return AttemptAnalysis {
            status: OUTCOME_ERROR,
            tavily_status_code: detected_code,
            key_health_action: classify_quarantine_reason(detected_code, body)
                .map(KeyHealthAction::Quarantine)
                .unwrap_or(KeyHealthAction::None),
            failure_kind: classify_failure_kind(
                "/mcp",
                Some(status.as_u16() as i64),
                detected_code,
                None,
                body,
            ),
            key_effect: KeyEffect::none(),
            api_key_id: None,
        };
    }

    if any_success {
        return AttemptAnalysis {
            status: OUTCOME_SUCCESS,
            tavily_status_code: detected_code,
            key_health_action: KeyHealthAction::None,
            failure_kind: None,
            key_effect: KeyEffect::none(),
            api_key_id: None,
        };
    }

    AttemptAnalysis {
        status: OUTCOME_UNKNOWN,
        tavily_status_code: detected_code,
        key_health_action: KeyHealthAction::None,
        failure_kind: None,
        key_effect: KeyEffect::none(),
        api_key_id: None,
    }
}

/// Analyze a single Tavily HTTP JSON response (e.g. `/search`) using HTTP status and
/// optional structured `status` field from the body.
pub fn analyze_http_attempt(status: StatusCode, body: &[u8]) -> AttemptAnalysis {
    let http_code = status.as_u16() as i64;

    let parsed = serde_json::from_slice::<Value>(body).ok();
    let structured = parsed.as_ref().and_then(extract_status_code);
    let structured_outcome = parsed
        .as_ref()
        .and_then(extract_status_text)
        .and_then(classify_status_text);

    let effective = structured.unwrap_or(http_code);
    let mut outcome = if let Some(code) = structured {
        let code_outcome = classify_status_code(code);
        if matches!(code_outcome, MessageOutcome::Success) {
            structured_outcome.unwrap_or(code_outcome)
        } else {
            code_outcome
        }
    } else {
        structured_outcome.unwrap_or_else(|| classify_status_code(effective))
    };

    // If HTTP status itself is an error, never treat the outcome as success.
    if !status.is_success() && matches!(outcome, MessageOutcome::Success) {
        outcome = if effective == 432 {
            MessageOutcome::QuotaExhausted
        } else {
            MessageOutcome::Error
        };
    }

    let (status_str, key_health_action) = match outcome {
        MessageOutcome::Success => (OUTCOME_SUCCESS, KeyHealthAction::None),
        MessageOutcome::Error => (
            OUTCOME_ERROR,
            classify_quarantine_reason(Some(effective), body)
                .map(KeyHealthAction::Quarantine)
                .unwrap_or(KeyHealthAction::None),
        ),
        MessageOutcome::QuotaExhausted => (OUTCOME_QUOTA_EXHAUSTED, KeyHealthAction::MarkExhausted),
    };

    AttemptAnalysis {
        status: status_str,
        tavily_status_code: Some(effective),
        key_health_action,
        failure_kind: if matches!(outcome, MessageOutcome::Error) {
            classify_failure_kind("/api/tavily", Some(http_code), Some(effective), None, body)
        } else {
            None
        },
        key_effect: if matches!(outcome, MessageOutcome::QuotaExhausted) {
            KeyEffect::new(
                KEY_EFFECT_MARKED_EXHAUSTED,
                "The system automatically marked this key as exhausted",
            )
        } else {
            KeyEffect::none()
        },
        api_key_id: None,
    }
}

/// Analyze a Tavily MCP JSON-RPC response (e.g. `/mcp tools/call`) using the same heuristics
/// as the core proxy request logger (supports JSON-RPC envelopes and SSE message streams).
pub fn analyze_mcp_attempt(status: StatusCode, body: &[u8]) -> AttemptAnalysis {
    analyze_attempt(status, body)
}

fn combined_failure_text(error_message: Option<&str>, body: &[u8]) -> String {
    let mut combined = String::new();
    if let Some(message) = error_message
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        combined.push_str(message);
    }
    if !body.is_empty()
        && let Ok(text) = std::str::from_utf8(body)
    {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            if !combined.is_empty() {
                combined.push('\n');
            }
            combined.push_str(trimmed);
        }
    }
    combined
}

pub(crate) fn classify_failure_kind(
    path: &str,
    http_status: Option<i64>,
    tavily_status: Option<i64>,
    error_message: Option<&str>,
    body: &[u8],
) -> Option<String> {
    let normalized_path = path.trim().to_ascii_lowercase();
    let combined = combined_failure_text(error_message, body);
    let normalized = combined.to_ascii_lowercase();
    let effective_status = tavily_status.or(http_status);

    if normalized.contains("error sending request for url")
        || normalized.contains("dns error")
        || normalized.contains("connection refused")
        || normalized.contains("tls handshake")
    {
        return Some(FAILURE_KIND_TRANSPORT_SEND_ERROR.to_string());
    }

    if matches!(http_status, Some(502..=504)) {
        return Some(FAILURE_KIND_UPSTREAM_GATEWAY_5XX.to_string());
    }

    if tavily_status == Some(429)
        || (http_status == Some(429) && normalized.contains("excessive requests"))
    {
        return Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429.to_string());
    }

    if matches!(effective_status, Some(401 | 403))
        && (normalized.contains("deactivated")
            || normalized.contains("revoked")
            || normalized.contains("invalid api key")
            || normalized.contains("api key is invalid"))
    {
        return Some(FAILURE_KIND_UPSTREAM_ACCOUNT_DEACTIVATED_401.to_string());
    }

    if http_status == Some(406)
        || normalized.contains("must accept both application/json and text/event-stream")
    {
        return Some(FAILURE_KIND_MCP_ACCEPT_406.to_string());
    }

    if http_status == Some(405) {
        return Some(FAILURE_KIND_MCP_METHOD_405.to_string());
    }

    if http_status == Some(404) && normalized_path.starts_with("/mcp") {
        return Some(FAILURE_KIND_MCP_PATH_404.to_string());
    }

    if normalized.contains("unknown tool") {
        return Some(FAILURE_KIND_UNKNOWN_TOOL_NAME.to_string());
    }

    if normalized.contains("unexpected keyword argument")
        || normalized.contains("input should be a valid")
        || normalized.contains("validation error")
    {
        return Some(FAILURE_KIND_TOOL_ARGUMENT_VALIDATION.to_string());
    }

    if normalized.contains("search depth")
        && (normalized.contains("ultra-fast")
            || normalized.contains("advanced")
            || normalized.contains("basic"))
    {
        return Some(FAILURE_KIND_INVALID_SEARCH_DEPTH.to_string());
    }

    if normalized.contains("country parameter is not supported for fast or ultra-fast search_depth")
    {
        return Some(FAILURE_KIND_INVALID_COUNTRY_SEARCH_DEPTH_COMBO.to_string());
    }

    if http_status == Some(422) && normalized_path == "/api/tavily/research" {
        return Some(FAILURE_KIND_RESEARCH_PAYLOAD_422.to_string());
    }

    if normalized.contains("max query length is") || normalized.contains("query is too long") {
        return Some(FAILURE_KIND_QUERY_TOO_LONG.to_string());
    }

    if effective_status.is_some() || !normalized.is_empty() {
        return Some(FAILURE_KIND_OTHER.to_string());
    }

    None
}

pub fn failure_kind_solution_guidance(kind: &str, prefer_zh: bool) -> Option<&'static str> {
    match kind {
        FAILURE_KIND_UPSTREAM_GATEWAY_5XX => Some(if prefer_zh {
            "建议：这是上游网关临时故障，可稍后重试；若持续出现，请检查上游连通性与代理健康状态。"
        } else {
            "Suggested handling: this is a temporary upstream gateway failure. Retry later and inspect upstream connectivity or proxy health."
        }),
        FAILURE_KIND_UPSTREAM_RATE_LIMITED_429 => Some(if prefer_zh {
            "建议：这是 Tavily 限流，请降低请求频率或切换其他 Key，稍后再试。"
        } else {
            "Suggested handling: Tavily is rate limiting this traffic. Reduce request rate, switch keys, or retry after cooldown."
        }),
        FAILURE_KIND_UPSTREAM_ACCOUNT_DEACTIVATED_401 => Some(if prefer_zh {
            "建议：该 Key 可能已失效、被撤销或账户停用，请更换可用 Key 并检查 Tavily 后台状态。"
        } else {
            "Suggested handling: this key may be invalid, revoked, or tied to a deactivated account. Replace it and verify the Tavily account state."
        }),
        FAILURE_KIND_TRANSPORT_SEND_ERROR => Some(if prefer_zh {
            "建议：这是链路/网络发送失败，请检查 DNS、TLS、代理链路或上游可达性。"
        } else {
            "Suggested handling: this request failed before getting an upstream response. Check DNS, TLS, proxy routing, and upstream reachability."
        }),
        FAILURE_KIND_MCP_ACCEPT_406 => Some(if prefer_zh {
            "建议：客户端需要同时接受 application/json 与 text/event-stream，请修正 Accept 请求头。"
        } else {
            "Suggested handling: the client must accept both application/json and text/event-stream. Fix the Accept header negotiation."
        }),
        _ => None,
    }
}

pub fn should_append_solution_guidance(kind: &str) -> bool {
    matches!(
        kind,
        FAILURE_KIND_UPSTREAM_GATEWAY_5XX
            | FAILURE_KIND_UPSTREAM_RATE_LIMITED_429
            | FAILURE_KIND_UPSTREAM_ACCOUNT_DEACTIVATED_401
            | FAILURE_KIND_TRANSPORT_SEND_ERROR
            | FAILURE_KIND_MCP_ACCEPT_406
    )
}

/// Best-effort detection of whether a Tavily MCP response contains *any* error.
///
/// This is used by downstream billing code to avoid over-charging when a JSON-RPC batch
/// contains partial failures (e.g. some items succeed but others error/quota-exhaust).
///
/// Conservative behavior: if we cannot confidently parse the response, treat it as "has error"
/// so we never apply the "expected credits" billing fallback on ambiguous payloads.
pub fn mcp_response_has_any_error(body: &[u8]) -> bool {
    let text = match std::str::from_utf8(body) {
        Ok(text) => text,
        Err(_) => return true,
    };

    let mut messages = extract_sse_json_messages(text);
    if messages.is_empty()
        && let Ok(value) = serde_json::from_str::<Value>(text)
    {
        match value {
            Value::Array(items) => messages.extend(items),
            other => messages.push(other),
        }
    }

    if messages.is_empty() {
        return true;
    }

    for message in messages {
        let Some((outcome, _code)) = analyze_json_message(&message) else {
            return true;
        };
        if outcome != MessageOutcome::Success {
            return true;
        }
    }

    false
}

/// Best-effort detection of whether a Tavily MCP response contains at least one successful item.
pub fn mcp_response_has_any_success(body: &[u8]) -> bool {
    let text = match std::str::from_utf8(body) {
        Ok(text) => text,
        Err(_) => return false,
    };

    let mut messages = extract_sse_json_messages(text);
    if messages.is_empty()
        && let Ok(value) = serde_json::from_str::<Value>(text)
    {
        match value {
            Value::Array(items) => messages.extend(items),
            other => messages.push(other),
        }
    }

    if messages.is_empty() {
        return false;
    }

    for message in messages {
        if let Some((outcome, _code)) = analyze_json_message(&message)
            && outcome == MessageOutcome::Success
        {
            return true;
        }
    }

    false
}

pub(crate) fn sanitize_headers_inner(
    headers: &HeaderMap,
    upstream: &Url,
    upstream_origin: &str,
) -> SanitizedHeaders {
    let mut sanitized = HeaderMap::new();
    let mut forwarded = Vec::new();
    let mut dropped = Vec::new();
    for (name, value) in headers.iter() {
        let key = name.as_str().to_ascii_lowercase();
        if !should_forward_header(name) {
            dropped.push(key);
            continue;
        }
        if let Some(transformed) = transform_header_value(name, value, upstream, upstream_origin) {
            sanitized.insert(name.clone(), transformed);
            forwarded.push(key);
        } else {
            dropped.push(key);
        }
    }
    SanitizedHeaders {
        headers: sanitized,
        forwarded,
        dropped,
    }
}

pub(crate) fn sanitize_mcp_headers_inner(headers: &HeaderMap) -> SanitizedHeaders {
    const MCP_ALLOWED_HEADERS: &[&str] = &[
        "accept",
        "accept-encoding",
        "cache-control",
        "content-type",
        "last-event-id",
        "mcp-protocol-version",
        "mcp-session-id",
        "pragma",
    ];

    let mut sanitized = HeaderMap::new();
    let mut forwarded = Vec::new();
    let mut dropped = Vec::new();

    for (name, value) in headers.iter() {
        let key = name.as_str().to_ascii_lowercase();
        let allowed = MCP_ALLOWED_HEADERS.iter().any(|allowed| key == *allowed)
            || ALLOWED_PREFIXES
                .iter()
                .any(|prefix| key.starts_with(prefix));

        if !allowed {
            dropped.push(key);
            continue;
        }

        sanitized.insert(name.clone(), value.clone());
        forwarded.push(key);
    }

    sanitized.insert(
        reqwest::header::USER_AGENT,
        HeaderValue::from_static(MCP_PROXY_USER_AGENT),
    );
    if !forwarded.iter().any(|name| name == "user-agent") {
        forwarded.push("user-agent".to_string());
    }

    SanitizedHeaders {
        headers: sanitized,
        forwarded,
        dropped,
    }
}

pub(crate) fn should_forward_header(name: &reqwest::header::HeaderName) -> bool {
    let lower = name.as_str().to_ascii_lowercase();
    if BLOCKED_HEADERS.iter().any(|blocked| lower == *blocked) {
        return false;
    }
    if ALLOWED_HEADERS.iter().any(|allowed| lower == *allowed) {
        return true;
    }
    if ALLOWED_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(prefix))
    {
        return true;
    }
    if lower.starts_with("x-") && !lower.starts_with("x-forwarded-") && lower != "x-real-ip" {
        return true;
    }
    false
}

pub(crate) fn transform_header_value(
    name: &reqwest::header::HeaderName,
    value: &HeaderValue,
    upstream: &Url,
    upstream_origin: &str,
) -> Option<HeaderValue> {
    let lower = name.as_str().to_ascii_lowercase();
    match lower.as_str() {
        "origin" => HeaderValue::from_str(upstream_origin).ok(),
        "referer" => match value.to_str() {
            Ok(raw) => {
                if let Ok(mut url) = Url::parse(raw) {
                    url.set_scheme(upstream.scheme()).ok()?;
                    url.set_host(upstream.host_str()).ok()?;
                    if let Some(port) = upstream.port() {
                        url.set_port(Some(port)).ok()?;
                    } else {
                        url.set_port(None).ok()?;
                    }
                    if url.path().is_empty() {
                        url.set_path("/");
                    }
                    HeaderValue::from_str(url.as_str()).ok()
                } else {
                    HeaderValue::from_str(upstream_origin).ok()
                }
            }
            Err(_) => HeaderValue::from_str(upstream_origin).ok(),
        },
        "sec-fetch-site" => Some(HeaderValue::from_static("same-origin")),
        _ => Some(value.clone()),
    }
}

pub(crate) fn origin_from_url(url: &Url) -> String {
    let mut origin = match url.host_str() {
        Some(host) => format!("{}://{}", url.scheme(), host),
        None => url.as_str().to_string(),
    };

    match (url.port(), url.port_or_known_default()) {
        (Some(port), Some(default)) if default != port => {
            origin.push(':');
            origin.push_str(&port.to_string());
        }
        (Some(port), None) => {
            origin.push(':');
            origin.push_str(&port.to_string());
        }
        _ => {}
    }

    origin
}

pub(crate) fn parse_header_list(raw: Option<String>) -> Vec<String> {
    raw.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub(crate) fn analyze_json_message(value: &Value) -> Option<(MessageOutcome, Option<i64>)> {
    if value.get("error").is_some_and(|v| !v.is_null()) {
        return Some((MessageOutcome::Error, None));
    }

    if let Some(result) = value.get("result") {
        return analyze_result_payload(result);
    }

    None
}

pub(crate) fn analyze_result_payload(result: &Value) -> Option<(MessageOutcome, Option<i64>)> {
    if let Some(outcome) = analyze_structured_content(result) {
        return Some(outcome);
    }

    if let Some(content) = result.get("content").and_then(|v| v.as_array()) {
        for item in content {
            if let Some(kind) = item.get("type").and_then(|v| v.as_str())
                && kind.eq_ignore_ascii_case("error")
            {
                return Some((MessageOutcome::Error, None));
            }
            if let Some(text) = item.get("text").and_then(|v| v.as_str())
                && let Some(code) = parse_embedded_status(text)
            {
                return Some((classify_status_code(code), Some(code)));
            }
        }
    }

    if result.get("error").is_some_and(|v| !v.is_null()) {
        return Some((MessageOutcome::Error, None));
    }

    if result
        .get("isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Some((MessageOutcome::Error, None));
    }

    Some((MessageOutcome::Success, None))
}

pub(crate) fn analyze_structured_content(result: &Value) -> Option<(MessageOutcome, Option<i64>)> {
    let structured = result.get("structuredContent")?;

    if let Some(code) = extract_status_code(structured) {
        let code_outcome = classify_status_code(code);
        if matches!(code_outcome, MessageOutcome::Success)
            && let Some(text_outcome) =
                extract_status_text(structured).and_then(classify_status_text)
        {
            return Some((text_outcome, Some(code)));
        }
        return Some((code_outcome, Some(code)));
    }

    if let Some(text_outcome) = extract_status_text(structured).and_then(classify_status_text) {
        return Some((text_outcome, None));
    }

    if structured
        .get("isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Some((MessageOutcome::Error, None));
    }

    structured
        .get("content")
        .and_then(|v| v.as_array())
        .and_then(|items| {
            for item in items {
                if let Some(text) = item.get("text").and_then(|v| v.as_str())
                    && let Some(code) = parse_embedded_status(text)
                {
                    return Some((classify_status_code(code), Some(code)));
                }
            }
            None
        })
        .or(Some((MessageOutcome::Success, None)))
}

pub(crate) fn extract_status_code(value: &Value) -> Option<i64> {
    if let Some(code) = value.get("status").and_then(|v| v.as_i64()) {
        return Some(code);
    }

    if let Some(detail) = value.get("detail")
        && let Some(code) = detail.get("status").and_then(|v| v.as_i64())
    {
        return Some(code);
    }

    None
}

pub(crate) fn classify_quarantine_reason(
    status_code: Option<i64>,
    body: &[u8],
) -> Option<QuarantineDecision> {
    if let Some(code) = status_code
        && code != 401
        && code != 403
    {
        return None;
    }

    let raw = String::from_utf8_lossy(body);
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let normalized = trimmed.to_ascii_lowercase();
    let status_label = status_code
        .map(|code| format!("HTTP {code}"))
        .unwrap_or_else(|| "MCP error".to_string());
    let (reason_code, reason_summary) = if normalized.contains("deactivated") {
        (
            "account_deactivated",
            format!("Tavily account deactivated ({status_label})"),
        )
    } else if normalized.contains("revoked") {
        (
            "key_revoked",
            format!("Tavily key revoked ({status_label})"),
        )
    } else if normalized.contains("invalid api key")
        || normalized.contains("invalid_token")
        || normalized.contains("api key is invalid")
    {
        (
            "invalid_api_key",
            format!("Tavily rejected the API key as invalid ({status_label})"),
        )
    } else {
        return None;
    };

    Some(QuarantineDecision {
        reason_code: reason_code.to_string(),
        reason_summary,
        reason_detail: truncate_text(trimmed, QUARANTINE_REASON_DETAIL_MAX_LEN),
    })
}

pub(crate) fn truncate_text(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let mut truncated = input.chars().take(max_chars).collect::<String>();
    truncated.push('…');
    truncated
}

pub(crate) fn extract_status_text(value: &Value) -> Option<&str> {
    if let Some(status) = value.get("status").and_then(|v| v.as_str()) {
        return Some(status);
    }

    if let Some(detail) = value.get("detail")
        && let Some(status) = detail.get("status").and_then(|v| v.as_str())
    {
        return Some(status);
    }

    None
}

pub(crate) fn extract_research_request_id_from_path(path: &str) -> Option<String> {
    let encoded_request_id = path.strip_prefix("/research/")?;
    if encoded_request_id.is_empty() {
        return None;
    }
    urlencoding::decode(encoded_request_id)
        .map(|decoded| decoded.into_owned())
        .ok()
}

pub(crate) fn extract_research_request_id(body: &[u8]) -> Option<String> {
    let parsed = serde_json::from_slice::<Value>(body).ok()?;
    let request_id = parsed
        .get("request_id")
        .and_then(|v| v.as_str())
        .or_else(|| parsed.get("requestId").and_then(|v| v.as_str()))?;
    let trimmed = request_id.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_owned())
}

pub(crate) fn request_kind_key(protocol: &str, value: &str) -> String {
    format!("{protocol}:{value}")
}

pub(crate) fn request_kind_label(protocol: &str, value: &str) -> String {
    format!("{protocol} | {value}")
}

pub(crate) fn build_api_request_kind_named(key: &str, label: &str) -> TokenRequestKind {
    TokenRequestKind::new(
        request_kind_key("api", key),
        request_kind_label("API", label),
        None,
    )
}

pub(crate) fn build_api_request_kind(value: &str) -> TokenRequestKind {
    build_api_request_kind_named(value, value)
}

pub(crate) fn build_mcp_request_kind_named(key: &str, label: &str) -> TokenRequestKind {
    TokenRequestKind::new(
        request_kind_key("mcp", key),
        request_kind_label("MCP", label),
        None,
    )
}

pub(crate) fn build_mcp_request_kind(value: &str) -> TokenRequestKind {
    build_mcp_request_kind_named(value, value)
}

pub(crate) fn build_mcp_request_kind_with_detail(
    key: &str,
    label: &str,
    detail: Option<String>,
) -> TokenRequestKind {
    TokenRequestKind::new(
        request_kind_key("mcp", key),
        request_kind_label("MCP", label),
        detail,
    )
}

pub(crate) fn build_api_unknown_path_kind(path: &str) -> TokenRequestKind {
    TokenRequestKind::new(
        "api:unknown-path",
        "API | unknown path",
        Some(path.to_string()),
    )
}

pub(crate) fn build_mcp_unsupported_path_kind(path: &str) -> TokenRequestKind {
    TokenRequestKind::new(
        "mcp:unsupported-path",
        "MCP | unsupported path",
        Some(path.to_string()),
    )
}

pub(crate) fn build_mcp_unknown_payload_kind(detail: Option<String>) -> TokenRequestKind {
    TokenRequestKind::new("mcp:unknown-payload", "MCP | unknown payload", detail)
}

pub(crate) fn build_mcp_unknown_method_kind(method: &str) -> TokenRequestKind {
    TokenRequestKind::new(
        "mcp:unknown-method",
        "MCP | unknown method",
        Some(method.to_string()),
    )
}

pub(crate) fn build_mcp_third_party_tool_kind(tool: &str) -> TokenRequestKind {
    TokenRequestKind::new(
        "mcp:third-party-tool",
        "MCP | third-party tool",
        Some(tool.to_string()),
    )
}

pub(crate) fn normalize_tavily_tool_name(tool: &str) -> Option<String> {
    let normalized = tool.trim().to_ascii_lowercase().replace('_', "-");
    let mapped = match normalized.as_str() {
        "tavily-search" => "search",
        "tavily-extract" => "extract",
        "tavily-crawl" => "crawl",
        "tavily-map" => "map",
        "tavily-research" => "research",
        _ => return None,
    };
    Some(mapped.to_string())
}

pub(crate) fn request_kind_label_penalty(label: &str) -> (usize, usize, usize, String) {
    let display = label.split('|').nth(1).unwrap_or(label).trim();
    let underscore_count = display.chars().filter(|ch| *ch == '_').count();
    let dash_count = display.chars().filter(|ch| *ch == '-').count();
    let lowercase_only = usize::from(
        display.chars().any(|ch| ch.is_ascii_alphabetic())
            && !display.chars().any(|ch| ch.is_ascii_uppercase()),
    );
    (
        underscore_count,
        dash_count,
        lowercase_only,
        display.to_string(),
    )
}

pub(crate) fn prefer_request_kind_label(current: &str, candidate: &str) -> bool {
    request_kind_label_penalty(candidate) < request_kind_label_penalty(current)
}

pub(crate) fn canonical_request_kind_label(key: &str) -> Option<&'static str> {
    match key.trim() {
        "api:search" => Some("API | search"),
        "api:extract" => Some("API | extract"),
        "api:crawl" => Some("API | crawl"),
        "api:map" => Some("API | map"),
        "api:research" => Some("API | research"),
        "api:research-result" => Some("API | research result"),
        "api:usage" => Some("API | usage"),
        "api:unknown-path" => Some("API | unknown path"),
        "mcp:search" => Some("MCP | search"),
        "mcp:extract" => Some("MCP | extract"),
        "mcp:crawl" => Some("MCP | crawl"),
        "mcp:map" => Some("MCP | map"),
        "mcp:research" => Some("MCP | research"),
        "mcp:batch" => Some("MCP | batch"),
        "mcp:initialize" => Some("MCP | initialize"),
        "mcp:ping" => Some("MCP | ping"),
        "mcp:tools/list" => Some("MCP | tools/list"),
        "mcp:unsupported-path" => Some("MCP | unsupported path"),
        "mcp:unknown-payload" => Some("MCP | unknown payload"),
        "mcp:unknown-method" => Some("MCP | unknown method"),
        "mcp:third-party-tool" => Some("MCP | third-party tool"),
        key if key.starts_with("mcp:resources/") => None,
        key if key.starts_with("mcp:prompts/") => None,
        key if key.starts_with("mcp:notifications/") => None,
        _ => None,
    }
}

pub fn is_canonical_request_kind_key(key: &str) -> bool {
    let normalized = key.trim();
    matches!(
        normalized,
        "api:search"
            | "api:extract"
            | "api:crawl"
            | "api:map"
            | "api:research"
            | "api:research-result"
            | "api:usage"
            | "api:unknown-path"
            | "mcp:search"
            | "mcp:extract"
            | "mcp:crawl"
            | "mcp:map"
            | "mcp:research"
            | "mcp:batch"
            | "mcp:initialize"
            | "mcp:ping"
            | "mcp:tools/list"
            | "mcp:unsupported-path"
            | "mcp:unknown-payload"
            | "mcp:unknown-method"
            | "mcp:third-party-tool"
    ) || normalized.starts_with("mcp:resources/")
        || normalized.starts_with("mcp:prompts/")
        || normalized.starts_with("mcp:notifications/")
}

pub(crate) fn token_request_kind_from_canonical_key(
    key: &str,
    detail: Option<String>,
) -> Option<TokenRequestKind> {
    let normalized = key.trim();
    let label = canonical_request_kind_label(normalized)
        .map(str::to_string)
        .or_else(|| {
            normalized
                .strip_prefix("mcp:")
                .filter(|value| {
                    value.starts_with("resources/")
                        || value.starts_with("prompts/")
                        || value.starts_with("notifications/")
                })
                .map(|value| format!("MCP | {value}"))
        })?;
    Some(TokenRequestKind::new(normalized, label, detail))
}

fn strip_request_kind_label_prefix(label: &str) -> Option<String> {
    let trimmed = label.trim();
    trimmed
        .split_once('|')
        .map(|(_, suffix)| suffix.trim().to_string())
        .filter(|suffix| !suffix.is_empty())
}

fn stored_third_party_tool_detail(
    key: Option<&str>,
    label: Option<&str>,
    detail: Option<&str>,
) -> Option<String> {
    detail
        .and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .or_else(|| {
            key.and_then(|value| {
                value
                    .trim()
                    .strip_prefix("mcp:tool:")
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
        })
        .or_else(|| {
            label.and_then(|value| {
                let stripped = strip_request_kind_label_prefix(value)?;
                (!stripped.eq_ignore_ascii_case("third-party tool")).then_some(stripped)
            })
        })
}

fn stored_unknown_method_detail(
    key: Option<&str>,
    label: Option<&str>,
    detail: Option<&str>,
) -> Option<String> {
    detail
        .and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .or_else(|| {
            label.and_then(|value| {
                let stripped = strip_request_kind_label_prefix(value)?;
                if stripped.eq_ignore_ascii_case("unknown method")
                    || stripped.eq_ignore_ascii_case("unknown payload")
                {
                    None
                } else {
                    Some(stripped)
                }
            })
        })
        .or_else(|| {
            key.and_then(|value| {
                let trimmed = value.trim();
                if is_canonical_request_kind_key(trimmed)
                    || trimmed.starts_with("mcp:tool:")
                    || trimmed.starts_with("mcp:raw:")
                {
                    None
                } else {
                    trimmed
                        .strip_prefix("mcp:")
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                }
            })
        })
}

pub(crate) fn classify_mcp_request_kind_from_message(value: &Value) -> Option<TokenRequestKind> {
    let method = value
        .get("method")
        .and_then(|raw| raw.as_str())
        .map(str::trim)
        .filter(|raw| !raw.is_empty())?;

    if matches!(method, "initialize" | "ping" | "tools/list")
        || method.starts_with("resources/")
        || method.starts_with("prompts/")
        || method.starts_with("notifications/")
    {
        return Some(build_mcp_request_kind(method));
    }

    if method == "tools/call" {
        let tool = value
            .get("params")
            .and_then(|params| params.get("name"))
            .and_then(|raw| raw.as_str())
            .map(str::trim)
            .filter(|raw| !raw.is_empty());
        return match tool {
            Some(tool) => match normalize_tavily_tool_name(tool) {
                Some(kind) => Some(build_mcp_request_kind(&kind)),
                None => Some(build_mcp_third_party_tool_kind(tool)),
            },
            None => Some(build_mcp_unknown_payload_kind(Some(
                "tools/call".to_string(),
            ))),
        };
    }

    Some(build_mcp_unknown_method_kind(method))
}

pub(crate) fn classify_mcp_request_kind(path: &str, body: Option<&[u8]>) -> TokenRequestKind {
    if path != "/mcp" {
        return build_mcp_unsupported_path_kind(path);
    }
    let Some(body) = body else {
        return build_mcp_unknown_payload_kind(Some(path.to_string()));
    };
    if body.is_empty() {
        return build_mcp_unknown_payload_kind(Some(path.to_string()));
    }

    let parsed = match serde_json::from_slice::<Value>(body) {
        Ok(value) => value,
        Err(_) => return build_mcp_unknown_payload_kind(Some(path.to_string())),
    };

    match parsed {
        Value::Array(items) => {
            let mut kinds: Vec<TokenRequestKind> = items
                .iter()
                .filter_map(classify_mcp_request_kind_from_message)
                .collect();
            if kinds.is_empty() {
                return build_mcp_unknown_payload_kind(Some(path.to_string()));
            }
            let first_key = kinds[0].key.clone();
            if kinds.iter().all(|kind| kind.key == first_key) {
                return kinds.remove(0);
            }
            let mut labels: Vec<String> = Vec::new();
            for kind in kinds {
                if let Some(label) = kind.label.strip_prefix("MCP | ")
                    && !labels.iter().any(|item| item == label)
                {
                    labels.push(label.to_string());
                }
            }
            build_mcp_request_kind_with_detail(
                "batch",
                "batch",
                (!labels.is_empty()).then(|| labels.join(", ")),
            )
        }
        Value::Object(_) => classify_mcp_request_kind_from_message(&parsed)
            .unwrap_or_else(|| build_mcp_unknown_payload_kind(Some(path.to_string()))),
        _ => build_mcp_unknown_payload_kind(Some(path.to_string())),
    }
}

pub fn classify_token_request_kind(path: &str, body: Option<&[u8]>) -> TokenRequestKind {
    match path {
        "/api/tavily/search" => build_api_request_kind("search"),
        "/api/tavily/extract" => build_api_request_kind("extract"),
        "/api/tavily/crawl" => build_api_request_kind("crawl"),
        "/api/tavily/map" => build_api_request_kind("map"),
        "/api/tavily/research" => build_api_request_kind("research"),
        "/api/tavily/usage" => build_api_request_kind("usage"),
        _ if path.starts_with("/api/tavily/research/") => {
            build_api_request_kind_named("research-result", "research result")
        }
        _ if path.starts_with("/mcp") => classify_mcp_request_kind(path, body),
        _ => build_api_unknown_path_kind(path),
    }
}

pub fn canonical_request_kind_key_for_filter(request_kind: &str) -> String {
    let trimmed = request_kind.trim();
    if is_canonical_request_kind_key(trimmed) {
        return trimmed.to_string();
    }
    if trimmed.starts_with("api:raw:") {
        return "api:unknown-path".to_string();
    }
    if trimmed.starts_with("api:") {
        return "api:unknown-path".to_string();
    }
    if trimmed.starts_with("mcp:tool:") {
        return "mcp:third-party-tool".to_string();
    }
    if trimmed == "mcp:tools/call" {
        return "mcp:unknown-payload".to_string();
    }
    if let Some(path) = trimmed.strip_prefix("mcp:raw:") {
        if path == "/mcp" {
            return "mcp:unknown-payload".to_string();
        }
        if path.starts_with("/mcp/") {
            return "mcp:unsupported-path".to_string();
        }
    }
    if trimmed.starts_with("mcp:") {
        return "mcp:unknown-method".to_string();
    }
    trimmed.to_string()
}

pub(crate) fn canonical_request_kind_stored_predicate_sql(expr: &str) -> String {
    let value = format!("COALESCE({expr}, '')");
    format!(
        "({value} IN ('api:search', 'api:extract', 'api:crawl', 'api:map', 'api:research', 'api:research-result', 'api:usage', 'api:unknown-path', 'mcp:search', 'mcp:extract', 'mcp:crawl', 'mcp:map', 'mcp:research', 'mcp:batch', 'mcp:initialize', 'mcp:ping', 'mcp:tools/list', 'mcp:unsupported-path', 'mcp:unknown-payload', 'mcp:unknown-method', 'mcp:third-party-tool') OR {value} LIKE 'mcp:resources/%' OR {value} LIKE 'mcp:prompts/%' OR {value} LIKE 'mcp:notifications/%')"
    )
}

pub(crate) fn legacy_request_kind_stored_predicate_sql(expr: &str) -> String {
    let value = format!("COALESCE({expr}, '')");
    let canonical = canonical_request_kind_stored_predicate_sql(expr);
    format!(
        "({value} = '' OR {value} LIKE 'api:raw:%' OR {value} LIKE 'mcp:tool:%' OR {value} = 'mcp:tools/call' OR {value} LIKE 'mcp:raw:%' OR ({value} LIKE 'api:%' AND NOT {canonical}) OR ({value} LIKE 'mcp:%' AND NOT {canonical}))"
    )
}

pub(crate) fn canonical_request_kind_label_sql(kind_expr: &str) -> String {
    let normalized = format!("LOWER(TRIM(COALESCE({kind_expr}, '')))");
    format!(
        "
        CASE
            WHEN {normalized} = 'api:search' THEN 'API | search'
            WHEN {normalized} = 'api:extract' THEN 'API | extract'
            WHEN {normalized} = 'api:crawl' THEN 'API | crawl'
            WHEN {normalized} = 'api:map' THEN 'API | map'
            WHEN {normalized} = 'api:research' THEN 'API | research'
            WHEN {normalized} = 'api:research-result' THEN 'API | research result'
            WHEN {normalized} = 'api:usage' THEN 'API | usage'
            WHEN {normalized} = 'api:unknown-path' THEN 'API | unknown path'
            WHEN {normalized} = 'mcp:search' THEN 'MCP | search'
            WHEN {normalized} = 'mcp:extract' THEN 'MCP | extract'
            WHEN {normalized} = 'mcp:crawl' THEN 'MCP | crawl'
            WHEN {normalized} = 'mcp:map' THEN 'MCP | map'
            WHEN {normalized} = 'mcp:research' THEN 'MCP | research'
            WHEN {normalized} = 'mcp:batch' THEN 'MCP | batch'
            WHEN {normalized} = 'mcp:initialize' THEN 'MCP | initialize'
            WHEN {normalized} = 'mcp:ping' THEN 'MCP | ping'
            WHEN {normalized} = 'mcp:tools/list' THEN 'MCP | tools/list'
            WHEN {normalized} = 'mcp:unsupported-path' THEN 'MCP | unsupported path'
            WHEN {normalized} = 'mcp:unknown-payload' THEN 'MCP | unknown payload'
            WHEN {normalized} = 'mcp:unknown-method' THEN 'MCP | unknown method'
            WHEN {normalized} = 'mcp:third-party-tool' THEN 'MCP | third-party tool'
            WHEN {normalized} LIKE 'mcp:resources/%'
                OR {normalized} LIKE 'mcp:prompts/%'
                OR {normalized} LIKE 'mcp:notifications/%'
                THEN 'MCP | ' || SUBSTR({normalized}, 5)
            ELSE TRIM(COALESCE({kind_expr}, ''))
        END
        "
    )
}

fn token_log_stored_kind_sql(path_expr: &str, key_expr: &str) -> String {
    let path = format!("LOWER(COALESCE({path_expr}, ''))");
    let key = format!("LOWER(TRIM(COALESCE({key_expr}, '')))");
    format!(
        "
        CASE
            WHEN {path} = '/api/tavily/search' THEN 'api:search'
            WHEN {path} = '/api/tavily/extract' THEN 'api:extract'
            WHEN {path} = '/api/tavily/crawl' THEN 'api:crawl'
            WHEN {path} = '/api/tavily/map' THEN 'api:map'
            WHEN {path} = '/api/tavily/research' THEN 'api:research'
            WHEN {path} = '/api/tavily/usage' THEN 'api:usage'
            WHEN {path} LIKE '/api/tavily/research/%' THEN 'api:research-result'
            WHEN {path} LIKE '/mcp/%' THEN 'mcp:unsupported-path'
            WHEN {key} IN (
                'api:search',
                'api:extract',
                'api:crawl',
                'api:map',
                'api:research',
                'api:research-result',
                'api:usage',
                'api:unknown-path',
                'mcp:search',
                'mcp:extract',
                'mcp:crawl',
                'mcp:map',
                'mcp:research',
                'mcp:batch',
                'mcp:initialize',
                'mcp:ping',
                'mcp:tools/list',
                'mcp:unsupported-path',
                'mcp:unknown-payload',
                'mcp:unknown-method',
                'mcp:third-party-tool'
            ) OR {key} LIKE 'mcp:resources/%'
              OR {key} LIKE 'mcp:prompts/%'
              OR {key} LIKE 'mcp:notifications/%'
                THEN {key}
            WHEN {key} LIKE 'api:raw:%' OR ({path} NOT LIKE '/mcp%' AND {path} NOT LIKE '/api/tavily/%')
                THEN 'api:unknown-path'
            WHEN {key} LIKE 'mcp:tool:%' THEN 'mcp:third-party-tool'
            WHEN {key} = 'mcp:tools/call' OR {key} LIKE 'mcp:raw:%' THEN 'mcp:unknown-payload'
            WHEN {key} LIKE 'mcp:%' AND {path} = '/mcp' THEN 'mcp:unknown-method'
            WHEN {path} = '/mcp' THEN 'mcp:unknown-payload'
            ELSE 'api:unknown-path'
        END
        "
    )
}

fn mcp_message_request_kind_sql(value_expr: &str) -> String {
    let method = request_body_json_text_sql(value_expr, "$.method");
    let tool_name = request_body_json_text_sql(value_expr, "$.params.name");
    format!(
        "
        CASE
            WHEN {method} IN ('initialize', 'ping', 'tools/list') THEN 'mcp:' || {method}
            WHEN {method} LIKE 'resources/%'
                OR {method} LIKE 'prompts/%'
                OR {method} LIKE 'notifications/%'
                THEN 'mcp:' || {method}
            WHEN {method} = 'tools/call' AND {tool_name} IN (
                'tavily-search',
                'tavily_search',
                'tavily_extract',
                'tavily-extract',
                'tavily-crawl',
                'tavily_crawl',
                'tavily_map',
                'tavily-map',
                'tavily-research',
                'tavily_research'
            ) THEN
                CASE REPLACE({tool_name}, '_', '-')
                    WHEN 'tavily-search' THEN 'mcp:search'
                    WHEN 'tavily-extract' THEN 'mcp:extract'
                    WHEN 'tavily-crawl' THEN 'mcp:crawl'
                    WHEN 'tavily-map' THEN 'mcp:map'
                    WHEN 'tavily-research' THEN 'mcp:research'
                    ELSE 'mcp:unknown-payload'
                END
            WHEN {method} = 'tools/call' AND {tool_name} <> '' THEN 'mcp:third-party-tool'
            WHEN {method} = 'tools/call' THEN 'mcp:unknown-payload'
            WHEN {method} <> '' THEN 'mcp:unknown-method'
            ELSE NULL
        END
        "
    )
}

pub(crate) fn token_log_request_kind_key_sql(path_expr: &str, key_expr: &str) -> String {
    token_log_stored_kind_sql(path_expr, key_expr)
}

pub(crate) fn request_log_request_kind_key_sql(
    path_expr: &str,
    body_expr: &str,
    key_expr: &str,
) -> String {
    let path = format!("LOWER(COALESCE({path_expr}, ''))");
    let body_json = format!("CAST({body_expr} AS TEXT)");
    let object_kind = mcp_message_request_kind_sql(body_expr);
    let array_item_kind = mcp_message_request_kind_sql("items.value");
    let token_fallback = token_log_stored_kind_sql(path_expr, key_expr);
    format!(
        "
        CASE
            WHEN {path} = '/mcp' AND json_valid({body_json}) AND json_type({body_json}) = 'object'
                THEN COALESCE(({object_kind}), 'mcp:unknown-payload')
            WHEN {path} = '/mcp' AND json_valid({body_json}) AND json_type({body_json}) = 'array'
                THEN CASE
                    WHEN NOT EXISTS (SELECT 1 FROM json_each({body_json}) AS items)
                        THEN 'mcp:unknown-payload'
                    WHEN EXISTS (
                        SELECT 1 FROM json_each({body_json}) AS items
                        WHERE ({array_item_kind}) IS NULL
                    ) THEN 'mcp:unknown-payload'
                    WHEN (
                        SELECT COUNT(DISTINCT ({array_item_kind}))
                        FROM json_each({body_json}) AS items
                    ) = 1 THEN (
                        SELECT MIN(({array_item_kind}))
                        FROM json_each({body_json}) AS items
                    )
                    ELSE 'mcp:batch'
                END
            ELSE {token_fallback}
        END
        "
    )
}

pub fn token_request_kind_protocol_group(key: &str) -> &'static str {
    if key.trim().starts_with("mcp:") {
        "mcp"
    } else {
        "api"
    }
}

pub fn token_request_kind_billing_group(key: &str) -> &'static str {
    let normalized = key.trim();
    if normalized == "api:research-result"
        || normalized == "api:usage"
        || normalized == "api:unknown-path"
        || normalized.starts_with("mcp:initialize")
        || normalized.starts_with("mcp:ping")
        || normalized.starts_with("mcp:tools/list")
        || normalized == "mcp:unsupported-path"
        || normalized == "mcp:unknown-payload"
        || normalized == "mcp:unknown-method"
        || normalized == "mcp:third-party-tool"
        || normalized.starts_with("mcp:resources/")
        || normalized.starts_with("mcp:prompts/")
        || normalized.starts_with("mcp:notifications/")
    {
        "non_billable"
    } else {
        "billable"
    }
}

pub(crate) fn token_request_kind_option_billing_group(
    key: &str,
    has_billable: bool,
    has_non_billable: bool,
) -> &'static str {
    let normalized = key.trim();
    if normalized == "mcp:batch" && !has_billable && has_non_billable {
        "non_billable"
    } else {
        token_request_kind_billing_group(normalized)
    }
}

pub fn token_request_kind_billing_group_for_token_log(
    request_kind_key: &str,
    counts_business_quota: bool,
) -> &'static str {
    let normalized = request_kind_key.trim();
    if !counts_business_quota && normalized == "mcp:batch" {
        "non_billable"
    } else {
        token_request_kind_billing_group(normalized)
    }
}

pub fn token_request_kind_billing_group_for_request(
    path: &str,
    body: Option<&[u8]>,
) -> &'static str {
    let request_kind = classify_token_request_kind(path, body);
    token_request_kind_billing_group_for_request_log(&request_kind.key, body)
}

pub(crate) const OPERATIONAL_CLASS_SUCCESS: &str = "success";
pub(crate) const OPERATIONAL_CLASS_NEUTRAL: &str = "neutral";
pub(crate) const OPERATIONAL_CLASS_CLIENT_ERROR: &str = "client_error";
pub(crate) const OPERATIONAL_CLASS_UPSTREAM_ERROR: &str = "upstream_error";
pub(crate) const OPERATIONAL_CLASS_SYSTEM_ERROR: &str = "system_error";
pub(crate) const OPERATIONAL_CLASS_QUOTA_EXHAUSTED: &str = "quota_exhausted";

pub fn normalize_operational_class_filter(value: Option<&str>) -> Option<&'static str> {
    match value.map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case(OPERATIONAL_CLASS_SUCCESS) => {
            Some(OPERATIONAL_CLASS_SUCCESS)
        }
        Some(value) if value.eq_ignore_ascii_case(OPERATIONAL_CLASS_NEUTRAL) => {
            Some(OPERATIONAL_CLASS_NEUTRAL)
        }
        Some(value) if value.eq_ignore_ascii_case(OPERATIONAL_CLASS_CLIENT_ERROR) => {
            Some(OPERATIONAL_CLASS_CLIENT_ERROR)
        }
        Some(value) if value.eq_ignore_ascii_case(OPERATIONAL_CLASS_UPSTREAM_ERROR) => {
            Some(OPERATIONAL_CLASS_UPSTREAM_ERROR)
        }
        Some(value) if value.eq_ignore_ascii_case(OPERATIONAL_CLASS_SYSTEM_ERROR) => {
            Some(OPERATIONAL_CLASS_SYSTEM_ERROR)
        }
        Some(value) if value.eq_ignore_ascii_case(OPERATIONAL_CLASS_QUOTA_EXHAUSTED) => {
            Some(OPERATIONAL_CLASS_QUOTA_EXHAUSTED)
        }
        _ => None,
    }
}

fn is_client_error_failure_kind(kind: &str) -> bool {
    matches!(
        kind.trim(),
        FAILURE_KIND_MCP_ACCEPT_406
            | FAILURE_KIND_TOOL_ARGUMENT_VALIDATION
            | FAILURE_KIND_UNKNOWN_TOOL_NAME
            | FAILURE_KIND_INVALID_SEARCH_DEPTH
            | FAILURE_KIND_INVALID_COUNTRY_SEARCH_DEPTH_COMBO
            | FAILURE_KIND_RESEARCH_PAYLOAD_422
            | FAILURE_KIND_QUERY_TOO_LONG
            | FAILURE_KIND_MCP_METHOD_405
            | FAILURE_KIND_MCP_PATH_404
    )
}

fn is_upstream_error_failure_kind(kind: &str) -> bool {
    matches!(
        kind.trim(),
        FAILURE_KIND_UPSTREAM_RATE_LIMITED_429
            | FAILURE_KIND_UPSTREAM_GATEWAY_5XX
            | FAILURE_KIND_UPSTREAM_ACCOUNT_DEACTIVATED_401
    )
}

pub fn operational_class_for_request_kind(
    request_kind_key: &str,
    result_status: &str,
    failure_kind: Option<&str>,
) -> &'static str {
    operational_class_for_token_log(request_kind_key, result_status, failure_kind, true)
}

pub fn operational_class_for_token_log(
    request_kind_key: &str,
    result_status: &str,
    failure_kind: Option<&str>,
    counts_business_quota: bool,
) -> &'static str {
    let normalized_result = result_status.trim().to_ascii_lowercase();
    if normalized_result == OUTCOME_QUOTA_EXHAUSTED {
        return OPERATIONAL_CLASS_QUOTA_EXHAUSTED;
    }

    if normalized_result == OUTCOME_ERROR {
        if let Some(kind) = failure_kind {
            if is_client_error_failure_kind(kind) {
                return OPERATIONAL_CLASS_CLIENT_ERROR;
            }
            if is_upstream_error_failure_kind(kind) {
                return OPERATIONAL_CLASS_UPSTREAM_ERROR;
            }
        }
        return OPERATIONAL_CLASS_SYSTEM_ERROR;
    }

    if token_request_kind_protocol_group(request_kind_key) == "mcp"
        && token_request_kind_billing_group_for_token_log(request_kind_key, counts_business_quota)
            == "non_billable"
    {
        return OPERATIONAL_CLASS_NEUTRAL;
    }

    if normalized_result == OUTCOME_SUCCESS {
        OPERATIONAL_CLASS_SUCCESS
    } else {
        OPERATIONAL_CLASS_SYSTEM_ERROR
    }
}

pub fn operational_class_for_request_path(
    path: &str,
    body: Option<&[u8]>,
    result_status: &str,
    failure_kind: Option<&str>,
) -> &'static str {
    let request_kind = classify_token_request_kind(path, body);
    let counts_business_quota = request_log_counts_business_quota(&request_kind.key, body);
    operational_class_for_token_log(
        &request_kind.key,
        result_status,
        failure_kind,
        counts_business_quota,
    )
}

fn request_log_counts_business_quota(request_kind_key: &str, body: Option<&[u8]>) -> bool {
    let normalized = request_kind_key.trim();
    normalized != "mcp:batch" || !mcp_request_body_all_non_billable(body)
}

pub fn token_request_kind_billing_group_for_request_log(
    request_kind_key: &str,
    body: Option<&[u8]>,
) -> &'static str {
    if !request_log_counts_business_quota(request_kind_key, body)
        && request_kind_key.trim() == "mcp:batch"
    {
        "non_billable"
    } else {
        token_request_kind_billing_group(request_kind_key)
    }
}

pub fn operational_class_for_request_log(
    request_kind_key: &str,
    body: Option<&[u8]>,
    result_status: &str,
    failure_kind: Option<&str>,
) -> &'static str {
    operational_class_for_token_log(
        request_kind_key,
        result_status,
        failure_kind,
        request_log_counts_business_quota(request_kind_key, body),
    )
}

fn token_request_kind_non_billable_mcp_sql(expr: &str) -> String {
    let normalized = format!("LOWER(TRIM(COALESCE({expr}, '')))");
    format!(
        "({normalized} IN ('mcp:initialize', 'mcp:ping', 'mcp:tools/list', 'mcp:unsupported-path', 'mcp:unknown-payload', 'mcp:unknown-method', 'mcp:third-party-tool') OR {normalized} LIKE 'mcp:resources/%' OR {normalized} LIKE 'mcp:prompts/%' OR {normalized} LIKE 'mcp:notifications/%')"
    )
}

fn request_body_json_text_sql(expr: &str, path: &str) -> String {
    format!("LOWER(COALESCE(NULLIF(json_extract(CAST({expr} AS TEXT), '{path}'), ''), ''))")
}

fn mcp_message_non_billable_kind_sql(value_expr: &str) -> String {
    let method = request_body_json_text_sql(value_expr, "$.method");
    let tool_name = request_body_json_text_sql(value_expr, "$.params.name");
    format!(
        "
        CASE
            WHEN {method} IN ('initialize', 'ping', 'tools/list') THEN 'mcp:' || {method}
            WHEN {method} LIKE 'resources/%' OR {method} LIKE 'prompts/%' OR {method} LIKE 'notifications/%'
                THEN 'mcp:' || {method}
            WHEN {method} = 'tools/call' AND {tool_name} IN (
                    'tavily-search',
                    'tavily_search',
                    'tavily_extract',
                    'tavily-extract',
                    'tavily-crawl',
                    'tavily_crawl',
                    'tavily_map',
                    'tavily-map',
                    'tavily-research',
                    'tavily_research'
                )
                THEN NULL
            WHEN {method} = 'tools/call' AND {tool_name} <> ''
                AND {tool_name} NOT IN (
                    'tavily-search',
                    'tavily_search',
                    'tavily_extract',
                    'tavily-extract',
                    'tavily-crawl',
                    'tavily_crawl',
                    'tavily_map',
                    'tavily-map',
                    'tavily-research',
                    'tavily_research'
                )
                THEN 'mcp:third-party-tool'
            WHEN {method} = 'tools/call' THEN 'mcp:unknown-payload'
            WHEN {method} <> '' THEN 'mcp:unknown-method'
            ELSE NULL
        END
        "
    )
}

fn mcp_request_body_all_non_billable_sql(body_expr: &str) -> String {
    let body_json = format!("CAST({body_expr} AS TEXT)");
    let array_item_kind = mcp_message_non_billable_kind_sql("items.value");
    format!(
        "
        (
            json_valid({body_json})
            AND json_type({body_json}) = 'array'
            AND EXISTS (SELECT 1 FROM json_each({body_json}) AS items)
            AND NOT EXISTS (
                SELECT 1
                FROM json_each({body_json}) AS items
                WHERE ({array_item_kind}) IS NULL
            )
        )
        "
    )
}

fn mcp_request_body_all_non_billable(body: Option<&[u8]>) -> bool {
    let Some(body) = body else {
        return false;
    };
    let Ok(Value::Array(items)) = serde_json::from_slice::<Value>(body) else {
        return false;
    };
    !items.is_empty()
        && items.iter().all(|item| {
            classify_mcp_request_kind_from_message(item).is_some_and(|kind| {
                token_request_kind_protocol_group(&kind.key) == "mcp"
                    && token_request_kind_billing_group(&kind.key) == "non_billable"
            })
        })
}

pub(crate) fn token_log_operational_class_case_sql(
    request_kind_expr: &str,
    counts_business_quota_expr: &str,
    result_status_expr: &str,
    failure_kind_expr: &str,
) -> String {
    let non_billable_mcp = token_request_kind_non_billable_mcp_sql(request_kind_expr);
    format!(
        "
        CASE
            WHEN {result_status_expr} = 'quota_exhausted' THEN '{quota_exhausted}'
            WHEN {result_status_expr} = 'error' AND {failure_kind_expr} IN (
                '{mcp_accept_406}',
                '{tool_argument_validation}',
                '{unknown_tool_name}',
                '{invalid_search_depth}',
                '{invalid_country_search_depth_combo}',
                '{research_payload_422}',
                '{query_too_long}',
                '{mcp_method_405}',
                '{mcp_path_404}'
            ) THEN '{client_error}'
            WHEN {result_status_expr} = 'error' AND {failure_kind_expr} IN (
                '{upstream_rate_limited_429}',
                '{upstream_gateway_5xx}',
                '{upstream_account_deactivated_401}'
            ) THEN '{upstream_error}'
            WHEN {result_status_expr} = 'error' THEN '{system_error}'
            WHEN {request_kind_expr} = 'mcp:batch' AND {counts_business_quota_expr} = 0
                THEN '{neutral}'
            WHEN {non_billable_mcp} THEN '{neutral}'
            WHEN {result_status_expr} = 'success' THEN '{success}'
            ELSE '{system_error}'
        END
        ",
        quota_exhausted = OPERATIONAL_CLASS_QUOTA_EXHAUSTED,
        client_error = OPERATIONAL_CLASS_CLIENT_ERROR,
        upstream_error = OPERATIONAL_CLASS_UPSTREAM_ERROR,
        system_error = OPERATIONAL_CLASS_SYSTEM_ERROR,
        neutral = OPERATIONAL_CLASS_NEUTRAL,
        success = OPERATIONAL_CLASS_SUCCESS,
        mcp_accept_406 = FAILURE_KIND_MCP_ACCEPT_406,
        tool_argument_validation = FAILURE_KIND_TOOL_ARGUMENT_VALIDATION,
        unknown_tool_name = FAILURE_KIND_UNKNOWN_TOOL_NAME,
        invalid_search_depth = FAILURE_KIND_INVALID_SEARCH_DEPTH,
        invalid_country_search_depth_combo = FAILURE_KIND_INVALID_COUNTRY_SEARCH_DEPTH_COMBO,
        research_payload_422 = FAILURE_KIND_RESEARCH_PAYLOAD_422,
        query_too_long = FAILURE_KIND_QUERY_TOO_LONG,
        mcp_method_405 = FAILURE_KIND_MCP_METHOD_405,
        mcp_path_404 = FAILURE_KIND_MCP_PATH_404,
        upstream_rate_limited_429 = FAILURE_KIND_UPSTREAM_RATE_LIMITED_429,
        upstream_gateway_5xx = FAILURE_KIND_UPSTREAM_GATEWAY_5XX,
        upstream_account_deactivated_401 = FAILURE_KIND_UPSTREAM_ACCOUNT_DEACTIVATED_401,
    )
}

pub(crate) fn request_log_operational_class_case_sql(
    request_kind_expr: &str,
    counts_business_quota_expr: &str,
    result_status_expr: &str,
    failure_kind_expr: &str,
) -> String {
    token_log_operational_class_case_sql(
        request_kind_expr,
        counts_business_quota_expr,
        result_status_expr,
        failure_kind_expr,
    )
}

pub(crate) fn request_log_counts_business_quota_sql(
    request_kind_expr: &str,
    body_expr: &str,
) -> String {
    let normalized = format!("LOWER(TRIM(COALESCE({request_kind_expr}, '')))");
    let batch_non_billable = mcp_request_body_all_non_billable_sql(body_expr);
    format!(
        "
        CASE
            WHEN {normalized} = 'mcp:batch' AND {batch_non_billable} THEN 0
            ELSE 1
        END
        "
    )
}

pub(crate) fn derive_token_request_kind_fallback(
    _method: &str,
    path: &str,
    _query: Option<&str>,
) -> TokenRequestKind {
    classify_token_request_kind(path, None)
}

fn normalize_request_kind_field(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

pub fn finalize_token_request_kind(
    method: &str,
    path: &str,
    query: Option<&str>,
    key: Option<String>,
    label: Option<String>,
    detail: Option<String>,
) -> TokenRequestKind {
    let key = normalize_request_kind_field(key);
    let label = normalize_request_kind_field(label);
    let detail = normalize_request_kind_field(detail);

    if let Some(stored_key) = key.as_deref() {
        if let Some(kind) = token_request_kind_from_canonical_key(stored_key, detail.clone()) {
            return kind;
        }

        if stored_key.starts_with("api:raw:") {
            return build_api_unknown_path_kind(path);
        }

        if path.starts_with("/mcp/") {
            return build_mcp_unsupported_path_kind(path);
        }

        if path == "/mcp" {
            if stored_key.starts_with("mcp:tool:") {
                return build_mcp_third_party_tool_kind(
                    stored_third_party_tool_detail(
                        Some(stored_key),
                        label.as_deref(),
                        detail.as_deref(),
                    )
                    .as_deref()
                    .unwrap_or("unknown"),
                );
            }

            if stored_key == "mcp:tools/call" || stored_key.starts_with("mcp:raw:") {
                return build_mcp_unknown_payload_kind(
                    detail.clone().or_else(|| Some(path.to_string())),
                );
            }

            if stored_key.starts_with("mcp:") {
                let method_detail = stored_unknown_method_detail(
                    Some(stored_key),
                    label.as_deref(),
                    detail.as_deref(),
                )
                .unwrap_or_else(|| "unknown".to_string());
                return TokenRequestKind::new(
                    "mcp:unknown-method",
                    "MCP | unknown method",
                    Some(method_detail),
                );
            }
        }
    }

    derive_token_request_kind_fallback(method, path, query)
}

pub fn canonicalize_request_log_request_kind(
    path: &str,
    body: Option<&[u8]>,
    key: Option<String>,
    label: Option<String>,
    detail: Option<String>,
) -> TokenRequestKind {
    if let Some(stored_key) = key.as_deref().map(str::trim)
        && is_canonical_request_kind_key(stored_key)
    {
        return token_request_kind_from_canonical_key(
            stored_key,
            normalize_request_kind_field(detail),
        )
        .unwrap_or_else(|| classify_token_request_kind(path, body));
    }

    let classified = classify_token_request_kind(path, body);
    if classified.key == "mcp:unknown-payload" && path == "/mcp" {
        finalize_token_request_kind("POST", path, None, key, label, detail)
    } else {
        classified
    }
}

/// Best-effort extraction of Tavily `usage.credits` from an upstream JSON response body.
///
/// - Returns `None` when the body isn't JSON or the field is missing.
/// - Handles nested MCP envelopes by recursively scanning for an object containing `{ "usage": { "credits": ... } }`.
/// - If credits is a float, rounds up to avoid under-charging.
pub fn extract_usage_credits_from_json_bytes(body: &[u8]) -> Option<i64> {
    if let Ok(parsed) = serde_json::from_slice::<Value>(body) {
        return extract_usage_credits_from_value(&parsed);
    }
    extract_usage_credits_from_sse_bytes(body)
}

/// Best-effort extraction of Tavily `usage.credits` from an upstream JSON response body,
/// summing across JSON-RPC batch responses (top-level arrays).
///
/// For non-batch responses, this matches `extract_usage_credits_from_json_bytes()`.
pub fn extract_usage_credits_total_from_json_bytes(body: &[u8]) -> Option<i64> {
    if let Ok(parsed) = serde_json::from_slice::<Value>(body) {
        return extract_usage_credits_total_from_value(&parsed);
    }
    extract_usage_credits_total_from_sse_bytes(body)
}

/// Best-effort extraction of `usage.credits` from an MCP response, keyed by JSON-RPC `id`.
///
/// This is primarily used by the `/mcp` proxy to avoid accidentally charging credits from
/// non-Tavily tool calls in a mixed JSON-RPC batch.
pub fn extract_mcp_usage_credits_by_id_from_bytes(body: &[u8]) -> HashMap<String, i64> {
    let mut messages: Vec<Value> = Vec::new();

    if let Ok(text) = std::str::from_utf8(body) {
        messages = extract_sse_json_messages(text);
        if messages.is_empty()
            && let Ok(value) = serde_json::from_str::<Value>(text)
        {
            match value {
                Value::Array(items) => messages.extend(items),
                other => messages.push(other),
            }
        }
    }

    if messages.is_empty()
        && let Ok(value) = serde_json::from_slice::<Value>(body)
    {
        match value {
            Value::Array(items) => messages.extend(items),
            other => messages.push(other),
        }
    }

    fn ingest(value: &Value, out: &mut HashMap<String, i64>) {
        match value {
            Value::Array(items) => {
                for item in items {
                    ingest(item, out);
                }
            }
            Value::Object(map) => {
                let Some(id) = map.get("id").filter(|v| !v.is_null()) else {
                    return;
                };
                let Some(credits) = extract_usage_credits_from_value(value) else {
                    return;
                };
                let key = id.to_string();
                out.entry(key)
                    .and_modify(|current| *current = (*current).max(credits))
                    .or_insert(credits);
            }
            _ => {}
        }
    }

    let mut out: HashMap<String, i64> = HashMap::new();
    for message in messages {
        ingest(&message, &mut out);
    }
    out
}

/// Best-effort extraction of whether an MCP response message contains an error, keyed by JSON-RPC `id`.
///
/// Values are `true` when we see any non-success outcome for that id (including quota exhausted).
/// This is used to scope billing fallbacks (like expected credits) to only the billable calls.
pub fn extract_mcp_has_error_by_id_from_bytes(body: &[u8]) -> HashMap<String, bool> {
    let mut messages: Vec<Value> = Vec::new();

    if let Ok(text) = std::str::from_utf8(body) {
        messages = extract_sse_json_messages(text);
        if messages.is_empty()
            && let Ok(value) = serde_json::from_str::<Value>(text)
        {
            match value {
                Value::Array(items) => messages.extend(items),
                other => messages.push(other),
            }
        }
    }

    if messages.is_empty()
        && let Ok(value) = serde_json::from_slice::<Value>(body)
    {
        match value {
            Value::Array(items) => messages.extend(items),
            other => messages.push(other),
        }
    }

    fn ingest(value: &Value, out: &mut HashMap<String, bool>) {
        match value {
            Value::Array(items) => {
                for item in items {
                    ingest(item, out);
                }
            }
            Value::Object(map) => {
                let Some(id) = map.get("id").filter(|v| !v.is_null()) else {
                    return;
                };

                let is_error = analyze_json_message(value)
                    .map(|(outcome, _code)| outcome != MessageOutcome::Success)
                    .unwrap_or(true);

                let key = id.to_string();
                out.entry(key)
                    .and_modify(|current| *current = *current || is_error)
                    .or_insert(is_error);
            }
            _ => {}
        }
    }

    let mut out: HashMap<String, bool> = HashMap::new();
    for message in messages {
        ingest(&message, &mut out);
    }
    out
}

pub(crate) fn extract_usage_credits_total_from_value(value: &Value) -> Option<i64> {
    match value {
        Value::Array(items) => {
            let mut total = 0i64;
            let mut found = false;
            for item in items {
                if let Some(credits) = extract_usage_credits_from_value(item) {
                    total = total.saturating_add(credits);
                    found = true;
                }
            }
            found.then_some(total)
        }
        other => extract_usage_credits_from_value(other),
    }
}

pub(crate) fn extract_usage_credits_from_value(value: &Value) -> Option<i64> {
    match value {
        Value::Object(map) => {
            if let Some(credits) = map
                .get("usage")
                .and_then(extract_usage_credits_from_usage_value)
            {
                return Some(credits);
            }
            // MCP responses can be wrapped in arbitrary envelopes. Scan all nested values.
            for nested in map.values() {
                if let Some(credits) = extract_usage_credits_from_value(nested) {
                    return Some(credits);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(extract_usage_credits_from_value),
        _ => None,
    }
}

pub(crate) fn extract_usage_credits_from_usage_value(value: &Value) -> Option<i64> {
    let Value::Object(map) = value else {
        return None;
    };

    for key in [
        "credits",
        // Some Tavily responses report fractional usage via an exact field instead of the
        // integer `credits` counter. We round up to avoid under-billing when only the exact
        // field is present.
        "total_credits_exact",
    ] {
        if let Some(credits) = map.get(key).and_then(parse_credits_value) {
            return Some(credits);
        }
    }

    None
}

pub(crate) fn parse_credits_value(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => {
            if let Some(v) = number.as_i64()
                && v >= 0
            {
                return Some(v);
            }
            number.as_f64().map(|v| v.ceil() as i64).filter(|v| *v >= 0)
        }
        Value::String(raw) => {
            let trimmed = raw.trim();
            if let Ok(v) = trimmed.parse::<i64>()
                && v >= 0
            {
                return Some(v);
            }
            trimmed
                .parse::<f64>()
                .ok()
                .map(|v| v.ceil() as i64)
                .filter(|v| *v >= 0)
        }
        _ => None,
    }
}

pub(crate) fn extract_usage_credits_from_sse_bytes(body: &[u8]) -> Option<i64> {
    let text = std::str::from_utf8(body).ok()?;
    let messages = extract_sse_json_messages(text);
    let mut best: Option<i64> = None;
    for message in messages {
        if let Some(credits) = extract_usage_credits_from_value(&message) {
            best = Some(best.map_or(credits, |current| current.max(credits)));
        }
    }
    best
}

pub(crate) fn extract_usage_credits_total_from_sse_bytes(body: &[u8]) -> Option<i64> {
    let text = std::str::from_utf8(body).ok()?;
    let messages = extract_sse_json_messages(text);
    if messages.is_empty() {
        return None;
    }

    // SSE streams can contain multiple messages for the same JSON-RPC `id` (e.g. progress updates).
    // To avoid double-charging, we take the maximum observed credits per id and then sum.
    let mut per_id_max: HashMap<String, i64> = HashMap::new();
    let mut found = false;

    for message in messages {
        let Some(credits) = extract_usage_credits_total_from_value(&message) else {
            continue;
        };
        found = true;

        let id_key = match &message {
            Value::Object(map) => map
                .get("id")
                .filter(|v| !v.is_null())
                .map(|v| v.to_string()),
            _ => None,
        }
        .unwrap_or_else(|| "__no_id__".to_string());

        per_id_max
            .entry(id_key)
            .and_modify(|current| *current = (*current).max(credits))
            .or_insert(credits);
    }

    found.then(|| per_id_max.values().copied().sum())
}

pub(crate) fn classify_status_text(status: &str) -> Option<MessageOutcome> {
    let normalized = status.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }

    if matches!(
        normalized.as_str(),
        "failed" | "failure" | "error" | "errored" | "cancelled" | "canceled"
    ) {
        return Some(MessageOutcome::Error);
    }

    if matches!(
        normalized.as_str(),
        "pending"
            | "processing"
            | "running"
            | "in_progress"
            | "queued"
            | "completed"
            | "success"
            | "succeeded"
            | "done"
    ) {
        return Some(MessageOutcome::Success);
    }

    None
}

pub(crate) fn classify_status_code(code: i64) -> MessageOutcome {
    if code == 432 {
        MessageOutcome::QuotaExhausted
    } else if code >= 400 {
        MessageOutcome::Error
    } else {
        MessageOutcome::Success
    }
}

pub(crate) fn parse_embedded_status(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    if !trimmed.starts_with('{') {
        return None;
    }
    serde_json::from_str::<Value>(trimmed)
        .ok()
        .and_then(|value| {
            extract_status_code(&value).or_else(|| value.get("status").and_then(|v| v.as_i64()))
        })
}

pub(crate) fn extract_sse_json_messages(text: &str) -> Vec<Value> {
    let mut messages = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            if !current.is_empty() {
                if let Ok(value) = serde_json::from_str::<Value>(&current) {
                    messages.push(value);
                }
                current.clear();
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("data:") {
            let content = rest.trim_start();
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(content);
        }
    }

    if !current.is_empty()
        && let Ok(value) = serde_json::from_str::<Value>(&current)
    {
        messages.push(value);
    }

    messages
}

/// Recursively replace any `api_key` field values in JSON with a fixed placeholder.
pub(crate) fn redact_api_key_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if k.eq_ignore_ascii_case("api_key") {
                    *v = Value::String("***redacted***".to_string());
                } else {
                    redact_api_key_fields(v);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_api_key_fields(item);
            }
        }
        _ => {}
    }
}

/// Best-effort redaction helper for request/response bodies written to persistent logs.
/// If the payload is valid JSON, any `api_key` fields are replaced; on parse failure,
/// an empty payload is returned to avoid leaking secrets in ambiguous formats.
pub(crate) fn redact_api_key_bytes(bytes: &[u8]) -> Vec<u8> {
    if bytes.is_empty() {
        return Vec::new();
    }
    match serde_json::from_slice::<Value>(bytes) {
        Ok(mut value) => {
            redact_api_key_fields(&mut value);
            serde_json::to_vec(&value).unwrap_or_else(|_| Vec::new())
        }
        Err(_) => Vec::new(),
    }
}
