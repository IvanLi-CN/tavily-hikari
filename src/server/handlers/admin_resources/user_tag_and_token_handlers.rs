async fn list_user_tags(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<ListUserTagsResponse>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let items = state
        .proxy
        .list_user_tags()
        .await
        .map_err(|err| admin_proxy_error_response("list user tags error", err))?
        .iter()
        .map(build_admin_user_tag_view)
        .collect();
    Ok(Json(ListUserTagsResponse { items }))
}

async fn create_user_tag(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserTagMutationRequest>,
) -> Result<Json<AdminUserTagView>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let name = payload.name.trim();
    let display_name = payload.display_name.trim();
    if name.is_empty() || display_name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "name and displayName are required".to_string(),
        ));
    }
    let icon = normalize_optional_text(payload.icon);
    let tag = state
        .proxy
        .create_user_tag(
            name,
            display_name,
            icon.as_deref(),
            payload.effect_kind.trim(),
            payload.hourly_any_delta,
            payload.hourly_delta,
            payload.daily_delta,
            payload.monthly_delta,
        )
        .await
        .map_err(|err| admin_proxy_error_response("create user tag error", err))?;
    Ok(Json(build_admin_user_tag_view(&tag)))
}

async fn update_user_tag(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(tag_id): Path<String>,
    Json(payload): Json<UserTagMutationRequest>,
) -> Result<Json<AdminUserTagView>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let name = payload.name.trim();
    let display_name = payload.display_name.trim();
    if name.is_empty() || display_name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "name and displayName are required".to_string(),
        ));
    }
    let icon = normalize_optional_text(payload.icon);
    let Some(tag) = state
        .proxy
        .update_user_tag(
            &tag_id,
            name,
            display_name,
            icon.as_deref(),
            payload.effect_kind.trim(),
            payload.hourly_any_delta,
            payload.hourly_delta,
            payload.daily_delta,
            payload.monthly_delta,
        )
        .await
        .map_err(|err| admin_proxy_error_response("update user tag error", err))?
    else {
        return Err((StatusCode::NOT_FOUND, "user tag not found".to_string()));
    };
    Ok(Json(build_admin_user_tag_view(&tag)))
}

