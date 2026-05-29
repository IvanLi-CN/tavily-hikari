#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserTokenView {
    token: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserTokenLogView {
    id: i64,
    method: String,
    path: String,
    query: Option<String>,
    http_status: Option<i64>,
    mcp_status: Option<i64>,
    business_credits: Option<i64>,
    result_status: String,
    error_message: Option<String>,
    created_at: i64,
}

impl UserTokenLogView {
    fn from_record(record: TokenLogRecord, language: UiLanguage) -> Self {
        let business_credits = record.business_credits;
        let public_view = PublicTokenLogView::from_record(record, language);
        Self {
            id: public_view.id,
            method: public_view.method,
            path: public_view.path,
            query: public_view.query,
            http_status: public_view.http_status,
            mcp_status: public_view.mcp_status,
            business_credits,
            result_status: public_view.result_status,
            error_message: public_view.error_message,
            created_at: public_view.created_at,
        }
    }
}

#[derive(Debug, Deserialize)]
struct LinuxDoCallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LinuxDoTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LinuxDoAuthForm {
    token: Option<String>,
}

#[derive(Debug)]
enum LinuxDoSyncError {
    Transport {
        stage: &'static str,
        source: reqwest::Error,
    },
    UpstreamStatus {
        stage: &'static str,
        status: reqwest::StatusCode,
        body: String,
    },
    Parse {
        stage: &'static str,
        detail: String,
    },
    InvalidPayload(&'static str),
    ProviderUserIdMismatch {
        expected: String,
        actual: String,
    },
    Crypto(String),
    Storage(String),
}

impl LinuxDoSyncError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::Transport { source, .. } => map_oauth_upstream_transport_error(source),
            Self::UpstreamStatus { status, .. } => map_oauth_upstream_status(*status),
            Self::Parse { .. }
            | Self::InvalidPayload(_)
            | Self::ProviderUserIdMismatch { .. }
            | Self::Crypto(_)
            | Self::Storage(_) => StatusCode::BAD_GATEWAY,
        }
    }
}

impl std::fmt::Display for LinuxDoSyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport { stage, source } => write!(f, "{stage} transport error: {source}"),
            Self::UpstreamStatus {
                stage,
                status,
                body,
            } => write!(f, "{stage} upstream status {status}: {body}"),
            Self::Parse { stage, detail } => write!(f, "{stage} parse error: {detail}"),
            Self::InvalidPayload(detail) => write!(f, "invalid LinuxDo payload: {detail}"),
            Self::ProviderUserIdMismatch { expected, actual } => {
                write!(
                    f,
                    "linuxdo provider_user_id mismatch: expected {expected}, got {actual}"
                )
            }
            Self::Crypto(detail) => write!(f, "linuxdo refresh-token crypto error: {detail}"),
            Self::Storage(detail) => write!(f, "linuxdo refresh-token storage error: {detail}"),
        }
    }
}

fn trim_to_option(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
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

async fn request_linuxdo_token(
    client: &reqwest::Client,
    cfg: &LinuxDoOAuthOptions,
    form: &[(&str, &str)],
) -> Result<LinuxDoTokenResponse, LinuxDoSyncError> {
    let token_resp = client
        .post(&cfg.token_url)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(form)
        .send()
        .await
        .map_err(|source| LinuxDoSyncError::Transport {
            stage: "token",
            source,
        })?;
    if !token_resp.status().is_success() {
        let status = token_resp.status();
        let body = token_resp.text().await.unwrap_or_default();
        return Err(LinuxDoSyncError::UpstreamStatus {
            stage: "token",
            status,
            body,
        });
    }

    let token_payload: LinuxDoTokenResponse =
        token_resp
            .json()
            .await
            .map_err(|err| LinuxDoSyncError::Parse {
                stage: "token",
                detail: err.to_string(),
            })?;
    let access_token = token_payload.access_token.trim().to_string();
    if access_token.is_empty() {
        return Err(LinuxDoSyncError::InvalidPayload(
            "token response missing access_token",
        ));
    }

    Ok(LinuxDoTokenResponse {
        access_token,
        refresh_token: trim_to_option(token_payload.refresh_token.as_deref()),
    })
}

async fn exchange_linuxdo_authorization_code(
    client: &reqwest::Client,
    cfg: &LinuxDoOAuthOptions,
    code: &str,
) -> Result<LinuxDoTokenResponse, LinuxDoSyncError> {
    request_linuxdo_token(
        client,
        cfg,
        &[
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
        ],
    )
    .await
}

async fn exchange_linuxdo_refresh_token(
    client: &reqwest::Client,
    cfg: &LinuxDoOAuthOptions,
    refresh_token: &str,
) -> Result<LinuxDoTokenResponse, LinuxDoSyncError> {
    request_linuxdo_token(
        client,
        cfg,
        &[
            ("client_id", cfg.client_id.as_deref().unwrap_or_default()),
            (
                "client_secret",
                cfg.client_secret.as_deref().unwrap_or_default(),
            ),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ],
    )
    .await
}

async fn fetch_linuxdo_user_json(
    client: &reqwest::Client,
    cfg: &LinuxDoOAuthOptions,
    access_token: &str,
) -> Result<Value, LinuxDoSyncError> {
    let user_resp = client
        .get(&cfg.userinfo_url)
        .bearer_auth(access_token)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|source| LinuxDoSyncError::Transport {
            stage: "userinfo",
            source,
        })?;
    if !user_resp.status().is_success() {
        let status = user_resp.status();
        let body = user_resp.text().await.unwrap_or_default();
        return Err(LinuxDoSyncError::UpstreamStatus {
            stage: "userinfo",
            status,
            body,
        });
    }

    user_resp
        .json()
        .await
        .map_err(|err| LinuxDoSyncError::Parse {
            stage: "userinfo",
            detail: err.to_string(),
        })
}

