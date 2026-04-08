#[derive(Deserialize)]
struct JobsQuery {
    limit: Option<usize>,
    group: Option<String>,
    page: Option<usize>,
    per_page: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PaginatedJobsView {
    items: Vec<JobLogView>,
    total: i64,
    page: usize,
    per_page: usize,
    group_counts: JobGroupCountsView,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JobGroupCountsView {
    all: i64,
    quota: i64,
    usage: i64,
    logs: i64,
    geo: i64,
    linuxdo: i64,
}

async fn list_jobs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<JobsQuery>,
) -> Result<Json<PaginatedJobsView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let page = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.or(q.limit).unwrap_or(10).clamp(1, 100);
    let group = q.group.as_deref().unwrap_or("all");

    state
        .proxy
        .list_recent_jobs_paginated(group, page, per_page)
        .await
        .map(|(items, total, group_counts)| {
            let view_items = items
                .into_iter()
                .map(|j| JobLogView {
                    id: j.id,
                    job_type: j.job_type,
                    key_id: j.key_id,
                    key_group: j.key_group,
                    status: j.status,
                    attempt: j.attempt,
                    message: j.message,
                    started_at: j.started_at,
                    finished_at: j.finished_at,
                })
                .collect();
            Json(PaginatedJobsView {
                items: view_items,
                total,
                page,
                per_page,
                group_counts: JobGroupCountsView {
                    all: group_counts.all,
                    quota: group_counts.quota,
                    usage: group_counts.usage,
                    logs: group_counts.logs,
                    geo: group_counts.geo,
                    linuxdo: group_counts.linuxdo,
                },
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

// ---- Key detail & manual quota sync ----

async fn get_api_key_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ApiKeyView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let items = state
        .proxy
        .get_api_key_metric(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(found) = items {
        Ok(Json(ApiKeyView::from_detail(found)))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn post_sync_key_usage(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    match run_manual_key_quota_sync(state.as_ref(), &id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT.into_response()),
        Err(err) => Ok((
            err.status_code,
            Json(json!({
                "error": err.error_code,
                "detail": err.detail,
            })),
        )
            .into_response()),
    }
}

#[derive(Debug)]
struct ManualQuotaSyncError {
    status_code: StatusCode,
    error_code: &'static str,
    detail: String,
}

impl ManualQuotaSyncError {
    fn new(status_code: StatusCode, error_code: &'static str, detail: String) -> Self {
        Self {
            status_code,
            error_code,
            detail,
        }
    }
}

async fn run_manual_key_quota_sync(
    state: &AppState,
    key_id: &str,
) -> Result<(), ManualQuotaSyncError> {
    let job_id = state
        .proxy
        .scheduled_job_start("quota_sync/manual", Some(key_id), 1)
        .await
        .map_err(|err| {
            ManualQuotaSyncError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "sync_failed",
                err.to_string(),
            )
        })?;

    match state
        .proxy
        .sync_key_quota(key_id, &state.usage_base, "quota_sync/manual")
        .await
    {
        Ok((limit, remaining)) => {
            let msg = format!("limit={limit} remaining={remaining}");
            let _ = state
                .proxy
                .scheduled_job_finish(job_id, "success", Some(&msg))
                .await;
            Ok(())
        }
        Err(ProxyError::QuotaDataMissing { reason }) => {
            let msg = format!("quota_data_missing: {reason}");
            let _ = state
                .proxy
                .scheduled_job_finish(job_id, "error", Some(&msg))
                .await;
            Err(ManualQuotaSyncError::new(
                StatusCode::BAD_REQUEST,
                "quota_data_missing",
                reason,
            ))
        }
        Err(ProxyError::UsageHttp { status, body }) => {
            let detail = format!("Tavily usage request failed with {status}: {body}");
            let http_status = if status == reqwest::StatusCode::UNAUTHORIZED {
                StatusCode::UNAUTHORIZED
            } else if status == reqwest::StatusCode::FORBIDDEN {
                StatusCode::FORBIDDEN
            } else if status.is_client_error() {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::BAD_GATEWAY
            };
            let _ = state
                .proxy
                .scheduled_job_finish(job_id, "error", Some(&detail))
                .await;
            Err(ManualQuotaSyncError::new(
                http_status,
                "usage_http",
                detail,
            ))
        }
        Err(err) => {
            let reason = err.to_string();
            let _ = state
                .proxy
                .scheduled_job_finish(job_id, "error", Some(&reason))
                .await;
            Err(ManualQuotaSyncError::new(
                StatusCode::BAD_GATEWAY,
                "sync_failed",
                reason,
            ))
        }
    }
}

async fn delete_api_key_quarantine(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    state
        .proxy
        .clear_key_quarantine_by_id_with_actor(
            &id,
            admin_maintenance_actor(state.as_ref(), &headers, None).await,
        )
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|err| {
            eprintln!("clear api key quarantine error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VersionView {
    backend: String,
    frontend: String,
}

async fn get_versions(State(state): State<Arc<AppState>>) -> Result<Json<VersionView>, StatusCode> {
    let (backend, frontend) = detect_versions(state.static_dir.as_deref());
    Ok(Json(VersionView { backend, frontend }))
}

#[derive(Debug, Serialize)]
struct AdminDebug {
    dev_open_admin: bool,
}

async fn get_admin_debug(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AdminDebug>, StatusCode> {
    Ok(Json(AdminDebug {
        dev_open_admin: state.dev_open_admin,
    }))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileView {
    display_name: Option<String>,
    is_admin: bool,
    forward_auth_enabled: bool,
    builtin_auth_enabled: bool,
    allow_registration: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_logged_in: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_display_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ForwardAuthDebugView {
    enabled: bool,
    user_header: Option<String>,
    admin_value: Option<String>,
    nickname_header: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminRegistrationSettingsView {
    allow_registration: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateAdminRegistrationSettingsRequest {
    allow_registration: bool,
}

async fn get_forward_auth_debug(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<ForwardAuthDebugView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let cfg = &state.forward_auth;
    Ok(Json(ForwardAuthDebugView {
        enabled: state.forward_auth_enabled && cfg.is_enabled(),
        user_header: cfg.user_header().map(|h| h.to_string()),
        admin_value: None,
        nickname_header: cfg.nickname_header().map(|h| h.to_string()),
    }))
}

async fn debug_headers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<serde_json::Value>), StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let mut map = serde_json::Map::new();
    for (k, v) in headers.iter() {
        map.insert(
            k.as_str().to_string(),
            serde_json::Value::String(v.to_str().unwrap_or("").to_string()),
        );
    }
    Ok((StatusCode::OK, Json(serde_json::Value::Object(map))))
}

async fn get_profile(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<ProfileView>, StatusCode> {
    let config = &state.forward_auth;
    let allow_registration = state.proxy.allow_registration().await.map_err(|err| {
        eprintln!("get allow registration setting error: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let forward_auth_enabled = state.forward_auth_enabled && config.is_enabled();
    let builtin_auth_enabled = state.builtin_admin.is_enabled();

    if state.dev_open_admin {
        return Ok(Json(ProfileView {
            display_name: Some("dev-mode".to_string()),
            is_admin: true,
            forward_auth_enabled,
            builtin_auth_enabled,
            allow_registration,
            user_logged_in: None,
            user_provider: None,
            user_display_name: None,
        }));
    }

    let forward_user_value = if forward_auth_enabled {
        config.user_value(&headers).map(str::to_string)
    } else {
        None
    };

    let forward_nickname = if forward_auth_enabled {
        config
            .nickname_value(&headers)
            .or_else(|| forward_user_value.clone())
    } else {
        None
    };

    let is_admin = is_admin_request(state.as_ref(), &headers);

    let display_name = forward_nickname
        .or_else(|| config.admin_override_name().map(str::to_string))
        .or_else(|| is_admin.then(|| "admin".to_string()));

    let user_session = resolve_user_session(state.as_ref(), &headers).await;
    let user_logged_in = if state.linuxdo_oauth.is_enabled_and_configured() {
        Some(user_session.is_some())
    } else {
        None
    };
    let user_provider = user_session
        .as_ref()
        .map(|session| session.user.provider.clone());
    let user_display_name = user_session.as_ref().and_then(|session| {
        session
            .user
            .display_name
            .clone()
            .or_else(|| session.user.username.clone())
    });

    Ok(Json(ProfileView {
        display_name,
        is_admin,
        forward_auth_enabled,
        builtin_auth_enabled,
        allow_registration,
        user_logged_in,
        user_provider,
        user_display_name,
    }))
}

async fn get_admin_registration_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<AdminRegistrationSettingsView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let allow_registration = state.proxy.allow_registration().await.map_err(|err| {
        eprintln!("get admin registration settings error: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(AdminRegistrationSettingsView { allow_registration }))
}

async fn patch_admin_registration_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    payload: Result<Json<UpdateAdminRegistrationSettingsRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<AdminRegistrationSettingsView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let Json(payload) = payload.map_err(|err| {
        eprintln!("patch admin registration settings payload error: {err}");
        StatusCode::BAD_REQUEST
    })?;
    let allow_registration = state
        .proxy
        .set_allow_registration(payload.allow_registration)
        .await
        .map_err(|err| {
            eprintln!("patch admin registration settings error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(AdminRegistrationSettingsView { allow_registration }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdminLoginRequest {
    password: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminLoginResponse {
    ok: bool,
}

fn session_set_cookie(token: &str, secure: bool) -> Result<HeaderValue, StatusCode> {
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{name}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}",
        name = BUILTIN_ADMIN_COOKIE_NAME,
        max_age = BUILTIN_ADMIN_SESSION_MAX_AGE_SECS,
        secure = secure
    );
    HeaderValue::from_str(&cookie).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn session_clear_cookie(secure: bool) -> Result<HeaderValue, StatusCode> {
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{name}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}",
        name = BUILTIN_ADMIN_COOKIE_NAME,
        secure = secure
    );
    HeaderValue::from_str(&cookie).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn user_session_set_cookie(
    token: &str,
    max_age_secs: i64,
    secure: bool,
) -> Result<HeaderValue, StatusCode> {
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{name}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}",
        name = USER_SESSION_COOKIE_NAME,
        max_age = max_age_secs.max(60),
        secure = secure
    );
    HeaderValue::from_str(&cookie).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn user_session_clear_cookie(secure: bool) -> Result<HeaderValue, StatusCode> {
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{name}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}",
        name = USER_SESSION_COOKIE_NAME,
        secure = secure
    );
    HeaderValue::from_str(&cookie).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn oauth_login_binding_set_cookie(
    token: &str,
    max_age_secs: i64,
    secure: bool,
) -> Result<HeaderValue, StatusCode> {
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{name}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}",
        name = OAUTH_LOGIN_BINDING_COOKIE_NAME,
        max_age = max_age_secs.max(60),
        secure = secure
    );
    HeaderValue::from_str(&cookie).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn oauth_login_binding_clear_cookie(secure: bool) -> Result<HeaderValue, StatusCode> {
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{name}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}",
        name = OAUTH_LOGIN_BINDING_COOKIE_NAME,
        secure = secure
    );
    HeaderValue::from_str(&cookie).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn new_cookie_nonce() -> String {
    use base64::Engine as _;
    use rand::RngCore as _;

    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn hash_oauth_binding(nonce: &str) -> String {
    use base64::Engine as _;
    let digest = Sha256::digest(nonce.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

fn map_oauth_upstream_transport_error(err: &reqwest::Error) -> StatusCode {
    if err.is_timeout() {
        StatusCode::GATEWAY_TIMEOUT
    } else {
        StatusCode::BAD_GATEWAY
    }
}

fn map_oauth_upstream_status(status: reqwest::StatusCode) -> StatusCode {
    if status.is_server_error() {
        return StatusCode::BAD_GATEWAY;
    }
    match status {
        reqwest::StatusCode::BAD_REQUEST => StatusCode::BAD_REQUEST,
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
            StatusCode::UNAUTHORIZED
        }
        reqwest::StatusCode::TOO_MANY_REQUESTS => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::BAD_GATEWAY,
    }
}

async fn post_admin_login(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<AdminLoginRequest>,
) -> Result<Response<Body>, StatusCode> {
    if !state.builtin_admin.is_enabled() {
        return Err(StatusCode::NOT_FOUND);
    }
    let password = payload.password.trim();
    let Some(token) = state.builtin_admin.login(password) else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    state.builtin_admin.remember_session(token.clone());
    let cookie = session_set_cookie(&token, wants_secure_cookie(&headers))?;
    Ok((
        StatusCode::OK,
        [(SET_COOKIE, cookie)],
        Json(AdminLoginResponse { ok: true }),
    )
        .into_response())
}

async fn post_admin_logout(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    if !state.builtin_admin.is_enabled() {
        return Err(StatusCode::NOT_FOUND);
    }
    state.builtin_admin.forget_session(&headers);
    let cookie = session_clear_cookie(wants_secure_cookie(&headers))?;
    Ok((StatusCode::NO_CONTENT, [(SET_COOKIE, cookie)]).into_response())
}
