impl AdminUsersSortDirection {
    fn apply(self, ordering: std::cmp::Ordering) -> std::cmp::Ordering {
        match self {
            Self::Asc => ordering,
            Self::Desc => ordering.reverse(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ListUnboundTokenUsageQuery {
    page: Option<i64>,
    per_page: Option<i64>,
    q: Option<String>,
    sort: Option<AdminUnboundTokenUsageSortField>,
    order: Option<AdminUsersSortDirection>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum AdminUnboundTokenUsageSortField {
    HourlyAnyUsed,
    QuotaHourlyUsed,
    QuotaDailyUsed,
    QuotaMonthlyUsed,
    MonthlyBrokenCount,
    DailySuccessRate,
    MonthlySuccessRate,
    LastUsedAt,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminQuotaView {
    hourly_any_limit: i64,
    hourly_limit: i64,
    daily_limit: i64,
    monthly_limit: i64,
    inherits_defaults: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminUserTagView {
    id: String,
    name: String,
    display_name: String,
    icon: Option<String>,
    system_key: Option<String>,
    effect_kind: String,
    hourly_any_delta: i64,
    hourly_delta: i64,
    daily_delta: i64,
    monthly_delta: i64,
    user_count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminUserTagBindingView {
    tag_id: String,
    name: String,
    display_name: String,
    icon: Option<String>,
    system_key: Option<String>,
    effect_kind: String,
    hourly_any_delta: i64,
    hourly_delta: i64,
    daily_delta: i64,
    monthly_delta: i64,
    source: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminUserQuotaBreakdownView {
    kind: String,
    label: String,
    tag_id: Option<String>,
    tag_name: Option<String>,
    source: Option<String>,
    effect_kind: String,
    hourly_any_delta: i64,
    hourly_delta: i64,
    daily_delta: i64,
    monthly_delta: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminUserSummaryView {
    user_id: String,
    display_name: Option<String>,
    username: Option<String>,
    active: bool,
    last_login_at: Option<i64>,
    token_count: i64,
    api_key_count: i64,
    request_rate: tavily_hikari::RequestRateView,
    hourly_any_used: i64,
    hourly_any_limit: i64,
    quota_hourly_used: i64,
    quota_hourly_limit: i64,
    quota_daily_used: i64,
    quota_daily_limit: i64,
    quota_monthly_used: i64,
    quota_monthly_limit: i64,
    daily_success: i64,
    daily_failure: i64,
    monthly_success: i64,
    monthly_failure: i64,
    monthly_broken_count: i64,
    monthly_broken_limit: i64,
    last_activity: Option<i64>,
    tags: Vec<AdminUserTagBindingView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminUserTokenSummaryView {
    token_id: String,
    enabled: bool,
    note: Option<String>,
    created_at: i64,
    last_used_at: Option<i64>,
    total_requests: i64,
    daily_success: i64,
    daily_failure: i64,
    monthly_success: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListUsersResponse {
    items: Vec<AdminUserSummaryView>,
    total: i64,
    page: i64,
    per_page: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListUserTagsResponse {
    items: Vec<AdminUserTagView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminUserUsageSeriesPointView {
    bucket_start: i64,
    display_bucket_start: Option<i64>,
    value: Option<i64>,
    limit_value: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminUserUsageSeriesView {
    limit: i64,
    points: Vec<AdminUserUsageSeriesPointView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminUserDetailView {
    user_id: String,
    display_name: Option<String>,
    username: Option<String>,
    active: bool,
    last_login_at: Option<i64>,
    token_count: i64,
    api_key_count: i64,
    request_rate: tavily_hikari::RequestRateView,
    hourly_any_used: i64,
    hourly_any_limit: i64,
    quota_hourly_used: i64,
    quota_hourly_limit: i64,
    quota_daily_used: i64,
    quota_daily_limit: i64,
    quota_monthly_used: i64,
    quota_monthly_limit: i64,
    daily_success: i64,
    daily_failure: i64,
    monthly_success: i64,
    monthly_failure: i64,
    monthly_broken_count: i64,
    monthly_broken_limit: i64,
    last_activity: Option<i64>,
    tags: Vec<AdminUserTagBindingView>,
    quota_base: AdminQuotaView,
    effective_quota: AdminQuotaView,
    quota_breakdown: Vec<AdminUserQuotaBreakdownView>,
    tokens: Vec<AdminUserTokenSummaryView>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateUserQuotaRequest {
    hourly_any_limit: Option<i64>,
    hourly_limit: i64,
    daily_limit: i64,
    monthly_limit: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateUserBrokenKeyLimitRequest {
    monthly_broken_limit: i64,
}

#[derive(Debug, Deserialize)]
struct BrokenKeysPageQuery {
    page: Option<i64>,
    per_page: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MonthlyBrokenKeyRelatedUserView {
    user_id: String,
    display_name: Option<String>,
    username: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MonthlyBrokenKeyDetailView {
    key_id: String,
    current_status: String,
    reason_code: Option<String>,
    reason_summary: Option<String>,
    latest_break_at: i64,
    source: String,
    breaker_token_id: Option<String>,
    breaker_user_id: Option<String>,
    breaker_user_display_name: Option<String>,
    manual_actor_display_name: Option<String>,
    related_users: Vec<MonthlyBrokenKeyRelatedUserView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PaginatedMonthlyBrokenKeysView {
    items: Vec<MonthlyBrokenKeyDetailView>,
    total: i64,
    page: i64,
    per_page: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserTagMutationRequest {
    name: String,
    display_name: String,
    icon: Option<String>,
    effect_kind: String,
    hourly_any_delta: i64,
    hourly_delta: i64,
    daily_delta: i64,
    monthly_delta: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BindUserTagRequest {
    tag_id: String,
}

#[derive(Debug, Clone)]
struct AdminUserSummaryRow {
    user: tavily_hikari::AdminUserIdentity,
    summary: tavily_hikari::UserDashboardSummary,
    monthly_broken_count: i64,
    monthly_broken_limit: i64,
}

#[derive(Debug, Clone)]
struct AdminUnboundTokenUsageRow {
    token: AuthToken,
    request_rate: tavily_hikari::RequestRateView,
    hourly_any_used: i64,
    hourly_any_limit: i64,
    daily_success: i64,
    daily_failure: i64,
    monthly_success: i64,
    monthly_failure: i64,
    monthly_broken_count: Option<i64>,
    monthly_broken_limit: Option<i64>,
    last_used_at: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminUnboundTokenUsageView {
    token_id: String,
    enabled: bool,
    note: Option<String>,
    group: Option<String>,
    request_rate: tavily_hikari::RequestRateView,
    hourly_any_used: i64,
    hourly_any_limit: i64,
    quota_hourly_used: i64,
    quota_hourly_limit: i64,
    quota_daily_used: i64,
    quota_daily_limit: i64,
    quota_monthly_used: i64,
    quota_monthly_limit: i64,
    daily_success: i64,
    daily_failure: i64,
    monthly_success: i64,
    monthly_failure: i64,
    monthly_broken_count: Option<i64>,
    monthly_broken_limit: Option<i64>,
    last_used_at: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListUnboundTokenUsageResponse {
    items: Vec<AdminUnboundTokenUsageView>,
    total: i64,
    page: i64,
    per_page: i64,
}

fn build_admin_quota_view(quota: &tavily_hikari::AdminQuotaLimitSet) -> AdminQuotaView {
    AdminQuotaView {
        hourly_any_limit: quota.hourly_any_limit,
        hourly_limit: quota.hourly_limit,
        daily_limit: quota.daily_limit,
        monthly_limit: quota.monthly_limit,
        inherits_defaults: quota.inherits_defaults,
    }
}

fn build_admin_user_tag_view(tag: &tavily_hikari::AdminUserTag) -> AdminUserTagView {
    AdminUserTagView {
        id: tag.id.clone(),
        name: tag.name.clone(),
        display_name: tag.display_name.clone(),
        icon: tag.icon.clone(),
        system_key: tag.system_key.clone(),
        effect_kind: tag.effect_kind.clone(),
        hourly_any_delta: tag.hourly_any_delta,
        hourly_delta: tag.hourly_delta,
        daily_delta: tag.daily_delta,
        monthly_delta: tag.monthly_delta,
        user_count: tag.user_count,
    }
}

fn build_admin_user_tag_binding_view(
    binding: &tavily_hikari::AdminUserTagBinding,
) -> AdminUserTagBindingView {
    AdminUserTagBindingView {
        tag_id: binding.tag_id.clone(),
        name: binding.name.clone(),
        display_name: binding.display_name.clone(),
        icon: binding.icon.clone(),
        system_key: binding.system_key.clone(),
        effect_kind: binding.effect_kind.clone(),
        hourly_any_delta: binding.hourly_any_delta,
        hourly_delta: binding.hourly_delta,
        daily_delta: binding.daily_delta,
        monthly_delta: binding.monthly_delta,
        source: binding.source.clone(),
    }
}

fn build_admin_quota_breakdown_view(
    entry: &tavily_hikari::AdminUserQuotaBreakdownEntry,
) -> AdminUserQuotaBreakdownView {
    AdminUserQuotaBreakdownView {
        kind: entry.kind.clone(),
        label: entry.label.clone(),
        tag_id: entry.tag_id.clone(),
        tag_name: entry.tag_name.clone(),
        source: entry.source.clone(),
        effect_kind: entry.effect_kind.clone(),
        hourly_any_delta: entry.hourly_any_delta,
        hourly_delta: entry.hourly_delta,
        daily_delta: entry.daily_delta,
        monthly_delta: entry.monthly_delta,
    }
}

fn admin_proxy_error_response(context: &str, err: ProxyError) -> (StatusCode, String) {
    eprintln!("{context}: {err}");
    let status = match err {
        ProxyError::Other(_) => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, err.to_string())
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|it| {
        let trimmed = it.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn default_request_rate_view(scope: tavily_hikari::RequestRateScope) -> tavily_hikari::RequestRateView {
    TokenHourlyRequestVerdict::new(
        0,
        request_rate_limit(),
        request_rate_limit_window_minutes(),
        scope,
        0,
    )
    .request_rate()
}

fn build_admin_user_summary_view(
    user: &tavily_hikari::AdminUserIdentity,
    summary: &tavily_hikari::UserDashboardSummary,
    api_key_count: i64,
    monthly_broken_count: i64,
    monthly_broken_limit: i64,
    tags: Vec<tavily_hikari::AdminUserTagBinding>,
) -> AdminUserSummaryView {
    AdminUserSummaryView {
        user_id: user.user_id.clone(),
        display_name: user.display_name.clone(),
        username: user.username.clone(),
        active: user.active,
        last_login_at: user.last_login_at,
        token_count: user.token_count,
        api_key_count,
        request_rate: summary.request_rate.clone(),
        hourly_any_used: summary.hourly_any_used,
        hourly_any_limit: summary.hourly_any_limit,
        quota_hourly_used: summary.quota_hourly_used,
        quota_hourly_limit: summary.quota_hourly_limit,
        quota_daily_used: summary.quota_daily_used,
        quota_daily_limit: summary.quota_daily_limit,
        quota_monthly_used: summary.quota_monthly_used,
        quota_monthly_limit: summary.quota_monthly_limit,
        daily_success: summary.daily_success,
        daily_failure: summary.daily_failure,
        monthly_success: summary.monthly_success,
        monthly_failure: summary.monthly_failure,
        monthly_broken_count,
        monthly_broken_limit,
        last_activity: summary.last_activity,
        tags: tags.iter().map(build_admin_user_tag_binding_view).collect(),
    }
}

fn build_monthly_broken_keys_view(
    page: tavily_hikari::PaginatedMonthlyBrokenKeys,
) -> PaginatedMonthlyBrokenKeysView {
    PaginatedMonthlyBrokenKeysView {
        total: page.total,
        page: page.page,
        per_page: page.per_page,
        items: page
            .items
            .into_iter()
            .map(|item| MonthlyBrokenKeyDetailView {
                key_id: item.key_id,
                current_status: item.current_status,
                reason_code: item.reason_code,
                reason_summary: item.reason_summary,
                latest_break_at: item.latest_break_at,
                source: item.source,
                breaker_token_id: item.breaker_token_id,
                breaker_user_id: item.breaker_user_id,
                breaker_user_display_name: item.breaker_user_display_name,
                manual_actor_display_name: None,
                related_users: item
                    .related_users
                    .into_iter()
                    .map(|user| MonthlyBrokenKeyRelatedUserView {
                        user_id: user.user_id,
                        display_name: user.display_name,
                        username: user.username,
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn empty_user_dashboard_summary() -> tavily_hikari::UserDashboardSummary {
    tavily_hikari::UserDashboardSummary {
        request_rate: default_request_rate_view(tavily_hikari::RequestRateScope::User),
        hourly_any_used: 0,
        hourly_any_limit: 0,
        quota_hourly_used: 0,
        quota_hourly_limit: 0,
        quota_daily_used: 0,
        quota_daily_limit: 0,
        quota_monthly_used: 0,
        quota_monthly_limit: 0,
        daily_success: 0,
        daily_failure: 0,
        monthly_success: 0,
        monthly_failure: 0,
        last_activity: None,
    }
}

fn token_quota_values(token: &AuthToken) -> (i64, i64, i64, i64, i64, i64) {
    if let Some(quota) = token.quota.as_ref() {
        (
            quota.hourly_used,
            quota.hourly_limit,
            quota.daily_used,
            quota.daily_limit,
            quota.monthly_used,
            quota.monthly_limit,
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
    }
}

fn compare_optional_timestamp(
    left: Option<i64>,
    right: Option<i64>,
    direction: AdminUsersSortDirection,
) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => direction.apply(left.cmp(&right)),
        (Some(_), None) => match direction {
            AdminUsersSortDirection::Asc => std::cmp::Ordering::Greater,
            AdminUsersSortDirection::Desc => std::cmp::Ordering::Less,
        },
        (None, Some(_)) => match direction {
            AdminUsersSortDirection::Asc => std::cmp::Ordering::Less,
            AdminUsersSortDirection::Desc => std::cmp::Ordering::Greater,
        },
        (None, None) => std::cmp::Ordering::Equal,
    }
}

fn compare_quota_usage(
    left_used: i64,
    left_limit: i64,
    right_used: i64,
    right_limit: i64,
    direction: AdminUsersSortDirection,
) -> std::cmp::Ordering {
    let used_order = direction.apply(left_used.cmp(&right_used));
    if used_order != std::cmp::Ordering::Equal {
        return used_order;
    }
    direction.apply(left_limit.cmp(&right_limit))
}

fn compare_optional_quota_usage(
    left_used: Option<i64>,
    left_limit: Option<i64>,
    right_used: Option<i64>,
    right_limit: Option<i64>,
    direction: AdminUsersSortDirection,
) -> std::cmp::Ordering {
    match (left_used, left_limit, right_used, right_limit) {
        (Some(left_used), Some(left_limit), Some(right_used), Some(right_limit)) => {
            compare_quota_usage(left_used, left_limit, right_used, right_limit, direction)
        }
        (Some(_), Some(_), _, _) => std::cmp::Ordering::Less,
        (_, _, Some(_), Some(_)) => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    }
}

fn compare_success_rate(
    left_success: i64,
    left_failure: i64,
    right_success: i64,
    right_failure: i64,
    direction: AdminUsersSortDirection,
) -> std::cmp::Ordering {
    let left_total = left_success + left_failure;
    let right_total = right_success + right_failure;
    match (left_total == 0, right_total == 0) {
        (true, true) => return std::cmp::Ordering::Equal,
        (true, false) => return std::cmp::Ordering::Greater,
        (false, true) => return std::cmp::Ordering::Less,
        (false, false) => {}
    }

    let left_ratio = i128::from(left_success) * i128::from(right_total);
    let right_ratio = i128::from(right_success) * i128::from(left_total);
    let ratio_order = direction.apply(left_ratio.cmp(&right_ratio));
    if ratio_order != std::cmp::Ordering::Equal {
        return ratio_order;
    }

    left_failure.cmp(&right_failure)
}

fn compare_admin_user_rows(
    left: &AdminUserSummaryRow,
    right: &AdminUserSummaryRow,
    sort: Option<AdminUsersSortField>,
    order: Option<AdminUsersSortDirection>,
) -> std::cmp::Ordering {
    let (sort_field, direction) = match sort {
        Some(field) => (field, order.unwrap_or(AdminUsersSortDirection::Desc)),
        None => (AdminUsersSortField::LastLoginAt, AdminUsersSortDirection::Desc),
    };

    let ordering = match sort_field {
        AdminUsersSortField::HourlyAnyUsed => compare_quota_usage(
            left.summary.hourly_any_used,
            left.summary.hourly_any_limit,
            right.summary.hourly_any_used,
            right.summary.hourly_any_limit,
            direction,
        ),
        AdminUsersSortField::QuotaHourlyUsed => compare_quota_usage(
            left.summary.quota_hourly_used,
            left.summary.quota_hourly_limit,
            right.summary.quota_hourly_used,
            right.summary.quota_hourly_limit,
            direction,
        ),
        AdminUsersSortField::QuotaDailyUsed => compare_quota_usage(
            left.summary.quota_daily_used,
            left.summary.quota_daily_limit,
            right.summary.quota_daily_used,
            right.summary.quota_daily_limit,
            direction,
        ),
        AdminUsersSortField::QuotaMonthlyUsed => compare_quota_usage(
            left.summary.quota_monthly_used,
            left.summary.quota_monthly_limit,
            right.summary.quota_monthly_used,
            right.summary.quota_monthly_limit,
            direction,
        ),
        AdminUsersSortField::DailySuccessRate => compare_success_rate(
            left.summary.daily_success,
            left.summary.daily_failure,
            right.summary.daily_success,
            right.summary.daily_failure,
            direction,
        ),
        AdminUsersSortField::MonthlySuccessRate => compare_success_rate(
            left.summary.monthly_success,
            left.summary.monthly_failure,
            right.summary.monthly_success,
            right.summary.monthly_failure,
            direction,
        ),
        AdminUsersSortField::MonthlyBrokenCount => {
            let count_order =
                direction.apply(left.monthly_broken_count.cmp(&right.monthly_broken_count));
            if count_order != std::cmp::Ordering::Equal {
                count_order
            } else {
                direction.apply(left.monthly_broken_limit.cmp(&right.monthly_broken_limit))
            }
        }
        AdminUsersSortField::LastActivity => compare_optional_timestamp(
            left.summary.last_activity,
            right.summary.last_activity,
            direction,
        ),
        AdminUsersSortField::LastLoginAt => compare_optional_timestamp(
            left.user.last_login_at,
            right.user.last_login_at,
            direction,
        ),
    };
    if ordering != std::cmp::Ordering::Equal {
        return ordering;
    }

    left.user.user_id.cmp(&right.user.user_id)
}

fn token_usage_matches_query(token: &AuthToken, query: &str) -> bool {
    let normalized_query = query.trim().to_ascii_lowercase();
    if normalized_query.is_empty() {
        return true;
    }

    token.id.to_ascii_lowercase().contains(&normalized_query)
        || token
            .note
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| value.to_ascii_lowercase().contains(&normalized_query))
        || token
            .group_name
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| value.to_ascii_lowercase().contains(&normalized_query))
}

fn compare_admin_unbound_token_usage_rows(
    left: &AdminUnboundTokenUsageRow,
    right: &AdminUnboundTokenUsageRow,
    sort: Option<AdminUnboundTokenUsageSortField>,
    order: Option<AdminUsersSortDirection>,
) -> std::cmp::Ordering {
    let (sort_field, direction) = match sort {
        Some(field) => (field, order.unwrap_or(AdminUsersSortDirection::Desc)),
        None => (
            AdminUnboundTokenUsageSortField::LastUsedAt,
            AdminUsersSortDirection::Desc,
        ),
    };
    let (
        left_quota_hourly_used,
        left_quota_hourly_limit,
        left_quota_daily_used,
        left_quota_daily_limit,
        left_quota_monthly_used,
        left_quota_monthly_limit,
    ) = token_quota_values(&left.token);
    let (
        right_quota_hourly_used,
        right_quota_hourly_limit,
        right_quota_daily_used,
        right_quota_daily_limit,
        right_quota_monthly_used,
        right_quota_monthly_limit,
    ) = token_quota_values(&right.token);

    let ordering = match sort_field {
        AdminUnboundTokenUsageSortField::HourlyAnyUsed => compare_quota_usage(
            left.hourly_any_used,
            left.hourly_any_limit,
            right.hourly_any_used,
            right.hourly_any_limit,
            direction,
        ),
        AdminUnboundTokenUsageSortField::QuotaHourlyUsed => compare_quota_usage(
            left_quota_hourly_used,
            left_quota_hourly_limit,
            right_quota_hourly_used,
            right_quota_hourly_limit,
            direction,
        ),
        AdminUnboundTokenUsageSortField::QuotaDailyUsed => compare_quota_usage(
            left_quota_daily_used,
            left_quota_daily_limit,
            right_quota_daily_used,
            right_quota_daily_limit,
            direction,
        ),
        AdminUnboundTokenUsageSortField::QuotaMonthlyUsed => compare_quota_usage(
            left_quota_monthly_used,
            left_quota_monthly_limit,
            right_quota_monthly_used,
            right_quota_monthly_limit,
            direction,
        ),
        AdminUnboundTokenUsageSortField::MonthlyBrokenCount => compare_optional_quota_usage(
            left.monthly_broken_count,
            left.monthly_broken_limit,
            right.monthly_broken_count,
            right.monthly_broken_limit,
            direction,
        ),
        AdminUnboundTokenUsageSortField::DailySuccessRate => compare_success_rate(
            left.daily_success,
            left.daily_failure,
            right.daily_success,
            right.daily_failure,
            direction,
        ),
        AdminUnboundTokenUsageSortField::MonthlySuccessRate => compare_success_rate(
            left.monthly_success,
            left.monthly_failure,
            right.monthly_success,
            right.monthly_failure,
            direction,
        ),
        AdminUnboundTokenUsageSortField::LastUsedAt => compare_optional_timestamp(
            left.last_used_at,
            right.last_used_at,
            direction,
        ),
    };

    if ordering != std::cmp::Ordering::Equal {
        return ordering;
    }

    left.token.id.cmp(&right.token.id)
}

fn build_admin_unbound_token_usage_view(
    row: AdminUnboundTokenUsageRow,
) -> AdminUnboundTokenUsageView {
    let (
        quota_hourly_used,
        quota_hourly_limit,
        quota_daily_used,
        quota_daily_limit,
        quota_monthly_used,
        quota_monthly_limit,
    ) = token_quota_values(&row.token);

    AdminUnboundTokenUsageView {
        token_id: row.token.id,
        enabled: row.token.enabled,
        note: row.token.note,
        group: row.token.group_name,
        request_rate: row.request_rate,
        hourly_any_used: row.hourly_any_used,
        hourly_any_limit: row.hourly_any_limit,
        quota_hourly_used,
        quota_hourly_limit,
        quota_daily_used,
        quota_daily_limit,
        quota_monthly_used,
        quota_monthly_limit,
        daily_success: row.daily_success,
        daily_failure: row.daily_failure,
        monthly_success: row.monthly_success,
        monthly_failure: row.monthly_failure,
        monthly_broken_count: row.monthly_broken_count,
        monthly_broken_limit: row.monthly_broken_limit,
        last_used_at: row.last_used_at,
    }
}
