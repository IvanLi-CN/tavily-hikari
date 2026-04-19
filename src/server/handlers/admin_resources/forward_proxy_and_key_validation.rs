async fn get_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<SettingsResponse>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let forward_proxy = state.proxy.get_forward_proxy_settings().await.map_err(|err| {
        eprintln!("get settings error: {err}");
        (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
    })?;
    let system_settings = state.proxy.get_system_settings().await.map_err(|err| {
        eprintln!("get system settings error: {err}");
        (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
    })?;
    Ok(Json(SettingsResponse {
        forward_proxy: Some(forward_proxy),
        system_settings,
    }))
}

async fn put_system_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<SystemSettingsUpdatePayload>,
) -> Result<Json<tavily_hikari::SystemSettings>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }

    let current_settings = state.proxy.get_system_settings().await.map_err(|err| {
        eprintln!("get current system settings error: {err}");
        (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
    })?;

    state
        .proxy
        .set_system_settings(&tavily_hikari::SystemSettings {
            request_rate_limit: payload
                .request_rate_limit
                .unwrap_or(current_settings.request_rate_limit),
            mcp_session_affinity_key_count: payload.mcp_session_affinity_key_count,
            rebalance_mcp_enabled: payload.rebalance_mcp_enabled,
            rebalance_mcp_session_percent: payload.rebalance_mcp_session_percent,
        })
        .await
        .map(Json)
        .map_err(|err| {
            eprintln!("update system settings error: {err}");
            let message = err.to_string();
            if message.contains("request_rate_limit must be at least")
                || message.contains("mcp_session_affinity_key_count must be between")
                || message.contains("rebalance_mcp_session_percent must be between")
            {
                (StatusCode::BAD_REQUEST, message)
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, message)
            }
        })
}

async fn put_forward_proxy_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ForwardProxySettingsUpdatePayload>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let settings = tavily_hikari::ForwardProxySettings {
        proxy_urls: payload.proxy_urls,
        subscription_urls: payload.subscription_urls,
        subscription_update_interval_secs: payload.subscription_update_interval_secs,
        insert_direct: payload.insert_direct,
        egress_socks5_enabled: payload.egress_socks5_enabled,
        egress_socks5_url: payload.egress_socks5_url,
    }
    .normalized();
    let skip_bootstrap_probe = payload.skip_bootstrap_probe;
    if request_accepts_event_stream(&headers) {
        let state = state.clone();
        let stream = stream! {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<tavily_hikari::ForwardProxyProgressEvent>();
            tokio::spawn(async move {
                let progress_tx = tx.clone();
                let progress = move |event| {
                    let _ = progress_tx.send(event);
                };
                match state
                    .proxy
                    .update_forward_proxy_settings_with_progress(
                        settings,
                        skip_bootstrap_probe,
                        Some(&progress),
                    )
                    .await
                {
                    Ok(response) => {
                        if let Ok(payload) = serde_json::to_value(&response) {
                            let _ = tx.send(tavily_hikari::ForwardProxyProgressEvent::complete(
                                "save",
                                payload,
                            ));
                        } else {
                            let _ = tx.send(tavily_hikari::ForwardProxyProgressEvent::error(
                                "save",
                                "failed to encode forward proxy settings response",
                                None,
                                None,
                                None,
                                None,
                                None,
                            ));
                        }
                    }
                    Err(err) => {
                        eprintln!("update forward proxy settings error: {err}");
                        let _ = tx.send(tavily_hikari::ForwardProxyProgressEvent::error(
                            "save",
                            err.to_string(),
                            None,
                            None,
                            None,
                            None,
                            None,
                        ));
                    }
                }
            });

            while let Some(event) = rx.recv().await {
                match serde_json::to_string(&event) {
                    Ok(json) => yield Ok::<Event, axum::http::Error>(Event::default().data(json)),
                    Err(err) => {
                        yield Ok::<Event, axum::http::Error>(Event::default().data(
                            serde_json::json!({
                                "type": "error",
                                "operation": "save",
                                "message": format!("failed to encode progress event: {err}"),
                            })
                            .to_string(),
                        ));
                        break;
                    }
                }
                if matches!(
                    event,
                    tavily_hikari::ForwardProxyProgressEvent::Complete { .. }
                        | tavily_hikari::ForwardProxyProgressEvent::Error { .. }
                ) {
                    break;
                }
            }
        };

        return Ok(
            Sse::new(stream)
                .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text(""))
                .into_response(),
        );
    }
    state
        .proxy
        .update_forward_proxy_settings(settings, skip_bootstrap_probe)
        .await
        .map(|response| Json(response).into_response())
        .map_err(|err| {
            eprintln!("update forward proxy settings error: {err}");
            (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
        })
}

