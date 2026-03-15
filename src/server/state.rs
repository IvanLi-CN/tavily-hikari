#[derive(Clone)]
struct AppState {
    proxy: TavilyProxy,
    static_dir: Option<PathBuf>,
    forward_auth: ForwardAuthConfig,
    forward_auth_enabled: bool,
    builtin_admin: BuiltinAdminAuth,
    linuxdo_oauth: LinuxDoOAuthOptions,
    dev_open_admin: bool,
    usage_base: String,
    api_key_ip_geo_origin: String,
}

#[derive(Clone, Debug)]
pub struct ForwardAuthConfig {
    user_header: Option<HeaderName>,
    admin_value: Option<String>,
    nickname_header: Option<HeaderName>,
    admin_override_name: Option<String>,
}

#[derive(Clone)]
pub struct AdminAuthOptions {
    pub forward_auth_enabled: bool,
    pub builtin_auth_enabled: bool,
    pub builtin_auth_password: Option<String>,
    pub builtin_auth_password_hash: Option<String>,
}

#[derive(Clone, Debug)]
pub struct LinuxDoOAuthOptions {
    pub enabled: bool,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub authorize_url: String,
    pub token_url: String,
    pub userinfo_url: String,
    pub scope: String,
    pub redirect_url: Option<String>,
    pub session_max_age_secs: i64,
    pub login_state_ttl_secs: i64,
}

impl LinuxDoOAuthOptions {
    #[cfg(test)]
    fn disabled() -> Self {
        Self {
            enabled: false,
            client_id: None,
            client_secret: None,
            authorize_url: "https://connect.linux.do/oauth2/authorize".to_string(),
            token_url: "https://connect.linux.do/oauth2/token".to_string(),
            userinfo_url: "https://connect.linux.do/api/user".to_string(),
            scope: "user".to_string(),
            redirect_url: None,
            session_max_age_secs: 60 * 60 * 24 * 14,
            login_state_ttl_secs: 600,
        }
    }

    fn is_enabled_and_configured(&self) -> bool {
        self.enabled
            && self
                .client_id
                .as_deref()
                .map(str::trim)
                .is_some_and(|v| !v.is_empty())
            && self
                .client_secret
                .as_deref()
                .map(str::trim)
                .is_some_and(|v| !v.is_empty())
            && self
                .redirect_url
                .as_deref()
                .map(str::trim)
                .is_some_and(|v| !v.is_empty())
    }
}

impl ForwardAuthConfig {
    pub fn new(
        user_header: Option<HeaderName>,
        admin_value: Option<String>,
        nickname_header: Option<HeaderName>,
        admin_override_name: Option<String>,
    ) -> Self {
        Self {
            user_header,
            admin_value,
            nickname_header,
            admin_override_name,
        }
    }

    fn is_enabled(&self) -> bool {
        self.user_header.is_some() || self.admin_override_name.is_some()
    }

    fn user_header(&self) -> Option<&HeaderName> {
        self.user_header.as_ref()
    }

    fn nickname_header(&self) -> Option<&HeaderName> {
        self.nickname_header.as_ref()
    }

    fn admin_value(&self) -> Option<&str> {
        self.admin_value.as_deref()
    }

    fn admin_override_name(&self) -> Option<&str> {
        self.admin_override_name.as_deref()
    }

    fn user_value<'a>(&self, headers: &'a HeaderMap) -> Option<&'a str> {
        // direct get
        if let Some(name) = self.user_header() {
            if let Some(value) = headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .filter(|v| !v.is_empty())
            {
                return Some(value);
            }
            // fallback: scan case-insensitively in case upstream mutated header casing
            let target = name.as_str();
            for (k, v) in headers.iter() {
                let Ok(s) = v.to_str() else {
                    continue;
                };
                if k.as_str().eq_ignore_ascii_case(target) && !s.is_empty() {
                    return Some(s);
                }
            }
        }
        None
    }

    fn nickname_value(&self, headers: &HeaderMap) -> Option<String> {
        self.nickname_header()
            .and_then(|name| headers.get(name))
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }

    fn is_request_admin(&self, headers: &HeaderMap) -> bool {
        if !self.is_enabled() {
            return false;
        }

        match (self.admin_value(), self.user_value(headers)) {
            (Some(expected), Some(actual)) => actual == expected,
            _ => false,
        }
    }
}