fn linuxdo_profile_from_user_json(user_json: Value) -> Result<OAuthAccountProfile, LinuxDoSyncError> {
    let provider_user_id = user_json
        .get("id")
        .and_then(json_value_to_string)
        .filter(|value| !value.is_empty())
        .ok_or(LinuxDoSyncError::InvalidPayload(
            "userinfo response missing id",
        ))?;
    let username = trim_to_option(user_json.get("username").and_then(|value| value.as_str()));
    let name = trim_to_option(user_json.get("name").and_then(|value| value.as_str()));
    let avatar_template =
        trim_to_option(user_json.get("avatar_template").and_then(|value| value.as_str()));
    let active = user_json
        .get("active")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let trust_level = user_json.get("trust_level").and_then(|value| value.as_i64());
    let raw_payload_json = serde_json::to_string(&user_json).ok();

    Ok(OAuthAccountProfile {
        provider: "linuxdo".to_string(),
        provider_user_id,
        username,
        name,
        avatar_template,
        active,
        trust_level,
        raw_payload_json,
    })
}

async fn fetch_linuxdo_profile_with_access_token(
    client: &reqwest::Client,
    cfg: &LinuxDoOAuthOptions,
    access_token: &str,
) -> Result<OAuthAccountProfile, LinuxDoSyncError> {
    let user_json = fetch_linuxdo_user_json(client, cfg, access_token).await?;
    linuxdo_profile_from_user_json(user_json)
}

async fn fetch_linuxdo_profile_from_authorization_code(
    client: &reqwest::Client,
    cfg: &LinuxDoOAuthOptions,
    code: &str,
) -> Result<(OAuthAccountProfile, LinuxDoTokenResponse), LinuxDoSyncError> {
    let token_payload = exchange_linuxdo_authorization_code(client, cfg, code).await?;
    let profile =
        fetch_linuxdo_profile_with_access_token(client, cfg, &token_payload.access_token).await?;
    Ok((profile, token_payload))
}

async fn fetch_linuxdo_profile_from_refresh_token(
    client: &reqwest::Client,
    cfg: &LinuxDoOAuthOptions,
    refresh_token: &str,
) -> Result<(OAuthAccountProfile, LinuxDoTokenResponse), LinuxDoSyncError> {
    let token_payload = exchange_linuxdo_refresh_token(client, cfg, refresh_token).await?;
    let profile =
        fetch_linuxdo_profile_with_access_token(client, cfg, &token_payload.access_token).await?;
    Ok((profile, token_payload))
}

fn encrypt_linuxdo_refresh_token(
    cfg: &LinuxDoOAuthOptions,
    refresh_token: &str,
) -> Result<Option<(String, String)>, LinuxDoSyncError> {
    use base64::Engine as _;
    use rand::RngCore as _;
    use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};

    let Some(key_bytes) = cfg.refresh_token_crypt_key() else {
        return Ok(None);
    };
    let refresh_token = refresh_token.trim();
    if refresh_token.is_empty() {
        return Ok(None);
    }

    let unbound_key = UnboundKey::new(&CHACHA20_POLY1305, key_bytes).map_err(|_| {
        LinuxDoSyncError::Crypto("invalid refresh-token crypt key length".to_string())
    })?;
    let key = LessSafeKey::new(unbound_key);
    let mut nonce_bytes = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);

    let mut in_out = refresh_token.as_bytes().to_vec();
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce_bytes),
        Aad::empty(),
        &mut in_out,
    )
    .map_err(|_| LinuxDoSyncError::Crypto("failed to encrypt refresh token".to_string()))?;

    Ok(Some((
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(in_out),
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(nonce_bytes),
    )))
}

fn decrypt_linuxdo_refresh_token(
    cfg: &LinuxDoOAuthOptions,
    refresh_token_ciphertext: &str,
    refresh_token_nonce: &str,
) -> Result<String, LinuxDoSyncError> {
    use base64::Engine as _;
    use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};

    let Some(key_bytes) = cfg.refresh_token_crypt_key() else {
        return Err(LinuxDoSyncError::Crypto(
            "missing refresh-token crypt key".to_string(),
        ));
    };
    let ciphertext = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(refresh_token_ciphertext)
        .map_err(|err| LinuxDoSyncError::Crypto(format!("invalid ciphertext: {err}")))?;
    let nonce = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(refresh_token_nonce)
        .map_err(|err| LinuxDoSyncError::Crypto(format!("invalid nonce: {err}")))?;
    if nonce.len() != 12 {
        return Err(LinuxDoSyncError::Crypto(
            "refresh-token nonce must be 12 bytes".to_string(),
        ));
    }

    let unbound_key = UnboundKey::new(&CHACHA20_POLY1305, key_bytes).map_err(|_| {
        LinuxDoSyncError::Crypto("invalid refresh-token crypt key length".to_string())
    })?;
    let key = LessSafeKey::new(unbound_key);

    let mut nonce_bytes = [0u8; 12];
    nonce_bytes.copy_from_slice(&nonce);
    let mut in_out = ciphertext;
    let plaintext = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce_bytes),
            Aad::empty(),
            &mut in_out,
        )
        .map_err(|_| LinuxDoSyncError::Crypto("failed to decrypt refresh token".to_string()))?;

    String::from_utf8(plaintext.to_vec())
        .map_err(|err| LinuxDoSyncError::Crypto(format!("refresh token is not valid UTF-8: {err}")))
}

