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
    counts_business_quota: bool,
    result_status: String,
    error_message: Option<String>,
    created_at: i64,
}

impl UserTokenLogView {
    fn from_record(record: TokenLogRecord, language: UiLanguage) -> Self {
        let business_credits = record.business_credits;
        let counts_business_quota = record.counts_business_quota;
        let public_view = PublicTokenLogView::from_record(record, language);
        Self {
            id: public_view.id,
            method: public_view.method,
            path: public_view.path,
            query: public_view.query,
            http_status: public_view.http_status,
            mcp_status: public_view.mcp_status,
            business_credits,
            counts_business_quota,
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
    error: Option<String>,
    error_description: Option<String>,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinuxDoFinalizeRequest {
    code: String,
    state: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum LinuxDoFinalizeOutcome {
    Success,
    InvalidState,
    RegistrationPaused,
    InactiveUser,
    UpstreamFailure,
    ServerError,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LinuxDoFinalizeResponse {
    outcome: LinuxDoFinalizeOutcome,
    provider: &'static str,
    redirect_to: Option<&'static str>,
    detail: Option<String>,
}

impl LinuxDoFinalizeResponse {
    fn success() -> Self {
        Self {
            outcome: LinuxDoFinalizeOutcome::Success,
            provider: "linuxdo",
            redirect_to: Some("/console"),
            detail: None,
        }
    }

    fn invalid_state(detail: impl Into<String>) -> Self {
        Self {
            outcome: LinuxDoFinalizeOutcome::InvalidState,
            provider: "linuxdo",
            redirect_to: None,
            detail: Some(detail.into()),
        }
    }

    fn registration_paused() -> Self {
        Self {
            outcome: LinuxDoFinalizeOutcome::RegistrationPaused,
            provider: "linuxdo",
            redirect_to: Some("/registration-paused"),
            detail: None,
        }
    }

    fn inactive_user() -> Self {
        Self {
            outcome: LinuxDoFinalizeOutcome::InactiveUser,
            provider: "linuxdo",
            redirect_to: None,
            detail: Some("linuxdo account is inactive".to_string()),
        }
    }

    fn upstream_failure(detail: impl Into<String>) -> Self {
        Self {
            outcome: LinuxDoFinalizeOutcome::UpstreamFailure,
            provider: "linuxdo",
            redirect_to: None,
            detail: Some(detail.into()),
        }
    }

    fn server_error(detail: impl Into<String>) -> Self {
        Self {
            outcome: LinuxDoFinalizeOutcome::ServerError,
            provider: "linuxdo",
            redirect_to: None,
            detail: Some(detail.into()),
        }
    }
}

#[derive(Debug)]
enum LinuxDoFinalizeResult {
    Success { session_token: String },
    InvalidState { detail: String },
    RegistrationPaused,
    InactiveUser,
    UpstreamFailure { detail: String },
    ServerError { detail: String },
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

async fn finalize_linuxdo_login(
    state: &AppState,
    code: &str,
    oauth_state: &str,
    binding_hash: &str,
) -> LinuxDoFinalizeResult {
    let cfg = &state.linuxdo_oauth;
    let state_payload = match state
        .proxy
        .consume_oauth_login_state_with_binding_and_token(
            "linuxdo",
            oauth_state,
            Some(binding_hash),
        )
        .await
    {
        Ok(Some(payload)) => payload,
        Ok(None) => {
            return LinuxDoFinalizeResult::InvalidState {
                detail: "oauth state is missing, expired, or already used".to_string(),
            };
        }
        Err(err) => {
            eprintln!("consume linuxdo oauth state error: {err}");
            return LinuxDoFinalizeResult::ServerError {
                detail: "failed to consume oauth login state".to_string(),
            };
        }
    };
    let preferred_token_id = state_payload.bind_token_id;

    let client = reqwest::Client::new();
    let (profile, token_payload) =
        match fetch_linuxdo_profile_from_authorization_code(&client, cfg, code).await {
            Ok(result) => result,
            Err(err) => {
                eprintln!("linuxdo finalize oauth flow error: {err}");
                return LinuxDoFinalizeResult::UpstreamFailure {
                    detail: err.to_string(),
                };
            }
        };
    let provider_user_id = profile.provider_user_id.clone();
    let allow_registration = match state.proxy.allow_registration().await {
        Ok(value) => value,
        Err(err) => {
            eprintln!("read allow registration during linuxdo finalize error: {err}");
            return LinuxDoFinalizeResult::ServerError {
                detail: "failed to read registration policy".to_string(),
            };
        }
    };
    if !allow_registration {
        let existing_account = match state
            .proxy
            .oauth_account_exists("linuxdo", &provider_user_id)
            .await
        {
            Ok(value) => value,
            Err(err) => {
                eprintln!("query linuxdo oauth account existence error: {err}");
                return LinuxDoFinalizeResult::ServerError {
                    detail: "failed to read existing linuxdo account binding".to_string(),
                };
            }
        };
        if !existing_account {
            return LinuxDoFinalizeResult::RegistrationPaused;
        }
    }
    let username = profile.username.clone();

    let user = match state.proxy.upsert_oauth_account(&profile).await {
        Ok(user) => user,
        Err(err) => {
            eprintln!("upsert linuxdo oauth account error: {err}");
            return LinuxDoFinalizeResult::ServerError {
                detail: "failed to persist linuxdo account".to_string(),
            };
        }
    };
    let sync_attempted_at = state.proxy.backend_time().now_ts();
    if let Err(err) = persist_linuxdo_refresh_token_best_effort(
        state,
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
            eprintln!("record linuxdo finalize sync failure error: {mark_err}");
        }
    } else if let Err(err) = state
        .proxy
        .record_oauth_account_profile_sync_success("linuxdo", &provider_user_id, sync_attempted_at)
        .await
    {
        eprintln!("record linuxdo finalize sync success error: {err}");
    }
    if !profile.active {
        return LinuxDoFinalizeResult::InactiveUser;
    }

    let note = format!(
        "linuxdo:{}",
        username.clone().unwrap_or_else(|| provider_user_id.clone())
    );
    if let Err(err) = state
        .proxy
        .ensure_user_token_binding_with_preferred(
            &user.user_id,
            Some(&note),
            preferred_token_id.as_deref(),
        )
        .await
    {
        eprintln!("ensure user token binding error: {err}");
        return LinuxDoFinalizeResult::ServerError {
            detail: "failed to ensure user token binding".to_string(),
        };
    }

    let session = match state
        .proxy
        .create_user_session(&user, cfg.session_max_age_secs)
        .await
    {
        Ok(session) => session,
        Err(err) => {
            eprintln!("create user session error: {err}");
            return LinuxDoFinalizeResult::ServerError {
                detail: "failed to create user session".to_string(),
            };
        }
    };
    LinuxDoFinalizeResult::Success {
        session_token: session.token,
    }
}

fn linuxdo_finalize_json_response(
    payload: LinuxDoFinalizeResponse,
    use_secure_cookie: bool,
    session_cookie: Option<HeaderValue>,
) -> Result<Response<Body>, StatusCode> {
    let clear_binding_cookie = oauth_login_binding_clear_cookie(use_secure_cookie)?;
    let mut response = Json(payload).into_response();
    if let Some(cookie) = session_cookie {
        response.headers_mut().append(SET_COOKIE, cookie);
    }
    response
        .headers_mut()
        .append(SET_COOKIE, clear_binding_cookie);
    Ok(response)
}

async fn render_linuxdo_callback_diagnostic(
    cfg: &LinuxDoOAuthOptions,
    query: &LinuxDoCallbackQuery,
) -> Result<Response<Body>, StatusCode> {
    let configured_redirect = cfg.redirect_url.as_deref().unwrap_or("(not configured)");
    let received_flags = [
        query.code.as_deref().filter(|value| !value.trim().is_empty()).map(|_| "code"),
        query.state.as_deref().filter(|value| !value.trim().is_empty()).map(|_| "state"),
        query.error.as_deref().filter(|value| !value.trim().is_empty()).map(|_| "error"),
        query.error_description
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|_| "error_description"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(", ");
    let body = format!(
        r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>LinuxDo OAuth callback moved</title>
    <style>
      :root {{
        color-scheme: light dark;
        font-family: ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      }}
      body {{
        margin: 0;
        min-height: 100vh;
        display: grid;
        place-items: center;
        padding: 24px;
        background:
          radial-gradient(circle at top, rgba(34, 197, 94, 0.12), transparent 32%),
          linear-gradient(180deg, rgba(248, 250, 252, 1), rgba(241, 245, 249, 1));
        color: #0f172a;
      }}
      @media (prefers-color-scheme: dark) {{
        body {{
          background:
            radial-gradient(circle at top, rgba(34, 197, 94, 0.16), transparent 32%),
            linear-gradient(180deg, rgba(2, 6, 23, 1), rgba(15, 23, 42, 1));
          color: #e2e8f0;
        }}
      }}
      main {{
        width: min(100%, 760px);
        border-radius: 28px;
        border: 1px solid rgba(148, 163, 184, 0.26);
        background: rgba(255, 255, 255, 0.9);
        box-shadow: 0 30px 90px -48px rgba(15, 23, 42, 0.42);
        padding: 28px;
      }}
      @media (prefers-color-scheme: dark) {{
        main {{
          background: rgba(15, 23, 42, 0.86);
        }}
      }}
      h1 {{ margin: 0 0 12px; font-size: 1.8rem; }}
      p {{ margin: 0 0 12px; line-height: 1.65; }}
      code {{
        display: inline-block;
        padding: 2px 6px;
        border-radius: 999px;
        background: rgba(148, 163, 184, 0.16);
      }}
      pre {{
        margin: 18px 0;
        padding: 14px 16px;
        border-radius: 20px;
        overflow: auto;
        background: rgba(15, 23, 42, 0.08);
      }}
      @media (prefers-color-scheme: dark) {{
        pre {{
          background: rgba(148, 163, 184, 0.14);
        }}
      }}
      a {{
        color: inherit;
        font-weight: 700;
      }}
    </style>
  </head>
  <body>
    <main>
      <h1>LinuxDo OAuth callback moved</h1>
      <p>
        <code>GET /auth/linuxdo/callback</code> is now a diagnostics-only endpoint. Normal login completion must land on the
        frontend callback route and then call <code>POST /auth/linuxdo/finalize</code>.
      </p>
      <p>
        Update the LinuxDo app redirect URI and <code>LINUXDO_OAUTH_REDIRECT_URL</code> so they point at the frontend callback path.
      </p>
      <pre>Configured redirect URI: {configured_redirect}
Received query keys: {received_flags}</pre>
      <p>
        If you reached this page during a login attempt, the redirect URI is still pointed at the legacy backend callback.
      </p>
      <p><a href="/">Return home</a></p>
    </main>
  </body>
</html>"#,
        received_flags = if received_flags.is_empty() {
            "(none)"
        } else {
            received_flags.as_str()
        },
    );
    Response::builder()
        .status(StatusCode::CONFLICT)
        .header(CONTENT_TYPE, "text/html; charset=utf-8")
        .body(Body::from(body))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn get_linuxdo_auth(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    if require_full_master_write(state.as_ref()).await.is_err() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    start_linuxdo_auth(state, headers, None).await
}

async fn post_linuxdo_auth(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Form(payload): Form<LinuxDoAuthForm>,
) -> Result<Response<Body>, StatusCode> {
    if require_full_master_write(state.as_ref()).await.is_err() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    start_linuxdo_auth(state, headers, payload.token).await
}

async fn get_linuxdo_callback(
    State(state): State<Arc<AppState>>,
    Query(query): Query<LinuxDoCallbackQuery>,
) -> Result<Response<Body>, StatusCode> {
    let cfg = &state.linuxdo_oauth;
    if !cfg.is_enabled_and_configured() {
        return Err(StatusCode::NOT_FOUND);
    }
    render_linuxdo_callback_diagnostic(cfg, &query).await
}

async fn post_linuxdo_finalize(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<LinuxDoFinalizeRequest>,
) -> Result<Response<Body>, StatusCode> {
    let cfg = &state.linuxdo_oauth;
    if !cfg.is_enabled_and_configured() {
        return Err(StatusCode::NOT_FOUND);
    }

    let use_secure_cookie = wants_secure_cookie(&headers);
    let code = payload.code.trim();
    let oauth_state = payload.state.trim();
    if code.is_empty() || oauth_state.is_empty() {
        return linuxdo_finalize_json_response(
            LinuxDoFinalizeResponse::invalid_state("missing code or state"),
            use_secure_cookie,
            None,
        );
    }
    let Some(binding_nonce) = cookie_value(&headers, OAUTH_LOGIN_BINDING_COOKIE_NAME)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return linuxdo_finalize_json_response(
            LinuxDoFinalizeResponse::invalid_state("missing oauth binding cookie"),
            use_secure_cookie,
            None,
        );
    };

    let binding_hash = hash_oauth_binding(&binding_nonce);
    let result = finalize_linuxdo_login(state.as_ref(), code, oauth_state, &binding_hash).await;
    match result {
        LinuxDoFinalizeResult::Success { session_token } => {
            let session_cookie =
                user_session_set_cookie(&session_token, cfg.session_max_age_secs, use_secure_cookie)?;
            linuxdo_finalize_json_response(
                LinuxDoFinalizeResponse::success(),
                use_secure_cookie,
                Some(session_cookie),
            )
        }
        LinuxDoFinalizeResult::InvalidState { detail } => linuxdo_finalize_json_response(
            LinuxDoFinalizeResponse::invalid_state(detail),
            use_secure_cookie,
            None,
        ),
        LinuxDoFinalizeResult::RegistrationPaused => linuxdo_finalize_json_response(
            LinuxDoFinalizeResponse::registration_paused(),
            use_secure_cookie,
            None,
        ),
        LinuxDoFinalizeResult::InactiveUser => linuxdo_finalize_json_response(
            LinuxDoFinalizeResponse::inactive_user(),
            use_secure_cookie,
            None,
        ),
        LinuxDoFinalizeResult::UpstreamFailure { detail } => linuxdo_finalize_json_response(
            LinuxDoFinalizeResponse::upstream_failure(detail),
            use_secure_cookie,
            None,
        ),
        LinuxDoFinalizeResult::ServerError { detail } => linuxdo_finalize_json_response(
            LinuxDoFinalizeResponse::server_error(detail),
            use_secure_cookie,
            None,
        ),
    }
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
    debug_info_shared: bool,
    request_rate: tavily_hikari::RequestRateView,
    business_calls_1h: tavily_hikari::BusinessCalls1hSummary,
    daily_credits_used: i64,
    daily_credits_limit: i64,
    monthly_credits_used: i64,
    monthly_credits_limit: i64,
    daily_success: i64,
    daily_failure: i64,
    monthly_success: i64,
    last_activity: Option<i64>,
    recharge: RechargeSummaryView,
}

impl From<tavily_hikari::UserDashboardSummary> for UserDashboardView {
    fn from(summary: tavily_hikari::UserDashboardSummary) -> Self {
        Self {
            debug_info_shared: summary.debug_info_shared,
            request_rate: summary.request_rate,
            business_calls_1h: summary.business_calls_1h,
            daily_credits_used: summary.daily_credits_used,
            daily_credits_limit: summary.daily_credits_limit,
            monthly_credits_used: summary.monthly_credits_used,
            monthly_credits_limit: summary.monthly_credits_limit,
            daily_success: summary.daily_success,
            daily_failure: summary.daily_failure,
            monthly_success: summary.monthly_success,
            last_activity: summary.last_activity,
            recharge: summary.recharge.into(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserDashboardOverviewSeriesPointView {
    bucket_start: i64,
    display_bucket_start: Option<i64>,
    value: Option<i64>,
    limit_value: Option<i64>,
}

impl From<tavily_hikari::UserDashboardOverviewSeriesPoint> for UserDashboardOverviewSeriesPointView {
    fn from(point: tavily_hikari::UserDashboardOverviewSeriesPoint) -> Self {
        Self {
            bucket_start: point.bucket_start,
            display_bucket_start: point.display_bucket_start,
            value: point.value,
            limit_value: point.limit_value,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserDashboardProgressCardView {
    used: i64,
    limit: i64,
    points: Vec<UserDashboardOverviewSeriesPointView>,
}

impl From<tavily_hikari::UserDashboardProgressCard> for UserDashboardProgressCardView {
    fn from(card: tavily_hikari::UserDashboardProgressCard) -> Self {
        Self {
            used: card.used,
            limit: card.limit,
            points: card.points.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserDashboardOverviewProgressView {
    request_rate: UserDashboardProgressCardView,
    business_calls_1h: UserDashboardProgressCardView,
    daily_credits: UserDashboardProgressCardView,
    monthly_credits: UserDashboardProgressCardView,
}

impl From<tavily_hikari::UserDashboardOverviewProgress> for UserDashboardOverviewProgressView {
    fn from(progress: tavily_hikari::UserDashboardOverviewProgress) -> Self {
        Self {
            request_rate: progress.request_rate.into(),
            business_calls_1h: progress.business_calls_1h.into(),
            daily_credits: progress.daily_credits.into(),
            monthly_credits: progress.monthly_credits.into(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserDashboardOverviewView {
    summary: UserDashboardView,
    progress: UserDashboardOverviewProgressView,
}

impl From<tavily_hikari::UserDashboardOverviewSnapshot> for UserDashboardOverviewView {
    fn from(snapshot: tavily_hikari::UserDashboardOverviewSnapshot) -> Self {
        Self {
            summary: snapshot.summary.into(),
            progress: snapshot.progress.into(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserDebugInfoSharingPayload {
    shared: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserDebugInfoSharingView {
    debug_info_shared: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RechargeSummaryView {
    current_month_start: i64,
    current_entitlement_credits: i64,
    current_entitlement_hourly_delta: i64,
    current_entitlement_daily_delta: i64,
    current_entitlement_monthly_delta: i64,
    effective_until_month_start: Option<i64>,
}

impl From<tavily_hikari::LinuxDoCreditRechargeSummary> for RechargeSummaryView {
    fn from(value: tavily_hikari::LinuxDoCreditRechargeSummary) -> Self {
        Self {
            current_month_start: value.current_month_start,
            current_entitlement_credits: value.current_month_entitlement_credits,
            current_entitlement_hourly_delta: value.current_month_entitlement_hourly_delta,
            current_entitlement_daily_delta: value.current_month_entitlement_daily_delta,
            current_entitlement_monthly_delta: value.current_month_entitlement_monthly_delta,
            effective_until_month_start: value.effective_until_month_start,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct RechargeQuoteMonthView {
    month_index: i64,
    month_start: i64,
    is_current_month: bool,
    hourly_delta: i64,
    daily_delta: i64,
    monthly_delta: i64,
    full_monthly_delta: i64,
    month_money_cents: i64,
    month_discount_cents: i64,
    month_end_clamp_applied: bool,
    discount_reason: Option<String>,
}

impl From<tavily_hikari::LinuxDoCreditRechargeQuoteMonth> for RechargeQuoteMonthView {
    fn from(value: tavily_hikari::LinuxDoCreditRechargeQuoteMonth) -> Self {
        Self {
            month_index: value.month_index,
            month_start: value.month_start,
            is_current_month: value.is_current_month,
            hourly_delta: value.hourly_delta,
            daily_delta: value.daily_delta,
            monthly_delta: value.monthly_delta,
            full_monthly_delta: value.full_monthly_delta,
            month_money_cents: value.month_money_cents,
            month_discount_cents: value.month_discount_cents,
            month_end_clamp_applied: value.month_end_clamp_applied,
            discount_reason: value.discount_reason,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct RechargeQuoteView {
    requested_credits: i64,
    requested_months: i64,
    quote_month_start: i64,
    remaining_days_inclusive: i64,
    unit_credits: i64,
    unit_price_cents: i64,
    full_month_hourly_delta: i64,
    full_month_daily_delta: i64,
    full_month_monthly_delta: i64,
    full_month_money_cents: i64,
    current_month_final_hourly_delta: i64,
    current_month_final_daily_delta: i64,
    current_month_final_monthly_delta: i64,
    current_month_final_money_cents: i64,
    full_order_money_cents: i64,
    final_order_money_cents: i64,
    month_end_clamp_applied: bool,
    order_name: String,
    schedule: Vec<RechargeQuoteMonthView>,
}

impl From<tavily_hikari::LinuxDoCreditRechargeQuote> for RechargeQuoteView {
    fn from(value: tavily_hikari::LinuxDoCreditRechargeQuote) -> Self {
        Self {
            requested_credits: value.requested_credits,
            requested_months: value.requested_months,
            quote_month_start: value.quote_month_start,
            remaining_days_inclusive: value.remaining_days_inclusive,
            unit_credits: value.unit_credits,
            unit_price_cents: value.unit_price_cents,
            full_month_hourly_delta: value.full_month_hourly_delta,
            full_month_daily_delta: value.full_month_daily_delta,
            full_month_monthly_delta: value.full_month_monthly_delta,
            full_month_money_cents: value.full_month_money_cents,
            current_month_final_hourly_delta: value.current_month_final_hourly_delta,
            current_month_final_daily_delta: value.current_month_final_daily_delta,
            current_month_final_monthly_delta: value.current_month_final_monthly_delta,
            current_month_final_money_cents: value.current_month_final_money_cents,
            full_order_money_cents: value.full_order_money_cents,
            final_order_money_cents: value.final_order_money_cents,
            month_end_clamp_applied: value.month_end_clamp_applied,
            order_name: value.order_name,
            schedule: value.schedule.into_iter().map(Into::into).collect(),
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
            money: tavily_hikari::format_linuxdo_credit_money(value.final_money_cents),
            quote_month_start: value.quote_month_start,
            final_money_cents: value.final_money_cents,
            final_hourly_delta: value.final_hourly_delta,
            final_daily_delta: value.final_daily_delta,
            final_monthly_delta: value.final_monthly_delta,
            month_end_clamp_applied: value.month_end_clamp_applied,
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
    current_entitlement_hourly_delta: i64,
    current_entitlement_daily_delta: i64,
    current_entitlement_monthly_delta: i64,
    effective_until_month_start: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RechargeOrdersView {
    items: Vec<RechargeOrderView>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateRechargeQuoteRequest {
    credits: i64,
    months: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateRechargeOrderRequest {
    credits: i64,
    months: i64,
    quote: RechargeQuoteView,
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
    business_calls_1h: tavily_hikari::BusinessCalls1hSummary,
    daily_credits_used: i64,
    daily_credits_limit: i64,
    monthly_credits_used: i64,
    monthly_credits_limit: i64,
    daily_success: i64,
    daily_failure: i64,
    monthly_success: i64,
}

#[derive(Debug, Deserialize)]
struct UserTokenLogsQuery {
    limit: Option<usize>,
    billing: Option<String>,
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

async fn build_user_dashboard_overview_snapshot_event(
    state: &Arc<AppState>,
    user_id: &str,
    daily_window: Option<tavily_hikari::TimeRangeUtc>,
) -> Option<(Event, String)> {
    let overview = state
        .proxy
        .user_dashboard_overview(user_id, daily_window)
        .await
        .ok()?;
    let payload: UserDashboardOverviewView = overview.into();
    let json = serde_json::to_string(&payload).ok()?;
    Some((Event::default().event("snapshot").data(json.clone()), json))
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
    Ok(Json(summary.into()))
}

async fn get_user_dashboard_overview(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserTodayWindowQuery>,
) -> Result<Json<UserDashboardOverviewView>, (StatusCode, String)> {
    if !state.linuxdo_oauth.is_enabled_and_configured() {
        return Err((StatusCode::NOT_FOUND, "not found".to_string()));
    }
    let Some(user_session) = resolve_user_session(state.as_ref(), &headers).await else {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
    };
    let daily_window = parse_user_today_window_query(&query)?;
    let overview = state
        .proxy
        .user_dashboard_overview(&user_session.user.user_id, daily_window)
        .await
        .map_err(|err| {
            eprintln!("get user dashboard overview error: {err}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load dashboard overview".to_string(),
            )
        })?;
    Ok(Json(overview.into()))
}

async fn put_user_debug_info_sharing(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserDebugInfoSharingPayload>,
) -> Result<Json<UserDebugInfoSharingView>, (StatusCode, String)> {
    require_full_master_write(state.as_ref()).await?;
    if !state.linuxdo_oauth.is_enabled_and_configured() {
        return Err((StatusCode::NOT_FOUND, "not found".to_string()));
    }
    let Some(user_session) = resolve_user_session(state.as_ref(), &headers).await else {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
    };
    let debug_info_shared = state
        .proxy
        .set_user_debug_info_shared(&user_session.user.user_id, payload.shared)
        .await
        .map_err(|err| {
            eprintln!("put user debug info sharing error: {err}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to update debug sharing".to_string(),
            )
        })?;
    Ok(Json(UserDebugInfoSharingView { debug_info_shared }))
}

fn linuxdo_credit_config_unavailable() -> (StatusCode, String) {
    (StatusCode::SERVICE_UNAVAILABLE, "recharge not configured".to_string())
}

fn user_local_month_start_utc_ts(now: chrono::DateTime<chrono::Local>) -> i64 {
    let first_day = chrono::NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
        .expect("valid start of month date");
    let naive = first_day
        .and_hms_opt(0, 0, 0)
        .expect("valid start of month time");
    match chrono::Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => dt.with_timezone(&chrono::Utc).timestamp(),
        chrono::LocalResult::Ambiguous(dt, _) => dt.with_timezone(&chrono::Utc).timestamp(),
        chrono::LocalResult::None => now.with_timezone(&chrono::Utc).timestamp(),
    }
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
        current_entitlement_hourly_delta: summary.current_month_entitlement_hourly_delta,
        current_entitlement_daily_delta: summary.current_month_entitlement_daily_delta,
        current_entitlement_monthly_delta: summary.current_month_entitlement_monthly_delta,
        effective_until_month_start: summary.effective_until_month_start,
    }))
}

async fn post_user_recharge_quote(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateRechargeQuoteRequest>,
) -> Result<Json<RechargeQuoteView>, (StatusCode, String)> {
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
    let now = state.proxy.backend_time().now_ts();
    let quote_month_start = user_local_month_start_utc_ts(state.proxy.backend_time().local_now());
    let Some(quote) = tavily_hikari::linuxdo_credit_recharge_quote(
        payload.credits,
        payload.months,
        price,
        quote_month_start,
        now,
    ) else {
        return Err((StatusCode::BAD_REQUEST, "unable to build recharge quote".to_string()));
    };
    let _ = user_session;
    Ok(Json(quote.into()))
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
    Json(raw_payload): Json<serde_json::Value>,
) -> Result<Json<CreateRechargeOrderResponse>, (StatusCode, String)> {
    require_full_master_write(state.as_ref()).await?;
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
    let payload: CreateRechargeOrderRequest = serde_json::from_value(raw_payload)
        .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))?;
    let price = state.linuxdo_credit.price_config();
    let now = state.proxy.backend_time().now_ts();
    let quote_month_start = user_local_month_start_utc_ts(state.proxy.backend_time().local_now());
    let Some(server_quote) = tavily_hikari::linuxdo_credit_recharge_quote(
        payload.credits,
        payload.months,
        price,
        quote_month_start,
        now,
    ) else {
        return Err((StatusCode::BAD_REQUEST, "unable to build recharge quote".to_string()));
    };
    let quote_json = serde_json::to_value(&server_quote)
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let client_quote_json = serde_json::to_value(&payload.quote)
        .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))?;
    if quote_json != client_quote_json {
        return Err((StatusCode::BAD_REQUEST, "quote mismatch".to_string()));
    }
    if payload.quote.quote_month_start != quote_month_start {
        return Err((StatusCode::BAD_REQUEST, "quote month expired".to_string()));
    }
    if payload.quote.requested_credits != payload.credits
        || payload.quote.requested_months != payload.months
    {
        return Err((StatusCode::BAD_REQUEST, "quote request mismatch".to_string()));
    }
    let money_cents = server_quote.full_order_money_cents;
    let final_money_cents = server_quote.final_order_money_cents;
    let out_trade_no = format!("ldc_{}", nanoid!(24));
    let order_name = server_quote.order_name.clone();
    let mut order = tavily_hikari::LinuxDoCreditRechargeOrder {
        out_trade_no: out_trade_no.clone(),
        user_id: user_session.user.user_id.clone(),
        status: tavily_hikari::LINUXDO_CREDIT_RECHARGE_STATUS_PENDING.to_string(),
        credits: payload.credits,
        months: payload.months,
        money_cents,
        quote_month_start,
        final_money_cents,
        final_hourly_delta: server_quote.current_month_final_hourly_delta,
        final_daily_delta: server_quote.current_month_final_daily_delta,
        final_monthly_delta: server_quote.current_month_final_monthly_delta,
        month_end_clamp_applied: server_quote.month_end_clamp_applied,
        quote_snapshot_json: Some(
            serde_json::to_string(&serde_json::json!({
                "version": 1,
                "source": "server_quote",
                "request": {
                    "credits": payload.credits,
                    "months": payload.months,
                },
                "quote": server_quote,
            }))
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?,
        ),
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
    let money = tavily_hikari::format_linuxdo_credit_money(final_money_cents);
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
            let failed_at = state.proxy.backend_time().now_ts();
            tokio::spawn(async move {
                let _ = proxy
                    .fail_linuxdo_credit_recharge_order(
                        &out_trade_no,
                        "payment upstream transport error",
                        failed_at,
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
                state.proxy.backend_time().now_ts(),
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
                let failed_at = state.proxy.backend_time().now_ts();
                tokio::spawn(async move {
                    let _ = proxy
                        .fail_linuxdo_credit_recharge_order(
                            &out_trade_no,
                            &stored_message,
                            failed_at,
                        )
                        .await;
                });
                (StatusCode::BAD_GATEWAY, message)
            },
        )?;
    state
        .proxy
        .set_linuxdo_credit_recharge_payment_url(
            &out_trade_no,
            &payment_url,
            state.proxy.backend_time().now_ts(),
        )
        .await
        .map_err(|err| map_recharge_error("persist recharge payment url", err))?;
    order.payment_url = Some(payment_url.clone());
    order.updated_at = state.proxy.backend_time().now_ts();
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
    require_full_master_write(state.as_ref()).await?;
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
    let expected_money = tavily_hikari::format_linuxdo_credit_money(order.final_money_cents);
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
            state.proxy.backend_time().now_ts(),
        )
        .await
        .map_err(|err| map_recharge_error("apply recharge notify", err))?;
    Ok((StatusCode::OK, "success"))
}

async fn build_user_token_detail_view(
    state: &Arc<AppState>,
    user_id: &str,
    token_id: &str,
    daily_window: Option<tavily_hikari::TimeRangeUtc>,
) -> Result<UserTokenSummaryView, (StatusCode, String)> {
    let shared_summary = state
        .proxy
        .user_dashboard_summary(user_id, daily_window)
        .await
        .map_err(|err| {
            eprintln!("get user token detail shared summary error: {err}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load shared quota".to_string(),
            )
        })?;
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

    Ok(UserTokenSummaryView {
        token_id: token.id,
        enabled: token.enabled,
        note: token.note,
        last_used_at: token.last_used_at,
        request_rate: request_rate.request_rate(),
        business_calls_1h: shared_summary.business_calls_1h,
        daily_credits_used: shared_summary.daily_credits_used,
        daily_credits_limit: shared_summary.daily_credits_limit,
        monthly_credits_used: shared_summary.monthly_credits_used,
        monthly_credits_limit: shared_summary.monthly_credits_limit,
        daily_success,
        daily_failure,
        monthly_success,
    })
}

async fn build_user_token_logs_view(
    state: &Arc<AppState>,
    token_id: &str,
    limit: usize,
    billing_filter: TokenLogBillingFilter,
    language: UiLanguage,
) -> Result<Vec<UserTokenLogView>, StatusCode> {
    let items = state
        .proxy
        .token_recent_logs_by_billing(token_id, limit.clamp(1, 50), None, billing_filter)
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
    let logs = build_user_token_logs_view(state, token_id, 50, TokenLogBillingFilter::All, language)
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
    let shared_summary = state
        .proxy
        .user_dashboard_summary(&user_session.user.user_id, daily_window)
        .await
        .map_err(|err| {
            eprintln!("list user tokens shared summary error: {err}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to load shared quota".to_string(),
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
        let request_rate = hourly_any
            .get(&token.id)
            .cloned()
            .unwrap_or_else(|| {
                state
                    .proxy
                    .default_request_rate_verdict(tavily_hikari::RequestRateScope::Token)
            });
        items.push(UserTokenSummaryView {
            token_id: token.id,
            enabled: token.enabled,
            note: token.note,
            last_used_at: token.last_used_at,
            request_rate: request_rate.request_rate(),
            business_calls_1h: shared_summary.business_calls_1h.clone(),
            daily_credits_used: shared_summary.daily_credits_used,
            daily_credits_limit: shared_summary.daily_credits_limit,
            monthly_credits_used: shared_summary.monthly_credits_used,
            monthly_credits_limit: shared_summary.monthly_credits_limit,
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
    let limit = q.limit.unwrap_or(50);
    let billing_filter = match q.billing.as_deref().map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case("billable") => TokenLogBillingFilter::Billable,
        _ => TokenLogBillingFilter::All,
    };
    let logs = build_user_token_logs_view(&state, &id, limit, billing_filter, language).await?;
    Ok(Json(logs))
}

async fn sse_user_dashboard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserTodayWindowQuery>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, axum::http::Error>>>, StatusCode> {
    if !state.linuxdo_oauth.is_enabled_and_configured() {
        return Err(StatusCode::NOT_FOUND);
    }
    let Some(user_session) = resolve_user_session(state.as_ref(), &headers).await else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    let daily_window = parse_user_today_window_query(&query).map_err(|(status, _)| status)?;
    let user_id = user_session.user.user_id.clone();
    let state = state.clone();
    let stream = stream! {
        let mut last_snapshot_json: Option<String> = None;
        if let Some((event, snapshot_json)) =
            build_user_dashboard_overview_snapshot_event(&state, &user_id, daily_window).await
        {
            last_snapshot_json = Some(snapshot_json);
            yield Ok(event);
        }
        loop {
            match build_user_dashboard_overview_snapshot_event(&state, &user_id, daily_window).await {
                Some((event, snapshot_json)) if last_snapshot_json.as_deref() != Some(snapshot_json.as_str()) => {
                    last_snapshot_json = Some(snapshot_json);
                    yield Ok(event);
                }
                Some(_) => {
                    yield Ok(Event::default().event("ping").data("{}"));
                }
                None => {
                    yield Ok(Event::default().event("ping").data("{}"));
                }
            }
            state.proxy.backend_time().sleep(Duration::from_secs(5)).await;
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("")))
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
            state.proxy.backend_time().sleep(Duration::from_secs(2)).await;
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
