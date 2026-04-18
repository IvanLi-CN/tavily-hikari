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

#[derive(Debug, Clone, Serialize)]
struct SummaryQuotaChargeView {
    local_estimated_credits: i64,
    upstream_actual_credits: i64,
    sampled_key_count: i64,
    stale_key_count: i64,
    latest_sync_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
struct SummaryWindowsView {
    today: SummaryWindowView,
    yesterday: SummaryWindowView,
    month: SummaryWindowView,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardHourlyRequestBucketView {
    bucket_start: i64,
    secondary_success: i64,
    primary_success: i64,
    secondary_failure: i64,
    primary_failure_429: i64,
    primary_failure_other: i64,
    unknown: i64,
    mcp_non_billable: i64,
    mcp_billable: i64,
    api_non_billable: i64,
    api_billable: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardHourlyRequestWindowView {
    bucket_seconds: i64,
    visible_buckets: i64,
    retained_buckets: i64,
    buckets: Vec<DashboardHourlyRequestBucketView>,
}

impl From<tavily_hikari::DashboardHourlyRequestWindow> for DashboardHourlyRequestWindowView {
    fn from(window: tavily_hikari::DashboardHourlyRequestWindow) -> Self {
        Self {
            bucket_seconds: window.bucket_seconds,
            visible_buckets: window.visible_buckets,
            retained_buckets: window.retained_buckets,
            buckets: window
                .buckets
                .into_iter()
                .map(|bucket| DashboardHourlyRequestBucketView {
                    bucket_start: bucket.bucket_start,
                    secondary_success: bucket.secondary_success,
                    primary_success: bucket.primary_success,
                    secondary_failure: bucket.secondary_failure,
                    primary_failure_429: bucket.primary_failure_429,
                    primary_failure_other: bucket.primary_failure_other,
                    unknown: bucket.unknown,
                    mcp_non_billable: bucket.mcp_non_billable,
                    mcp_billable: bucket.mcp_billable,
                    api_non_billable: bucket.api_non_billable,
                    api_billable: bucket.api_billable,
                })
                .collect(),
        }
    }
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

#[derive(Debug, Deserialize)]
struct PublicTodayWindowQuery {
    today_start: Option<String>,
    today_end: Option<String>,
}

fn parse_public_today_window_query(
    query: &PublicTodayWindowQuery,
) -> Result<Option<tavily_hikari::TimeRangeUtc>, (StatusCode, String)> {
    tavily_hikari::parse_explicit_today_window(query.today_start.as_deref(), query.today_end.as_deref())
        .map_err(|message| (StatusCode::BAD_REQUEST, message))
}

async fn get_public_metrics(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PublicTodayWindowQuery>,
) -> Result<Json<PublicMetricsView>, (StatusCode, String)> {
    let daily_window = parse_public_today_window_query(&query)?;
    state
        .proxy
        .success_breakdown(daily_window)
        .await
        .map(|metrics| {
            Json(PublicMetricsView {
                monthly_success: metrics.monthly_success,
                daily_success: metrics.daily_success,
            })
        })
        .map_err(|err| {
            eprintln!("public metrics error: {err}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load public metrics".to_string(),
            )
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
    today_start: Option<String>,
    today_end: Option<String>,
}

async fn get_token_metrics_public(
    State(state): State<Arc<AppState>>,
    Query(q): Query<TokenQuery>,
) -> Result<Json<TokenMetricsView>, (StatusCode, String)> {
    let daily_window = parse_public_today_window_query(&PublicTodayWindowQuery {
        today_start: q.today_start.clone(),
        today_end: q.today_end.clone(),
    })?;
    // Validate token first
    if !state
        .proxy
        .validate_access_token(&q.token)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to validate token".to_string(),
            )
        })?
    {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
    }

    // Extract id
    let token_id = q
        .token
        .strip_prefix("th-")
        .and_then(|rest| rest.split_once('-').map(|(id, _)| id))
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "invalid token".to_string()))?;
    let (monthly_success, daily_success, daily_failure) = state
        .proxy
        .token_success_breakdown(token_id, daily_window)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load token metrics".to_string(),
            )
        })?;

    // Use the same quota snapshot logic as the admin views so numbers stay consistent.
    let quota_verdict = state
        .proxy
        .token_quota_snapshot(token_id)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load token quota".to_string(),
            )
        })?;
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
    today_start: Option<String>,
    today_end: Option<String>,
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
) -> Result<Json<TavilyUsageView>, (StatusCode, String)> {
    let daily_window = parse_public_today_window_query(&PublicTodayWindowQuery {
        today_start: q.today_start.clone(),
        today_end: q.today_end.clone(),
    })?;
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
                .ok_or_else(|| (StatusCode::UNAUTHORIZED, "unauthorized".to_string()))?;
            format!("th-{id}-dev")
        }
        // Production: usage endpoint always requires an access token.
        (false, None) => return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string())),
    };

    // Validate token when not in dev-open-admin mode.
    if !using_dev_open_admin_fallback {
        let valid = state
            .proxy
            .validate_access_token(&token_str)
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to validate token".to_string(),
                )
            })?;
        if !valid {
            return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
        }
    }

    let token_id_from_token = token_str
        .strip_prefix("th-")
        .and_then(|rest| rest.split_once('-').map(|(id, _)| id.to_string()));

    let token_id = if let Some(explicit) = q.token_id.as_ref() {
        let trimmed = explicit.trim();
        if trimmed.is_empty() {
            return Err((StatusCode::BAD_REQUEST, "invalid token_id".to_string()));
        }
        if !using_dev_open_admin_fallback
            && token_id_from_token
                .as_ref()
                .is_some_and(|from_token| trimmed != from_token)
        {
            return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
        }
        trimmed.to_string()
    } else {
        token_id_from_token.ok_or_else(|| (StatusCode::BAD_REQUEST, "invalid token".to_string()))?
    };

    let (monthly_success, daily_success, daily_failure) = state
        .proxy
        .token_success_breakdown(&token_id, daily_window)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load token usage".to_string(),
            )
        })?;

    let now = Utc::now();
    let month_start = start_of_month_dt(now).timestamp();
    let now_ts = now.timestamp();
    let summary = state
        .proxy
        .token_summary_since(&token_id, month_start, Some(now_ts))
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load token summary".to_string(),
            )
        })?;

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

