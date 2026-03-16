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

    // Require Authorization: Bearer th-<id>-<secret>
    let auth_bearer = parts
        .headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string());

    let header_token = auth_bearer
        .as_deref()
        .and_then(|raw| raw.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string());

    let token = if let Some(t) = header_token {
        t
    } else if let Some(t) = query_token {
        t
    } else if state.dev_open_admin {
        "th-dev-override".to_string()
    } else {
        return Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(CONTENT_TYPE, "application/json; charset=utf-8")
            .body(Body::from("{\"error\":\"missing token\"}"))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
    };

    let valid = if state.dev_open_admin {
        true
    } else {
        state
            .proxy
            .validate_access_token(&token)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };
    if !valid {
        return Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(CONTENT_TYPE, "application/json; charset=utf-8")
            .body(Body::from("{\"error\":\"invalid or disabled token\"}"))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
    }

    let mut headers = clone_headers(&parts.headers);
    // prevent leaking our Authorization to upstream
    headers.remove(axum::http::header::AUTHORIZATION);
    let body_bytes = body::to_bytes(body, BODY_LIMIT)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let request_kind = classify_token_request_kind(&path, Some(body_bytes.as_ref()));

    // Billing plan (1:1 upstream credits):
    // - Non-business whitelist methods are ignored by business quota.
    // - tools/call for tavily-* injects include_usage=true so upstream returns usage.credits.
    // - Known Tavily tools use a reserved-credit precheck derived from request parameters.
    // - For unknown / batch / positional request shapes, default to billable to avoid bypass.
    let mut billable_flag = false;
    let mut reserved_billable_credits: Option<i64> = None;
    let mut expected_search_credits: Option<i64> = None;
    let mut forwarded_body = body_bytes.clone();
    let mut lockable_tool = false;
    let mut billable_mcp_ids: HashSet<String> = HashSet::new();
    let mut billable_search_mcp_ids: HashSet<String> = HashSet::new();
    let mut has_billable_mcp_without_id = false;
    let mut has_search_mcp_without_id = false;
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
                let mut mutated = false;
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
                                        mutated: &mut bool,
                                        reserved_billable_total: &mut i64,
                                        expected_search_total: &mut i64,
                                        billable_mcp_ids: &mut HashSet<String>,
                                        billable_search_mcp_ids: &mut HashSet<String>,
                                        has_billable_mcp_without_id: &mut bool,
                                        has_search_mcp_without_id: &mut bool,
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

                        let supported_billable_tool = matches!(
                            tool.as_str(),
                            "tavily-search" | "tavily-extract" | "tavily-crawl" | "tavily-map"
                        );
                        let is_tavily_tool = tool.starts_with("tavily-");

                        if supported_billable_tool || is_tavily_tool {
                            *any_billable = true;
                            *all_non_billable = false;

                            if let Some(id_key) = id_key.as_ref() {
                                billable_mcp_ids.insert(id_key.clone());
                                if tool == "tavily-search" {
                                    billable_search_mcp_ids.insert(id_key.clone());
                                }
                            } else {
                                *has_billable_mcp_without_id = true;
                                if tool == "tavily-search" {
                                    *has_search_mcp_without_id = true;
                                }
                            }

                            if supported_billable_tool {
                                let mut injected_include_usage = false;
                                if !params.contains_key("arguments") {
                                    params.insert(
                                        "arguments".to_string(),
                                        Value::Object(serde_json::Map::new()),
                                    );
                                    injected_include_usage = true;
                                }

                                let args_entry = params
                                    .get_mut("arguments")
                                    .expect("arguments must exist after insertion when absent");
                                if let Value::Object(args) = args_entry {
                                    let already_true = args
                                        .get("include_usage")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false);
                                    if !already_true {
                                        args.insert("include_usage".to_string(), Value::Bool(true));
                                        injected_include_usage = true;
                                    }
                                }
                                *mutated |= injected_include_usage;

                                let reserved = tavily_mcp_reserved_credits(tool.as_str(), args_entry);
                                *reserved_billable_total =
                                    (*reserved_billable_total).saturating_add(reserved);

                                if tool == "tavily-search" {
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
                            } else {
                                // Unknown `tavily-*` tool: keep the original arguments/body shape,
                                // but still treat it as billable so new upstream tools cannot bypass quota.
                                *reserved_billable_total =
                                    (*reserved_billable_total).saturating_add(1);
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
                                &mut mutated,
                                &mut reserved_billable_total,
                                &mut expected_search_total,
                                &mut billable_mcp_ids,
                                &mut billable_search_mcp_ids,
                                &mut has_billable_mcp_without_id,
                                &mut has_search_mcp_without_id,
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
                                    &mut mutated,
                                    &mut reserved_billable_total,
                                    &mut expected_search_total,
                                    &mut billable_mcp_ids,
                                    &mut billable_search_mcp_ids,
                                    &mut has_billable_mcp_without_id,
                                    &mut has_search_mcp_without_id,
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

                if mutated
                    && let Ok(encoded) = serde_json::to_vec(&value)
                {
                    forwarded_body = bytes::Bytes::from(encoded);
                }
            }
            Err(_) => {
                // Non-JSON / unparseable: treat as billable to avoid bypass.
                billable_flag = true;
                lockable_tool = true;
            }
        }
    }

    let auth_token_id = if state.dev_open_admin {
        Some("dev".to_string())
    } else {
        token
            .strip_prefix("th-")
            .and_then(|rest| rest.split_once('-').map(|(id, _)| id))
            .map(|s| s.to_string())
    };

    let proxy_request = ProxyRequest {
        method: method.clone(),
        path: path.clone(),
        query: query.clone(),
        headers,
        body: forwarded_body.clone(),
        auth_token_id,
    };

    let token_id = if state.dev_open_admin {
        Some("dev".to_string())
    } else {
        token
            .strip_prefix("th-")
            .and_then(|rest| rest.split('-').next())
            .map(|s| s.to_string())
    };

    // Serialize per-token billable tool calls to keep `peek -> upstream -> charge` consistent.
    let token_billing_guard = if !state.dev_open_admin
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
        if !state.dev_open_admin {
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
                    if !state.dev_open_admin {
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

    match state.proxy.proxy_request(proxy_request).await {
        Ok(resp) => {
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
                                } else if let Some(expected) = expected_search_credits {
                                    credits.max(expected)
                                } else {
                                    credits
                                }
                            }
                            None => {
                                if response_has_error || !response_has_success {
                                    0
                                } else if let Some(expected) = expected_search_credits {
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
                            }
                        }

                        if has_search_mcp_without_id && expected_search_credits_without_id_total > 0 {
                            total = total.saturating_add(expected_search_credits_without_id_total);
                        }

                        total
                    };

                    if credits > 0 {
                        match if let Some(subject) = billing_subject.as_deref() {
                            state
                                .proxy
                                .record_pending_billing_attempt_for_subject_with_kind(
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
                                )
                                .await
                        } else {
                            state
                                .proxy
                                .record_pending_billing_attempt_with_kind(
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
                        .record_token_attempt_with_kind(
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
        Self {
            id: record.id,
            key_id: record.key_id,
            auth_token_id: record.auth_token_id,
            method: record.method,
            path: record.path,
            query: record.query,
            http_status: record.status_code,
            mcp_status: record.tavily_status_code,
            result_status: record.result_status,
            created_at: record.created_at,
            error_message: record.error_message,
            request_body: decode_body(&record.request_body),
            response_body: decode_body(&record.response_body),
            forwarded_headers: record.forwarded_headers,
            dropped_headers: record.dropped_headers,
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
