#[derive(Clone)]
pub(crate) struct AppState {
    proxy: TavilyProxy,
    static_dir: Option<PathBuf>,
    forward_auth: ForwardAuthConfig,
    forward_auth_enabled: bool,
    builtin_admin: BuiltinAdminAuth,
    admin_passkey: AdminPasskeyOptions,
    linuxdo_oauth: LinuxDoOAuthOptions,
    linuxdo_credit: LinuxDoCreditOptions,
    ha: tavily_hikari::HaRuntime,
    dev_open_admin: bool,
    usage_base: String,
    api_key_ip_geo_origin: String,
    dashboard_overview_cache: Arc<Mutex<DashboardOverviewCacheState>>,
    remote_attempt_admission: Arc<RemoteAttemptAdmissionController>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DashboardOverviewFreshness {
    summary: [i64; 10],
    summary_last_activity: Option<i64>,
    summary_window_starts: [i64; 3],
    dashboard_rollup_signature: [i64; 19],
    pending_dashboard_rollup_signature: [i64; 10],
    rollup_integrity: (String, Option<i64>, Option<i64>, i64),
    dashboard_api_key_lifecycle_signature: [i64; 3],
    dashboard_quarantine_lifecycle_signature: [i64; 3],
    dashboard_exhausted_lifecycle_signature: [i64; 3],
    dashboard_quota_charge_token: [i64; 5],
    dashboard_stale_key_count: i64,
    forward_proxy: Option<(i64, i64)>,
    exhausted_keys: Vec<String>,
    latest_quota_sync_sample_at: Option<i64>,
    latest_request_log_id: Option<i64>,
    recent_request_logs: Vec<(i64, i64)>,
    trend_request_logs: Vec<(i64, i64)>,
    recent_jobs: Vec<(i64, String, Option<i64>)>,
    recent_alerts_token: [i64; 4],
    recent_alerts_total_events: i64,
    recent_alerts_grouped_count: i64,
    recent_alerts_counts: Vec<(String, i64)>,
    recent_alerts_top_groups: Vec<(String, i64, i64)>,
    request_log_retention_days: i64,
    hourly_window_anchor: i64,
    retention_since: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DashboardBuildReason {
    Cold,
    RequestStatsDirty,
    AlertProjectionDirty,
    SafetyProbeChanged,
}

impl DashboardBuildReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::RequestStatsDirty => "request_stats_dirty",
            Self::AlertProjectionDirty => "alert_projection_dirty",
            Self::SafetyProbeChanged => "safety_probe_changed",
        }
    }
}

#[derive(Debug, Clone)]
struct CachedDashboardOverviewSnapshot {
    snapshot: Arc<DashboardOverviewSnapshot>,
    freshness: Arc<DashboardOverviewFreshness>,
}

#[derive(Debug)]
struct DashboardOverviewCacheState {
    cached: Option<CachedDashboardOverviewSnapshot>,
    loading: bool,
    loading_started_at: Option<tokio::time::Instant>,
    loading_generation: u64,
    built_request_stats_generation: Option<u64>,
    alert_projection_generation: u64,
    built_alert_projection_generation: Option<u64>,
    last_refresh_requested_at: Option<tokio::time::Instant>,
    last_freshness_probe_at: Option<tokio::time::Instant>,
    last_build_reason: Option<DashboardBuildReason>,
    notify: Arc<tokio::sync::Notify>,
    admin_alerts: AdminAlertsReadCache,
    admin_alerts_prewarm_in_flight: bool,
    admin_alerts_prewarm_not_before: Option<tokio::time::Instant>,
    admin_alerts_prewarm_defers: u8,
    admin_privacy_status: AdminPrivacyStatusController,
    #[cfg(test)]
    build_count: usize,
    #[cfg(test)]
    freshness_probe_count: usize,
    #[cfg(test)]
    admin_privacy_refresh_count: usize,
}

impl Default for DashboardOverviewCacheState {
    fn default() -> Self {
        Self {
            cached: None,
            loading: false,
            loading_started_at: None,
            loading_generation: 0,
            built_request_stats_generation: None,
            alert_projection_generation: 0,
            built_alert_projection_generation: None,
            last_refresh_requested_at: None,
            last_freshness_probe_at: None,
            last_build_reason: None,
            notify: Arc::new(tokio::sync::Notify::new()),
            admin_alerts: AdminAlertsReadCache::default(),
            admin_alerts_prewarm_in_flight: false,
            admin_alerts_prewarm_not_before: None,
            admin_alerts_prewarm_defers: 0,
            admin_privacy_status: AdminPrivacyStatusController::default(),
            #[cfg(test)]
            build_count: 0,
            #[cfg(test)]
            freshness_probe_count: 0,
            #[cfg(test)]
            admin_privacy_refresh_count: 0,
        }
    }
}

const ADMIN_ALERTS_CACHE_CAPACITY: usize = 64;
const ADMIN_ALERTS_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(5 * 60);
const ADMIN_ALERTS_PREWARM_MIN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

fn admin_alerts_warm_deferred(reason: &'static str) -> tavily_hikari::ProxyError {
    tavily_hikari::ProxyError::Deferred {
        operation: "admin_alerts_warm",
        reason: reason.to_string(),
    }
}

fn admin_alerts_warm_error_reason(error: &tavily_hikari::ProxyError) -> &'static str {
    let tavily_hikari::ProxyError::Deferred { reason, .. } = error else {
        return "sqlite_pressure";
    };
    match reason.as_str() {
        "foreground_pressure" => "foreground_pressure",
        "pool_pressure" => "pool_pressure",
        "recent_contention" => "recent_contention",
        "projection_generation_changed" => "projection_generation_changed",
        "read_budget" => "read_budget",
        _ => "deferred",
    }
}

impl DashboardOverviewCacheState {
    fn try_start_admin_alerts_prewarm(&mut self, now: tokio::time::Instant) -> bool {
        if self.admin_alerts_prewarm_in_flight
            || self
                .admin_alerts_prewarm_not_before
                .is_some_and(|not_before| now < not_before)
        {
            return false;
        }

        self.admin_alerts_prewarm_in_flight = true;
        self.admin_alerts_prewarm_not_before = Some(now + ADMIN_ALERTS_PREWARM_MIN_INTERVAL);
        true
    }

