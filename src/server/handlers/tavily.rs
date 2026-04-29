#[derive(Clone, Copy)]
enum TavilyUpstreamMode {
    Search,
    Json,
}

#[derive(Clone, Copy)]
struct TavilyEndpointConfig {
    upstream_path: &'static str,
    mode: TavilyUpstreamMode,
    enforce_hourly_any_limit: bool,
    validate_max_results: bool,
}

impl TavilyEndpointConfig {
    const fn search() -> Self {
        Self {
            upstream_path: "/search",
            mode: TavilyUpstreamMode::Search,
            enforce_hourly_any_limit: true,
            validate_max_results: true,
        }
    }

    const fn extract() -> Self {
        Self {
            upstream_path: "/extract",
            mode: TavilyUpstreamMode::Json,
            enforce_hourly_any_limit: true,
            validate_max_results: false,
        }
    }

    const fn crawl() -> Self {
        Self {
            upstream_path: "/crawl",
            mode: TavilyUpstreamMode::Json,
            enforce_hourly_any_limit: true,
            validate_max_results: false,
        }
    }

    const fn map() -> Self {
        Self {
            upstream_path: "/map",
            mode: TavilyUpstreamMode::Json,
            enforce_hourly_any_limit: true,
            validate_max_results: false,
        }
    }

    const fn research() -> Self {
        Self {
            upstream_path: "/research",
            mode: TavilyUpstreamMode::Json,
            enforce_hourly_any_limit: true,
            validate_max_results: false,
        }
    }
}

fn tavily_search_expected_credits(options: &Value) -> i64 {
    let depth = options
        .get("search_depth")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if depth == "advanced" {
        2
    } else {
        // basic / fast / ultra-fast / unknown defaults to the low-cost tier
        1
    }
}

fn parse_positive_credit_count(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|v| i64::try_from(v).ok())),
        Value::String(raw) => raw.trim().parse::<f64>().ok().and_then(|parsed| {
            if parsed.is_finite() {
                Some(parsed.ceil() as i64)
            } else {
                None
            }
        }),
        _ => None,
    }
    .filter(|value| *value > 0)
}

fn non_empty_str(value: &Value) -> Option<&str> {
    value.as_str().map(str::trim).filter(|value| !value.is_empty())
}

fn extract_http_project_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-project-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn chunked_credits(items: usize, chunk_size: usize, credits_per_chunk: i64) -> i64 {
    if items == 0 || credits_per_chunk <= 0 {
        return 0;
    }
    let items = i64::try_from(items).unwrap_or(i64::MAX);
    let chunk_size = i64::try_from(chunk_size).unwrap_or(1).max(1);
    ((items + chunk_size - 1) / chunk_size).saturating_mul(credits_per_chunk)
}