const BUILTIN_ADMIN_COOKIE_NAME: &str = "hikari_admin_session";
const BUILTIN_ADMIN_SESSION_MAX_AGE_SECS: u64 = 60 * 60 * 24 * 14;
const BUILTIN_ADMIN_SESSION_MAX_COUNT: usize = 1024;
const USER_SESSION_COOKIE_NAME: &str = "hikari_user_session";
const OAUTH_LOGIN_BINDING_COOKIE_NAME: &str = "hikari_oauth_login_binding";

#[derive(Clone, Debug)]
struct BuiltinAdminSession {
    issued_at: Instant,
    expires_at: Instant,
}

#[derive(Clone, Debug)]
struct BuiltinAdminAuth {
    enabled: bool,
    password: Option<String>,
    password_hash: Option<String>,
    sessions: Arc<std::sync::RwLock<HashMap<String, BuiltinAdminSession>>>,
}

impl BuiltinAdminAuth {
    fn new(enabled: bool, password: Option<String>, password_hash: Option<String>) -> Self {
        Self {
            enabled,
            password,
            password_hash,
            sessions: Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn is_admin(&self, headers: &HeaderMap) -> bool {
        if !self.enabled {
            return false;
        }
        let Some(value) = cookie_value(headers, BUILTIN_ADMIN_COOKIE_NAME) else {
            return false;
        };
        let now = Instant::now();
        let Ok(mut sessions) = self.sessions.write() else {
            return false;
        };
        sessions.retain(|_, session| session.expires_at > now);
        sessions
            .get(&value)
            .is_some_and(|session| session.expires_at > now)
    }

    fn login(&self, password: &str) -> Option<String> {
        if !self.enabled {
            return None;
        }
        if let Some(hash) = self.password_hash.as_deref() {
            let parsed = PasswordHash::new(hash).ok()?;
            if Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_err()
            {
                return None;
            }
        } else {
            let expected = self.password.as_deref()?;
            if password != expected {
                return None;
            }
        }
        Some(self.new_session())
    }

    fn remember_session(&self, token: String) {
        if !self.enabled {
            return;
        }
        let now = Instant::now();
        let expires_at = now + Duration::from_secs(BUILTIN_ADMIN_SESSION_MAX_AGE_SECS);
        if let Ok(mut sessions) = self.sessions.write() {
            sessions.retain(|_, session| session.expires_at > now);
            sessions.insert(
                token,
                BuiltinAdminSession {
                    issued_at: now,
                    expires_at,
                },
            );

            // Bound memory usage: if too many sessions accumulate, evict oldest.
            if sessions.len() > BUILTIN_ADMIN_SESSION_MAX_COUNT {
                let over = sessions.len() - BUILTIN_ADMIN_SESSION_MAX_COUNT;
                let mut issued: Vec<(String, Instant)> = sessions
                    .iter()
                    .map(|(k, v)| (k.clone(), v.issued_at))
                    .collect();
                issued.sort_by_key(|(_, ts)| *ts);
                for (key, _) in issued.into_iter().take(over) {
                    sessions.remove(&key);
                }
            }
        }
    }

    fn forget_session(&self, headers: &HeaderMap) {
        if !self.enabled {
            return;
        }
        let Some(value) = cookie_value(headers, BUILTIN_ADMIN_COOKIE_NAME) else {
            return;
        };
        if let Ok(mut sessions) = self.sessions.write() {
            sessions.remove(&value);
        }
    }

    fn new_session(&self) -> String {
        use base64::Engine as _;
        use rand::RngCore as _;

        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    }
}

fn cookie_value(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    let raw = headers.get(COOKIE)?.to_str().ok()?;
    for part in raw.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some((name, value)) = part.split_once('=') else {
            continue;
        };
        if name.trim() == cookie_name {
            return Some(value.trim().to_string());
        }
    }
    None
}

fn wants_secure_cookie(headers: &HeaderMap) -> bool {
    // Best-effort HTTPS detection for typical reverse proxy deployments.
    // - RFC 7239: Forwarded: proto=https;host=...
    // - De-facto: X-Forwarded-Proto: https
    if headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .next()
                .map(str::trim)
                .is_some_and(|v| v.eq_ignore_ascii_case("https"))
        })
    {
        return true;
    }

    if headers
        .get("forwarded")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("proto=https"))
    {
        return true;
    }

    false
}

