use crate::analysis::*;
use crate::backend_time::BackendTime;
use crate::models::*;
use crate::store::*;
use crate::*;
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::collections::VecDeque;
use std::sync::{
    OnceLock,
    atomic::{AtomicI64, Ordering},
};

#[derive(Clone, Debug)]
struct TokenQuota {
    store: Arc<KeyStore>,
    cleanup: Arc<Mutex<CleanupState>>,
    hourly_limit: i64,
    daily_limit: i64,
    monthly_limit: i64,
    backend_time: BackendTime,
}

/// Lightweight per-token hourly request limiter that counts *all* authenticated
/// requests, regardless of MCP method or HTTP endpoint.
#[derive(Clone, Debug)]
struct TokenRequestLimit {
    store: Arc<KeyStore>,
    backend: RequestRateLimitBackend,
    request_limit: Arc<AtomicI64>,
    window_minutes: i64,
    window_secs: i64,
    backend_time: BackendTime,
}

#[derive(Clone, Debug)]
struct UserBusinessCalls1hWindow {
    store: Arc<KeyStore>,
    backend: UserBusinessCalls1hBackend,
    window_minutes: i64,
    rolling_window_secs: i64,
    retention_secs: i64,
    backend_time: BackendTime,
}

#[derive(Clone, Debug)]
enum RequestRateLimitBackend {
    Memory(Arc<MemoryRequestRateLimitBackend>),
}

#[derive(Clone, Debug)]
enum UserBusinessCalls1hBackend {
    Memory(Arc<MemoryUserBusinessCalls1hBackend>),
}

#[derive(Clone, Debug, Default)]
struct MemoryRequestRateLimitBackend {
    state: Arc<Mutex<MemoryRequestRateLimitState>>,
}

#[derive(Clone, Debug, Default)]
struct MemoryUserBusinessCalls1hBackend {
    state: Arc<Mutex<MemoryUserBusinessCalls1hState>>,
}

#[derive(Clone, Debug, Default)]
struct MemoryRequestRateLimitState {
    entries: HashMap<String, VecDeque<i64>>,
    next_gc_at: i64,
}

