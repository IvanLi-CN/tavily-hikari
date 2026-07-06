async fn fetch_summary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<SummaryView>, StatusCode> {
    let is_admin = is_admin_request(state.as_ref(), &headers).await;
    state
        .proxy
        .summary()
        .await
        .map(|mut summary| {
            if !is_admin {
                summary.active_keys += summary.temporary_isolated_keys;
                summary.quarantined_keys = 0;
                summary.temporary_isolated_keys = 0;
            }
            Json(summary.into())
        })
        .map_err(|err| {
            eprintln!("summary error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

fn dashboard_hourly_window_anchor(now_ts: i64) -> i64 {
    const DASHBOARD_HOURLY_WINDOW_BUCKET_SECS: i64 = 5 * 60;
    now_ts
        .div_euclid(DASHBOARD_HOURLY_WINDOW_BUCKET_SECS)
        .saturating_mul(DASHBOARD_HOURLY_WINDOW_BUCKET_SECS)
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
    today_start: i64,
    today_end: i64,
    today_period_end: i64,
    yesterday_start: i64,
    yesterday_end: i64,
    month_start: i64,
    month_end: i64,
    month_period_end: i64,
    previous_month_start: i64,
    previous_month_end: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserRankingIdentityView {
    user_id: String,
    display_name: Option<String>,
    username: Option<String>,
    avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserRankingRowView {
    rank: i64,
    value: i64,
    user: UserRankingIdentityView,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserRankingWindowView {
    primary_success_top: Vec<UserRankingRowView>,
    business_credits_top: Vec<UserRankingRowView>,
    unique_ip_top: Vec<UserRankingRowView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserRankingsSnapshotView {
    generated_at: i64,
    refresh_interval_secs: i64,
    last24h: UserRankingWindowView,
    last7d: UserRankingWindowView,
    last30d: UserRankingWindowView,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardMonthSeriesPointView {
    bucket_start: i64,
    display_bucket_start: Option<i64>,
    total: Option<i64>,
    valuable_success: Option<i64>,
    valuable_failure: Option<i64>,
    other_success: Option<i64>,
    other_failure: Option<i64>,
    unknown: Option<i64>,
    upstream_exhausted: Option<i64>,
    new_keys: Option<i64>,
    new_quarantines: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardMonthSeriesView {
    current: Vec<DashboardMonthSeriesPointView>,
    comparison: Vec<DashboardMonthSeriesPointView>,
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

impl From<tavily_hikari::DashboardMonthSeriesPoint> for DashboardMonthSeriesPointView {
    fn from(point: tavily_hikari::DashboardMonthSeriesPoint) -> Self {
        Self {
            bucket_start: point.bucket_start,
            display_bucket_start: point.display_bucket_start,
            total: point.total,
            valuable_success: point.valuable_success,
            valuable_failure: point.valuable_failure,
            other_success: point.other_success,
            other_failure: point.other_failure,
            unknown: point.unknown,
            upstream_exhausted: point.upstream_exhausted,
            new_keys: point.new_keys,
            new_quarantines: point.new_quarantines,
        }
    }
}

impl From<tavily_hikari::DashboardMonthSeries> for DashboardMonthSeriesView {
    fn from(series: tavily_hikari::DashboardMonthSeries) -> Self {
        Self {
            current: series.current.into_iter().map(Into::into).collect(),
            comparison: series.comparison.into_iter().map(Into::into).collect(),
        }
    }
}

fn build_user_ranking_row_view(
    row: tavily_hikari::UserRankingRow,
    cfg: &LinuxDoOAuthOptions,
) -> UserRankingRowView {
    UserRankingRowView {
        rank: row.rank,
        value: row.value,
        user: UserRankingIdentityView {
            user_id: row.user.user_id,
            display_name: row.user.display_name,
            username: row.user.username,
            avatar_url: resolve_linuxdo_avatar_url(cfg, row.user.avatar_template.as_deref()),
        },
    }
}

fn build_user_ranking_window_view(
    window: tavily_hikari::UserRankingWindow,
    cfg: &LinuxDoOAuthOptions,
) -> UserRankingWindowView {
    UserRankingWindowView {
        primary_success_top: window
            .primary_success_top
            .into_iter()
            .map(|row| build_user_ranking_row_view(row, cfg))
            .collect(),
        business_credits_top: window
            .business_credits_top
            .into_iter()
            .map(|row| build_user_ranking_row_view(row, cfg))
            .collect(),
        unique_ip_top: window
            .unique_ip_top
            .into_iter()
            .map(|row| build_user_ranking_row_view(row, cfg))
            .collect(),
    }
}

fn build_user_rankings_snapshot_view(
    snapshot: tavily_hikari::UserRankingsSnapshot,
    cfg: &LinuxDoOAuthOptions,
) -> UserRankingsSnapshotView {
    UserRankingsSnapshotView {
        generated_at: snapshot.generated_at,
        refresh_interval_secs: snapshot.refresh_interval_secs,
        last24h: build_user_ranking_window_view(snapshot.last24h, cfg),
        last7d: build_user_ranking_window_view(snapshot.last7d, cfg),
        last30d: build_user_ranking_window_view(snapshot.last30d, cfg),
    }
}

async fn fetch_summary_windows(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<SummaryWindowsView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err(StatusCode::FORBIDDEN);
    }

    state
        .proxy
        .summary_windows()
        .await
        .map(|summary| Json(SummaryWindowsView::from(summary)))
        .map_err(|err| {
            eprintln!("summary windows error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn get_user_rankings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<UserRankingsSnapshotView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err(StatusCode::FORBIDDEN);
    }

    state
        .proxy
        .user_rankings_snapshot()
        .await
        .map(|snapshot| Json(build_user_rankings_snapshot_view(snapshot, &state.linuxdo_oauth)))
        .map_err(|err| {
            eprintln!("user rankings error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn sse_user_rankings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, axum::http::Error>>>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err(StatusCode::FORBIDDEN);
    }
    let state = state.clone();

    let stream = stream! {
        loop {
            match state.proxy.user_rankings_snapshot().await {
                Ok(snapshot) => {
                    let view = build_user_rankings_snapshot_view(snapshot, &state.linuxdo_oauth);
                    match serde_json::to_string(&view) {
                        Ok(json) => yield Ok(Event::default().event("snapshot").data(json)),
                        Err(_) => yield Ok(Event::default().event("degraded").data("{}")),
                    }
                }
                Err(_) => yield Ok(Event::default().event("degraded").data("{}")),
            }

            state.proxy.backend_time().sleep(Duration::from_secs(10)).await;
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("")))
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

impl From<tavily_hikari::SummaryWindows> for SummaryWindowsView {
    fn from(summary: tavily_hikari::SummaryWindows) -> Self {
        let tavily_hikari::SummaryWindows {
            today,
            yesterday,
            month,
            today_start,
            today_end,
            today_period_end,
            yesterday_start,
            yesterday_end,
            month_start,
            month_end,
            month_period_end,
            previous_month_start,
            previous_month_end,
        } = summary;
        Self {
            today: SummaryWindowView::from(today),
            yesterday: SummaryWindowView::from(yesterday),
            month: SummaryWindowView::from(month),
            today_start,
            today_end,
            today_period_end,
            yesterday_start,
            yesterday_end,
            month_start,
            month_end,
            month_period_end,
            previous_month_start,
            previous_month_end,
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
    ensure_ha_allows_basic_business_status(&state, "/api/tavily/usage").await?;

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

    let now = state.proxy.backend_time().now_utc();
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
const DASHBOARD_OVERVIEW_LOADING_STALE_AFTER: Duration = Duration::from_secs(30);
const DASHBOARD_RECENT_ALERTS_TOKEN_TIMEOUT: Duration = Duration::from_secs(2);

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
    #[serde(rename = "monthSeries")]
    month_series: DashboardMonthSeriesView,
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

#[derive(Debug, Clone)]
struct DashboardOverviewSnapshot {
    payload: DashboardOverviewPayload,
    freshness: DashboardOverviewFreshness,
}

struct DashboardOverviewLoadGuard {
    state: Arc<Mutex<DashboardOverviewCacheState>>,
    generation: u64,
    armed: bool,
}

impl DashboardOverviewLoadGuard {
    fn new(state: Arc<Mutex<DashboardOverviewCacheState>>, generation: u64) -> Self {
        Self {
            state,
            generation,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for DashboardOverviewLoadGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        let state = self.state.clone();
        let generation = self.generation;
        tokio::spawn(async move {
            let mut cache = state.lock().await;
            if cache.loading && cache.loading_generation == generation {
                cache.loading = false;
                cache.loading_started_at = None;
                cache.notify.notify_waiters();
            }
        });
    }
}

#[cfg(test)]
async fn reset_dashboard_overview_build_count(state: &Arc<AppState>) {
    let cache_handle = dashboard_overview_cache_for_state(state.as_ref());
    let mut cache = cache_handle.lock().await;
    cache.build_count = 0;
}

#[cfg(test)]
async fn dashboard_overview_build_count(state: &Arc<AppState>) -> usize {
    let cache_handle = dashboard_overview_cache_for_state(state.as_ref());
    let cache = cache_handle.lock().await;
    cache.build_count
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
    temporary_isolated_keys: i64,
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
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err(StatusCode::FORBIDDEN);
    }
    load_dashboard_overview_snapshot(&state)
        .await
        .map(|snapshot| {
            tavily_hikari::emit_low_memory_protection_decision(
                "admin_read",
                tavily_hikari::PerfLogScope {
                    route: Some("/api/dashboard/overview"),
                    scope: Some("dashboard"),
                    degraded: Some("full"),
                    ..Default::default()
                },
            );
            Json(snapshot.payload)
        })
        .map_err(|err| {
            eprintln!("dashboard overview error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn sse_dashboard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, axum::http::Error>>>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers).await {
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
                        if let Some((event, emitted_sig)) = build_snapshot_event(&state).await {
                            yield Ok(event);
                            last_log_id = emitted_sig.freshness.latest_request_log_id;
                            last_sig = Some(emitted_sig);
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

            state.proxy.backend_time().sleep(Duration::from_secs(2)).await;
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
            state.proxy.backend_time().sleep(Duration::from_secs(2)).await;
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("")))
}

async fn build_dashboard_overview_payload(
    state: &Arc<AppState>,
) -> Result<DashboardOverviewSnapshot, ProxyError> {
    #[cfg(test)]
    {
        let cache_handle = dashboard_overview_cache_for_state(state.as_ref());
        let mut cache = cache_handle.lock().await;
        cache.build_count = cache.build_count.saturating_add(1);
    }

    let summary = state.proxy.summary().await?;
    let summary_windows = state.proxy.summary_windows().await?;
    let hourly_request_window = state.proxy.dashboard_hourly_request_window().await?;
    let month_series = state.proxy.dashboard_month_series(&summary_windows).await?;
    let dashboard_rollup_signature = state
        .proxy
        .dashboard_rollup_freshness_signature_without_flush(summary_windows.previous_month_start)
        .await?;
    let pending_dashboard_rollup_signature = state
        .proxy
        .pending_dashboard_rollup_freshness_signature()
        .await;
    let dashboard_api_key_lifecycle_signature = state
        .proxy
        .dashboard_api_key_lifecycle_signature(summary_windows.previous_month_start)
        .await?;
    let dashboard_quarantine_lifecycle_signature = state
        .proxy
        .dashboard_quarantine_lifecycle_signature(summary_windows.previous_month_start)
        .await?;
    let dashboard_exhausted_lifecycle_signature = state
        .proxy
        .dashboard_exhausted_lifecycle_signature(
            summary_windows.previous_month_start,
            summary_windows.month_period_end,
        )
        .await?;
    let now_ts = state.proxy.backend_time().now_ts();
    let hot_active_since = now_ts.saturating_sub(2 * 60 * 60);
    let hot_stale_before = now_ts.saturating_sub(15 * 60);
    let cold_stale_before = now_ts.saturating_sub(24 * 60 * 60);
    let dashboard_stale_key_count = state
        .proxy
        .dashboard_stale_key_count(hot_active_since, hot_stale_before, cold_stale_before)
        .await?;
    let dashboard_quota_charge_token = state
        .proxy
        .dashboard_quota_charge_token(
            dashboard_stale_key_count,
            start_of_month_dt(state.proxy.backend_time().now_utc()).timestamp(),
            summary_windows.today_end,
        )
        .await?;
    let forward_proxy = state.proxy.get_forward_proxy_dashboard_summary().await?;
    let (request_log_retention_days, retention_since) =
        dashboard_request_log_retention(state).await?;
    let exhausted_keys = state
        .proxy
        .list_dashboard_exhausted_key_metrics(DASHBOARD_EXHAUSTED_KEYS_LIMIT)
        .await
        .unwrap_or_default();
    let exhausted_key_ids = exhausted_keys
        .iter()
        .map(|key| key.id.clone())
        .collect::<Vec<_>>();
    let recent_log_views: Vec<RequestLogView> = state
        .proxy
        .recent_request_logs(DASHBOARD_TREND_SOURCE_LIMIT)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(RequestLogView::from_summary_record)
        .collect();
    let trend_request_logs = recent_log_views
        .iter()
        .map(|log| (log.id, log.created_at))
        .collect::<Vec<_>>();
    let trend = build_dashboard_trend(&recent_log_views);
    let recent_logs: Vec<RequestLogView> = recent_log_views
        .into_iter()
        .take(DASHBOARD_RECENT_LOGS_LIMIT)
        .collect();
    let recent_request_logs = recent_logs
        .iter()
        .map(|log| (log.id, log.created_at))
        .collect::<Vec<_>>();
    let latest_request_log_id = recent_logs.first().map(|log| log.id);
    let recent_jobs = state
        .proxy
        .list_recent_jobs(DASHBOARD_RECENT_JOBS_LIMIT)
        .await
        .unwrap_or_default();
    let recent_alerts = state
        .proxy
        .dashboard_recent_alerts_summary(24)
        .await
        .unwrap_or_else(|_| tavily_hikari::RecentAlertsSummary::default());
    let recent_alerts_token = dashboard_recent_alerts_token_or_fallback(state, 24, &recent_alerts).await;
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

    let hourly_window_anchor = dashboard_hourly_window_anchor(state.proxy.backend_time().now_ts());
    let disabled_tokens_error = token_coverage == "error";
    let disabled_token_truncated = token_coverage == "truncated";
    let disabled_token_ids = disabled_tokens
        .iter()
        .map(|token| token.id.clone())
        .collect::<Vec<_>>();
    let recent_job_signatures = recent_jobs
        .iter()
        .map(|job| (job.id, job.status.clone(), job.finished_at))
        .collect::<Vec<_>>();
    let recent_alert_counts = recent_alerts
        .counts_by_type
        .iter()
        .map(|item| (item.alert_type.clone(), item.count))
        .collect::<Vec<_>>();
    let recent_alert_top_groups = recent_alerts
        .top_groups
        .iter()
        .map(|group| (group.id.clone(), group.count, group.last_seen))
        .collect::<Vec<_>>();
    let recent_alerts_view = DashboardRecentAlertsView::from(recent_alerts.clone());

    Ok(DashboardOverviewSnapshot {
        payload: DashboardOverviewPayload {
            summary: summary.clone().into(),
            summary_windows: SummaryWindowsView::from(summary_windows.clone()),
            hourly_request_window: DashboardHourlyRequestWindowView::from(hourly_request_window),
            month_series: DashboardMonthSeriesView::from(month_series.clone()),
            site_status: DashboardSiteStatusView {
                remaining_quota: summary.total_quota_remaining,
                total_quota_limit: summary.total_quota_limit,
                active_keys: summary.active_keys,
                quarantined_keys: summary.quarantined_keys,
                temporary_isolated_keys: summary.temporary_isolated_keys,
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
            recent_alerts: recent_alerts_view,
        },
        freshness: DashboardOverviewFreshness {
            summary: [
                summary.total_requests,
                summary.success_count,
                summary.error_count,
                summary.quota_exhausted_count,
                summary.active_keys,
                summary.exhausted_keys,
                summary.quarantined_keys,
                summary.temporary_isolated_keys,
                summary.total_quota_limit,
                summary.total_quota_remaining,
            ],
            summary_last_activity: summary.last_activity,
            summary_window_starts: [
                summary_windows.today_start,
                summary_windows.yesterday_start,
                summary_windows.month_start,
            ],
            dashboard_rollup_signature,
            pending_dashboard_rollup_signature,
            dashboard_api_key_lifecycle_signature,
            dashboard_quarantine_lifecycle_signature,
            dashboard_exhausted_lifecycle_signature,
            dashboard_quota_charge_token,
            dashboard_stale_key_count,
            forward_proxy: Some((forward_proxy.available_nodes, forward_proxy.total_nodes)),
            exhausted_keys: exhausted_key_ids,
            latest_quota_sync_sample_at: state.proxy.latest_dashboard_quota_sync_sample_at().await?,
            latest_request_log_id,
            recent_request_logs,
            trend_request_logs,
            recent_jobs: recent_job_signatures,
            disabled_tokens: disabled_token_ids,
            disabled_tokens_error,
            disabled_tokens_truncated: disabled_token_truncated,
            recent_alerts_token,
            recent_alerts_total_events: recent_alerts.total_events,
            recent_alerts_grouped_count: recent_alerts.grouped_count,
            recent_alerts_counts: recent_alert_counts,
            recent_alerts_top_groups: recent_alert_top_groups,
            request_log_retention_days,
            hourly_window_anchor,
            retention_since,
        },
    })
}

async fn dashboard_request_log_retention(
    state: &Arc<AppState>,
) -> Result<(i64, i64), ProxyError> {
    let settings = state.proxy.get_system_settings().await?;
    let retention_days = settings.request_log_retention.max_log_retention_days;
    let now = state.proxy.backend_time().local_now();
    Ok((retention_days, dashboard_retention_since(retention_days, now)))
}

async fn dashboard_recent_alerts_freshness(
    state: &Arc<AppState>,
    window_hours: i64,
) -> Result<tavily_hikari::RecentAlertsSummary, ProxyError> {
    state.proxy.dashboard_recent_alerts_summary(window_hours).await
}

fn fallback_recent_alerts_token(summary: &tavily_hikari::RecentAlertsSummary) -> [i64; 4] {
    let top_group_last_seen_sum = summary
        .top_groups
        .iter()
        .map(|group| group.last_seen)
        .sum::<i64>();
    let typed_count_sum = summary
        .counts_by_type
        .iter()
        .map(|item| item.count)
        .sum::<i64>();
    [
        summary.total_events,
        summary.grouped_count,
        top_group_last_seen_sum,
        typed_count_sum,
    ]
}

async fn dashboard_recent_alerts_token_or_fallback(
    state: &Arc<AppState>,
    window_hours: i64,
    summary: &tavily_hikari::RecentAlertsSummary,
) -> [i64; 4] {
    match tokio::time::timeout(
        DASHBOARD_RECENT_ALERTS_TOKEN_TIMEOUT,
        state.proxy.dashboard_recent_alerts_token(window_hours),
    )
    .await
    {
        Ok(Ok(token)) => token,
        Ok(Err(err)) => {
            tracing::warn!(
                component = "admin_read",
                event = "dashboard_recent_alerts_token_degraded",
                err = %err,
                "dashboard recent-alerts token degraded to summary-derived freshness"
            );
            fallback_recent_alerts_token(summary)
        }
        Err(_) => {
            tracing::warn!(
                component = "admin_read",
                event = "dashboard_recent_alerts_token_timeout",
                timeout_ms = DASHBOARD_RECENT_ALERTS_TOKEN_TIMEOUT.as_millis() as u64,
                "dashboard recent-alerts token timed out; using summary-derived freshness"
            );
            fallback_recent_alerts_token(summary)
        }
    }
}

fn dashboard_retention_since(retention_days: i64, now: chrono::DateTime<Local>) -> i64 {
    let days = retention_days.max(0);
    if days == 0 {
        return now.with_timezone(&Utc).timestamp();
    }
    let keep_from_date = now
        .date_naive()
        .checked_sub_days(chrono::Days::new((days - 1) as u64))
        .unwrap_or_else(|| now.date_naive());
    dashboard_local_midnight_utc_ts(keep_from_date, now)
}

fn dashboard_start_of_local_day_utc_ts(now: chrono::DateTime<Local>) -> i64 {
    dashboard_local_midnight_utc_ts(now.date_naive(), now)
}

fn dashboard_previous_local_day_start_utc_ts(now: chrono::DateTime<Local>) -> i64 {
    let previous_date = now
        .date_naive()
        .pred_opt()
        .unwrap_or_else(|| now.date_naive());
    dashboard_local_midnight_utc_ts(previous_date, now)
}

fn dashboard_start_of_local_month_utc_ts(now: chrono::DateTime<Local>) -> i64 {
    let first_day = chrono::NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
        .expect("valid local month start date");
    dashboard_local_midnight_utc_ts(first_day, now)
}

fn dashboard_next_local_day_start_utc_ts(current_day_start_utc_ts: i64) -> i64 {
    let Some(utc_dt) = Utc.timestamp_opt(current_day_start_utc_ts, 0).single() else {
        return current_day_start_utc_ts.saturating_add(86_400);
    };
    let local_dt = utc_dt.with_timezone(&Local);
    let next_date = local_dt
        .date_naive()
        .succ_opt()
        .unwrap_or_else(|| local_dt.date_naive());
    dashboard_local_midnight_utc_ts(next_date, local_dt)
}

fn dashboard_previous_local_month_start_utc_ts(now: chrono::DateTime<Local>) -> i64 {
    let (year, month) = if now.month() == 1 {
        (now.year() - 1, 12)
    } else {
        (now.year(), now.month() - 1)
    };
    let first_day =
        chrono::NaiveDate::from_ymd_opt(year, month, 1).expect("valid previous month date");
    dashboard_local_midnight_utc_ts(first_day, now)
}

fn dashboard_shift_local_month_start_utc_ts(current_month_start_utc_ts: i64, delta_months: i32) -> i64 {
    let Some(utc_dt) = Utc.timestamp_opt(current_month_start_utc_ts, 0).single() else {
        return current_month_start_utc_ts;
    };
    let local_dt = utc_dt.with_timezone(&Local);
    let total_months = local_dt.year() * 12 + local_dt.month0() as i32 + delta_months;
    let shifted_year = total_months.div_euclid(12);
    let shifted_month0 = total_months.rem_euclid(12);
    let shifted_month = (shifted_month0 + 1) as u32;
    let shifted_day = chrono::NaiveDate::from_ymd_opt(shifted_year, shifted_month, 1)
        .expect("valid shifted month date");
    dashboard_local_midnight_utc_ts(shifted_day, local_dt)
}

fn dashboard_local_midnight_utc_ts(
    date: chrono::NaiveDate,
    fallback_now: chrono::DateTime<Local>,
) -> i64 {
    let naive = date.and_hms_opt(0, 0, 0).expect("valid local midnight");
    match Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => dt.with_timezone(&Utc).timestamp(),
        chrono::LocalResult::Ambiguous(dt, _) => dt.with_timezone(&Utc).timestamp(),
        chrono::LocalResult::None => fallback_now.with_timezone(&Utc).timestamp(),
    }
}

async fn compute_dashboard_overview_freshness(
    state: &Arc<AppState>,
) -> Result<DashboardOverviewFreshness, ProxyError> {
    let summary = state.proxy.summary_without_flush().await?;
    let now_local = state.proxy.backend_time().local_now();
    let now_utc = now_local.with_timezone(&Utc);
    let today_start = dashboard_start_of_local_day_utc_ts(now_local);
    let yesterday_start = dashboard_previous_local_day_start_utc_ts(now_local);
    let month_start = dashboard_start_of_local_month_utc_ts(now_local);
    let month_period_end = dashboard_next_local_day_start_utc_ts(today_start)
        .max(dashboard_shift_local_month_start_utc_ts(month_start, 1));
    let previous_month_start = dashboard_previous_local_month_start_utc_ts(now_local);
    let dashboard_rollup_signature = state
        .proxy
        .dashboard_rollup_freshness_signature_without_flush(previous_month_start)
        .await?;
    let pending_dashboard_rollup_signature = state
        .proxy
        .pending_dashboard_rollup_freshness_signature()
        .await;
    let dashboard_api_key_lifecycle_signature = state
        .proxy
        .dashboard_api_key_lifecycle_signature(previous_month_start)
        .await?;
    let dashboard_quarantine_lifecycle_signature = state
        .proxy
        .dashboard_quarantine_lifecycle_signature(previous_month_start)
        .await?;
    let dashboard_exhausted_lifecycle_signature = state
        .proxy
        .dashboard_exhausted_lifecycle_signature(previous_month_start, month_period_end)
        .await?;
    let now_ts = state.proxy.backend_time().now_ts();
    let hot_active_since = now_ts.saturating_sub(2 * 60 * 60);
    let hot_stale_before = now_ts.saturating_sub(15 * 60);
    let cold_stale_before = now_ts.saturating_sub(24 * 60 * 60);
    let dashboard_stale_key_count = state
        .proxy
        .dashboard_stale_key_count(hot_active_since, hot_stale_before, cold_stale_before)
        .await?;
    let dashboard_quota_charge_token = state
        .proxy
        .dashboard_quota_charge_token(
            dashboard_stale_key_count,
            start_of_month_dt(now_utc).timestamp(),
            state.proxy.backend_time().now_ts().saturating_add(1),
        )
        .await?;
    let forward_proxy = state.proxy.get_forward_proxy_dashboard_summary().await?;
    let summary_window_starts = [today_start, yesterday_start, month_start];
    let (request_log_retention_days, retention_since) =
        dashboard_request_log_retention(state).await?;
    let trend_request_logs = state
        .proxy
        .recent_request_log_signature(DASHBOARD_TREND_SOURCE_LIMIT, retention_since)
        .await?;
    let recent_request_logs = trend_request_logs
        .iter()
        .take(DASHBOARD_RECENT_LOGS_LIMIT)
        .copied()
        .collect::<Vec<_>>();
    let latest_request_log_id = trend_request_logs.first().map(|(id, _)| *id);
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
    let recent_alerts = dashboard_recent_alerts_freshness(state, 24)
        .await
        .unwrap_or_else(|_| tavily_hikari::RecentAlertsSummary::default());
    let recent_alerts_token = dashboard_recent_alerts_token_or_fallback(state, 24, &recent_alerts).await;
    Ok(DashboardOverviewFreshness {
        summary: [
            summary.total_requests,
            summary.success_count,
            summary.error_count,
            summary.quota_exhausted_count,
            summary.active_keys,
            summary.exhausted_keys,
            summary.quarantined_keys,
            summary.temporary_isolated_keys,
            summary.total_quota_limit,
            summary.total_quota_remaining,
        ],
        summary_last_activity: summary.last_activity,
        summary_window_starts,
        dashboard_rollup_signature,
        pending_dashboard_rollup_signature,
        dashboard_api_key_lifecycle_signature,
        dashboard_quarantine_lifecycle_signature,
        dashboard_exhausted_lifecycle_signature,
        dashboard_quota_charge_token,
        dashboard_stale_key_count,
        forward_proxy: Some((forward_proxy.available_nodes, forward_proxy.total_nodes)),
        exhausted_keys,
        latest_quota_sync_sample_at: state.proxy.latest_dashboard_quota_sync_sample_at().await?,
        latest_request_log_id,
        recent_request_logs,
        trend_request_logs,
        recent_jobs,
        disabled_tokens: disabled_tokens
            .iter()
            .take(DASHBOARD_DISABLED_TOKENS_LIMIT)
            .cloned()
            .collect(),
        disabled_tokens_error,
        disabled_tokens_truncated: disabled_tokens.len() > DASHBOARD_DISABLED_TOKENS_LIMIT,
        recent_alerts_token,
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
        request_log_retention_days,
        hourly_window_anchor: dashboard_hourly_window_anchor(state.proxy.backend_time().now_ts()),
        retention_since,
    })
}

async fn load_dashboard_overview_snapshot(
    state: &Arc<AppState>,
) -> Result<DashboardOverviewSnapshot, ProxyError> {
    let perf = tavily_hikari::RuntimePerfScope::start();
    loop {
        let cache_handle = dashboard_overview_cache_for_state(state.as_ref());
        let waiter = {
            let mut cache = cache_handle.lock().await;
            if cache.loading {
                let stale = cache
                    .loading_started_at
                    .is_some_and(|started_at| started_at.elapsed() >= DASHBOARD_OVERVIEW_LOADING_STALE_AFTER);
                if stale {
                    tracing::warn!(
                        component = "admin_read",
                        event = "dashboard_overview_loading_stale",
                        stale_after_ms = DASHBOARD_OVERVIEW_LOADING_STALE_AFTER.as_millis() as u64,
                        "dashboard overview shared snapshot loader was stale; allowing a new request to rebuild"
                    );
                    cache.loading = false;
                    cache.loading_started_at = None;
                    cache.notify.notify_waiters();
                    None
                } else {
                    Some(cache.notify.clone().notified_owned())
                }
            } else {
                None
            }
        };

        if let Some(waiter) = waiter {
            tavily_hikari::emit_perf_log(
                tavily_hikari::DbLogStatus::Info,
                "admin_read",
                "dashboard_overview_phase",
                Duration::ZERO,
                tavily_hikari::PerfLogScope {
                    route: Some("/api/dashboard/overview"),
                    scope: Some("dashboard"),
                    phase: Some("cache_wait"),
                    degraded: Some("shared_snapshot"),
                    ..Default::default()
                },
            );
            waiter.await;
            continue;
        }

        let freshness_started = Instant::now();
        let freshness = compute_dashboard_overview_freshness(state).await?;
        tavily_hikari::emit_perf_log(
            tavily_hikari::DbLogStatus::Info,
            "admin_read",
            "dashboard_overview_phase",
            freshness_started.elapsed(),
            tavily_hikari::PerfLogScope {
                route: Some("/api/dashboard/overview"),
                scope: Some("dashboard"),
                phase: Some("freshness_probe"),
                degraded: Some("cheap_token"),
                ..Default::default()
            },
        );

        let mut load_generation = None;
        let waiter = {
            let mut cache = cache_handle.lock().await;
            if let Some(cached) = cache.cached.as_ref()
                && cached.freshness == freshness
            {
                tavily_hikari::emit_low_memory_protection_decision(
                    "admin_read",
                    tavily_hikari::PerfLogScope {
                        route: Some("dashboard_shared_snapshot"),
                        scope: Some("dashboard"),
                        phase: Some("cache_serve"),
                        degraded: Some("cache_hit"),
                        ..Default::default()
                    },
                );
                tavily_hikari::emit_perf_log(
                    tavily_hikari::DbLogStatus::Info,
                    "admin_read",
                    "dashboard_snapshot_cache_hit",
                    Duration::from_millis(perf.elapsed_ms()),
                    tavily_hikari::PerfLogScope {
                        route: Some("dashboard_shared_snapshot"),
                        scope: Some("dashboard"),
                        phase: Some("cache_serve"),
                        degraded: Some("cache_hit"),
                        ..Default::default()
                    },
                );
                return Ok(cached.snapshot.clone());
            }
            if cache.loading {
                let stale = cache
                    .loading_started_at
                    .is_some_and(|started_at| started_at.elapsed() >= DASHBOARD_OVERVIEW_LOADING_STALE_AFTER);
                if stale {
                    tracing::warn!(
                        component = "admin_read",
                        event = "dashboard_overview_loading_stale",
                        stale_after_ms = DASHBOARD_OVERVIEW_LOADING_STALE_AFTER.as_millis() as u64,
                        "dashboard overview shared snapshot loader was stale after freshness probe; taking over rebuild"
                    );
                    cache.loading = true;
                    cache.loading_generation = cache.loading_generation.wrapping_add(1);
                    cache.loading_started_at = Some(tokio::time::Instant::now());
                    load_generation = Some(cache.loading_generation);
                    None
                } else {
                    Some(cache.notify.clone().notified_owned())
                }
            } else {
                cache.loading = true;
                cache.loading_generation = cache.loading_generation.wrapping_add(1);
                cache.loading_started_at = Some(tokio::time::Instant::now());
                load_generation = Some(cache.loading_generation);
                None
            }
        };

        if let Some(waiter) = waiter {
            waiter.await;
            continue;
        }

        let load_generation = load_generation.expect("dashboard overview loader generation should be assigned");
        let payload_started = Instant::now();
        let mut load_guard = DashboardOverviewLoadGuard::new(cache_handle.clone(), load_generation);
        let result = build_dashboard_overview_payload(state).await;
        tavily_hikari::emit_perf_log(
            tavily_hikari::DbLogStatus::Info,
            "admin_read",
            "dashboard_overview_phase",
            payload_started.elapsed(),
            tavily_hikari::PerfLogScope {
                route: Some("/api/dashboard/overview"),
                scope: Some("dashboard"),
                phase: Some("overview_payload_build"),
                degraded: Some("rebuilt"),
                ..Default::default()
            },
        );
        let mut cache = cache_handle.lock().await;
        if cache.loading_generation != load_generation {
            tracing::warn!(
                component = "admin_read",
                event = "dashboard_overview_loader_superseded",
                loader_generation = load_generation,
                current_generation = cache.loading_generation,
                "dashboard overview loader finished after a newer loader took ownership"
            );
            load_guard.disarm();
            return result;
        }

        cache.loading = false;
        cache.loading_started_at = None;
        if let Ok(snapshot) = result.as_ref() {
            cache.cached = Some(CachedDashboardOverviewSnapshot {
                snapshot: snapshot.clone(),
                freshness: snapshot.freshness.clone(),
            });
            tavily_hikari::emit_low_memory_protection_decision(
                "admin_read",
                tavily_hikari::PerfLogScope {
                    route: Some("dashboard_shared_snapshot"),
                    scope: Some("dashboard"),
                    phase: Some("cache_serve"),
                    row_count: Some(snapshot.payload.recent_logs.len()),
                    degraded: Some("rebuilt"),
                    ..Default::default()
                },
            );
            tavily_hikari::emit_perf_log(
                tavily_hikari::DbLogStatus::Info,
                "admin_read",
                "dashboard_snapshot_rebuilt",
                Duration::from_millis(perf.elapsed_ms()),
                tavily_hikari::PerfLogScope {
                    route: Some("dashboard_shared_snapshot"),
                    scope: Some("dashboard"),
                    phase: Some("cache_serve"),
                    row_count: Some(snapshot.payload.recent_logs.len()),
                    degraded: Some("rebuilt"),
                    ..Default::default()
                },
            );
        }
        cache.notify.notify_waiters();
        load_guard.disarm();
        return result;
    }
}

async fn build_snapshot_event(state: &Arc<AppState>) -> Option<(Event, SummarySig)> {
    let overview = load_dashboard_overview_snapshot(state).await.ok()?;
    let payload = DashboardSnapshot {
        keys: overview.payload.exhausted_keys.clone(),
        logs: overview.payload.recent_logs.clone(),
        overview: overview.payload,
    };

    let serialize_started = Instant::now();
    let json = serde_json::to_string(&payload).ok()?;
    tavily_hikari::emit_perf_log(
        tavily_hikari::DbLogStatus::Info,
        "admin_read",
        "dashboard_overview_phase",
        serialize_started.elapsed(),
        tavily_hikari::PerfLogScope {
            route: Some("/api/dashboard/overview"),
            scope: Some("dashboard"),
            phase: Some("overview_serialize"),
            row_count: Some(payload.logs.len()),
            degraded: Some("snapshot_sse"),
            ..Default::default()
        },
    );
    Some((
        Event::default().event("snapshot").data(json),
        SummarySig {
            freshness: overview.freshness,
        },
    ))
}

async fn compute_signatures(
    state: &Arc<AppState>,
) -> Result<(Option<SummarySig>, Option<i64>), ()> {
    let freshness = compute_dashboard_overview_freshness(state).await.map_err(|_| ())?;
    let latest_id = freshness.latest_request_log_id;
    let sig: Option<SummarySig> = Some(SummarySig { freshness });
    Ok((sig, latest_id))
}

// ---- Jobs listing ----
