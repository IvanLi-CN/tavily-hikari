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
            enforce_hourly_any_limit: false,
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

    let token = if let Some(t) = header_token {
        t
    } else if state.dev_open_admin {
        "th-dev-override".to_string()
    } else {
        let resp = Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(CONTENT_TYPE, "application/json; charset=utf-8")
            .body(Body::from("{\"error\":\"missing token\"}"))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok(resp);
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
        let resp = Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(CONTENT_TYPE, "application/json; charset=utf-8")
            .body(Body::from("{\"error\":\"invalid or disabled token\"}"))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok(resp);
    }

    let auth_token_id = if state.dev_open_admin {
        Some("dev".to_string())
    } else {
        token
            .strip_prefix("th-")
            .and_then(|rest| rest.split_once('-').map(|(id, _)| id.to_string()))
    };

    if let Some(ref tid) = auth_token_id
        && !state.dev_open_admin
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
                    let payload = json!({
                        "error": "quota_exhausted",
                        "message": "hourly request limit reached for this token",
                    });
                    let resp = Response::builder()
                        .status(StatusCode::TOO_MANY_REQUESTS)
                        .header(CONTENT_TYPE, "application/json; charset=utf-8")
                        .body(Body::from(payload.to_string()))
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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

    if !state.dev_open_admin {
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

    if let Some(ref tid) = auth_token_id
        && !state.dev_open_admin
    {
        match state.proxy.check_token_quota(tid).await {
            Ok(verdict) => {
                if !verdict.allowed {
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
                            Some("daily / hourly limit reached for this token"),
                        )
                        .await;
                    let payload = json!({
                        "error": "quota_exhausted",
                        "message": "daily / hourly limit reached for this token",
                    });
                    let resp = Response::builder()
                        .status(StatusCode::TOO_MANY_REQUESTS)
                        .header(CONTENT_TYPE, "application/json; charset=utf-8")
                        .body(Body::from(payload.to_string()))
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                    return Ok(resp);
                }
            }
            Err(err) => {
                eprintln!("quota check failed for {path}: {err}");
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
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }

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
                    .record_token_attempt(
                        tid,
                        &method,
                        &path,
                        None,
                        Some(http_code),
                        analysis.tavily_status_code,
                        true,
                        analysis.status,
                        None,
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
                        true,
                        "error",
                        Some(msg.as_str()),
                    )
                    .await;
            }

            let status = match err {
                ProxyError::Http(_) | ProxyError::NoAvailableKeys => StatusCode::BAD_GATEWAY,
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

    let token = if let Some(t) = header_token {
        t
    } else if let Some(t) = body_token {
        t
    } else if state.dev_open_admin {
        "th-dev-override".to_string()
    } else {
        let resp = Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(CONTENT_TYPE, "application/json; charset=utf-8")
            .body(Body::from("{\"error\":\"missing token\"}"))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok(resp);
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
        let resp = Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(CONTENT_TYPE, "application/json; charset=utf-8")
            .body(Body::from("{\"error\":\"invalid or disabled token\"}"))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok(resp);
    }

    let auth_token_id = if state.dev_open_admin {
        Some("dev".to_string())
    } else {
        token
            .strip_prefix("th-")
            .and_then(|rest| rest.split_once('-').map(|(id, _)| id.to_string()))
    };

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

    if config.enforce_hourly_any_limit
        && let Some(ref tid) = auth_token_id
        && !state.dev_open_admin
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
                    let payload = json!({
                        "error": "quota_exhausted",
                        "message": "hourly request limit reached for this token",
                    });
                    let resp = Response::builder()
                        .status(StatusCode::TOO_MANY_REQUESTS)
                        .header(CONTENT_TYPE, "application/json; charset=utf-8")
                        .body(Body::from(payload.to_string()))
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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

    if let Some(ref tid) = auth_token_id {
        match state.proxy.check_token_quota(tid).await {
            Ok(verdict) => {
                if !state.dev_open_admin && !verdict.allowed {
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
                            Some("daily / hourly limit reached for this token"),
                        )
                        .await;
                    let payload = json!({
                        "error": "quota_exhausted",
                        "message": "daily / hourly limit reached for this token",
                    });
                    let resp = Response::builder()
                        .status(StatusCode::TOO_MANY_REQUESTS)
                        .header(CONTENT_TYPE, "application/json; charset=utf-8")
                        .body(Body::from(payload.to_string()))
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                    return Ok(resp);
                }
            }
            Err(err) => {
                eprintln!("quota check failed for {path}: {err}");
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

    let result = match config.mode {
        TavilyUpstreamMode::Search => {
            state
                .proxy
                .proxy_http_search(
                    &state.usage_base,
                    auth_token_id.as_deref(),
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
            if let Some(tid) = token_id_for_logs.as_deref() {
                let http_code = resp.status.as_u16() as i64;
                let _ = state
                    .proxy
                    .record_token_attempt(
                        tid,
                        &method,
                        &path,
                        None,
                        Some(http_code),
                        analysis.tavily_status_code,
                        true,
                        analysis.status,
                        None,
                    )
                    .await;
            }
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
                ProxyError::Http(_) | ProxyError::NoAvailableKeys => StatusCode::BAD_GATEWAY,
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
