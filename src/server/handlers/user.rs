#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserTokenView {
    token: String,
}

#[derive(Debug, Deserialize)]
struct LinuxDoCallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LinuxDoTokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct LinuxDoAuthForm {
    token: Option<String>,
}

fn json_value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(v) => Some(v.clone()),
        Value::Number(v) => Some(v.to_string()),
        _ => None,
    }
}

fn parse_full_token_id(token: &str) -> Option<String> {
    let token = token.trim();
    let rest = token.strip_prefix("th-")?;
    let (id, secret) = rest.split_once('-')?;
    if id.len() != 4 || !id.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return None;
    }
    let secret_len_ok = secret.len() == 12 || secret.len() == 24;
    if !secret_len_ok || !secret.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return None;
    }
    Some(id.to_string())
}

async fn start_linuxdo_auth(
    state: Arc<AppState>,
    headers: HeaderMap,
    token: Option<String>,
) -> Result<Response<Body>, StatusCode> {
    let cfg = &state.linuxdo_oauth;
    if !cfg.is_enabled_and_configured() {
        return Err(StatusCode::NOT_FOUND);
    }

    let bind_token_id = if let Some(raw_token) = token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if state
            .proxy
            .validate_access_token(raw_token)
            .await
            .map_err(|err| {
                eprintln!("validate preferred token error: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })? {
            parse_full_token_id(raw_token)
        } else {
            None
        }
    } else {
        None
    };

    let binding_nonce = new_cookie_nonce();
    let binding_hash = hash_oauth_binding(&binding_nonce);
    let state_token = state
        .proxy
        .create_oauth_login_state_with_binding_and_token(
            "linuxdo",
            None,
            cfg.login_state_ttl_secs,
            Some(&binding_hash),
            bind_token_id.as_deref(),
        )
        .await
        .map_err(|err| {
            eprintln!("create linuxdo oauth state error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let mut url =
        reqwest::Url::parse(&cfg.authorize_url).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("client_id", cfg.client_id.as_deref().unwrap_or_default());
        pairs.append_pair(
            "redirect_uri",
            cfg.redirect_url.as_deref().unwrap_or_default(),
        );
        pairs.append_pair("response_type", "code");
        pairs.append_pair("scope", &cfg.scope);
        pairs.append_pair("state", &state_token);
    }

    let binding_cookie = oauth_login_binding_set_cookie(
        &binding_nonce,
        cfg.login_state_ttl_secs,
        wants_secure_cookie(&headers),
    )?;
    Ok((
        [(SET_COOKIE, binding_cookie)],
        // Use 303 to force the subsequent request to be a GET.
        //
        // This avoids browsers preserving the original POST body when following the redirect,
        // which can break OAuth authorize endpoints (GET-only) and risk leaking form fields.
        Redirect::to(url.as_ref()),
    )
        .into_response())
}

async fn get_linuxdo_auth(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    start_linuxdo_auth(state, headers, None).await
}

async fn post_linuxdo_auth(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Form(payload): Form<LinuxDoAuthForm>,
) -> Result<Response<Body>, StatusCode> {
    start_linuxdo_auth(state, headers, payload.token).await
}

async fn get_linuxdo_callback(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<LinuxDoCallbackQuery>,
) -> Result<Response<Body>, StatusCode> {
    let cfg = &state.linuxdo_oauth;
    if !cfg.is_enabled_and_configured() {
        return Err(StatusCode::NOT_FOUND);
    }
    let use_secure_cookie = wants_secure_cookie(&headers);
    let code = query
        .code
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let oauth_state = query
        .state
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let (Some(code), Some(oauth_state)) = (code, oauth_state) else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let binding_nonce = cookie_value(&headers, OAUTH_LOGIN_BINDING_COOKIE_NAME)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let binding_hash = hash_oauth_binding(&binding_nonce);

    let state_payload = state
        .proxy
        .consume_oauth_login_state_with_binding_and_token("linuxdo", oauth_state, Some(&binding_hash))
        .await
        .map_err(|err| {
            eprintln!("consume linuxdo oauth state error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let Some(state_payload) = state_payload else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let redirect_to = state_payload.redirect_to;
    let preferred_token_id = state_payload.bind_token_id;

    let client = reqwest::Client::new();
    let token_resp = client
        .post(&cfg.token_url)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&[
            ("client_id", cfg.client_id.as_deref().unwrap_or_default()),
            (
                "client_secret",
                cfg.client_secret.as_deref().unwrap_or_default(),
            ),
            ("code", code),
            (
                "redirect_uri",
                cfg.redirect_url.as_deref().unwrap_or_default(),
            ),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
        .map_err(|err| {
            eprintln!("linuxdo token request error: {err}");
            map_oauth_upstream_transport_error(&err)
        })?;
    if !token_resp.status().is_success() {
        let status = token_resp.status();
        let body = token_resp.text().await.unwrap_or_default();
        eprintln!("linuxdo token response status={} body={}", status, body);
        return Err(map_oauth_upstream_status(status));
    }
    let token_payload: LinuxDoTokenResponse = token_resp.json().await.map_err(|err| {
        eprintln!("linuxdo token parse error: {err}");
        StatusCode::BAD_GATEWAY
    })?;
    let access_token = token_payload.access_token.trim().to_string();
    if access_token.is_empty() {
        return Err(StatusCode::BAD_GATEWAY);
    }

    let user_resp = client
        .get(&cfg.userinfo_url)
        .bearer_auth(&access_token)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|err| {
            eprintln!("linuxdo userinfo request error: {err}");
            map_oauth_upstream_transport_error(&err)
        })?;
    if !user_resp.status().is_success() {
        let status = user_resp.status();
        let body = user_resp.text().await.unwrap_or_default();
        eprintln!("linuxdo userinfo response status={} body={}", status, body);
        return Err(map_oauth_upstream_status(status));
    }
    let user_json: Value = user_resp.json().await.map_err(|err| {
        eprintln!("linuxdo userinfo parse error: {err}");
        StatusCode::BAD_GATEWAY
    })?;

    let provider_user_id = user_json
        .get("id")
        .and_then(json_value_to_string)
        .filter(|v| !v.is_empty())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let allow_registration = state.proxy.allow_registration().await.map_err(|err| {
        eprintln!("read allow registration during linuxdo callback error: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    if !allow_registration {
        let existing_account = state
            .proxy
            .oauth_account_exists("linuxdo", &provider_user_id)
            .await
            .map_err(|err| {
                eprintln!("query linuxdo oauth account existence error: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        if !existing_account {
            let clear_binding_cookie = oauth_login_binding_clear_cookie(use_secure_cookie)?;
            let mut response = Redirect::temporary("/registration-paused").into_response();
            response
                .headers_mut()
                .append(SET_COOKIE, clear_binding_cookie);
            return Ok(response);
        }
    }
    let username = user_json
        .get("username")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    let name = user_json
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    let avatar_template = user_json
        .get("avatar_template")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    let active = user_json
        .get("active")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let trust_level = user_json.get("trust_level").and_then(|v| v.as_i64());
    let raw_payload_json = serde_json::to_string(&user_json).ok();

    let profile = OAuthAccountProfile {
        provider: "linuxdo".to_string(),
        provider_user_id: provider_user_id.clone(),
        username: username.clone(),
        name: name.clone(),
        avatar_template,
        active,
        trust_level,
        raw_payload_json,
    };

    let user = state
        .proxy
        .upsert_oauth_account(&profile)
        .await
        .map_err(|err| {
            eprintln!("upsert linuxdo oauth account error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let note = format!(
        "linuxdo:{}",
        username.clone().unwrap_or_else(|| provider_user_id.clone())
    );
    state
        .proxy
        .ensure_user_token_binding_with_preferred(
            &user.user_id,
            Some(&note),
            preferred_token_id.as_deref(),
        )
        .await
        .map_err(|err| {
            eprintln!("ensure user token binding error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let session = state
        .proxy
        .create_user_session(&user, cfg.session_max_age_secs)
        .await
        .map_err(|err| {
            eprintln!("create user session error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let cookie =
        user_session_set_cookie(&session.token, cfg.session_max_age_secs, use_secure_cookie)?;
    let clear_binding_cookie = oauth_login_binding_clear_cookie(use_secure_cookie)?;

    let default_target = if spa_file_exists(state.as_ref(), "console.html").await {
        "/console"
    } else if spa_file_exists(state.as_ref(), "index.html").await {
        "/"
    } else {
        "/api/profile"
    };
    let target = redirect_to.unwrap_or_else(|| default_target.to_string());
    let mut response = Redirect::temporary(&target).into_response();
    response.headers_mut().append(SET_COOKIE, cookie);
    response
        .headers_mut()
        .append(SET_COOKIE, clear_binding_cookie);
    Ok(response)
}

async fn post_user_logout(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    if !state.linuxdo_oauth.is_enabled_and_configured() {
        return Err(StatusCode::NOT_FOUND);
    }
    if let Some(token) = cookie_value(&headers, USER_SESSION_COOKIE_NAME) {
        let _ = state.proxy.revoke_user_session(&token).await;
    }
    let cookie = user_session_clear_cookie(wants_secure_cookie(&headers))?;
    Ok((StatusCode::NO_CONTENT, [(SET_COOKIE, cookie)]).into_response())
}

async fn get_user_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<UserTokenView>, StatusCode> {
    if !state.linuxdo_oauth.is_enabled_and_configured() {
        return Err(StatusCode::NOT_FOUND);
    }
    let Some(user_session) = resolve_user_session(state.as_ref(), &headers).await else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    match state.proxy.get_user_token(&user_session.user.user_id).await {
        Ok(UserTokenLookup::Found(secret)) => Ok(Json(UserTokenView {
            token: secret.token,
        })),
        Ok(UserTokenLookup::MissingBinding) => Err(StatusCode::NOT_FOUND),
        Ok(UserTokenLookup::Unavailable) => Err(StatusCode::CONFLICT),
        Err(err) => {
            eprintln!("get user token error: {err}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserDashboardView {
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
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserTokenSummaryView {
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

#[derive(Debug, Deserialize)]
struct UserTokenLogsQuery {
    limit: Option<usize>,
}

async fn get_user_dashboard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<UserDashboardView>, StatusCode> {
    if !state.linuxdo_oauth.is_enabled_and_configured() {
        return Err(StatusCode::NOT_FOUND);
    }
    let Some(user_session) = resolve_user_session(state.as_ref(), &headers).await else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    let summary = state
        .proxy
        .user_dashboard_summary(&user_session.user.user_id)
        .await
        .map_err(|err| {
            eprintln!("get user dashboard error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(UserDashboardView {
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
    }))
}

fn user_token_quota_values(token: &AuthToken) -> (i64, i64, i64, i64, i64, i64) {
    token
        .quota
        .as_ref()
        .map(|q| {
            (
                q.hourly_used,
                q.hourly_limit,
                q.daily_used,
                q.daily_limit,
                q.monthly_used,
                q.monthly_limit,
            )
        })
        .unwrap_or((
            0,
            effective_token_hourly_limit(),
            0,
            effective_token_daily_limit(),
            0,
            effective_token_monthly_limit(),
        ))
}

async fn get_user_tokens(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<UserTokenSummaryView>>, StatusCode> {
    if !state.linuxdo_oauth.is_enabled_and_configured() {
        return Err(StatusCode::NOT_FOUND);
    }
    let Some(user_session) = resolve_user_session(state.as_ref(), &headers).await else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    let tokens = state
        .proxy
        .list_user_tokens(&user_session.user.user_id)
        .await
        .map_err(|err| {
            eprintln!("list user tokens error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let token_ids: Vec<String> = tokens.iter().map(|t| t.id.clone()).collect();
    let hourly_any = state
        .proxy
        .token_hourly_any_snapshot(&token_ids)
        .await
        .map_err(|err| {
            eprintln!("list user tokens hourly snapshot error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let mut items = Vec::with_capacity(tokens.len());
    for token in tokens {
        let (monthly_success, daily_success, daily_failure) = state
            .proxy
            .token_success_breakdown(&token.id)
            .await
            .map_err(|err| {
                eprintln!("list user tokens success breakdown error: {err}");
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
            .map(|v| (v.hourly_used, v.hourly_limit))
            .unwrap_or((0, effective_token_hourly_request_limit()));
        items.push(UserTokenSummaryView {
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
    Ok(Json(items))
}

async fn get_user_token_detail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<UserTokenSummaryView>, StatusCode> {
    if !state.linuxdo_oauth.is_enabled_and_configured() {
        return Err(StatusCode::NOT_FOUND);
    }
    let Some(user_session) = resolve_user_session(state.as_ref(), &headers).await else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    let owned = state
        .proxy
        .is_user_token_bound(&user_session.user.user_id, &id)
        .await
        .map_err(|err| {
            eprintln!("get user token detail ownership error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if !owned {
        return Err(StatusCode::NOT_FOUND);
    }
    let tokens = state
        .proxy
        .list_user_tokens(&user_session.user.user_id)
        .await
        .map_err(|err| {
            eprintln!("get user token detail list error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let Some(token) = tokens.into_iter().find(|t| t.id == id) else {
        return Err(StatusCode::NOT_FOUND);
    };
    let (monthly_success, daily_success, daily_failure) = state
        .proxy
        .token_success_breakdown(&token.id)
        .await
        .map_err(|err| {
            eprintln!("get user token detail breakdown error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let hourly_any = state
        .proxy
        .token_hourly_any_snapshot(std::slice::from_ref(&token.id))
        .await
        .map_err(|err| {
            eprintln!("get user token detail hourly snapshot error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let (hourly_any_used, hourly_any_limit) = hourly_any
        .get(&token.id)
        .map(|v| (v.hourly_used, v.hourly_limit))
        .unwrap_or((0, effective_token_hourly_request_limit()));
    let (
        quota_hourly_used,
        quota_hourly_limit,
        quota_daily_used,
        quota_daily_limit,
        quota_monthly_used,
        quota_monthly_limit,
    ) = user_token_quota_values(&token);
    Ok(Json(UserTokenSummaryView {
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
    }))
}

async fn get_user_token_secret(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<UserTokenView>, StatusCode> {
    if !state.linuxdo_oauth.is_enabled_and_configured() {
        return Err(StatusCode::NOT_FOUND);
    }
    let Some(user_session) = resolve_user_session(state.as_ref(), &headers).await else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    match state
        .proxy
        .get_user_token_secret(&user_session.user.user_id, &id)
        .await
    {
        Ok(Some(token)) => Ok(Json(UserTokenView { token: token.token })),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(err) => {
            eprintln!("get user token secret error: {err}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn get_user_token_logs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<UserTokenLogsQuery>,
) -> Result<Json<Vec<PublicTokenLogView>>, StatusCode> {
    if !state.linuxdo_oauth.is_enabled_and_configured() {
        return Err(StatusCode::NOT_FOUND);
    }
    let Some(user_session) = resolve_user_session(state.as_ref(), &headers).await else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    let owned = state
        .proxy
        .is_user_token_bound(&user_session.user.user_id, &id)
        .await
        .map_err(|err| {
            eprintln!("get user token logs ownership error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if !owned {
        return Err(StatusCode::NOT_FOUND);
    }
    let limit = q.limit.unwrap_or(20).clamp(1, 20);
    let language = ui_language_from_headers(&headers);
    let items = state
        .proxy
        .token_recent_logs(&id, limit, None)
        .await
        .map_err(|err| {
            eprintln!("get user token logs error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let mapped = items
        .into_iter()
        .map(|record| PublicTokenLogView::from_record(record, language))
        .map(|mut v| {
            if let Some(err) = v.error_message.as_ref() {
                v.error_message = Some(redact_sensitive(err));
            }
            v.path = redact_sensitive(&v.path);
            if let Some(query) = v.query.as_ref() {
                v.query = Some(redact_sensitive(query));
            }
            v
        })
        .collect::<Vec<_>>();
    Ok(Json(mapped))
}
