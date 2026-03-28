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
        return Err(
            missing_token_response()
                .unwrap_or_else(|status| Response::builder().status(status).body(Body::empty()).unwrap()),
        );
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
        return Err(
            invalid_token_response()
                .unwrap_or_else(|status| Response::builder().status(status).body(Body::empty()).unwrap()),
        );
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

fn mcp_request_contains_method(body: &[u8], needle: &str) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return false;
    };

    let matches_method = |value: &Value| {
        value
            .get("method")
            .and_then(|method| method.as_str())
            .is_some_and(|method| method == needle)
    };

    match value {
        Value::Object(_) => matches_method(&value),
        Value::Array(items) => items.iter().any(matches_method),
        _ => false,
    }
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

async fn mcp_subpath_reject_handler(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    let (parts, body) = req.into_parts();
    let method = parts.method.clone();
    let path = parts.uri.path().to_owned();
    let (query, query_token) = extract_token_from_query(parts.uri.query());
    let authenticated = match authenticate_request_token(&state, &parts.headers, query_token).await {
        Ok(authenticated) => authenticated,
        Err(response) => return Ok(response),
    };
    if authenticated.using_dev_open_admin_fallback {
        return mcp_session_response(
            StatusCode::UNAUTHORIZED,
            "explicit_token_required",
            "MCP requests must provide an explicit token when --dev-open-admin is enabled.",
        );
    }
    let body_bytes = body::to_bytes(body, BODY_LIMIT)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let request_kind = classify_token_request_kind(&path, Some(body_bytes.as_ref()));
    let response_body = b"Not Found".as_slice();
    let empty_headers: [String; 0] = [];

    let request_log_id = match state
        .proxy
        .record_local_request_log_without_key(
            authenticated.token_id.as_deref(),
            &method,
            &path,
            query.as_deref(),
            StatusCode::NOT_FOUND,
            Some(StatusCode::NOT_FOUND.as_u16() as i64),
            &body_bytes,
            response_body,
            "error",
            Some("mcp_path_404"),
            &empty_headers,
            &empty_headers,
        )
        .await
    {
        Ok(log_id) => Some(log_id),
        Err(err) => {
            eprintln!("local MCP subpath reject request_log failed for {path}: {err}");
            None
        }
    };

    if let Some(token_id) = authenticated.token_id.as_deref() {
        let _ = state
            .proxy
            .record_token_attempt_with_kind_request_log_metadata(
                token_id,
                &method,
                &path,
                query.as_deref(),
                Some(StatusCode::NOT_FOUND.as_u16() as i64),
                Some(StatusCode::NOT_FOUND.as_u16() as i64),
                false,
                "error",
                None,
                &request_kind,
                Some("mcp_path_404"),
                Some("none"),
                None,
                request_log_id,
            )
            .await;
    }

    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(response_body.to_vec()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn proxy_handler(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    let (parts, body) = req.into_parts();
    let method = parts.method.clone();
    let path = parts.uri.path().to_owned();
    let (query, query_token) = extract_token_from_query(parts.uri.query());

    if method == Method::GET && accepts_event_stream(&parts.headers) {
        let response = Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .body(Body::empty())
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok(response);
    }

    let authenticated = match authenticate_request_token(&state, &parts.headers, query_token).await {
        Ok(authenticated) => authenticated,
        Err(response) => return Ok(response),
    };
    let token_id = authenticated.token_id;
    let using_dev_open_admin_fallback = authenticated.using_dev_open_admin_fallback;

    let mut headers = clone_headers(&parts.headers);
    // prevent leaking our Authorization to upstream
    headers.remove(axum::http::header::AUTHORIZATION);
    let body_bytes = body::to_bytes(body, BODY_LIMIT)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let is_mcp_request = path.starts_with("/mcp");
    if is_mcp_request && using_dev_open_admin_fallback {
        return mcp_session_response(
            StatusCode::UNAUTHORIZED,
            "explicit_token_required",
            "MCP requests must provide an explicit token when --dev-open-admin is enabled.",
        );
    }
    let is_mcp_initialize = is_mcp_request && mcp_request_contains_method(&body_bytes, "initialize");
    let incoming_proxy_session_id = if is_mcp_request {
        header_string(&headers, "mcp-session-id")
    } else {
        None
    };
    let incoming_protocol_version = if is_mcp_request {
        header_string(&headers, "mcp-protocol-version")
    } else {
        None
    };
    let incoming_last_event_id = if is_mcp_request {
        header_string(&headers, "last-event-id")
    } else {
        None
    };
    let token_user_id = if is_mcp_request {
        if let Some(token_id) = token_id.as_deref() {
            state
                .proxy
                .find_user_id_by_token(token_id)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        } else {
            None
        }
    } else {
        None
    };
    let mut pinned_api_key_id: Option<String> = None;
    if let Some(proxy_session_id) = incoming_proxy_session_id.as_deref() {
        let session = state
            .proxy
            .get_active_mcp_session(proxy_session_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let Some(session) = session else {
            return mcp_session_response(
                StatusCode::CONFLICT,
                "session_unavailable",
                "MCP session is unavailable, please reconnect to initialize a new session.",
            );
        };

        if session.auth_token_id.as_deref() != token_id.as_deref() {
            return mcp_session_response(
                StatusCode::FORBIDDEN,
                "session_forbidden",
                "This MCP session belongs to a different token. Please reconnect.",
            );
        }

        headers.insert(
            HeaderName::from_static("mcp-session-id"),
            ReqHeaderValue::from_str(&session.upstream_session_id)
                .map_err(|_| StatusCode::BAD_REQUEST)?,
        );

        let effective_protocol_version = incoming_protocol_version
            .clone()
            .or(session.protocol_version.clone());
        if let Some(protocol_version) = effective_protocol_version.as_deref() {
            headers.insert(
                HeaderName::from_static("mcp-protocol-version"),
                ReqHeaderValue::from_str(protocol_version)
                    .map_err(|_| StatusCode::BAD_REQUEST)?,
            );
        }

        pinned_api_key_id = Some(session.upstream_key_id.clone());
    }
    let request_kind = classify_token_request_kind(&path, Some(body_bytes.as_ref()));

    // Billing plan (1:1 upstream credits):
    // - Non-business whitelist methods are ignored by business quota.
    // - tools/call for tavily-* does not inject extra MCP arguments unless the upstream contract
    //   is explicitly proven compatible.
    // - Known Tavily tools use a reserved-credit precheck derived from request parameters.
    // - For unknown / batch / positional request shapes, default to billable to avoid bypass.
    let mut billable_flag = false;
    let mut reserved_billable_credits: Option<i64> = None;
    let mut expected_search_credits: Option<i64> = None;
    let forwarded_body = body_bytes.clone();
    let mut lockable_tool = false;
    let mut billable_mcp_ids: HashSet<String> = HashSet::new();
    let mut billable_search_mcp_ids: HashSet<String> = HashSet::new();
    let mut has_billable_mcp_without_id = false;
    let mut has_search_mcp_without_id = false;
    let mut missing_usage_fallback_credits_by_id: HashMap<String, i64> = HashMap::new();
    let mut missing_usage_fallback_credits_without_id_total: i64 = 0;
    let mut expected_search_credits_by_id: HashMap<String, i64> = HashMap::new();
    let mut expected_search_credits_without_id_total: i64 = 0;
    let mut invalid_mcp_request_message: Option<String> = None;
    if path.starts_with("/mcp") {
        match serde_json::from_slice::<Value>(&body_bytes) {
            Ok(mut value) => {
                // Default to billable unless we can *prove* it's a non-billable control plane call.
                let mut any_billable = false;
                let mut any_lockable = false;
                let mut all_non_billable = true;
                let mut reserved_billable_total = 0i64;
                let mut expected_search_total = 0i64;

                let is_non_billable_method = |method: &str| {
                    matches!(method, "initialize" | "ping" | "tools/list")
                        || method.starts_with("resources/")
                        || method.starts_with("prompts/")
                        || method.starts_with("notifications/")
                };

                let handle_tool_call = |map: &mut serde_json::Map<String, Value>,
                                        any_billable: &mut bool,
                                        any_lockable: &mut bool,
                                        all_non_billable: &mut bool,
                                        reserved_billable_total: &mut i64,
                                        expected_search_total: &mut i64,
                                        billable_mcp_ids: &mut HashSet<String>,
                                        billable_search_mcp_ids: &mut HashSet<String>,
                                        has_billable_mcp_without_id: &mut bool,
                                        has_search_mcp_without_id: &mut bool,
                                        missing_usage_fallback_credits_by_id: &mut HashMap<String, i64>,
                                        missing_usage_fallback_credits_without_id_total: &mut i64,
                                        expected_search_credits_by_id: &mut HashMap<String, i64>,
                                        expected_search_credits_without_id_total: &mut i64| {
                    // tools/call is treated as billable by default unless we can prove it's
                    // a non-Tavily tool call (name does not start with `tavily-`).
                    *any_lockable = true;

                    let id_key = map
                        .get("id")
                        .filter(|v| !v.is_null())
                        .map(|v| v.to_string());

                    if let Some(Value::Object(params)) = map.get_mut("params") {
                        let tool = params
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .trim()
                            .to_string();

                        let normalized_tool = tool.to_ascii_lowercase().replace('_', "-");
                        let usage_metered_tool = matches!(
                            normalized_tool.as_str(),
                            "tavily-search" | "tavily-extract" | "tavily-crawl" | "tavily-map"
                        );
                        let reserved_billable_tool = matches!(
                            normalized_tool.as_str(),
                            "tavily-search"
                                | "tavily-extract"
                                | "tavily-crawl"
                                | "tavily-map"
                                | "tavily-research"
                        );
                        let is_tavily_tool = normalized_tool.starts_with("tavily-");

                        if reserved_billable_tool || is_tavily_tool {
                            *any_billable = true;
                            *all_non_billable = false;

                            if let Some(id_key) = id_key.as_ref() {
                                billable_mcp_ids.insert(id_key.clone());
                                if normalized_tool == "tavily-search" {
                                    billable_search_mcp_ids.insert(id_key.clone());
                                }
                            } else {
                                *has_billable_mcp_without_id = true;
                                if normalized_tool == "tavily-search" {
                                    *has_search_mcp_without_id = true;
                                }
                            }

                            let record_reserved_credits = |reserved: i64,
                                                           reserved_billable_total: &mut i64| {
                                *reserved_billable_total =
                                    (*reserved_billable_total).saturating_add(reserved);
                            };

                            let record_missing_usage_fallback = |fallback: i64,
                                                                 id_key: Option<&String>,
                                                                 missing_usage_fallback_credits_by_id: &mut HashMap<String, i64>,
                                                                 missing_usage_fallback_credits_without_id_total: &mut i64| {
                                if let Some(id_key) = id_key {
                                    missing_usage_fallback_credits_by_id
                                        .entry(id_key.clone())
                                        .and_modify(|current| {
                                            *current = (*current).saturating_add(fallback)
                                        })
                                        .or_insert(fallback);
                                } else {
                                    *missing_usage_fallback_credits_without_id_total =
                                        (*missing_usage_fallback_credits_without_id_total)
                                            .saturating_add(fallback);
                                }
                            };

                            if usage_metered_tool {
                                let args_entry = params.get("arguments").unwrap_or(&Value::Null);
                                let reserved =
                                    tavily_mcp_reserved_credits(normalized_tool.as_str(), args_entry);
                                record_reserved_credits(
                                    reserved,
                                    reserved_billable_total,
                                );

                                if normalized_tool == "tavily-search" {
                                    let expected = tavily_search_expected_credits(args_entry);
                                    *expected_search_total =
                                        (*expected_search_total).saturating_add(expected);
                                    if let Some(id_key) = id_key.as_ref() {
                                        expected_search_credits_by_id
                                            .entry(id_key.clone())
                                            .and_modify(|current| {
                                                *current = (*current).saturating_add(expected)
                                            })
                                            .or_insert(expected);
                                    } else {
                                        *expected_search_credits_without_id_total =
                                            (*expected_search_credits_without_id_total)
                                                .saturating_add(expected);
                                    }
                                }
                            } else if reserved_billable_tool {
                                let args_entry = params.get("arguments").unwrap_or(&Value::Null);
                                let reserved =
                                    tavily_mcp_reserved_credits(normalized_tool.as_str(), args_entry);
                                record_reserved_credits(
                                    reserved,
                                    reserved_billable_total,
                                );
                                record_missing_usage_fallback(
                                    reserved,
                                    id_key.as_ref(),
                                    missing_usage_fallback_credits_by_id,
                                    missing_usage_fallback_credits_without_id_total,
                                );
                            } else {
                                // Unknown `tavily-*` tool: keep the original arguments/body shape,
                                // but still treat it as billable so new upstream tools cannot bypass quota.
                                record_reserved_credits(
                                    1,
                                    reserved_billable_total,
                                );
                            }
                        } else if tool.is_empty() {
                            // Unknown tool name: billable safe default.
                            *any_billable = true;
                            *all_non_billable = false;

                            if let Some(id_key) = id_key.as_ref() {
                                billable_mcp_ids.insert(id_key.clone());
                            } else {
                                *has_billable_mcp_without_id = true;
                            }
                        } else {
                            // Proven non-Tavily tool call: do not charge business quota.
                        }
                    } else {
                        // Missing params: billable safe default.
                        *any_billable = true;
                        *all_non_billable = false;

                        if let Some(id_key) = id_key.as_ref() {
                            billable_mcp_ids.insert(id_key.clone());
                        } else {
                            *has_billable_mcp_without_id = true;
                        }
                    }
                };

                match value {
                    Value::Object(ref mut map) => {
                        let method = map.get("method").and_then(|v| v.as_str()).unwrap_or("");
                        let non_billable = is_non_billable_method(method);
                        if !non_billable {
                            all_non_billable = false;
                        }

                        if method == "tools/call" {
                            handle_tool_call(
                                map,
                                &mut any_billable,
                                &mut any_lockable,
                                &mut all_non_billable,
                                &mut reserved_billable_total,
                                &mut expected_search_total,
                                &mut billable_mcp_ids,
                                &mut billable_search_mcp_ids,
                                &mut has_billable_mcp_without_id,
                                &mut has_search_mcp_without_id,
                                &mut missing_usage_fallback_credits_by_id,
                                &mut missing_usage_fallback_credits_without_id_total,
                                &mut expected_search_credits_by_id,
                                &mut expected_search_credits_without_id_total,
                            );
                        } else if !non_billable {
                            // Unknown object-shaped method: treat as billable (safe default).
                            any_billable = true;
                            any_lockable = true;
                        }
                    }
                    Value::Array(ref mut items) => {
                        // JSON-RPC batch: only treat as non-billable if *every* item is provably
                        // a control-plane method or a non-Tavily tool call.
                        let mut seen_ids: HashSet<String> = HashSet::new();
                        for item in items.iter_mut() {
                            let Some(map) = item.as_object_mut() else {
                                // Positional/batch junk: billable safe default.
                                any_billable = true;
                                any_lockable = true;
                                all_non_billable = false;
                                continue;
                            };

                            if map
                                .get("id")
                                .filter(|v| !v.is_null())
                                .map(|v| v.to_string())
                                .is_some_and(|id_key| !seen_ids.insert(id_key))
                            {
                                invalid_mcp_request_message.get_or_insert_with(|| {
                                    "duplicate JSON-RPC id in batch".to_string()
                                });
                            }

                            let method =
                                map.get("method").and_then(|v| v.as_str()).unwrap_or("");
                            let non_billable = is_non_billable_method(method);
                            if !non_billable {
                                all_non_billable = false;
                            }

                            if method == "tools/call" {
                                handle_tool_call(
                                    map,
                                    &mut any_billable,
                                    &mut any_lockable,
                                    &mut all_non_billable,
                                    &mut reserved_billable_total,
                                    &mut expected_search_total,
                                    &mut billable_mcp_ids,
                                    &mut billable_search_mcp_ids,
                                    &mut has_billable_mcp_without_id,
                                    &mut has_search_mcp_without_id,
                                    &mut missing_usage_fallback_credits_by_id,
                                    &mut missing_usage_fallback_credits_without_id_total,
                                    &mut expected_search_credits_by_id,
                                    &mut expected_search_credits_without_id_total,
                                );
                            } else if !non_billable {
                                any_billable = true;
                                any_lockable = true;
                            }
                        }
                    }
                    _ => {
                        // Unknown / non-object: treat as billable to avoid bypass.
                        any_billable = true;
                        any_lockable = true;
                        all_non_billable = false;
                    }
                }

                billable_flag = any_billable && !all_non_billable;
                lockable_tool = any_lockable && billable_flag;
                if reserved_billable_total > 0 {
                    reserved_billable_credits = Some(reserved_billable_total);
                }
                if expected_search_total > 0 {
                    expected_search_credits = Some(expected_search_total);
                }

            }
            Err(_) => {
                // Non-JSON / unparseable: treat as billable to avoid bypass.
                billable_flag = true;
                lockable_tool = true;
            }
        }
    }

    let proxy_request = ProxyRequest {
        method: method.clone(),
        path: path.clone(),
        query: query.clone(),
        headers,
        body: forwarded_body.clone(),
        auth_token_id: token_id.clone(),
        pinned_api_key_id,
    };

    // Serialize per-token billable tool calls to keep `peek -> upstream -> charge` consistent.
    let token_billing_guard = if !using_dev_open_admin_fallback
        && billable_flag
        && lockable_tool
        && invalid_mcp_request_message.is_none()
        && let Some(tid) = token_id.as_deref()
    {
        Some(
            state
                .proxy
                .lock_token_billing(tid)
                .await
                .map_err(|err| {
                    eprintln!("token billing lock failed: {err}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?,
        )
    } else {
        None
    };
    let billing_subject = token_billing_guard
        .as_ref()
        .map(|guard| guard.billing_subject().to_string());
    if let Some(guard) = token_billing_guard.as_ref() {
        guard.ensure_live().map_err(|err| {
            eprintln!("token billing lock lost before precheck: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    let mut _quota_verdict: Option<TokenQuotaVerdict> = None;
    if let Some(tid) = token_id.as_deref() {
        // 1) 全量“任意请求”小时限频：所有通过鉴权的请求都会计入。
        if !using_dev_open_admin_fallback {
            match state.proxy.check_token_hourly_requests(tid).await {
                Ok(verdict) => {
                    if !verdict.allowed {
                        let message = build_request_limit_error_message(&verdict);
                        let _ = state
                            .proxy
                            .record_token_attempt_with_kind(
                                tid,
                                &method,
                                &path,
                                query.as_deref(),
                                Some(StatusCode::TOO_MANY_REQUESTS.as_u16() as i64),
                                None,
                                false,
                                "quota_exhausted",
                                Some(&message),
                                &request_kind,
                            )
                            .await;
                        let response = request_limit_exceeded_response(&verdict)?;
                        return Ok(response);
                    }
                }
                Err(err) => {
                    eprintln!("hourly request limit check failed: {err}");
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
            }
        }

        // Reject billable MCP calls that cannot be safely attributed/billed.
        if billable_flag
            && invalid_mcp_request_message.is_some()
            && path.starts_with("/mcp")
        {
            let message = invalid_mcp_request_message
                .clone()
                .unwrap_or_else(|| "invalid MCP request".to_string());

            if let Some(tid) = token_id.as_deref() {
                let _ = state
                    .proxy
                    .record_token_attempt_with_kind(
                        tid,
                        &method,
                        &path,
                        query.as_deref(),
                        Some(StatusCode::BAD_REQUEST.as_u16() as i64),
                        None,
                        billable_flag,
                        "error",
                        Some(&message),
                        &request_kind,
                    )
                    .await;
            }

            let payload = json!({
                "error": "invalid_request",
                "message": message,
            });
            let resp = Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header(CONTENT_TYPE, "application/json; charset=utf-8")
                .body(Body::from(payload.to_string()))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            return Ok(resp);
        }

        // 2) 业务配额（小时 / 日 / 月）只对 MCP 工具调用生效。
        if billable_flag {
            match if let Some(subject) = billing_subject.as_deref() {
                state.proxy.peek_token_quota_for_subject(subject).await
            } else {
                state.proxy.peek_token_quota(tid).await
            } {
                Ok(verdict) => {
                    if !using_dev_open_admin_fallback {
                        let blocked = if let Some(expected) = reserved_billable_credits {
                            quota_would_exceed(&verdict, expected)
                        } else {
                            quota_exhausted_now(&verdict)
                        };

                        if blocked {
                            let message = build_quota_error_message(&verdict, reserved_billable_credits);
                            let _ = state
                                .proxy
                                .record_token_attempt_with_kind(
                                    tid,
                                    &method,
                                    &path,
                                    query.as_deref(),
                                    Some(StatusCode::TOO_MANY_REQUESTS.as_u16() as i64),
                                    None,
                                    true,
                                    "quota_exhausted",
                                    Some(&message),
                                    &request_kind,
                                )
                                .await;
                            let response = quota_exceeded_response(&verdict, reserved_billable_credits)?;
                            return Ok(response);
                        }
                    }
                    _quota_verdict = Some(verdict);
                }
                Err(err) => {
                    eprintln!("quota peek failed: {err}");
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
            }
        }
    }

    let proxy_result = state.proxy.proxy_request(proxy_request).await;

    match proxy_result {
        Ok(mut resp) => {
            if is_mcp_request {
                if let Some(proxy_session_id) = incoming_proxy_session_id.as_deref() {
                    if mcp_response_requires_reconnect(resp.status, &resp.body) {
                        let _ = state
                            .proxy
                            .revoke_mcp_session(proxy_session_id, "upstream_session_invalid")
                            .await;
                        resp.status = StatusCode::CONFLICT;
                        resp.headers = ReqHeaderMap::new();
                        resp.headers.insert(
                            CONTENT_TYPE,
                            ReqHeaderValue::from_static("application/json; charset=utf-8"),
                        );
                        resp.body = mcp_session_body(
                            "session_unavailable",
                            "Upstream MCP session expired or became unavailable. Please reconnect.",
                        );
                    } else {
                        let response_protocol_version = resp
                            .headers
                            .get("mcp-protocol-version")
                            .and_then(|value| value.to_str().ok())
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(str::to_string);
                        let _ = state
                            .proxy
                            .touch_mcp_session(
                                proxy_session_id,
                                response_protocol_version
                                    .as_deref()
                                    .or(incoming_protocol_version.as_deref()),
                                incoming_last_event_id.as_deref(),
                            )
                            .await;
                        if let Some(upstream_session_id) = resp
                            .headers
                            .get("mcp-session-id")
                            .and_then(|value| value.to_str().ok())
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                        {
                            let _ = state
                                .proxy
                                .update_mcp_session_upstream_identity(
                                    proxy_session_id,
                                    upstream_session_id,
                                    response_protocol_version
                                        .as_deref()
                                        .or(incoming_protocol_version.as_deref()),
                                )
                                .await;
                            if let Ok(proxy_header) = ReqHeaderValue::from_str(proxy_session_id) {
                                resp.headers
                                    .insert(HeaderName::from_static("mcp-session-id"), proxy_header);
                            }
                        }
                    }
                } else if is_mcp_initialize && resp.status.is_success() {
                    let upstream_session_id = resp
                        .headers
                        .get("mcp-session-id")
                        .and_then(|value| value.to_str().ok())
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string);
                    if let (Some(upstream_session_id), Some(upstream_key_id)) =
                        (upstream_session_id.as_deref(), resp.api_key_id.as_deref())
                    {
                        let proxy_session_id = state
                            .proxy
                            .create_mcp_session(
                                upstream_session_id,
                                upstream_key_id,
                                token_id.as_deref(),
                                token_user_id.as_deref(),
                                incoming_protocol_version.as_deref(),
                                incoming_last_event_id.as_deref(),
                            )
                            .await
                            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                        resp.headers.insert(
                            HeaderName::from_static("mcp-session-id"),
                            ReqHeaderValue::from_str(&proxy_session_id)
                                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                        );
                    }
                }
            }
            let mut billing_error: Option<String> = None;
            if let Some(tid) = token_id.as_deref() {
                let analysis = analyze_mcp_attempt(resp.status, &resp.body);
                let api_key_id = resp.api_key_id.as_deref();
                let tavily_code: Option<i64> = analysis.tavily_status_code;
                let result_status = analysis.status;
                let mut attempt_logged = false;

                // Charge credits after a successful billable Tavily tool call.
                //
                // NOTE: We also charge when the overall attempt is marked `quota_exhausted`,
                // because JSON-RPC batches can contain a mix of successes and quota errors. In
                // that case we only charge credits we can actually observe from `usage.credits`
                // to avoid guessing partial failures.
                let allow_empty_body_search_fallback =
                    resp.body.is_empty() && expected_search_credits.is_some();
                if billable_flag && resp.status.is_success() {
                    let missing_usage_fallback_total = {
                        let total = expected_search_credits
                            .unwrap_or(0)
                            .saturating_add(
                                missing_usage_fallback_credits_by_id
                                    .values()
                                    .copied()
                                    .sum::<i64>(),
                            )
                            .saturating_add(missing_usage_fallback_credits_without_id_total);
                        (total > 0).then_some(total)
                    };
                    let credits = if has_billable_mcp_without_id {
                        let mut response_has_error = mcp_response_has_any_error(&resp.body);
                        let mut response_has_success = mcp_response_has_any_success(&resp.body);
                        if allow_empty_body_search_fallback {
                            response_has_error = false;
                            response_has_success = true;
                        }

                        // Without JSON-RPC ids we cannot reliably separate billable vs non-billable
                        // response items, so we only charge observed credits when the response
                        // still shows at least one successful tool call.
                        match extract_usage_credits_total_from_json_bytes(&resp.body) {
                            Some(credits) => {
                                if response_has_error && !response_has_success {
                                    0
                                } else if response_has_error {
                                    credits
                                } else if let Some(expected) = missing_usage_fallback_total {
                                    credits.max(expected)
                                } else {
                                    credits
                                }
                            }
                            None => {
                                if response_has_error || !response_has_success {
                                    0
                                } else if let Some(expected) = missing_usage_fallback_total {
                                    expected
                                } else {
                                    eprintln!(
                                        "missing usage.credits for MCP tool response; skipping billing"
                                    );
                                    0
                                }
                            }
                        }
                    } else {
                        let errors_by_id = extract_mcp_has_error_by_id_from_bytes(&resp.body);
                        let credits_by_id = extract_mcp_usage_credits_by_id_from_bytes(&resp.body);
                        let mut total = 0i64;

                        for id in billable_mcp_ids.iter() {
                            let id_has_error = if allow_empty_body_search_fallback {
                                false
                            } else {
                                *errors_by_id.get(id).unwrap_or(&true)
                            };
                            if id_has_error {
                                continue;
                            }

                            if let Some(credits) = credits_by_id.get(id) {
                                total = total.saturating_add(*credits);
                                continue;
                            }

                            if billable_search_mcp_ids.contains(id)
                                && let Some(expected) = expected_search_credits_by_id.get(id)
                            {
                                total = total.saturating_add(*expected);
                                continue;
                            }

                            if let Some(fallback) = missing_usage_fallback_credits_by_id.get(id) {
                                total = total.saturating_add(*fallback);
                            }
                        }

                        total
                    };

                    if credits > 0 {
                        match if let Some(subject) = billing_subject.as_deref() {
                            state
                                .proxy
                                .record_pending_billing_attempt_for_subject_with_kind_request_log(
                                    tid,
                                    &method,
                                    &path,
                                    query.as_deref(),
                                    Some(resp.status.as_u16() as i64),
                                    tavily_code,
                                    billable_flag,
                                    result_status,
                                    None,
                                    credits,
                                    subject,
                                    &request_kind,
                                    api_key_id,
                                    analysis.failure_kind.as_deref(),
                                    Some(resp.key_effect_code.as_str()),
                                    resp.key_effect_summary.as_deref(),
                                    resp.request_log_id,
                                )
                                .await
                        } else {
                            state
                                .proxy
                                .record_pending_billing_attempt_with_kind_request_log_metadata(
                                    tid,
                                    &method,
                                    &path,
                                    query.as_deref(),
                                    Some(resp.status.as_u16() as i64),
                                    tavily_code,
                                    billable_flag,
                                    result_status,
                                    None,
                                    credits,
                                    &request_kind,
                                    api_key_id,
                                    analysis.failure_kind.as_deref(),
                                    Some(resp.key_effect_code.as_str()),
                                    resp.key_effect_summary.as_deref(),
                                    resp.request_log_id,
                                )
                                .await
                        }
                        {
                            Ok(log_id) => {
                                attempt_logged = true;
                                let lock_lost_msg = token_billing_guard
                                    .as_ref()
                                    .and_then(|guard| guard.ensure_live().err())
                                    .map(|err| {
                                        format!(
                                            "charge_token_quota deferred for {path}: {err}; pending billing will retry"
                                        )
                                    });
                                if let Some(msg) = lock_lost_msg {
                                    eprintln!("{msg}");
                                    let _ = state
                                        .proxy
                                        .annotate_pending_billing_attempt(log_id, &msg)
                                        .await;
                                    billing_error = Some(msg);
                                } else {
                                    match state.proxy.settle_pending_billing_attempt(log_id).await {
                                    Ok(PendingBillingSettleOutcome::Charged)
                                    | Ok(PendingBillingSettleOutcome::AlreadySettled) => {}
                                    Ok(PendingBillingSettleOutcome::RetryLater) => {
                                        let msg = format!(
                                            "charge_token_quota delayed for {path}: pending billing claim miss; will retry"
                                        );
                                        eprintln!("{msg}");
                                        let _ = state
                                            .proxy
                                            .annotate_pending_billing_attempt(log_id, &msg)
                                            .await;
                                        billing_error = Some(msg);
                                    }
                                    Err(err) => {
                                        let msg = format!("charge_token_quota failed for {path}: {err}");
                                        eprintln!("{msg}");
                                        let _ = state
                                            .proxy
                                            .annotate_pending_billing_attempt(log_id, &msg)
                                            .await;
                                        billing_error = Some(msg);
                                    }
                                }
                                }
                            }
                            Err(err) => {
                                let msg = format!(
                                    "record_pending_billing_attempt failed for {path}: {err}"
                                );
                                eprintln!("{msg}");
                                billing_error = Some(msg);
                            }
                        }
                    }
                }
                if !attempt_logged {
                    let http_code = resp.status.as_u16() as i64;
                    let _ = state
                        .proxy
                        .record_token_attempt_with_kind_request_log_metadata(
                            tid,
                            &method,
                            &path,
                            query.as_deref(),
                            Some(http_code),
                            tavily_code,
                            billable_flag,
                            result_status,
                            billing_error.as_deref(),
                            &request_kind,
                            analysis.failure_kind.as_deref(),
                            Some(resp.key_effect_code.as_str()),
                            resp.key_effect_summary.as_deref(),
                            resp.request_log_id,
                        )
                        .await;
                }
            }
            // Always return the upstream response, even if local billing persistence fails.
            // Returning a 5xx here can trigger client retries and cause duplicate upstream charges.
            Ok(build_response(resp))
        }
        Err(err) => {
            eprintln!("proxy error: {err}");
            let pinned_mcp_session_unavailable =
                matches!(&err, ProxyError::PinnedMcpSessionUnavailable);
            if let Some(tid) = token_id.as_deref() {
                let err_str = err.to_string();
                let _ = state
                    .proxy
                    .record_token_attempt_with_kind(
                        tid,
                        &method,
                        &path,
                        query.as_deref(),
                        None,
                        None,
                        billable_flag,
                        "error",
                        Some(err_str.as_str()),
                        &request_kind,
                    )
                    .await;
            }
            if pinned_mcp_session_unavailable
                && let Some(proxy_session_id) = incoming_proxy_session_id.as_deref()
            {
                let _ = state
                    .proxy
                    .revoke_mcp_session(proxy_session_id, "pinned_key_unavailable")
                    .await;
                return mcp_session_response(
                    StatusCode::CONFLICT,
                    "session_unavailable",
                    "The pinned MCP session key is unavailable. Please reconnect.",
                );
            }
            Err(StatusCode::BAD_GATEWAY)
        }
    }
}

fn clone_headers(headers: &HeaderMap) -> ReqHeaderMap {
    let mut map = ReqHeaderMap::new();
    for (name, value) in headers.iter() {
        if let Ok(cloned) = ReqHeaderValue::from_bytes(value.as_bytes()) {
            map.insert(name.clone(), cloned);
        }
    }
    map
}

fn accepts_event_stream(headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .map(|raw| {
            raw.split(',')
                .any(|v| v.trim().eq_ignore_ascii_case("text/event-stream"))
        })
        .unwrap_or(false)
}

fn build_response(resp: ProxyResponse) -> Response<Body> {
    let mut builder = Response::builder().status(resp.status);
    if let Some(headers) = builder.headers_mut() {
        for (name, value) in resp.headers.iter() {
            if name == TRANSFER_ENCODING || name == CONNECTION || name == CONTENT_LENGTH {
                continue;
            }
            headers.append(name.clone(), value.clone());
        }
        headers.insert(CONTENT_LENGTH, value_from_len(resp.body.len()));
    }
    builder
        .body(Body::from(resp.body))
        .unwrap_or_else(|_| Response::builder().status(500).body(Body::empty()).unwrap())
}

fn value_from_len(len: usize) -> axum::http::HeaderValue {
    axum::http::HeaderValue::from_str(len.to_string().as_str())
        .unwrap_or_else(|_| axum::http::HeaderValue::from_static("0"))
}

fn request_limit_exceeded_response(
    verdict: &TokenHourlyRequestVerdict,
) -> Result<Response<Body>, StatusCode> {
    let payload = json!({
        "error": "quota_exhausted",
        "window": "hour",
        "hourlyAny": {
            "limit": verdict.hourly_limit,
            "used": verdict.hourly_used,
        },
    });

    Response::builder()
        .status(StatusCode::TOO_MANY_REQUESTS)
        .header(CONTENT_TYPE, "application/json; charset=utf-8")
        .body(Body::from(payload.to_string()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn quota_exceeded_response(
    verdict: &TokenQuotaVerdict,
    projected_delta: Option<i64>,
) -> Result<Response<Body>, StatusCode> {
    let window = verdict.window_name_for_delta(projected_delta.unwrap_or(0));
    let payload = json!({
        "error": "quota_exceeded",
        "window": window,
        "hourly": {
            "limit": verdict.hourly_limit,
            "used": verdict.hourly_used,
        },
        "daily": {
            "limit": verdict.daily_limit,
            "used": verdict.daily_used,
        },
        "monthly": {
            "limit": verdict.monthly_limit,
            "used": verdict.monthly_used,
        },
    });

    Response::builder()
        .status(StatusCode::TOO_MANY_REQUESTS)
        .header(CONTENT_TYPE, "application/json; charset=utf-8")
        .body(Body::from(payload.to_string()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn build_request_limit_error_message(verdict: &TokenHourlyRequestVerdict) -> String {
    format!(
        "token hourly request limit exceeded (limit {}, used {})",
        verdict.hourly_limit, verdict.hourly_used
    )
}

fn build_quota_error_message(verdict: &TokenQuotaVerdict, projected_delta: Option<i64>) -> String {
    let delta = projected_delta.unwrap_or(0);
    let (limit, used) = quota_window_stats(verdict, delta);
    let window = verdict.window_name_for_delta(delta).unwrap_or("unknown");
    format!("token quota exceeded on {window} window (limit {limit}, used {used})")
}

fn quota_window_stats(verdict: &TokenQuotaVerdict, projected_delta: i64) -> (i64, i64) {
    match verdict.window_name_for_delta(projected_delta).unwrap_or("hour") {
        "month" => (verdict.monthly_limit, verdict.monthly_used),
        "day" => (verdict.daily_limit, verdict.daily_used),
        _ => (verdict.hourly_limit, verdict.hourly_used),
    }
}

impl ApiKeyView {
    fn from_list(metrics: ApiKeyMetrics) -> Self {
        Self::from_metrics(metrics, false)
    }

    fn from_detail(metrics: ApiKeyMetrics) -> Self {
        Self::from_metrics(metrics, true)
    }

    fn from_metrics(metrics: ApiKeyMetrics, include_quarantine_detail: bool) -> Self {
        Self {
            id: metrics.id,
            status: metrics.status,
            group: metrics.group_name,
            registration_ip: metrics.registration_ip,
            registration_region: metrics.registration_region,
            status_changed_at: metrics.status_changed_at,
            last_used_at: metrics.last_used_at,
            deleted_at: metrics.deleted_at,
            quota_limit: metrics.quota_limit,
            quota_remaining: metrics.quota_remaining,
            quota_synced_at: metrics.quota_synced_at,
            total_requests: metrics.total_requests,
            success_count: metrics.success_count,
            error_count: metrics.error_count,
            quota_exhausted_count: metrics.quota_exhausted_count,
            quarantine: metrics.quarantine.map(|quarantine| ApiKeyQuarantineView {
                source: quarantine.source,
                reason_code: quarantine.reason_code,
                reason_summary: quarantine.reason_summary,
                reason_detail: include_quarantine_detail.then_some(quarantine.reason_detail),
                created_at: quarantine.created_at,
            }),
        }
    }
}

fn decode_body(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        None
    } else {
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

impl From<RequestLogRecord> for RequestLogView {
    fn from(record: RequestLogRecord) -> Self {
        let request_kind_protocol_group =
            token_request_kind_protocol_group(&record.request_kind_key).to_string();
        let operational_class = operational_class_for_request_log(
            &record.request_kind_key,
            Some(&record.request_body),
            &record.result_status,
            record.failure_kind.as_deref(),
        );
        let request_kind_billing_group = token_request_kind_billing_group_for_request_log(
            &record.request_kind_key,
            Some(&record.request_body),
        );
        Self {
            id: record.id,
            key_id: record.key_id,
            auth_token_id: record.auth_token_id,
            method: record.method,
            path: record.path,
            query: record.query,
            http_status: record.status_code,
            mcp_status: record.tavily_status_code,
            business_credits: record.business_credits,
            request_kind_key: record.request_kind_key,
            request_kind_label: record.request_kind_label,
            request_kind_detail: record.request_kind_detail,
            result_status: record.result_status,
            created_at: record.created_at,
            error_message: record.error_message,
            failure_kind: record.failure_kind,
            key_effect_code: record.key_effect_code,
            key_effect_summary: record.key_effect_summary,
            request_body: decode_body(&record.request_body),
            response_body: decode_body(&record.response_body),
            forwarded_headers: record.forwarded_headers,
            dropped_headers: record.dropped_headers,
            operational_class: operational_class.to_string(),
            request_kind_protocol_group,
            request_kind_billing_group: request_kind_billing_group.to_string(),
        }
    }
}

impl RequestLogView {
    fn from_token_record(record: TokenLogRecord, token_id: &str) -> Self {
        let request_kind_key = record.request_kind_key.clone();
        let request_kind_protocol_group =
            token_request_kind_protocol_group(&request_kind_key).to_string();
        let request_kind_billing_group = token_request_kind_billing_group_for_token_log(
            &request_kind_key,
            record.counts_business_quota,
        )
        .to_string();
        let operational_class = operational_class_for_token_log(
            &request_kind_key,
            &record.result_status,
            record.failure_kind.as_deref(),
            record.counts_business_quota,
        )
        .to_string();
        Self {
            id: record.id,
            key_id: record.key_id,
            auth_token_id: Some(token_id.to_string()),
            method: record.method,
            path: record.path,
            query: record.query,
            http_status: record.http_status,
            mcp_status: record.mcp_status,
            business_credits: record.business_credits,
            request_kind_key: record.request_kind_key,
            request_kind_label: record.request_kind_label,
            request_kind_detail: record.request_kind_detail,
            result_status: record.result_status,
            created_at: record.created_at,
            error_message: record.error_message,
            failure_kind: record.failure_kind,
            key_effect_code: record.key_effect_code,
            key_effect_summary: record.key_effect_summary,
            request_body: None,
            response_body: None,
            forwarded_headers: Vec::new(),
            dropped_headers: Vec::new(),
            operational_class,
            request_kind_protocol_group,
            request_kind_billing_group,
        }
    }
}

impl From<ProxySummary> for SummaryView {
    fn from(summary: ProxySummary) -> Self {
        Self {
            total_requests: summary.total_requests,
            success_count: summary.success_count,
            error_count: summary.error_count,
            quota_exhausted_count: summary.quota_exhausted_count,
            active_keys: summary.active_keys,
            exhausted_keys: summary.exhausted_keys,
            quarantined_keys: summary.quarantined_keys,
            last_activity: summary.last_activity,
            total_quota_limit: summary.total_quota_limit,
            total_quota_remaining: summary.total_quota_remaining,
        }
    }
}