fn is_admin_request(state: &AppState, headers: &HeaderMap) -> bool {
    if state.dev_open_admin {
        return true;
    }
    if state.forward_auth_enabled && state.forward_auth.is_request_admin(headers) {
        return true;
    }
    if state.builtin_admin.is_admin(headers) {
        return true;
    }
    false
}

async fn resolve_user_session(
    state: &AppState,
    headers: &HeaderMap,
) -> Option<tavily_hikari::UserSession> {
    if !state.linuxdo_oauth.is_enabled_and_configured() {
        return None;
    }
    let cookie = cookie_value(headers, USER_SESSION_COOKIE_NAME)?;
    match state.proxy.get_user_session(&cookie).await {
        Ok(Some(session)) => Some(session),
        _ => None,
    }
}

fn parse_iso_timestamp(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc).timestamp())
        .ok()
}

fn default_since(period: Option<&str>) -> i64 {
    let now = Utc::now();
    match period {
        Some("day") => now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp(),
        Some("week") => {
            let weekday = now.weekday().num_days_from_monday() as i64;
            (now - ChronoDuration::days(weekday))
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc()
                .timestamp()
        }
        _ => {
            let first = Utc
                .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
                .single()
                .expect("valid start of month");
            first.timestamp()
        }
    }
}

fn default_until(period: Option<&str>, since: i64) -> i64 {
    let base = DateTime::<Utc>::from_timestamp(since, 0).unwrap_or_else(Utc::now);
    match period {
        Some("day") => (base + ChronoDuration::days(1)).timestamp(),
        Some("week") => (base + ChronoDuration::days(7)).timestamp(),
        _ => {
            let date = base.date_naive();
            let (year, month) = if date.month() == 12 {
                (date.year() + 1, 1)
            } else {
                (date.year(), date.month() + 1)
            };
            let naive = NaiveDate::from_ymd_opt(year, month, 1)
                .unwrap_or(date)
                .and_hms_opt(0, 0, 0)
                .unwrap();
            Utc.from_utc_datetime(&naive).timestamp()
        }
    }
}

fn start_of_day_dt(now: chrono::DateTime<Utc>) -> chrono::DateTime<Utc> {
    now.date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("valid start of day")
        .and_utc()
}

fn start_of_month_dt(now: chrono::DateTime<Utc>) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
        .expect("valid start of month")
}

#[derive(Debug, Serialize)]
struct IsAdminDebug {
    is_admin: bool,
    forward_auth_admin: bool,
    builtin_admin: bool,
    user_value: Option<String>,
}

async fn debug_is_admin(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<IsAdminDebug>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let cfg = &state.forward_auth;
    let user_value = if state.forward_auth_enabled {
        cfg.user_value(&headers).map(|s| s.to_string())
    } else {
        None
    };
    let forward_auth_admin = state.forward_auth_enabled && cfg.is_request_admin(&headers);
    let builtin_admin = state.builtin_admin.is_admin(&headers);
    let is_admin = state.dev_open_admin || forward_auth_admin || builtin_admin;
    Ok(Json(IsAdminDebug {
        is_admin,
        forward_auth_admin,
        builtin_admin,
        user_value,
    }))
}

async fn health_check() -> &'static str {
    "ok"
}