const DASHBOARD_EXHAUSTED_KEYS_LIMIT: usize = 5;
const DASHBOARD_RECENT_LOGS_LIMIT: usize = 5;
const DASHBOARD_TREND_SOURCE_LIMIT: usize = 64;
const DASHBOARD_TREND_WINDOW_SIZE: usize = 8;
const DASHBOARD_RECENT_JOBS_LIMIT: usize = 5;
const DASHBOARD_DISABLED_TOKENS_LIMIT: usize = 5;
const DASHBOARD_DISABLED_TOKENS_QUERY_LIMIT: usize = DASHBOARD_DISABLED_TOKENS_LIMIT + 1;

#[derive(Debug, Clone, Serialize)]
struct DashboardTrendView {
    request: Vec<i64>,
    error: Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardOverviewPayload {
    summary: SummaryView,
    #[serde(rename = "summaryWindows")]
    summary_windows: SummaryWindowsView,
    #[serde(rename = "hourlyRequestWindow")]
    hourly_request_window: DashboardHourlyRequestWindowView,
    #[serde(rename = "siteStatus")]
    site_status: DashboardSiteStatusView,
    #[serde(rename = "forwardProxy")]
    forward_proxy: DashboardForwardProxyView,
    trend: DashboardTrendView,
    #[serde(rename = "exhaustedKeys")]
    exhausted_keys: Vec<ApiKeyView>,
    #[serde(rename = "recentLogs")]
    recent_logs: Vec<RequestLogView>,
    #[serde(rename = "recentJobs")]
    recent_jobs: Vec<JobLogView>,
    #[serde(rename = "disabledTokens")]
    disabled_tokens: Vec<AuthTokenView>,
    #[serde(rename = "tokenCoverage")]
    token_coverage: String,
    #[serde(rename = "recentAlerts")]
    recent_alerts: DashboardRecentAlertsView,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardSnapshot {
    #[serde(flatten)]
    overview: DashboardOverviewPayload,
    keys: Vec<ApiKeyView>,
    logs: Vec<RequestLogView>,
}

fn build_dashboard_trend(logs: &[RequestLogView]) -> DashboardTrendView {
    let mut sorted: Vec<&RequestLogView> = logs
        .iter()
        .filter(|log| log.created_at >= 0)
        .collect();
    sorted.sort_by_key(|log| log.created_at);

    let mut request = vec![0_i64; DASHBOARD_TREND_WINDOW_SIZE];
    let mut error = vec![0_i64; DASHBOARD_TREND_WINDOW_SIZE];

    let Some(first) = sorted.first() else {
        return DashboardTrendView { request, error };
    };
    let Some(last) = sorted.last() else {
        return DashboardTrendView { request, error };
    };

    let min_time = first.created_at;
    let max_time = last.created_at;
    let span = (max_time - min_time).max(0) + 1;

    for log in sorted {
        let offset = (log.created_at - min_time).max(0);
        let index = (((offset as u128) * (DASHBOARD_TREND_WINDOW_SIZE as u128)) / (span as u128))
            .min((DASHBOARD_TREND_WINDOW_SIZE - 1) as u128) as usize;
        request[index] += 1;
        if log.result_status == "error" || log.result_status == "quota_exhausted" {
            error[index] += 1;
        }
    }

    DashboardTrendView { request, error }
}

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardForwardProxyView {
    available_nodes: Option<i64>,
    total_nodes: Option<i64>,
}

async fn get_dashboard_overview(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<DashboardOverviewPayload>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    build_dashboard_overview_payload(&state)
        .await
        .map(Json)
        .map_err(|err| {
            eprintln!("dashboard overview error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
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
    today_start: Option<String>,
    today_end: Option<String>,
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
) -> Result<Sse<impl Stream<Item = Result<Event, axum::http::Error>>>, (StatusCode, String)> {
    let state = state.clone();
    let token_param = q.token.clone();
    let daily_window = parse_public_today_window_query(&PublicTodayWindowQuery {
        today_start: q.today_start.clone(),
        today_end: q.today_end.clone(),
    })?;

    let stream = stream! {
        type TokenSig = (i64, i64, i64, i64, i64, i64, i64, i64, i64);
        type PublicSig = (i64, i64, Option<TokenSig>);
        async fn compute(
            state: &Arc<AppState>,
            token_param: &Option<String>,
            daily_window: Option<tavily_hikari::TimeRangeUtc>,
        ) -> Option<(PublicMetricsPayload, PublicSig)> {
            let m = state.proxy.success_breakdown(daily_window).await.ok()?;
            let public = PublicMetricsView { monthly_success: m.monthly_success, daily_success: m.daily_success };
            let token_sig: Option<TokenSig> = if let Some(token) = token_param.as_ref() {
                let valid = state.proxy.validate_access_token(token).await.ok()?;
                if !valid { None } else {
                    let id = token.strip_prefix("th-").and_then(|r| r.split_once('-').map(|(id, _)| id))?;
                    let (ms, ds, df) = state.proxy.token_success_breakdown(id, daily_window).await.ok()?;
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
        if let Some((payload, sig)) = compute(&state, &token_param, daily_window).await {
            let json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
            yield Ok(Event::default().event("metrics").data(json));
            last_sig = Some(sig);
        }
        loop {
            if let Some((payload, sig)) = compute(&state, &token_param, daily_window).await {
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

async fn build_dashboard_overview_payload(
    state: &Arc<AppState>,
) -> Result<DashboardOverviewPayload, ProxyError> {
    let summary = state.proxy.summary().await?;
    let tavily_hikari::SummaryWindows {
        today,
        yesterday,
        month,
    } = state.proxy.summary_windows().await?;
    let hourly_request_window = state.proxy.dashboard_hourly_request_window().await?;
    let forward_proxy = state.proxy.get_forward_proxy_dashboard_summary().await?;
    let exhausted_keys = state
        .proxy
        .list_dashboard_exhausted_key_metrics(DASHBOARD_EXHAUSTED_KEYS_LIMIT)
        .await
        .unwrap_or_default();
    let recent_log_views: Vec<RequestLogView> = state
        .proxy
        .recent_request_logs(DASHBOARD_TREND_SOURCE_LIMIT)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(RequestLogView::from_summary_record)
        .collect();
    let trend = build_dashboard_trend(&recent_log_views);
    let recent_logs: Vec<RequestLogView> = recent_log_views
        .into_iter()
        .take(DASHBOARD_RECENT_LOGS_LIMIT)
        .collect();
    let recent_jobs = state
        .proxy
        .list_recent_jobs(DASHBOARD_RECENT_JOBS_LIMIT)
        .await
        .unwrap_or_default();
    let recent_alerts = state
        .proxy
        .recent_alerts_summary(24)
        .await
        .unwrap_or(tavily_hikari::RecentAlertsSummary {
            window_hours: 24,
            total_events: 0,
            grouped_count: 0,
            counts_by_type: tavily_hikari::default_alert_type_counts(),
            top_groups: Vec::new(),
        });
    let (mut disabled_tokens, token_coverage) = match state
        .proxy
        .list_dashboard_disabled_tokens(DASHBOARD_DISABLED_TOKENS_QUERY_LIMIT)
        .await
    {
        Ok(disabled_tokens) => {
            let token_coverage = if disabled_tokens.len() > DASHBOARD_DISABLED_TOKENS_LIMIT {
                "truncated"
            } else {
                "ok"
            };
            (disabled_tokens, token_coverage)
        }
        Err(_) => (Vec::new(), "error"),
    };
    if disabled_tokens.len() > DASHBOARD_DISABLED_TOKENS_LIMIT {
        disabled_tokens.truncate(DASHBOARD_DISABLED_TOKENS_LIMIT);
    }

    Ok(DashboardOverviewPayload {
        summary: summary.clone().into(),
        summary_windows: SummaryWindowsView {
            today: SummaryWindowView::from(today),
            yesterday: SummaryWindowView::from(yesterday),
            month: SummaryWindowView::from(month),
        },
        hourly_request_window: DashboardHourlyRequestWindowView::from(hourly_request_window),
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
        trend,
        exhausted_keys: exhausted_keys.into_iter().map(ApiKeyView::from_list).collect(),
        recent_logs,
        recent_jobs: recent_jobs.into_iter().map(JobLogView::from).collect(),
        disabled_tokens: disabled_tokens.into_iter().map(AuthTokenView::from).collect(),
        token_coverage: token_coverage.to_string(),
        recent_alerts: DashboardRecentAlertsView::from(recent_alerts),
    })
}

async fn build_snapshot_event(state: &Arc<AppState>) -> Option<Event> {
    let overview = build_dashboard_overview_payload(state).await.ok()?;
    let payload = DashboardSnapshot {
        keys: overview.exhausted_keys.clone(),
        logs: overview.recent_logs.clone(),
        overview,
    };

    let json = serde_json::to_string(&payload).ok()?;
    Some(Event::default().event("snapshot").data(json))
}

async fn compute_signatures(
    state: &Arc<AppState>,
) -> Result<(Option<SummarySig>, Option<i64>), ()> {
    const DASHBOARD_HOURLY_BUCKET_SECS: i64 = 3600;
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
    let latest_id = state
        .proxy
        .latest_visible_request_log_id()
        .await
        .map_err(|_| ())?;
    let exhausted_keys = state
        .proxy
        .list_dashboard_exhausted_key_ids(DASHBOARD_EXHAUSTED_KEYS_LIMIT)
        .await
        .unwrap_or_default();
    let (disabled_tokens, disabled_tokens_error) = match state
        .proxy
        .list_dashboard_disabled_token_ids(DASHBOARD_DISABLED_TOKENS_QUERY_LIMIT)
        .await
    {
        Ok(disabled_tokens) => (disabled_tokens, false),
        Err(_) => (Vec::new(), true),
    };
    let recent_jobs = state
        .proxy
        .list_recent_job_signatures(DASHBOARD_RECENT_JOBS_LIMIT)
        .await
        .unwrap_or_default();
    let recent_alerts = state
        .proxy
        .recent_alerts_summary(24)
        .await
        .unwrap_or(tavily_hikari::RecentAlertsSummary {
            window_hours: 24,
            total_events: 0,
            grouped_count: 0,
            counts_by_type: tavily_hikari::default_alert_type_counts(),
            top_groups: Vec::new(),
        });
    let hourly_window_anchor = Utc::now()
        .timestamp()
        .div_euclid(DASHBOARD_HOURLY_BUCKET_SECS)
        .saturating_mul(DASHBOARD_HOURLY_BUCKET_SECS);
    let disabled_token_ids = disabled_tokens
        .iter()
        .take(DASHBOARD_DISABLED_TOKENS_LIMIT)
        .cloned()
        .collect::<Vec<_>>();
    let disabled_token_truncated = disabled_tokens.len() > DASHBOARD_DISABLED_TOKENS_LIMIT;
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
        exhausted_keys,
        disabled_tokens: disabled_token_ids,
        disabled_tokens_error,
        disabled_tokens_truncated: disabled_token_truncated,
        recent_jobs,
        hourly_window_anchor,
        recent_alerts_total_events: recent_alerts.total_events,
        recent_alerts_grouped_count: recent_alerts.grouped_count,
        recent_alerts_counts: recent_alerts
            .counts_by_type
            .into_iter()
            .map(|item| (item.alert_type, item.count))
            .collect(),
        recent_alerts_top_groups: recent_alerts
            .top_groups
            .into_iter()
            .map(|group| (group.id, group.count, group.last_seen))
            .collect(),
    });
    Ok((sig, latest_id))
}

// ---- Jobs listing ----
