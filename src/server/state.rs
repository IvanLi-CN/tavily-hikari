#[derive(Clone)]
struct AppState {
    proxy: TavilyProxy,
    static_dir: Option<PathBuf>,
    forward_auth: ForwardAuthConfig,
    forward_auth_enabled: bool,
    builtin_admin: BuiltinAdminAuth,
    linuxdo_oauth: LinuxDoOAuthOptions,
    linuxdo_credit: LinuxDoCreditOptions,
    ha: tavily_hikari::HaRuntime,
    dev_open_admin: bool,
    usage_base: String,
    api_key_ip_geo_origin: String,
    dashboard_overview_cache: Arc<Mutex<DashboardOverviewCacheState>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DashboardOverviewFreshness {
    summary: [i64; 10],
    summary_last_activity: Option<i64>,
    forward_proxy: Option<(i64, i64)>,
    exhausted_keys: Vec<String>,
    latest_quota_sync_sample_at: Option<i64>,
    latest_request_log_id: Option<i64>,
    recent_jobs: Vec<(i64, String, Option<i64>)>,
    disabled_tokens: Vec<String>,
    disabled_tokens_error: bool,
    disabled_tokens_truncated: bool,
    recent_alerts_total_events: i64,
    recent_alerts_grouped_count: i64,
    recent_alerts_counts: Vec<(String, i64)>,
    recent_alerts_top_groups: Vec<(String, i64, i64)>,
    request_log_retention_days: i64,
    hourly_window_anchor: i64,
}

#[derive(Debug, Clone)]
struct CachedDashboardOverviewSnapshot {
    snapshot: DashboardOverviewSnapshot,
    freshness: DashboardOverviewFreshness,
}

#[derive(Debug, Clone)]
struct DashboardOverviewCacheState {
    cached: Option<CachedDashboardOverviewSnapshot>,
    loading: bool,
    notify: Arc<tokio::sync::Notify>,
}

impl Default for DashboardOverviewCacheState {
    fn default() -> Self {
        Self {
            cached: None,
            loading: false,
            notify: Arc::new(tokio::sync::Notify::new()),
        }
    }
}

fn new_dashboard_overview_cache() -> Arc<Mutex<DashboardOverviewCacheState>> {
    Arc::new(Mutex::new(DashboardOverviewCacheState::default()))
}

static DB_MAINTENANCE_GATE: OnceLock<RwLock<()>> = OnceLock::new();
static DB_JOB_EXECUTION_GATES: OnceLock<std::sync::Mutex<HashMap<usize, std::sync::Weak<Mutex<()>>>>> =
    OnceLock::new();
static MAINTENANCE_WORKER_WAKES: OnceLock<
    std::sync::Mutex<HashMap<usize, std::sync::Weak<tokio::sync::Notify>>>,
> = OnceLock::new();
static MAINTENANCE_REMOTE_IO_SLOTS: OnceLock<
    std::sync::Mutex<HashMap<usize, std::sync::Weak<Semaphore>>>,
> = OnceLock::new();

fn db_maintenance_gate() -> &'static RwLock<()> {
    DB_MAINTENANCE_GATE.get_or_init(|| RwLock::new(()))
}

fn db_job_execution_gate_for_state(state: &AppState) -> Arc<Mutex<()>> {
    let key = state as *const AppState as usize;
    let gates = DB_JOB_EXECUTION_GATES.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut gates = gates.lock().expect("db job execution gate map lock");
    if let Some(gate) = gates.get(&key).and_then(std::sync::Weak::upgrade) {
        return gate;
    }
    let gate = Arc::new(Mutex::new(()));
    gates.insert(key, Arc::downgrade(&gate));
    gate
}

fn maintenance_worker_wake_for_state(state: &AppState) -> Arc<tokio::sync::Notify> {
    let key = state as *const AppState as usize;
    let wakes = MAINTENANCE_WORKER_WAKES.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut wakes = wakes.lock().expect("maintenance worker wake map lock");
    if let Some(wake) = wakes.get(&key).and_then(std::sync::Weak::upgrade) {
        return wake;
    }
    let wake = Arc::new(tokio::sync::Notify::new());
    wakes.insert(key, Arc::downgrade(&wake));
    wake
}

