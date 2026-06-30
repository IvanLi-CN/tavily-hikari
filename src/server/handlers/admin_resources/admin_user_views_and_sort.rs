impl AdminUsersSortDirection {
    fn apply(self, ordering: std::cmp::Ordering) -> std::cmp::Ordering {
        match self {
            Self::Asc => ordering,
            Self::Desc => ordering.reverse(),
        }
    }

    fn to_admin_list_sort_direction(self) -> tavily_hikari::AdminListSortDirection {
        match self {
            Self::Asc => tavily_hikari::AdminListSortDirection::Asc,
            Self::Desc => tavily_hikari::AdminListSortDirection::Desc,
        }
    }
}

impl AdminUsersSortField {
    fn to_paged_admin_user_sort_field(self) -> Option<tavily_hikari::AdminUserListSortField> {
        match self {
            Self::RequestRateUsed => None,
            Self::BusinessCalls1hUsed => None,
            Self::DailyCreditsUsed => Some(tavily_hikari::AdminUserListSortField::DailyCreditsUsed),
            Self::MonthlyCreditsUsed => {
                Some(tavily_hikari::AdminUserListSortField::MonthlyCreditsUsed)
            }
            Self::DailySuccessRate => Some(tavily_hikari::AdminUserListSortField::DailySuccessRate),
            Self::MonthlySuccessRate => {
                Some(tavily_hikari::AdminUserListSortField::MonthlySuccessRate)
            }
            Self::MonthlyBrokenCount => {
                Some(tavily_hikari::AdminUserListSortField::MonthlyBrokenCount)
            }
            Self::RecentIpCount7d => Some(tavily_hikari::AdminUserListSortField::RecentIpCount7d),
            Self::LastActivity => Some(tavily_hikari::AdminUserListSortField::LastActivity),
            Self::LastLoginAt => Some(tavily_hikari::AdminUserListSortField::LastLoginAt),
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
    business_calls_1h_limit: i64,
    daily_credits_limit: i64,
    monthly_credits_limit: i64,
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
    business_calls_1h_delta: i64,
    daily_credits_delta: i64,
    monthly_credits_delta: i64,
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
    business_calls_1h_delta: i64,
    daily_credits_delta: i64,
    monthly_credits_delta: i64,
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
    business_calls_1h_delta: i64,
    daily_credits_delta: i64,
    monthly_credits_delta: i64,
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
    business_calls_1h: AdminBusinessCalls1hSummaryView,
    daily_credits_used: i64,
    daily_credits_limit: i64,
    monthly_credits_used: i64,
    monthly_credits_limit: i64,
    daily_success: i64,
    daily_failure: i64,
    monthly_success: i64,
    monthly_failure: i64,
    monthly_broken_count: i64,
    monthly_broken_limit: i64,
    recent_ip_count_24h: i64,
    recent_ip_count_7d: i64,
    last_activity: Option<i64>,
    tags: Vec<AdminUserTagBindingView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminBusinessCalls1hSummaryView {
    success_count: i64,
    failure_count: i64,
    total_count: i64,
    limit: i64,
    window_minutes: i64,
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
struct AdminUserUsageSeriesQuotaPointView {
    bucket_start: i64,
    display_bucket_start: Option<i64>,
    value: Option<i64>,
    limit_value: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminUserBusinessCalls1hBarsPointView {
    success: Option<i64>,
    failure: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminUserBusinessCalls1hPointView {
    bucket_start: i64,
    display_bucket_start: Option<i64>,
    bars: AdminUserBusinessCalls1hBarsPointView,
    pressure: Option<i64>,
    limit_value: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "kind")]
enum AdminUserUsageSeriesView {
    #[serde(rename = "quotaLike")]
    QuotaLike {
        limit: i64,
        points: Vec<AdminUserUsageSeriesQuotaPointView>,
    },
    #[serde(rename = "businessCalls1h")]
    BusinessCalls1h {
        limit: i64,
        points: Vec<AdminUserBusinessCalls1hPointView>,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminUserIpTimelineEntryView {
    ip_address: String,
    first_seen_at: i64,
    last_seen_at: i64,
    request_count: i64,
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
    business_calls_1h: AdminBusinessCalls1hSummaryView,
    daily_credits_used: i64,
    daily_credits_limit: i64,
    monthly_credits_used: i64,
    monthly_credits_limit: i64,
    daily_success: i64,
    daily_failure: i64,
    monthly_success: i64,
    monthly_failure: i64,
    monthly_broken_count: i64,
    monthly_broken_limit: i64,
    recent_ip_count_24h: i64,
    recent_ip_count_7d: i64,
    recent_ip_addresses_24h: Vec<String>,
    recent_ip_addresses_7d: Vec<String>,
    recent_ip_timeline_7d: Vec<AdminUserIpTimelineEntryView>,
    last_activity: Option<i64>,
    tags: Vec<AdminUserTagBindingView>,
    quota_base: AdminQuotaView,
    effective_quota: AdminQuotaView,
    quota_breakdown: Vec<AdminUserQuotaBreakdownView>,
    recharge: AdminUserRechargeAuditView,
    tokens: Vec<AdminUserTokenSummaryView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminUserRechargeAuditView {
    current_month_entitlement_credits: i64,
    effective_until_month_start: Option<i64>,
    orders: Vec<AdminUserRechargeOrderView>,
    entitlements: Vec<AdminUserRechargeEntitlementView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminUserRechargeOrderView {
    out_trade_no: String,
    status: String,
    credits: i64,
    months: i64,
    money: String,
    quote_month_start: i64,
    final_money_cents: i64,
    final_hourly_delta: i64,
    final_daily_delta: i64,
    final_monthly_delta: i64,
    month_end_clamp_applied: bool,
    trade_no: Option<String>,
    payment_url: Option<String>,
    created_at: i64,
    updated_at: i64,
    paid_at: Option<i64>,
    refunded_at: Option<i64>,
    refund_actor: Option<String>,
    last_notify_at: Option<i64>,
    last_error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminUserRechargeEntitlementView {
    id: i64,
    out_trade_no: String,
    month_start: i64,
    credits: i64,
    hourly_delta: i64,
    daily_delta: i64,
    monthly_delta: i64,
    created_at: i64,
}

fn build_admin_user_recharge_order_view(
    order: tavily_hikari::LinuxDoCreditRechargeOrder,
) -> AdminUserRechargeOrderView {
    AdminUserRechargeOrderView {
        out_trade_no: order.out_trade_no,
        status: order.status,
        credits: order.credits,
        months: order.months,
        money: tavily_hikari::format_linuxdo_credit_money(order.money_cents),
        quote_month_start: order.quote_month_start,
        final_money_cents: order.final_money_cents,
        final_hourly_delta: order.final_hourly_delta,
        final_daily_delta: order.final_daily_delta,
        final_monthly_delta: order.final_monthly_delta,
        month_end_clamp_applied: order.month_end_clamp_applied,
        trade_no: order.trade_no,
        payment_url: order.payment_url,
        created_at: order.created_at,
        updated_at: order.updated_at,
        paid_at: order.paid_at,
        refunded_at: order.refunded_at,
        refund_actor: order.refund_actor,
        last_notify_at: order.last_notify_at,
        last_error: order.last_error,
    }
}

fn build_admin_user_recharge_entitlement_view(
    entitlement: tavily_hikari::LinuxDoCreditRechargeEntitlement,
) -> AdminUserRechargeEntitlementView {
    AdminUserRechargeEntitlementView {
        id: entitlement.id,
        out_trade_no: entitlement.out_trade_no,
        month_start: entitlement.month_start,
        credits: entitlement.credits,
        hourly_delta: entitlement.hourly_delta,
        daily_delta: entitlement.daily_delta,
        monthly_delta: entitlement.monthly_delta,
        created_at: entitlement.created_at,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateUserQuotaRequest {
    business_calls_1h_limit: i64,
    daily_credits_limit: i64,
    monthly_credits_limit: i64,
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
    business_calls_1h_delta: i64,
    daily_credits_delta: i64,
    monthly_credits_delta: i64,
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
    recent_ip_count_7d: i64,
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
        business_calls_1h_limit: quota.business_calls_1h_limit,
        daily_credits_limit: quota.daily_credits_limit,
        monthly_credits_limit: quota.monthly_credits_limit,
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
        business_calls_1h_delta: tag.business_calls_1h_delta,
        daily_credits_delta: tag.daily_credits_delta,
        monthly_credits_delta: tag.monthly_credits_delta,
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
        business_calls_1h_delta: binding.business_calls_1h_delta,
        daily_credits_delta: binding.daily_credits_delta,
        monthly_credits_delta: binding.monthly_credits_delta,
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
        business_calls_1h_delta: entry.business_calls_1h_delta,
        daily_credits_delta: entry.daily_credits_delta,
        monthly_credits_delta: entry.monthly_credits_delta,
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

struct AdminUserSummaryViewInput {
    api_key_count: i64,
    monthly_broken_count: i64,
    monthly_broken_limit: i64,
    recent_ip_count_24h: i64,
    recent_ip_count_7d: i64,
    tags: Vec<tavily_hikari::AdminUserTagBinding>,
}

fn build_admin_user_summary_view(
    user: &tavily_hikari::AdminUserIdentity,
    summary: &tavily_hikari::UserDashboardSummary,
    input: AdminUserSummaryViewInput,
) -> AdminUserSummaryView {
    AdminUserSummaryView {
        user_id: user.user_id.clone(),
        display_name: user.display_name.clone(),
        username: user.username.clone(),
        active: user.active,
        last_login_at: user.last_login_at,
        token_count: user.token_count,
        api_key_count: input.api_key_count,
        request_rate: summary.request_rate.clone(),
        business_calls_1h: AdminBusinessCalls1hSummaryView {
            success_count: summary.business_calls_1h.success_count,
            failure_count: summary.business_calls_1h.failure_count,
            total_count: summary.business_calls_1h.total_count,
            limit: summary.business_calls_1h.limit,
            window_minutes: summary.business_calls_1h.window_minutes,
        },
        daily_credits_used: summary.daily_credits_used,
        daily_credits_limit: summary.daily_credits_limit,
        monthly_credits_used: summary.monthly_credits_used,
        monthly_credits_limit: summary.monthly_credits_limit,
        daily_success: summary.daily_success,
        daily_failure: summary.daily_failure,
        monthly_success: summary.monthly_success,
        monthly_failure: summary.monthly_failure,
        monthly_broken_count: input.monthly_broken_count,
        monthly_broken_limit: input.monthly_broken_limit,
        recent_ip_count_24h: input.recent_ip_count_24h,
        recent_ip_count_7d: input.recent_ip_count_7d,
        last_activity: summary.last_activity,
        tags: input
            .tags
            .iter()
            .map(build_admin_user_tag_binding_view)
            .collect(),
    }
}

fn build_admin_user_ip_timeline_entry_view(
    entry: tavily_hikari::AdminUserIpTimelineEntry,
) -> AdminUserIpTimelineEntryView {
    AdminUserIpTimelineEntryView {
        ip_address: entry.ip_address,
        first_seen_at: entry.first_seen_at,
        last_seen_at: entry.last_seen_at,
        request_count: entry.request_count,
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
        debug_info_shared: false,
        request_rate: default_request_rate_view(tavily_hikari::RequestRateScope::User),
        business_calls_1h: tavily_hikari::BusinessCalls1hSummary {
            window_minutes: 60,
            ..tavily_hikari::BusinessCalls1hSummary::default()
        },
        daily_credits_used: 0,
        daily_credits_limit: 0,
        monthly_credits_used: 0,
        monthly_credits_limit: 0,
        daily_success: 0,
        daily_failure: 0,
        monthly_success: 0,
        monthly_failure: 0,
        last_activity: None,
        recharge: tavily_hikari::LinuxDoCreditRechargeSummary::default(),
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
        AdminUsersSortField::RequestRateUsed => compare_quota_usage(
            left.summary.request_rate.used,
            left.summary.request_rate.limit,
            right.summary.request_rate.used,
            right.summary.request_rate.limit,
            direction,
        ),
        AdminUsersSortField::BusinessCalls1hUsed => compare_quota_usage(
            left.summary.business_calls_1h.total_count,
            left.summary.business_calls_1h.limit,
            right.summary.business_calls_1h.total_count,
            right.summary.business_calls_1h.limit,
            direction,
        ),
        AdminUsersSortField::DailyCreditsUsed => compare_quota_usage(
            left.summary.daily_credits_used,
            left.summary.daily_credits_limit,
            right.summary.daily_credits_used,
            right.summary.daily_credits_limit,
            direction,
        ),
        AdminUsersSortField::MonthlyCreditsUsed => compare_quota_usage(
            left.summary.monthly_credits_used,
            left.summary.monthly_credits_limit,
            right.summary.monthly_credits_used,
            right.summary.monthly_credits_limit,
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
        AdminUsersSortField::RecentIpCount7d => {
            direction.apply(left.recent_ip_count_7d.cmp(&right.recent_ip_count_7d))
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