fn tavily_extract_depth_credits(options: &Value) -> i64 {
    let depth = options
        .get("extract_depth")
        .or_else(|| options.get("depth"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if depth == "advanced" {
        2
    } else {
        1
    }
}

fn tavily_requested_url_count(options: &Value) -> usize {
    let from_urls = options.get("urls").and_then(|value| match value {
        Value::Array(items) => Some(items.iter().filter(|item| non_empty_str(item).is_some()).count()),
        Value::String(raw) => Some(usize::from(!raw.trim().is_empty())),
        _ => None,
    });
    if let Some(count) = from_urls
        && count > 0
    {
        return count;
    }
    if options.get("url").and_then(non_empty_str).is_some() {
        return 1;
    }
    1
}

fn tavily_requested_limit(options: &Value, default: i64) -> i64 {
    options
        .get("limit")
        .and_then(parse_positive_credit_count)
        .unwrap_or(default)
}

fn tavily_has_instructions(options: &Value) -> bool {
    options
        .get("instructions")
        .and_then(non_empty_str)
        .is_some()
}

fn tavily_extract_expected_credits(options: &Value) -> i64 {
    chunked_credits(
        tavily_requested_url_count(options),
        5,
        tavily_extract_depth_credits(options),
    )
}

fn tavily_map_expected_credits(options: &Value) -> i64 {
    let limit = tavily_requested_limit(options, 50);
    let pages = usize::try_from(limit).unwrap_or(usize::MAX);
    let per_chunk = if tavily_has_instructions(options) { 2 } else { 1 };
    chunked_credits(pages, 10, per_chunk)
}

fn tavily_crawl_expected_credits(options: &Value) -> i64 {
    let limit = tavily_requested_limit(options, 50);
    let pages = usize::try_from(limit).unwrap_or(usize::MAX);
    let mapping_credits = {
        let per_chunk = if tavily_has_instructions(options) { 2 } else { 1 };
        chunked_credits(pages, 10, per_chunk)
    };
    let extract_credits = chunked_credits(pages, 5, tavily_extract_depth_credits(options));
    mapping_credits.saturating_add(extract_credits)
}

fn tavily_research_model_validation_message(options: &Value) -> Option<&'static str> {
    match options.get("model") {
        None => None,
        Some(Value::String(model)) => {
            let model = model.trim().to_ascii_lowercase();
            if matches!(model.as_str(), "mini" | "auto" | "pro" | "") {
                None
            } else {
                Some("model must be one of mini, auto, or pro")
            }
        }
        Some(_) => Some("model must be a string"),
    }
}

fn tavily_research_estimated_credits(options: &Value) -> Option<i64> {
    if tavily_research_model_validation_message(options).is_some() {
        return None;
    }

    let model = options
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match model.as_str() {
        "mini" => Some(40),
        "pro" => Some(100),
        // Tavily defaults missing model to auto; shared upstream keys make per-request
        // research usage deltas unsafe to attribute, so auto gets its own estimate.
        "auto" | "" => Some(50),
        _ => None,
    }
}

fn tavily_http_reserved_credits(upstream_path: &str, options: &Value) -> i64 {
    match upstream_path {
        "/search" => tavily_search_expected_credits(options),
        "/extract" => tavily_extract_expected_credits(options),
        "/crawl" => tavily_crawl_expected_credits(options),
        "/map" => tavily_map_expected_credits(options),
        "/research" => tavily_research_estimated_credits(options).unwrap_or(0),
        _ => 1,
    }
}

fn tavily_mcp_reserved_credits(tool: &str, options: &Value) -> i64 {
    match tool {
        "tavily-search" => tavily_search_expected_credits(options),
        "tavily-extract" => tavily_extract_expected_credits(options),
        "tavily-crawl" => tavily_crawl_expected_credits(options),
        "tavily-map" => tavily_map_expected_credits(options),
        "tavily-research" => tavily_research_estimated_credits(options).unwrap_or(0),
        _ => 1,
    }
}

fn quota_exhausted_now(verdict: &TokenQuotaVerdict) -> bool {
    verdict.hourly_used >= verdict.hourly_limit
        || verdict.daily_used >= verdict.daily_limit
        || verdict.monthly_used >= verdict.monthly_limit
}

fn quota_would_exceed(verdict: &TokenQuotaVerdict, delta: i64) -> bool {
    if delta <= 0 {
        return false;
    }
    verdict.hourly_used.saturating_add(delta) > verdict.hourly_limit
        || verdict.daily_used.saturating_add(delta) > verdict.daily_limit
        || verdict.monthly_used.saturating_add(delta) > verdict.monthly_limit
}

#[axum::debug_handler]
async fn tavily_http_search(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    proxy_tavily_http_endpoint(state, req, TavilyEndpointConfig::search()).await
}

async fn tavily_http_extract(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    proxy_tavily_http_endpoint(state, req, TavilyEndpointConfig::extract()).await
}

async fn tavily_http_crawl(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    proxy_tavily_http_endpoint(state, req, TavilyEndpointConfig::crawl()).await
}

async fn tavily_http_map(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    proxy_tavily_http_endpoint(state, req, TavilyEndpointConfig::map()).await
}

async fn tavily_http_research(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    proxy_tavily_http_endpoint(state, req, TavilyEndpointConfig::research()).await
}

async fn tavily_http_research_result(
    State(state): State<Arc<AppState>>,
    Path(request_id): Path<String>,
    req: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    let (parts, _body) = req.into_parts();
    let method = parts.method.clone();
    let path = format!("/api/tavily/research/{request_id}");

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
        .map(ToOwned::to_owned);

    let Some(token_resolution) = resolve_request_token(state.dev_open_admin, vec![header_token])
    else {
        let resp = Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(CONTENT_TYPE, "application/json; charset=utf-8")
            .body(Body::from("{\"error\":\"missing token\"}"))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok(resp);
    };
    let token = token_resolution.token;
    let auth_token_id = token_resolution.auth_token_id;
    let using_dev_open_admin_fallback = token_resolution.using_dev_open_admin_fallback;

    let valid = if using_dev_open_admin_fallback {
        true
    } else {
        state
            .proxy
            .validate_access_token(&token)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };
    if !valid {
        let resp = Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(CONTENT_TYPE, "application/json; charset=utf-8")
            .body(Body::from("{\"error\":\"invalid or disabled token\"}"))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok(resp);
    }

    if let Some(ref tid) = auth_token_id
        && !using_dev_open_admin_fallback
    {
        match state.proxy.check_token_hourly_requests(tid).await {
            Ok(verdict) => {
                if !verdict.allowed {
                    let message = build_request_limit_error_message(&verdict);
                    let _ = state
                        .proxy
                        .record_token_attempt(
                            tid,
                            &method,
                            &path,
                            None,
                            Some(StatusCode::TOO_MANY_REQUESTS.as_u16() as i64),
                            None,
                            false,
                            "quota_exhausted",
                            Some(&message),
                        )
                        .await;
                    let resp = request_limit_exceeded_response(&verdict)?;
                    return Ok(resp);
                }
            }
            Err(err) => {
                eprintln!("hourly request limit check failed for {path}: {err}");
                if let Some(tid) = auth_token_id.as_deref() {
                    let msg = err.to_string();
                    let _ = state
                        .proxy
                        .record_token_attempt(
                            tid,
                            &method,
                            &path,
                            None,
                            Some(StatusCode::INTERNAL_SERVER_ERROR.as_u16() as i64),
                            None,
                            false,
                            "error",
                            Some(msg.as_str()),
                        )
                        .await;
                }
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }

    if !using_dev_open_admin_fallback {
        match state
            .proxy
            .is_research_request_owned_by(&request_id, auth_token_id.as_deref())
            .await
        {
            Ok(true) => {}
            Ok(false) => {
                if let Some(tid) = auth_token_id.as_deref() {
                    let _ = state
                        .proxy
                        .record_token_attempt(
                            tid,
                            &method,
                            &path,
                            None,
                            Some(StatusCode::NOT_FOUND.as_u16() as i64),
                            Some(StatusCode::NOT_FOUND.as_u16() as i64),
                            false,
                            "error",
                            Some("research request not found"),
                        )
                        .await;
                }
                let payload = json!({
                    "error": "research_request_not_found",
                    "message": "research request not found",
                });
                let resp = Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .header(CONTENT_TYPE, "application/json; charset=utf-8")
                    .body(Body::from(payload.to_string()))
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                return Ok(resp);
            }
            Err(err) => {
                eprintln!("research request owner check failed for {path}: {err}");
                if let Some(tid) = auth_token_id.as_deref() {
                    let msg = err.to_string();
                    let _ = state
                        .proxy
                        .record_token_attempt(
                            tid,
                            &method,
                            &path,
                            None,
                            Some(StatusCode::INTERNAL_SERVER_ERROR.as_u16() as i64),
                            None,
                            false,
                            "error",
                            Some(msg.as_str()),
                        )
                        .await;
                }
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }

    // NOTE: `GET /api/tavily/research/:request_id` is a *result retrieval* endpoint.
    // Billing is charged on `POST /api/tavily/research` using model-based estimates, so this
    // endpoint must not consume business quota nor block due to exhausted credits quota.

    let mut headers = clone_headers(&parts.headers);
    headers.remove(axum::http::header::AUTHORIZATION);
    let upstream_path = format!("/research/{}", urlencoding::encode(&request_id));
    let token_id_for_logs = auth_token_id.clone();

    let result = state
        .proxy
        .proxy_http_get_endpoint(
            &state.usage_base,
            &upstream_path,
            auth_token_id.as_deref(),
            &method,
            &path,
            &headers,
            true,
        )
        .await;

    match result {
        Ok((resp, analysis)) => {
            if let Some(tid) = token_id_for_logs.as_deref() {
                let http_code = resp.status.as_u16() as i64;
                let _ = state
                    .proxy
                    .record_token_attempt_request_log_metadata(
                        tid,
                        &method,
                        &path,
                        None,
                        Some(http_code),
                        analysis.tavily_status_code,
                        false,
                        analysis.status,
                        None,
                        analysis.failure_kind.as_deref(),
                        Some(resp.key_effect_code.as_str()),
                        resp.key_effect_summary.as_deref(),
                        Some(resp.binding_effect_code.as_str()),
                        resp.binding_effect_summary.as_deref(),
                        Some(resp.selection_effect_code.as_str()),
                        resp.selection_effect_summary.as_deref(),
                        resp.request_log_id,
                    )
                    .await;
            }
            Ok(build_response(resp))
        }
        Err(err) => {
            eprintln!("tavily http /research/{request_id} proxy error: {err}");
            if let Some(tid) = token_id_for_logs.as_deref() {
                let msg = err.to_string();
                let _ = state
                    .proxy
                    .record_token_attempt(
                        tid,
                        &method,
                        &path,
                        None,
                        None,
                        None,
                        false,
                        "error",
                        Some(msg.as_str()),
                    )
                    .await;
            }

            let status = match err {
                ProxyError::Http(_)
                | ProxyError::NoAvailableKeys
                | ProxyError::PinnedMcpSessionUnavailable => StatusCode::BAD_GATEWAY,
                ProxyError::Database(_)
                | ProxyError::InvalidEndpoint { .. }
                | ProxyError::QuotaDataMissing { .. }
                | ProxyError::UsageHttp { .. }
                | ProxyError::Other(_) => StatusCode::INTERNAL_SERVER_ERROR,
            };

            let payload = json!({
                "error": "proxy_error",
                "message": "upstream unavailable",
            });
            let resp = Response::builder()
                .status(status)
                .header(CONTENT_TYPE, "application/json; charset=utf-8")
                .body(Body::from(payload.to_string()))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            Ok(resp)
        }
    }
}

async fn proxy_tavily_http_endpoint(
    state: Arc<AppState>,
    req: Request<Body>,
    config: TavilyEndpointConfig,
) -> Result<Response<Body>, StatusCode> {
    let (parts, body) = req.into_parts();
    let method = parts.method.clone();
    let path = parts.uri.path().to_owned();

    let body_bytes = body::to_bytes(body, BODY_LIMIT)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let mut options: Value =
        serde_json::from_slice(&body_bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
    if !options.is_object() {
        return Err(StatusCode::BAD_REQUEST);
    }

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
        .map(ToOwned::to_owned);

    let body_token = options
        .get("api_key")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned);

    let Some(token_resolution) =
        resolve_request_token(state.dev_open_admin, vec![header_token, body_token])
    else {
        let resp = Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(CONTENT_TYPE, "application/json; charset=utf-8")
            .body(Body::from("{\"error\":\"missing token\"}"))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok(resp);
    };
    let token = token_resolution.token;
    let auth_token_id = token_resolution.auth_token_id;
    let using_dev_open_admin_fallback = token_resolution.using_dev_open_admin_fallback;

    let valid = if using_dev_open_admin_fallback {
        true
    } else {
        state
            .proxy
            .validate_access_token(&token)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };
    if !valid {
        let resp = Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(CONTENT_TYPE, "application/json; charset=utf-8")
            .body(Body::from("{\"error\":\"invalid or disabled token\"}"))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok(resp);
    }

    if let Value::Object(ref mut map) = options {
        map.remove("api_key");
    }

    if config.validate_max_results
        && let Value::Object(ref map) = options
        && let Some(val) = map.get("max_results").and_then(|v| v.as_i64())
        && val < 0
    {
        let payload = json!({
            "error": "invalid_request",
            "message": "max_results must be non-negative",
        });
        let resp = Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header(CONTENT_TYPE, "application/json; charset=utf-8")
            .body(Body::from(payload.to_string()))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok(resp);
    }

    let token_id_for_logs = auth_token_id.clone();
    let http_project_id =
        (!using_dev_open_admin_fallback).then(|| extract_http_project_id(&parts.headers)).flatten();
    let expected_search_credits = (config.upstream_path == "/search").then(|| {
        // Search billing is predictable based on `search_depth`.
        tavily_search_expected_credits(&options)
    });
    let reserved_credits = tavily_http_reserved_credits(config.upstream_path, &options);

    // Serialize billable requests per quota subject so `peek -> upstream -> charge` stays
    // consistent across local concurrency and other instances sharing the same SQLite database.
    let token_billing_guard = if !using_dev_open_admin_fallback {
        if let Some(tid) = auth_token_id.as_deref() {
            Some(
                state
                    .proxy
                    .lock_token_billing(tid)
                    .await
                    .map_err(|err| {
                        eprintln!("token billing lock failed for {path}: {err}");
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?,
            )
        } else {
            None
        }
    } else {
        None
    };
    let billing_subject = token_billing_guard
        .as_ref()
        .map(|guard| guard.billing_subject().to_string());
    if let Some(guard) = token_billing_guard.as_ref() {
        guard.ensure_live().map_err(|err| {
            eprintln!("token billing lock lost before precheck for {path}: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    if config.enforce_hourly_any_limit
        && let Some(ref tid) = auth_token_id
        && !using_dev_open_admin_fallback
    {
        match state.proxy.check_token_hourly_requests(tid).await {
            Ok(verdict) => {
                if !verdict.allowed {
                    let message = build_request_limit_error_message(&verdict);
                    let _ = state
                        .proxy
                        .record_token_attempt(
                            tid,
                            &method,
                            &path,
                            None,
                            Some(StatusCode::TOO_MANY_REQUESTS.as_u16() as i64),
                            None,
                            false,
                            "quota_exhausted",
                            Some(&message),
                        )
                        .await;
                    let resp = request_limit_exceeded_response(&verdict)?;
                    return Ok(resp);
                }
            }
            Err(err) => {
                eprintln!("hourly request limit check failed for {path}: {err}");
                if let Some(tid) = auth_token_id.as_deref() {
                    let msg = err.to_string();
                    let _ = state
                        .proxy
                        .record_token_attempt(
                            tid,
                            &method,
                            &path,
                            None,
                            Some(StatusCode::INTERNAL_SERVER_ERROR.as_u16() as i64),
                            None,
                            false,
                            "error",
                            Some(msg.as_str()),
                        )
                        .await;
                }
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }

    if config.upstream_path == "/research"
        && let Some(message) = tavily_research_model_validation_message(&options)
    {
        if let Some(tid) = auth_token_id.as_deref() {
            let _ = state
                .proxy
                .record_token_attempt(
                    tid,
                    &method,
                    &path,
                    None,
                    Some(StatusCode::BAD_REQUEST.as_u16() as i64),
                    Some(StatusCode::BAD_REQUEST.as_u16() as i64),
                    false,
                    "error",
                    Some(message),
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

    if let Some(ref tid) = auth_token_id {
        match if let Some(subject) = billing_subject.as_deref() {
            state.proxy.peek_token_quota_for_subject(subject).await
        } else {
            state.proxy.peek_token_quota(tid).await
        } {
            Ok(verdict) => {
                if !using_dev_open_admin_fallback {
                    let blocked = quota_would_exceed(&verdict, reserved_credits);

                    if blocked {
                        let _ = state
                            .proxy
                            .record_token_attempt(
                                tid,
                                &method,
                                &path,
                                None,
                                Some(StatusCode::TOO_MANY_REQUESTS.as_u16() as i64),
                                None,
                                true,
                                "quota_exhausted",
                                Some("credits quota exhausted for this token"),
                            )
                            .await;
                        let payload = json!({
                            "error": "quota_exhausted",
                            "message": "quota exhausted for this token",
                        });
                        let resp = Response::builder()
                            .status(StatusCode::TOO_MANY_REQUESTS)
                            .header(CONTENT_TYPE, "application/json; charset=utf-8")
                            .body(Body::from(payload.to_string()))
                            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                        return Ok(resp);
                    }
                }
            }
            Err(err) => {
                eprintln!("quota peek failed for {path}: {err}");
                if let Some(tid) = auth_token_id.as_deref() {
                    let msg = err.to_string();
                    let _ = state
                        .proxy
                        .record_token_attempt(
                            tid,
                            &method,
                            &path,
                            None,
                            Some(StatusCode::INTERNAL_SERVER_ERROR.as_u16() as i64),
                            None,
                            true,
                            "error",
                            Some(msg.as_str()),
                        )
                        .await;
                }
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }

    let mut headers = clone_headers(&parts.headers);
    headers.remove(axum::http::header::AUTHORIZATION);

    if config.upstream_path == "/research" {
        let result = state
            .proxy
            .proxy_http_research(
                &state.usage_base,
                auth_token_id.as_deref(),
                http_project_id.as_deref(),
                &method,
                &path,
                options,
                &headers,
                true,
            )
            .await;

        match result {
            Ok((resp, analysis, usage_delta)) => {
                let mut billing_error: Option<String> = None;
                let mut attempt_logged = false;

                if resp.status.is_success()
                    && analysis.status == "success"
                    && let Some(tid) = token_id_for_logs.as_deref()
                {
                    let credits = usage_delta.unwrap_or(reserved_credits);
                    if credits > 0 {
                        match if let Some(subject) = billing_subject.as_deref() {
                            state
                                .proxy
                                .record_pending_billing_attempt_for_subject_request_log_metadata(
                                    tid,
                                    &method,
                                    &path,
                                    None,
                                    Some(resp.status.as_u16() as i64),
                                    analysis.tavily_status_code,
                                    true,
                                    analysis.status,
                                    None,
                                    credits,
                                    subject,
                                    analysis.api_key_id.as_deref(),
                                    analysis.failure_kind.as_deref(),
                                    Some(resp.key_effect_code.as_str()),
                                    resp.key_effect_summary.as_deref(),
                                    Some(resp.binding_effect_code.as_str()),
                                    resp.binding_effect_summary.as_deref(),
                                    Some(resp.selection_effect_code.as_str()),
                                    resp.selection_effect_summary.as_deref(),
                                    resp.request_log_id,
                                )
                                .await
                        } else {
                            state
                                .proxy
                                .record_pending_billing_attempt_request_log_metadata(
                                    tid,
                                    &method,
                                    &path,
                                    None,
                                    Some(resp.status.as_u16() as i64),
                                    analysis.tavily_status_code,
                                    true,
                                    analysis.status,
                                    None,
                                    credits,
                                    analysis.api_key_id.as_deref(),
                                    analysis.failure_kind.as_deref(),
                                    Some(resp.key_effect_code.as_str()),
                                    resp.key_effect_summary.as_deref(),
                                    Some(resp.binding_effect_code.as_str()),
                                    resp.binding_effect_summary.as_deref(),
                                    Some(resp.selection_effect_code.as_str()),
                                    resp.selection_effect_summary.as_deref(),
                                    resp.request_log_id,
                                )
                                .await
                        }
                        {
                            Ok(log_id) => {
                                attempt_logged = true;
                                if let Some(msg) = billing_error.as_deref() {
                                    let _ = state
                                        .proxy
                                        .annotate_pending_billing_attempt(log_id, msg)
                                        .await;
                                }
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

                if !attempt_logged
                    && let Some(tid) = token_id_for_logs.as_deref()
                {
                    let http_code = resp.status.as_u16() as i64;
                    let _ = state
                        .proxy
                        .record_token_attempt_request_log_metadata(
                            tid,
                            &method,
                            &path,
                            None,
                            Some(http_code),
                            analysis.tavily_status_code,
                            true,
                            analysis.status,
                            billing_error.as_deref(),
                            analysis.failure_kind.as_deref(),
                            Some(resp.key_effect_code.as_str()),
                            resp.key_effect_summary.as_deref(),
                            Some(resp.binding_effect_code.as_str()),
                            resp.binding_effect_summary.as_deref(),
                            Some(resp.selection_effect_code.as_str()),
                            resp.selection_effect_summary.as_deref(),
                            resp.request_log_id,
                        )
                        .await;
                }
                // Return the upstream response once billing either succeeded or we captured a local audit error.
                return Ok(build_response(resp));
            }
            Err(err) => {
                eprintln!("tavily http {} proxy error: {err}", config.upstream_path);
                if let Some(tid) = token_id_for_logs.as_deref() {
                    let msg = err.to_string();
                    let _ = state
                        .proxy
                        .record_token_attempt(
                            tid,
                            &method,
                            &path,
                            None,
                            None,
                            None,
                            false,
                            "error",
                            Some(msg.as_str()),
                        )
                        .await;
                }

                let status = match err {
                    ProxyError::Http(_)
                    | ProxyError::NoAvailableKeys
                    | ProxyError::PinnedMcpSessionUnavailable
                    | ProxyError::QuotaDataMissing { .. }
                    | ProxyError::UsageHttp { .. } => StatusCode::BAD_GATEWAY,
                    ProxyError::Database(_)
                    | ProxyError::InvalidEndpoint { .. }
                    | ProxyError::Other(_) => StatusCode::INTERNAL_SERVER_ERROR,
                };

                let payload = json!({
                    "error": "proxy_error",
                    "message": "upstream unavailable",
                });
                let resp = Response::builder()
                    .status(status)
                    .header(CONTENT_TYPE, "application/json; charset=utf-8")
                    .body(Body::from(payload.to_string()))
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                return Ok(resp);
            }
        }
    }

    let result = match config.mode {
        TavilyUpstreamMode::Search => {
            state
                .proxy
                .proxy_http_search(
                    &state.usage_base,
                    auth_token_id.as_deref(),
                    http_project_id.as_deref(),
                    &method,
                    &path,
                    options,
                    &headers,
                )
                .await
        }
        TavilyUpstreamMode::Json => {
            state
                .proxy
                .proxy_http_json_endpoint(
                    &state.usage_base,
                    config.upstream_path,
                    auth_token_id.as_deref(),
                    http_project_id.as_deref(),
                    &method,
                    &path,
                    options,
                    &headers,
                    true,
                )
                .await
        }
    };

    match result {
        Ok((resp, analysis)) => {
            let mut billing_error: Option<String> = None;
            let mut attempt_logged = false;
            if resp.status.is_success()
                && analysis.status == "success"
                && let Some(tid) = token_id_for_logs.as_deref()
            {
                let credits = if config.upstream_path == "/search" {
                    extract_usage_credits_from_json_bytes(&resp.body)
                        .unwrap_or_else(|| expected_search_credits.unwrap_or(1))
                } else {
                    match extract_usage_credits_from_json_bytes(&resp.body) {
                        Some(credits) => credits,
                        None => {
                            eprintln!(
                                "missing usage.credits for {} response; skipping billing",
                                config.upstream_path
                            );
                            0
                        }
                    }
                };
                if credits > 0 {
                    match if let Some(subject) = billing_subject.as_deref() {
                        state
                            .proxy
                            .record_pending_billing_attempt_for_subject_request_log_metadata(
                                tid,
                                &method,
                                &path,
                                None,
                                Some(resp.status.as_u16() as i64),
                                analysis.tavily_status_code,
                                true,
                                analysis.status,
                                None,
                                credits,
                                subject,
                                analysis.api_key_id.as_deref(),
                                analysis.failure_kind.as_deref(),
                                Some(resp.key_effect_code.as_str()),
                                resp.key_effect_summary.as_deref(),
                                Some(resp.binding_effect_code.as_str()),
                                resp.binding_effect_summary.as_deref(),
                                Some(resp.selection_effect_code.as_str()),
                                resp.selection_effect_summary.as_deref(),
                                resp.request_log_id,
                            )
                            .await
                    } else {
                        state
                            .proxy
                            .record_pending_billing_attempt_request_log_metadata(
                                tid,
                                &method,
                                &path,
                                None,
                                Some(resp.status.as_u16() as i64),
                                analysis.tavily_status_code,
                                true,
                                analysis.status,
                                None,
                                credits,
                                analysis.api_key_id.as_deref(),
                                analysis.failure_kind.as_deref(),
                                Some(resp.key_effect_code.as_str()),
                                resp.key_effect_summary.as_deref(),
                                Some(resp.binding_effect_code.as_str()),
                                resp.binding_effect_summary.as_deref(),
                                Some(resp.selection_effect_code.as_str()),
                                resp.selection_effect_summary.as_deref(),
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

            if !attempt_logged
                && let Some(tid) = token_id_for_logs.as_deref()
            {
                let http_code = resp.status.as_u16() as i64;
                let _ = state
                    .proxy
                    .record_token_attempt_request_log_metadata(
                        tid,
                        &method,
                        &path,
                        None,
                        Some(http_code),
                        analysis.tavily_status_code,
                        true,
                        analysis.status,
                        billing_error.as_deref(),
                        analysis.failure_kind.as_deref(),
                        Some(resp.key_effect_code.as_str()),
                        resp.key_effect_summary.as_deref(),
                        Some(resp.binding_effect_code.as_str()),
                        resp.binding_effect_summary.as_deref(),
                        Some(resp.selection_effect_code.as_str()),
                        resp.selection_effect_summary.as_deref(),
                        resp.request_log_id,
                    )
                    .await;
            }
            // Always return the upstream response, even if local billing persistence fails.
            // Returning a 5xx here can trigger client retries and cause duplicate upstream charges.
            Ok(build_response(resp))
        }
        Err(err) => {
            eprintln!("tavily http {} proxy error: {err}", config.upstream_path);
            if let Some(tid) = token_id_for_logs.as_deref() {
                let msg = err.to_string();
                let _ = state
                    .proxy
                    .record_token_attempt(
                        tid,
                        &method,
                        &path,
                        None,
                        None,
                        None,
                        true,
                        "error",
                        Some(msg.as_str()),
                    )
                    .await;
            }

            let status = match err {
                ProxyError::Http(_)
                | ProxyError::NoAvailableKeys
                | ProxyError::PinnedMcpSessionUnavailable => StatusCode::BAD_GATEWAY,
                ProxyError::Database(_)
                | ProxyError::InvalidEndpoint { .. }
                | ProxyError::QuotaDataMissing { .. }
                | ProxyError::UsageHttp { .. }
                | ProxyError::Other(_) => StatusCode::INTERNAL_SERVER_ERROR,
            };

            let payload = json!({
                "error": "proxy_error",
                "message": "upstream unavailable",
            });
            let resp = Response::builder()
                .status(status)
                .header(CONTENT_TYPE, "application/json; charset=utf-8")
                .body(Body::from(payload.to_string()))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            Ok(resp)
        }
    }
}
