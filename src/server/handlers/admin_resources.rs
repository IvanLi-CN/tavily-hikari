fn detect_versions(static_dir: Option<&FsPath>) -> (String, String) {
    let backend_base = option_env!("APP_EFFECTIVE_VERSION")
        .map(|s| s.to_string())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    let backend = if cfg!(debug_assertions) {
        format!("{}-dev", backend_base)
    } else {
        backend_base
    };

    // Try reading version.json produced by front-end build
    let frontend_from_dist = static_dir.and_then(|dir| {
        let path = dir.join("version.json");
        fs::File::open(&path).ok().and_then(|mut f| {
            let mut s = String::new();
            if f.read_to_string(&mut s).is_ok() {
                serde_json::from_str::<serde_json::Value>(&s)
                    .ok()
                    .and_then(|v| {
                        v.get("version")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
            } else {
                None
            }
        })
    });

    // Fallback to web/package.json for dev setups
    let frontend = frontend_from_dist
        .or_else(|| {
            let path = FsPath::new("web").join("package.json");
            fs::File::open(&path).ok().and_then(|mut f| {
                let mut s = String::new();
                if f.read_to_string(&mut s).is_ok() {
                    serde_json::from_str::<serde_json::Value>(&s)
                        .ok()
                        .and_then(|v| {
                            v.get("version")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                        })
                } else {
                    None
                }
            })
        })
        .unwrap_or_else(|| "unknown".to_string());

    let frontend = if cfg!(debug_assertions) {
        format!("{}-dev", frontend)
    } else {
        frontend
    };

    (backend, frontend)
}

async fn list_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: axum::http::Uri,
) -> Result<Json<PaginatedApiKeysView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let query = ListKeysQuery::from_query(uri.query());
    state
        .proxy
        .list_api_key_metrics_paged(
            query.page.unwrap_or(1),
            query.per_page.unwrap_or(20),
            &query.group,
            &query.status,
        )
        .await
        .map(|result| {
            Json(PaginatedApiKeysView {
                items: result
                    .items
                    .into_iter()
                    .map(ApiKeyView::from_list)
                    .collect(),
                total: result.total,
                page: result.page,
                per_page: result.per_page,
                facets: ApiKeyFacetsView {
                    groups: result
                        .facets
                        .groups
                        .into_iter()
                        .map(|facet| ApiKeyFacetCountView {
                            value: facet.value,
                            count: facet.count,
                        })
                        .collect(),
                    statuses: result
                        .facets
                        .statuses
                        .into_iter()
                        .map(|facet| ApiKeyFacetCountView {
                            value: facet.value,
                            count: facet.count,
                        })
                        .collect(),
                },
            })
        })
        .map_err(|err| {
            eprintln!("list keys error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

#[derive(Debug, Default)]
struct ListKeysQuery {
    page: Option<i64>,
    per_page: Option<i64>,
    group: Vec<String>,
    status: Vec<String>,
}

impl ListKeysQuery {
    fn from_query(raw_query: Option<&str>) -> Self {
        let mut query = Self::default();
        let Some(raw_query) = raw_query else {
            return query;
        };

        for (key, value) in url::form_urlencoded::parse(raw_query.as_bytes()) {
            match key.as_ref() {
                "page" => {
                    if let Ok(parsed) = value.parse::<i64>() {
                        query.page = Some(parsed);
                    }
                }
                "per_page" => {
                    if let Ok(parsed) = value.parse::<i64>() {
                        query.per_page = Some(parsed);
                    }
                }
                "group" => query.group.push(value.into_owned()),
                "status" => query.status.push(value.into_owned()),
                _ => {}
            }
        }

        query
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyFacetCountView {
    value: String,
    count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyFacetsView {
    groups: Vec<ApiKeyFacetCountView>,
    statuses: Vec<ApiKeyFacetCountView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PaginatedApiKeysView {
    items: Vec<ApiKeyView>,
    total: i64,
    page: i64,
    per_page: i64,
    facets: ApiKeyFacetsView,
}

#[derive(Debug, Deserialize)]
struct CreateKeyRequest {
    api_key: String,
    group: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreateKeyResponse {
    id: String,
}

const API_KEYS_BATCH_LIMIT: usize = 1000;

#[derive(Debug, Deserialize)]
struct BatchCreateKeysRequest {
    api_keys: Vec<String>,
    group: Option<String>,
    exhausted_api_keys: Option<Vec<String>>,
}

#[derive(Debug, Default, Serialize)]
struct BatchCreateKeysSummary {
    input_lines: u64,
    valid_lines: u64,
    unique_in_input: u64,
    created: u64,
    undeleted: u64,
    existed: u64,
    duplicate_in_input: u64,
    failed: u64,
    ignored_empty: u64,
}

#[derive(Debug, Serialize)]
struct BatchCreateKeysResult {
    api_key: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    marked_exhausted: Option<bool>,
}

#[derive(Debug, Serialize)]
struct BatchCreateKeysResponse {
    summary: BatchCreateKeysSummary,
    results: Vec<BatchCreateKeysResult>,
}

#[derive(Debug, Deserialize)]
struct ValidateKeysRequest {
    api_keys: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
struct ValidateKeysSummary {
    input_lines: u64,
    valid_lines: u64,
    unique_in_input: u64,
    duplicate_in_input: u64,
    ok: u64,
    exhausted: u64,
    invalid: u64,
    error: u64,
}

#[derive(Debug, Serialize)]
struct ValidateKeyResult {
    api_key: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    quota_limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quota_remaining: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Debug, Serialize)]
struct ValidateKeysResponse {
    summary: ValidateKeysSummary,
    results: Vec<ValidateKeyResult>,
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
    api_key: String,
) -> (ValidateKeyResult, &'static str) {
    match proxy.probe_api_key_quota(&api_key, &usage_base).await {
        Ok((limit, remaining)) => {
            if remaining <= 0 {
                (
                    ValidateKeyResult {
                        api_key,
                        status: "ok_exhausted".to_string(),
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
                quota_limit: None,
                quota_remaining: None,
                detail: Some(truncate_detail(err.to_string(), 1400)),
            },
            "error",
        ),
    }
}

async fn post_validate_api_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ValidateKeysRequest>,
) -> Result<Response<Body>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    let ValidateKeysRequest { api_keys } = payload;

    let mut summary = ValidateKeysSummary {
        input_lines: api_keys.len() as u64,
        ..Default::default()
    };

    let mut trimmed = Vec::<String>::with_capacity(api_keys.len());
    for api_key in api_keys {
        let api_key = api_key.trim();
        if api_key.is_empty() {
            continue;
        }
        trimmed.push(api_key.to_string());
    }
    summary.valid_lines = trimmed.len() as u64;

    if trimmed.len() > API_KEYS_BATCH_LIMIT {
        let body = Json(json!({
            "error": "too_many_items",
            "detail": format!("api_keys exceeds limit (max {})", API_KEYS_BATCH_LIMIT),
        }));
        return Ok((StatusCode::BAD_REQUEST, body).into_response());
    }

    let mut results = Vec::<ValidateKeyResult>::with_capacity(trimmed.len());
    let mut pending = Vec::<(usize, String)>::new();
    let mut seen = HashSet::<String>::new();

    for api_key in trimmed {
        if !seen.insert(api_key.clone()) {
            summary.duplicate_in_input += 1;
            results.push(ValidateKeyResult {
                api_key,
                status: "duplicate_in_input".to_string(),
                quota_limit: None,
                quota_remaining: None,
                detail: None,
            });
            continue;
        }

        let pos = results.len();
        results.push(ValidateKeyResult {
            api_key: api_key.clone(),
            status: "pending".to_string(),
            quota_limit: None,
            quota_remaining: None,
            detail: None,
        });
        pending.push((pos, api_key));
    }

    summary.unique_in_input = seen.len() as u64;

    let proxy = state.proxy.clone();
    let usage_base = state.usage_base.clone();
    let checked = futures_stream::iter(pending.into_iter())
        .map(|(pos, api_key)| {
            let proxy = proxy.clone();
            let usage_base = usage_base.clone();
            async move {
                let (result, kind) = validate_single_key(proxy, usage_base, api_key).await;
                (pos, result, kind)
            }
        })
        .buffer_unordered(8)
        .collect::<Vec<_>>()
        .await;

    for (pos, result, kind) in checked {
        if let Some(slot) = results.get_mut(pos) {
            *slot = result;
        }
        match kind {
            "ok" => summary.ok += 1,
            "exhausted" => summary.exhausted += 1,
            "invalid" => summary.invalid += 1,
            _ => summary.error += 1,
        }
    }

    Ok((
        StatusCode::OK,
        Json(ValidateKeysResponse { summary, results }),
    )
        .into_response())
}

async fn create_api_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateKeyRequest>,
) -> Result<(StatusCode, Json<CreateKeyResponse>), StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    let CreateKeyRequest {
        api_key,
        group: group_raw,
    } = payload;
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let group = group_raw
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match state
        .proxy
        .add_or_undelete_key_in_group(api_key, group)
        .await
    {
        Ok(id) => Ok((StatusCode::CREATED, Json(CreateKeyResponse { id }))),
        Err(err) => {
            eprintln!("create api key error: {err}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn create_api_keys_batch(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<BatchCreateKeysRequest>,
) -> Result<Response<Body>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    let BatchCreateKeysRequest {
        api_keys,
        group: group_raw,
        exhausted_api_keys,
    } = payload;
    let group = group_raw
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let exhausted_set: HashSet<String> = exhausted_api_keys
        .unwrap_or_default()
        .into_iter()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .collect();

    let mut summary = BatchCreateKeysSummary {
        input_lines: api_keys.len() as u64,
        ..Default::default()
    };

    let mut trimmed = Vec::<String>::with_capacity(api_keys.len());
    for api_key in api_keys {
        let api_key = api_key.trim();
        if api_key.is_empty() {
            summary.ignored_empty += 1;
            continue;
        }
        trimmed.push(api_key.to_string());
    }
    summary.valid_lines = trimmed.len() as u64;

    if trimmed.len() > API_KEYS_BATCH_LIMIT {
        let body = Json(json!({
            "error": "too_many_items",
            "detail": format!("api_keys exceeds limit (max {})", API_KEYS_BATCH_LIMIT),
        }));
        return Ok((StatusCode::BAD_REQUEST, body).into_response());
    }

    let mut results = Vec::with_capacity(trimmed.len());
    let mut seen = HashSet::<String>::new();

    for api_key in trimmed {
        if !seen.insert(api_key.clone()) {
            summary.duplicate_in_input += 1;
            results.push(BatchCreateKeysResult {
                api_key,
                status: "duplicate_in_input".to_string(),
                id: None,
                error: None,
                marked_exhausted: None,
            });
            continue;
        }

        match state
            .proxy
            .add_or_undelete_key_with_status_in_group(&api_key, group)
            .await
        {
            Ok((id, status)) => {
                match status.as_str() {
                    "created" => summary.created += 1,
                    "undeleted" => summary.undeleted += 1,
                    "existed" => summary.existed += 1,
                    _ => {}
                }
                let mut marked_exhausted = None;
                if exhausted_set.contains(&api_key) {
                    marked_exhausted = match state
                        .proxy
                        .mark_key_quota_exhausted_by_secret(&api_key)
                        .await
                    {
                        Ok(changed) => Some(changed),
                        Err(err) => {
                            eprintln!("mark exhausted failed for key: {err}");
                            Some(false)
                        }
                    };
                }
                results.push(BatchCreateKeysResult {
                    api_key,
                    status: status.as_str().to_string(),
                    id: Some(id),
                    error: None,
                    marked_exhausted,
                });
            }
            Err(err) => {
                summary.failed += 1;
                results.push(BatchCreateKeysResult {
                    api_key,
                    status: "failed".to_string(),
                    id: None,
                    error: Some(err.to_string()),
                    marked_exhausted: None,
                });
            }
        }
    }

    summary.unique_in_input = seen.len() as u64;

    Ok((
        StatusCode::OK,
        Json(BatchCreateKeysResponse { summary, results }),
    )
        .into_response())
}

async fn delete_api_key(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    match state.proxy.soft_delete_key_by_id(&id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(err) => {
            eprintln!("delete api key error: {err}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Debug, Deserialize)]
struct UpdateKeyStatus {
    status: String,
}

async fn update_api_key_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<UpdateKeyStatus>,
) -> Result<StatusCode, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    let status = payload.status.trim().to_ascii_lowercase();
    match status.as_str() {
        "disabled" => match state.proxy.disable_key_by_id(&id).await {
            Ok(()) => Ok(StatusCode::NO_CONTENT),
            Err(err) => {
                eprintln!("disable api key error: {err}");
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        },
        "active" => match state.proxy.enable_key_by_id(&id).await {
            Ok(()) => Ok(StatusCode::NO_CONTENT),
            Err(err) => {
                eprintln!("enable api key error: {err}");
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        },
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

async fn get_api_key_secret(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ApiKeySecretView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    match state.proxy.get_api_key_secret(&id).await {
        Ok(Some(secret)) => Ok(Json(ApiKeySecretView { api_key: secret })),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(err) => {
            eprintln!("fetch api key secret error: {err}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PaginatedLogsView {
    items: Vec<RequestLogView>,
    total: i64,
    page: i64,
    per_page: i64,
}

async fn list_logs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<LogsQuery>,
) -> Result<Json<PaginatedLogsView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).clamp(1, 200);

    // Optional result_status filter: normalize to known values.
    let result_status: Option<&str> = match params.result.as_deref().map(str::trim) {
        Some(v) if v.eq_ignore_ascii_case("success") => Some("success"),
        Some(v) if v.eq_ignore_ascii_case("error") => Some("error"),
        Some(v) if v.eq_ignore_ascii_case("quota_exhausted") || v.eq_ignore_ascii_case("quota") => {
            Some("quota_exhausted")
        }
        _ => None,
    };

    state
        .proxy
        .recent_request_logs_page(result_status, page, per_page)
        .await
        .map(|(logs, total)| {
            let view_items = logs.into_iter().map(RequestLogView::from).collect();
            Json(PaginatedLogsView {
                items: view_items,
                total,
                page,
                per_page,
            })
        })
        .map_err(|err| {
            eprintln!("list logs error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

#[derive(Debug, Deserialize)]
struct ListUsersQuery {
    page: Option<i64>,
    per_page: Option<i64>,
    q: Option<String>,
    #[serde(rename = "tagId")]
    tag_id: Option<String>,
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
    last_activity: Option<i64>,
    tags: Vec<AdminUserTagBindingView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminUserTokenSummaryView {
    token_id: String,
    enabled: bool,
    note: Option<String>,
    last_used_at: Option<i64>,
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
struct AdminUserDetailView {
    user_id: String,
    display_name: Option<String>,
    username: Option<String>,
    active: bool,
    last_login_at: Option<i64>,
    token_count: i64,
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
    hourly_any_limit: i64,
    hourly_limit: i64,
    daily_limit: i64,
    monthly_limit: i64,
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

fn build_admin_user_summary_view(
    user: &tavily_hikari::AdminUserIdentity,
    summary: &tavily_hikari::UserDashboardSummary,
    tags: Vec<tavily_hikari::AdminUserTagBinding>,
) -> AdminUserSummaryView {
    AdminUserSummaryView {
        user_id: user.user_id.clone(),
        display_name: user.display_name.clone(),
        username: user.username.clone(),
        active: user.active,
        last_login_at: user.last_login_at,
        token_count: user.token_count,
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
        last_activity: summary.last_activity,
        tags: tags.iter().map(build_admin_user_tag_binding_view).collect(),
    }
}

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
    let (users, total) = state
        .proxy
        .list_admin_users_paged(page, per_page, q.q.as_deref(), q.tag_id.as_deref())
        .await
        .map_err(|err| {
            eprintln!("list admin users error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let user_ids: Vec<String> = users.iter().map(|user| user.user_id.clone()).collect();
    let mut user_tags = state
        .proxy
        .list_user_tag_bindings_for_users(&user_ids)
        .await
        .map_err(|err| {
            eprintln!("list admin user tags error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let mut items = Vec::with_capacity(users.len());
    for user in users {
        let summary = state
            .proxy
            .user_dashboard_summary(&user.user_id)
            .await
            .map_err(|err| {
                eprintln!("list admin users dashboard summary error: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        let tags = user_tags.remove(&user.user_id).unwrap_or_default();
        items.push(build_admin_user_summary_view(&user, &summary, tags));
    }
    Ok(Json(ListUsersResponse {
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
        .user_dashboard_summary(&user.user_id)
        .await
        .map_err(|err| {
            eprintln!("get admin user dashboard summary error: {err}");
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
    let token_ids: Vec<String> = tokens.iter().map(|token| token.id.clone()).collect();
    let hourly_any = state
        .proxy
        .token_hourly_any_snapshot(&token_ids)
        .await
        .map_err(|err| {
            eprintln!("get admin user token hourly snapshot error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let mut token_items = Vec::with_capacity(tokens.len());
    for token in tokens {
        let (monthly_success, daily_success, daily_failure) = state
            .proxy
            .token_success_breakdown(&token.id)
            .await
            .map_err(|err| {
                eprintln!("get admin user token success breakdown error: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        let (
            quota_hourly_used,
            quota_hourly_limit,
            quota_daily_used,
            quota_daily_limit,
            quota_monthly_used,
            quota_monthly_limit,
        ) = user_token_quota_values(&token);
        let (hourly_any_used, hourly_any_limit) = hourly_any
            .get(&token.id)
            .map(|value| (value.hourly_used, value.hourly_limit))
            .unwrap_or((0, effective_token_hourly_request_limit()));
        token_items.push(AdminUserTokenSummaryView {
            token_id: token.id,
            enabled: token.enabled,
            note: token.note,
            last_used_at: token.last_used_at,
            hourly_any_used,
            hourly_any_limit,
            quota_hourly_used,
            quota_hourly_limit,
            quota_daily_used,
            quota_daily_limit,
            quota_monthly_used,
            quota_monthly_limit,
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

async fn update_user_quota(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(payload): Json<UpdateUserQuotaRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err((StatusCode::FORBIDDEN, "forbidden".to_string()));
    }
    if payload.hourly_any_limit < 0
        || payload.hourly_limit < 0
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
        .update_account_quota_limits(
            &id,
            payload.hourly_any_limit,
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