async fn delete_user_tag(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(tag_id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let deleted = state
        .proxy
        .delete_user_tag(&tag_id)
        .await
        .map_err(|err| admin_proxy_error_response("delete user tag error", err))?;
    if !deleted {
        return Err((StatusCode::NOT_FOUND, "user tag not found".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn bind_user_tag(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<BindUserTagRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let tag_id = payload.tag_id.trim();
    if tag_id.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "tagId is required".to_string()));
    }
    let bound = state
        .proxy
        .bind_user_tag_to_user(&id, tag_id)
        .await
        .map_err(|err| admin_proxy_error_response("bind user tag error", err))?;
    if !bound {
        return Err((StatusCode::NOT_FOUND, "user or tag not found".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn unbind_user_tag(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((id, tag_id)): Path<(String, String)>,
) -> Result<StatusCode, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let unbound = state
        .proxy
        .unbind_user_tag_from_user(&id, &tag_id)
        .await
        .map_err(|err| admin_proxy_error_response("unbind user tag error", err))?;
    if !unbound {
        return Err((
            StatusCode::NOT_FOUND,
            "user tag binding not found".to_string(),
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn list_users(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<ListUsersQuery>,
) -> Result<Json<ListUsersResponse>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let page = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(20).clamp(1, 100);
    let requested_sort = q.sort;
    let requested_order = if requested_sort.is_some() {
        Some(q.order.unwrap_or(AdminUsersSortDirection::Desc))
    } else {
        None
    };
    let effective_sort_field = requested_sort.unwrap_or(AdminUsersSortField::LastLoginAt);
    let effective_sort_order = requested_order.unwrap_or(AdminUsersSortDirection::Desc);
    let use_default_paged_query =
        requested_sort.is_none()
            || (effective_sort_field == AdminUsersSortField::LastLoginAt
                && effective_sort_order == AdminUsersSortDirection::Desc);

    let (paged_rows, total) = if use_default_paged_query {
        let (users, total) = state
            .proxy
            .list_admin_users_paged(page, per_page, q.q.as_deref(), q.tag_id.as_deref())
            .await
            .map_err(|err| {
                eprintln!("list admin users error: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        let user_ids: Vec<String> = users.iter().map(|user| user.user_id.clone()).collect();
        let summaries = state
            .proxy
            .user_dashboard_summaries_for_users(&user_ids, None)
            .await
            .map_err(|err| {
                eprintln!("list admin users dashboard summaries error: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        let monthly_broken_counts = state
            .proxy
            .fetch_monthly_broken_counts_for_users(&user_ids)
            .await
            .map_err(|err| {
                eprintln!("list admin users monthly broken counts error: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        let monthly_broken_limits = state
            .proxy
            .fetch_account_monthly_broken_limits_bulk(&user_ids)
            .await
            .map_err(|err| {
                eprintln!("list admin users monthly broken limits error: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        let rows: Vec<AdminUserSummaryRow> = users
            .into_iter()
            .map(|user| AdminUserSummaryRow {
                summary: summaries
                    .get(&user.user_id)
                    .cloned()
                    .unwrap_or_else(empty_user_dashboard_summary),
                monthly_broken_count: monthly_broken_counts
                    .get(&user.user_id)
                    .copied()
                    .unwrap_or_default(),
                monthly_broken_limit: monthly_broken_limits
                    .get(&user.user_id)
                    .copied()
                    .unwrap_or(USER_MONTHLY_BROKEN_LIMIT_DEFAULT),
                user,
            })
            .collect();
        (rows, total)
    } else {
        let users = state
            .proxy
            .list_admin_users_filtered(q.q.as_deref(), q.tag_id.as_deref())
            .await
            .map_err(|err| {
                eprintln!("list admin users error: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        let user_ids: Vec<String> = users.iter().map(|user| user.user_id.clone()).collect();
        let summaries = state
            .proxy
            .user_dashboard_summaries_for_users(&user_ids, None)
            .await
            .map_err(|err| {
                eprintln!("list admin users dashboard summaries error: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        let monthly_broken_counts = state
            .proxy
            .fetch_monthly_broken_counts_for_users(&user_ids)
            .await
            .map_err(|err| {
                eprintln!("list admin users monthly broken counts error: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        let monthly_broken_limits = state
            .proxy
            .fetch_account_monthly_broken_limits_bulk(&user_ids)
            .await
            .map_err(|err| {
                eprintln!("list admin users monthly broken limits error: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        let mut rows: Vec<AdminUserSummaryRow> = users
            .into_iter()
            .map(|user| AdminUserSummaryRow {
                summary: summaries
                    .get(&user.user_id)
                    .cloned()
                    .unwrap_or_else(empty_user_dashboard_summary),
                monthly_broken_count: monthly_broken_counts
                    .get(&user.user_id)
                    .copied()
                    .unwrap_or_default(),
                monthly_broken_limit: monthly_broken_limits
                    .get(&user.user_id)
                    .copied()
                    .unwrap_or(USER_MONTHLY_BROKEN_LIMIT_DEFAULT),
                user,
            })
            .collect();
        rows.sort_by(|left, right| {
            compare_admin_user_rows(
                left,
                right,
                Some(effective_sort_field),
                Some(effective_sort_order),
            )
        });
        let total = rows.len() as i64;
        let offset = ((page - 1) * per_page) as usize;
        let paged_rows = rows
            .into_iter()
            .skip(offset)
            .take(per_page as usize)
            .collect();
        (paged_rows, total)
    };
    let page_user_ids: Vec<String> = paged_rows
        .iter()
        .map(|row| row.user.user_id.clone())
        .collect();
    let mut user_tags = if page_user_ids.is_empty() {
        std::collections::HashMap::new()
    } else {
        state
            .proxy
            .list_user_tag_bindings_for_users(&page_user_ids)
            .await
            .map_err(|err| {
                eprintln!("list admin user tags error: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?
    };
    let mut items = Vec::with_capacity(paged_rows.len());
    let api_key_counts = if page_user_ids.is_empty() {
        std::collections::HashMap::new()
    } else {
        state
            .proxy
            .list_api_key_binding_counts_for_users(&page_user_ids)
            .await
            .map_err(|err| {
                eprintln!("list admin user api key counts error: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?
    };
    for row in paged_rows {
        let tags = user_tags.remove(&row.user.user_id).unwrap_or_default();
        let api_key_count = api_key_counts
            .get(&row.user.user_id)
            .copied()
            .unwrap_or_default();
        items.push(build_admin_user_summary_view(
            &row.user,
            &row.summary,
            api_key_count,
            row.monthly_broken_count,
            row.monthly_broken_limit,
            tags,
        ));
    }
    Ok(Json(ListUsersResponse {
        items,
        total,
        page,
        per_page,
    }))
}

async fn list_unbound_token_usage(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<ListUnboundTokenUsageQuery>,
) -> Result<Json<ListUnboundTokenUsageResponse>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    let page = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(20).clamp(1, 100);
    let normalized_query = q.q.as_deref().map(str::trim).filter(|value| !value.is_empty());

    let tokens = state.proxy.list_access_tokens().await.map_err(|err| {
        eprintln!("list unbound token usage tokens error: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let token_ids: Vec<String> = tokens.iter().map(|token| token.id.clone()).collect();
    let owners = state
        .proxy
        .get_admin_token_owners(&token_ids)
        .await
        .map_err(|err| {
            eprintln!("list unbound token usage owners error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let filtered_tokens: Vec<AuthToken> = tokens
        .into_iter()
        .filter(|token| !owners.contains_key(&token.id))
        .filter(|token| {
            normalized_query
                .map(|query| token_usage_matches_query(token, query))
                .unwrap_or(true)
        })
        .collect();
    let filtered_ids: Vec<String> = filtered_tokens
        .iter()
        .map(|token| token.id.clone())
        .collect();

    let hourly_any_map = state
        .proxy
        .token_hourly_any_snapshot(&filtered_ids)
        .await
        .map_err(|err| {
            eprintln!("list unbound token usage hourly-any error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let log_metrics = state
        .proxy
        .token_log_metrics_for_tokens(&filtered_ids)
        .await
        .map_err(|err| {
            eprintln!("list unbound token usage log metrics error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let monthly_broken_counts = state
        .proxy
        .fetch_monthly_broken_counts_for_tokens(&filtered_ids)
        .await
        .map_err(|err| {
            eprintln!("list unbound token usage monthly broken counts error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let monthly_broken_subjects = state
        .proxy
        .list_monthly_broken_subjects_for_tokens(&filtered_ids)
        .await
        .map_err(|err| {
            eprintln!("list unbound token usage monthly broken subjects error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let mut rows: Vec<AdminUnboundTokenUsageRow> = filtered_tokens
        .into_iter()
        .map(|token| {
            let hourly_any = hourly_any_map.get(&token.id).cloned().unwrap_or_else(|| {
                state
                    .proxy
                    .default_request_rate_verdict(tavily_hikari::RequestRateScope::Token)
            });
            let metrics = log_metrics.get(&token.id).cloned().unwrap_or_default();
            let has_monthly_broken_record = monthly_broken_subjects.contains(&token.id);
            AdminUnboundTokenUsageRow {
                last_used_at: metrics.last_activity.or(token.last_used_at),
                monthly_broken_count: has_monthly_broken_record.then(|| {
                    monthly_broken_counts
                        .get(&token.id)
                        .copied()
                        .unwrap_or_default()
                }),
                monthly_broken_limit: has_monthly_broken_record
                    .then_some(UNBOUND_TOKEN_MONTHLY_BROKEN_LIMIT_DEFAULT),
                token,
                request_rate: hourly_any.request_rate(),
                hourly_any_used: hourly_any.hourly_used,
                hourly_any_limit: hourly_any.hourly_limit,
                daily_success: metrics.daily_success,
                daily_failure: metrics.daily_failure,
                monthly_success: metrics.monthly_success,
                monthly_failure: metrics.monthly_failure,
            }
        })
        .collect();

    rows.sort_by(|left, right| {
        compare_admin_unbound_token_usage_rows(left, right, q.sort, q.order)
    });

    let total = rows.len() as i64;
    let offset = ((page - 1) * per_page) as usize;
    let items = rows
        .into_iter()
        .skip(offset)
        .take(per_page as usize)
        .map(build_admin_unbound_token_usage_view)
        .collect();

    Ok(Json(ListUnboundTokenUsageResponse {
        items,
        total,
        page,
        per_page,
    }))
}

async fn get_user_detail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<AdminUserDetailView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let Some(user) = state
        .proxy
        .get_admin_user_identity(&id)
        .await
        .map_err(|err| {
            eprintln!("get admin user identity error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
    else {
        return Err(StatusCode::NOT_FOUND);
    };

    let Some(quota_details) = state
        .proxy
        .get_admin_user_quota_details(&id)
        .await
        .map_err(|err| {
            eprintln!("get admin user quota details error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
    else {
        return Err(StatusCode::NOT_FOUND);
    };

    let summary = state
        .proxy
        .user_dashboard_summary(&user.user_id, None)
        .await
        .map_err(|err| {
            eprintln!("get admin user dashboard summary error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let api_key_count = state
        .proxy
        .list_api_key_binding_counts_for_users(std::slice::from_ref(&user.user_id))
        .await
        .map_err(|err| {
            eprintln!("get admin user api key counts error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .get(&user.user_id)
        .copied()
        .unwrap_or_default();
    let monthly_broken_count = state
        .proxy
        .fetch_monthly_broken_counts_for_users(std::slice::from_ref(&user.user_id))
        .await
        .map_err(|err| {
            eprintln!("get admin user monthly broken counts error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .get(&user.user_id)
        .copied()
        .unwrap_or_default();
    let monthly_broken_limit = state
        .proxy
        .fetch_account_monthly_broken_limit(&user.user_id)
        .await
        .map_err(|err| {
            eprintln!("get admin user monthly broken limit error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let tokens = state
        .proxy
        .list_user_tokens(&user.user_id)
        .await
        .map_err(|err| {
            eprintln!("get admin user tokens error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let mut token_items = Vec::with_capacity(tokens.len());
    for token in tokens {
        let (monthly_success, daily_success, daily_failure) = state
            .proxy
            .token_success_breakdown(&token.id, None)
            .await
            .map_err(|err| {
                eprintln!("get admin user token success breakdown error: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        token_items.push(AdminUserTokenSummaryView {
            token_id: token.id,
            enabled: token.enabled,
            note: token.note,
            created_at: token.created_at,
            last_used_at: token.last_used_at,
            total_requests: token.total_requests,
            daily_success,
            daily_failure,
            monthly_success,
        });
    }

    Ok(Json(AdminUserDetailView {
        user_id: user.user_id,
        display_name: user.display_name,
        username: user.username,
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
        tags: quota_details
            .tags
            .iter()
            .map(build_admin_user_tag_binding_view)
            .collect(),
        quota_base: build_admin_quota_view(&quota_details.base),
        effective_quota: build_admin_quota_view(&quota_details.effective),
        quota_breakdown: quota_details
            .breakdown
            .iter()
            .map(build_admin_quota_breakdown_view)
            .collect(),
        tokens: token_items,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdminUserUsageSeriesQuery {
    series: Option<String>,
}

async fn get_user_usage_series(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<AdminUserUsageSeriesQuery>,
) -> Result<Json<AdminUserUsageSeriesView>, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let Some(series_key) = q.series.as_deref() else {
        return Err((StatusCode::BAD_REQUEST, "series is required".to_string()));
    };
    let Some(series) = AdminUserUsageSeriesKind::parse(series_key) else {
        return Err((StatusCode::BAD_REQUEST, "invalid series".to_string()));
    };
    let Some(_) = state
        .proxy
        .get_admin_user_identity(&id)
        .await
        .map_err(|err| admin_proxy_error_response("get admin user identity error", err))?
    else {
        return Err((StatusCode::NOT_FOUND, "user not found".to_string()));
    };

    let usage = state
        .proxy
        .admin_user_usage_series(&id, series)
        .await
        .map_err(|err| admin_proxy_error_response("get admin user usage series error", err))?;

    Ok(Json(AdminUserUsageSeriesView {
        limit: usage.limit,
        points: usage
            .points
            .into_iter()
            .map(|point| AdminUserUsageSeriesPointView {
                bucket_start: point.bucket_start,
                display_bucket_start: point.display_bucket_start,
                value: point.value,
                limit_value: point.limit_value,
            })
            .collect(),
    }))
}

async fn update_user_quota(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<UpdateUserQuotaRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    let _legacy_hourly_any_limit = payload.hourly_any_limit;
    if payload.hourly_limit < 0
        || payload.daily_limit < 0
        || payload.monthly_limit < 0
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "quota base values must be non-negative integers".to_string(),
        ));
    }
    let updated = state
        .proxy
        .update_account_business_quota_limits(
            &id,
            payload.hourly_limit,
            payload.daily_limit,
            payload.monthly_limit,
        )
        .await
        .map_err(|err| admin_proxy_error_response("update user quota error", err))?;
    if !updated {
        return Err((StatusCode::NOT_FOUND, "user not found".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn update_user_broken_key_limit(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<UpdateUserBrokenKeyLimitRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    if payload.monthly_broken_limit < 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "monthlyBrokenLimit must be a non-negative integer".to_string(),
        ));
    }
    let updated = state
        .proxy
        .update_account_monthly_broken_limit(&id, payload.monthly_broken_limit)
        .await
        .map_err(|err| admin_proxy_error_response("update user broken key limit error", err))?;
    if !updated {
        return Err((StatusCode::NOT_FOUND, "user not found".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn get_user_monthly_broken_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<BrokenKeysPageQuery>,
) -> Result<Json<PaginatedMonthlyBrokenKeysView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let exists = state
        .proxy
        .get_admin_user_identity(&id)
        .await
        .map_err(|err| {
            eprintln!("get admin user identity for broken keys error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if exists.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    state
        .proxy
        .fetch_user_monthly_broken_keys(&id, q.page.unwrap_or(1), q.per_page.unwrap_or(20))
        .await
        .map(build_monthly_broken_keys_view)
        .map(Json)
        .map_err(|err| {
            eprintln!("get user monthly broken keys error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

// ----- Access token management handlers -----

#[derive(Debug, Deserialize)]
struct ListTokensQuery {
    page: Option<i64>,
    per_page: Option<i64>,
    group: Option<String>,
    no_group: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListTokensResponse {
    items: Vec<AuthTokenView>,
    total: i64,
    page: i64,
    per_page: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenGroupView {
    name: String,
    token_count: i64,
    latest_created_at: i64,
}

async fn build_auth_token_views(
    state: &Arc<AppState>,
    items: Vec<AuthToken>,
) -> Result<Vec<AuthTokenView>, ProxyError> {
    if items.is_empty() {
        return Ok(Vec::new());
    }

    let token_ids: Vec<String> = items.iter().map(|token| token.id.clone()).collect();
    let owners = state.proxy.get_admin_token_owners(&token_ids).await?;
    Ok(items
        .into_iter()
        .map(|token| {
            let owner = owners.get(&token.id);
            AuthTokenView::from_token_and_owner(token, owner)
        })
        .collect())
}

async fn list_tokens(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<ListTokensQuery>,
) -> Result<Json<ListTokensResponse>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let page = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(10).clamp(1, 200);
    let group = q
        .group
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let no_group = q.no_group.unwrap_or(false);

    if no_group {
        match state.proxy.list_access_tokens().await {
            Ok(items) => {
                let filtered: Vec<AuthToken> = items
                    .into_iter()
                    .filter(|t| {
                        t.group_name
                            .as_deref()
                            .map(str::trim)
                            .map(|g| g.is_empty())
                            .unwrap_or(true)
                    })
                    .collect();
                let total = filtered.len() as i64;
                let start = ((page - 1) * per_page).max(0) as usize;
                let end = start.saturating_add(per_page as usize).min(total as usize);
                let slice = if start >= total as usize {
                    Vec::new()
                } else {
                    filtered[start..end].to_vec()
                };
                Ok(Json(ListTokensResponse {
                    items: build_auth_token_views(&state, slice).await.map_err(|err| {
                        eprintln!("list tokens owner resolution error: {err}");
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?,
                    total,
                    page,
                    per_page,
                }))
            }
            Err(err) => {
                eprintln!("list tokens (no_group filter) error: {err}");
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    } else if let Some(group) = group {
        match state.proxy.list_access_tokens().await {
            Ok(items) => {
                let filtered: Vec<AuthToken> = items
                    .into_iter()
                    .filter(|t| t.group_name.as_deref() == Some(group.as_str()))
                    .collect();
                let total = filtered.len() as i64;
                let start = ((page - 1) * per_page).max(0) as usize;
                let end = start.saturating_add(per_page as usize).min(total as usize);
                let slice = if start >= total as usize {
                    Vec::new()
                } else {
                    filtered[start..end].to_vec()
                };
                Ok(Json(ListTokensResponse {
                    items: build_auth_token_views(&state, slice).await.map_err(|err| {
                        eprintln!("list tokens owner resolution error: {err}");
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?,
                    total,
                    page,
                    per_page,
                }))
            }
            Err(err) => {
                eprintln!("list tokens (group filter) error: {err}");
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    } else {
        match state.proxy.list_access_tokens_paged(page, per_page).await {
            Ok((items, total)) => Ok(Json(ListTokensResponse {
                items: build_auth_token_views(&state, items).await.map_err(|err| {
                    eprintln!("list tokens owner resolution error: {err}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?,
                total,
                page,
                per_page,
            })),
            Err(err) => {
                eprintln!("list tokens error: {err}");
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }
}

#[axum::debug_handler]
async fn list_token_groups(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<TokenGroupView>>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    match state.proxy.list_access_tokens().await {
        Ok(tokens) => {
            let mut groups: HashMap<String, TokenGroupView> = HashMap::new();
            for t in tokens {
                let raw = t.group_name.as_deref().map(str::trim).unwrap_or("");
                let key = raw.to_owned();
                let entry = groups.entry(key.clone()).or_insert(TokenGroupView {
                    name: key.clone(),
                    token_count: 0,
                    latest_created_at: t.created_at,
                });
                entry.token_count += 1;
                if t.created_at > entry.latest_created_at {
                    entry.latest_created_at = t.created_at;
                }
            }
            let mut out: Vec<TokenGroupView> = groups.into_values().collect();
            out.sort_by(|a, b| {
                b.latest_created_at
                    .cmp(&a.latest_created_at)
                    .then_with(|| a.name.cmp(&b.name))
            });
            Ok(Json(out))
        }
        Err(err) => {
            eprintln!("list token groups error: {err}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[axum::debug_handler]
async fn create_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateTokenRequest>,
) -> Result<(StatusCode, Json<AuthTokenSecretView>), StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    state
        .proxy
        .create_access_token(payload.note.as_deref())
        .await
        .map(|secret| {
            (
                StatusCode::CREATED,
                Json(AuthTokenSecretView {
                    token: secret.token,
                }),
            )
        })
        .map_err(|err| {
            eprintln!("create token error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn delete_token(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    state
        .proxy
        .delete_access_token(&id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|err| {
            eprintln!("delete token error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

#[derive(Debug, Deserialize)]
struct UpdateTokenStatus {
    enabled: bool,
}

async fn update_token_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<UpdateTokenStatus>,
) -> Result<StatusCode, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    state
        .proxy
        .set_access_token_enabled(&id, payload.enabled)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|err| {
            eprintln!("update token status error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

#[derive(Debug, Deserialize)]
struct UpdateTokenNote {
    note: String,
}

async fn update_token_note(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<UpdateTokenNote>,
) -> Result<StatusCode, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    state
        .proxy
        .update_access_token_note(&id, payload.note.trim())
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|err| {
            eprintln!("update token note error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn get_token_secret(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<AuthTokenSecretView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    match state.proxy.get_access_token_secret(&id).await {
        Ok(Some(secret)) => Ok(Json(AuthTokenSecretView {
            token: secret.token,
        })),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(err) => {
            eprintln!("get token secret error: {err}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[axum::debug_handler]
async fn rotate_token_secret(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<AuthTokenSecretView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    state
        .proxy
        .rotate_access_token_secret(&id)
        .await
        .map(|secret| {
            Json(AuthTokenSecretView {
                token: secret.token,
            })
        })
        .map_err(|err| {
            eprintln!("rotate token secret error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

#[derive(Debug, Deserialize)]
struct BatchCreateTokenRequest {
    group: String,
    count: usize,
    note: Option<String>,
}

#[derive(Debug, Serialize)]
struct BatchCreateTokenResponse {
    tokens: Vec<String>,
}

#[axum::debug_handler]
async fn create_tokens_batch(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<BatchCreateTokenRequest>,
) -> Result<Json<BatchCreateTokenResponse>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let group = payload.group.trim();
    if group.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let count = payload.count.clamp(1, 1000);
    state
        .proxy
        .create_access_tokens_batch(group, count, payload.note.as_deref())
        .await
        .map(|secrets| {
            Json(BatchCreateTokenResponse {
                tokens: secrets.into_iter().map(|s| s.token).collect(),
            })
        })
        .map_err(|err| {
            eprintln!("batch create tokens error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}