fn maintenance_remote_io_slot_for_state(state: &AppState) -> Arc<Semaphore> {
    let key = state as *const AppState as usize;
    let slots =
        MAINTENANCE_REMOTE_IO_SLOTS.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut slots = slots.lock().expect("maintenance remote I/O slot map lock");
    if let Some(slot) = slots.get(&key).and_then(std::sync::Weak::upgrade) {
        return slot;
    }
    let slot = Arc::new(Semaphore::new(1));
    slots.insert(key, Arc::downgrade(&slot));
    slot
}

fn dashboard_overview_cache_for_state(state: &AppState) -> Arc<Mutex<DashboardOverviewCacheState>> {
    state.dashboard_overview_cache.clone()
}

fn try_acquire_maintenance_remote_io_slot_for_state(
    state: &AppState,
) -> Option<tokio::sync::OwnedSemaphorePermit> {
    maintenance_remote_io_slot_for_state(state)
        .try_acquire_owned()
        .ok()
}

async fn acquire_db_maintenance_read_gate() -> tokio::sync::RwLockReadGuard<'static, ()> {
    db_maintenance_gate().read().await
}

async fn acquire_db_maintenance_write_gate() -> tokio::sync::RwLockWriteGuard<'static, ()> {
    db_maintenance_gate().write().await
}

async fn acquire_db_job_execution_gate_for_state(
    state: &AppState,
) -> tokio::sync::OwnedMutexGuard<()> {
    db_job_execution_gate_for_state(state).lock_owned().await
}

#[cfg(test)]
pub(crate) async fn acquire_db_job_execution_gate() -> tokio::sync::OwnedMutexGuard<()> {
    static TEST_DB_JOB_EXECUTION_GATE: OnceLock<Arc<Mutex<()>>> = OnceLock::new();
    TEST_DB_JOB_EXECUTION_GATE
        .get_or_init(|| Arc::new(Mutex::new(())))
        .clone()
        .lock_owned()
        .await
}

async fn db_maintenance_http_gate(
    req: Request<Body>,
    next: axum::middleware::Next,
) -> Response<Body> {
    let path = req.uri().path();
    if path == "/health" || !db_maintenance_gated_path(path) {
        return next.run(req).await;
    }

    let _guard = acquire_db_maintenance_read_gate().await;
    next.run(req).await
}

fn db_maintenance_gated_path(path: &str) -> bool {
    path == "/mcp"
        || path.starts_with("/mcp/")
        || path.starts_with("/api/")
        || path == "/auth/linuxdo"
        || path.starts_with("/auth/linuxdo/")
}

#[cfg(test)]
mod db_maintenance_gate_tests {
    use super::db_maintenance_gated_path;

    #[test]
    fn maintenance_gate_only_covers_db_backed_routes() {
        assert!(db_maintenance_gated_path("/api/jobs"));
        assert!(db_maintenance_gated_path("/mcp"));
        assert!(db_maintenance_gated_path("/auth/linuxdo/callback"));

        assert!(!db_maintenance_gated_path("/health"));
        assert!(!db_maintenance_gated_path("/admin"));
        assert!(!db_maintenance_gated_path("/assets/admin.js"));
        assert!(!db_maintenance_gated_path("/favicon.svg"));
    }
}

async fn ensure_ha_allows_basic_business(
    state: &Arc<AppState>,
    path: &str,
) -> Result<(), Response<Body>> {
    let status = state.ha.status().await;
    if status.allows_basic_business {
        return Ok(());
    }

    let payload = json!({
        "error": "ha_role_not_serving",
        "message": format!(
            "HA role {} does not serve external business traffic",
            status.role.as_str()
        ),
        "role": status.role,
        "path": path,
    });
    let response = Response::builder()
        .status(StatusCode::SERVICE_UNAVAILABLE)
        .header(CONTENT_TYPE, "application/json; charset=utf-8")
        .body(Body::from(payload.to_string()))
        .unwrap_or_else(|_| Response::builder().status(503).body(Body::empty()).unwrap());
    Err(response)
}