#[derive(Clone, Debug, Default)]
struct MemoryUserBusinessCalls1hState {
    entries: HashMap<String, VecDeque<UserBusinessCallEvent>>,
    next_gc_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RequestRateSubject {
    key: String,
    scope: RequestRateScope,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct UserBusinessCallEvent {
    created_at: i64,
    outcome: UserBusinessCallOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UserBusinessCallOutcome {
    Success,
    Failure,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct UserBusinessCallCounts {
    success_count: i64,
    failure_count: i64,
}

#[derive(Clone, Debug)]
struct UserBusinessCalls1hBackfillRow {
    user_id: String,
    created_at: i64,
    outcome: UserBusinessCallOutcome,
}

impl RequestRateSubject {
    fn user(user_id: &str) -> Self {
        Self {
            key: format!("user:{user_id}"),
            scope: RequestRateScope::User,
        }
    }

    fn token(token_id: &str) -> Self {
        Self {
            key: format!("token:{token_id}"),
            scope: RequestRateScope::Token,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct CachedForwardProxyAffinityRecord {
    record: forward_proxy::ForwardProxyAffinityRecord,
    has_persisted_row: bool,
}

#[derive(Clone, Debug)]
struct LoadedProxyAffinityState {
    record: forward_proxy::ForwardProxyAffinityRecord,
    registration_ip: Option<String>,
    registration_region: Option<String>,
    has_explicit_empty_marker: bool,
}

#[derive(Clone, Debug)]
struct CachedSummaryWindows {
    generated_at: Instant,
    value: SummaryWindows,
}

#[derive(Clone, Debug)]
struct CachedDashboardHourlyRequestWindow {
    generated_at: Instant,
    value: DashboardHourlyRequestWindow,
}

#[derive(Clone, Debug)]
struct SummaryWindowsCacheState {
    cached: Option<CachedSummaryWindows>,
    loading: bool,
    notify: Arc<tokio::sync::Notify>,
}

impl Default for SummaryWindowsCacheState {
    fn default() -> Self {
        Self {
            cached: None,
            loading: false,
            notify: Arc::new(tokio::sync::Notify::new()),
        }
    }
}

#[derive(Clone, Debug)]
struct DashboardHourlyRequestWindowCacheState {
    cached: Option<CachedDashboardHourlyRequestWindow>,
    loading: bool,
    notify: Arc<tokio::sync::Notify>,
}

type SharedTokenBillingLockMap = Arc<Mutex<HashMap<String, Weak<Mutex<()>>>>>;

fn shared_token_billing_locks() -> SharedTokenBillingLockMap {
    static LOCKS: OnceLock<SharedTokenBillingLockMap> = OnceLock::new();
    LOCKS
        .get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
        .clone()
}

impl Default for DashboardHourlyRequestWindowCacheState {
    fn default() -> Self {
        Self {
            cached: None,
            loading: false,
            notify: Arc::new(tokio::sync::Notify::new()),
        }
    }
}

#[derive(Clone, Debug)]
struct PendingHaNodeState {
    node_id: String,
    role: HaNodeRole,
    edgeone_origin: Option<String>,
    source_settings: Option<HaSourceSettingsView>,
    message: Option<String>,
}

#[derive(Clone, Debug)]
struct PendingHaSyncWatermark {
    source_node_id: Option<String>,
    target_node_id: Option<String>,
    watermark: i64,
    detail: Option<String>,
}

#[derive(Debug, Default)]
struct HaStateCoalescerState {
    pending_node_state: Option<PendingHaNodeState>,
    pending_sync_watermarks: HashMap<String, PendingHaSyncWatermark>,
    flush_deadline: Option<Instant>,
    flushing: bool,
    shutdown: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct HaStateCoalescer {
    state: Arc<Mutex<HaStateCoalescerState>>,
    wake: Arc<Notify>,
    flushed: Arc<Notify>,
}

impl Default for HaStateCoalescer {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(HaStateCoalescerState::default())),
            wake: Arc::new(Notify::new()),
            flushed: Arc::new(Notify::new()),
        }
    }
}

impl HaStateCoalescer {
    const MAX_PENDING_KEYS: usize = 100;
    const FLUSH_INTERVAL: Duration = Duration::from_secs(1);

    fn pending_key_count(state: &HaStateCoalescerState) -> usize {
        usize::from(state.pending_node_state.is_some()) + state.pending_sync_watermarks.len()
    }

    fn mark_flush_deadline_if_pending(state: &mut HaStateCoalescerState) {
        if Self::pending_key_count(state) > 0 && state.flush_deadline.is_none() {
            state.flush_deadline = Some(Instant::now() + Self::FLUSH_INTERVAL);
        }
    }

    async fn enqueue_node_state(
        &self,
        node_id: &str,
        role: HaNodeRole,
        edgeone_origin: Option<&str>,
        source_settings: Option<&HaSourceSettingsView>,
        message: Option<&str>,
    ) {
        {
            let mut state = self.state.lock().await;
            state.pending_node_state = Some(PendingHaNodeState {
                node_id: node_id.to_string(),
                role,
                edgeone_origin: edgeone_origin.map(str::to_string),
                source_settings: source_settings.cloned(),
                message: message.map(str::to_string),
            });
            Self::mark_flush_deadline_if_pending(&mut state);
        }
        self.wake.notify_one();
    }

    async fn enqueue_sync_watermark(
        &self,
        name: &str,
        source_node_id: Option<&str>,
        target_node_id: Option<&str>,
        watermark: i64,
        detail: Option<&str>,
    ) {
        {
            let mut state = self.state.lock().await;
            state.pending_sync_watermarks.insert(
                name.to_string(),
                PendingHaSyncWatermark {
                    source_node_id: source_node_id.map(str::to_string),
                    target_node_id: target_node_id.map(str::to_string),
                    watermark,
                    detail: detail.map(str::to_string),
                },
            );
            Self::mark_flush_deadline_if_pending(&mut state);
        }
        self.wake.notify_one();
    }

    async fn pending_sync_watermark(&self, name: &str) -> Option<PendingHaSyncWatermark> {
        self.state
            .lock()
            .await
            .pending_sync_watermarks
            .get(name)
            .cloned()
    }

    async fn wait_until_flushed(&self) {
        loop {
            let notified = {
                let state = self.state.lock().await;
                if !state.flushing
                    && state.pending_node_state.is_none()
                    && state.pending_sync_watermarks.is_empty()
                {
                    return;
                }
                self.flushed.notified()
            };
            notified.await;
        }
    }
}

struct SummaryWindowsLoadGuard {
    state: Arc<Mutex<SummaryWindowsCacheState>>,
    armed: bool,
}

impl SummaryWindowsLoadGuard {
    fn new(state: Arc<Mutex<SummaryWindowsCacheState>>) -> Self {
        Self { state, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for SummaryWindowsLoadGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        let state = self.state.clone();
        tokio::spawn(async move {
            let mut cache = state.lock().await;
            if cache.loading {
                cache.loading = false;
                cache.notify.notify_waiters();
            }
        });
    }
}

struct DashboardHourlyRequestWindowLoadGuard {
    state: Arc<Mutex<DashboardHourlyRequestWindowCacheState>>,
    armed: bool,
}

impl DashboardHourlyRequestWindowLoadGuard {
    fn new(state: Arc<Mutex<DashboardHourlyRequestWindowCacheState>>) -> Self {
        Self { state, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for DashboardHourlyRequestWindowLoadGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        let state = self.state.clone();
        tokio::spawn(async move {
            let mut cache = state.lock().await;
            if cache.loading {
                cache.loading = false;
                cache.notify.notify_waiters();
            }
        });
    }
}

/// 负责均衡 Tavily API key 并透传请求的代理。
#[derive(Clone, Debug)]
pub struct TavilyProxy {
    pub(crate) client: Client,
    pub(crate) forward_proxy_clients: forward_proxy::ForwardProxyClientPool,
    pub(crate) forward_proxy: Arc<Mutex<forward_proxy::ForwardProxyManager>>,
    forward_proxy_affinity: Arc<Mutex<HashMap<String, CachedForwardProxyAffinityRecord>>>,
    pub(crate) forward_proxy_trace_url: Url,
    #[cfg(test)]
    pub(crate) forward_proxy_trace_overrides: Arc<Mutex<HashMap<String, (String, String)>>>,
    pub(crate) xray_supervisor: Arc<Mutex<forward_proxy::XraySupervisor>>,
    pub(crate) upstream: Url,
    pub(crate) key_store: Arc<KeyStore>,
    pub(crate) upstream_origin: String,
    pub(crate) api_key_geo_origin: String,
    token_quota: TokenQuota,
    token_request_limit: TokenRequestLimit,
    user_business_calls_1h_window: UserBusinessCalls1hWindow,
    pub(crate) research_request_affinity: Arc<Mutex<TokenAffinityState>>,
    pub(crate) research_request_owner_affinity: Arc<Mutex<TokenAffinityState>>,
    summary_windows_cache: Arc<Mutex<SummaryWindowsCacheState>>,
    dashboard_hourly_request_window_cache: Arc<Mutex<DashboardHourlyRequestWindowCacheState>>,
    pub(crate) ha_state_coalescer: HaStateCoalescer,
    // Fast in-process lock to collapse duplicate work within one instance.
    pub(crate) token_billing_locks: Arc<Mutex<HashMap<String, Weak<Mutex<()>>>>>,
    pub(crate) mcp_session_init_locks: Arc<Mutex<HashMap<String, Weak<Mutex<()>>>>>,
    pub(crate) mcp_session_request_locks: Arc<Mutex<HashMap<String, Weak<Mutex<()>>>>>,
    pub(crate) low_quota_depletion_threshold: i64,
    health_readiness_grace_until: tokio::time::Instant,
    pub(crate) backend_time: BackendTime,
}

#[derive(Clone, Debug)]
pub struct TavilyProxyOptions {
    pub xray_binary: String,
    pub xray_runtime_dir: std::path::PathBuf,
    pub forward_proxy_trace_url: Url,
    pub low_quota_depletion_threshold: i64,
    pub health_readiness_grace_period: Duration,
}

impl TavilyProxyOptions {
    pub fn from_database_path(database_path: &str) -> Self {
        let layout = SqliteDatabaseLayout::from_database_path(database_path);
        Self {
            xray_binary: forward_proxy::default_xray_binary(),
            xray_runtime_dir: forward_proxy::default_xray_runtime_dir(&layout.core_database_path),
            forward_proxy_trace_url: default_forward_proxy_trace_url(),
            low_quota_depletion_threshold: low_quota_depletion_threshold_from_env(),
            health_readiness_grace_period: Duration::from_secs(90),
        }
    }
}

#[derive(Debug)]
struct QuotaSubjectLockGuard {
    store: Arc<KeyStore>,
    subject: String,
}

impl QuotaSubjectLockGuard {
    pub(crate) fn new(store: Arc<KeyStore>, subject: String) -> Self {
        Self { store, subject }
    }

    pub(crate) fn ensure_live(&self) -> Result<(), ProxyError> {
        let mut forced = self
            .store
            .forced_quota_subject_lock_loss_subjects
            .lock()
            .expect("forced quota subject lock loss mutex poisoned");
        if forced.remove(&self.subject) {
            return Err(ProxyError::Other(format!(
                "quota subject lock lost for {}",
                self.subject,
            )));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct TokenBillingGuard {
    billing_subject: String,
    _local: tokio::sync::OwnedMutexGuard<()>,
    _subject_lock: QuotaSubjectLockGuard,
}

impl TokenBillingGuard {
    pub fn billing_subject(&self) -> &str {
        &self.billing_subject
    }

    pub fn ensure_live(&self) -> Result<(), ProxyError> {
        self._subject_lock.ensure_live()
    }
}

#[derive(Debug)]
pub struct McpSessionInitGuard {
    _local: tokio::sync::OwnedMutexGuard<()>,
    _subject_lock: QuotaSubjectLockGuard,
}

impl McpSessionInitGuard {
    pub fn ensure_live(&self) -> Result<(), ProxyError> {
        self._subject_lock.ensure_live()
    }
}

#[derive(Debug)]
pub struct McpSessionRequestGuard {
    _local: tokio::sync::OwnedMutexGuard<()>,
    _subject_lock: QuotaSubjectLockGuard,
}

impl McpSessionRequestGuard {
    pub fn ensure_live(&self) -> Result<(), ProxyError> {
        self._subject_lock.ensure_live()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingBillingSettleOutcome {
    Charged,
    AlreadySettled,
    RetryLater,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiKeyUpsertStatus {
    Created,
    Undeleted,
    Existed,
}

impl ApiKeyUpsertStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Undeleted => "undeleted",
            Self::Existed => "existed",
        }
    }
}

pub(crate) const FORWARD_PROXY_PROGRESS_OPERATION_SAVE: &str = "save";
pub(crate) const FORWARD_PROXY_PROGRESS_OPERATION_VALIDATE: &str = "validate";
pub(crate) const FORWARD_PROXY_PROGRESS_OPERATION_REVALIDATE: &str = "revalidate";

pub(crate) const FORWARD_PROXY_PHASE_SAVE_SETTINGS: &str = "save_settings";
pub(crate) const FORWARD_PROXY_PHASE_VALIDATE_EGRESS_SOCKS5: &str = "validate_egress_socks5";
pub(crate) const FORWARD_PROXY_PHASE_APPLY_EGRESS_SOCKS5: &str = "apply_egress_socks5";
pub(crate) const FORWARD_PROXY_PHASE_REFRESH_SUBSCRIPTION: &str = "refresh_subscription";
pub(crate) const FORWARD_PROXY_PHASE_BOOTSTRAP_PROBE: &str = "bootstrap_probe";
pub(crate) const FORWARD_PROXY_PHASE_NORMALIZE_INPUT: &str = "normalize_input";
pub(crate) const FORWARD_PROXY_PHASE_PARSE_INPUT: &str = "parse_input";
pub(crate) const FORWARD_PROXY_PHASE_FETCH_SUBSCRIPTION: &str = "fetch_subscription";
pub(crate) const FORWARD_PROXY_PHASE_PROBE_NODES: &str = "probe_nodes";
pub(crate) const FORWARD_PROXY_PHASE_GENERATE_RESULT: &str = "generate_result";

pub(crate) const FORWARD_PROXY_LABEL_SAVE_SETTINGS: &str = "Saving forward proxy settings";
pub(crate) const FORWARD_PROXY_LABEL_VALIDATE_EGRESS_SOCKS5: &str =
    "Validating global SOCKS5 relay";
pub(crate) const FORWARD_PROXY_LABEL_APPLY_EGRESS_SOCKS5: &str = "Applying global SOCKS5 relay";
pub(crate) const FORWARD_PROXY_LABEL_REFRESH_SUBSCRIPTION: &str = "Refreshing subscription nodes";
pub(crate) const FORWARD_PROXY_LABEL_BOOTSTRAP_PROBE: &str = "Running bootstrap probes";
pub(crate) const FORWARD_PROXY_LABEL_NORMALIZE_INPUT: &str = "Normalizing input";
pub(crate) const FORWARD_PROXY_LABEL_PARSE_INPUT: &str = "Parsing input";
pub(crate) const FORWARD_PROXY_LABEL_FETCH_SUBSCRIPTION: &str = "Fetching subscription";
pub(crate) const FORWARD_PROXY_LABEL_PROBE_NODES: &str = "Probing nodes";
pub(crate) const FORWARD_PROXY_LABEL_GENERATE_RESULT: &str = "Preparing result";
pub(crate) const FORWARD_PROXY_TRACE_URL: &str = "http://cloudflare.com/cdn-cgi/trace";
pub(crate) const FORWARD_PROXY_TRACE_TIMEOUT_MS: u64 = 900;
pub(crate) const FORWARD_PROXY_GEO_NEGATIVE_RETRY_COOLDOWN_SECS: i64 = 15 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct McpSessionInitCandidate {
    pub(crate) key_id: String,
    pub(crate) stable_rank_index: usize,
    pub(crate) cooldown_until: Option<i64>,
    pub(crate) recent_rate_limited_count: i64,
    pub(crate) recent_billable_request_count: i64,
    pub(crate) active_session_count: i64,
    pub(crate) last_used_at: i64,
}

#[derive(Debug)]
pub(crate) struct McpSessionInitSelection {
    pub(crate) lease: ApiKeyLease,
    pub(crate) key_effect: KeyEffect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HttpProjectAffinityCandidate {
    pub(crate) key_id: String,
    pub(crate) stable_rank_index: usize,
    pub(crate) cooldown_until: Option<i64>,
    pub(crate) recent_rate_limited_count: i64,
    pub(crate) recent_billable_request_count: i64,
    pub(crate) last_used_at: i64,
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct HttpProjectAffinitySelection {
    pub(crate) lease: ApiKeyLease,
    pub(crate) binding_effect: KeyEffect,
    pub(crate) selection_effect: KeyEffect,
}

#[derive(Debug)]
pub(crate) struct ApiRouteAffinitySelection {
    pub(crate) lease: ApiKeyLease,
    pub(crate) binding_effect: KeyEffect,
    pub(crate) selection_effect: KeyEffect,
}

fn default_forward_proxy_trace_url() -> Url {
    std::env::var("FORWARD_PROXY_TRACE_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .and_then(|value| Url::parse(&value).ok())
        .unwrap_or_else(|| Url::parse(FORWARD_PROXY_TRACE_URL).expect("valid trace url"))
}

pub fn parse_low_quota_depletion_threshold(raw: Option<&str>, source: &str) -> i64 {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return LOW_QUOTA_DEPLETION_THRESHOLD_DEFAULT;
    };
    match raw.parse::<i64>() {
        Ok(value) if value >= 0 => value,
        _ => {
            eprintln!(
                "warning: {source} must be a non-negative integer; using default {LOW_QUOTA_DEPLETION_THRESHOLD_DEFAULT}"
            );
            LOW_QUOTA_DEPLETION_THRESHOLD_DEFAULT
        }
    }
}

fn low_quota_depletion_threshold_from_env() -> i64 {
    parse_low_quota_depletion_threshold(
        std::env::var("LOW_QUOTA_DEPLETION_THRESHOLD")
            .ok()
            .as_deref(),
        "LOW_QUOTA_DEPLETION_THRESHOLD",
    )
}

include!("proxy_core.rs");
include!("proxy_affinity.rs");
include!("proxy_http_and_logs.rs");
include!("proxy_auth_and_oauth.rs");
include!("proxy_usage_and_metrics.rs");
include!("proxy_request_limits.rs");
include!("proxy_alerts.rs");
include!("proxy_announcements.rs");
include!("proxy_admin_user_usage_series.rs");
include!("proxy_user_dashboard_overview.rs");
include!("proxy_quota_sync_and_jobs.rs");
include!("proxy_forward_proxy_maintenance.rs");
include!("proxy_ha.rs");

impl TokenQuota {
    pub(crate) fn new(store: Arc<KeyStore>, backend_time: BackendTime) -> Self {
        Self {
            store,
            cleanup: Arc::new(Mutex::new(CleanupState::default())),
            hourly_limit: effective_token_hourly_limit(),
            daily_limit: effective_token_daily_limit(),
            monthly_limit: effective_token_monthly_limit(),
            backend_time,
        }
    }

    pub(crate) async fn resolve_subject(&self, token_id: &str) -> Result<QuotaSubject, ProxyError> {
        if let Some(user_id) = self.store.find_user_id_by_token_fresh(token_id).await? {
            Ok(QuotaSubject::Account(user_id))
        } else {
            Ok(QuotaSubject::Token(token_id.to_string()))
        }
    }

    async fn current_token_daily_used(
        &self,
        token_id: &str,
        day_start: i64,
        day_end: i64,
    ) -> Result<i64, ProxyError> {
        let current_day = self
            .store
            .sum_usage_buckets(token_id, GRANULARITY_DAY, day_start)
            .await?;
        let legacy_same_day = self
            .store
            .sum_usage_buckets_between(token_id, GRANULARITY_HOUR, day_start, day_end)
            .await?;
        Ok(current_day + legacy_same_day)
    }

    async fn current_account_daily_used(
        &self,
        user_id: &str,
        day_start: i64,
        day_end: i64,
    ) -> Result<i64, ProxyError> {
        let current_day = self
            .store
            .sum_account_usage_buckets(user_id, GRANULARITY_DAY, day_start)
            .await?;
        let legacy_same_day = self
            .store
            .sum_account_usage_buckets_between(user_id, GRANULARITY_HOUR, day_start, day_end)
            .await?;
        Ok(current_day + legacy_same_day)
    }

    pub(crate) async fn check(&self, token_id: &str) -> Result<TokenQuotaVerdict, ProxyError> {
        let now = self.backend_time.now_utc();
        let now_ts = now.timestamp();
        let minute_bucket = now_ts - (now_ts % SECS_PER_MINUTE);
        let local_now = now.with_timezone(&Local);
        let day_bucket = start_of_local_day_utc_ts(local_now);
        let day_bucket_end = next_local_day_start_utc_ts(day_bucket);

        let hour_window_start = minute_bucket - 59 * SECS_PER_MINUTE;
        let month_start = start_of_month(now).timestamp();

        let verdict = match self.resolve_subject(token_id).await? {
            QuotaSubject::Account(user_id) => {
                let resolution = self
                    .store
                    .resolve_account_quota_resolution(&user_id)
                    .await?;
                let limits = resolution.effective;
                if limits.hourly_limit <= 0 || limits.daily_limit <= 0 || limits.monthly_limit <= 0
                {
                    let hourly_used = self
                        .store
                        .sum_account_usage_buckets(&user_id, GRANULARITY_MINUTE, hour_window_start)
                        .await?;
                    let daily_used = self
                        .current_account_daily_used(&user_id, day_bucket, day_bucket_end)
                        .await?;
                    let monthly_used = self
                        .store
                        .fetch_account_monthly_count(&user_id, month_start)
                        .await?;
                    TokenQuotaVerdict::new(
                        hourly_used,
                        limits.hourly_limit,
                        daily_used,
                        limits.daily_limit,
                        monthly_used,
                        limits.monthly_limit,
                    )
                } else {
                    self.store
                        .increment_account_usage_bucket(&user_id, minute_bucket, GRANULARITY_MINUTE)
                        .await?;
                    self.store
                        .increment_account_usage_bucket(&user_id, day_bucket, GRANULARITY_DAY)
                        .await?;
                    let hourly_used = self
                        .store
                        .sum_account_usage_buckets(&user_id, GRANULARITY_MINUTE, hour_window_start)
                        .await?;
                    let daily_used = self
                        .current_account_daily_used(&user_id, day_bucket, day_bucket_end)
                        .await?;
                    let monthly_used = self
                        .store
                        .increment_account_monthly_quota(&user_id, month_start)
                        .await?;
                    TokenQuotaVerdict::new(
                        hourly_used,
                        limits.hourly_limit,
                        daily_used,
                        limits.daily_limit,
                        monthly_used,
                        limits.monthly_limit,
                    )
                }
            }
            QuotaSubject::Token(token_id) => {
                // Increment usage buckets and monthly quota as an approximate, cheap counter
                // for *business* quota decisions. This path is allowed to drift slightly
                // from the detailed logs in exchange for lower per-request overhead.
                self.store
                    .increment_usage_bucket(&token_id, minute_bucket, GRANULARITY_MINUTE)
                    .await?;
                self.store
                    .increment_usage_bucket(&token_id, day_bucket, GRANULARITY_DAY)
                    .await?;

                let hourly_used = self
                    .store
                    .sum_usage_buckets(&token_id, GRANULARITY_MINUTE, hour_window_start)
                    .await?;
                let daily_used = self
                    .current_token_daily_used(&token_id, day_bucket, day_bucket_end)
                    .await?;
                let monthly_used = self
                    .store
                    .increment_monthly_quota(&token_id, month_start)
                    .await?;

                TokenQuotaVerdict::new(
                    hourly_used,
                    self.hourly_limit,
                    daily_used,
                    self.daily_limit,
                    monthly_used,
                    self.monthly_limit,
                )
            }
        };

        self.maybe_cleanup(now_ts).await?;
        Ok(verdict)
    }

    pub(crate) async fn charge(&self, token_id: &str, credits: i64) -> Result<(), ProxyError> {
        if credits <= 0 {
            return Ok(());
        }

        let now = self.backend_time.now_utc();
        let now_ts = now.timestamp();
        let minute_bucket = now_ts - (now_ts % SECS_PER_MINUTE);
        let day_bucket = start_of_local_day_utc_ts(now.with_timezone(&Local));
        let month_start = start_of_month(now).timestamp();

        match self.resolve_subject(token_id).await? {
            QuotaSubject::Account(user_id) => {
                self.store
                    .increment_account_usage_bucket_by(
                        &user_id,
                        minute_bucket,
                        GRANULARITY_MINUTE,
                        credits,
                    )
                    .await?;
                self.store
                    .increment_account_usage_bucket_by(
                        &user_id,
                        day_bucket,
                        GRANULARITY_DAY,
                        credits,
                    )
                    .await?;
                let _ = self
                    .store
                    .increment_account_monthly_quota_by(&user_id, month_start, credits)
                    .await?;
            }
            QuotaSubject::Token(token_id) => {
                self.store
                    .increment_usage_bucket_by(
                        &token_id,
                        minute_bucket,
                        GRANULARITY_MINUTE,
                        credits,
                    )
                    .await?;
                self.store
                    .increment_usage_bucket_by(&token_id, day_bucket, GRANULARITY_DAY, credits)
                    .await?;
                let _ = self
                    .store
                    .increment_monthly_quota_by(&token_id, month_start, credits)
                    .await?;
            }
        }

        self.maybe_cleanup(now_ts).await?;
        Ok(())
    }

    pub(crate) async fn snapshot_for_token(
        &self,
        token_id: &str,
        now: chrono::DateTime<Utc>,
    ) -> Result<TokenQuotaVerdict, ProxyError> {
        let subject = self.resolve_subject(token_id).await?;
        self.snapshot_for_subject(&subject, now).await
    }

    pub(crate) async fn snapshot_for_billing_subject(
        &self,
        billing_subject: &str,
        now: chrono::DateTime<Utc>,
    ) -> Result<TokenQuotaVerdict, ProxyError> {
        let subject = QuotaSubject::from_billing_subject(billing_subject)?;
        self.snapshot_for_subject(&subject, now).await
    }

    pub(crate) async fn snapshot_for_subject(
        &self,
        subject: &QuotaSubject,
        now: chrono::DateTime<Utc>,
    ) -> Result<TokenQuotaVerdict, ProxyError> {
        let now_ts = now.timestamp();
        let minute_bucket = now_ts - (now_ts % SECS_PER_MINUTE);
        let local_now = now.with_timezone(&Local);
        let hour_window_start = minute_bucket - 59 * SECS_PER_MINUTE;
        let day_window_start = start_of_local_day_utc_ts(local_now);
        let day_window_end = next_local_day_start_utc_ts(day_window_start);
        let month_start = start_of_month(now).timestamp();
        match subject {
            QuotaSubject::Account(user_id) => {
                let limits = self
                    .store
                    .resolve_account_quota_resolution(user_id)
                    .await?
                    .effective;
                let hourly_used = self
                    .store
                    .sum_account_usage_buckets(user_id, GRANULARITY_MINUTE, hour_window_start)
                    .await?;
                let daily_used = self
                    .current_account_daily_used(user_id, day_window_start, day_window_end)
                    .await?;
                let monthly_used = self
                    .store
                    .fetch_account_monthly_count(user_id, month_start)
                    .await?;
                Ok(TokenQuotaVerdict::new(
                    hourly_used,
                    limits.hourly_limit,
                    daily_used,
                    limits.daily_limit,
                    monthly_used,
                    limits.monthly_limit,
                ))
            }
            QuotaSubject::Token(token_id) => {
                let hourly_used = self
                    .store
                    .sum_usage_buckets(token_id, GRANULARITY_MINUTE, hour_window_start)
                    .await?;
                let daily_used = self
                    .current_token_daily_used(token_id, day_window_start, day_window_end)
                    .await?;
                let monthly_used = self
                    .store
                    .fetch_monthly_count(token_id, month_start)
                    .await?;
                Ok(TokenQuotaVerdict::new(
                    hourly_used,
                    self.hourly_limit,
                    daily_used,
                    self.daily_limit,
                    monthly_used,
                    self.monthly_limit,
                ))
            }
        }
    }

    pub(crate) async fn snapshot_many(
        &self,
        token_ids: &[String],
    ) -> Result<HashMap<String, TokenQuotaVerdict>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let now = self.backend_time.now_utc();
        let now_ts = now.timestamp();
        let minute_bucket = now_ts - (now_ts % SECS_PER_MINUTE);
        let local_now = now.with_timezone(&Local);
        let hour_window_start = minute_bucket - 59 * SECS_PER_MINUTE;
        let day_window_start = start_of_local_day_utc_ts(local_now);
        let day_window_end = next_local_day_start_utc_ts(day_window_start);
        let month_start = start_of_month(now).timestamp();

        let token_bindings = self.store.list_user_bindings_for_tokens(token_ids).await?;
        let mut token_subjects: Vec<String> = Vec::new();
        let mut account_subjects: Vec<(String, String)> = Vec::new();
        let mut account_user_ids: Vec<String> = Vec::new();
        for token_id in token_ids {
            if let Some(user_id) = token_bindings.get(token_id) {
                account_subjects.push((token_id.clone(), user_id.clone()));
                account_user_ids.push(user_id.clone());
            } else {
                token_subjects.push(token_id.clone());
            }
        }
        account_user_ids.sort_unstable();
        account_user_ids.dedup();

        let token_hourly_totals = self
            .store
            .sum_usage_buckets_bulk(&token_subjects, GRANULARITY_MINUTE, hour_window_start)
            .await?;
        let token_daily_totals = self
            .store
            .sum_usage_buckets_bulk(&token_subjects, GRANULARITY_DAY, day_window_start)
            .await?;
        let token_legacy_daily_totals = self
            .store
            .sum_usage_buckets_bulk_between(
                &token_subjects,
                GRANULARITY_HOUR,
                day_window_start,
                day_window_end,
            )
            .await?;
        let token_monthly_totals = self
            .store
            .fetch_monthly_counts(&token_subjects, month_start)
            .await?;

        let mut verdicts = HashMap::new();
        for token_id in token_subjects {
            let hourly_used = token_hourly_totals.get(&token_id).copied().unwrap_or(0);
            let daily_used = token_daily_totals.get(&token_id).copied().unwrap_or(0)
                + token_legacy_daily_totals
                    .get(&token_id)
                    .copied()
                    .unwrap_or(0);
            let monthly_used = token_monthly_totals.get(&token_id).copied().unwrap_or(0);
            verdicts.insert(
                token_id,
                TokenQuotaVerdict::new(
                    hourly_used,
                    self.hourly_limit,
                    daily_used,
                    self.daily_limit,
                    monthly_used,
                    self.monthly_limit,
                ),
            );
        }
        if !account_user_ids.is_empty() {
            let account_limits = self
                .store
                .resolve_account_quota_limits_bulk(&account_user_ids)
                .await?;
            let account_hourly_totals = self
                .store
                .sum_account_usage_buckets_bulk(
                    &account_user_ids,
                    GRANULARITY_MINUTE,
                    hour_window_start,
                )
                .await?;
            let account_daily_totals = self
                .store
                .sum_account_usage_buckets_bulk(
                    &account_user_ids,
                    GRANULARITY_DAY,
                    day_window_start,
                )
                .await?;
            let account_legacy_daily_totals = self
                .store
                .sum_account_usage_buckets_bulk_between(
                    &account_user_ids,
                    GRANULARITY_HOUR,
                    day_window_start,
                    day_window_end,
                )
                .await?;
            let account_monthly_totals = self
                .store
                .fetch_account_monthly_counts(&account_user_ids, month_start)
                .await?;
            let default_limits = AccountQuotaLimits::zero_base();

            for (token_id, user_id) in account_subjects {
                let limits = account_limits
                    .get(&user_id)
                    .cloned()
                    .unwrap_or_else(|| default_limits.clone());
                let hourly_used = account_hourly_totals.get(&user_id).copied().unwrap_or(0);
                let daily_used = account_daily_totals.get(&user_id).copied().unwrap_or(0)
                    + account_legacy_daily_totals
                        .get(&user_id)
                        .copied()
                        .unwrap_or(0);
                let monthly_used = account_monthly_totals.get(&user_id).copied().unwrap_or(0);
                verdicts.insert(
                    token_id,
                    TokenQuotaVerdict::new(
                        hourly_used,
                        limits.hourly_limit,
                        daily_used,
                        limits.daily_limit,
                        monthly_used,
                        limits.monthly_limit,
                    ),
                );
            }
        }
        Ok(verdicts)
    }

    pub(crate) async fn maybe_cleanup(&self, now_ts: i64) -> Result<(), ProxyError> {
        let should_prune = {
            let mut guard = self.cleanup.lock().await;
            if now_ts - guard.last_pruned < CLEANUP_INTERVAL_SECS {
                false
            } else {
                guard.last_pruned = now_ts;
                true
            }
        };

        if should_prune {
            let threshold = now_ts - BUCKET_RETENTION_SECS;
            self.store
                .delete_old_usage_buckets(GRANULARITY_MINUTE, threshold)
                .await?;
            self.store
                .delete_old_usage_buckets(GRANULARITY_HOUR, threshold)
                .await?;
            self.store
                .delete_old_usage_buckets(GRANULARITY_DAY, threshold)
                .await?;
            self.store
                .delete_old_account_usage_buckets(GRANULARITY_MINUTE, threshold)
                .await?;
            self.store
                .delete_old_account_usage_buckets(GRANULARITY_HOUR, threshold)
                .await?;
            self.store
                .delete_old_account_usage_buckets(GRANULARITY_DAY, threshold)
                .await?;
            self.store
                .delete_old_account_usage_rollup_buckets(
                    AccountUsageRollupMetricKind::RequestCount,
                    AccountUsageRollupBucketKind::FiveMinute,
                    now_ts.saturating_sub(ACCOUNT_USAGE_ROLLUP_FIVE_MINUTE_RETENTION_SECS),
                )
                .await?;
            self.store
                .delete_old_account_usage_rollup_buckets(
                    AccountUsageRollupMetricKind::BusinessCredits,
                    AccountUsageRollupBucketKind::Hour,
                    now_ts.saturating_sub(ACCOUNT_USAGE_ROLLUP_HOUR_RETENTION_SECS),
                )
                .await?;
            self.store
                .delete_old_account_usage_rollup_buckets(
                    AccountUsageRollupMetricKind::BusinessCredits,
                    AccountUsageRollupBucketKind::Day,
                    now_ts.saturating_sub(ACCOUNT_USAGE_ROLLUP_DAY_RETENTION_SECS),
                )
                .await?;
            self.store
                .delete_old_account_usage_rollup_buckets(
                    AccountUsageRollupMetricKind::BusinessCredits,
                    AccountUsageRollupBucketKind::Month,
                    shift_month_start_utc_ts(
                        start_of_month(self.backend_time.now_utc()).timestamp(),
                        -ACCOUNT_USAGE_ROLLUP_MONTH_RETENTION_MONTHS,
                    ),
                )
                .await?;
        }

        Ok(())
    }
}

impl TokenRequestLimit {
    pub(crate) fn new(store: Arc<KeyStore>, backend_time: BackendTime) -> Self {
        Self {
            store,
            backend: RequestRateLimitBackend::Memory(Arc::new(
                MemoryRequestRateLimitBackend::default(),
            )),
            request_limit: Arc::new(AtomicI64::new(request_rate_limit())),
            window_minutes: request_rate_limit_window_minutes(),
            window_secs: request_rate_limit_window_secs(),
            backend_time,
        }
    }

    pub(crate) fn current_request_limit(&self) -> i64 {
        self.request_limit
            .load(Ordering::Relaxed)
            .max(REQUEST_RATE_LIMIT_MIN)
    }

    pub(crate) fn set_request_limit(&self, request_limit: i64) {
        self.request_limit
            .store(request_limit.max(REQUEST_RATE_LIMIT_MIN), Ordering::Relaxed);
    }

    pub(crate) async fn check(
        &self,
        token_id: &str,
    ) -> Result<TokenHourlyRequestVerdict, ProxyError> {
        let now_ts = self.backend_time.now_ts();
        let subject = self.resolve_subject_for_token(token_id).await?;
        let request_limit = self.current_request_limit();
        Ok(self
            .backend
            .check(
                &subject,
                now_ts,
                request_limit,
                self.window_minutes,
                self.window_secs,
            )
            .await)
    }

    /// Read-only snapshot of rolling request-rate usage for a set of tokens.
    /// This does NOT increment counters and is intended for dashboards / leaderboards.
    pub(crate) async fn snapshot_many(
        &self,
        token_ids: &[String],
    ) -> Result<HashMap<String, TokenHourlyRequestVerdict>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let now_ts = self.backend_time.now_ts();
        let request_limit = self.current_request_limit();
        let subjects_by_token = self.resolve_subjects_for_tokens(token_ids).await?;
        let mut unique_subjects: Vec<RequestRateSubject> =
            subjects_by_token.values().cloned().collect();
        unique_subjects.sort_by(|left, right| left.key.cmp(&right.key));
        unique_subjects.dedup_by(|left, right| left.key == right.key);
        let verdicts = self
            .backend
            .snapshot_many(
                &unique_subjects,
                now_ts,
                request_limit,
                self.window_minutes,
                self.window_secs,
            )
            .await;
        Ok(token_ids
            .iter()
            .filter_map(|token_id| {
                subjects_by_token
                    .get(token_id)
                    .and_then(|subject| verdicts.get(&subject.key).cloned())
                    .map(|verdict| (token_id.clone(), verdict))
            })
            .collect())
    }

    pub(crate) async fn snapshot_for_users(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, TokenHourlyRequestVerdict>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let now_ts = self.backend_time.now_ts();
        let request_limit = self.current_request_limit();
        let mut unique_subjects: Vec<RequestRateSubject> = user_ids
            .iter()
            .map(|user_id| RequestRateSubject::user(user_id))
            .collect();
        unique_subjects.sort_by(|left, right| left.key.cmp(&right.key));
        unique_subjects.dedup_by(|left, right| left.key == right.key);
        let verdicts = self
            .backend
            .snapshot_many(
                &unique_subjects,
                now_ts,
                request_limit,
                self.window_minutes,
                self.window_secs,
            )
            .await;
        Ok(user_ids
            .iter()
            .filter_map(|user_id| {
                let subject = RequestRateSubject::user(user_id);
                verdicts
                    .get(&subject.key)
                    .cloned()
                    .map(|verdict| (user_id.clone(), verdict))
            })
            .collect())
    }

    pub(crate) async fn recent_timestamps_for_user(&self, user_id: &str) -> Vec<i64> {
        let subject = RequestRateSubject::user(user_id);
        self.backend
            .recent_timestamps(&subject, self.backend_time.now_ts(), self.window_secs)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn debug_memory_subject_count(&self) -> usize {
        self.backend.debug_subject_count().await
    }

    #[cfg(test)]
    pub(crate) async fn debug_prune_idle_subjects_at(&self, now_ts: i64) {
        self.backend
            .debug_prune_idle_subjects(now_ts, self.window_secs)
            .await;
    }

    async fn resolve_subject_for_token(
        &self,
        token_id: &str,
    ) -> Result<RequestRateSubject, ProxyError> {
        Ok(
            if let Some(user_id) = self.store.find_user_id_by_token_fresh(token_id).await? {
                RequestRateSubject::user(&user_id)
            } else {
                RequestRateSubject::token(token_id)
            },
        )
    }

    async fn resolve_subjects_for_tokens(
        &self,
        token_ids: &[String],
    ) -> Result<HashMap<String, RequestRateSubject>, ProxyError> {
        let bindings = self.store.list_user_bindings_for_tokens(token_ids).await?;
        Ok(token_ids
            .iter()
            .map(|token_id| {
                let subject = bindings
                    .get(token_id)
                    .map(|user_id| RequestRateSubject::user(user_id))
                    .unwrap_or_else(|| RequestRateSubject::token(token_id));
                (token_id.clone(), subject)
            })
            .collect())
    }
}

impl UserBusinessCallCounts {
    fn total_count(&self) -> i64 {
        self.success_count + self.failure_count
    }

    fn record(&mut self, outcome: UserBusinessCallOutcome) {
        match outcome {
            UserBusinessCallOutcome::Success => self.success_count += 1,
            UserBusinessCallOutcome::Failure => self.failure_count += 1,
        }
    }
}

impl UserBusinessCalls1hWindow {
    const WINDOW_MINUTES: i64 = 60;
    const ROLLING_WINDOW_SECS: i64 = SECS_PER_HOUR;
    const RETENTION_SECS: i64 = 25 * SECS_PER_HOUR;

    pub(crate) fn new(store: Arc<KeyStore>, backend_time: BackendTime) -> Self {
        Self {
            store,
            backend: UserBusinessCalls1hBackend::Memory(Arc::new(
                MemoryUserBusinessCalls1hBackend::default(),
            )),
            window_minutes: Self::WINDOW_MINUTES,
            rolling_window_secs: Self::ROLLING_WINDOW_SECS,
            retention_secs: Self::RETENTION_SECS,
            backend_time,
        }
    }

    pub(crate) async fn backfill_recent(&self) -> Result<(), ProxyError> {
        let now_ts = self.backend_time.now_ts();
        let since_ts = now_ts.saturating_sub(self.retention_secs);
        let rows = sqlx::query(
            r#"
            SELECT request_user_id, created_at, result_status
            FROM request_logs INDEXED BY idx_request_logs_time
            WHERE created_at >= ?
              AND request_user_id IS NOT NULL
              AND counts_business_quota = 1
              AND result_status != ?
              AND (
                upstream_operation IS NOT NULL
                OR (
                  api_key_id IS NOT NULL
                  AND path IN (
                    '/api/tavily/search',
                    '/api/tavily/extract',
                    '/api/tavily/crawl',
                    '/api/tavily/map'
                )
                )
              )
            ORDER BY created_at ASC, id ASC
            "#,
        )
        .bind(since_ts)
        .bind(OUTCOME_QUOTA_EXHAUSTED)
        .fetch_all(&self.store.pool)
        .await?;
        let events: Vec<UserBusinessCalls1hBackfillRow> = rows
            .into_iter()
            .filter_map(|row| {
                let user_id = row.try_get::<String, _>("request_user_id").ok()?;
                let created_at = row.try_get::<i64, _>("created_at").ok()?;
                let result_status = row.try_get::<String, _>("result_status").ok()?;
                let outcome = if result_status == OUTCOME_SUCCESS {
                    UserBusinessCallOutcome::Success
                } else {
                    UserBusinessCallOutcome::Failure
                };
                Some(UserBusinessCalls1hBackfillRow {
                    user_id,
                    created_at,
                    outcome,
                })
            })
            .collect();
        self.backend
            .replace_from_backfill(&events, now_ts, self.retention_secs)
            .await;
        Ok(())
    }

    pub(crate) async fn record_event(
        &self,
        user_id: &str,
        created_at: i64,
        outcome: UserBusinessCallOutcome,
    ) {
        self.backend
            .record_event(
                user_id,
                UserBusinessCallEvent {
                    created_at,
                    outcome,
                },
                self.backend_time.now_ts(),
                self.retention_secs,
            )
            .await;
    }

    pub(crate) async fn snapshot_for_users(
        &self,
        user_ids: &[String],
    ) -> HashMap<String, BusinessCalls1hSummary> {
        if user_ids.is_empty() {
            return HashMap::new();
        }
        let now_ts = self.backend_time.now_ts();
        let counts = self
            .backend
            .snapshot_many(
                user_ids,
                now_ts,
                self.rolling_window_secs,
                self.retention_secs,
            )
            .await;
        user_ids
            .iter()
            .map(|user_id| {
                let counts = counts.get(user_id).cloned().unwrap_or_default();
                (
                    user_id.clone(),
                    BusinessCalls1hSummary {
                        success_count: counts.success_count,
                        failure_count: counts.failure_count,
                        total_count: counts.total_count(),
                        window_minutes: self.window_minutes,
                    },
                )
            })
            .collect()
    }

    pub(crate) async fn usage_series(&self, user_id: &str) -> Vec<AdminUserBusinessCalls1hPoint> {
        let now_ts = self.backend_time.now_ts();
        let current_bucket_start = now_ts - now_ts.rem_euclid(SECS_PER_FIVE_MINUTES);
        let start = current_bucket_start - 287 * SECS_PER_FIVE_MINUTES;
        let bucket_starts: Vec<i64> = (0..288)
            .map(|index| start + index * SECS_PER_FIVE_MINUTES)
            .collect();
        let events = self
            .backend
            .retained_events_for_user(user_id, now_ts, self.retention_secs)
            .await;
        if bucket_starts.is_empty() {
            return Vec::new();
        }

        let coverage_floor = now_ts.saturating_sub(self.retention_secs);
        let mut points = Vec::with_capacity(bucket_starts.len());
        let mut rolling = UserBusinessCallCounts::default();
        let mut rolling_start = 0usize;
        let mut event_cursor = 0usize;
        let mut bucket_start_cursor = 0usize;

        for (index, bucket_start) in bucket_starts.iter().copied().enumerate() {
            let bucket_end = bucket_start + SECS_PER_FIVE_MINUTES;
            let pressure_at = if index + 1 == bucket_starts.len() {
                now_ts
            } else {
                bucket_end
            };

            while event_cursor < events.len() && events[event_cursor].created_at < bucket_end {
                rolling.record(events[event_cursor].outcome);
                event_cursor += 1;
            }
            let rolling_cutoff = pressure_at - self.rolling_window_secs;
            while rolling_start < event_cursor && events[rolling_start].created_at <= rolling_cutoff
            {
                match events[rolling_start].outcome {
                    UserBusinessCallOutcome::Success => rolling.success_count -= 1,
                    UserBusinessCallOutcome::Failure => rolling.failure_count -= 1,
                }
                rolling_start += 1;
            }

            let mut bars = UserBusinessCallCounts::default();
            while bucket_start_cursor < event_cursor
                && events[bucket_start_cursor].created_at < bucket_start
            {
                bucket_start_cursor += 1;
            }
            let mut bucket_cursor = bucket_start_cursor;
            while bucket_cursor < event_cursor && events[bucket_cursor].created_at < bucket_end {
                bars.record(events[bucket_cursor].outcome);
                bucket_cursor += 1;
            }
            let has_coverage = bucket_start >= coverage_floor;
            points.push(AdminUserBusinessCalls1hPoint {
                bucket_start,
                display_bucket_start: None,
                bars: AdminUserBusinessCalls1hBarsPoint {
                    success: has_coverage.then_some(bars.success_count),
                    failure: has_coverage.then_some(bars.failure_count),
                },
                pressure: has_coverage.then_some(rolling.total_count()),
                limit_value: None,
            });
        }
        points
    }
}

impl RequestRateLimitBackend {
    async fn check(
        &self,
        subject: &RequestRateSubject,
        now_ts: i64,
        request_limit: i64,
        window_minutes: i64,
        window_secs: i64,
    ) -> TokenHourlyRequestVerdict {
        match self {
            Self::Memory(backend) => {
                backend
                    .check(subject, now_ts, request_limit, window_minutes, window_secs)
                    .await
            }
        }
    }

    async fn snapshot_many(
        &self,
        subjects: &[RequestRateSubject],
        now_ts: i64,
        request_limit: i64,
        window_minutes: i64,
        window_secs: i64,
    ) -> HashMap<String, TokenHourlyRequestVerdict> {
        match self {
            Self::Memory(backend) => {
                backend
                    .snapshot_many(subjects, now_ts, request_limit, window_minutes, window_secs)
                    .await
            }
        }
    }

    async fn recent_timestamps(
        &self,
        subject: &RequestRateSubject,
        now_ts: i64,
        window_secs: i64,
    ) -> Vec<i64> {
        match self {
            Self::Memory(backend) => {
                backend
                    .recent_timestamps(subject, now_ts, window_secs)
                    .await
            }
        }
    }

    #[cfg(test)]
    async fn debug_subject_count(&self) -> usize {
        match self {
            Self::Memory(backend) => backend.debug_subject_count().await,
        }
    }

    #[cfg(test)]
    async fn debug_prune_idle_subjects(&self, now_ts: i64, window_secs: i64) {
        match self {
            Self::Memory(backend) => backend.debug_prune_idle_subjects(now_ts, window_secs).await,
        }
    }
}

impl UserBusinessCalls1hBackend {
    async fn replace_from_backfill(
        &self,
        rows: &[UserBusinessCalls1hBackfillRow],
        now_ts: i64,
        retention_secs: i64,
    ) {
        match self {
            Self::Memory(backend) => {
                backend
                    .replace_from_backfill(rows, now_ts, retention_secs)
                    .await
            }
        }
    }

    async fn record_event(
        &self,
        user_id: &str,
        event: UserBusinessCallEvent,
        now_ts: i64,
        retention_secs: i64,
    ) {
        match self {
            Self::Memory(backend) => {
                backend
                    .record_event(user_id, event, now_ts, retention_secs)
                    .await
            }
        }
    }

    async fn snapshot_many(
        &self,
        user_ids: &[String],
        now_ts: i64,
        rolling_window_secs: i64,
        retention_secs: i64,
    ) -> HashMap<String, UserBusinessCallCounts> {
        match self {
            Self::Memory(backend) => {
                backend
                    .snapshot_many(user_ids, now_ts, rolling_window_secs, retention_secs)
                    .await
            }
        }
    }

    async fn retained_events_for_user(
        &self,
        user_id: &str,
        now_ts: i64,
        retention_secs: i64,
    ) -> Vec<UserBusinessCallEvent> {
        match self {
            Self::Memory(backend) => {
                backend
                    .retained_events_for_user(user_id, now_ts, retention_secs)
                    .await
            }
        }
    }
}

impl MemoryRequestRateLimitBackend {
    async fn check(
        &self,
        subject: &RequestRateSubject,
        now_ts: i64,
        request_limit: i64,
        window_minutes: i64,
        window_secs: i64,
    ) -> TokenHourlyRequestVerdict {
        if request_limit <= 0 {
            return TokenHourlyRequestVerdict::new(
                0,
                request_limit,
                window_minutes,
                subject.scope,
                window_secs.max(1),
            );
        }

        let mut state = self.state.lock().await;
        Self::maybe_gc(&mut state, now_ts, window_secs);
        let queue = state.entries.entry(subject.key.clone()).or_default();
        Self::prune_queue(queue, now_ts, window_secs);
        if (queue.len() as i64) >= request_limit {
            let retry_after_seconds = queue
                .front()
                .map(|oldest| (oldest + window_secs - now_ts).max(1))
                .unwrap_or(1);
            let used = queue.len() as i64;
            if queue.is_empty() {
                state.entries.remove(&subject.key);
            }
            return TokenHourlyRequestVerdict::new(
                used.saturating_add(1),
                request_limit,
                window_minutes,
                subject.scope,
                retry_after_seconds,
            );
        }

        queue.push_back(now_ts);
        let used = queue.len() as i64;
        TokenHourlyRequestVerdict::new(used, request_limit, window_minutes, subject.scope, 0)
    }

    async fn snapshot_many(
        &self,
        subjects: &[RequestRateSubject],
        now_ts: i64,
        request_limit: i64,
        window_minutes: i64,
        window_secs: i64,
    ) -> HashMap<String, TokenHourlyRequestVerdict> {
        let mut state = self.state.lock().await;
        Self::maybe_gc(&mut state, now_ts, window_secs);
        let mut verdicts = HashMap::new();
        let mut empty_keys = Vec::new();
        for subject in subjects {
            let (used, retry_after_seconds, should_remove) =
                if let Some(queue) = state.entries.get_mut(&subject.key) {
                    Self::prune_queue(queue, now_ts, window_secs);
                    let used = queue.len() as i64;
                    let retry_after_seconds = if used >= request_limit {
                        queue
                            .front()
                            .map(|oldest| (oldest + window_secs - now_ts).max(1))
                            .unwrap_or(0)
                    } else {
                        0
                    };
                    (used, retry_after_seconds, queue.is_empty())
                } else {
                    (0, 0, false)
                };
            if should_remove {
                empty_keys.push(subject.key.clone());
            }
            verdicts.insert(
                subject.key.clone(),
                TokenHourlyRequestVerdict::new(
                    used,
                    request_limit,
                    window_minutes,
                    subject.scope,
                    retry_after_seconds,
                ),
            );
        }
        for key in empty_keys {
            state.entries.remove(&key);
        }
        verdicts
    }

    fn maybe_gc(state: &mut MemoryRequestRateLimitState, now_ts: i64, window_secs: i64) {
        if now_ts < state.next_gc_at {
            return;
        }
        state.entries.retain(|_, queue| {
            Self::prune_queue(queue, now_ts, window_secs);
            !queue.is_empty()
        });
        state.next_gc_at = now_ts.saturating_add(window_secs.max(60));
    }

    fn prune_queue(queue: &mut VecDeque<i64>, now_ts: i64, window_secs: i64) {
        let expires_at = now_ts - window_secs;
        while queue
            .front()
            .is_some_and(|timestamp| *timestamp <= expires_at)
        {
            queue.pop_front();
        }
    }

    async fn recent_timestamps(
        &self,
        subject: &RequestRateSubject,
        now_ts: i64,
        window_secs: i64,
    ) -> Vec<i64> {
        let mut state = self.state.lock().await;
        Self::maybe_gc(&mut state, now_ts, window_secs);
        let Some(queue) = state.entries.get_mut(&subject.key) else {
            return Vec::new();
        };
        Self::prune_queue(queue, now_ts, window_secs);
        let values = queue.iter().copied().collect();
        if queue.is_empty() {
            state.entries.remove(&subject.key);
        }
        values
    }

    #[cfg(test)]
    async fn debug_subject_count(&self) -> usize {
        self.state.lock().await.entries.len()
    }

    #[cfg(test)]
    async fn debug_prune_idle_subjects(&self, now_ts: i64, window_secs: i64) {
        let mut state = self.state.lock().await;
        state.next_gc_at = 0;
        Self::maybe_gc(&mut state, now_ts, window_secs);
    }
}

impl MemoryUserBusinessCalls1hBackend {
    async fn replace_from_backfill(
        &self,
        rows: &[UserBusinessCalls1hBackfillRow],
        now_ts: i64,
        retention_secs: i64,
    ) {
        let mut state = self.state.lock().await;
        state.entries.clear();
        state.next_gc_at = 0;
        for row in rows {
            state
                .entries
                .entry(row.user_id.clone())
                .or_default()
                .push_back(UserBusinessCallEvent {
                    created_at: row.created_at,
                    outcome: row.outcome,
                });
        }
        Self::maybe_gc(&mut state, now_ts, retention_secs);
    }

    async fn record_event(
        &self,
        user_id: &str,
        event: UserBusinessCallEvent,
        now_ts: i64,
        retention_secs: i64,
    ) {
        let mut state = self.state.lock().await;
        Self::maybe_gc(&mut state, now_ts, retention_secs);
        let queue = state.entries.entry(user_id.to_string()).or_default();
        Self::insert_event_sorted(queue, event);
        Self::prune_queue(queue, now_ts, retention_secs);
        if queue.is_empty() {
            state.entries.remove(user_id);
        }
    }

    async fn snapshot_many(
        &self,
        user_ids: &[String],
        now_ts: i64,
        rolling_window_secs: i64,
        retention_secs: i64,
    ) -> HashMap<String, UserBusinessCallCounts> {
        let mut state = self.state.lock().await;
        Self::maybe_gc(&mut state, now_ts, retention_secs);
        let mut out = HashMap::with_capacity(user_ids.len());
        let mut empty_keys = Vec::new();
        for user_id in user_ids {
            let counts = if let Some(queue) = state.entries.get_mut(user_id) {
                Self::prune_queue(queue, now_ts, retention_secs);
                let counts = Self::rolling_counts(queue, now_ts, rolling_window_secs);
                if queue.is_empty() {
                    empty_keys.push(user_id.clone());
                }
                counts
            } else {
                UserBusinessCallCounts::default()
            };
            out.insert(user_id.clone(), counts);
        }
        for key in empty_keys {
            state.entries.remove(&key);
        }
        out
    }

    async fn retained_events_for_user(
        &self,
        user_id: &str,
        now_ts: i64,
        retention_secs: i64,
    ) -> Vec<UserBusinessCallEvent> {
        let mut state = self.state.lock().await;
        Self::maybe_gc(&mut state, now_ts, retention_secs);
        let Some(queue) = state.entries.get_mut(user_id) else {
            return Vec::new();
        };
        Self::prune_queue(queue, now_ts, retention_secs);
        let out = queue.iter().cloned().collect();
        if queue.is_empty() {
            state.entries.remove(user_id);
        }
        out
    }

    fn maybe_gc(state: &mut MemoryUserBusinessCalls1hState, now_ts: i64, retention_secs: i64) {
        if now_ts < state.next_gc_at {
            return;
        }
        state.entries.retain(|_, queue| {
            Self::prune_queue(queue, now_ts, retention_secs);
            !queue.is_empty()
        });
        state.next_gc_at = now_ts.saturating_add(retention_secs.clamp(60, SECS_PER_HOUR));
    }

    fn prune_queue(queue: &mut VecDeque<UserBusinessCallEvent>, now_ts: i64, retention_secs: i64) {
        let expires_at = now_ts - retention_secs;
        while queue
            .front()
            .is_some_and(|event| event.created_at <= expires_at)
        {
            queue.pop_front();
        }
    }

    fn insert_event_sorted(
        queue: &mut VecDeque<UserBusinessCallEvent>,
        event: UserBusinessCallEvent,
    ) {
        let insert_at = queue
            .iter()
            .position(|existing| existing.created_at > event.created_at)
            .unwrap_or(queue.len());
        queue.insert(insert_at, event);
    }

    fn rolling_counts(
        queue: &VecDeque<UserBusinessCallEvent>,
        now_ts: i64,
        rolling_window_secs: i64,
    ) -> UserBusinessCallCounts {
        let cutoff = now_ts - rolling_window_secs;
        let mut counts = UserBusinessCallCounts::default();
        for event in queue.iter().filter(|event| event.created_at > cutoff) {
            counts.record(event.outcome);
        }
        counts
    }
}