async fn post_forward_proxy_candidate_validation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ForwardProxyValidationPayload>,
) -> Result<axum::response::Response, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    if request_accepts_event_stream(&headers) {
        let state = state.clone();
        let cancellation = tavily_hikari::ForwardProxyCancellation::default();
        let worker_cancellation = cancellation.clone();
        let stream = stream! {
            let _cancel_guard = ForwardProxyStreamCancelGuard::new(cancellation.clone());
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<tavily_hikari::ForwardProxyProgressEvent>();
            tokio::spawn(async move {
                let progress_tx = tx.clone();
                let progress = move |event| {
                    let _ = progress_tx.send(event);
                };
                let validation = match payload.kind {
                    ForwardProxyValidationKindPayload::ProxyUrl => state
                        .proxy
                        .validate_forward_proxy_candidates_with_progress(
                            vec![payload.value.clone()],
                            Vec::new(),
                            Some(&progress),
                            Some(&worker_cancellation),
                        )
                        .await,
                    ForwardProxyValidationKindPayload::SubscriptionUrl => state
                        .proxy
                        .validate_forward_proxy_candidates_with_progress(
                            Vec::new(),
                            vec![payload.value.clone()],
                            Some(&progress),
                            Some(&worker_cancellation),
                        )
                        .await,
                };

                match validation {
                    Ok(response) => {
                        let view = build_forward_proxy_validation_view(response);
                        if let Ok(payload) = serde_json::to_value(&view) {
                            let _ = tx.send(tavily_hikari::ForwardProxyProgressEvent::complete(
                                "validate",
                                payload,
                            ));
                        } else {
                            let _ = tx.send(tavily_hikari::ForwardProxyProgressEvent::error(
                                "validate",
                                "failed to encode forward proxy validation response",
                                None,
                                None,
                                None,
                                None,
                                None,
                            ));
                        }
                    }
                    Err(err) => {
                        if worker_cancellation.is_cancelled() {
                            return;
                        }
                        eprintln!("validate forward proxy candidate error: {err}");
                        let _ = tx.send(tavily_hikari::ForwardProxyProgressEvent::error(
                            "validate",
                            err.to_string(),
                            None,
                            None,
                            None,
                            None,
                            None,
                        ));
                    }
                }
            });

            while let Some(event) = rx.recv().await {
                match serde_json::to_string(&event) {
                    Ok(json) => yield Ok::<Event, axum::http::Error>(Event::default().data(json)),
                    Err(err) => {
                        yield Ok::<Event, axum::http::Error>(Event::default().data(
                            serde_json::json!({
                                "type": "error",
                                "operation": "validate",
                                "message": format!("failed to encode progress event: {err}"),
                            })
                            .to_string(),
                        ));
                        break;
                    }
                }
                if matches!(
                    event,
                    tavily_hikari::ForwardProxyProgressEvent::Complete { .. }
                        | tavily_hikari::ForwardProxyProgressEvent::Error { .. }
                ) {
                    break;
                }
            }
            cancellation.cancel();
        };

        return Ok(
            Sse::new(stream)
                .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text(""))
                .into_response(),
        );
    }

    let validation = match payload.kind {
        ForwardProxyValidationKindPayload::ProxyUrl => state
            .proxy
            .validate_forward_proxy_candidates(vec![payload.value.clone()], Vec::new())
            .await,
        ForwardProxyValidationKindPayload::SubscriptionUrl => state
            .proxy
            .validate_forward_proxy_candidates(Vec::new(), vec![payload.value.clone()])
            .await,
    }
    .map_err(|err| {
        eprintln!("validate forward proxy candidate error: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(build_forward_proxy_validation_view(validation)).into_response())
}

async fn post_forward_proxy_revalidate(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<axum::response::Response, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }

    if request_accepts_event_stream(&headers) {
        let state = state.clone();
        let stream = stream! {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<tavily_hikari::ForwardProxyProgressEvent>();
            tokio::spawn(async move {
                let progress_tx = tx.clone();
                let progress = move |event| {
                    let _ = progress_tx.send(event);
                };

                match state
                    .proxy
                    .revalidate_forward_proxy_with_progress(Some(&progress))
                    .await
                {
                    Ok(response) => {
                        if let Ok(payload) = serde_json::to_value(&response) {
                            let _ = tx.send(tavily_hikari::ForwardProxyProgressEvent::complete(
                                "revalidate",
                                payload,
                            ));
                        } else {
                            let _ = tx.send(tavily_hikari::ForwardProxyProgressEvent::error(
                                "revalidate",
                                "failed to encode forward proxy settings response",
                                None,
                                None,
                                None,
                                None,
                                None,
                            ));
                        }
                    }
                    Err(err) => {
                        eprintln!("revalidate forward proxy settings error: {err}");
                        let _ = tx.send(tavily_hikari::ForwardProxyProgressEvent::error(
                            "revalidate",
                            err.to_string(),
                            None,
                            None,
                            None,
                            None,
                            None,
                        ));
                    }
                }
            });

            while let Some(event) = rx.recv().await {
                match serde_json::to_string(&event) {
                    Ok(json) => yield Ok::<Event, axum::http::Error>(Event::default().data(json)),
                    Err(err) => {
                        yield Ok::<Event, axum::http::Error>(Event::default().data(
                            serde_json::json!({
                                "type": "error",
                                "operation": "revalidate",
                                "message": format!("failed to encode progress event: {err}"),
                            })
                            .to_string(),
                        ));
                        break;
                    }
                }
                if matches!(
                    event,
                    tavily_hikari::ForwardProxyProgressEvent::Complete { .. }
                        | tavily_hikari::ForwardProxyProgressEvent::Error { .. }
                ) {
                    break;
                }
            }
        };

        return Ok(
            Sse::new(stream)
                .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text(""))
                .into_response(),
        );
    }

    state
        .proxy
        .revalidate_forward_proxy_with_progress(None)
        .await
        .map(|response| Json(response).into_response())
        .map_err(|err| {
            eprintln!("revalidate forward proxy settings error: {err}");
            (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
        })
}

async fn get_forward_proxy_live_stats(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<tavily_hikari::ForwardProxyLiveStatsResponse>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    state
        .proxy
        .get_forward_proxy_live_stats()
        .await
        .map(Json)
        .map_err(|err| {
            eprintln!("get forward proxy live stats error: {err}");
            (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
        })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ForwardProxyDashboardSummaryView {
    available_nodes: i64,
    total_nodes: i64,
}

async fn get_forward_proxy_dashboard_summary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<ForwardProxyDashboardSummaryView>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    state
        .proxy
        .get_forward_proxy_dashboard_summary()
        .await
        .map(|summary| {
            Json(ForwardProxyDashboardSummaryView {
                available_nodes: summary.available_nodes,
                total_nodes: summary.total_nodes,
            })
        })
        .map_err(|err| {
            eprintln!("get forward proxy dashboard summary error: {err}");
            (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
        })
}

fn truncate_detail(mut input: String, max_len: usize) -> String {
    if input.len() <= max_len {
        return input;
    }

    // `String::truncate` requires a UTF-8 char boundary; otherwise it panics.
    if max_len == 0 {
        return String::new();
    }

    let ellipsis = '…';
    let ellipsis_len = ellipsis.len_utf8();
    // Keep the output length <= max_len (including the ellipsis).
    let mut end = if max_len > ellipsis_len {
        max_len - ellipsis_len
    } else {
        max_len
    };
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    input.truncate(end);
    if max_len > ellipsis_len {
        input.push(ellipsis);
    }
    input
}

async fn validate_single_key(
    proxy: TavilyProxy,
    usage_base: String,
    geo_origin: String,
    api_key: String,
    registration_ip: Option<String>,
    registration_region: Option<String>,
) -> (ValidateKeyResult, &'static str) {
    match proxy
        .probe_api_key_quota_with_registration(
            &api_key,
            &usage_base,
            registration_ip.as_deref(),
            registration_region.as_deref(),
            &geo_origin,
        )
        .await
    {
        Ok((limit, remaining, assigned_proxy)) => {
            let assigned_proxy_key = assigned_proxy.as_ref().map(|item| item.key.clone());
            let assigned_proxy_label = assigned_proxy.as_ref().map(|item| item.label.clone());
            let assigned_proxy_match_kind = assigned_proxy.map(|item| item.match_kind);
            if remaining <= 0 {
                (
                    ValidateKeyResult {
                        api_key,
                        status: "ok_exhausted".to_string(),
                        registration_ip,
                        registration_region,
                        assigned_proxy_key,
                        assigned_proxy_label,
                        assigned_proxy_match_kind,
                        quota_limit: Some(limit),
                        quota_remaining: Some(remaining),
                        detail: None,
                    },
                    "exhausted",
                )
            } else {
                (
                    ValidateKeyResult {
                        api_key,
                        status: "ok".to_string(),
                        registration_ip,
                        registration_region,
                        assigned_proxy_key,
                        assigned_proxy_label,
                        assigned_proxy_match_kind,
                        quota_limit: Some(limit),
                        quota_remaining: Some(remaining),
                        detail: None,
                    },
                    "ok",
                )
            }
        }
        Err(ProxyError::UsageHttp { status, body }) => {
            let mut detail = format!("Tavily usage request failed with {status}: {body}");
            detail = truncate_detail(detail, 1400);
            if status == reqwest::StatusCode::UNAUTHORIZED {
                (
                    ValidateKeyResult {
                        api_key,
                        status: "unauthorized".to_string(),
                        registration_ip,
                        registration_region,
                        assigned_proxy_key: None,
                        assigned_proxy_label: None,
                        assigned_proxy_match_kind: None,
                        quota_limit: None,
                        quota_remaining: None,
                        detail: Some(detail),
                    },
                    "invalid",
                )
            } else if status == reqwest::StatusCode::FORBIDDEN {
                (
                    ValidateKeyResult {
                        api_key,
                        status: "forbidden".to_string(),
                        registration_ip,
                        registration_region,
                        assigned_proxy_key: None,
                        assigned_proxy_label: None,
                        assigned_proxy_match_kind: None,
                        quota_limit: None,
                        quota_remaining: None,
                        detail: Some(detail),
                    },
                    "invalid",
                )
            } else if status == reqwest::StatusCode::BAD_REQUEST {
                (
                    ValidateKeyResult {
                        api_key,
                        status: "invalid".to_string(),
                        registration_ip,
                        registration_region,
                        assigned_proxy_key: None,
                        assigned_proxy_label: None,
                        assigned_proxy_match_kind: None,
                        quota_limit: None,
                        quota_remaining: None,
                        detail: Some(detail),
                    },
                    "invalid",
                )
            } else {
                (
                    ValidateKeyResult {
                        api_key,
                        status: "error".to_string(),
                        registration_ip,
                        registration_region,
                        assigned_proxy_key: None,
                        assigned_proxy_label: None,
                        assigned_proxy_match_kind: None,
                        quota_limit: None,
                        quota_remaining: None,
                        detail: Some(detail),
                    },
                    "error",
                )
            }
        }
        Err(ProxyError::QuotaDataMissing { reason }) => (
            ValidateKeyResult {
                api_key,
                status: "invalid".to_string(),
                registration_ip,
                registration_region,
                assigned_proxy_key: None,
                assigned_proxy_label: None,
                assigned_proxy_match_kind: None,
                quota_limit: None,
                quota_remaining: None,
                detail: Some(truncate_detail(
                    format!("quota_data_missing: {reason}"),
                    1400,
                )),
            },
            "invalid",
        ),
        Err(err) => (
            ValidateKeyResult {
                api_key,
                status: "error".to_string(),
                registration_ip,
                registration_region,
                assigned_proxy_key: None,
                assigned_proxy_label: None,
                assigned_proxy_match_kind: None,
                quota_limit: None,
                quota_remaining: None,
                detail: Some(truncate_detail(err.to_string(), 1400)),
            },
            "error",
        ),
    }
}
