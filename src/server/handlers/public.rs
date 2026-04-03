async fn fetch_summary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<SummaryView>, StatusCode> {
    state
        .proxy
        .summary()
        .await
        .map(|mut summary| {
            if !is_admin_request(state.as_ref(), &headers) {
                summary.quarantined_keys = 0;
            }
            Json(summary.into())
        })
        .map_err(|err| {
            eprintln!("summary error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

#[derive(Debug, Serialize)]
struct SummaryQuotaChargeView {
    local_estimated_credits: i64,
    upstream_actual_credits: i64,
    sampled_key_count: i64,
    stale_key_count: i64,
    latest_sync_at: Option<i64>,
}

#[derive(Debug, Serialize)]
struct SummaryWindowView {
    total_requests: i64,
    success_count: i64,
    error_count: i64,
    quota_exhausted_count: i64,
    valuable_success_count: i64,
    valuable_failure_count: i64,
    other_success_count: i64,
    other_failure_count: i64,
    unknown_count: i64,
    upstream_exhausted_key_count: i64,
    new_keys: i64,
    new_quarantines: i64,
    quota_charge: SummaryQuotaChargeView,
}

#[derive(Debug, Serialize)]
struct SummaryWindowsView {
    today: SummaryWindowView,
    yesterday: SummaryWindowView,
    month: SummaryWindowView,
}

async fn fetch_summary_windows(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<SummaryWindowsView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    state
        .proxy
        .summary_windows()
        .await
        .map(|summary| {
            let tavily_hikari::SummaryWindows {
                today,
                yesterday,
                month,
            } = summary;
            Json(SummaryWindowsView {
                today: SummaryWindowView::from(today),
                yesterday: SummaryWindowView::from(yesterday),
                month: SummaryWindowView::from(month),
            })
        })
        .map_err(|err| {
            eprintln!("summary windows error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn get_public_metrics(
    State(state): State<Arc<AppState>>,
) -> Result<Json<PublicMetricsView>, StatusCode> {
    state
        .proxy
        .success_breakdown()
        .await
        .map(|metrics| {
            Json(PublicMetricsView {
                monthly_success: metrics.monthly_success,
                daily_success: metrics.daily_success,
            })
        })
        .map_err(|err| {
            eprintln!("public metrics error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

impl From<tavily_hikari::SummaryWindowMetrics> for SummaryWindowView {
    fn from(summary: tavily_hikari::SummaryWindowMetrics) -> Self {
        Self {
            total_requests: summary.total_requests,
            success_count: summary.success_count,
            error_count: summary.error_count,
            quota_exhausted_count: summary.quota_exhausted_count,
            valuable_success_count: summary.valuable_success_count,
            valuable_failure_count: summary.valuable_failure_count,
            other_success_count: summary.other_success_count,
            other_failure_count: summary.other_failure_count,
            unknown_count: summary.unknown_count,
            upstream_exhausted_key_count: summary.upstream_exhausted_key_count,
            new_keys: summary.new_keys,
            new_quarantines: summary.new_quarantines,
            quota_charge: SummaryQuotaChargeView {
                local_estimated_credits: summary.quota_charge.local_estimated_credits,
                upstream_actual_credits: summary.quota_charge.upstream_actual_credits,
                sampled_key_count: summary.quota_charge.sampled_key_count,
                stale_key_count: summary.quota_charge.stale_key_count,
                latest_sync_at: summary.quota_charge.latest_sync_at,
            },
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenMetricsView {
    monthly_success: i64,
    daily_success: i64,
    daily_failure: i64,
    // Business quota (tools/call) windows
    quota_hourly_used: i64,
    quota_hourly_limit: i64,
    quota_daily_used: i64,
    quota_daily_limit: i64,
    quota_monthly_used: i64,
    quota_monthly_limit: i64,
}

#[derive(Deserialize)]
struct TokenQuery {
    token: String,
}

async fn get_token_metrics_public(
    State(state): State<Arc<AppState>>,
    Query(q): Query<TokenQuery>,
) -> Result<Json<TokenMetricsView>, StatusCode> {
    // Validate token first
    if !state
        .proxy
        .validate_access_token(&q.token)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Extract id
    let token_id = q
        .token
        .strip_prefix("th-")
        .and_then(|rest| rest.split_once('-').map(|(id, _)| id))
        .ok_or(StatusCode::BAD_REQUEST)?;
    let (monthly_success, daily_success, daily_failure) = state
        .proxy
        .token_success_breakdown(token_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Use the same quota snapshot logic as the admin views so numbers stay consistent.
    let quota_verdict = state
        .proxy
        .token_quota_snapshot(token_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let (
        quota_hourly_used,
        quota_hourly_limit,
        quota_daily_used,
        quota_daily_limit,
        quota_monthly_used,
        quota_monthly_limit,
    ) = if let Some(q) = quota_verdict {
        (
            q.hourly_used,
            q.hourly_limit,
            q.daily_used,
            q.daily_limit,
            q.monthly_used,
            q.monthly_limit,
        )
    } else {
        (
            0,
            effective_token_hourly_limit(),
            0,
            effective_token_daily_limit(),
            0,
            effective_token_monthly_limit(),
        )
    };

    Ok(Json(TokenMetricsView {
        monthly_success,
        daily_success,
        daily_failure,
        quota_hourly_used,
        quota_hourly_limit,
        quota_daily_used,
        quota_daily_limit,
        quota_monthly_used,
        quota_monthly_limit,
    }))
}

#[derive(Debug, Deserialize)]
struct TavilyUsageQuery {
    token_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TavilyUsageView {
    token_id: String,
    daily_success: i64,
    daily_error: i64,
    monthly_success: i64,
    monthly_quota_exhausted: i64,
}

async fn tavily_http_usage(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<TavilyUsageQuery>,
) -> Result<Json<TavilyUsageView>, StatusCode> {
    // Prefer Authorization: Bearer th-<id>-<secret>.
    let auth_bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string());
    let header_token = auth_bearer
        .as_deref()
        .and_then(|raw| raw.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string());

    let using_dev_open_admin_fallback = header_token.is_none() && state.dev_open_admin;
    let token_str = match (state.dev_open_admin, header_token) {
        // Normal path: Authorization header present.
        (_, Some(t)) => t,
        // Dev mode: allow specifying token_id directly for ad-hoc queries.
        (true, None) => {
            let id = q
                .token_id
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or(StatusCode::UNAUTHORIZED)?;
            format!("th-{id}-dev")
        }
        // Production: usage endpoint always requires an access token.
        (false, None) => return Err(StatusCode::UNAUTHORIZED),
    };

    // Validate token when not in dev-open-admin mode.
    if !using_dev_open_admin_fallback {
        let valid = state
            .proxy
            .validate_access_token(&token_str)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if !valid {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    let token_id_from_token = token_str
        .strip_prefix("th-")
        .and_then(|rest| rest.split_once('-').map(|(id, _)| id.to_string()));

    let token_id = if let Some(explicit) = q.token_id.as_ref() {
        let trimmed = explicit.trim();
        if trimmed.is_empty() {
            return Err(StatusCode::BAD_REQUEST);
        }
        if !using_dev_open_admin_fallback
            && token_id_from_token
                .as_ref()
                .is_some_and(|from_token| trimmed != from_token)
        {
            return Err(StatusCode::FORBIDDEN);
        }
        trimmed.to_string()
    } else {
        token_id_from_token.ok_or(StatusCode::BAD_REQUEST)?
    };

    let (monthly_success, daily_success, daily_failure) = state
        .proxy
        .token_success_breakdown(&token_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let now = Utc::now();
    let month_start = start_of_month_dt(now).timestamp();
    let now_ts = now.timestamp();
    let summary = state
        .proxy
        .token_summary_since(&token_id, month_start, Some(now_ts))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(TavilyUsageView {
        token_id,
        daily_success,
        daily_error: daily_failure,
        monthly_success,
        monthly_quota_exhausted: summary.quota_exhausted_count,
    }))
}

#[derive(Deserialize)]
struct PublicLogsQuery {
    token: String,
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicTokenLogView {
    id: i64,
    method: String,
    path: String,
    query: Option<String>,
    http_status: Option<i64>,
    mcp_status: Option<i64>,
    result_status: String,
    error_message: Option<String>,
    created_at: i64,
}

impl From<TokenLogRecord> for PublicTokenLogView {
    fn from(r: TokenLogRecord) -> Self {
        Self::from_record(r, UiLanguage::En)
    }
}

impl PublicTokenLogView {
    fn from_record(r: TokenLogRecord, language: UiLanguage) -> Self {
        let result_status =
            display_result_status_for_request_kind(&r.request_kind_key, &r.result_status);
        Self {
            id: r.id,
            method: r.method,
            path: r.path,
            query: r.query,
            http_status: r.http_status,
            mcp_status: r.mcp_status,
            result_status,
            error_message: append_solution_guidance_to_error(
                r.error_message,
                r.failure_kind.as_deref(),
                language,
            ),
            created_at: r.created_at,
        }
    }
}

fn redact_sensitive(input: &str) -> String {
    // Redact query parameter values like tavilyApiKey=... (case-insensitive)
    let mut s = input.to_string();
    let mut lower = s.to_lowercase();
    let needle = "tavilyapikey=";
    let redacted = "<redacted>";
    let mut offset = 0usize;
    while let Some(pos) = lower[offset..].find(needle) {
        let idx = offset + pos;
        let start = idx + needle.len();
        // find earliest delimiter among &, ), space, quote, newline
        let mut end = s.len();
        for delim in ['&', ')', ' ', '"', '\'', '\n'] {
            if let Some(p) = s[start..].find(delim) {
                end = (start + p).min(end);
            }
        }
        s.replace_range(start..end, redacted);
        lower = s.to_lowercase();
        offset = start + redacted.len();
    }
    // Redact header-like phrase "Tavily-Api-Key: <value>"
    // naive pass: case-insensitive search for "tavily-api-key"
    let mut out = String::new();
    let mut i = 0usize;
    let s_lower = s.to_lowercase();
    while let Some(pos) = s_lower[i..].find("tavily-api-key") {
        let idx = i + pos;
        out.push_str(&s[i..idx]);
        // advance to after possible colon
        let rest = &s[idx..];
        if let Some(colon) = rest.find(':') {
            out.push_str(&s[idx..idx + colon + 1]);
            out.push(' ');
            out.push_str(redacted);
            // skip value until whitespace or line break
            let after = idx + colon + 1;
            let mut end = s.len();
            for delim in ['\n', '\r'] {
                if let Some(p) = s[after..].find(delim) {
                    end = (after + p).min(end);
                }
            }
            i = end;
        } else {
            // no colon, just append token
            out.push_str("tavily-api-key");
            i = idx + "tavily-api-key".len();
        }
    }
    out.push_str(&s[i..]);
    out
}

async fn get_public_logs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<PublicLogsQuery>,
) -> Result<Json<Vec<PublicTokenLogView>>, StatusCode> {
    // Validate full token first
    if !state
        .proxy
        .validate_access_token(&q.token)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Extract short token id
    let token_id = q
        .token
        .strip_prefix("th-")
        .and_then(|rest| rest.split_once('-').map(|(id, _)| id))
        .ok_or(StatusCode::BAD_REQUEST)?;

    let limit = q.limit.unwrap_or(20).clamp(1, 20);
    let language = ui_language_from_headers(&headers);

    state
        .proxy
        .token_recent_logs(token_id, limit, None)
        .await
        .map(|items| {
            let mapped: Vec<PublicTokenLogView> = items
                .into_iter()
                .map(|record| PublicTokenLogView::from_record(record, language))
                .map(|mut v| {
                    // Redact sensitive patterns across error_message, path and query
                    if let Some(err) = v.error_message.as_ref() {
                        v.error_message = Some(redact_sensitive(err));
                    }
                    v.path = redact_sensitive(&v.path);
                    if let Some(q) = v.query.as_ref() {
                        v.query = Some(redact_sensitive(q));
                    }
                    v
                })
                .collect();
            Json(mapped)
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardSnapshot {
    summary: SummaryView,
    #[serde(rename = "summaryWindows")]
    summary_windows: SummaryWindowsView,
    #[serde(rename = "siteStatus")]
    site_status: DashboardSiteStatusView,
    #[serde(rename = "forwardProxy")]
    forward_proxy: DashboardForwardProxyView,
    keys: Vec<ApiKeyView>,
    logs: Vec<RequestLogView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardSiteStatusView {
    remaining_quota: i64,
    total_quota_limit: i64,
    active_keys: i64,
    quarantined_keys: i64,
    exhausted_keys: i64,
    available_proxy_nodes: Option<i64>,
    total_proxy_nodes: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardForwardProxyView {
    available_nodes: Option<i64>,
    total_nodes: Option<i64>,
}

async fn sse_dashboard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, axum::http::Error>>>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let state = state.clone();

    let stream = stream! {
        let mut last_sig: Option<SummarySig> = None;
        let mut last_log_id: Option<i64> = None;

        loop {
            match compute_signatures(&state).await {
                Ok((sig, latest_id)) => {
                    if last_sig.is_none() || sig != last_sig || latest_id != last_log_id {
                        if let Some(event) = build_snapshot_event(&state).await {
                            yield Ok(event);
                            last_sig = sig;
                            last_log_id = latest_id;
                        } else {
                            let degraded = Event::default().event("degraded").data("{}");
                            yield Ok(degraded);
                        }
                    } else {
                        let keep = Event::default().event("ping").data("{}");
                        yield Ok(keep);
                    }
                }
                Err(_e) => {
                    let degraded = Event::default().event("degraded").data("{}");
                    yield Ok(degraded);
                }
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("")))
}

#[derive(Deserialize)]
struct PublicEventsQuery {
    token: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicMetricsPayload {
    public: PublicMetricsView,
    token: Option<TokenMetricsView>,
}

async fn sse_public(
    State(state): State<Arc<AppState>>,
    Query(q): Query<PublicEventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, axum::http::Error>>>, StatusCode> {
    let state = state.clone();
    let token_param = q.token.clone();

    let stream = stream! {
        type TokenSig = (i64, i64, i64, i64, i64, i64, i64, i64, i64);
        type PublicSig = (i64, i64, Option<TokenSig>);
        async fn compute(state: &Arc<AppState>, token_param: &Option<String>) -> Option<(PublicMetricsPayload, PublicSig)> {
            let m = state.proxy.success_breakdown().await.ok()?;
            let public = PublicMetricsView { monthly_success: m.monthly_success, daily_success: m.daily_success };
            let token_sig: Option<TokenSig> = if let Some(token) = token_param.as_ref() {
                let valid = state.proxy.validate_access_token(token).await.ok()?;
                if !valid { None } else {
                    let id = token.strip_prefix("th-").and_then(|r| r.split_once('-').map(|(id, _)| id))?;
                    let (ms, ds, df) = state.proxy.token_success_breakdown(id).await.ok()?;
                    let quota_verdict = state.proxy.token_quota_snapshot(id).await.ok()?;
                    let (
                        quota_hourly_used,
                        quota_hourly_limit,
                        quota_daily_used,
                        quota_daily_limit,
                        quota_monthly_used,
                        quota_monthly_limit,
                    ) = if let Some(q) = quota_verdict {
                        (
                            q.hourly_used,
                            q.hourly_limit,
                            q.daily_used,
                            q.daily_limit,
                            q.monthly_used,
                            q.monthly_limit,
                        )
                    } else {
                        (
                            0,
                            effective_token_hourly_limit(),
                            0,
                            effective_token_daily_limit(),
                            0,
                            effective_token_monthly_limit(),
                        )
                    };
                    Some((
                        ms,
                        ds,
                        df,
                        quota_hourly_used,
                        quota_hourly_limit,
                        quota_daily_used,
                        quota_daily_limit,
                        quota_monthly_used,
                        quota_monthly_limit,
                    ))
                }
            } else { None };
            let token = token_sig.map(
                |(
                    ms,
                    ds,
                    df,
                    quota_hourly_used,
                    quota_hourly_limit,
                    quota_daily_used,
                    quota_daily_limit,
                    quota_monthly_used,
                    quota_monthly_limit,
                )| TokenMetricsView {
                    monthly_success: ms,
                    daily_success: ds,
                    daily_failure: df,
                    quota_hourly_used,
                    quota_hourly_limit,
                    quota_daily_used,
                    quota_daily_limit,
                    quota_monthly_used,
                    quota_monthly_limit,
                },
            );
            let sig: PublicSig = (public.monthly_success, public.daily_success, token_sig);
            let payload = PublicMetricsPayload { public, token };
            Some((payload, sig))
        }

        let mut last_sig: Option<PublicSig> = None;
        if let Some((payload, sig)) = compute(&state, &token_param).await {
            let json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
            yield Ok(Event::default().event("metrics").data(json));
            last_sig = Some(sig);
        }
        loop {
            if let Some((payload, sig)) = compute(&state, &token_param).await {
                if last_sig != Some(sig) {
                    let json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
                    yield Ok(Event::default().event("metrics").data(json));
                    last_sig = Some(sig);
                } else {
                    yield Ok(Event::default().event("ping").data("{}"));
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("")))
}

async fn build_snapshot_event(state: &Arc<AppState>) -> Option<Event> {
    let summary = state.proxy.summary().await.ok()?;
    let tavily_hikari::SummaryWindows {
        today,
        yesterday,
        month,
    } = state.proxy.summary_windows().await.ok()?;
    let forward_proxy = state.proxy.get_forward_proxy_dashboard_summary().await.ok()?;
    let keys = state.proxy.list_api_key_metrics().await.ok()?;
    let logs = state
        .proxy
        .recent_request_logs(DEFAULT_LOG_LIMIT)
        .await
        .ok()?;

    let payload = DashboardSnapshot {
        summary: summary.clone().into(),
        summary_windows: SummaryWindowsView {
            today: SummaryWindowView::from(today),
            yesterday: SummaryWindowView::from(yesterday),
            month: SummaryWindowView::from(month),
        },
        site_status: DashboardSiteStatusView {
            remaining_quota: summary.total_quota_remaining,
            total_quota_limit: summary.total_quota_limit,
            active_keys: summary.active_keys,
            quarantined_keys: summary.quarantined_keys,
            exhausted_keys: summary.exhausted_keys,
            available_proxy_nodes: Some(forward_proxy.available_nodes),
            total_proxy_nodes: Some(forward_proxy.total_nodes),
        },
        forward_proxy: DashboardForwardProxyView {
            available_nodes: Some(forward_proxy.available_nodes),
            total_nodes: Some(forward_proxy.total_nodes),
        },
        keys: keys.into_iter().map(ApiKeyView::from_list).collect(),
        logs: logs.into_iter().map(RequestLogView::from).collect(),
    };

    let json = serde_json::to_string(&payload).ok()?;
    Some(Event::default().event("snapshot").data(json))
}

async fn compute_signatures(
    state: &Arc<AppState>,
) -> Result<(Option<SummarySig>, Option<i64>), ()> {
    let summary = state.proxy.summary().await.map_err(|_| ())?;
    let tavily_hikari::SummaryWindows {
        today,
        yesterday,
        month,
    } = state.proxy.summary_windows().await.map_err(|_| ())?;
    let forward_proxy = state
        .proxy
        .get_forward_proxy_dashboard_summary()
        .await
        .map_err(|_| ())?;
    let logs = state.proxy.recent_request_logs(1).await.map_err(|_| ())?;
    let latest_id = logs.first().map(|l| l.id);
    let sig: Option<SummarySig> = Some(SummarySig {
        summary: [
            summary.total_requests,
            summary.success_count,
            summary.error_count,
            summary.quota_exhausted_count,
            summary.active_keys,
            summary.exhausted_keys,
            summary.quarantined_keys,
            summary.total_quota_limit,
            summary.total_quota_remaining,
        ],
        summary_last_activity: summary.last_activity,
        today: [
            today.total_requests,
            today.success_count,
            today.error_count,
            today.quota_exhausted_count,
            today.valuable_success_count,
            today.valuable_failure_count,
            today.other_success_count,
            today.other_failure_count,
            today.unknown_count,
            today.upstream_exhausted_key_count,
            today.quota_charge.local_estimated_credits,
            today.quota_charge.upstream_actual_credits,
            today.quota_charge.sampled_key_count,
            today.quota_charge.stale_key_count,
            today.quota_charge.latest_sync_at.unwrap_or_default(),
        ],
        yesterday: [
            yesterday.total_requests,
            yesterday.success_count,
            yesterday.error_count,
            yesterday.quota_exhausted_count,
            yesterday.valuable_success_count,
            yesterday.valuable_failure_count,
            yesterday.other_success_count,
            yesterday.other_failure_count,
            yesterday.unknown_count,
            yesterday.upstream_exhausted_key_count,
            yesterday.quota_charge.local_estimated_credits,
            yesterday.quota_charge.upstream_actual_credits,
            yesterday.quota_charge.sampled_key_count,
            yesterday.quota_charge.stale_key_count,
            yesterday.quota_charge.latest_sync_at.unwrap_or_default(),
        ],
        month: [
            month.total_requests,
            month.success_count,
            month.error_count,
            month.quota_exhausted_count,
            month.valuable_success_count,
            month.valuable_failure_count,
            month.other_success_count,
            month.other_failure_count,
            month.unknown_count,
            month.upstream_exhausted_key_count,
            month.new_keys,
            month.new_quarantines,
            month.quota_charge.local_estimated_credits,
            month.quota_charge.upstream_actual_credits,
            month.quota_charge.sampled_key_count,
            month.quota_charge.stale_key_count,
            month.quota_charge.latest_sync_at.unwrap_or_default(),
        ],
        proxy: Some((forward_proxy.available_nodes, forward_proxy.total_nodes)),
    });
    Ok((sig, latest_id))
}

// ---- Jobs listing ----