    fn finish_admin_alerts_prewarm(&mut self) {
        self.admin_alerts_prewarm_in_flight = false;
        self.admin_alerts_prewarm_defers = 0;
        self.admin_alerts_prewarm_not_before = Some(
            tokio::time::Instant::now() + ADMIN_ALERTS_PREWARM_MIN_INTERVAL,
        );
    }

    fn defer_admin_alerts_prewarm(&mut self, now: tokio::time::Instant) -> std::time::Duration {
        self.admin_alerts_prewarm_defers = self.admin_alerts_prewarm_defers.saturating_add(1);
        let delay = match self.admin_alerts_prewarm_defers {
            1 | 2 => std::time::Duration::from_secs(5),
            _ => std::time::Duration::from_secs(30),
        };
        self.admin_alerts_prewarm_not_before = Some(now + delay);
        delay
    }
}

#[derive(Debug, Clone)]
enum AdminAlertsReadCacheValue {
    Catalog(AlertCatalog),
    Events(PaginatedAlertEvents),
    Groups(PaginatedAlertGroups),
}

#[derive(Debug, Clone)]
struct AdminAlertsReadCacheEntry {
    key: String,
    value: AdminAlertsReadCacheValue,
    generation: u64,
    canonical: bool,
    observed_at: i64,
    stored_at: tokio::time::Instant,
}

#[derive(Debug, Clone, Default)]
struct AdminAlertsReadCache {
    entries: VecDeque<AdminAlertsReadCacheEntry>,
}

#[derive(Debug, Clone)]
struct AdminPrivacyStatusCacheEntry {
    value: tavily_hikari::UpstreamPrivacyStatus,
    observed_at: i64,
    stored_at: tokio::time::Instant,
}