async fn persist_linuxdo_refresh_token_best_effort(
    state: &AppState,
    provider_user_id: &str,
    refresh_token: Option<&str>,
) -> Result<bool, LinuxDoSyncError> {
    let Some(refresh_token) = trim_to_option(refresh_token).filter(|value| !value.is_empty()) else {
        return Ok(false);
    };
    let Some((ciphertext, nonce)) =
        encrypt_linuxdo_refresh_token(&state.linuxdo_oauth, &refresh_token)?
    else {
        return Ok(false);
    };

    state
        .proxy
        .set_oauth_account_refresh_token("linuxdo", provider_user_id, &ciphertext, &nonce)
        .await
        .map_err(|err| LinuxDoSyncError::Storage(err.to_string()))?;
    Ok(true)
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
    let (profile, token_payload) = fetch_linuxdo_profile_from_authorization_code(&client, cfg, code)
        .await
        .map_err(|err| {
            eprintln!("linuxdo callback oauth flow error: {err}");
            err.status_code()
        })?;
    let provider_user_id = profile.provider_user_id.clone();
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
    let username = profile.username.clone();

    let user = state
        .proxy
        .upsert_oauth_account(&profile)
        .await
        .map_err(|err| {
            eprintln!("upsert linuxdo oauth account error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let sync_attempted_at = Utc::now().timestamp();
    if let Err(err) = persist_linuxdo_refresh_token_best_effort(
        state.as_ref(),
        &provider_user_id,
        token_payload.refresh_token.as_deref(),
    )
    .await
    {
        eprintln!("persist linuxdo refresh token error: {err}");
        if let Err(mark_err) = state
            .proxy
            .record_oauth_account_profile_sync_failure(
                "linuxdo",
                &provider_user_id,
                sync_attempted_at,
                &err.to_string(),
            )
            .await
        {
            eprintln!("record linuxdo callback sync failure error: {mark_err}");
        }
    } else if let Err(err) = state
        .proxy
        .record_oauth_account_profile_sync_success("linuxdo", &provider_user_id, sync_attempted_at)
        .await
    {
        eprintln!("record linuxdo callback sync success error: {err}");
    }
    if !profile.active {
        let clear_binding_cookie = oauth_login_binding_clear_cookie(use_secure_cookie)?;
        let mut response =
            (StatusCode::FORBIDDEN, "linuxdo account is inactive").into_response();
        response
            .headers_mut()
            .append(SET_COOKIE, clear_binding_cookie);
        return Ok(response);
    }

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
    last_activity: Option<i64>,
    recharge: RechargeSummaryView,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RechargeSummaryView {
    current_month_start: i64,
    current_entitlement_credits: i64,
    effective_until_month_start: Option<i64>,
}

impl From<tavily_hikari::LinuxDoCreditRechargeSummary> for RechargeSummaryView {
    fn from(value: tavily_hikari::LinuxDoCreditRechargeSummary) -> Self {
        Self {
            current_month_start: value.current_month_start,
            current_entitlement_credits: value.current_month_entitlement_credits,
            effective_until_month_start: value.effective_until_month_start,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RechargeOrderView {
    out_trade_no: String,
    status: String,
    credits: i64,
    months: i64,
    money: String,
    trade_no: Option<String>,
    payment_url: Option<String>,
    created_at: i64,
    updated_at: i64,
    paid_at: Option<i64>,
    last_notify_at: Option<i64>,
    last_error: Option<String>,
}

impl From<tavily_hikari::LinuxDoCreditRechargeOrder> for RechargeOrderView {
    fn from(value: tavily_hikari::LinuxDoCreditRechargeOrder) -> Self {
        Self {
            out_trade_no: value.out_trade_no,
            status: value.status,
            credits: value.credits,
            months: value.months,
            money: tavily_hikari::format_linuxdo_credit_money(value.money_cents),
            trade_no: value.trade_no,
            payment_url: value.payment_url,
            created_at: value.created_at,
            updated_at: value.updated_at,
            paid_at: value.paid_at,
            last_notify_at: value.last_notify_at,
            last_error: value.last_error,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RechargeConfigView {
    visible: bool,
    enabled: bool,
    unit_credits: i64,
    unit_price_ldc: i64,
    min_credits: i64,
    max_credits: i64,
    credits_step: i64,
    default_credits: i64,
    min_months: i64,
    max_months: i64,
    quota_delta_base_credits: i64,
    hourly_delta_per_quota_unit: i64,
    daily_delta_per_quota_unit: i64,
    monthly_delta_per_quota_unit: i64,
    test_price_enabled: bool,
    current_month_start: i64,
    current_entitlement_credits: i64,
    effective_until_month_start: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RechargeOrdersView {
    items: Vec<RechargeOrderView>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateRechargeOrderRequest {
    credits: i64,
    months: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateRechargeOrderResponse {
    order: RechargeOrderView,
    payment_url: String,
}

#[derive(Debug, Deserialize)]
struct LinuxDoCreditNotifyQuery {
    pid: Option<String>,
    trade_no: Option<String>,
    out_trade_no: Option<String>,
    #[serde(rename = "type")]
    payment_type: Option<String>,
    name: Option<String>,
    money: Option<String>,
    trade_status: Option<String>,
    sign: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserTokenSummaryView {
    token_id: String,
    enabled: bool,
    note: Option<String>,
    last_used_at: Option<i64>,
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
}

#[derive(Debug, Deserialize)]
struct UserTokenLogsQuery {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct UserTodayWindowQuery {
    today_start: Option<String>,
    today_end: Option<String>,
}

#[derive(Debug, Serialize)]
struct UserTokenSnapshot {
    token: UserTokenSummaryView,
    logs: Vec<UserTokenLogView>,
}

fn parse_user_today_window_query(
    query: &UserTodayWindowQuery,
) -> Result<Option<tavily_hikari::TimeRangeUtc>, (StatusCode, String)> {
    tavily_hikari::parse_explicit_today_window(query.today_start.as_deref(), query.today_end.as_deref())
        .map_err(|message| (StatusCode::BAD_REQUEST, message))
}

async fn get_user_dashboard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserTodayWindowQuery>,
) -> Result<Json<UserDashboardView>, (StatusCode, String)> {
    if !state.linuxdo_oauth.is_enabled_and_configured() {
        return Err((StatusCode::NOT_FOUND, "not found".to_string()));
    }
    let Some(user_session) = resolve_user_session(state.as_ref(), &headers).await else {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
    };
    let daily_window = parse_user_today_window_query(&query)?;
    let summary = state
        .proxy
        .user_dashboard_summary(&user_session.user.user_id, daily_window)
        .await
        .map_err(|err| {
            eprintln!("get user dashboard error: {err}");
            (StatusCode::INTERNAL_SERVER_ERROR, "failed to load dashboard".to_string())
        })?;
    Ok(Json(UserDashboardView {
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
        last_activity: summary.last_activity,
        recharge: summary.recharge.into(),
    }))
}

fn linuxdo_credit_config_unavailable() -> (StatusCode, String) {
    (StatusCode::SERVICE_UNAVAILABLE, "recharge not configured".to_string())
}

async fn user_recharge_gate_for_request(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(bool, bool), (StatusCode, String)> {
    let settings = state
        .proxy
        .get_system_settings()
        .await
        .map_err(|err| map_recharge_error("load recharge gate settings", err))?;
    let visible = settings.recharge_feature_enabled
        && (settings.recharge_user_enabled || is_admin_request(state, headers));
    Ok((visible, visible && state.linuxdo_credit.is_enabled_and_configured()))
}

fn map_recharge_error(stage: &'static str, err: impl std::fmt::Display) -> (StatusCode, String) {
    eprintln!("{stage}: {err}");
    (StatusCode::INTERNAL_SERVER_ERROR, "recharge failed".to_string())
}

fn linuxdo_credit_signature_payload(params: &[(&str, String)], secret: &str) -> String {
    let mut pairs: Vec<(&str, &str)> = params
        .iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(key, value)| (*key, value.as_str()))
        .collect();
    pairs.sort_by(|left, right| left.0.cmp(right.0));
    let joined = pairs
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    format!("{joined}{secret}")
}

fn decode_ed25519_private_key(raw: &str) -> Result<Vec<u8>, String> {
    let trimmed = raw.trim();
    let pem_body = if trimmed.contains("-----BEGIN") {
        trimmed
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .map(str::trim)
            .collect::<String>()
    } else {
        trimmed.to_string()
    };
    for engine in [
        base64::engine::general_purpose::STANDARD,
        base64::engine::general_purpose::STANDARD_NO_PAD,
        base64::engine::general_purpose::URL_SAFE,
        base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ] {
        if let Ok(decoded) = engine.decode(&pem_body)
            && (decoded.len() == 32 || decoded.len() > 32)
        {
            if let Some(seed) = ed25519_seed_from_pkcs8_v1_der(&decoded) {
                return Ok(seed);
            }
            return Ok(decoded);
        }
    }
    if trimmed.len() == 64 && trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
        let mut bytes = Vec::with_capacity(32);
        for index in (0..trimmed.len()).step_by(2) {
            let byte = u8::from_str_radix(&trimmed[index..index + 2], 16)
                .map_err(|err| format!("invalid hex private key: {err}"))?;
            bytes.push(byte);
        }
        return Ok(bytes);
    }
    Err("private key must be base64/base64url/hex seed or PKCS#8 DER/PEM".to_string())
}

fn ed25519_seed_from_pkcs8_v1_der(decoded: &[u8]) -> Option<Vec<u8>> {
    const PREFIX: [u8; 16] = [
        0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22,
        0x04, 0x20,
    ];
    decoded
        .strip_prefix(&PREFIX)
        .filter(|seed| seed.len() == 32)
        .map(|seed| seed.to_vec())
}

fn sign_linuxdo_credit_ldc(params: &[(&str, String)], cfg: &LinuxDoCreditOptions) -> Result<String, String> {
    let secret = cfg
        .client_secret
        .as_deref()
        .ok_or_else(|| "missing client secret".to_string())?;
    let private_key = cfg
        .merchant_private_key
        .as_deref()
        .ok_or_else(|| "missing merchant private key".to_string())?;
    let payload = linuxdo_credit_signature_payload(params, secret);
    let key_bytes = decode_ed25519_private_key(private_key)?;
    let signature = if key_bytes.len() == 32 {
        let key_pair = ring::signature::Ed25519KeyPair::from_seed_unchecked(&key_bytes)
            .map_err(|_| "invalid Ed25519 seed".to_string())?;
        key_pair.sign(payload.as_bytes())
    } else {
        let key_pair = ring::signature::Ed25519KeyPair::from_pkcs8(&key_bytes)
            .map_err(|_| "invalid Ed25519 PKCS#8 private key".to_string())?;
        key_pair.sign(payload.as_bytes())
    };
    Ok(base64::engine::general_purpose::STANDARD.encode(signature.as_ref()))
}

fn verify_linuxdo_credit_notify_sign(query: &LinuxDoCreditNotifyQuery, secret: &str) -> bool {
    let Some(sign) = query.sign.as_deref().map(str::trim).filter(|it| !it.is_empty()) else {
        return false;
    };
    let params = [
        ("money", query.money.clone().unwrap_or_default()),
        ("name", query.name.clone().unwrap_or_default()),
        ("out_trade_no", query.out_trade_no.clone().unwrap_or_default()),
        ("pid", query.pid.clone().unwrap_or_default()),
        ("trade_no", query.trade_no.clone().unwrap_or_default()),
        ("trade_status", query.trade_status.clone().unwrap_or_default()),
        ("type", query.payment_type.clone().unwrap_or_default()),
    ];
    let payload = linuxdo_credit_signature_payload(&params, secret);
    let digest = format!("{:x}", md5::compute(payload.as_bytes()));
    digest.eq_ignore_ascii_case(sign)
}

fn linuxdo_credit_location_payment_url(
    submit_url: &str,
    headers: &reqwest::header::HeaderMap,
) -> Option<String> {
    let raw = headers
        .get(reqwest::header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return Some(raw.to_string());
    }
    reqwest::Url::parse(submit_url)
        .ok()
        .and_then(|base| base.join(raw).ok())
        .map(|url| url.to_string())
}

fn linuxdo_credit_payment_url_from_response(
    submit_url: &str,
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &str,
) -> Result<String, &'static str> {
    if status.is_redirection()
        && let Some(payment_url) = linuxdo_credit_location_payment_url(submit_url, headers)
    {
        return Ok(payment_url);
    }
    let value: Value = serde_json::from_str(body).map_err(|_| "invalid payment response")?;
    value
        .get("url")
        .or_else(|| value.get("payment_url"))
        .or_else(|| value.get("pay_url"))
        .and_then(|it| it.as_str())
        .map(str::to_string)
        .or_else(|| {
            value
                .get("data")
                .and_then(|data| {
                    data.get("url")
                        .or_else(|| data.get("payment_url"))
                        .or_else(|| data.get("pay_url"))
                })
                .and_then(|it| it.as_str())
                .map(str::to_string)
        })
        .filter(|url| !url.trim().is_empty())
        .ok_or("payment url missing")
}

async fn get_user_recharge_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<RechargeConfigView>, (StatusCode, String)> {
    if !state.linuxdo_oauth.is_enabled_and_configured() {
        return Err((StatusCode::NOT_FOUND, "not found".to_string()));
    }
    let Some(user_session) = resolve_user_session(state.as_ref(), &headers).await else {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
    };
    let summary = state
        .proxy
        .linuxdo_credit_recharge_summary(&user_session.user.user_id)
        .await
        .map_err(|err| map_recharge_error("load recharge summary", err))?;
    let price = state.linuxdo_credit.price_config();
    let quota_delta_base_credits = tavily_hikari::LINUXDO_CREDIT_RECHARGE_UNIT_CREDITS;
    let quota_delta = tavily_hikari::linuxdo_credit_recharge_quota_delta(quota_delta_base_credits);
    let (visible, enabled) = user_recharge_gate_for_request(state.as_ref(), &headers).await?;
    Ok(Json(RechargeConfigView {
        visible,
        enabled,
        unit_credits: price.unit_credits,
        unit_price_ldc: price.unit_price_cents / 100,
        min_credits: price.min_credits,
        max_credits: price.max_credits,
        credits_step: price.credits_step,
        default_credits: price.default_credits,
        min_months: price.min_months,
        max_months: price.max_months,
        quota_delta_base_credits,
        hourly_delta_per_quota_unit: quota_delta.hourly_delta,
        daily_delta_per_quota_unit: quota_delta.daily_delta,
        monthly_delta_per_quota_unit: quota_delta.monthly_delta,
        test_price_enabled: state.linuxdo_credit.test_price_enabled,
        current_month_start: summary.current_month_start,
        current_entitlement_credits: summary.current_month_entitlement_credits,
        effective_until_month_start: summary.effective_until_month_start,
    }))
}

async fn get_user_recharge_orders(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<RechargeOrdersView>, (StatusCode, String)> {
    if !state.linuxdo_oauth.is_enabled_and_configured() {
        return Err((StatusCode::NOT_FOUND, "not found".to_string()));
    }
    let Some(user_session) = resolve_user_session(state.as_ref(), &headers).await else {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
    };
    let items = state
        .proxy
        .list_linuxdo_credit_recharge_orders(&user_session.user.user_id, 20)
        .await
        .map_err(|err| map_recharge_error("list recharge orders", err))?
        .into_iter()
        .map(RechargeOrderView::from)
        .collect();
    Ok(Json(RechargeOrdersView { items }))
}

async fn get_user_recharge_order(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(out_trade_no): Path<String>,
) -> Result<Json<RechargeOrderView>, (StatusCode, String)> {
    if !state.linuxdo_oauth.is_enabled_and_configured() {
        return Err((StatusCode::NOT_FOUND, "not found".to_string()));
    }
    let Some(user_session) = resolve_user_session(state.as_ref(), &headers).await else {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
    };
    let Some(order) = state
        .proxy
        .get_linuxdo_credit_recharge_order(&out_trade_no)
        .await
        .map_err(|err| map_recharge_error("get recharge order", err))?
    else {
        return Err((StatusCode::NOT_FOUND, "not found".to_string()));
    };
    if order.user_id != user_session.user.user_id {
        return Err((StatusCode::NOT_FOUND, "not found".to_string()));
    }
    Ok(Json(RechargeOrderView::from(order)))
}

async fn post_user_recharge_order(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateRechargeOrderRequest>,
) -> Result<Json<CreateRechargeOrderResponse>, (StatusCode, String)> {
    if !state.linuxdo_oauth.is_enabled_and_configured() {
        return Err((StatusCode::NOT_FOUND, "not found".to_string()));
    }
    let Some(user_session) = resolve_user_session(state.as_ref(), &headers).await else {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
    };
    let (_, recharge_enabled) = user_recharge_gate_for_request(state.as_ref(), &headers).await?;
    if !recharge_enabled {
        return Err(linuxdo_credit_config_unavailable());
    }
    let price = state.linuxdo_credit.price_config();
    let Some(money_cents) =
        tavily_hikari::linuxdo_credit_recharge_money_cents(payload.credits, payload.months, price)
    else {
        return Err((StatusCode::BAD_REQUEST, "invalid credits or months".to_string()));
    };
    let now = Utc::now().timestamp();
    let out_trade_no = format!("ldc_{}", nanoid!(24));
    let order_name = format!("Tavily Hikari {} credits x {} month(s)", payload.credits, payload.months);
    let mut order = tavily_hikari::LinuxDoCreditRechargeOrder {
        out_trade_no: out_trade_no.clone(),
        user_id: user_session.user.user_id.clone(),
        status: tavily_hikari::LINUXDO_CREDIT_RECHARGE_STATUS_PENDING.to_string(),
        credits: payload.credits,
        months: payload.months,
        money_cents,
        trade_no: None,
        payment_url: None,
        order_name: order_name.clone(),
        notify_payload: None,
        created_at: now,
        updated_at: now,
        paid_at: None,
        refunded_at: None,
        refund_actor: None,
        refund_payload: None,
        last_notify_at: None,
        last_error: None,
    };

    let client_id = state
        .linuxdo_credit
        .client_id
        .as_deref()
        .ok_or_else(linuxdo_credit_config_unavailable)?;
    let money = tavily_hikari::format_linuxdo_credit_money(money_cents);
    let mut submit_params = vec![
        ("client_id", client_id.to_string()),
        ("type", "ldcpay".to_string()),
        ("out_trade_no", out_trade_no.clone()),
        ("money", money),
        ("order_name", order_name),
    ];
    if let Some(notify_url) = state.linuxdo_credit.notify_url.clone() {
        submit_params.push(("notify_url", notify_url));
    }
    if let Some(return_url) = state.linuxdo_credit.return_url.clone() {
        submit_params.push(("return_url", return_url));
    }
    let sign = sign_linuxdo_credit_ldc(&submit_params, &state.linuxdo_credit)
        .map_err(|err| (StatusCode::SERVICE_UNAVAILABLE, err))?;
    submit_params.push(("sign", sign));

    let submit_url = state.linuxdo_credit.submit_url.clone();
    state
        .proxy
        .create_linuxdo_credit_recharge_order(&order)
        .await
        .map_err(|err| map_recharge_error("persist recharge order", err))?;

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|err| {
            eprintln!("linuxdo credit client build error: {err}");
            (StatusCode::BAD_GATEWAY, "failed to create payment order".to_string())
        })?;
    let resp = client
        .post(&submit_url)
        .form(&submit_params)
        .send()
        .await
        .map_err(|err| {
            eprintln!("linuxdo credit create order transport error: {err}");
            let proxy = state.proxy.clone();
            let out_trade_no = out_trade_no.clone();
            tokio::spawn(async move {
                let _ = proxy
                    .fail_linuxdo_credit_recharge_order(
                        &out_trade_no,
                        "payment upstream transport error",
                        Utc::now().timestamp(),
                    )
                    .await;
            });
            (StatusCode::BAD_GATEWAY, "failed to create payment order".to_string())
        })?;
    let status = resp.status();
    let headers = resp.headers().clone();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() && !status.is_redirection() {
        state
            .proxy
            .fail_linuxdo_credit_recharge_order(
                &out_trade_no,
                &format!("payment upstream returned {status}"),
                Utc::now().timestamp(),
            )
            .await
            .ok();
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("payment upstream returned {status}"),
        ));
    }
    let payment_url =
        linuxdo_credit_payment_url_from_response(&submit_url, status, &headers, &body).map_err(
            |message| {
                let proxy = state.proxy.clone();
                let out_trade_no = out_trade_no.clone();
                let message = message.to_string();
                let stored_message = message.clone();
                tokio::spawn(async move {
                    let _ = proxy
                        .fail_linuxdo_credit_recharge_order(
                            &out_trade_no,
                            &stored_message,
                            Utc::now().timestamp(),
                        )
                        .await;
                });
                (StatusCode::BAD_GATEWAY, message)
            },
        )?;
    state
        .proxy
        .set_linuxdo_credit_recharge_payment_url(&out_trade_no, &payment_url, Utc::now().timestamp())
        .await
        .map_err(|err| map_recharge_error("persist recharge payment url", err))?;
    order.payment_url = Some(payment_url.clone());
    order.updated_at = Utc::now().timestamp();
    Ok(Json(CreateRechargeOrderResponse {
        order: RechargeOrderView::from(order),
        payment_url,
    }))
}

async fn get_linuxdo_credit_notify(
    State(state): State<Arc<AppState>>,
    RawQuery(raw_query): RawQuery,
    Query(query): Query<LinuxDoCreditNotifyQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if !state.linuxdo_credit.is_enabled_and_configured() {
        return Err(linuxdo_credit_config_unavailable());
    }
    let Some(out_trade_no) = query
        .out_trade_no
        .as_deref()
        .map(str::trim)
        .filter(|it| !it.is_empty())
    else {
        return Err((StatusCode::BAD_REQUEST, "missing out_trade_no".to_string()));
    };
    let Some(order) = state
        .proxy
        .get_linuxdo_credit_recharge_order(out_trade_no)
        .await
        .map_err(|err| map_recharge_error("load recharge notify order", err))?
    else {
        return Err((StatusCode::BAD_REQUEST, "order not found".to_string()));
    };
    let client_secret = state
        .linuxdo_credit
        .client_secret
        .as_deref()
        .ok_or_else(linuxdo_credit_config_unavailable)?;
    if !verify_linuxdo_credit_notify_sign(&query, client_secret) {
        return Err((StatusCode::BAD_REQUEST, "invalid sign".to_string()));
    }
    let paid = query.trade_status.as_deref() == Some("TRADE_SUCCESS")
        || query.trade_status.as_deref() == Some("success");
    if !paid {
        return Err((StatusCode::BAD_REQUEST, "trade not successful".to_string()));
    }
    let expected_money = tavily_hikari::format_linuxdo_credit_money(order.money_cents);
    if query.money.as_deref() != Some(expected_money.as_str()) {
        return Err((StatusCode::BAD_REQUEST, "money mismatch".to_string()));
    }
    let trade_no = query.trade_no.as_deref().unwrap_or_default();
    let notify_payload = raw_query.unwrap_or_default();
    state
        .proxy
        .apply_linuxdo_credit_recharge_payment(
            out_trade_no,
            trade_no,
            &notify_payload,
            Utc::now().timestamp(),
        )
        .await
        .map_err(|err| map_recharge_error("apply recharge notify", err))?;
    Ok((StatusCode::OK, "success"))
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

async fn build_user_token_detail_view(
    state: &Arc<AppState>,
    user_id: &str,
    token_id: &str,
    daily_window: Option<tavily_hikari::TimeRangeUtc>,
) -> Result<UserTokenSummaryView, (StatusCode, String)> {
    let tokens = state
        .proxy
        .list_user_tokens(user_id)
        .await
        .map_err(|err| {
            eprintln!("get user token detail list error: {err}");
            (StatusCode::INTERNAL_SERVER_ERROR, "failed to load token".to_string())
        })?;
    let Some(token) = tokens.into_iter().find(|token| token.id == token_id) else {
        return Err((StatusCode::NOT_FOUND, "not found".to_string()));
    };
    let (monthly_success, daily_success, daily_failure) = state
        .proxy
        .token_success_breakdown(&token.id, daily_window)
        .await
        .map_err(|err| {
            eprintln!("get user token detail breakdown error: {err}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load token metrics".to_string(),
            )
        })?;
    let hourly_any = state
        .proxy
        .token_hourly_any_snapshot(std::slice::from_ref(&token.id))
        .await
        .map_err(|err| {
            eprintln!("get user token detail hourly snapshot error: {err}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load token hourly limits".to_string(),
            )
        })?;
    let request_rate = hourly_any
        .get(&token.id)
        .cloned()
        .unwrap_or_else(|| {
            state
                .proxy
                .default_request_rate_verdict(tavily_hikari::RequestRateScope::Token)
        });
    let (hourly_any_used, hourly_any_limit) = (request_rate.hourly_used, request_rate.hourly_limit);
    let (
        quota_hourly_used,
        quota_hourly_limit,
        quota_daily_used,
        quota_daily_limit,
        quota_monthly_used,
        quota_monthly_limit,
    ) = user_token_quota_values(&token);

    Ok(UserTokenSummaryView {
        token_id: token.id,
        enabled: token.enabled,
        note: token.note,
        last_used_at: token.last_used_at,
        request_rate: request_rate.request_rate(),
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
    })
}

async fn build_user_token_logs_view(
    state: &Arc<AppState>,
    token_id: &str,
    limit: usize,
    language: UiLanguage,
) -> Result<Vec<UserTokenLogView>, StatusCode> {
    let items = state
        .proxy
        .token_recent_logs(token_id, limit.clamp(1, 20), None)
        .await
        .map_err(|err| {
            eprintln!("get user token logs error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(items
        .into_iter()
        .map(|record| UserTokenLogView::from_record(record, language))
        .map(|mut view| {
            if let Some(err) = view.error_message.as_ref() {
                view.error_message = Some(redact_sensitive(err));
            }
            view.path = redact_sensitive(&view.path);
            if let Some(query) = view.query.as_ref() {
                view.query = Some(redact_sensitive(query));
            }
            view
        })
        .collect())
}

async fn build_user_token_snapshot_event(
    state: &Arc<AppState>,
    user_id: &str,
    token_id: &str,
    daily_window: Option<tavily_hikari::TimeRangeUtc>,
    language: UiLanguage,
) -> Option<(Event, Option<i64>)> {
    let token = build_user_token_detail_view(state, user_id, token_id, daily_window)
        .await
        .ok()?;
    let logs = build_user_token_logs_view(state, token_id, 20, language)
        .await
        .ok()?;
    let latest_log_id = logs.first().map(|log| log.id);
    let payload = UserTokenSnapshot { token, logs };
    let json = serde_json::to_string(&payload).ok()?;
    Some((Event::default().event("snapshot").data(json), latest_log_id))
}

async fn get_user_tokens(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserTodayWindowQuery>,
) -> Result<Json<Vec<UserTokenSummaryView>>, (StatusCode, String)> {
    if !state.linuxdo_oauth.is_enabled_and_configured() {
        return Err((StatusCode::NOT_FOUND, "not found".to_string()));
    }
    let Some(user_session) = resolve_user_session(state.as_ref(), &headers).await else {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
    };
    let daily_window = parse_user_today_window_query(&query)?;

    let tokens = state
        .proxy
        .list_user_tokens(&user_session.user.user_id)
        .await
        .map_err(|err| {
            eprintln!("list user tokens error: {err}");
            (StatusCode::INTERNAL_SERVER_ERROR, "failed to load tokens".to_string())
        })?;
    let token_ids: Vec<String> = tokens.iter().map(|t| t.id.clone()).collect();
    let hourly_any = state
        .proxy
        .token_hourly_any_snapshot(&token_ids)
        .await
        .map_err(|err| {
            eprintln!("list user tokens hourly snapshot error: {err}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load token hourly limits".to_string(),
            )
        })?;
    let mut items = Vec::with_capacity(tokens.len());
    for token in tokens {
        let (monthly_success, daily_success, daily_failure) = state
            .proxy
            .token_success_breakdown(&token.id, daily_window)
            .await
            .map_err(|err| {
                eprintln!("list user tokens success breakdown error: {err}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to load token metrics".to_string(),
                )
            })?;
        let (
            quota_hourly_used,
            quota_hourly_limit,
            quota_daily_used,
            quota_daily_limit,
            quota_monthly_used,
            quota_monthly_limit,
        ) = user_token_quota_values(&token);
        let request_rate = hourly_any
            .get(&token.id)
            .cloned()
            .unwrap_or_else(|| {
                state
                    .proxy
                    .default_request_rate_verdict(tavily_hikari::RequestRateScope::Token)
            });
        let (hourly_any_used, hourly_any_limit) =
            (request_rate.hourly_used, request_rate.hourly_limit);
        items.push(UserTokenSummaryView {
            token_id: token.id,
            enabled: token.enabled,
            note: token.note,
            last_used_at: token.last_used_at,
            request_rate: request_rate.request_rate(),
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
    Query(query): Query<UserTodayWindowQuery>,
) -> Result<Json<UserTokenSummaryView>, (StatusCode, String)> {
    if !state.linuxdo_oauth.is_enabled_and_configured() {
        return Err((StatusCode::NOT_FOUND, "not found".to_string()));
    }
    let Some(user_session) = resolve_user_session(state.as_ref(), &headers).await else {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
    };
    let daily_window = parse_user_today_window_query(&query)?;
    let owned = state
        .proxy
        .is_user_token_bound(&user_session.user.user_id, &id)
        .await
        .map_err(|err| {
            eprintln!("get user token detail ownership error: {err}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to verify token ownership".to_string(),
            )
        })?;
    if !owned {
        return Err((StatusCode::NOT_FOUND, "not found".to_string()));
    }
    let detail =
        build_user_token_detail_view(&state, &user_session.user.user_id, &id, daily_window).await?;
    Ok(Json(detail))
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

async fn rotate_user_token_secret(
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
    let visible_token = state
        .proxy
        .get_user_token_secret(&user_session.user.user_id, &id)
        .await
        .map_err(|err| {
            eprintln!("rotate user token secret visibility error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if visible_token.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }
    match state.proxy.rotate_access_token_secret(&id).await {
        Ok(secret) => Ok(Json(UserTokenView {
            token: secret.token,
        })),
        Err(ProxyError::Database(sqlx::Error::RowNotFound)) => Err(StatusCode::NOT_FOUND),
        Err(err) => {
            eprintln!("rotate user token secret error: {err}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn get_user_token_logs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<UserTokenLogsQuery>,
) -> Result<Json<Vec<UserTokenLogView>>, StatusCode> {
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
    let language = ui_language_from_headers(&headers);
    let limit = q.limit.unwrap_or(20);
    let logs = build_user_token_logs_view(&state, &id, limit, language).await?;
    Ok(Json(logs))
}

async fn sse_user_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<UserTodayWindowQuery>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, axum::http::Error>>>, StatusCode> {
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
            eprintln!("get user token events ownership error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if !owned {
        return Err(StatusCode::NOT_FOUND);
    }

    let daily_window = parse_user_today_window_query(&query).map_err(|(status, _)| status)?;
    let user_id = user_session.user.user_id.clone();
    let language = ui_language_from_headers(&headers);
    let state = state.clone();
    let stream = stream! {
        let mut last_log_id: Option<i64> = None;
        if let Some((event, latest_log_id)) =
            build_user_token_snapshot_event(&state, &user_id, &id, daily_window, language).await
        {
            last_log_id = latest_log_id;
            yield Ok(event);
        }
        loop {
            match build_user_token_snapshot_event(&state, &user_id, &id, daily_window, language).await {
                Some((event, latest_log_id)) if latest_log_id != last_log_id => {
                    last_log_id = latest_log_id;
                    yield Ok(event);
                }
                Some(_) => {
                    yield Ok(Event::default().event("ping").data("{}"));
                }
                None => {
                    yield Ok(Event::default().event("ping").data("{}"));
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("")))
}

#[cfg(test)]
mod linuxdo_credit_key_tests {
    use super::*;

    #[test]
    fn linuxdo_credit_accepts_minimal_ed25519_pkcs8_v1_der() {
        const SEED: [u8; 32] = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31, 32,
        ];
        let mut der = vec![
            0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04,
            0x22, 0x04, 0x20,
        ];
        der.extend_from_slice(&SEED);
        let encoded = base64::engine::general_purpose::STANDARD.encode(&der);

        let decoded = decode_ed25519_private_key(&encoded).expect("decode pkcs8 v1 der");
        assert_eq!(decoded, SEED);
    }

    #[test]
    fn linuxdo_credit_signs_with_minimal_ed25519_pkcs8_v1_der() {
        const SEED: [u8; 32] = [
            32, 31, 30, 29, 28, 27, 26, 25, 24, 23, 22, 21, 20, 19, 18, 17, 16, 15, 14, 13,
            12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1,
        ];
        let mut der = vec![
            0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04,
            0x22, 0x04, 0x20,
        ];
        der.extend_from_slice(&SEED);
        let private_key = base64::engine::general_purpose::STANDARD.encode(&der);
        let cfg = LinuxDoCreditOptions {
            enabled: true,
            client_id: Some("pid".to_string()),
            client_secret: Some("secret".to_string()),
            merchant_private_key: Some(private_key),
            submit_url: "https://credit.linux.do/epay/pay/submit.php".to_string(),
            notify_url: None,
            return_url: None,
            test_price_enabled: false,
        };

        let signature = sign_linuxdo_credit_ldc(&[("money", "50.00".to_string())], &cfg)
            .expect("sign with pkcs8 v1 der");
        assert!(!signature.is_empty());
    }

    #[test]
    fn linuxdo_credit_submit_redirect_location_is_payment_url() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::LOCATION,
            reqwest::header::HeaderValue::from_static("/epay/pay/checkout.php?trade_no=123"),
        );

        let payment_url = linuxdo_credit_payment_url_from_response(
            "https://credit.linux.do/epay/pay/submit.php",
            reqwest::StatusCode::FOUND,
            &headers,
            "",
        )
        .expect("redirect location should be accepted");

        assert_eq!(
            payment_url,
            "https://credit.linux.do/epay/pay/checkout.php?trade_no=123"
        );
    }

    #[test]
    fn linuxdo_credit_submit_json_payment_url_is_still_supported() {
        let headers = reqwest::header::HeaderMap::new();

        let payment_url = linuxdo_credit_payment_url_from_response(
            "https://credit.linux.do/epay/pay/submit.php",
            reqwest::StatusCode::OK,
            &headers,
            r#"{"data":{"pay_url":"https://credit.linux.do/pay/123"}}"#,
        )
        .expect("json payment url should be accepted");

        assert_eq!(payment_url, "https://credit.linux.do/pay/123");
    }
}
