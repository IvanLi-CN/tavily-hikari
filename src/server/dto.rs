#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyQuarantineView {
    source: String,
    reason_code: String,
    reason_summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason_detail: Option<String>,
    created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
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
    binding_effect_code: String,
    binding_effect_summary: Option<String>,
    selection_effect_code: String,
    selection_effect_summary: Option<String>,
    gateway_mode: Option<String>,
    experiment_variant: Option<String>,
    proxy_session_id: Option<String>,
    routing_subject_hash: Option<String>,
    upstream_operation: Option<String>,
    fallback_reason: Option<String>,
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
struct RequestLogBodiesView {
    request_body: Option<String>,
    response_body: Option<String>,
}

impl From<RequestLogBodiesRecord> for RequestLogBodiesView {
    fn from(value: RequestLogBodiesRecord) -> Self {
        Self {
            request_body: value.request_body.as_deref().and_then(decode_body),
            response_body: value.response_body.as_deref().and_then(decode_body),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
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
#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
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
    binding_effect_code: String,
    binding_effect_summary: Option<String>,
    selection_effect_code: String,
    selection_effect_summary: Option<String>,
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
        let result_status =
            display_result_status_for_request_kind(&r.request_kind_key, &r.result_status);
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
            result_status,
            error_message: r.error_message,
            failure_kind: r.failure_kind,
            key_effect_code: r.key_effect_code,
            key_effect_summary: r.key_effect_summary,
            binding_effect_code: r.binding_effect_code,
            binding_effect_summary: r.binding_effect_summary,
            selection_effect_code: r.selection_effect_code,
            selection_effect_summary: r.selection_effect_summary,
            created_at: r.created_at,
            operational_class: operational_class.to_string(),
            request_kind_protocol_group: request_kind_protocol_group.to_string(),
            request_kind_billing_group: request_kind_billing_group.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
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
    binding_effect: Option<String>,
    selection_effect: Option<String>,
    auth_token_id: Option<String>,
    key_id: Option<String>,
    operational_class: Option<String>,
    include_bodies: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct CursorLogsQuery {
    limit: Option<i64>,
    cursor: Option<String>,
    direction: Option<String>,
    result: Option<String>,
    key_effect: Option<String>,
    binding_effect: Option<String>,
    selection_effect: Option<String>,
    auth_token_id: Option<String>,
    key_id: Option<String>,
    operational_class: Option<String>,
    since: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TokenCursorLogsQuery {
    limit: Option<i64>,
    cursor: Option<String>,
    direction: Option<String>,
    since: Option<String>,
    until: Option<String>,
    result: Option<String>,
    key_effect: Option<String>,
    binding_effect: Option<String>,
    selection_effect: Option<String>,
    key_id: Option<String>,
    operational_class: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AlertsQuery {
    page: Option<i64>,
    per_page: Option<i64>,
    #[serde(rename = "type")]
    alert_type: Option<String>,
    since: Option<String>,
    until: Option<String>,
    user_id: Option<String>,
    token_id: Option<String>,
    key_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
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
    binding_effects: Vec<LogFacetOptionView>,
    selection_effects: Vec<LogFacetOptionView>,
    tokens: Vec<LogFacetOptionView>,
    keys: Vec<LogFacetOptionView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestLogsCursorPageView {
    items: Vec<RequestLogView>,
    page_size: i64,
    next_cursor: Option<String>,
    prev_cursor: Option<String>,
    has_older: bool,
    has_newer: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestLogsCatalogView {
    retention_days: i64,
    request_kind_options: Vec<TokenRequestKindOptionView>,
    facets: RequestLogFacetsView,
}

impl From<RequestLogsCatalog> for RequestLogsCatalogView {
    fn from(value: RequestLogsCatalog) -> Self {
        Self {
            retention_days: value.retention_days,
            request_kind_options: value
                .request_kind_options
                .into_iter()
                .map(TokenRequestKindOptionView::from)
                .collect(),
            facets: RequestLogFacetsView {
                results: value
                    .facets
                    .results
                    .into_iter()
                    .map(LogFacetOptionView::from)
                    .collect(),
                key_effects: value
                    .facets
                    .key_effects
                    .into_iter()
                    .map(LogFacetOptionView::from)
                    .collect(),
                binding_effects: value
                    .facets
                    .binding_effects
                    .into_iter()
                    .map(LogFacetOptionView::from)
                    .collect(),
                selection_effects: value
                    .facets
                    .selection_effects
                    .into_iter()
                    .map(LogFacetOptionView::from)
                    .collect(),
                tokens: value
                    .facets
                    .tokens
                    .into_iter()
                    .map(LogFacetOptionView::from)
                    .collect(),
                keys: value
                    .facets
                    .keys
                    .into_iter()
                    .map(LogFacetOptionView::from)
                    .collect(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AlertFacetOptionView {
    value: String,
    label: String,
    count: i64,
}

impl From<tavily_hikari::AlertFacetOption> for AlertFacetOptionView {
    fn from(value: tavily_hikari::AlertFacetOption) -> Self {
        Self {
            value: value.value,
            label: value.label,
            count: value.count,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AlertEntityRefView {
    id: String,
    label: String,
}

impl From<tavily_hikari::AlertEntityRef> for AlertEntityRefView {
    fn from(value: tavily_hikari::AlertEntityRef) -> Self {
        Self {
            id: value.id,
            label: value.label,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AlertUserView {
    user_id: String,
    display_name: Option<String>,
    username: Option<String>,
}

impl From<tavily_hikari::AlertUserRef> for AlertUserView {
    fn from(value: tavily_hikari::AlertUserRef) -> Self {
        Self {
            user_id: value.user_id,
            display_name: value.display_name,
            username: value.username,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AlertRequestRefView {
    id: i64,
    method: String,
    path: String,
    query: Option<String>,
}

impl From<tavily_hikari::AlertRequestRef> for AlertRequestRefView {
    fn from(value: tavily_hikari::AlertRequestRef) -> Self {
        Self {
            id: value.id,
            method: value.method,
            path: value.path,
            query: value.query,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AlertSourceView {
    kind: String,
    id: String,
}

impl From<tavily_hikari::AlertSourceRef> for AlertSourceView {
    fn from(value: tavily_hikari::AlertSourceRef) -> Self {
        Self {
            kind: value.kind,
            id: value.id,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AlertTypeCountView {
    #[serde(rename = "type")]
    alert_type: String,
    count: i64,
}

impl From<tavily_hikari::AlertTypeCount> for AlertTypeCountView {
    fn from(value: tavily_hikari::AlertTypeCount) -> Self {
        Self {
            alert_type: value.alert_type,
            count: value.count,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AlertRequestKindView {
    key: String,
    label: String,
    detail: Option<String>,
}

impl From<tavily_hikari::TokenRequestKind> for AlertRequestKindView {
    fn from(value: tavily_hikari::TokenRequestKind) -> Self {
        Self {
            key: value.key,
            label: value.label,
            detail: value.detail,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AlertEventView {
    id: String,
    #[serde(rename = "type")]
    alert_type: String,
    title: String,
    summary: String,
    occurred_at: i64,
    subject_kind: String,
    subject_id: String,
    subject_label: String,
    user: Option<AlertUserView>,
    token: Option<AlertEntityRefView>,
    key: Option<AlertEntityRefView>,
    request: Option<AlertRequestRefView>,
    request_kind: Option<AlertRequestKindView>,
    failure_kind: Option<String>,
    result_status: Option<String>,
    error_message: Option<String>,
    reason_code: Option<String>,
    reason_summary: Option<String>,
    reason_detail: Option<String>,
    source: AlertSourceView,
}

impl From<tavily_hikari::AlertEventRecord> for AlertEventView {
    fn from(value: tavily_hikari::AlertEventRecord) -> Self {
        Self {
            id: value.id,
            alert_type: value.alert_type,
            title: value.title,
            summary: value.summary,
            occurred_at: value.occurred_at,
            subject_kind: value.subject_kind,
            subject_id: value.subject_id,
            subject_label: value.subject_label,
            user: value.user.map(AlertUserView::from),
            token: value.token.map(AlertEntityRefView::from),
            key: value.key.map(AlertEntityRefView::from),
            request: value.request.map(AlertRequestRefView::from),
            request_kind: value.request_kind.map(AlertRequestKindView::from),
            failure_kind: value.failure_kind,
            result_status: value.result_status,
            error_message: value.error_message,
            reason_code: value.reason_code,
            reason_summary: value.reason_summary,
            reason_detail: value.reason_detail,
            source: AlertSourceView::from(value.source),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AlertGroupView {
    id: String,
    #[serde(rename = "type")]
    alert_type: String,
    subject_kind: String,
    subject_id: String,
    subject_label: String,
    user: Option<AlertUserView>,
    token: Option<AlertEntityRefView>,
    key: Option<AlertEntityRefView>,
    request_kind: Option<AlertRequestKindView>,
    count: i64,
    first_seen: i64,
    last_seen: i64,
    latest_event: AlertEventView,
}

impl From<tavily_hikari::AlertGroupRecord> for AlertGroupView {
    fn from(value: tavily_hikari::AlertGroupRecord) -> Self {
        Self {
            id: value.id,
            alert_type: value.alert_type,
            subject_kind: value.subject_kind,
            subject_id: value.subject_id,
            subject_label: value.subject_label,
            user: value.user.map(AlertUserView::from),
            token: value.token.map(AlertEntityRefView::from),
            key: value.key.map(AlertEntityRefView::from),
            request_kind: value.request_kind.map(AlertRequestKindView::from),
            count: value.count,
            first_seen: value.first_seen,
            last_seen: value.last_seen,
            latest_event: AlertEventView::from(value.latest_event),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PaginatedAlertEventsView {
    items: Vec<AlertEventView>,
    total: i64,
    page: i64,
    per_page: i64,
}

impl From<tavily_hikari::PaginatedAlertEvents> for PaginatedAlertEventsView {
    fn from(value: tavily_hikari::PaginatedAlertEvents) -> Self {
        Self {
            items: value.items.into_iter().map(AlertEventView::from).collect(),
            total: value.total,
            page: value.page,
            per_page: value.per_page,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PaginatedAlertGroupsView {
    items: Vec<AlertGroupView>,
    total: i64,
    page: i64,
    per_page: i64,
}

impl From<tavily_hikari::PaginatedAlertGroups> for PaginatedAlertGroupsView {
    fn from(value: tavily_hikari::PaginatedAlertGroups) -> Self {
        Self {
            items: value.items.into_iter().map(AlertGroupView::from).collect(),
            total: value.total,
            page: value.page,
            per_page: value.per_page,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AlertCatalogView {
    retention_days: i64,
    types: Vec<LogFacetOptionView>,
    request_kind_options: Vec<TokenRequestKindOptionView>,
    users: Vec<AlertFacetOptionView>,
    tokens: Vec<AlertFacetOptionView>,
    keys: Vec<AlertFacetOptionView>,
}

impl From<tavily_hikari::AlertCatalog> for AlertCatalogView {
    fn from(value: tavily_hikari::AlertCatalog) -> Self {
        Self {
            retention_days: value.retention_days,
            types: value.types.into_iter().map(LogFacetOptionView::from).collect(),
            request_kind_options: value
                .request_kind_options
                .into_iter()
                .map(TokenRequestKindOptionView::from)
                .collect(),
            users: value.users.into_iter().map(AlertFacetOptionView::from).collect(),
            tokens: value.tokens.into_iter().map(AlertFacetOptionView::from).collect(),
            keys: value.keys.into_iter().map(AlertFacetOptionView::from).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardRecentAlertsView {
    window_hours: i64,
    total_events: i64,
    grouped_count: i64,
    counts_by_type: Vec<AlertTypeCountView>,
    top_groups: Vec<AlertGroupView>,
}

impl From<tavily_hikari::RecentAlertsSummary> for DashboardRecentAlertsView {
    fn from(value: tavily_hikari::RecentAlertsSummary) -> Self {
        Self {
            window_hours: value.window_hours,
            total_events: value.total_events,
            grouped_count: value.grouped_count,
            counts_by_type: value
                .counts_by_type
                .into_iter()
                .map(AlertTypeCountView::from)
                .collect(),
            top_groups: value.top_groups.into_iter().map(AlertGroupView::from).collect(),
        }
    }
}

fn build_request_logs_cursor_page_view(page: RequestLogsCursorPage) -> RequestLogsCursorPageView {
    RequestLogsCursorPageView {
        items: page
            .items
            .into_iter()
            .map(RequestLogView::from_summary_record)
            .collect(),
        page_size: page.page_size,
        next_cursor: page.next_cursor.as_ref().map(encode_request_logs_cursor),
        prev_cursor: page.prev_cursor.as_ref().map(encode_request_logs_cursor),
        has_older: page.has_older,
        has_newer: page.has_newer,
    }
}

fn build_token_logs_cursor_page_view(page: TokenLogsCursorPage, token_id: &str) -> RequestLogsCursorPageView {
    RequestLogsCursorPageView {
        items: page
            .items
            .into_iter()
            .map(|record| RequestLogView::from_token_record(record, token_id))
            .map(|mut view| {
                if let Some(err) = view.error_message.as_ref() {
                    view.error_message = Some(redact_sensitive(err));
                }
                view
            })
            .collect(),
        page_size: page.page_size,
        next_cursor: page.next_cursor.as_ref().map(encode_request_logs_cursor),
        prev_cursor: page.prev_cursor.as_ref().map(encode_request_logs_cursor),
        has_older: page.has_older,
        has_newer: page.has_newer,
    }
}

fn encode_request_logs_cursor(cursor: &RequestLogsCursor) -> String {
    format!("{}:{}", cursor.created_at, cursor.id)
}

fn parse_request_logs_cursor(value: Option<&str>) -> Result<Option<RequestLogsCursor>, StatusCode> {
    let Some(raw) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let Some((created_at_raw, id_raw)) = raw.split_once(':') else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let created_at = created_at_raw
        .trim()
        .parse::<i64>()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let id = id_raw
        .trim()
        .parse::<i64>()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok(Some(RequestLogsCursor { created_at, id }))
}

fn normalize_request_logs_cursor_direction(
    value: Option<&str>,
) -> Result<RequestLogsCursorDirection, StatusCode> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(RequestLogsCursorDirection::Older),
        Some(value) if value.eq_ignore_ascii_case("older") => Ok(RequestLogsCursorDirection::Older),
        Some(value) if value.eq_ignore_ascii_case("newer") => Ok(RequestLogsCursorDirection::Newer),
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

fn normalize_result_status_filter(value: Option<&str>) -> Option<&'static str> {
    match value.map(str::trim) {
        Some(v) if v.eq_ignore_ascii_case("success") => Some("success"),
        Some(v) if v.eq_ignore_ascii_case("error") => Some("error"),
        Some(v) if v.eq_ignore_ascii_case("neutral") => Some("neutral"),
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
        Some(v) if v.eq_ignore_ascii_case("mcp_session_init_backoff_set") => {
            Some("mcp_session_init_backoff_set")
        }
        Some(v) if v.eq_ignore_ascii_case("mcp_session_retry_waited") => {
            Some("mcp_session_retry_waited")
        }
        Some(v) if v.eq_ignore_ascii_case("mcp_session_retry_scheduled") => {
            Some("mcp_session_retry_scheduled")
        }
        _ => None,
    }
}

fn normalize_binding_effect_filter(value: Option<&str>) -> Option<&'static str> {
    match value.map(str::trim) {
        Some(v) if v.eq_ignore_ascii_case("none") => Some("none"),
        Some(v) if v.eq_ignore_ascii_case("http_project_affinity_bound") => {
            Some("http_project_affinity_bound")
        }
        Some(v) if v.eq_ignore_ascii_case("http_project_affinity_reused") => {
            Some("http_project_affinity_reused")
        }
        Some(v) if v.eq_ignore_ascii_case("http_project_affinity_rebound") => {
            Some("http_project_affinity_rebound")
        }
        _ => None,
    }
}

fn normalize_selection_effect_filter(value: Option<&str>) -> Option<&'static str> {
    match value.map(str::trim) {
        Some(v) if v.eq_ignore_ascii_case("none") => Some("none"),
        Some(v) if v.eq_ignore_ascii_case("mcp_session_init_cooldown_avoided") => {
            Some("mcp_session_init_cooldown_avoided")
        }
        Some(v) if v.eq_ignore_ascii_case("mcp_session_init_rate_limit_avoided") => {
            Some("mcp_session_init_rate_limit_avoided")
        }
        Some(v) if v.eq_ignore_ascii_case("mcp_session_init_pressure_avoided") => {
            Some("mcp_session_init_pressure_avoided")
        }
        Some(v) if v.eq_ignore_ascii_case("http_project_affinity_cooldown_avoided") => {
            Some("http_project_affinity_cooldown_avoided")
        }
        Some(v) if v.eq_ignore_ascii_case("http_project_affinity_rate_limit_avoided") => {
            Some("http_project_affinity_rate_limit_avoided")
        }
        Some(v) if v.eq_ignore_ascii_case("http_project_affinity_pressure_avoided") => {
            Some("http_project_affinity_pressure_avoided")
        }
        _ => None,
    }
}

fn validate_logs_effect_filters(
    result_status: Option<&str>,
    key_effect_code: Option<&str>,
    binding_effect_code: Option<&str>,
    selection_effect_code: Option<&str>,
) -> Result<(), StatusCode> {
    let key_effect_active = key_effect_code.is_some();
    let binding_effect_active = binding_effect_code.is_some();
    let selection_effect_active = selection_effect_code.is_some();
    if key_effect_active && (binding_effect_active || selection_effect_active) {
        return Err(StatusCode::BAD_REQUEST);
    }
    if result_status.is_some()
        && (key_effect_active || binding_effect_active || selection_effect_active)
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(())
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
    binding_effect: Option<String>,
    selection_effect: Option<String>,
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
        .map(|logs| Json(logs.into_iter().map(RequestLogView::from_summary_record).collect()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn get_key_log_details(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((id, log_id)): Path<(String, i64)>,
) -> Result<Json<RequestLogBodiesView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    state
        .proxy
        .key_request_log_bodies(&id, log_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map(RequestLogBodiesView::from)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
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
    let binding_effect_code = normalize_binding_effect_filter(q.binding_effect.as_deref());
    let selection_effect_code = normalize_selection_effect_filter(q.selection_effect.as_deref());
    validate_logs_effect_filters(
        result_status,
        key_effect_code,
        binding_effect_code,
        selection_effect_code,
    )?;
    let auth_token_id = normalize_optional_filter(q.auth_token_id.as_deref());

    state
        .proxy
        .key_logs_page(
            &id,
            q.since,
            &request_kinds,
            result_status,
            key_effect_code,
            binding_effect_code,
            selection_effect_code,
            auth_token_id,
            page,
            per_page,
        )
        .await
        .map(|logs| {
            Json(KeyLogsPageView {
                items: logs
                    .items
                    .into_iter()
                    .map(RequestLogView::from_summary_record)
                    .collect(),
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
                    binding_effects: logs
                        .facets
                        .binding_effects
                        .into_iter()
                        .map(LogFacetOptionView::from)
                        .collect(),
                    selection_effects: logs
                        .facets
                        .selection_effects
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

async fn get_key_logs_list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    RawQuery(raw_query): RawQuery,
    Query(q): Query<CursorLogsQuery>,
) -> Result<Json<RequestLogsCursorPageView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let page_size = q.limit.unwrap_or(20).clamp(1, 200);
    let cursor = parse_request_logs_cursor(q.cursor.as_deref())?;
    let direction = normalize_request_logs_cursor_direction(q.direction.as_deref())?;
    let request_kinds = parse_request_kind_filters(raw_query.as_deref());
    let result_status = normalize_result_status_filter(q.result.as_deref());
    let key_effect_code = normalize_key_effect_filter(q.key_effect.as_deref());
    let binding_effect_code = normalize_binding_effect_filter(q.binding_effect.as_deref());
    let selection_effect_code = normalize_selection_effect_filter(q.selection_effect.as_deref());
    validate_logs_effect_filters(
        result_status,
        key_effect_code,
        binding_effect_code,
        selection_effect_code,
    )?;
    let auth_token_id = normalize_optional_filter(q.auth_token_id.as_deref());
    let operational_class = normalize_operational_class_filter(q.operational_class.as_deref());
    if q
        .operational_class
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
        && operational_class.is_none()
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    state
        .proxy
        .key_logs_list(
            &id,
            q.since,
            &request_kinds,
            result_status,
            key_effect_code,
            binding_effect_code,
            selection_effect_code,
            auth_token_id,
            operational_class,
            cursor.as_ref(),
            direction,
            page_size,
        )
        .await
        .map(build_request_logs_cursor_page_view)
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn get_key_logs_catalog(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    RawQuery(raw_query): RawQuery,
    Query(q): Query<CursorLogsQuery>,
) -> Result<Json<RequestLogsCatalogView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let request_kinds = parse_request_kind_filters(raw_query.as_deref());
    let result_status = normalize_result_status_filter(q.result.as_deref());
    let key_effect_code = normalize_key_effect_filter(q.key_effect.as_deref());
    let binding_effect_code = normalize_binding_effect_filter(q.binding_effect.as_deref());
    let selection_effect_code = normalize_selection_effect_filter(q.selection_effect.as_deref());
    validate_logs_effect_filters(
        result_status,
        key_effect_code,
        binding_effect_code,
        selection_effect_code,
    )?;
    let auth_token_id = normalize_optional_filter(q.auth_token_id.as_deref());
    let operational_class = normalize_operational_class_filter(q.operational_class.as_deref());
    if q
        .operational_class
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
        && operational_class.is_none()
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    state
        .proxy
        .key_logs_catalog(
            &id,
            q.since,
            &request_kinds,
            result_status,
            key_effect_code,
            binding_effect_code,
            selection_effect_code,
            auth_token_id,
            operational_class,
        )
        .await
        .map(RequestLogsCatalogView::from)
        .map(Json)
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
    binding_effect: Option<String>,
    selection_effect: Option<String>,
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
    owner: Option<TokenOwnerView>,
    total_requests: i64,
    last_used_at: Option<i64>,
    quota_state: String,
    request_rate: tavily_hikari::RequestRateView,
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
    monthly_broken_count: Option<i64>,
    monthly_broken_limit: Option<i64>,
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
    let binding_effect_code = normalize_binding_effect_filter(q.binding_effect.as_deref());
    let selection_effect_code = normalize_selection_effect_filter(q.selection_effect.as_deref());
    validate_logs_effect_filters(
        result_status,
        key_effect_code,
        binding_effect_code,
        selection_effect_code,
    )?;
    let key_id = normalize_optional_filter(q.key_id.as_deref());
    let operational_class = normalize_operational_class_filter(q.operational_class.as_deref());
    if q
        .operational_class
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
        && operational_class.is_none()
    {
        return Err(StatusCode::BAD_REQUEST);
    }
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
            binding_effect_code,
            selection_effect_code,
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
                    binding_effects: logs
                        .facets
                        .binding_effects
                        .into_iter()
                        .map(LogFacetOptionView::from)
                        .collect(),
                    selection_effects: logs
                        .facets
                        .selection_effects
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

async fn get_token_logs_list(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Query(q): Query<TokenCursorLogsQuery>,
) -> Result<Json<RequestLogsCursorPageView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let page_size = q.limit.unwrap_or(20).clamp(1, 200);
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
    let cursor = parse_request_logs_cursor(q.cursor.as_deref())?;
    let direction = normalize_request_logs_cursor_direction(q.direction.as_deref())?;
    let request_kinds = parse_request_kind_filters(raw_query.as_deref());
    let result_status = normalize_result_status_filter(q.result.as_deref());
    let key_effect_code = normalize_key_effect_filter(q.key_effect.as_deref());
    let binding_effect_code = normalize_binding_effect_filter(q.binding_effect.as_deref());
    let selection_effect_code = normalize_selection_effect_filter(q.selection_effect.as_deref());
    validate_logs_effect_filters(
        result_status,
        key_effect_code,
        binding_effect_code,
        selection_effect_code,
    )?;
    let key_id = normalize_optional_filter(q.key_id.as_deref());
    let operational_class = normalize_operational_class_filter(q.operational_class.as_deref());
    if q
        .operational_class
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
        && operational_class.is_none()
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    state
        .proxy
        .token_logs_list(
            &id,
            page_size,
            since,
            Some(until),
            &request_kinds,
            result_status,
            key_effect_code,
            binding_effect_code,
            selection_effect_code,
            key_id,
            operational_class,
            cursor.as_ref(),
            direction,
        )
        .await
        .map(|page| build_token_logs_cursor_page_view(page, &id))
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn get_token_logs_catalog(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Query(q): Query<TokenCursorLogsQuery>,
) -> Result<Json<RequestLogsCatalogView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
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
    let binding_effect_code = normalize_binding_effect_filter(q.binding_effect.as_deref());
    let selection_effect_code = normalize_selection_effect_filter(q.selection_effect.as_deref());
    validate_logs_effect_filters(
        result_status,
        key_effect_code,
        binding_effect_code,
        selection_effect_code,
    )?;
    let key_id = normalize_optional_filter(q.key_id.as_deref());
    let operational_class = normalize_operational_class_filter(q.operational_class.as_deref());
    if q
        .operational_class
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
        && operational_class.is_none()
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    state
        .proxy
        .token_logs_catalog(
            &id,
            since,
            Some(until),
            &request_kinds,
            result_status,
            key_effect_code,
            binding_effect_code,
            selection_effect_code,
            key_id,
            operational_class,
        )
        .await
        .map(RequestLogsCatalogView::from)
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn get_log_details(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(log_id): Path<i64>,
) -> Result<Json<RequestLogBodiesView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    state
        .proxy
        .request_log_bodies(log_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map(RequestLogBodiesView::from)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

async fn get_token_log_details(
    State(state): State<Arc<AppState>>,
    Path((id, log_id)): Path<(String, i64)>,
    headers: HeaderMap,
) -> Result<Json<RequestLogBodiesView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    state
        .proxy
        .token_request_log_bodies(&id, log_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map(RequestLogBodiesView::from)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
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
    let owners = state
        .proxy
        .get_admin_token_owners(&token_ids)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let monthly_broken_counts = state
        .proxy
        .fetch_monthly_broken_counts_for_tokens(&token_ids)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let monthly_broken_subjects = state
        .proxy
        .list_monthly_broken_subjects_for_tokens(&token_ids)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut owner_user_ids: Vec<String> = owners.values().map(|owner| owner.user_id.clone()).collect();
    owner_user_ids.sort_unstable();
    owner_user_ids.dedup();
    let owner_monthly_broken_limits = state
        .proxy
        .fetch_account_monthly_broken_limits_bulk(&owner_user_ids)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let hourly_any_map = state
        .proxy
        .token_hourly_any_snapshot(&token_ids)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut items: Vec<TokenLeaderboardItemView> = Vec::with_capacity(tokens.len());

    for token in tokens {
        let owner = owners.get(&token.id);
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

        let request_rate = hourly_any_map
            .get(&token.id)
            .cloned()
            .unwrap_or_else(|| {
                state
                    .proxy
                    .default_request_rate_verdict(tavily_hikari::RequestRateScope::Token)
            });
        let (hourly_any_used, hourly_any_limit) =
            (request_rate.hourly_used, request_rate.hourly_limit);
        let has_monthly_broken_record = monthly_broken_subjects.contains(&token.id);
        let monthly_broken_count = has_monthly_broken_record.then(|| {
            monthly_broken_counts
                .get(&token.id)
                .copied()
                .unwrap_or_default()
        });
        let monthly_broken_limit = has_monthly_broken_record.then(|| {
            owner
                .and_then(|identity| owner_monthly_broken_limits.get(&identity.user_id).copied())
                .unwrap_or(UNBOUND_TOKEN_MONTHLY_BROKEN_LIMIT_DEFAULT)
        });

        let item = TokenLeaderboardItemView {
            id: token.id.clone(),
            enabled: token.enabled,
            note: token.note.clone(),
            group: token.group_name.clone(),
            owner: owner.map(TokenOwnerView::from),
            total_requests: all.total_requests,
            last_used_at: all.last_activity,
            quota_state,
            request_rate: request_rate.request_rate(),
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
            monthly_broken_count,
            monthly_broken_limit,
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

async fn get_token_monthly_broken_keys(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(q): Query<BrokenKeysPageQuery>,
) -> Result<Json<PaginatedMonthlyBrokenKeysView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let exists = state
        .proxy
        .get_access_token_secret(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if exists.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    state
        .proxy
        .fetch_token_monthly_broken_keys(&id, q.page.unwrap_or(1), q.per_page.unwrap_or(20))
        .await
        .map(build_monthly_broken_keys_view)
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[cfg(test)]
mod dto_tests {
    use super::validate_logs_effect_filters;
    use axum::http::StatusCode;

    #[test]
    fn validate_logs_effect_filters_allows_binding_and_selection_together() {
        assert_eq!(
            validate_logs_effect_filters(
                None,
                None,
                Some("http_project_affinity_rebound"),
                Some("http_project_affinity_cooldown_avoided"),
            ),
            Ok(())
        );
    }

    #[test]
    fn validate_logs_effect_filters_rejects_key_plus_binding() {
        assert_eq!(
            validate_logs_effect_filters(
                None,
                Some("quarantined"),
                Some("http_project_affinity_rebound"),
                None,
            ),
            Err(StatusCode::BAD_REQUEST)
        );
    }

    #[test]
    fn validate_logs_effect_filters_rejects_result_plus_key_effect() {
        assert_eq!(
            validate_logs_effect_filters(Some("success"), Some("quarantined"), None, None),
            Err(StatusCode::BAD_REQUEST)
        );
    }

    #[test]
    fn validate_logs_effect_filters_rejects_result_plus_binding_effect() {
        assert_eq!(
            validate_logs_effect_filters(
                Some("success"),
                None,
                Some("http_project_affinity_rebound"),
                None,
            ),
            Err(StatusCode::BAD_REQUEST)
        );
    }
}