async fn ensure_ha_allows_basic_business_status(
    state: &Arc<AppState>,
    path: &str,
) -> Result<(), (StatusCode, String)> {
    let status = state.ha.status().await;
    if status.allows_basic_business {
        return Ok(());
    }

    Err((
        StatusCode::SERVICE_UNAVAILABLE,
        format!(
            "HA role {} does not serve external business traffic at {path}",
            status.role.as_str()
        ),
    ))
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
pub struct LinuxDoCreditOptions {
    pub enabled: bool,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub merchant_private_key: Option<String>,
    pub submit_url: String,
    pub notify_url: Option<String>,
    pub return_url: Option<String>,
    pub test_price_enabled: bool,
}

impl LinuxDoCreditOptions {
    #[cfg(test)]
    fn disabled() -> Self {
        Self {
            enabled: false,
            client_id: None,
            client_secret: None,
            merchant_private_key: None,
            submit_url: "https://credit.linux.do/epay/pay/submit.php".to_string(),
            notify_url: None,
            return_url: None,
            test_price_enabled: false,
        }
    }

    fn is_enabled_and_configured(&self) -> bool {
        self.enabled
            && self
                .client_id
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
            && self
                .client_secret
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
            && self
                .merchant_private_key
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
    }

    pub(crate) fn price_config(&self) -> tavily_hikari::LinuxDoCreditRechargePriceConfig {
        if self.test_price_enabled {
            tavily_hikari::LinuxDoCreditRechargePriceConfig::test_price()
        } else {
            tavily_hikari::LinuxDoCreditRechargePriceConfig::normal()
        }
    }
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
    pub refresh_token_crypt_key: Option<[u8; 32]>,
    pub user_sync_enabled: bool,
    pub user_sync_at: (u32, u32),
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
            refresh_token_crypt_key: None,
            user_sync_enabled: true,
            user_sync_at: (6, 20),
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

    fn has_refresh_token_crypt_key(&self) -> bool {
        self.refresh_token_crypt_key.is_some()
    }

    fn refresh_token_crypt_key(&self) -> Option<&[u8; 32]> {
        self.refresh_token_crypt_key.as_ref()
    }

    fn is_user_sync_scheduler_enabled(&self) -> bool {
        self.user_sync_enabled && self.is_enabled_and_configured()
    }

    fn user_sync_time(&self) -> (u32, u32) {
        self.user_sync_at
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
const DEV_OPEN_ADMIN_REQUEST_TOKEN: &str = "th-dev-override";
const DEV_OPEN_ADMIN_REQUEST_TOKEN_ID: &str = "dev";

#[derive(Clone, Debug)]
struct BuiltinAdminSession {
    issued_at: tokio::time::Instant,
    expires_at: tokio::time::Instant,
}

#[derive(Clone, Debug)]
struct BuiltinAdminAuth {
    enabled: bool,
    password: Option<String>,
    password_hash: Option<String>,
    sessions: Arc<std::sync::RwLock<HashMap<String, BuiltinAdminSession>>>,
    backend_time: tavily_hikari::BackendTime,
}

impl BuiltinAdminAuth {
    fn new(enabled: bool, password: Option<String>, password_hash: Option<String>) -> Self {
        Self::new_with_time(
            enabled,
            password,
            password_hash,
            tavily_hikari::BackendTime::system(),
        )
    }

    fn new_with_time(
        enabled: bool,
        password: Option<String>,
        password_hash: Option<String>,
        backend_time: tavily_hikari::BackendTime,
    ) -> Self {
        Self {
            enabled,
            password,
            password_hash,
            sessions: Arc::new(std::sync::RwLock::new(HashMap::new())),
            backend_time,
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
        let now = self.backend_time.instant_now();
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
        let now = self.backend_time.instant_now();
        let expires_at =
            now + std::time::Duration::from_secs(BUILTIN_ADMIN_SESSION_MAX_AGE_SECS);
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
                let mut issued: Vec<(String, tokio::time::Instant)> = sessions
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

async fn require_full_master_write(state: &AppState) -> Result<(), (StatusCode, String)> {
    if let Some(reason) = state.ha.block_full_write_reason().await {
        return Err((StatusCode::SERVICE_UNAVAILABLE, reason));
    }
    Ok(())
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

async fn admin_maintenance_actor(
    state: &AppState,
    headers: &HeaderMap,
    auth_token_id: Option<&str>,
) -> tavily_hikari::MaintenanceActor {
    let mut actor = tavily_hikari::MaintenanceActor {
        auth_token_id: auth_token_id.map(str::to_string),
        actor_user_id: None,
        actor_display_name: None,
    };

    if let Some(session) = resolve_user_session(state, headers).await {
        actor.actor_user_id = Some(session.user.user_id);
        actor.actor_display_name = session
            .user
            .display_name
            .or(session.user.username)
            .or(Some(session.user.provider));
        return actor;
    }

    if state.dev_open_admin {
        actor.actor_display_name = Some("dev-open-admin".to_string());
        return actor;
    }

    if state.forward_auth_enabled {
        actor.actor_display_name = state
            .forward_auth
            .nickname_value(headers)
            .or_else(|| state.forward_auth.user_value(headers).map(str::to_string))
            .or_else(|| state.forward_auth.admin_override_name().map(str::to_string));
        return actor;
    }

    if state.builtin_admin.is_admin(headers) {
        actor.actor_display_name = Some("builtin-admin".to_string());
    }

    actor
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum UiLanguage {
    En,
    Zh,
}

fn ui_language_from_headers(headers: &HeaderMap) -> UiLanguage {
    headers
        .get(axum::http::header::ACCEPT_LANGUAGE)
        .and_then(|value| value.to_str().ok())
        .map(|raw| {
            raw.split(',')
                .map(|segment| segment.trim().to_ascii_lowercase())
                .find(|segment| !segment.is_empty())
                .filter(|segment| segment.starts_with("zh"))
                .map(|_| UiLanguage::Zh)
                .unwrap_or(UiLanguage::En)
        })
        .unwrap_or(UiLanguage::En)
}

fn append_solution_guidance_to_error(
    error_message: Option<String>,
    failure_kind: Option<&str>,
    language: UiLanguage,
) -> Option<String> {
    let guidance = failure_kind
        .filter(|kind| tavily_hikari::should_append_solution_guidance(kind))
        .and_then(|kind| {
            tavily_hikari::failure_kind_solution_guidance(kind, matches!(language, UiLanguage::Zh))
        })?;
    match error_message {
        Some(message) if !message.trim().is_empty() => Some(format!("{message}\n\n{guidance}")),
        _ => Some(guidance.to_string()),
    }
}

fn parse_iso_timestamp(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc).timestamp())
        .ok()
}

fn default_since_at(now: DateTime<Utc>, period: Option<&str>) -> i64 {
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

fn default_until_at(now: DateTime<Utc>, period: Option<&str>, since: i64) -> i64 {
    let base = DateTime::<Utc>::from_timestamp(since, 0).unwrap_or(now);
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

#[derive(Debug, Clone)]
struct RequestTokenResolution {
    token: String,
    auth_token_id: Option<String>,
    using_dev_open_admin_fallback: bool,
}

fn auth_token_id_from_secret(token: &str) -> Option<String> {
    token
        .strip_prefix("th-")
        .and_then(|rest| rest.split_once('-').map(|(id, _)| id.to_string()))
}

fn resolve_request_token(
    dev_open_admin: bool,
    candidates: Vec<Option<String>>,
) -> Option<RequestTokenResolution> {
    if let Some(token) = candidates.into_iter().flatten().find(|token| !token.is_empty()) {
        return Some(RequestTokenResolution {
            auth_token_id: auth_token_id_from_secret(&token),
            token,
            using_dev_open_admin_fallback: false,
        });
    }

    if dev_open_admin {
        return Some(RequestTokenResolution {
            token: DEV_OPEN_ADMIN_REQUEST_TOKEN.to_string(),
            auth_token_id: Some(DEV_OPEN_ADMIN_REQUEST_TOKEN_ID.to_string()),
            using_dev_open_admin_fallback: true,
        });
    }

    None
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

async fn health_check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if state.proxy.is_forward_proxy_xray_ready().await {
        (StatusCode::OK, "ok")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "xray not ready")
    }
}
