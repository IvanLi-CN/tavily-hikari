#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyQuarantineView {
    source: String,
    reason_code: String,
    reason_summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason_detail: Option<String>,
    created_at: i64,
}

#[derive(Debug, Serialize)]
struct ApiKeyView {
    id: String,
    status: String,
    group: Option<String>,
    registration_ip: Option<String>,
    registration_region: Option<String>,
    status_changed_at: Option<i64>,
    last_used_at: Option<i64>,
    deleted_at: Option<i64>,
    quota_limit: Option<i64>,
    quota_remaining: Option<i64>,
    quota_synced_at: Option<i64>,
    total_requests: i64,
    success_count: i64,
    error_count: i64,
    quota_exhausted_count: i64,
    quarantine: Option<ApiKeyQuarantineView>,
}

#[derive(Debug, Serialize)]
struct ApiKeySecretView {
    api_key: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StickyUserIdentityView {
    user_id: String,
    display_name: Option<String>,
    username: Option<String>,
    active: bool,
    last_login_at: Option<i64>,
    token_count: i64,
}

impl From<&AdminUserIdentity> for StickyUserIdentityView {
    fn from(value: &AdminUserIdentity) -> Self {
        Self {
            user_id: value.user_id.clone(),
            display_name: value.display_name.clone(),
            username: value.username.clone(),
            active: value.active,
            last_login_at: value.last_login_at,
            token_count: value.token_count,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StickyCreditsWindowView {
    success_credits: i64,
    failure_credits: i64,
}

impl From<&StickyCreditsWindow> for StickyCreditsWindowView {
    fn from(value: &StickyCreditsWindow) -> Self {
        Self {
            success_credits: value.success_credits,
            failure_credits: value.failure_credits,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StickyUsersWindowsView {
    yesterday: StickyCreditsWindowView,
    today: StickyCreditsWindowView,
    month: StickyCreditsWindowView,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StickyUserDailyBucketView {
    bucket_start: i64,
    bucket_end: i64,
    success_credits: i64,
    failure_credits: i64,
}

impl From<ApiKeyUserUsageBucket> for StickyUserDailyBucketView {
    fn from(value: ApiKeyUserUsageBucket) -> Self {
        Self {
            bucket_start: value.bucket_start,
            bucket_end: value.bucket_end,
            success_credits: value.success_credits,
            failure_credits: value.failure_credits,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StickyUserView {
    user: StickyUserIdentityView,
    last_success_at: i64,
    windows: StickyUsersWindowsView,
    daily_buckets: Vec<StickyUserDailyBucketView>,
}

impl From<ApiKeyStickyUser> for StickyUserView {
    fn from(value: ApiKeyStickyUser) -> Self {
        Self {
            user: StickyUserIdentityView::from(&value.user),
            last_success_at: value.last_success_at,
            windows: StickyUsersWindowsView {
                yesterday: StickyCreditsWindowView::from(&value.yesterday),
                today: StickyCreditsWindowView::from(&value.today),
                month: StickyCreditsWindowView::from(&value.month),
            },
            daily_buckets: value
                .daily_buckets
                .into_iter()
                .map(StickyUserDailyBucketView::from)
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PaginatedStickyUsersView {
    items: Vec<StickyUserView>,
    total: i64,
    page: i64,
    per_page: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StickyNodeView {
    role: String,
    key: String,
    source: String,
    display_name: String,
    endpoint_url: Option<String>,
    weight: f64,
    available: bool,
    last_error: Option<String>,
    penalized: bool,
    primary_assignment_count: i64,
    secondary_assignment_count: i64,
    stats: ForwardProxyStatsResponse,
    last24h: Vec<ForwardProxyHourlyBucketResponse>,
    weight24h: Vec<ForwardProxyWeightHourlyBucketResponse>,
}

impl From<ApiKeyStickyNode> for StickyNodeView {
    fn from(value: ApiKeyStickyNode) -> Self {
        Self {
            role: value.role.to_string(),
            key: value.node.key,
            source: value.node.source,
            display_name: value.node.display_name,
            endpoint_url: value.node.endpoint_url,
            weight: value.node.weight,
            available: value.node.available,
            last_error: value.node.last_error,
            penalized: value.node.penalized,
            primary_assignment_count: value.node.primary_assignment_count,
            secondary_assignment_count: value.node.secondary_assignment_count,
            stats: value.node.stats,
            last24h: value.node.last24h,
            weight24h: value.node.weight24h,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StickyNodesView {
    range_start: String,
    range_end: String,
    bucket_seconds: i64,
    nodes: Vec<StickyNodeView>,
}

#[derive(Debug, Serialize)]
struct RequestLogView {
    id: i64,
    key_id: Option<String>,
    auth_token_id: Option<String>,
    method: String,
    path: String,
    query: Option<String>,
    http_status: Option<i64>,
    mcp_status: Option<i64>,
    business_credits: Option<i64>,
    request_kind_key: String,
    request_kind_label: String,
    request_kind_detail: Option<String>,
    result_status: String,
    created_at: i64,
    error_message: Option<String>,
    failure_kind: Option<String>,
    key_effect_code: String,
    key_effect_summary: Option<String>,
    request_body: Option<String>,
    response_body: Option<String>,
    forwarded_headers: Vec<String>,
    dropped_headers: Vec<String>,
    #[serde(rename = "operationalClass")]
    operational_class: String,
    #[serde(rename = "requestKindProtocolGroup")]
    request_kind_protocol_group: String,
    #[serde(rename = "requestKindBillingGroup")]
    request_kind_billing_group: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JobLogView {
    id: i64,
    job_type: String,
    key_id: Option<String>,
    key_group: Option<String>,
    status: String,
    attempt: i64,
    message: Option<String>,
    started_at: i64,
    finished_at: Option<i64>,
}

#[derive(Debug, Serialize)]
struct SummaryView {
    total_requests: i64,
    success_count: i64,
    error_count: i64,
    quota_exhausted_count: i64,
    active_keys: i64,
    exhausted_keys: i64,
    quarantined_keys: i64,
    last_activity: Option<i64>,
    total_quota_limit: i64,
    total_quota_remaining: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicMetricsView {
    monthly_success: i64,
    daily_success: i64,
}

// ---- Access Token views ----
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenOwnerView {
    user_id: String,
    display_name: Option<String>,
    username: Option<String>,
}

impl From<&tavily_hikari::AdminUserIdentity> for TokenOwnerView {
    fn from(user: &tavily_hikari::AdminUserIdentity) -> Self {
        Self {
            user_id: user.user_id.clone(),
            display_name: user.display_name.clone(),
            username: user.username.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
struct AuthTokenView {
    id: String,
    enabled: bool,
    note: Option<String>,
    group: Option<String>,
    owner: Option<TokenOwnerView>,
    total_requests: i64,
    created_at: i64,
    last_used_at: Option<i64>,
    quota_state: String,
    quota_hourly_used: i64,
    quota_hourly_limit: i64,
    quota_daily_used: i64,
    quota_daily_limit: i64,
    quota_monthly_used: i64,
    quota_monthly_limit: i64,
    quota_hourly_reset_at: Option<i64>,
    quota_daily_reset_at: Option<i64>,
    quota_monthly_reset_at: Option<i64>,
}

impl AuthTokenView {
    fn from_token_and_owner(
        t: AuthToken,
        owner: Option<&tavily_hikari::AdminUserIdentity>,
    ) -> Self {
        let (
            quota_state,
            quota_hourly_used,
            quota_hourly_limit,
            quota_daily_used,
            quota_daily_limit,
            quota_monthly_used,
            quota_monthly_limit,
        ) = if let Some(quota) = t.quota {
            (
                quota.state_key().to_string(),
                quota.hourly_used,
                quota.hourly_limit,
                quota.daily_used,
                quota.daily_limit,
                quota.monthly_used,
                quota.monthly_limit,
            )
        } else {
            (
                "normal".to_string(),
                0,
                effective_token_hourly_limit(),
                0,
                effective_token_daily_limit(),
                0,
                effective_token_monthly_limit(),
            )
        };
        Self {
            id: t.id,
            enabled: t.enabled,
            note: t.note,
            group: t.group_name,
            owner: owner.map(TokenOwnerView::from),
            total_requests: t.total_requests,
            created_at: t.created_at,
            last_used_at: t.last_used_at,
            quota_state,
            quota_hourly_used,
            quota_hourly_limit,
            quota_daily_used,
            quota_daily_limit,
            quota_monthly_used,
            quota_monthly_limit,
            quota_hourly_reset_at: t.quota_hourly_reset_at,
            quota_daily_reset_at: t.quota_daily_reset_at,
            quota_monthly_reset_at: t.quota_monthly_reset_at,
        }
    }
}

impl From<AuthToken> for AuthTokenView {
    fn from(t: AuthToken) -> Self {
        Self::from_token_and_owner(t, None)
    }
}

#[derive(Debug, Serialize)]
struct AuthTokenSecretView {
    token: String,
}

// ---- Token Detail views ----
#[derive(Debug, Serialize)]
struct TokenSummaryView {
    total_requests: i64,
    success_count: i64,
    error_count: i64,
    quota_exhausted_count: i64,
    last_activity: Option<i64>,
}

impl From<TokenSummary> for TokenSummaryView {
    fn from(s: TokenSummary) -> Self {
        Self {
            total_requests: s.total_requests,
            success_count: s.success_count,
            error_count: s.error_count,
            quota_exhausted_count: s.quota_exhausted_count,
            last_activity: s.last_activity,
        }
    }
}

#[derive(Debug, Serialize)]
struct TokenLogView {
    id: i64,
    key_id: Option<String>,
    method: String,
    path: String,
    query: Option<String>,
    http_status: Option<i64>,
    mcp_status: Option<i64>,
    business_credits: Option<i64>,
    request_kind_key: String,
    request_kind_label: String,
    request_kind_detail: Option<String>,
    result_status: String,
    error_message: Option<String>,
    failure_kind: Option<String>,
    key_effect_code: String,
    key_effect_summary: Option<String>,
    created_at: i64,
    #[serde(rename = "operationalClass")]
    operational_class: String,
    #[serde(rename = "requestKindProtocolGroup")]
    request_kind_protocol_group: String,
    #[serde(rename = "requestKindBillingGroup")]
    request_kind_billing_group: String,
}

impl From<TokenLogRecord> for TokenLogView {
    fn from(r: TokenLogRecord) -> Self {
        let request_kind_protocol_group = token_request_kind_protocol_group(&r.request_kind_key);
        let request_kind_billing_group =
            token_request_kind_billing_group_for_token_log(
                &r.request_kind_key,
                r.counts_business_quota,
            );
        let operational_class = operational_class_for_token_log(
            &r.request_kind_key,
            &r.result_status,
            r.failure_kind.as_deref(),
            r.counts_business_quota,
        );
        Self {
            id: r.id,
            key_id: r.key_id,
            method: r.method,
            path: r.path,
            query: r.query,
            http_status: r.http_status,
            mcp_status: r.mcp_status,
            business_credits: r.business_credits,
            request_kind_key: r.request_kind_key,
            request_kind_label: r.request_kind_label,
            request_kind_detail: r.request_kind_detail,
            result_status: r.result_status,
            error_message: r.error_message,
            failure_kind: r.failure_kind,
            key_effect_code: r.key_effect_code,
            key_effect_summary: r.key_effect_summary,
            created_at: r.created_at,
            operational_class: operational_class.to_string(),
            request_kind_protocol_group: request_kind_protocol_group.to_string(),
            request_kind_billing_group: request_kind_billing_group.to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
struct TokenRequestKindOptionView {
    key: String,
    label: String,
    protocol_group: String,
    billing_group: String,
    count: i64,
}

impl From<TokenRequestKindOption> for TokenRequestKindOptionView {
    fn from(value: TokenRequestKindOption) -> Self {
        Self {
            key: value.key,
            label: value.label,
            protocol_group: value.protocol_group,
            billing_group: value.billing_group,
            count: value.count,
        }
    }
}

#[derive(Debug, Deserialize)]
struct CreateTokenRequest {
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LogsQuery {
    page: Option<i64>,
    per_page: Option<i64>,
    result: Option<String>,
    key_effect: Option<String>,
    auth_token_id: Option<String>,
    key_id: Option<String>,
    operational_class: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LogFacetOptionView {
    value: String,
    count: i64,
}

impl From<LogFacetOption> for LogFacetOptionView {
    fn from(value: LogFacetOption) -> Self {
        Self {
            value: value.value,
            count: value.count,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestLogFacetsView {
    results: Vec<LogFacetOptionView>,
    key_effects: Vec<LogFacetOptionView>,
    tokens: Vec<LogFacetOptionView>,
    keys: Vec<LogFacetOptionView>,
}

fn normalize_result_status_filter(value: Option<&str>) -> Option<&'static str> {
    match value.map(str::trim) {
        Some(v) if v.eq_ignore_ascii_case("success") => Some("success"),
        Some(v) if v.eq_ignore_ascii_case("error") => Some("error"),
        Some(v) if v.eq_ignore_ascii_case("quota_exhausted") || v.eq_ignore_ascii_case("quota") => {
            Some("quota_exhausted")
        }
        _ => None,
    }
}

fn normalize_key_effect_filter(value: Option<&str>) -> Option<&'static str> {
    match value.map(str::trim) {
        Some(v) if v.eq_ignore_ascii_case("none") => Some("none"),
        Some(v) if v.eq_ignore_ascii_case("quarantined") => Some("quarantined"),
        Some(v) if v.eq_ignore_ascii_case("marked_exhausted") => Some("marked_exhausted"),
        Some(v) if v.eq_ignore_ascii_case("restored_active") => Some("restored_active"),
        Some(v) if v.eq_ignore_ascii_case("cleared_quarantine") => Some("cleared_quarantine"),
        _ => None,
    }
}

fn normalize_optional_filter(value: Option<&str>) -> Option<&str> {
    value.and_then(|raw| {
        let trimmed = raw.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

fn parse_request_kind_filters(raw_query: Option<&str>) -> Vec<String> {
    raw_query
        .map(|query| {
            form_urlencoded::parse(query.as_bytes())
                .filter_map(|(key, value)| {
                    if key == "request_kind" {
                        let trimmed = value.trim();
                        (!trimmed.is_empty()).then(|| canonical_request_kind_key_for_filter(trimmed))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
struct KeyMetricsQuery {
    period: Option<String>,
    since: Option<i64>,
}

async fn get_key_metrics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<KeyMetricsQuery>,
) -> Result<Json<SummaryView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let since = if let Some(since) = q.since {
        since
    } else {
        // fallback by period
        let now = chrono::Local::now();
        let local_midnight_ts = |date: chrono::NaiveDate| -> i64 {
            let naive = date.and_hms_opt(0, 0, 0).expect("valid midnight");
            match chrono::Local.from_local_datetime(&naive) {
                chrono::LocalResult::Single(dt) => dt.with_timezone(&Utc).timestamp(),
                chrono::LocalResult::Ambiguous(dt, _) => dt.with_timezone(&Utc).timestamp(),
                chrono::LocalResult::None => now.with_timezone(&Utc).timestamp(),
            }
        };
        match q.period.as_deref() {
            Some("day") => local_midnight_ts(now.date_naive()),
            Some("week") => {
                let weekday = now.weekday().num_days_from_monday() as i64;
                let start = (now - chrono::Duration::days(weekday)).date_naive();
                local_midnight_ts(start)
            }
            _ => {
                // month default
                let first =
                    chrono::NaiveDate::from_ymd_opt(now.year(), now.month(), 1).expect("valid");
                local_midnight_ts(first)
            }
        }
    };

    state
        .proxy
        .key_summary_since(&id, since)
        .await
        .map(|s| Json(s.into()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Debug, Deserialize)]
struct KeyLogsQuery {
    limit: Option<usize>,
    since: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct KeyLogsPageQuery {
    page: Option<i64>,
    per_page: Option<i64>,
    since: Option<i64>,
    result: Option<String>,
    key_effect: Option<String>,
    auth_token_id: Option<String>,
}

async fn get_key_logs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<KeyLogsQuery>,
) -> Result<Json<Vec<RequestLogView>>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let limit = q.limit.unwrap_or(DEFAULT_LOG_LIMIT).clamp(1, 500);
    state
        .proxy
        .key_recent_logs(&id, limit, q.since)
        .await
        .map(|logs| Json(logs.into_iter().map(RequestLogView::from).collect()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct KeyLogsPageView {
    items: Vec<RequestLogView>,
    total: i64,
    page: i64,
    per_page: i64,
    request_kind_options: Vec<TokenRequestKindOptionView>,
    facets: RequestLogFacetsView,
}

async fn get_key_logs_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    RawQuery(raw_query): RawQuery,
    Query(q): Query<KeyLogsPageQuery>,
) -> Result<Json<KeyLogsPageView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let page = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(20).clamp(1, 200);
    let request_kinds = parse_request_kind_filters(raw_query.as_deref());
    let result_status = normalize_result_status_filter(q.result.as_deref());
    let key_effect_code = normalize_key_effect_filter(q.key_effect.as_deref());
    if result_status.is_some() && key_effect_code.is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let auth_token_id = normalize_optional_filter(q.auth_token_id.as_deref());

    state
        .proxy
        .key_logs_page(
            &id,
            q.since,
            &request_kinds,
            result_status,
            key_effect_code,
            auth_token_id,
            page,
            per_page,
        )
        .await
        .map(|logs| {
            Json(KeyLogsPageView {
                items: logs.items.into_iter().map(RequestLogView::from).collect(),
                total: logs.total,
                page,
                per_page,
                request_kind_options: logs
                    .request_kind_options
                    .into_iter()
                    .map(TokenRequestKindOptionView::from)
                    .collect(),
                facets: RequestLogFacetsView {
                    results: logs
                        .facets
                        .results
                        .into_iter()
                        .map(LogFacetOptionView::from)
                        .collect(),
                    key_effects: logs
                        .facets
                        .key_effects
                        .into_iter()
                        .map(LogFacetOptionView::from)
                        .collect(),
                    tokens: logs
                        .facets
                        .tokens
                        .into_iter()
                        .map(LogFacetOptionView::from)
                        .collect(),
                    keys: logs
                        .facets
                        .keys
                        .into_iter()
                        .map(LogFacetOptionView::from)
                        .collect(),
                },
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Debug, Deserialize)]
struct StickyUsersQuery {
    page: Option<i64>,
    per_page: Option<i64>,
}

async fn get_key_sticky_users(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<StickyUsersQuery>,
) -> Result<Json<PaginatedStickyUsersView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    state
        .proxy
        .key_sticky_users_paged(&id, q.page.unwrap_or(1), q.per_page.unwrap_or(20))
        .await
        .map(|result| {
            Json(PaginatedStickyUsersView {
                items: result.items.into_iter().map(StickyUserView::from).collect(),
                total: result.total,
                page: result.page,
                per_page: result.per_page,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn get_key_sticky_nodes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<StickyNodesView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    state
        .proxy
        .key_sticky_nodes(&id)
        .await
        .map(|result| {
            Json(StickyNodesView {
                range_start: result.range_start,
                range_end: result.range_end,
                bucket_seconds: result.bucket_seconds,
                nodes: result.nodes.into_iter().map(StickyNodeView::from).collect(),
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

// ---- Token detail endpoints ----

#[derive(Debug, Deserialize)]
struct TokenMetricsQuery {
    period: Option<String>,
    since: Option<String>,
    until: Option<String>,
}

async fn get_token_metrics(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(q): Query<TokenMetricsQuery>,
) -> Result<Json<TokenSummaryView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let since = q
        .since
        .as_deref()
        .and_then(parse_iso_timestamp)
        .unwrap_or_else(|| default_since(q.period.as_deref()));
    let until = q
        .until
        .as_deref()
        .and_then(parse_iso_timestamp)
        .unwrap_or_else(|| default_until(q.period.as_deref(), since));

    state
        .proxy
        .token_summary_since(&id, since, Some(until))
        .await
        .map(|s| Json(s.into()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Debug, Deserialize)]
struct TokenLogsQuery {
    limit: Option<usize>,
    before: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TokenHourlyQuery {
    hours: Option<i64>,
}

async fn get_token_logs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(q): Query<TokenLogsQuery>,
) -> Result<Json<Vec<TokenLogView>>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let limit = q.limit.unwrap_or(DEFAULT_LOG_LIMIT).clamp(1, 500);
    state
        .proxy
        .token_recent_logs(&id, limit, q.before)
        .await
        .map(|logs| {
            let mapped: Vec<TokenLogView> = logs
                .into_iter()
                .map(TokenLogView::from)
                .map(|mut v| {
                    if let Some(err) = v.error_message.as_ref() {
                        v.error_message = Some(redact_sensitive(err));
                    }
                    v
                })
                .collect();
            Json(mapped)
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Debug, Deserialize)]
struct TokenLogsPageQuery {
    page: Option<usize>,
    per_page: Option<usize>,
    since: Option<String>,
    until: Option<String>,
    result: Option<String>,
    key_effect: Option<String>,
    key_id: Option<String>,
    operational_class: Option<String>,
}

#[derive(Debug, Serialize)]
struct TokenLogsPageView {
    items: Vec<RequestLogView>,
    page: usize,
    per_page: usize,
    total: i64,
    request_kind_options: Vec<TokenRequestKindOptionView>,
    facets: RequestLogFacetsView,
}

#[derive(Debug, Serialize)]
struct TokenHourlyBucketView {
    bucket_start: i64,
    success_count: i64,
    system_failure_count: i64,
    external_failure_count: i64,
}

#[derive(Debug, Serialize)]
struct TokenUsageBucketView {
    bucket_start: i64,
    success_count: i64,
    system_failure_count: i64,
    external_failure_count: i64,
}

#[derive(Debug, Deserialize)]
struct TokenLeaderboardQuery {
    period: Option<String>,
    focus: Option<String>,
}

#[derive(Debug, Serialize)]
struct TokenLeaderboardItemView {
    id: String,
    enabled: bool,
    note: Option<String>,
    group: Option<String>,
    total_requests: i64,
    last_used_at: Option<i64>,
    quota_state: String,
    // Business quota windows (tools/call)
    quota_hourly_used: i64,
    quota_hourly_limit: i64,
    quota_daily_used: i64,
    quota_daily_limit: i64,
    // Hourly raw request limiter (any authenticated request)
    hourly_any_used: i64,
    hourly_any_limit: i64,
    today_total: i64,
    today_errors: i64,
    today_other: i64,
    month_total: i64,
    month_errors: i64,
    month_other: i64,
    all_total: i64,
    all_errors: i64,
    all_other: i64,
}

async fn get_token_logs_page(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Query(q): Query<TokenLogsPageQuery>,
) -> Result<Json<TokenLogsPageView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let page = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(20).clamp(1, 200);
    let since = q
        .since
        .as_deref()
        .and_then(parse_iso_timestamp)
        .unwrap_or_else(|| default_since(Some("month")));
    let until = q
        .until
        .as_deref()
        .and_then(parse_iso_timestamp)
        .unwrap_or_else(|| default_until(Some("month"), since));
    if until <= since {
        return Err(StatusCode::BAD_REQUEST);
    }
    let request_kinds = parse_request_kind_filters(raw_query.as_deref());
    let result_status = normalize_result_status_filter(q.result.as_deref());
    let key_effect_code = normalize_key_effect_filter(q.key_effect.as_deref());
    if result_status.is_some() && key_effect_code.is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let key_id = normalize_optional_filter(q.key_id.as_deref());
    let operational_class = normalize_operational_class_filter(q.operational_class.as_deref());
    state
        .proxy
        .token_logs_page(
            &id,
            page,
            per_page,
            since,
            Some(until),
            &request_kinds,
            result_status,
            key_effect_code,
            key_id,
            operational_class,
        )
        .await
        .map(|logs| {
            let mapped: Vec<RequestLogView> = logs
                .items
                .into_iter()
                .map(|record| RequestLogView::from_token_record(record, &id))
                .map(|mut v| {
                    if let Some(err) = v.error_message.as_ref() {
                        v.error_message = Some(redact_sensitive(err));
                    }
                    v
                })
                .collect();
            Json(TokenLogsPageView {
                items: mapped,
                page,
                per_page,
                total: logs.total,
                request_kind_options: logs
                    .request_kind_options
                    .into_iter()
                    .map(TokenRequestKindOptionView::from)
                    .collect(),
                facets: RequestLogFacetsView {
                    results: logs
                        .facets
                        .results
                        .into_iter()
                        .map(LogFacetOptionView::from)
                        .collect(),
                    key_effects: logs
                        .facets
                        .key_effects
                        .into_iter()
                        .map(LogFacetOptionView::from)
                        .collect(),
                    tokens: logs
                        .facets
                        .tokens
                        .into_iter()
                        .map(LogFacetOptionView::from)
                        .collect(),
                    keys: logs
                        .facets
                        .keys
                        .into_iter()
                        .map(LogFacetOptionView::from)
                        .collect(),
                },
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn get_token_hourly_breakdown(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(q): Query<TokenHourlyQuery>,
) -> Result<Json<Vec<TokenHourlyBucketView>>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let hours = q.hours.unwrap_or(25);
    state
        .proxy
        .token_hourly_breakdown(&id, hours)
        .await
        .map(|buckets| {
            Json(
                buckets
                    .into_iter()
                    .map(
                        |TokenHourlyBucket {
                             bucket_start,
                             success_count,
                             system_failure_count,
                             external_failure_count,
                         }| TokenHourlyBucketView {
                            bucket_start,
                            success_count,
                            system_failure_count,
                            external_failure_count,
                        },
                    )
                    .collect(),
            )
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Debug, Deserialize)]
struct UsageSeriesQuery {
    since: Option<String>,
    until: Option<String>,
    bucket_secs: Option<i64>,
}

async fn get_token_usage_series(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(q): Query<UsageSeriesQuery>,
) -> Result<Json<Vec<TokenUsageBucketView>>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let now = Utc::now().timestamp();
    let until = q
        .until
        .as_deref()
        .and_then(parse_iso_timestamp)
        .unwrap_or(now);
    let default_since = until - ChronoDuration::hours(25).num_seconds();
    let since = q
        .since
        .as_deref()
        .and_then(parse_iso_timestamp)
        .unwrap_or(default_since);
    if until <= since {
        return Err(StatusCode::BAD_REQUEST);
    }
    let bucket_secs = q
        .bucket_secs
        .unwrap_or(ChronoDuration::hours(1).num_seconds());
    state
        .proxy
        .token_usage_series(&id, since, until, bucket_secs)
        .await
        .map(|series| {
            Json(
                series
                    .into_iter()
                    .map(
                        |TokenUsageBucket {
                             bucket_start,
                             success_count,
                             system_failure_count,
                             external_failure_count,
                         }| TokenUsageBucketView {
                            bucket_start,
                            success_count,
                            system_failure_count,
                            external_failure_count,
                        },
                    )
                    .collect(),
            )
        })
        .map_err(|err| match err {
            ProxyError::Other(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        })
}

async fn get_token_leaderboard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<TokenLeaderboardQuery>,
) -> Result<Json<Vec<TokenLeaderboardItemView>>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    let now = Utc::now();
    let day_since = start_of_day_dt(now).timestamp();
    let month_since = start_of_month_dt(now).timestamp();

    let period = match q.period.as_deref() {
        Some("day") | None => "day",
        Some("month") => "month",
        Some("all") => "all",
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    let focus = match q.focus.as_deref() {
        Some("usage") | None => "usage",
        Some("errors") => "errors",
        Some("other") => "other",
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    let tokens = state
        .proxy
        .list_access_tokens()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let token_ids: Vec<String> = tokens.iter().map(|t| t.id.clone()).collect();
    let hourly_any_map = state
        .proxy
        .token_hourly_any_snapshot(&token_ids)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut items: Vec<TokenLeaderboardItemView> = Vec::with_capacity(tokens.len());

    for token in tokens {
        // summaries
        let today = state
            .proxy
            .token_summary_since(&token.id, day_since, None)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let month = state
            .proxy
            .token_summary_since(&token.id, month_since, None)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let all = state
            .proxy
            .token_summary_since(&token.id, 0, None)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let other_today = (today.total_requests - today.success_count - today.error_count).max(0);
        let other_month = (month.total_requests - month.success_count - month.error_count).max(0);
        let other_all = (all.total_requests - all.success_count - all.error_count).max(0);

        // quota snapshot
        let quota_verdict = match token.quota {
            Some(ref v) => Some(v.clone()),
            None => state
                .proxy
                .token_quota_snapshot(&token.id)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .clone(),
        };
        let (hour_used, hour_limit, day_used, day_limit) = quota_verdict
            .as_ref()
            .map(|q| (q.hourly_used, q.hourly_limit, q.daily_used, q.daily_limit))
            .unwrap_or((
                0,
                effective_token_hourly_limit(),
                0,
                effective_token_daily_limit(),
            ));
        let quota_state = quota_verdict
            .as_ref()
            .and_then(|q| q.exceeded_window)
            .map(|w| w.as_str().to_string())
            .unwrap_or_else(|| "normal".to_string());

        let (hourly_any_used, hourly_any_limit) = hourly_any_map
            .get(&token.id)
            .map(|v| (v.hourly_used, v.hourly_limit))
            .unwrap_or((0, effective_token_hourly_request_limit()));

        let item = TokenLeaderboardItemView {
            id: token.id.clone(),
            enabled: token.enabled,
            note: token.note.clone(),
            group: token.group_name.clone(),
            total_requests: all.total_requests,
            last_used_at: all.last_activity,
            quota_state,
            quota_hourly_used: hour_used,
            quota_hourly_limit: hour_limit,
            quota_daily_used: day_used,
            quota_daily_limit: day_limit,
            hourly_any_used,
            hourly_any_limit,
            today_total: today.total_requests,
            today_errors: today.error_count,
            today_other: other_today,
            month_total: month.total_requests,
            month_errors: month.error_count,
            month_other: other_month,
            all_total: all.total_requests,
            all_errors: all.error_count,
            all_other: other_all,
        };
        items.push(item);
    }

    let metric = |it: &TokenLeaderboardItemView, p: &str, f: &str| -> i64 {
        match (p, f) {
            ("day", "usage") => it.today_total,
            ("day", "errors") => it.today_errors,
            ("day", "other") => it.today_other,
            ("month", "usage") => it.month_total,
            ("month", "errors") => it.month_errors,
            ("month", "other") => it.month_other,
            ("all", "usage") => it.all_total,
            ("all", "errors") => it.all_errors,
            ("all", "other") => it.all_other,
            _ => 0,
        }
    };

    items.sort_by(|a, b| {
        metric(b, period, focus)
            .cmp(&metric(a, period, focus))
            .then_with(|| b.total_requests.cmp(&a.total_requests))
    });

    items.truncate(50);

    Ok(Json(items))
}