#[derive(Debug, Default)]
struct AdminPrivacyStatusController {
    last_good: Option<AdminPrivacyStatusCacheEntry>,
    refresh_in_flight: bool,
    last_refresh_reason: Option<&'static str>,
    refresh_task: Option<tokio::task::JoinHandle<()>>,
    prewarm_task: Option<tokio::task::JoinHandle<()>>,
    shutting_down: bool,
    #[cfg(test)]
    prewarm_deferred: Arc<Notify>,
    #[cfg(test)]
    last_good_published: Arc<Notify>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AdminPrivacyStatusRefreshStart {
    Fresh,
    Started,
    InFlight,
    Deferred { reason: &'static str },
}

pub(crate) enum AdminPrivacyStatusResponse {
    Fresh(tavily_hikari::UpstreamPrivacyStatus),
    Stale(tavily_hikari::UpstreamPrivacyStatus),
    Cold,
}

const ADMIN_PRIVACY_STATUS_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(60);

async fn admin_alerts_last_good(
    state: &AppState,
    key: &str,
) -> Option<(AdminAlertsReadCacheValue, i64)> {
    let cache = dashboard_overview_cache_for_state(state);
    let mut cache = cache.lock().await;
    let position = cache
        .admin_alerts
        .entries
        .iter()
        .position(|entry| entry.key == key && entry.stored_at.elapsed() <= ADMIN_ALERTS_CACHE_TTL)?;
    let entry = cache.admin_alerts.entries.remove(position)?;
    let observed_at = entry.observed_at;
    let value = entry.value.clone();
    cache.admin_alerts.entries.push_front(entry);
    Some((value, observed_at))
}

async fn admin_alerts_canonical_last_good(
    state: &AppState,
    key: &str,
) -> Option<(AdminAlertsReadCacheValue, i64, u64, u64)> {
    let cache = dashboard_overview_cache_for_state(state);
    let mut cache = cache.lock().await;
    let position = cache.admin_alerts.entries.iter().position(|entry| {
        entry.key == key && entry.canonical && entry.stored_at.elapsed() <= ADMIN_ALERTS_CACHE_TTL
    })?;
    let entry = cache.admin_alerts.entries.remove(position)?;
    let result = (entry.value.clone(), entry.observed_at, entry.generation, cache.alert_projection_generation);
    cache.admin_alerts.entries.push_front(entry);
    Some(result)
}

async fn record_admin_alerts_last_good(
    state: &AppState,
    key: String,
    value: AdminAlertsReadCacheValue,
) {
    record_admin_alerts_last_good_at_generation(
        state,
        key,
        value,
        current_admin_alerts_generation(state).await,
    )
    .await;
}

async fn current_admin_alerts_generation(state: &AppState) -> u64 {
    dashboard_overview_cache_for_state(state)
        .lock()
        .await
        .alert_projection_generation
}

async fn record_admin_alerts_last_good_at_generation(
    state: &AppState,
    key: String,
    value: AdminAlertsReadCacheValue,
    generation: u64,
) {
    let cache = dashboard_overview_cache_for_state(state);
    let mut cache = cache.lock().await;
    let canonical = key == "catalog" || key == default_admin_alert_cache_key("events") || key == default_admin_alert_cache_key("groups");
    cache.admin_alerts.entries.retain(|entry| entry.key != key);
    cache.admin_alerts.entries.push_front(AdminAlertsReadCacheEntry {
        key,
        value,
        generation,
        canonical,
        observed_at: state.proxy.backend_time().now_ts(),
        stored_at: tokio::time::Instant::now(),
    });
    while cache.admin_alerts.entries.len() > ADMIN_ALERTS_CACHE_CAPACITY {
        if let Some(index) = cache
            .admin_alerts
            .entries
            .iter()
            .rposition(|entry| !entry.canonical)
        {
            cache.admin_alerts.entries.remove(index);
        } else {
            break;
        }
    }
}

async fn publish_admin_alerts_canonical(
    state: &AppState,
    generation: u64,
    catalog: AlertCatalog,
    events: PaginatedAlertEvents,
    groups: PaginatedAlertGroups,
) -> bool {
    let cache = dashboard_overview_cache_for_state(state);
    let mut cache = cache.lock().await;
    if cache.alert_projection_generation != generation {
        return false;
    }
    let observed_at = state.proxy.backend_time().now_ts();
    let stored_at = tokio::time::Instant::now();
    for (key, value) in [
        ("catalog".to_string(), AdminAlertsReadCacheValue::Catalog(catalog)),
        (
            default_admin_alert_cache_key("events"),
            AdminAlertsReadCacheValue::Events(events),
        ),
        (
            default_admin_alert_cache_key("groups"),
            AdminAlertsReadCacheValue::Groups(groups),
        ),
    ] {
        cache.admin_alerts.entries.retain(|entry| entry.key != key);
        cache.admin_alerts.entries.push_front(AdminAlertsReadCacheEntry {
            key,
            value,
            generation,
            canonical: true,
            observed_at,
            stored_at,
        });
    }
    while cache.admin_alerts.entries.len() > ADMIN_ALERTS_CACHE_CAPACITY {
        if let Some(index) = cache
            .admin_alerts
            .entries
            .iter()
            .rposition(|entry| !entry.canonical)
        {
            cache.admin_alerts.entries.remove(index);
        } else {
            break;
        }
    }
    true
}

fn default_admin_alert_cache_key(kind: &str) -> String {
    serde_json::to_string(&(
        kind,
        Option::<&str>::None,
        Option::<i64>::None,
        Option::<i64>::None,
        Option::<&str>::None,
        Option::<&str>::None,
        Option::<&str>::None,
        Vec::<String>::new(),
        1_i64,
        20_i64,
    ))
    .expect("default admin Alerts cache key fields are serializable")
}

pub(crate) async fn prewarm_admin_alerts(state: Arc<AppState>) {
    let cache = dashboard_overview_cache_for_state(state.as_ref());
    {
        let mut cache = cache.lock().await;
        if !cache.try_start_admin_alerts_prewarm(tokio::time::Instant::now()) {
            return;
        }
    }
    tokio::spawn(async move {
        loop {
            if let Some(reason) = state.proxy.admin_alerts_cache_warm_defer_reason() {
                state.proxy.record_admin_alerts_warm_defer();
                let delay = dashboard_overview_cache_for_state(state.as_ref())
                    .lock()
                    .await
                    .defer_admin_alerts_prewarm(tokio::time::Instant::now());
                tracing::debug!(
                    component = "admin_read",
                    event = "alerts_canonical_warm_deferred",
                    reason,
                    retry_after_secs = delay.as_secs(),
                    "deferred canonical administrator Alerts cache before SQLite admission"
                );
                tokio::time::sleep(delay).await;
                continue;
            }
            let generation = current_admin_alerts_generation(state.as_ref()).await;
            let result = async {
                state.proxy.record_admin_alerts_warm_slice();
                let catalog = state.proxy.admin_alert_catalog_for_cache_warm().await?;
                if let Some(reason) = state.proxy.admin_alerts_cache_warm_defer_reason() {
                    return Err(admin_alerts_warm_deferred(reason));
                }
                state.proxy.record_admin_alerts_warm_slice();
                let events = state
                    .proxy
                    .admin_alert_events_page_for_cache_warm(1, 20)
                    .await?;
                if let Some(reason) = state.proxy.admin_alerts_cache_warm_defer_reason() {
                    return Err(admin_alerts_warm_deferred(reason));
                }
                state.proxy.record_admin_alerts_warm_slice();
                let groups = state
                    .proxy
                    .admin_alert_groups_page_for_cache_warm(1, 20)
                    .await?;
                if !publish_admin_alerts_canonical(
                    state.as_ref(),
                    generation,
                    catalog,
                    events,
                    groups,
                )
                .await
                {
                    state.proxy.record_admin_alerts_warm_generation_discard();
                    return Err(tavily_hikari::ProxyError::Deferred {
                        operation: "admin_alerts_warm",
                        reason: "projection_generation_changed".to_string(),
                    });
                }
                Ok::<(), tavily_hikari::ProxyError>(())
            }
            .await;

            match result {
                Ok(()) => {
                    state.proxy.record_admin_alerts_warm_publish();
                    dashboard_overview_cache_for_state(state.as_ref())
                        .lock()
                        .await
                        .finish_admin_alerts_prewarm();
                    tracing::debug!(
                        component = "admin_read",
                        event = "alerts_canonical_warm_published",
                        "published canonical administrator Alerts cache"
                    );
                    break;
                }
                Err(error)
                    if tavily_hikari::is_transient_sqlite_write_error(&error)
                        || error.is_deferred() =>
                {
                    state.proxy.record_admin_alerts_warm_defer();
                    let delay = dashboard_overview_cache_for_state(state.as_ref())
                        .lock()
                        .await
                        .defer_admin_alerts_prewarm(tokio::time::Instant::now());
                    tracing::debug!(
                        component = "admin_read",
                        event = "alerts_canonical_warm_deferred",
                        reason = admin_alerts_warm_error_reason(&error),
                        retry_after_secs = delay.as_secs(),
                        "deferred canonical administrator Alerts cache"
                    );
                    tokio::time::sleep(delay).await;
                }
                Err(error) => {
                    tracing::error!(
                        component = "admin_read",
                        event = "alerts_canonical_warm_failed",
                        error = %error,
                        "canonical administrator Alerts cache failed"
                    );
                    dashboard_overview_cache_for_state(state.as_ref())
                        .lock()
                        .await
                        .finish_admin_alerts_prewarm();
                    break;
                }
            }
        }
    });
}

#[cfg(test)]
pub(crate) async fn admin_privacy_status_cached(
    state: &AppState,
) -> Option<(tavily_hikari::UpstreamPrivacyStatus, i64)> {
    let cache = dashboard_overview_cache_for_state(state);
    let cache = cache.lock().await;
    let entry = cache.admin_privacy_status.last_good.as_ref()?;
    Some((entry.value.clone(), entry.observed_at))
}

#[cfg(test)]
pub(crate) async fn admin_privacy_status_prewarm_deferred_signal_for_test(
    state: &AppState,
) -> Arc<Notify> {
    dashboard_overview_cache_for_state(state)
        .lock()
        .await
        .admin_privacy_status
        .prewarm_deferred
        .clone()
}

#[cfg(test)]
pub(crate) async fn admin_privacy_status_last_good_published_signal_for_test(
    state: &AppState,
) -> Arc<Notify> {
    dashboard_overview_cache_for_state(state)
        .lock()
        .await
        .admin_privacy_status
        .last_good_published
        .clone()
}

pub(crate) async fn admin_privacy_status_last_good(
    state: &AppState,
) -> Option<(tavily_hikari::UpstreamPrivacyStatus, i64)> {
    let cache = dashboard_overview_cache_for_state(state);
    let cache = cache.lock().await;
    let entry = cache.admin_privacy_status.last_good.as_ref()?;
    (entry.stored_at.elapsed() <= ADMIN_PRIVACY_STATUS_CACHE_TTL)
        .then(|| (entry.value.clone(), entry.observed_at))
}

pub(crate) async fn read_admin_privacy_status(
    state: Arc<AppState>,
) -> AdminPrivacyStatusResponse {
    let cache = dashboard_overview_cache_for_state(state.as_ref());
    let mut cache = cache.lock().await;
    if let Some(entry) = cache.admin_privacy_status.last_good.as_ref()
        && entry.stored_at.elapsed() <= ADMIN_PRIVACY_STATUS_CACHE_TTL
    {
        return AdminPrivacyStatusResponse::Fresh(entry.value.clone());
    }

    start_admin_privacy_status_refresh_locked(state.clone(), &mut cache);
    let controller = &cache.admin_privacy_status;
    let response = if let Some(entry) = controller.last_good.as_ref() {
        let mut status = entry.value.clone();
        status.coverage = "stale".to_string();
        status.observed_at = Some(entry.observed_at);
        status.stale_reason = Some(
            controller
                .last_refresh_reason
                .unwrap_or("refresh_in_flight")
                .to_string(),
        );
        AdminPrivacyStatusResponse::Stale(status)
    } else {
        AdminPrivacyStatusResponse::Cold
    };
    drop(cache);
    prewarm_admin_privacy_status(state).await;
    response
}

pub(crate) async fn start_admin_privacy_status_refresh(
    state: Arc<AppState>,
) -> AdminPrivacyStatusRefreshStart {
    let cache = dashboard_overview_cache_for_state(state.as_ref());
    let mut cache = cache.lock().await;
    start_admin_privacy_status_refresh_locked(state, &mut cache)
}

fn start_admin_privacy_status_refresh_locked(
    state: Arc<AppState>,
    cache: &mut DashboardOverviewCacheState,
) -> AdminPrivacyStatusRefreshStart {
    if cache
        .admin_privacy_status
        .last_good
        .as_ref()
        .is_some_and(|entry| entry.stored_at.elapsed() <= ADMIN_PRIVACY_STATUS_CACHE_TTL)
    {
        return AdminPrivacyStatusRefreshStart::Fresh;
    }
    if cache.admin_privacy_status.refresh_in_flight {
        return AdminPrivacyStatusRefreshStart::InFlight;
    }
    if cache.admin_privacy_status.shutting_down {
        cache.admin_privacy_status.last_refresh_reason = Some("shutdown");
        return AdminPrivacyStatusRefreshStart::Deferred { reason: "shutdown" };
    }
    if let Some(reason) = state.proxy.admin_privacy_status_refresh_defer_reason() {
        cache.admin_privacy_status.last_refresh_reason = Some(reason);
        return AdminPrivacyStatusRefreshStart::Deferred { reason };
    }

    cache.admin_privacy_status.refresh_in_flight = true;
    cache.admin_privacy_status.last_refresh_reason = Some("refresh_in_flight");
    #[cfg(test)]
    {
        cache.admin_privacy_refresh_count = cache.admin_privacy_refresh_count.saturating_add(1);
    }
    let refresh_task = tokio::spawn(async move {
        let result = state.proxy.upstream_privacy_status().await;
        finish_admin_privacy_status_refresh(state.as_ref(), result).await;
    });
    cache.admin_privacy_status.refresh_task = Some(refresh_task);
    AdminPrivacyStatusRefreshStart::Started
}

async fn finish_admin_privacy_status_refresh(
    state: &AppState,
    result: Result<tavily_hikari::UpstreamPrivacyStatus, tavily_hikari::ProxyError>,
) {
    let cache = dashboard_overview_cache_for_state(state);
    let mut cache = cache.lock().await;
    cache.admin_privacy_status.refresh_in_flight = false;
    match result {
        Ok(mut value) => {
            let observed_at = state.proxy.backend_time().now_ts();
            value.coverage = "ok".to_string();
            value.observed_at = Some(observed_at);
            value.stale_reason = None;
            cache.admin_privacy_status.last_good = Some(AdminPrivacyStatusCacheEntry {
                value,
                observed_at,
                stored_at: tokio::time::Instant::now(),
            });
            cache.admin_privacy_status.last_refresh_reason = None;
            #[cfg(test)]
            cache
                .admin_privacy_status
                .last_good_published
                .notify_waiters();
        }
        Err(error) => {
            cache.admin_privacy_status.last_refresh_reason = Some(
                if tavily_hikari::is_transient_sqlite_write_error(&error) || error.is_deferred() {
                    "sqlite_pressure"
                } else {
                    "refresh_failed"
                },
            );
            tracing::debug!(
                component = "admin_read",
                event = "privacy_status_refresh_deferred",
                reason = cache.admin_privacy_status.last_refresh_reason.unwrap_or("unknown"),
                "privacy status refresh completed without replacing last-good data"
            );
        }
    }
}

fn emit_admin_privacy_status_prewarm_started() {
    tracing::info!(
        component = "startup",
        event = "admin_privacy_status_prewarm_started",
        "scheduled immutable privacy-status last-good prewarm"
    );
}

fn emit_admin_privacy_status_prewarm_deferred(reason: &'static str) {
    tracing::debug!(
        component = "startup",
        event = "admin_privacy_status_prewarm_deferred",
        reason,
        "deferred privacy-status prewarm before SQLite acquisition"
    );
}

pub(crate) async fn prewarm_admin_privacy_status(state: Arc<AppState>) {
    let cache = dashboard_overview_cache_for_state(state.as_ref());
    let mut cache = cache.lock().await;
    if cache.admin_privacy_status.shutting_down
        || cache
            .admin_privacy_status
            .prewarm_task
            .as_ref()
            .is_some_and(|task| !task.is_finished())
    {
        return;
    }
    cache.admin_privacy_status.prewarm_task = Some(tokio::spawn(async move {
        loop {
            if admin_privacy_status_last_good(state.as_ref()).await.is_some() {
                return;
            }
            match start_admin_privacy_status_refresh(state.clone()).await {
                AdminPrivacyStatusRefreshStart::Fresh => return,
                AdminPrivacyStatusRefreshStart::Started => emit_admin_privacy_status_prewarm_started(),
                // A prior prewarm iteration owns the singleflight refresh. Keep retrying for
                // completion, but do not report another start for the same operation.
                AdminPrivacyStatusRefreshStart::InFlight => {}
                AdminPrivacyStatusRefreshStart::Deferred { reason: "shutdown" } => return,
                AdminPrivacyStatusRefreshStart::Deferred { reason } => {
                    #[cfg(test)]
                    dashboard_overview_cache_for_state(state.as_ref())
                        .lock()
                        .await
                        .admin_privacy_status
                        .prewarm_deferred
                        .notify_waiters();
                    emit_admin_privacy_status_prewarm_deferred(reason);
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }));
}

pub(crate) async fn shutdown_admin_privacy_status_refresh(state: &AppState) {
    fence_admin_privacy_status_refresh(state).await;
    let (prewarm_task, refresh_task) = {
        let cache = dashboard_overview_cache_for_state(state);
        let mut cache = cache.lock().await;
        (
            cache.admin_privacy_status.prewarm_task.take(),
            cache.admin_privacy_status.refresh_task.take(),
        )
    };
    if let Some(prewarm_task) = prewarm_task
        && let Err(error) = prewarm_task.await
    {
        tracing::warn!(
            component = "shutdown",
            event = "admin_privacy_status_prewarm_join_failed",
            error = %error,
            "privacy-status prewarm ended before its cooperative boundary"
        );
    }
    let Some(refresh_task) = refresh_task else {
        return;
    };

    // A refresh owns an open immutable read snapshot until it reaches its
    // explicit close boundary. Do not abort it during shutdown, because Drop
    // must remain a fault-containment path rather than normal control flow.
    if let Err(error) = refresh_task.await {
        tracing::warn!(
            component = "shutdown",
            event = "admin_privacy_status_refresh_join_failed",
            error = %error,
            "privacy-status refresh ended before its cooperative close boundary"
        );
    }
}

pub(crate) async fn fence_admin_privacy_status_refresh(state: &AppState) {
    let cache = dashboard_overview_cache_for_state(state);
    cache.lock().await.admin_privacy_status.shutting_down = true;
}

#[cfg(test)]
pub(crate) async fn expire_admin_privacy_status_last_good_for_test(state: &AppState) {
    let cache = dashboard_overview_cache_for_state(state);
    let mut cache = cache.lock().await;
    if let Some(entry) = cache.admin_privacy_status.last_good.as_mut() {
        entry.stored_at = tokio::time::Instant::now() - ADMIN_PRIVACY_STATUS_CACHE_TTL;
    }
}

#[cfg(test)]
pub(crate) async fn admin_privacy_refresh_count_for_test(state: &AppState) -> usize {
    dashboard_overview_cache_for_state(state)
        .lock()
        .await
        .admin_privacy_refresh_count
}

#[cfg(test)]
pub(crate) async fn wait_for_admin_privacy_status_refresh(state: &AppState) {
    let cache = dashboard_overview_cache_for_state(state);
    for _ in 0..350 {
        if !cache.lock().await.admin_privacy_status.refresh_in_flight {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("admin privacy status refresh did not finish");
}

#[cfg(test)]
pub(crate) async fn wait_for_admin_privacy_status_last_good(state: &AppState) {
    for _ in 0..700 {
        if admin_privacy_status_last_good(state).await.is_some() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("admin privacy status refresh did not publish fresh last-good data");
}

#[cfg(test)]
pub(crate) async fn prime_admin_privacy_status_for_test(state: Arc<AppState>) {
    prewarm_admin_privacy_status(state.clone()).await;
    wait_for_admin_privacy_status_last_good(state.as_ref()).await;
}

#[cfg(test)]
mod admin_alerts_prewarm_tests {
    use super::{ADMIN_ALERTS_PREWARM_MIN_INTERVAL, DashboardOverviewCacheState};

    #[test]
    fn admin_alerts_prewarm_coalesces_projection_updates_for_one_minute() {
        let mut cache = DashboardOverviewCacheState::default();
        let now = tokio::time::Instant::now();

        assert!(cache.try_start_admin_alerts_prewarm(now));
        assert!(
            !cache.try_start_admin_alerts_prewarm(now),
            "an in-flight prewarm must remain singleflight"
        );

        cache.finish_admin_alerts_prewarm();
        assert!(
            !cache.try_start_admin_alerts_prewarm(
                now + ADMIN_ALERTS_PREWARM_MIN_INTERVAL - std::time::Duration::from_secs(1)
            ),
            "a busy projection must not restart the full admin cache warmup every slice"
        );
        assert!(cache.try_start_admin_alerts_prewarm(
            now + ADMIN_ALERTS_PREWARM_MIN_INTERVAL + std::time::Duration::from_millis(1)
        ));
    }

    #[test]
    fn admin_alerts_prewarm_backoff_is_bounded_and_recovers() {
        let mut cache = DashboardOverviewCacheState::default();
        let now = tokio::time::Instant::now();

        assert_eq!(cache.defer_admin_alerts_prewarm(now), std::time::Duration::from_secs(5));
        assert_eq!(cache.defer_admin_alerts_prewarm(now), std::time::Duration::from_secs(5));
        assert_eq!(cache.defer_admin_alerts_prewarm(now), std::time::Duration::from_secs(30));

        cache.finish_admin_alerts_prewarm();
        assert_eq!(cache.admin_alerts_prewarm_defers, 0);
    }
}

#[cfg(test)]
mod privacy_status_prewarm_logging_tests {
    use super::{
        emit_admin_privacy_status_prewarm_deferred, emit_admin_privacy_status_prewarm_started,
    };
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::{EnvFilter, fmt::MakeWriter};

    #[derive(Clone, Default)]
    struct SharedWriter {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    struct SharedWriterGuard {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for SharedWriterGuard {
        fn write(&mut self, value: &[u8]) -> io::Result<usize> {
            self.buffer
                .lock()
                .expect("tracing buffer lock")
                .extend_from_slice(value);
            Ok(value.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for SharedWriter {
        type Writer = SharedWriterGuard;

        fn make_writer(&'a self) -> Self::Writer {
            SharedWriterGuard {
                buffer: self.buffer.clone(),
            }
        }
    }

    #[test]
    fn privacy_status_prewarm_events_keep_their_observability_contract() {
        let writer = SharedWriter::default();
        let buffer = writer.buffer.clone();
        let dispatch = tracing::Dispatch::new(
            tracing_subscriber::fmt()
                .with_env_filter(EnvFilter::new("debug"))
                .with_writer(writer)
                .json()
                .flatten_event(true)
                .with_current_span(false)
                .with_span_list(false)
                .finish(),
        );
        tracing::dispatcher::with_default(&dispatch, || {
            emit_admin_privacy_status_prewarm_started();
            emit_admin_privacy_status_prewarm_deferred("foreground_pressure");
        });
        let output = String::from_utf8(buffer.lock().expect("tracing buffer lock").clone())
            .expect("utf8 tracing output");

        assert!(output.contains("\"level\":\"INFO\""));
        assert!(output.contains("\"event\":\"admin_privacy_status_prewarm_started\""));
        assert!(output.contains("\"level\":\"DEBUG\""));
        assert!(output.contains("\"event\":\"admin_privacy_status_prewarm_deferred\""));
        assert!(output.contains("\"reason\":\"foreground_pressure\""));
    }
}

fn new_dashboard_overview_cache() -> Arc<Mutex<DashboardOverviewCacheState>> {
    Arc::new(Mutex::new(DashboardOverviewCacheState::default()))
}

fn new_remote_attempt_admission() -> Arc<RemoteAttemptAdmissionController> {
    Arc::new(RemoteAttemptAdmissionController::default())
}

static DB_MAINTENANCE_GATE: OnceLock<RwLock<()>> = OnceLock::new();

static DB_JOB_EXECUTION_GATES: OnceLock<std::sync::Mutex<HashMap<usize, std::sync::Weak<Mutex<()>>>>> =
    OnceLock::new();
static MAINTENANCE_WORKER_WAKES: OnceLock<
    std::sync::Mutex<HashMap<usize, std::sync::Weak<tokio::sync::Notify>>>,
> = OnceLock::new();

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

fn dashboard_overview_cache_for_state(state: &AppState) -> Arc<Mutex<DashboardOverviewCacheState>> {
    state.dashboard_overview_cache.clone()
}

fn remote_attempt_admission_for_state(state: &AppState) -> Arc<RemoteAttemptAdmissionController> {
    state.remote_attempt_admission.clone()
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
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
    next: axum::middleware::Next,
) -> Response<Body> {
    let path = req.uri().path();
    if path == "/health" || !db_maintenance_gated_path(path) {
        return next.run(req).await;
    }
    state.proxy.record_foreground_activity();
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
    pub passkey_auth_enabled: bool,
    pub passkey_rp_id: Option<String>,
    pub passkey_rp_origin: Option<String>,
    pub passkey_challenge_ttl_secs: i64,
    pub passkey_session_max_age_secs: i64,
}

#[derive(Clone, Debug)]
pub struct AdminPasskeyOptions {
    pub enabled: bool,
    pub rp_id: Option<String>,
    pub rp_origin: Option<String>,
    pub scope: Option<tavily_hikari::AdminPasskeyScope>,
    pub challenge_ttl_secs: i64,
    pub session_max_age_secs: i64,
}

impl AdminPasskeyOptions {
    #[cfg(test)]
    fn disabled() -> Self {
        Self {
            enabled: false,
            rp_id: None,
            rp_origin: None,
            scope: None,
            challenge_ttl_secs: 300,
            session_max_age_secs: 60 * 60 * 24 * 14,
        }
    }

    fn is_configured(&self) -> bool {
        self.enabled
            && self.scope.is_some()
            && self
                .rp_id
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
            && self
                .rp_origin
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
    }

    fn webauthn(&self) -> Result<Webauthn, ProxyError> {
        let rp_id = self
            .rp_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ProxyError::Other("admin passkey RP ID is not configured".to_string()))?;
        let rp_origin = self
            .rp_origin
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ProxyError::Other("admin passkey RP origin is not configured".to_string())
            })?;
        let origin = Url::parse(rp_origin)
            .map_err(|err| ProxyError::Other(format!("invalid admin passkey RP origin: {err}")))?;
        WebauthnBuilder::new(rp_id, &origin)
            .and_then(|builder| builder.build())
            .map_err(|err| ProxyError::Other(format!("admin passkey webauthn setup failed: {err}")))
    }
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
const ADMIN_PASSKEY_COOKIE_NAME: &str = "hikari_admin_passkey_session";
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
struct BuiltinAdminCredentialState {
    password: Option<String>,
    password_hash: Option<String>,
    disabled: bool,
    updated_at: Option<i64>,
    login_totp_required: bool,
}

impl BuiltinAdminCredentialState {
    fn has_login_credential(&self) -> bool {
        !self.disabled
            && (self
                .password_hash
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
                || self
                    .password
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|value| !value.is_empty()))
    }
}

#[derive(Clone, Debug)]
struct BuiltinAdminPasswordStatus {
    enabled: bool,
    updated_at: Option<i64>,
    login_totp_required: bool,
}

#[derive(Clone, Debug)]
struct BuiltinAdminAuth {
    startup_credentials: BuiltinAdminCredentialState,
    persisted_password_allowed: bool,
    credentials: Arc<std::sync::RwLock<BuiltinAdminCredentialState>>,
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
        let credentials = if enabled {
            BuiltinAdminCredentialState {
                password,
                password_hash,
                disabled: false,
                updated_at: None,
                login_totp_required: false,
            }
        } else {
            BuiltinAdminCredentialState {
                password: None,
                password_hash: None,
                disabled: true,
                updated_at: None,
                login_totp_required: false,
            }
        };
        Self {
            startup_credentials: credentials.clone(),
            persisted_password_allowed: enabled,
            credentials: Arc::new(std::sync::RwLock::new(credentials)),
            sessions: Arc::new(std::sync::RwLock::new(HashMap::new())),
            backend_time,
        }
    }

    fn is_enabled(&self) -> bool {
        self.credentials
            .read()
            .map(|credentials| credentials.has_login_credential())
            .unwrap_or(false)
    }

    fn persisted_password_allowed(&self) -> bool {
        self.persisted_password_allowed
    }

    fn status(&self) -> BuiltinAdminPasswordStatus {
        self.credentials
            .read()
            .map(|credentials| BuiltinAdminPasswordStatus {
                enabled: credentials.has_login_credential(),
                updated_at: credentials.updated_at,
                login_totp_required: credentials.login_totp_required,
            })
            .unwrap_or(BuiltinAdminPasswordStatus {
                enabled: false,
                updated_at: None,
                login_totp_required: false,
            })
    }

    fn login_totp_required(&self) -> bool {
        self.credentials
            .read()
            .map(|credentials| credentials.login_totp_required)
            .unwrap_or(false)
    }

    fn apply_persisted_settings(
        &self,
        settings: Option<tavily_hikari::AdminPasswordSettingsRecord>,
    ) {
        let Some(settings) = settings else {
            if let Ok(mut credentials) = self.credentials.write() {
                *credentials = self.startup_credentials.clone();
            }
            self.clear_sessions();
            return;
        };
        let mut should_clear_sessions = !self.persisted_password_allowed;
        if let Ok(mut credentials) = self.credentials.write() {
            if !self.persisted_password_allowed {
                credentials.password = None;
                credentials.password_hash = None;
                credentials.disabled = true;
            } else if settings.password_hash.is_some() || settings.disabled_at.is_some() {
                should_clear_sessions = credentials.password.is_some()
                    || credentials.password_hash != settings.password_hash
                    || credentials.disabled != settings.disabled_at.is_some();
                credentials.password = None;
                credentials.password_hash = settings.password_hash;
                credentials.disabled = settings.disabled_at.is_some();
            }
            should_clear_sessions |= !credentials.login_totp_required && settings.login_totp_required;
            credentials.updated_at = Some(settings.updated_at);
            credentials.login_totp_required = settings.login_totp_required;
        }
        if should_clear_sessions {
            self.clear_sessions();
        }
    }

    fn set_password_hash(&self, password_hash: String, updated_at: Option<i64>) {
        if let Ok(mut credentials) = self.credentials.write() {
            if !self.persisted_password_allowed {
                credentials.password = None;
                credentials.password_hash = None;
                credentials.disabled = true;
                credentials.updated_at = updated_at;
                self.clear_sessions();
                return;
            }
            credentials.password = None;
            credentials.password_hash = Some(password_hash);
            credentials.disabled = false;
            credentials.updated_at = updated_at;
        }
        self.clear_sessions();
    }

    fn disable_password(&self, updated_at: Option<i64>) {
        if let Ok(mut credentials) = self.credentials.write() {
            credentials.password = None;
            credentials.password_hash = None;
            credentials.disabled = true;
            credentials.updated_at = updated_at;
        }
        self.clear_sessions();
    }

    fn set_login_totp_required(&self, required: bool, updated_at: Option<i64>) {
        if let Ok(mut credentials) = self.credentials.write() {
            credentials.login_totp_required = required;
            credentials.updated_at = updated_at;
        }
    }

    fn clear_sessions(&self) {
        if let Ok(mut sessions) = self.sessions.write() {
            sessions.clear();
        }
    }

    fn is_admin(&self, headers: &HeaderMap) -> bool {
        if !self.is_enabled() {
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
        let credentials = self.credentials.read().ok()?.clone();
        if !credentials.has_login_credential() {
            return None;
        }
        if let Some(hash) = credentials.password_hash.as_deref() {
            let parsed = PasswordHash::new(hash).ok()?;
            if Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_err()
            {
                return None;
            }
        } else {
            let expected = credentials.password.as_deref()?;
            if password != expected {
                return None;
            }
        }
        Some(self.new_session())
    }

    fn remember_session(&self, token: String) {
        if !self.is_enabled() {
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

#[cfg(test)]
mod builtin_admin_auth_tests {
    use axum::http::{HeaderMap, HeaderValue, header::COOKIE};

    use super::{BUILTIN_ADMIN_COOKIE_NAME, BuiltinAdminAuth};

    #[test]
    fn admin_passkey_totp_only_persisted_settings_keep_env_password() {
        let admin = BuiltinAdminAuth::new(true, Some("env-password".to_string()), None);

        admin.apply_persisted_settings(Some(tavily_hikari::AdminPasswordSettingsRecord {
            password_hash: None,
            disabled_at: None,
            updated_at: 123,
            login_totp_required: true,
        }));

        assert!(admin.is_enabled());
        assert!(admin.login_totp_required());
        assert!(admin.login("env-password").is_some());
    }

    #[test]
    fn persisted_password_settings_do_not_override_startup_disable() {
        let admin = BuiltinAdminAuth::new(false, None, None);

        admin.apply_persisted_settings(Some(tavily_hikari::AdminPasswordSettingsRecord {
            password_hash: Some("stored-password-hash".to_string()),
            disabled_at: None,
            updated_at: 456,
            login_totp_required: true,
        }));

        assert!(!admin.is_enabled());
        assert!(admin.login_totp_required());
        assert!(admin.login("stored-password").is_none());
    }

    #[test]
    fn setting_password_does_not_override_startup_disable() {
        let admin = BuiltinAdminAuth::new(false, None, None);

        admin.set_password_hash("stored-password-hash".to_string(), Some(789));

        assert!(!admin.is_enabled());
        assert!(admin.login("stored-password").is_none());
    }

    #[test]
    fn rotating_password_revokes_existing_builtin_sessions() {
        let admin = BuiltinAdminAuth::new(true, Some("old-password".to_string()), None);
        let token = admin.login("old-password").expect("old password should log in");
        admin.remember_session(token.clone());

        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&format!("{BUILTIN_ADMIN_COOKIE_NAME}={token}"))
                .expect("cookie header should be valid"),
        );
        assert!(admin.is_admin(&headers));

        admin.set_password_hash("stored-password-hash".to_string(), Some(789));

        assert!(!admin.is_admin(&headers));
    }

    #[test]
    fn applying_rotated_persisted_password_revokes_existing_builtin_sessions() {
        let admin = BuiltinAdminAuth::new(true, Some("old-password".to_string()), None);
        let token = admin.login("old-password").expect("old password should log in");
        admin.remember_session(token.clone());

        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&format!("{BUILTIN_ADMIN_COOKIE_NAME}={token}"))
                .expect("cookie header should be valid"),
        );
        assert!(admin.is_admin(&headers));

        admin.apply_persisted_settings(Some(tavily_hikari::AdminPasswordSettingsRecord {
            password_hash: Some("stored-password-hash".to_string()),
            disabled_at: None,
            updated_at: 987,
            login_totp_required: false,
        }));

        assert!(!admin.is_admin(&headers));
    }

    #[test]
    fn applying_persisted_login_totp_requirement_revokes_existing_builtin_sessions() {
        let admin = BuiltinAdminAuth::new(true, Some("old-password".to_string()), None);
        let token = admin.login("old-password").expect("old password should log in");
        admin.remember_session(token.clone());

        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&format!("{BUILTIN_ADMIN_COOKIE_NAME}={token}"))
                .expect("cookie header should be valid"),
        );
        assert!(admin.is_admin(&headers));

        admin.apply_persisted_settings(Some(tavily_hikari::AdminPasswordSettingsRecord {
            password_hash: None,
            disabled_at: None,
            updated_at: 988,
            login_totp_required: true,
        }));

        assert!(admin.login_totp_required());
        assert!(!admin.is_admin(&headers));
        assert!(admin.login("old-password").is_some());
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

async fn is_admin_request(state: &AppState, headers: &HeaderMap) -> bool {
    try_is_admin_request(state, headers).await.unwrap_or(false)
}

async fn try_is_admin_request(state: &AppState, headers: &HeaderMap) -> Result<bool, ProxyError> {
    if state.dev_open_admin {
        return Ok(true);
    }
    if state.forward_auth_enabled && state.forward_auth.is_request_admin(headers) {
        return Ok(true);
    }
    if state.builtin_admin.is_admin(headers) {
        return Ok(true);
    }
    Ok(resolve_admin_passkey_session(state, headers).await?.is_some())
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

async fn resolve_admin_passkey_session(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<tavily_hikari::AdminPasskeySessionRecord>, ProxyError> {
    if !state.admin_passkey.is_configured() {
        return Ok(None);
    }
    let Some(token) = cookie_value(headers, ADMIN_PASSKEY_COOKIE_NAME) else {
        return Ok(None);
    };
    let Some(scope) = state.admin_passkey.scope.as_ref() else {
        return Ok(None);
    };
    state.proxy.get_active_admin_passkey_session(scope, &token).await
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

    if state.forward_auth_enabled && state.forward_auth.is_request_admin(headers) {
        actor.actor_display_name = state
            .forward_auth
            .nickname_value(headers)
            .or_else(|| state.forward_auth.user_value(headers).map(str::to_string))
            .or_else(|| state.forward_auth.admin_override_name().map(str::to_string));
        return actor;
    }

    if state.builtin_admin.is_admin(headers) {
        actor.actor_display_name = Some("builtin-admin".to_string());
        return actor;
    }

    if let Some(session) = resolve_admin_passkey_session(state, headers)
        .await
        .ok()
        .flatten()
    {
        actor.actor_display_name = Some(
            session
                .credential_id
                .as_deref()
                .map(|credential_id| {
                    let prefix = credential_id.chars().take(16).collect::<String>();
                    format!("admin-passkey:{prefix}")
                })
                .unwrap_or_else(|| "admin-passkey".to_string()),
        );
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
    admin_passkey: bool,
    user_value: Option<String>,
}

async fn debug_is_admin(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<IsAdminDebug>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers).await {
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
    let admin_passkey = resolve_admin_passkey_session(state.as_ref(), &headers)
        .await
        .ok()
        .flatten()
        .is_some();
    let is_admin = state.dev_open_admin || forward_auth_admin || builtin_admin || admin_passkey;
    Ok(Json(IsAdminDebug {
        is_admin,
        forward_auth_admin,
        builtin_admin,
        admin_passkey,
        user_value,
    }))
}

async fn health_check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let status = state.ha.status().await;
    if status.mode == tavily_hikari::HaMode::ActiveStandby && !status.allows_basic_business {
        return (StatusCode::OK, "ok");
    }
    if state.proxy.is_forward_proxy_xray_ready_strict().await {
        (StatusCode::OK, "ok")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "xray not ready")
    }
}
fn db_maintenance_gate() -> &'static RwLock<()> {
    DB_MAINTENANCE_GATE.get_or_init(|| RwLock::new(()))
}

async fn acquire_db_maintenance_read_gate() -> tokio::sync::RwLockReadGuard<'static, ()> {
    db_maintenance_gate().read().await
}

async fn acquire_db_maintenance_write_gate() -> tokio::sync::RwLockWriteGuard<'static, ()> {
    db_maintenance_gate().write().await
}
