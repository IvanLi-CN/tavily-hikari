use crate::analysis::*;
use crate::models::*;
use crate::store::*;
use crate::*;

#[derive(Clone, Debug)]
struct TokenQuota {
    store: Arc<KeyStore>,
    cleanup: Arc<Mutex<CleanupState>>,
    hourly_limit: i64,
    daily_limit: i64,
    monthly_limit: i64,
}

/// Lightweight per-token hourly request limiter that counts *all* authenticated
/// requests, regardless of MCP method or HTTP endpoint.
#[derive(Clone, Debug)]
struct TokenRequestLimit {
    store: Arc<KeyStore>,
    cleanup: Arc<Mutex<CleanupState>>,
    hourly_limit: i64,
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
    pub(crate) research_request_affinity: Arc<Mutex<TokenAffinityState>>,
    pub(crate) research_request_owner_affinity: Arc<Mutex<TokenAffinityState>>,
    // Fast in-process lock to collapse duplicate work within one instance. Cross-instance
    // serialization is provided by quota_subject_locks in SQLite.
    pub(crate) token_billing_locks: Arc<Mutex<HashMap<String, Weak<Mutex<()>>>>>,
    pub(crate) research_key_locks: Arc<Mutex<HashMap<String, Weak<Mutex<()>>>>>,
}

#[derive(Clone, Debug)]
pub struct TavilyProxyOptions {
    pub xray_binary: String,
    pub xray_runtime_dir: std::path::PathBuf,
    pub forward_proxy_trace_url: Url,
}

impl TavilyProxyOptions {
    pub fn from_database_path(database_path: &str) -> Self {
        Self {
            xray_binary: forward_proxy::default_xray_binary(),
            xray_runtime_dir: forward_proxy::default_xray_runtime_dir(database_path),
            forward_proxy_trace_url: default_forward_proxy_trace_url(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct QuotaSubjectDbLease {
    pub(crate) subject: String,
    pub(crate) owner: String,
    pub(crate) ttl: Duration,
}

#[derive(Debug)]
struct QuotaSubjectLockGuard {
    store: Arc<KeyStore>,
    lease: QuotaSubjectDbLease,
    refresh_stop: Arc<AtomicBool>,
    lease_lost: Arc<AtomicBool>,
    refresh_task: tokio::task::JoinHandle<()>,
}

impl QuotaSubjectLockGuard {
    pub(crate) fn new(store: Arc<KeyStore>, lease: QuotaSubjectDbLease) -> Self {
        let refresh_stop = Arc::new(AtomicBool::new(false));
        let lease_lost = Arc::new(AtomicBool::new(false));
        let refresh_task = {
            let store = Arc::clone(&store);
            let lease = lease.clone();
            let refresh_stop = Arc::clone(&refresh_stop);
            let lease_lost = Arc::clone(&lease_lost);
            tokio::spawn(async move {
                let refresh_every = Duration::from_secs(QUOTA_SUBJECT_LOCK_REFRESH_SECS);
                let retry_every = Duration::from_secs(QUOTA_SUBJECT_LOCK_REFRESH_RETRY_SECS);
                while !refresh_stop.load(AtomicOrdering::Relaxed) {
                    tokio::time::sleep(refresh_every).await;
                    if refresh_stop.load(AtomicOrdering::Relaxed) {
                        break;
                    }

                    let retry_budget = lease.ttl.saturating_sub(refresh_every);
                    let retry_deadline = Instant::now() + retry_budget.max(retry_every);
                    loop {
                        match store.refresh_quota_subject_lock(&lease).await {
                            Ok(()) => break,
                            Err(err) => {
                                if refresh_stop.load(AtomicOrdering::Relaxed) {
                                    return;
                                }
                                if Instant::now() >= retry_deadline {
                                    lease_lost.store(true, AtomicOrdering::Relaxed);
                                    eprintln!(
                                        "quota subject lock refresh exhausted retries (subject={} owner={}): {}",
                                        lease.subject, lease.owner, err
                                    );
                                    return;
                                }
                                eprintln!(
                                    "quota subject lock refresh failed (subject={} owner={}): {}; retrying",
                                    lease.subject, lease.owner, err
                                );
                                tokio::time::sleep(retry_every).await;
                            }
                        }
                    }
                }
            })
        };

        Self {
            store,
            lease,
            refresh_stop,
            lease_lost,
            refresh_task,
        }
    }

    pub(crate) fn ensure_live(&self) -> Result<(), ProxyError> {
        if self.lease_lost.load(AtomicOrdering::Relaxed) {
            return Err(ProxyError::Other(format!(
                "quota subject lock lost for {}",
                self.lease.subject,
            )));
        }
        let mut forced = self
            .store
            .forced_quota_subject_lock_loss_subjects
            .lock()
            .expect("forced quota subject lock loss mutex poisoned");
        if forced.remove(&self.lease.subject) {
            return Err(ProxyError::Other(format!(
                "quota subject lock lost for {}",
                self.lease.subject,
            )));
        }
        Ok(())
    }
}

impl Drop for QuotaSubjectLockGuard {
    fn drop(&mut self) {
        self.refresh_stop.store(true, AtomicOrdering::Relaxed);
        self.refresh_task.abort();

        let store = Arc::clone(&self.store);
        let lease = self.lease.clone();
        tokio::spawn(async move {
            if let Err(err) = store.release_quota_subject_lock(&lease).await {
                eprintln!(
                    "quota subject lock release failed (subject={} owner={}): {}",
                    lease.subject, lease.owner, err
                );
            }
        });
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

fn default_forward_proxy_trace_url() -> Url {
    std::env::var("FORWARD_PROXY_TRACE_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .and_then(|value| Url::parse(&value).ok())
        .unwrap_or_else(|| Url::parse(FORWARD_PROXY_TRACE_URL).expect("valid trace url"))
}

impl TavilyProxy {
    pub async fn new<I, S>(keys: I, database_path: &str) -> Result<Self, ProxyError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::with_options(
            keys,
            DEFAULT_UPSTREAM,
            database_path,
            TavilyProxyOptions::from_database_path(database_path),
        )
        .await
    }

    pub async fn with_endpoint<I, S>(
        keys: I,
        upstream: &str,
        database_path: &str,
    ) -> Result<Self, ProxyError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::with_options(
            keys,
            upstream,
            database_path,
            TavilyProxyOptions::from_database_path(database_path),
        )
        .await
    }

    pub async fn with_options<I, S>(
        keys: I,
        upstream: &str,
        database_path: &str,
        options: TavilyProxyOptions,
    ) -> Result<Self, ProxyError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let sanitized: Vec<String> = keys
            .into_iter()
            .map(|k| k.into().trim().to_owned())
            .filter(|k| !k.is_empty())
            .collect();

        let key_store = KeyStore::new(database_path).await?;
        if !sanitized.is_empty() {
            key_store.sync_keys(&sanitized).await?;
        }
        let upstream = Url::parse(upstream).map_err(|source| ProxyError::InvalidEndpoint {
            endpoint: upstream.to_owned(),
            source,
        })?;
        let upstream_origin = origin_from_url(&upstream);
        let forward_proxy_settings =
            forward_proxy::load_forward_proxy_settings(&key_store.pool).await?;
        let forward_proxy_runtime =
            forward_proxy::load_forward_proxy_runtime_states(&key_store.pool).await?;
        let forward_proxy = Arc::new(Mutex::new(forward_proxy::ForwardProxyManager::new(
            forward_proxy_settings,
            forward_proxy_runtime,
        )));
        let key_store = Arc::new(key_store);
        let token_quota = TokenQuota::new(key_store.clone());
        let token_request_limit = TokenRequestLimit::new(key_store.clone());
        let forward_proxy_clients = forward_proxy::ForwardProxyClientPool::new()?;
        let mut proxy = Self {
            client: forward_proxy_clients.direct_client(),
            forward_proxy_clients,
            forward_proxy,
            forward_proxy_affinity: Arc::new(Mutex::new(HashMap::new())),
            forward_proxy_trace_url: options.forward_proxy_trace_url,
            #[cfg(test)]
            forward_proxy_trace_overrides: Arc::new(Mutex::new(HashMap::new())),
            xray_supervisor: Arc::new(Mutex::new(forward_proxy::XraySupervisor::new(
                options.xray_binary,
                options.xray_runtime_dir,
            ))),
            upstream,
            key_store,
            upstream_origin,
            api_key_geo_origin: std::env::var("API_KEY_IP_GEO_ORIGIN")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "https://api.country.is".to_string()),
            token_quota,
            token_request_limit,
            research_request_affinity: Arc::new(Mutex::new(TokenAffinityState::new(
                RESEARCH_REQUEST_AFFINITY_TTL_SECS,
            ))),
            research_request_owner_affinity: Arc::new(Mutex::new(TokenAffinityState::new(
                RESEARCH_REQUEST_AFFINITY_TTL_SECS,
            ))),
            token_billing_locks: Arc::new(Mutex::new(HashMap::new())),
            research_key_locks: Arc::new(Mutex::new(HashMap::new())),
        };
        proxy.initialize_forward_proxy_runtime().await?;
        Ok(proxy)
    }

    pub(crate) async fn initialize_forward_proxy_runtime(&mut self) -> Result<(), ProxyError> {
        if let Err(err) = self.refresh_forward_proxy_subscriptions().await {
            eprintln!("forward-proxy startup subscription refresh error: {err}");
        }
        let manager = self.forward_proxy.lock().await;
        forward_proxy::sync_manager_runtime_to_store(&self.key_store, &manager).await
    }

    pub async fn get_forward_proxy_settings(
        &self,
    ) -> Result<ForwardProxySettingsResponse, ProxyError> {
        let manager = self.forward_proxy.lock().await;
        forward_proxy::build_forward_proxy_settings_response(&self.key_store.pool, &manager).await
    }

    pub async fn get_forward_proxy_live_stats(
        &self,
    ) -> Result<ForwardProxyLiveStatsResponse, ProxyError> {
        let manager = self.forward_proxy.lock().await;
        forward_proxy::build_forward_proxy_live_stats_response(&self.key_store.pool, &manager).await
    }

    pub async fn get_forward_proxy_dashboard_summary(
        &self,
    ) -> Result<ForwardProxyDashboardSummary, ProxyError> {
        let manager = self.forward_proxy.lock().await;
        let runtime_rows = manager.snapshot_runtime();
        Ok(ForwardProxyDashboardSummary {
            available_nodes: runtime_rows
                .iter()
                .filter(|node| node.available && !node.is_penalized())
                .count() as i64,
            total_nodes: runtime_rows.len() as i64,
        })
    }

    pub(crate) async fn validate_forward_proxy_egress_socks5(
        &self,
        egress_socks5_url: &Url,
    ) -> Result<(), ProxyError> {
        let probe_url = forward_proxy::derive_probe_url(&self.upstream);
        let client = self
            .forward_proxy_clients
            .direct_client_via_egress(Some(egress_socks5_url))
            .await?;
        let response = tokio::time::timeout(
            Duration::from_secs(forward_proxy::FORWARD_PROXY_VALIDATION_TIMEOUT_SECS),
            client.get(probe_url).send(),
        )
        .await
        .map_err(|_| ProxyError::Other("global SOCKS5 validation timed out".to_string()))?
        .map_err(ProxyError::Http)?;
        if !response.status().is_success()
            && response.status() != StatusCode::UNAUTHORIZED
            && response.status() != StatusCode::FORBIDDEN
            && response.status() != StatusCode::NOT_FOUND
        {
            return Err(ProxyError::Other(format!(
                "global SOCKS5 validation returned status {}",
                response.status()
            )));
        }
        Ok(())
    }

    pub(crate) async fn current_forward_proxy_egress_socks5_url(&self) -> Option<Url> {
        let manager = self.forward_proxy.lock().await;
        manager.settings.effective_egress_socks5_url()
    }

    pub async fn update_forward_proxy_settings(
        &self,
        settings: ForwardProxySettings,
        skip_bootstrap_probe: bool,
    ) -> Result<ForwardProxySettingsResponse, ProxyError> {
        self.update_forward_proxy_settings_with_progress(settings, skip_bootstrap_probe, None)
            .await
    }

    pub async fn update_forward_proxy_settings_with_progress(
        &self,
        settings: ForwardProxySettings,
        skip_bootstrap_probe: bool,
        progress: Option<&ForwardProxyProgressCallback>,
    ) -> Result<ForwardProxySettingsResponse, ProxyError> {
        let normalized = settings.normalized();
        let next_egress_socks5_url = normalized.effective_egress_socks5_url();
        if normalized.egress_socks5_enabled {
            let egress_socks5_url = next_egress_socks5_url.as_ref().ok_or_else(|| {
                ProxyError::Other(
                    "global SOCKS5 relay must be a valid socks5:// or socks5h:// URL".to_string(),
                )
            })?;
            emit_forward_proxy_progress(
                progress,
                ForwardProxyProgressEvent::phase(
                    FORWARD_PROXY_PROGRESS_OPERATION_SAVE,
                    FORWARD_PROXY_PHASE_VALIDATE_EGRESS_SOCKS5,
                    FORWARD_PROXY_LABEL_VALIDATE_EGRESS_SOCKS5,
                ),
            );
            self.validate_forward_proxy_egress_socks5(egress_socks5_url)
                .await?;
        }
        let previous_manager = {
            let manager = self.forward_proxy.lock().await;
            manager.clone()
        };
        let previous_subscription_urls = previous_manager
            .settings
            .subscription_urls
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let added_subscription_urls = normalized
            .subscription_urls
            .iter()
            .filter(|subscription_url| !previous_subscription_urls.contains(*subscription_url))
            .cloned()
            .collect::<Vec<_>>();
        emit_forward_proxy_progress(
            progress,
            ForwardProxyProgressEvent::phase(
                FORWARD_PROXY_PROGRESS_OPERATION_SAVE,
                FORWARD_PROXY_PHASE_SAVE_SETTINGS,
                FORWARD_PROXY_LABEL_SAVE_SETTINGS,
            ),
        );
        forward_proxy::save_forward_proxy_settings(&self.key_store.pool, normalized.clone())
            .await?;
        emit_forward_proxy_progress(
            progress,
            ForwardProxyProgressEvent::phase(
                FORWARD_PROXY_PROGRESS_OPERATION_SAVE,
                FORWARD_PROXY_PHASE_APPLY_EGRESS_SOCKS5,
                FORWARD_PROXY_LABEL_APPLY_EGRESS_SOCKS5,
            ),
        );
        {
            let mut manager = self.forward_proxy.lock().await;
            manager.update_settings_only(normalized.clone());
            {
                let mut xray = self.xray_supervisor.lock().await;
                xray.sync_endpoints(&mut manager.endpoints, next_egress_socks5_url.as_ref())
                    .await?;
            }
            self.sync_forward_proxy_runtime_state(&mut manager).await?;
        }
        let fetched_subscriptions = self
            .fetch_forward_proxy_subscription_map_with_progress(
                &added_subscription_urls,
                next_egress_socks5_url.clone(),
                FORWARD_PROXY_PROGRESS_OPERATION_SAVE,
                progress,
                false,
            )
            .await?;
        let bootstrap_targets = {
            let mut manager = self.forward_proxy.lock().await;
            let bootstrap_targets =
                manager.apply_incremental_settings(normalized.clone(), &fetched_subscriptions);
            {
                let mut xray = self.xray_supervisor.lock().await;
                xray.sync_endpoints(&mut manager.endpoints, next_egress_socks5_url.as_ref())
                    .await?;
            }
            self.sync_forward_proxy_runtime_state(&mut manager).await?;
            bootstrap_targets
                .into_iter()
                .filter(|endpoint| !endpoint.is_direct())
                .collect::<Vec<_>>()
        };
        let geo_metadata_targets = if skip_bootstrap_probe {
            bootstrap_targets
                .iter()
                .filter(|endpoint| endpoint.source == forward_proxy::FORWARD_PROXY_SOURCE_MANUAL)
                .cloned()
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        if skip_bootstrap_probe && !bootstrap_targets.is_empty() {
            emit_forward_proxy_progress(
                progress,
                ForwardProxyProgressEvent::phase_with_progress(
                    FORWARD_PROXY_PROGRESS_OPERATION_SAVE,
                    FORWARD_PROXY_PHASE_BOOTSTRAP_PROBE,
                    FORWARD_PROXY_LABEL_BOOTSTRAP_PROBE,
                    1,
                    1,
                    Some("Skipped after recent validation".to_string()),
                ),
            );
        } else if !bootstrap_targets.is_empty() {
            let bootstrap_total = bootstrap_targets.len();
            for (index, endpoint) in bootstrap_targets.into_iter().enumerate() {
                emit_forward_proxy_progress(
                    progress,
                    ForwardProxyProgressEvent::phase_with_progress(
                        FORWARD_PROXY_PROGRESS_OPERATION_SAVE,
                        FORWARD_PROXY_PHASE_BOOTSTRAP_PROBE,
                        FORWARD_PROXY_LABEL_BOOTSTRAP_PROBE,
                        index + 1,
                        bootstrap_total,
                        Some(endpoint.display_name.clone()),
                    ),
                );
                let _ = self
                    .probe_and_record_forward_proxy_endpoint(
                        &endpoint,
                        "settings_update",
                        None,
                        Duration::from_secs(forward_proxy::FORWARD_PROXY_VALIDATION_TIMEOUT_SECS),
                        None,
                    )
                    .await;
            }
        }
        if !geo_metadata_targets.is_empty() {
            let _ = self
                .resolve_forward_proxy_geo_candidates(
                    &self.api_key_geo_origin,
                    geo_metadata_targets,
                    ForwardProxyGeoRefreshMode::LazyFillMissing,
                )
                .await?;
        }
        self.get_forward_proxy_settings().await
    }

    pub async fn revalidate_forward_proxy_with_progress(
        &self,
        progress: Option<&ForwardProxyProgressCallback>,
    ) -> Result<ForwardProxySettingsResponse, ProxyError> {
        self.refresh_forward_proxy_subscriptions_for_operation(
            FORWARD_PROXY_PROGRESS_OPERATION_REVALIDATE,
            progress,
        )
        .await?;
        let targets = {
            let manager = self.forward_proxy.lock().await;
            manager
                .endpoints
                .iter()
                .filter(|endpoint| !endpoint.is_direct())
                .cloned()
                .collect::<Vec<_>>()
        };
        let total = targets.len();
        for (index, endpoint) in targets.into_iter().enumerate() {
            emit_forward_proxy_progress(
                progress,
                ForwardProxyProgressEvent::phase_with_progress(
                    FORWARD_PROXY_PROGRESS_OPERATION_REVALIDATE,
                    FORWARD_PROXY_PHASE_PROBE_NODES,
                    FORWARD_PROXY_LABEL_PROBE_NODES,
                    index + 1,
                    total,
                    Some(endpoint.display_name.clone()),
                ),
            );
            let _ = self
                .probe_and_record_forward_proxy_endpoint(
                    &endpoint,
                    "revalidate",
                    None,
                    Duration::from_secs(forward_proxy::FORWARD_PROXY_VALIDATION_TIMEOUT_SECS),
                    None,
                )
                .await;
        }
        self.get_forward_proxy_settings().await
    }

    pub async fn validate_forward_proxy_candidates(
        &self,
        proxy_urls: Vec<String>,
        subscription_urls: Vec<String>,
    ) -> Result<ForwardProxyValidationResponse, ProxyError> {
        self.validate_forward_proxy_candidates_with_progress(
            proxy_urls,
            subscription_urls,
            None,
            None,
        )
        .await
    }

    pub async fn validate_forward_proxy_candidates_with_progress(
        &self,
        proxy_urls: Vec<String>,
        subscription_urls: Vec<String>,
        progress: Option<&ForwardProxyProgressCallback>,
        cancellation: Option<&ForwardProxyCancellation>,
    ) -> Result<ForwardProxyValidationResponse, ProxyError> {
        let mut results = Vec::new();
        let mut normalized_values = Vec::new();
        let mut discovered_nodes = 0usize;
        let mut best_latency: Option<f64> = None;
        let probe_url = forward_proxy::derive_probe_url(&self.upstream);
        let normalized_proxy_urls = forward_proxy::normalize_proxy_url_entries(proxy_urls);
        let normalized_subscription_urls =
            forward_proxy::normalize_subscription_entries(subscription_urls);

        if !normalized_proxy_urls.is_empty() {
            emit_forward_proxy_progress(
                progress,
                ForwardProxyProgressEvent::phase(
                    FORWARD_PROXY_PROGRESS_OPERATION_VALIDATE,
                    FORWARD_PROXY_PHASE_PARSE_INPUT,
                    FORWARD_PROXY_LABEL_PARSE_INPUT,
                ),
            );
        }

        let manual_total = normalized_proxy_urls.len();
        for (index, raw) in normalized_proxy_urls.into_iter().enumerate() {
            ensure_forward_proxy_not_cancelled(cancellation)?;
            let Some(parsed) = forward_proxy::parse_forward_proxy_entry(&raw) else {
                results.push(ForwardProxyValidationProbeResult {
                    value: raw.clone(),
                    normalized_value: None,
                    ok: false,
                    discovered_nodes: Some(0),
                    latency_ms: None,
                    error_code: Some("proxy_invalid".to_string()),
                    message: "unsupported proxy url or unsupported scheme".to_string(),
                    nodes: Vec::new(),
                });
                continue;
            };
            let endpoint = forward_proxy::ForwardProxyEndpoint::new_manual(
                format!(
                    "__validate_proxy__{:016x}",
                    forward_proxy::stable_hash_u64(&parsed.normalized)
                ),
                parsed.display_name.clone(),
                parsed.protocol,
                parsed.endpoint_url.clone(),
                Some(parsed.normalized.clone()),
            );
            emit_forward_proxy_progress(
                progress,
                ForwardProxyProgressEvent::phase_with_progress(
                    FORWARD_PROXY_PROGRESS_OPERATION_VALIDATE,
                    FORWARD_PROXY_PHASE_PROBE_NODES,
                    FORWARD_PROXY_LABEL_PROBE_NODES,
                    index + 1,
                    manual_total,
                    Some(endpoint.display_name.clone()),
                ),
            );
            match self
                .probe_forward_proxy_endpoint(
                    &endpoint,
                    Duration::from_secs(forward_proxy::FORWARD_PROXY_VALIDATION_TIMEOUT_SECS),
                    &probe_url,
                    cancellation,
                )
                .await
            {
                Ok(latency_ms) => {
                    let trace = self
                        .fetch_forward_proxy_trace(
                            &endpoint,
                            Duration::from_millis(FORWARD_PROXY_TRACE_TIMEOUT_MS),
                            cancellation,
                        )
                        .await;
                    let (ip, location) = trace
                        .map(|(ip, location)| (Some(ip), Some(location)))
                        .unwrap_or((None, None));
                    normalized_values.push(parsed.normalized.clone());
                    discovered_nodes += 1;
                    best_latency =
                        Some(best_latency.map_or(latency_ms, |current| current.min(latency_ms)));
                    results.push(ForwardProxyValidationProbeResult {
                        value: raw,
                        normalized_value: Some(parsed.normalized),
                        ok: true,
                        discovered_nodes: Some(1),
                        latency_ms: Some(latency_ms),
                        error_code: None,
                        message: "proxy validation succeeded".to_string(),
                        nodes: vec![ForwardProxyValidationNodeResult {
                            display_name: endpoint.display_name.clone(),
                            protocol: endpoint.protocol.as_str().to_string(),
                            ok: true,
                            latency_ms: Some(latency_ms),
                            ip,
                            location,
                            message: None,
                        }],
                    });
                }
                Err(err) => {
                    results.push(ForwardProxyValidationProbeResult {
                        value: raw,
                        normalized_value: Some(parsed.normalized),
                        ok: false,
                        discovered_nodes: Some(1),
                        latency_ms: None,
                        error_code: Some(map_forward_proxy_validation_error_code(&err)),
                        message: err.to_string(),
                        nodes: vec![ForwardProxyValidationNodeResult {
                            display_name: endpoint.display_name.clone(),
                            protocol: endpoint.protocol.as_str().to_string(),
                            ok: false,
                            latency_ms: None,
                            ip: None,
                            location: None,
                            message: Some(err.to_string()),
                        }],
                    });
                }
            }
        }

        if !normalized_subscription_urls.is_empty() {
            emit_forward_proxy_progress(
                progress,
                ForwardProxyProgressEvent::phase(
                    FORWARD_PROXY_PROGRESS_OPERATION_VALIDATE,
                    FORWARD_PROXY_PHASE_NORMALIZE_INPUT,
                    FORWARD_PROXY_LABEL_NORMALIZE_INPUT,
                ),
            );
        }

        for subscription_url in normalized_subscription_urls {
            ensure_forward_proxy_not_cancelled(cancellation)?;
            match self
                .validate_forward_proxy_subscription_with_progress(
                    &subscription_url,
                    progress,
                    cancellation,
                )
                .await
            {
                Ok((count, latency_ms, mut normalized, nodes)) => {
                    discovered_nodes += count;
                    best_latency =
                        Some(best_latency.map_or(latency_ms, |current| current.min(latency_ms)));
                    normalized_values.push(subscription_url.clone());
                    normalized_values.append(&mut normalized);
                    results.push(ForwardProxyValidationProbeResult {
                        value: subscription_url.clone(),
                        normalized_value: Some(subscription_url),
                        ok: true,
                        discovered_nodes: Some(count),
                        latency_ms: Some(latency_ms),
                        error_code: None,
                        message: "subscription validation succeeded".to_string(),
                        nodes,
                    });
                }
                Err(err) => {
                    results.push(ForwardProxyValidationProbeResult {
                        value: subscription_url.clone(),
                        normalized_value: Some(subscription_url),
                        ok: false,
                        discovered_nodes: Some(0),
                        latency_ms: None,
                        error_code: Some(map_forward_proxy_validation_error_code(&err)),
                        message: err.to_string(),
                        nodes: Vec::new(),
                    });
                }
            }
        }

        emit_forward_proxy_progress(
            progress,
            ForwardProxyProgressEvent::phase(
                FORWARD_PROXY_PROGRESS_OPERATION_VALIDATE,
                FORWARD_PROXY_PHASE_GENERATE_RESULT,
                FORWARD_PROXY_LABEL_GENERATE_RESULT,
            ),
        );
        normalized_values.sort();
        normalized_values.dedup();
        let ok = results.iter().any(|result| result.ok);
        let first_error =
            results
                .iter()
                .find(|result| !result.ok)
                .map(|result| ForwardProxyValidationError {
                    code: result
                        .error_code
                        .clone()
                        .unwrap_or_else(|| "validation_failed".to_string()),
                    message: result.message.clone(),
                });

        Ok(ForwardProxyValidationResponse {
            ok,
            normalized_values,
            discovered_nodes,
            latency_ms: best_latency,
            results,
            first_error,
        })
    }

    pub(crate) async fn validate_forward_proxy_subscription_with_progress(
        &self,
        subscription_url: &str,
        progress: Option<&ForwardProxyProgressCallback>,
        cancellation: Option<&ForwardProxyCancellation>,
    ) -> Result<
        (
            usize,
            f64,
            Vec<String>,
            Vec<ForwardProxyValidationNodeResult>,
        ),
        ProxyError,
    > {
        ensure_forward_proxy_not_cancelled(cancellation)?;
        let validation_timeout =
            Duration::from_secs(forward_proxy::FORWARD_PROXY_SUBSCRIPTION_VALIDATION_TIMEOUT_SECS);
        let validation_started = Instant::now();
        let normalized_subscription =
            forward_proxy::normalize_subscription_entries(vec![subscription_url.to_string()])
                .into_iter()
                .next()
                .ok_or_else(|| {
                    ProxyError::Other("subscription url must be a valid http/https url".to_string())
                })?;
        emit_forward_proxy_progress(
            progress,
            ForwardProxyProgressEvent::phase(
                FORWARD_PROXY_PROGRESS_OPERATION_VALIDATE,
                FORWARD_PROXY_PHASE_FETCH_SUBSCRIPTION,
                FORWARD_PROXY_LABEL_FETCH_SUBSCRIPTION,
            ),
        );
        let egress_socks5_url = self.current_forward_proxy_egress_socks5_url().await;
        let subscription_client = self
            .forward_proxy_clients
            .direct_client_via_egress(egress_socks5_url.as_ref())
            .await?;
        let urls = run_forward_proxy_future_with_cancel(
            cancellation,
            forward_proxy::fetch_subscription_proxy_urls_with_validation_budget(
                &subscription_client,
                &normalized_subscription,
                validation_timeout,
                validation_started,
            ),
        )
        .await?
        .map_err(|err| {
            ProxyError::Other(format!(
                "failed to fetch or decode subscription payload: {err}"
            ))
        })?;
        if urls.is_empty() {
            return Err(ProxyError::Other(
                "subscription resolved zero proxy entries".to_string(),
            ));
        }
        let endpoints = forward_proxy::normalize_subscription_endpoints_from_urls(
            &urls,
            &normalized_subscription,
        );
        if endpoints.is_empty() {
            return Err(ProxyError::Other(
                "subscription contains no supported proxy entries".to_string(),
            ));
        }
        emit_forward_proxy_progress(
            progress,
            ForwardProxyProgressEvent::nodes(
                FORWARD_PROXY_PROGRESS_OPERATION_VALIDATE,
                endpoints
                    .iter()
                    .map(|endpoint| ForwardProxyProgressNodeState {
                        node_key: endpoint.key.clone(),
                        display_name: endpoint.display_name.clone(),
                        protocol: endpoint.protocol.as_str().to_string(),
                        status: "pending",
                        ok: None,
                        latency_ms: None,
                        ip: None,
                        location: None,
                        message: None,
                    })
                    .collect(),
            ),
        );
        let probe_url = forward_proxy::derive_probe_url(&self.upstream);
        let mut last_error: Option<ProxyError> = None;
        let probe_total = endpoints.len();
        let validation_timeout =
            Duration::from_secs(forward_proxy::FORWARD_PROXY_VALIDATION_TIMEOUT_SECS);
        let probe_sample_total = 1usize;
        let mut completed_nodes = 0usize;
        let mut latency_samples = vec![Vec::<f64>::new(); probe_total];
        let mut latest_latency = vec![None; probe_total];
        let mut last_messages: Vec<Option<String>> = vec![None; probe_total];
        let mut ips: Vec<Option<String>> = vec![None; probe_total];
        let mut locations: Vec<Option<String>> = vec![None; probe_total];
        let mut temporary_xray_keys = Vec::with_capacity(probe_total);
        let validation_result = async {
            let mut resolved_endpoints = Vec::with_capacity(probe_total);

            for endpoint in &endpoints {
                ensure_forward_proxy_not_cancelled(cancellation)?;
                let (resolved_endpoint, temporary_xray_key) = self
                    .resolve_forward_proxy_validation_endpoint(endpoint)
                    .await?;
                if let Some(temp_key) = temporary_xray_key {
                    temporary_xray_keys.push(temp_key);
                }
                resolved_endpoints.push(resolved_endpoint);
            }

            for round in 0..probe_sample_total {
                ensure_forward_proxy_not_cancelled(cancellation)?;
                let probe_endpoints = resolved_endpoints.clone();
                let mut probe_stream =
                    futures_util::stream::iter(probe_endpoints.into_iter().enumerate())
                        .map(|(index, endpoint)| {
                            let probe_url = probe_url.clone();
                            async move {
                                emit_forward_proxy_progress(
                                    progress,
                                    ForwardProxyProgressEvent::node(
                                        FORWARD_PROXY_PROGRESS_OPERATION_VALIDATE,
                                        ForwardProxyProgressNodeState {
                                            node_key: endpoint.key.clone(),
                                            display_name: endpoint.display_name.clone(),
                                            protocol: endpoint.protocol.as_str().to_string(),
                                            status: "probing",
                                            ok: None,
                                            latency_ms: None,
                                            ip: None,
                                            location: None,
                                            message: None,
                                        },
                                    ),
                                );

                                let result = self
                                    .probe_forward_proxy_endpoint(
                                        &endpoint,
                                        validation_timeout,
                                        &probe_url,
                                        cancellation,
                                    )
                                    .await;
                                (index, endpoint, result)
                            }
                        })
                        .buffer_unordered(3);

                while let Some((index, endpoint, result)) =
                    run_forward_proxy_future_with_cancel(cancellation, probe_stream.next()).await?
                {
                    match result {
                        Ok(latency_ms) => {
                            latency_samples[index].push(latency_ms);
                            let median_latency = compute_latency_median(&latency_samples[index])
                                .unwrap_or(latency_ms);
                            latest_latency[index] = Some(median_latency);
                            if (ips[index].is_none() || locations[index].is_none())
                                && let Some((ip, location)) = self
                                    .fetch_forward_proxy_trace(
                                        &endpoint,
                                        Duration::from_millis(FORWARD_PROXY_TRACE_TIMEOUT_MS),
                                        cancellation,
                                    )
                                    .await
                            {
                                ips[index] = Some(ip);
                                locations[index] = Some(location);
                            }
                            let is_final_sample = round + 1 == probe_sample_total;
                            if is_final_sample {
                                completed_nodes += 1;
                            }
                            emit_forward_proxy_progress(
                                progress,
                                ForwardProxyProgressEvent::node(
                                    FORWARD_PROXY_PROGRESS_OPERATION_VALIDATE,
                                    ForwardProxyProgressNodeState {
                                        node_key: endpoint.key.clone(),
                                        display_name: endpoint.display_name.clone(),
                                        protocol: endpoint.protocol.as_str().to_string(),
                                        status: if is_final_sample { "ok" } else { "probing" },
                                        ok: if is_final_sample { Some(true) } else { None },
                                        latency_ms: Some(median_latency),
                                        ip: ips[index].clone(),
                                        location: locations[index].clone(),
                                        message: None,
                                    },
                                ),
                            );
                            if is_final_sample {
                                emit_forward_proxy_progress(
                                    progress,
                                    ForwardProxyProgressEvent::phase_with_progress(
                                        FORWARD_PROXY_PROGRESS_OPERATION_VALIDATE,
                                        FORWARD_PROXY_PHASE_PROBE_NODES,
                                        FORWARD_PROXY_LABEL_PROBE_NODES,
                                        completed_nodes,
                                        probe_total,
                                        Some(endpoint.display_name.clone()),
                                    ),
                                );
                            }
                        }
                        Err(err) => {
                            let message = err.to_string();
                            last_messages[index] = Some(message.clone());
                            last_error = Some(err);
                            let is_final_sample = round + 1 == probe_sample_total
                                && latency_samples[index].is_empty();
                            if is_final_sample {
                                completed_nodes += 1;
                            }
                            emit_forward_proxy_progress(
                                progress,
                                ForwardProxyProgressEvent::node(
                                    FORWARD_PROXY_PROGRESS_OPERATION_VALIDATE,
                                    ForwardProxyProgressNodeState {
                                        node_key: endpoint.key.clone(),
                                        display_name: endpoint.display_name.clone(),
                                        protocol: endpoint.protocol.as_str().to_string(),
                                        status: if is_final_sample { "failed" } else { "probing" },
                                        ok: if is_final_sample { Some(false) } else { None },
                                        latency_ms: latest_latency[index],
                                        ip: ips[index].clone(),
                                        location: locations[index].clone(),
                                        message: Some(message),
                                    },
                                ),
                            );
                            if is_final_sample {
                                emit_forward_proxy_progress(
                                    progress,
                                    ForwardProxyProgressEvent::phase_with_progress(
                                        FORWARD_PROXY_PROGRESS_OPERATION_VALIDATE,
                                        FORWARD_PROXY_PHASE_PROBE_NODES,
                                        FORWARD_PROXY_LABEL_PROBE_NODES,
                                        completed_nodes,
                                        probe_total,
                                        Some(endpoint.display_name.clone()),
                                    ),
                                );
                            }
                        }
                    }
                }
            }

            Ok::<(), ProxyError>(())
        }
        .await;
        for temp_key in temporary_xray_keys {
            self.cleanup_forward_proxy_validation_endpoint(Some(temp_key))
                .await;
        }
        validation_result?;
        let mut best_latency: Option<f64> = None;
        let probed_nodes = endpoints
            .iter()
            .enumerate()
            .map(|(index, endpoint)| {
                if let Some(median_latency) = compute_latency_median(&latency_samples[index]) {
                    best_latency = Some(
                        best_latency.map_or(median_latency, |current| current.min(median_latency)),
                    );
                    ForwardProxyValidationNodeResult {
                        display_name: endpoint.display_name.clone(),
                        protocol: endpoint.protocol.as_str().to_string(),
                        ok: true,
                        latency_ms: Some(median_latency),
                        ip: ips[index].clone(),
                        location: locations[index].clone(),
                        message: None,
                    }
                } else {
                    ForwardProxyValidationNodeResult {
                        display_name: endpoint.display_name.clone(),
                        protocol: endpoint.protocol.as_str().to_string(),
                        ok: false,
                        latency_ms: None,
                        ip: ips[index].clone(),
                        location: locations[index].clone(),
                        message: last_messages[index].clone(),
                    }
                }
            })
            .collect::<Vec<_>>();
        let Some(latency_ms) = best_latency else {
            if let Some(err) = last_error {
                return Err(ProxyError::Other(format!(
                    "subscription proxy probe failed: {err}; no entry passed validation"
                )));
            }
            return Err(ProxyError::Other(
                "no subscription proxy entry passed validation".to_string(),
            ));
        };
        Ok((
            endpoints.len(),
            latency_ms,
            endpoints
                .into_iter()
                .filter_map(|endpoint| endpoint.raw_url)
                .collect(),
            probed_nodes,
        ))
    }

    pub async fn refresh_forward_proxy_subscriptions(&self) -> Result<(), ProxyError> {
        self.refresh_forward_proxy_subscriptions_with_progress(None)
            .await
    }

    pub async fn refresh_forward_proxy_subscriptions_with_progress(
        &self,
        progress: Option<&ForwardProxyProgressCallback>,
    ) -> Result<(), ProxyError> {
        self.refresh_forward_proxy_subscriptions_for_operation(
            FORWARD_PROXY_PROGRESS_OPERATION_SAVE,
            progress,
        )
        .await
    }

    pub(crate) async fn refresh_forward_proxy_subscriptions_for_operation(
        &self,
        operation: &'static str,
        progress: Option<&ForwardProxyProgressCallback>,
    ) -> Result<(), ProxyError> {
        let settings = {
            let manager = self.forward_proxy.lock().await;
            manager.settings.clone()
        };
        let egress_socks5_url = settings.effective_egress_socks5_url();
        let subscription_urls = self
            .fetch_forward_proxy_subscription_map_with_progress(
                &settings.subscription_urls,
                egress_socks5_url.clone(),
                operation,
                progress,
                true,
            )
            .await?;

        let mut manager = self.forward_proxy.lock().await;
        manager.apply_subscription_refresh(&subscription_urls);
        {
            let mut xray = self.xray_supervisor.lock().await;
            xray.sync_endpoints(&mut manager.endpoints, egress_socks5_url.as_ref())
                .await?;
        }
        self.sync_forward_proxy_runtime_state(&mut manager).await?;
        Ok(())
    }

    pub(crate) async fn fetch_forward_proxy_subscription_map_with_progress(
        &self,
        subscription_urls: &[String],
        egress_socks5_url: Option<Url>,
        operation: &'static str,
        progress: Option<&ForwardProxyProgressCallback>,
        fail_when_all_fail: bool,
    ) -> Result<HashMap<String, Vec<String>>, ProxyError> {
        let mut fetched = HashMap::new();
        let mut fetched_any_subscription = false;
        let subscription_client = self
            .forward_proxy_clients
            .direct_client_via_egress(egress_socks5_url.as_ref())
            .await?;
        let total = subscription_urls.len();
        for (index, subscription_url) in subscription_urls.iter().enumerate() {
            emit_forward_proxy_progress(
                progress,
                ForwardProxyProgressEvent::phase_with_progress(
                    operation,
                    FORWARD_PROXY_PHASE_REFRESH_SUBSCRIPTION,
                    FORWARD_PROXY_LABEL_REFRESH_SUBSCRIPTION,
                    index + 1,
                    total,
                    Some(subscription_url.clone()),
                ),
            );
            match forward_proxy::fetch_subscription_proxy_urls(
                &subscription_client,
                subscription_url,
                Duration::from_secs(
                    forward_proxy::FORWARD_PROXY_SUBSCRIPTION_VALIDATION_TIMEOUT_SECS,
                ),
            )
            .await
            {
                Ok(urls) => {
                    fetched_any_subscription = true;
                    fetched.insert(subscription_url.clone(), urls);
                }
                Err(err) => {
                    eprintln!(
                        "failed to refresh forward proxy subscription {subscription_url}: {err}"
                    );
                }
            }
        }

        if fail_when_all_fail && !subscription_urls.is_empty() && !fetched_any_subscription {
            return Err(ProxyError::Other(
                "all forward proxy subscriptions failed to refresh".to_string(),
            ));
        }

        Ok(fetched)
    }

    pub(crate) async fn sync_forward_proxy_runtime_state(
        &self,
        manager: &mut forward_proxy::ForwardProxyManager,
    ) -> Result<(), ProxyError> {
        let endpoints = manager.endpoints.clone();
        for endpoint in &endpoints {
            if let Some(runtime) = manager.runtime.get_mut(&endpoint.key) {
                runtime.source = endpoint.source.clone();
                runtime.kind = endpoint.protocol.as_str().to_string();
                runtime.endpoint_url = endpoint
                    .endpoint_url
                    .as_ref()
                    .map(Url::to_string)
                    .or_else(|| endpoint.raw_url.clone());
                runtime.available = endpoint.is_selectable();
                if endpoint.is_direct() || endpoint.is_selectable() {
                    runtime.last_error = None;
                } else {
                    runtime.last_error = Some("xray_missing".to_string());
                }
            }
        }
        forward_proxy::sync_manager_runtime_to_store(&self.key_store, manager).await
    }

    pub async fn maybe_run_forward_proxy_maintenance(&self) -> Result<(), ProxyError> {
        let should_refresh = {
            let manager = self.forward_proxy.lock().await;
            manager.should_refresh_subscriptions()
        };
        if should_refresh {
            self.refresh_forward_proxy_subscriptions().await?;
        }
        let probe_candidate = {
            let mut manager = self.forward_proxy.lock().await;
            manager
                .mark_probe_started()
                .and_then(|selected| manager.endpoint_by_key(&selected.key))
        };
        if let Some(endpoint) = probe_candidate {
            let probe_url = forward_proxy::derive_probe_url(&self.upstream);
            let probe_result = self
                .probe_forward_proxy_endpoint(
                    &endpoint,
                    Duration::from_secs(forward_proxy::FORWARD_PROXY_VALIDATION_TIMEOUT_SECS),
                    &probe_url,
                    None,
                )
                .await;
            match probe_result {
                Ok(latency_ms) => {
                    let _ = self
                        .record_forward_proxy_attempt_inner(
                            &endpoint.key,
                            true,
                            Some(latency_ms),
                            None,
                            true,
                        )
                        .await;
                }
                Err(err) => {
                    let failure_kind = map_forward_proxy_validation_error_code(&err);
                    let _ = self
                        .record_forward_proxy_attempt_inner(
                            &endpoint.key,
                            false,
                            None,
                            Some(failure_kind.as_str()),
                            true,
                        )
                        .await;
                }
            }
            let mut manager = self.forward_proxy.lock().await;
            manager.mark_probe_finished();
        }
        Ok(())
    }

    pub(crate) async fn resolve_forward_proxy_validation_endpoint(
        &self,
        endpoint: &forward_proxy::ForwardProxyEndpoint,
    ) -> Result<(forward_proxy::ForwardProxyEndpoint, Option<String>), ProxyError> {
        if endpoint.uses_local_relay {
            return Ok((endpoint.clone(), None));
        }
        let egress_socks5_url = self.current_forward_proxy_egress_socks5_url().await;
        let mut temporary_xray_key = None;
        let resolved = if endpoint.needs_local_relay(egress_socks5_url.as_ref()) {
            let raw_url = endpoint
                .raw_url
                .as_deref()
                .or_else(|| endpoint.endpoint_url.as_ref().map(Url::as_str))
                .ok_or_else(|| {
                    ProxyError::Other("xray proxy validation requires raw proxy url".to_string())
                })?;
            let validate_key = format!(
                "__validate_xray__{:016x}",
                forward_proxy::stable_hash_u64(&format!(
                    "{}|{}",
                    raw_url,
                    egress_socks5_url
                        .as_ref()
                        .map(Url::as_str)
                        .unwrap_or_default()
                ))
            );
            let mut validate_endpoint = forward_proxy::ForwardProxyEndpoint::new_manual(
                validate_key.clone(),
                endpoint.display_name.clone(),
                endpoint.protocol,
                endpoint.endpoint_url.clone(),
                Some(raw_url.to_string()),
            );
            validate_endpoint.source = endpoint.source.clone();
            validate_endpoint.manual_present = endpoint.manual_present;
            validate_endpoint.subscription_sources = endpoint.subscription_sources.clone();
            let route_url = self
                .xray_supervisor
                .lock()
                .await
                .ensure_instance(&validate_endpoint, egress_socks5_url.as_ref())
                .await?;
            temporary_xray_key = Some(validate_key);
            let mut resolved = endpoint.clone();
            resolved.endpoint_url = Some(route_url);
            resolved.uses_local_relay = true;
            resolved
        } else {
            endpoint.clone()
        };
        Ok((resolved, temporary_xray_key))
    }

    pub(crate) async fn cleanup_forward_proxy_validation_endpoint(
        &self,
        temporary_xray_key: Option<String>,
    ) {
        if let Some(temp_key) = temporary_xray_key {
            self.xray_supervisor
                .lock()
                .await
                .remove_instance(&temp_key)
                .await;
        }
    }

    pub(crate) async fn probe_forward_proxy_endpoint(
        &self,
        endpoint: &forward_proxy::ForwardProxyEndpoint,
        timeout: Duration,
        probe_url: &Url,
        cancellation: Option<&ForwardProxyCancellation>,
    ) -> Result<f64, ProxyError> {
        ensure_forward_proxy_not_cancelled(cancellation)?;
        let (resolved, temporary_xray_key) = self
            .resolve_forward_proxy_validation_endpoint(endpoint)
            .await?;
        let result = run_forward_proxy_future_with_cancel(
            cancellation,
            forward_proxy::probe_forward_proxy_endpoint(
                &self.forward_proxy_clients,
                &resolved,
                probe_url,
                timeout,
            ),
        )
        .await?;
        self.cleanup_forward_proxy_validation_endpoint(temporary_xray_key)
            .await;
        result
    }

    pub(crate) async fn fetch_forward_proxy_trace(
        &self,
        endpoint: &forward_proxy::ForwardProxyEndpoint,
        timeout: Duration,
        cancellation: Option<&ForwardProxyCancellation>,
    ) -> Option<(String, String)> {
        if timeout.is_zero() {
            return None;
        }
        if ensure_forward_proxy_not_cancelled(cancellation).is_err() {
            return None;
        }
        #[cfg(test)]
        if let Some(trace) = self.forward_proxy_trace_for_test(endpoint).await {
            return Some(trace);
        }
        let trace_url = self.forward_proxy_trace_url.clone();
        let (resolved, temporary_xray_key) = self
            .resolve_forward_proxy_validation_endpoint(endpoint)
            .await
            .ok()?;
        let result = run_forward_proxy_future_with_cancel(cancellation, async {
            let client = self
                .forward_proxy_clients
                .client_for(resolved.endpoint_url.as_ref())
                .await
                .ok()?;
            tokio::time::timeout(timeout, async {
                let response = client.get(trace_url).send().await.ok()?;
                if !response.status().is_success() {
                    return None;
                }
                let body = response.text().await.ok()?;
                parse_forward_proxy_trace_response(&body)
            })
            .await
            .ok()
            .flatten()
        })
        .await
        .ok()
        .flatten();
        self.cleanup_forward_proxy_validation_endpoint(temporary_xray_key)
            .await;
        result
    }

    #[cfg(test)]
    pub(crate) async fn forward_proxy_trace_for_test(
        &self,
        endpoint: &forward_proxy::ForwardProxyEndpoint,
    ) -> Option<(String, String)> {
        if let Some(trace) = self
            .forward_proxy_trace_overrides
            .lock()
            .await
            .get(&endpoint.key)
            .cloned()
        {
            return Some(trace);
        }
        forward_proxy::endpoint_host(endpoint)
            .and_then(|host| normalize_ip_string(&host))
            .filter(|ip| is_global_geo_ip(ip))
            .map(|ip| {
                let location = format!("TEST / {ip}");
                (ip, location)
            })
    }

    #[cfg(test)]
    pub(crate) async fn set_forward_proxy_trace_override_for_test(
        &self,
        proxy_key: impl Into<String>,
        ip: impl Into<String>,
        location: impl Into<String>,
    ) {
        self.forward_proxy_trace_overrides
            .lock()
            .await
            .insert(proxy_key.into(), (ip.into(), location.into()));
    }

    pub(crate) async fn probe_and_record_forward_proxy_endpoint(
        &self,
        endpoint: &forward_proxy::ForwardProxyEndpoint,
        request_kind: &str,
        api_key_id: Option<&str>,
        timeout: Duration,
        cancellation: Option<&ForwardProxyCancellation>,
    ) -> Result<f64, ProxyError> {
        let probe_url = forward_proxy::derive_probe_url(&self.upstream);
        let result = self
            .probe_forward_proxy_endpoint(endpoint, timeout, &probe_url, cancellation)
            .await;
        match result {
            Ok(latency_ms) => {
                self.record_forward_proxy_attempt(
                    endpoint.key.as_str(),
                    api_key_id,
                    request_kind,
                    true,
                    Some(latency_ms),
                    None,
                )
                .await?;
                Ok(latency_ms)
            }
            Err(err) => {
                let error_code = map_forward_proxy_validation_error_code(&err);
                self.record_forward_proxy_attempt(
                    endpoint.key.as_str(),
                    api_key_id,
                    request_kind,
                    false,
                    None,
                    Some(error_code.as_str()),
                )
                .await?;
                Err(err)
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) async fn load_proxy_affinity_record(
        &self,
        api_key_id: &str,
    ) -> Result<forward_proxy::ForwardProxyAffinityRecord, ProxyError> {
        Ok(self
            .load_cached_proxy_affinity_record(api_key_id)
            .await?
            .record)
    }

    async fn load_cached_proxy_affinity_record(
        &self,
        api_key_id: &str,
    ) -> Result<CachedForwardProxyAffinityRecord, ProxyError> {
        {
            let cache = self.forward_proxy_affinity.lock().await;
            if let Some(record) = cache.get(api_key_id) {
                return Ok(record.clone());
            }
        }
        let persisted =
            forward_proxy::load_forward_proxy_key_affinity(&self.key_store.pool, api_key_id)
                .await?;
        let record = CachedForwardProxyAffinityRecord {
            record: persisted.clone().unwrap_or_default(),
            has_persisted_row: persisted.is_some(),
        };
        let mut cache = self.forward_proxy_affinity.lock().await;
        cache.insert(api_key_id.to_string(), record.clone());
        Ok(record)
    }

    pub(crate) async fn store_proxy_affinity_record(
        &self,
        api_key_id: &str,
        record: forward_proxy::ForwardProxyAffinityRecord,
    ) -> Result<(), ProxyError> {
        forward_proxy::save_forward_proxy_key_affinity(&self.key_store.pool, api_key_id, &record)
            .await?;
        let mut cache = self.forward_proxy_affinity.lock().await;
        cache.insert(
            api_key_id.to_string(),
            CachedForwardProxyAffinityRecord {
                record,
                has_persisted_row: true,
            },
        );
        Ok(())
    }

    pub(crate) async fn remove_proxy_affinity_record_from_cache(&self, api_key_id: &str) {
        let mut cache = self.forward_proxy_affinity.lock().await;
        cache.remove(api_key_id);
    }

    pub(crate) async fn load_api_key_registration_metadata(
        &self,
        api_key_id: &str,
    ) -> Result<(Option<String>, Option<String>), ProxyError> {
        let row = sqlx::query_as::<_, (Option<String>, Option<String>)>(
            "SELECT registration_ip, registration_region FROM api_keys WHERE id = ? LIMIT 1",
        )
        .bind(api_key_id)
        .fetch_optional(&self.key_store.pool)
        .await?;
        Ok(row.unwrap_or((None, None)))
    }

    pub(crate) async fn rank_registration_aware_candidates(
        &self,
        subject: &str,
        affinity: RegistrationAffinityContext<'_>,
        exclude: &HashSet<String>,
        allow_direct: bool,
        limit: usize,
    ) -> Result<Vec<forward_proxy::ForwardProxyEndpoint>, ProxyError> {
        let ranked = {
            let mut manager = self.forward_proxy.lock().await;
            manager.ensure_non_zero_weight();
            manager.rank_candidates_for_subject(subject, exclude, allow_direct, limit)
        };
        let normalized_registration_ip = affinity.registration_ip.and_then(normalize_ip_string);
        let normalized_registration_region = affinity
            .registration_region
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if normalized_registration_ip.is_none() && normalized_registration_region.is_none() {
            return Ok(ranked);
        }

        let mut direct = Vec::new();
        let mut non_direct = Vec::new();
        for endpoint in ranked {
            if endpoint.is_direct() {
                direct.push(endpoint);
            } else {
                non_direct.push(endpoint);
            }
        }
        if non_direct.is_empty() {
            return Ok(direct);
        }

        let geo_candidates = self
            .resolve_forward_proxy_geo_candidates(
                affinity.geo_origin,
                non_direct.clone(),
                ForwardProxyGeoRefreshMode::LazyFillMissing,
            )
            .await?;
        let mut exact_keys = HashSet::new();
        let mut region_keys = HashSet::new();
        for candidate in geo_candidates {
            if normalized_registration_ip
                .as_ref()
                .is_some_and(|registration_ip| {
                    candidate.host_ips.iter().any(|ip| ip == registration_ip)
                })
            {
                exact_keys.insert(candidate.endpoint.key.clone());
            }
            if normalized_registration_region
                .as_ref()
                .is_some_and(|registration_region| {
                    candidate
                        .regions
                        .iter()
                        .any(|region| region == registration_region)
                })
            {
                region_keys.insert(candidate.endpoint.key.clone());
            }
        }

        let mut ordered = Vec::new();
        let mut seen = HashSet::new();
        for endpoint in &non_direct {
            if exact_keys.contains(&endpoint.key) && seen.insert(endpoint.key.clone()) {
                ordered.push(endpoint.clone());
            }
        }
        for endpoint in &non_direct {
            if region_keys.contains(&endpoint.key) && seen.insert(endpoint.key.clone()) {
                ordered.push(endpoint.clone());
            }
        }
        for endpoint in non_direct {
            if seen.insert(endpoint.key.clone()) {
                ordered.push(endpoint);
            }
        }
        if allow_direct {
            ordered.extend(direct);
        }
        Ok(ordered)
    }

    async fn load_proxy_affinity_state(
        &self,
        api_key_id: &str,
    ) -> Result<LoadedProxyAffinityState, ProxyError> {
        let cached = self.load_cached_proxy_affinity_record(api_key_id).await?;
        let (registration_ip, registration_region) =
            self.load_api_key_registration_metadata(api_key_id).await?;
        let has_registration_metadata = registration_ip.is_some() || registration_region.is_some();
        let has_explicit_empty_marker = !has_registration_metadata
            && cached.has_persisted_row
            && cached.record.primary_proxy_key.is_none()
            && cached.record.secondary_proxy_key.is_none();
        Ok(LoadedProxyAffinityState {
            record: cached.record,
            registration_ip,
            registration_region,
            has_explicit_empty_marker,
        })
    }

    pub(crate) async fn resolve_proxy_affinity_record(
        &self,
        api_key_id: &str,
        persist: bool,
    ) -> Result<forward_proxy::ForwardProxyAffinityRecord, ProxyError> {
        let state = self.load_proxy_affinity_state(api_key_id).await?;
        let record = self
            .reconcile_proxy_affinity_record_with_state(api_key_id, state)
            .await?;
        if persist {
            self.store_proxy_affinity_record(api_key_id, record.clone())
                .await?;
        }
        Ok(record)
    }

    async fn reconcile_proxy_affinity_record_with_state(
        &self,
        api_key_id: &str,
        state: LoadedProxyAffinityState,
    ) -> Result<forward_proxy::ForwardProxyAffinityRecord, ProxyError> {
        let mut record = state.record;
        let registration_ip = state.registration_ip;
        let registration_region = state.registration_region;
        let has_registration_metadata = registration_ip.is_some() || registration_region.is_some();
        let now = Utc::now().timestamp();
        {
            let mut manager = self.forward_proxy.lock().await;
            manager.ensure_non_zero_weight();

            let is_selectable_endpoint =
                |proxy_key: &str,
                 manager: &forward_proxy::ForwardProxyManager,
                 allow_direct_primary: bool| {
                    let Some(endpoint) = manager.endpoint(proxy_key) else {
                        return false;
                    };
                    if endpoint.is_direct() && !allow_direct_primary {
                        return false;
                    }
                    endpoint.is_selectable() && manager.runtime(proxy_key).is_some()
                };
            let is_available = |proxy_key: &str,
                                manager: &forward_proxy::ForwardProxyManager,
                                allow_direct_primary: bool| {
                if !is_selectable_endpoint(proxy_key, manager, allow_direct_primary) {
                    return false;
                }
                manager
                    .runtime(proxy_key)
                    .is_some_and(|runtime| runtime.available && runtime.weight > 0.0)
            };
            let keep_primary = |proxy_key: &str, manager: &forward_proxy::ForwardProxyManager| {
                if has_registration_metadata {
                    is_available(proxy_key, manager, true)
                } else {
                    is_selectable_endpoint(proxy_key, manager, true)
                }
            };

            if let Some(primary) = record.primary_proxy_key.as_deref()
                && !keep_primary(primary, &manager)
            {
                record.primary_proxy_key = None;
            }
            if let Some(secondary) = record.secondary_proxy_key.as_deref()
                && !is_available(secondary, &manager, true)
            {
                record.secondary_proxy_key = None;
            }
        }
        if record.primary_proxy_key == record.secondary_proxy_key {
            record.secondary_proxy_key = None;
        }

        if record.primary_proxy_key.is_none() {
            let exclude = HashSet::new();
            if let Some(primary) = self
                .rank_registration_aware_candidates(
                    &format!("{api_key_id}:primary"),
                    RegistrationAffinityContext {
                        geo_origin: &self.api_key_geo_origin,
                        registration_ip: registration_ip.as_deref(),
                        registration_region: registration_region.as_deref(),
                    },
                    &exclude,
                    true,
                    forward_proxy::FORWARD_PROXY_DEFAULT_PRIMARY_CANDIDATE_COUNT,
                )
                .await?
                .into_iter()
                .next()
            {
                record.primary_proxy_key = Some(primary.key.clone());
            }
        }

        if record.secondary_proxy_key.is_none() {
            let mut exclude = HashSet::new();
            if let Some(primary) = record.primary_proxy_key.as_ref() {
                exclude.insert(primary.clone());
            }
            if let Some(secondary) = self
                .rank_registration_aware_candidates(
                    &format!("{api_key_id}:secondary"),
                    RegistrationAffinityContext {
                        geo_origin: &self.api_key_geo_origin,
                        registration_ip: registration_ip.as_deref(),
                        registration_region: registration_region.as_deref(),
                    },
                    &exclude,
                    true,
                    forward_proxy::FORWARD_PROXY_DEFAULT_SECONDARY_CANDIDATE_COUNT,
                )
                .await?
                .into_iter()
                .next()
            {
                record.secondary_proxy_key = Some(secondary.key.clone());
            }
        }

        if record.primary_proxy_key.is_none() && record.secondary_proxy_key.is_some() {
            record.primary_proxy_key = record.secondary_proxy_key.take();
        }
        record.updated_at = now;
        Ok(record)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) async fn reconcile_proxy_affinity_record(
        &self,
        api_key_id: &str,
    ) -> Result<forward_proxy::ForwardProxyAffinityRecord, ProxyError> {
        self.resolve_proxy_affinity_record(api_key_id, true).await
    }

    pub(crate) async fn promote_proxy_affinity_secondary(
        &self,
        api_key_id: &str,
        succeeded_proxy_key: &str,
    ) -> Result<(), ProxyError> {
        let state = self.load_proxy_affinity_state(api_key_id).await?;
        if state.has_explicit_empty_marker {
            let mut exclude = HashSet::new();
            exclude.insert(succeeded_proxy_key.to_string());
            let secondary_proxy_key = self
                .rank_registration_aware_candidates(
                    &format!("{api_key_id}:secondary"),
                    RegistrationAffinityContext {
                        geo_origin: &self.api_key_geo_origin,
                        registration_ip: state.registration_ip.as_deref(),
                        registration_region: state.registration_region.as_deref(),
                    },
                    &exclude,
                    true,
                    forward_proxy::FORWARD_PROXY_DEFAULT_SECONDARY_CANDIDATE_COUNT,
                )
                .await?
                .into_iter()
                .next()
                .map(|endpoint| endpoint.key);
            self.store_proxy_affinity_record(
                api_key_id,
                forward_proxy::ForwardProxyAffinityRecord {
                    primary_proxy_key: Some(succeeded_proxy_key.to_string()),
                    secondary_proxy_key,
                    updated_at: Utc::now().timestamp(),
                },
            )
            .await?;
            return Ok(());
        }
        let mut record = self
            .reconcile_proxy_affinity_record_with_state(api_key_id, state)
            .await?;
        if record.primary_proxy_key.as_deref() == Some(succeeded_proxy_key) {
            return Ok(());
        }
        if record.secondary_proxy_key.as_deref() == Some(succeeded_proxy_key) {
            record.primary_proxy_key = Some(succeeded_proxy_key.to_string());
            record.secondary_proxy_key = None;
            let (registration_ip, registration_region) =
                self.load_api_key_registration_metadata(api_key_id).await?;
            let mut exclude = HashSet::new();
            exclude.insert(succeeded_proxy_key.to_string());
            if let Some(next_secondary) = self
                .rank_registration_aware_candidates(
                    &format!("{api_key_id}:secondary"),
                    RegistrationAffinityContext {
                        geo_origin: &self.api_key_geo_origin,
                        registration_ip: registration_ip.as_deref(),
                        registration_region: registration_region.as_deref(),
                    },
                    &exclude,
                    true,
                    forward_proxy::FORWARD_PROXY_DEFAULT_SECONDARY_CANDIDATE_COUNT,
                )
                .await?
                .into_iter()
                .next()
            {
                record.secondary_proxy_key = Some(next_secondary.key.clone());
            }
            record.updated_at = Utc::now().timestamp();
            self.store_proxy_affinity_record(api_key_id, record).await?;
        }
        Ok(())
    }

    pub(crate) async fn apply_forward_proxy_geo_candidates_in_memory(
        &self,
        candidates: &[ForwardProxyGeoCandidate],
    ) {
        let mut manager = self.forward_proxy.lock().await;
        for candidate in candidates {
            if let Some(entry) = manager.runtime.get_mut(&candidate.endpoint.key) {
                entry.resolved_ip_source = candidate.source.as_str().to_string();
                entry.resolved_ips = candidate.host_ips.clone();
                entry.resolved_regions = candidate.regions.clone();
                entry.geo_refreshed_at = candidate.geo_refreshed_at;
            }
        }
    }

    pub(crate) async fn persist_forward_proxy_geo_candidates(
        &self,
        candidates: &[ForwardProxyGeoCandidate],
    ) -> Result<(), ProxyError> {
        let changed = {
            let manager = self.forward_proxy.lock().await;
            let mut changed = Vec::new();
            for candidate in candidates {
                let Some(runtime) = manager.runtime.get(&candidate.endpoint.key) else {
                    continue;
                };
                if runtime.resolved_ips == candidate.host_ips
                    && runtime.resolved_regions == candidate.regions
                    && runtime.resolved_ip_source == candidate.source.as_str()
                    && runtime.geo_refreshed_at == candidate.geo_refreshed_at
                {
                    continue;
                }
                changed.push(forward_proxy::ForwardProxyRuntimeGeoMetadataUpdate {
                    proxy_key: candidate.endpoint.key.clone(),
                    display_name: runtime.display_name.clone(),
                    source: runtime.source.clone(),
                    endpoint_url: runtime.endpoint_url.clone(),
                    resolved_ip_source: candidate.source.as_str().to_string(),
                    resolved_ips: candidate.host_ips.clone(),
                    resolved_regions: candidate.regions.clone(),
                    geo_refreshed_at: candidate.geo_refreshed_at,
                    weight: runtime.weight,
                    success_ema: runtime.success_ema,
                    latency_ema_ms: runtime.latency_ema_ms,
                    consecutive_failures: runtime.consecutive_failures,
                    is_penalized: runtime.is_penalized(),
                });
            }
            changed
        };
        forward_proxy::persist_forward_proxy_runtime_geo_metadata_atomic(
            &self.key_store.pool,
            &changed,
        )
        .await?;
        let mut manager = self.forward_proxy.lock().await;
        for update in changed {
            if let Some(entry) = manager.runtime.get_mut(&update.proxy_key) {
                entry.resolved_ip_source = update.resolved_ip_source;
                entry.resolved_ips = update.resolved_ips;
                entry.resolved_regions = update.resolved_regions;
                entry.geo_refreshed_at = update.geo_refreshed_at;
            }
        }
        Ok(())
    }

    pub(crate) fn is_forward_proxy_geo_cache_complete(
        endpoint: &forward_proxy::ForwardProxyEndpoint,
        source: ForwardProxyGeoSource,
        resolved_ips: &[String],
        regions: &[String],
        geo_refreshed_at: i64,
    ) -> bool {
        if endpoint.is_direct() {
            return true;
        }
        if geo_refreshed_at <= 0 {
            return false;
        }
        match source {
            ForwardProxyGeoSource::Negative => true,
            ForwardProxyGeoSource::Trace => {
                !regions.is_empty() && resolved_ips.iter().any(|ip| is_global_geo_ip(ip))
            }
            ForwardProxyGeoSource::Unknown => false,
        }
    }

    pub(crate) fn is_forward_proxy_geo_request_cache_complete(
        endpoint: &forward_proxy::ForwardProxyEndpoint,
        source: ForwardProxyGeoSource,
        resolved_ips: &[String],
        regions: &[String],
        geo_refreshed_at: i64,
        now: i64,
    ) -> bool {
        if endpoint.is_direct() {
            return true;
        }
        if geo_refreshed_at <= 0 {
            return false;
        }
        match source {
            ForwardProxyGeoSource::Negative => {
                now.saturating_sub(geo_refreshed_at)
                    < FORWARD_PROXY_GEO_NEGATIVE_RETRY_COOLDOWN_SECS
            }
            ForwardProxyGeoSource::Trace => {
                !regions.is_empty() && resolved_ips.iter().any(|ip| is_global_geo_ip(ip))
            }
            ForwardProxyGeoSource::Unknown => false,
        }
    }

    pub(crate) async fn resolve_forward_proxy_geo_candidates(
        &self,
        geo_origin: &str,
        endpoints: Vec<forward_proxy::ForwardProxyEndpoint>,
        refresh_mode: ForwardProxyGeoRefreshMode,
    ) -> Result<Vec<ForwardProxyGeoCandidate>, ProxyError> {
        let cached = {
            let manager = self.forward_proxy.lock().await;
            endpoints
                .iter()
                .filter_map(|endpoint| {
                    manager.runtime(&endpoint.key).map(|runtime| {
                        (
                            endpoint.key.clone(),
                            (
                                ForwardProxyGeoSource::from_runtime(&runtime.resolved_ip_source),
                                runtime.resolved_ips.clone(),
                                runtime.resolved_regions.clone(),
                                runtime.geo_refreshed_at,
                            ),
                        )
                    })
                })
                .collect::<HashMap<_, _>>()
        };

        let now = Utc::now().timestamp();
        let mut refresh_targets = Vec::new();
        for endpoint in &endpoints {
            let (cached_source, cached_ips, cached_regions, geo_refreshed_at) = cached
                .get(&endpoint.key)
                .cloned()
                .unwrap_or_else(|| (ForwardProxyGeoSource::Unknown, Vec::new(), Vec::new(), 0));
            let cache_complete = match refresh_mode {
                ForwardProxyGeoRefreshMode::LazyFillMissing => {
                    Self::is_forward_proxy_geo_request_cache_complete(
                        endpoint,
                        cached_source,
                        &cached_ips,
                        &cached_regions,
                        geo_refreshed_at,
                        now,
                    )
                }
                ForwardProxyGeoRefreshMode::ForceRefreshAll => {
                    Self::is_forward_proxy_geo_cache_complete(
                        endpoint,
                        cached_source,
                        &cached_ips,
                        &cached_regions,
                        geo_refreshed_at,
                    )
                }
            };
            let should_refresh = match refresh_mode {
                ForwardProxyGeoRefreshMode::LazyFillMissing => !cache_complete,
                ForwardProxyGeoRefreshMode::ForceRefreshAll => !endpoint.is_direct(),
            };
            if should_refresh && !endpoint.is_direct() {
                refresh_targets.push((
                    endpoint.clone(),
                    cached_source,
                    cached_ips,
                    cached_regions,
                    geo_refreshed_at,
                ));
            }
        }

        let refreshed_at = Utc::now().timestamp();
        let trace_timeout = Duration::from_millis(FORWARD_PROXY_TRACE_TIMEOUT_MS);
        let resolved_refresh = futures_util::stream::iter(refresh_targets.into_iter().map(
            |(endpoint, cached_source, cached_ips, cached_regions, geo_refreshed_at)| async move {
                if refresh_mode == ForwardProxyGeoRefreshMode::LazyFillMissing
                    && cached_source == ForwardProxyGeoSource::Trace
                    && geo_refreshed_at > 0
                    && !cached_ips.is_empty()
                    && cached_regions.is_empty()
                    && cached_ips.iter().any(|ip| is_global_geo_ip(ip))
                {
                    return ForwardProxyGeoCandidate {
                        endpoint,
                        host_ips: cached_ips,
                        regions: Vec::new(),
                        source: ForwardProxyGeoSource::Trace,
                        geo_refreshed_at,
                    };
                }

                if let Some((ip, _location)) = self
                    .fetch_forward_proxy_trace(&endpoint, trace_timeout, None)
                    .await
                {
                    return ForwardProxyGeoCandidate {
                        endpoint,
                        host_ips: vec![ip],
                        regions: Vec::new(),
                        source: ForwardProxyGeoSource::Trace,
                        geo_refreshed_at: refreshed_at,
                    };
                }

                ForwardProxyGeoCandidate {
                    endpoint,
                    host_ips: Vec::new(),
                    regions: Vec::new(),
                    source: ForwardProxyGeoSource::Negative,
                    geo_refreshed_at: refreshed_at,
                }
            },
        ))
        .buffer_unordered(3)
        .collect::<Vec<_>>()
        .await;

        let geo_lookup_ips = resolved_refresh
            .iter()
            .flat_map(|candidate| {
                if candidate.source == ForwardProxyGeoSource::Trace {
                    candidate
                        .host_ips
                        .iter()
                        .filter(|ip| is_global_geo_ip(ip))
                        .cloned()
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                }
            })
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let region_by_ip = resolve_registration_regions(geo_origin, &geo_lookup_ips).await;

        let refreshed_candidates = resolved_refresh
            .into_iter()
            .map(|mut candidate| {
                if candidate.source == ForwardProxyGeoSource::Trace {
                    let mut seen_regions = HashSet::new();
                    candidate.regions = candidate
                        .host_ips
                        .iter()
                        .filter_map(|ip| region_by_ip.get(ip).cloned())
                        .filter(|region| seen_regions.insert(region.clone()))
                        .collect::<Vec<_>>();
                }
                candidate
            })
            .collect::<Vec<_>>();
        if !refreshed_candidates.is_empty()
            && let Err(err) = self
                .persist_forward_proxy_geo_candidates(&refreshed_candidates)
                .await
        {
            if refresh_mode == ForwardProxyGeoRefreshMode::ForceRefreshAll {
                return Err(err);
            }
            eprintln!("forward-proxy-geo-persist: {err}");
            self.apply_forward_proxy_geo_candidates_in_memory(&refreshed_candidates)
                .await;
        }
        let refreshed_by_key = refreshed_candidates
            .into_iter()
            .map(|candidate| (candidate.endpoint.key.clone(), candidate))
            .collect::<HashMap<_, _>>();

        Ok(endpoints
            .into_iter()
            .map(|endpoint| {
                if let Some(candidate) = refreshed_by_key.get(&endpoint.key) {
                    return candidate.clone();
                }
                if let Some((source, resolved_ips, regions, geo_refreshed_at)) =
                    cached.get(&endpoint.key)
                {
                    return ForwardProxyGeoCandidate {
                        endpoint,
                        host_ips: resolved_ips.clone(),
                        regions: regions.clone(),
                        source: *source,
                        geo_refreshed_at: *geo_refreshed_at,
                    };
                }
                ForwardProxyGeoCandidate {
                    endpoint,
                    host_ips: Vec::new(),
                    regions: Vec::new(),
                    source: ForwardProxyGeoSource::Unknown,
                    geo_refreshed_at: 0,
                }
            })
            .collect())
    }

    pub async fn refresh_forward_proxy_geo_metadata(
        &self,
        geo_origin: &str,
        force_all: bool,
    ) -> Result<usize, ProxyError> {
        let endpoints = {
            let manager = self.forward_proxy.lock().await;
            manager
                .endpoints
                .iter()
                .filter(|endpoint| !endpoint.is_direct())
                .cloned()
                .collect::<Vec<_>>()
        };
        let refresh_mode = if force_all {
            ForwardProxyGeoRefreshMode::ForceRefreshAll
        } else {
            ForwardProxyGeoRefreshMode::LazyFillMissing
        };
        let candidates = self
            .resolve_forward_proxy_geo_candidates(geo_origin, endpoints, refresh_mode)
            .await?;
        Ok(candidates.len())
    }

    pub(crate) fn forward_proxy_geo_incomplete_retry_wait_secs(
        source: ForwardProxyGeoSource,
        resolved_ips: &[String],
        geo_refreshed_at: i64,
        now: i64,
    ) -> Option<i64> {
        if source != ForwardProxyGeoSource::Trace
            || geo_refreshed_at <= 0
            || !resolved_ips.iter().any(|ip| is_global_geo_ip(ip))
        {
            return None;
        }
        let age = now.saturating_sub(geo_refreshed_at);
        let remaining = FORWARD_PROXY_GEO_NEGATIVE_RETRY_COOLDOWN_SECS.saturating_sub(age);
        (remaining > 0).then_some(remaining)
    }

    pub async fn forward_proxy_geo_refresh_wait_secs(&self, max_age_secs: i64) -> i64 {
        let now = Utc::now().timestamp();
        let manager = self.forward_proxy.lock().await;
        let mut saw_non_direct = false;
        let mut min_wait = max_age_secs.max(0);
        for endpoint in &manager.endpoints {
            if endpoint.is_direct() {
                continue;
            }
            saw_non_direct = true;
            let (source, resolved_ips, resolved_regions, refreshed_at) = manager
                .runtime(&endpoint.key)
                .map(|runtime| {
                    (
                        ForwardProxyGeoSource::from_runtime(&runtime.resolved_ip_source),
                        runtime.resolved_ips.clone(),
                        runtime.resolved_regions.clone(),
                        runtime.geo_refreshed_at,
                    )
                })
                .unwrap_or_else(|| (ForwardProxyGeoSource::Unknown, Vec::new(), Vec::new(), 0));
            if !Self::is_forward_proxy_geo_cache_complete(
                endpoint,
                source,
                &resolved_ips,
                &resolved_regions,
                refreshed_at,
            ) {
                if let Some(wait_secs) = Self::forward_proxy_geo_incomplete_retry_wait_secs(
                    source,
                    &resolved_ips,
                    refreshed_at,
                    now,
                ) {
                    min_wait = min_wait.min(wait_secs);
                    continue;
                }
                return 0;
            }
            let age = now.saturating_sub(refreshed_at);
            if age >= max_age_secs {
                return 0;
            }
            min_wait = min_wait.min(max_age_secs.saturating_sub(age));
        }
        if saw_non_direct {
            min_wait
        } else {
            max_age_secs.max(0)
        }
    }

    pub async fn forward_proxy_geo_refresh_due(&self, max_age_secs: i64) -> bool {
        self.forward_proxy_geo_refresh_wait_secs(max_age_secs).await <= 0
    }

    pub(crate) async fn select_proxy_affinity_preview_for_registration_with_hint(
        &self,
        subject: &str,
        geo_origin: &str,
        registration_ip: Option<&str>,
        registration_region: Option<&str>,
        preferred_primary_proxy_key: Option<&str>,
    ) -> Result<
        (
            forward_proxy::ForwardProxyAffinityRecord,
            Option<ForwardProxyAssignmentPreview>,
        ),
        ProxyError,
    > {
        let (ranked_non_direct, ranked_any) = {
            let mut manager = self.forward_proxy.lock().await;
            manager.ensure_non_zero_weight();
            let exclude = HashSet::new();
            let limit = manager.endpoints.len().max(1);
            (
                manager.rank_candidates_for_subject(subject, &exclude, false, limit),
                manager.rank_candidates_for_subject(subject, &exclude, true, limit),
            )
        };

        let primary_pool = if ranked_non_direct.is_empty() {
            ranked_any.clone()
        } else {
            ranked_non_direct.clone()
        };
        let geo_candidates = self
            .resolve_forward_proxy_geo_candidates(
                geo_origin,
                primary_pool.clone(),
                ForwardProxyGeoRefreshMode::LazyFillMissing,
            )
            .await?;
        let normalized_registration_ip = registration_ip.and_then(normalize_ip_string);
        let normalized_registration_region = registration_region
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let preferred_primary = preferred_primary_proxy_key.and_then(|preferred_key| {
            ranked_any
                .iter()
                .find(|endpoint| endpoint.key == preferred_key)
                .cloned()
        });
        let select_ranked_geo_match = |matching_keys: &HashSet<String>| {
            primary_pool
                .iter()
                .find(|endpoint| matching_keys.contains(&endpoint.key))
                .cloned()
                .or_else(|| {
                    ranked_any
                        .iter()
                        .find(|endpoint| matching_keys.contains(&endpoint.key))
                        .cloned()
                })
        };
        let exact_match_keys = normalized_registration_ip
            .as_ref()
            .map(|registration_ip| {
                geo_candidates
                    .iter()
                    .filter(|candidate| candidate.host_ips.iter().any(|ip| ip == registration_ip))
                    .map(|candidate| candidate.endpoint.key.clone())
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();
        let region_match_keys = normalized_registration_region
            .as_ref()
            .map(|registration_region| {
                geo_candidates
                    .iter()
                    .filter(|candidate| {
                        candidate
                            .regions
                            .iter()
                            .any(|region| region == registration_region)
                    })
                    .map(|candidate| candidate.endpoint.key.clone())
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();
        let exact_match = if exact_match_keys.is_empty() {
            None
        } else {
            select_ranked_geo_match(&exact_match_keys)
        };
        let region_match = if region_match_keys.is_empty() {
            None
        } else {
            select_ranked_geo_match(&region_match_keys)
        };

        let primary = exact_match
            .or(region_match)
            .or(preferred_primary)
            .or_else(|| primary_pool.first().cloned())
            .or_else(|| ranked_any.first().cloned());
        let primary_proxy_key = primary.as_ref().map(|endpoint| endpoint.key.clone());
        let primary_match_kind = primary.as_ref().map(|endpoint| {
            if exact_match_keys.contains(&endpoint.key) {
                AssignedProxyMatchKind::RegistrationIp
            } else if region_match_keys.contains(&endpoint.key) {
                AssignedProxyMatchKind::SameRegion
            } else {
                AssignedProxyMatchKind::Other
            }
        });

        let mut secondary_exclude = HashSet::new();
        if let Some(primary_proxy_key) = primary_proxy_key.as_ref() {
            secondary_exclude.insert(primary_proxy_key.clone());
        }
        let secondary_proxy_key = self
            .rank_registration_aware_candidates(
                &format!("{subject}:secondary"),
                RegistrationAffinityContext {
                    geo_origin,
                    registration_ip: normalized_registration_ip.as_deref(),
                    registration_region: normalized_registration_region.as_deref(),
                },
                &secondary_exclude,
                true,
                ranked_any.len().max(1),
            )
            .await?
            .into_iter()
            .next()
            .map(|endpoint| endpoint.key);

        Ok((
            forward_proxy::ForwardProxyAffinityRecord {
                primary_proxy_key,
                secondary_proxy_key,
                updated_at: Utc::now().timestamp(),
            },
            primary
                .zip(primary_match_kind)
                .map(|(endpoint, match_kind)| ForwardProxyAssignmentPreview {
                    key: endpoint.key,
                    label: endpoint.display_name,
                    match_kind,
                }),
        ))
    }

    pub(crate) async fn select_proxy_affinity_for_registration_with_hint(
        &self,
        subject: &str,
        geo_origin: &str,
        registration_ip: Option<&str>,
        registration_region: Option<&str>,
        preferred_primary_proxy_key: Option<&str>,
    ) -> Result<forward_proxy::ForwardProxyAffinityRecord, ProxyError> {
        self.select_proxy_affinity_preview_for_registration_with_hint(
            subject,
            geo_origin,
            registration_ip,
            registration_region,
            preferred_primary_proxy_key,
        )
        .await
        .map(|(record, _preview)| record)
    }

    pub(crate) async fn select_proxy_affinity_for_hint_only(
        &self,
        subject: &str,
        geo_origin: &str,
        preferred_primary_proxy_key: &str,
    ) -> Result<forward_proxy::ForwardProxyAffinityRecord, ProxyError> {
        if preferred_primary_proxy_key == forward_proxy::FORWARD_PROXY_DIRECT_KEY {
            return Ok(forward_proxy::ForwardProxyAffinityRecord {
                updated_at: Utc::now().timestamp(),
                ..Default::default()
            });
        }
        let (preferred_exists, candidate_limit) = {
            let manager = self.forward_proxy.lock().await;
            (
                manager.endpoint(preferred_primary_proxy_key).is_some(),
                manager.endpoints.len().max(1),
            )
        };
        if !preferred_exists {
            return Ok(forward_proxy::ForwardProxyAffinityRecord {
                updated_at: Utc::now().timestamp(),
                ..Default::default()
            });
        }

        let mut secondary_exclude = HashSet::new();
        secondary_exclude.insert(preferred_primary_proxy_key.to_string());
        let secondary_proxy_key = self
            .rank_registration_aware_candidates(
                &format!("{subject}:secondary"),
                RegistrationAffinityContext {
                    geo_origin,
                    registration_ip: None,
                    registration_region: None,
                },
                &secondary_exclude,
                true,
                candidate_limit,
            )
            .await?
            .into_iter()
            .next()
            .map(|endpoint| endpoint.key);

        Ok(forward_proxy::ForwardProxyAffinityRecord {
            primary_proxy_key: Some(preferred_primary_proxy_key.to_string()),
            secondary_proxy_key,
            updated_at: Utc::now().timestamp(),
        })
    }

    pub(crate) async fn build_proxy_attempt_plan_for_record(
        &self,
        subject: &str,
        record: &forward_proxy::ForwardProxyAffinityRecord,
        allow_direct_fallback: bool,
    ) -> Result<Vec<forward_proxy::SelectedForwardProxy>, ProxyError> {
        let mut plan = Vec::new();
        let mut seen = HashSet::new();
        {
            let manager = self.forward_proxy.lock().await;
            for key in [
                record.primary_proxy_key.as_ref(),
                record.secondary_proxy_key.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                if seen.insert(key.clone())
                    && let Some(endpoint) = manager.endpoint(key)
                    && endpoint.is_selectable()
                    && manager.runtime(key).is_some_and(|runtime| {
                        runtime.available && runtime.weight.is_finite() && runtime.weight > 0.0
                    })
                {
                    plan.push(forward_proxy::SelectedForwardProxy::from_endpoint(endpoint));
                }
            }
        }
        let (registration_ip, registration_region) =
            self.load_api_key_registration_metadata(subject).await?;
        let limit = {
            let manager = self.forward_proxy.lock().await;
            manager.endpoints.len().max(1)
        };
        for endpoint in self
            .rank_registration_aware_candidates(
                subject,
                RegistrationAffinityContext {
                    geo_origin: &self.api_key_geo_origin,
                    registration_ip: registration_ip.as_deref(),
                    registration_region: registration_region.as_deref(),
                },
                &seen,
                allow_direct_fallback,
                limit,
            )
            .await?
        {
            if seen.insert(endpoint.key.clone()) {
                plan.push(forward_proxy::SelectedForwardProxy::from_endpoint(
                    &endpoint,
                ));
            }
        }
        Ok(plan)
    }

    pub(crate) async fn build_proxy_attempt_plan(
        &self,
        api_key_id: &str,
    ) -> Result<Vec<forward_proxy::SelectedForwardProxy>, ProxyError> {
        let state = self.load_proxy_affinity_state(api_key_id).await?;
        if state.has_explicit_empty_marker {
            return self
                .build_proxy_attempt_plan_for_record(api_key_id, &state.record, true)
                .await;
        }
        let record = self.resolve_proxy_affinity_record(api_key_id, true).await?;
        self.build_proxy_attempt_plan_for_record(api_key_id, &record, false)
            .await
    }

    pub(crate) async fn record_forward_proxy_attempt(
        &self,
        proxy_key: &str,
        _api_key_id: Option<&str>,
        _request_kind: &str,
        success: bool,
        latency_ms: Option<f64>,
        failure_kind: Option<&str>,
    ) -> Result<(), ProxyError> {
        self.record_forward_proxy_attempt_inner(proxy_key, success, latency_ms, failure_kind, false)
            .await
    }

    pub(crate) async fn record_forward_proxy_attempt_inner(
        &self,
        proxy_key: &str,
        success: bool,
        latency_ms: Option<f64>,
        failure_kind: Option<&str>,
        is_probe: bool,
    ) -> Result<(), ProxyError> {
        forward_proxy::insert_forward_proxy_attempt(
            &self.key_store.pool,
            proxy_key,
            success,
            latency_ms,
            failure_kind,
            is_probe,
        )
        .await?;
        {
            let mut manager = self.forward_proxy.lock().await;
            manager.record_attempt(proxy_key, success, latency_ms, failure_kind);
            if let Some(runtime) = manager.runtime(proxy_key).cloned() {
                let bucket_start = (Utc::now().timestamp() / 3600) * 3600;
                let sample_epoch_us = Utc::now().timestamp_nanos_opt().unwrap_or_default() / 1_000;
                forward_proxy::persist_forward_proxy_runtime_health_state(
                    &self.key_store.pool,
                    &runtime,
                )
                .await?;
                forward_proxy::upsert_forward_proxy_weight_hourly_bucket(
                    &self.key_store.pool,
                    proxy_key,
                    bucket_start,
                    runtime.weight,
                    sample_epoch_us,
                )
                .await?;
            }
        }
        Ok(())
    }

    pub(crate) async fn send_with_forward_proxy_plan<F>(
        &self,
        _subject: &str,
        affinity_owner_key_id: Option<&str>,
        request_kind: &str,
        plan: Vec<forward_proxy::SelectedForwardProxy>,
        mut build: F,
    ) -> Result<(reqwest::Response, forward_proxy::SelectedForwardProxy), ProxyError>
    where
        F: FnMut(Client) -> reqwest::RequestBuilder,
    {
        {
            let mut manager = self.forward_proxy.lock().await;
            manager.note_request();
        }
        if let Err(err) = self.maybe_run_forward_proxy_maintenance().await {
            eprintln!("forward-proxy maintenance error: {err}");
        }
        let mut last_error: Option<ProxyError> = None;
        for candidate in plan {
            let client = match self
                .forward_proxy_clients
                .client_for(candidate.endpoint_url.as_ref())
                .await
            {
                Ok(client) => client,
                Err(err) => {
                    let error_code = map_forward_proxy_validation_error_code(&err);
                    let _ = self
                        .record_forward_proxy_attempt(
                            &candidate.key,
                            affinity_owner_key_id,
                            request_kind,
                            false,
                            None,
                            Some(error_code.as_str()),
                        )
                        .await;
                    last_error = Some(err);
                    continue;
                }
            };
            let started = Instant::now();
            match build(client).send().await {
                Ok(response) => {
                    let latency_ms = started.elapsed().as_secs_f64() * 1000.0;
                    let _ = self
                        .record_forward_proxy_attempt(
                            &candidate.key,
                            affinity_owner_key_id,
                            request_kind,
                            true,
                            Some(latency_ms),
                            None,
                        )
                        .await;
                    if let Some(api_key_id) = affinity_owner_key_id {
                        let _ = self
                            .promote_proxy_affinity_secondary(api_key_id, &candidate.key)
                            .await;
                    }
                    return Ok((response, candidate));
                }
                Err(err) => {
                    let failure_kind = forward_proxy::failure_kind_from_http_error(&err);
                    let _ = self
                        .record_forward_proxy_attempt(
                            &candidate.key,
                            affinity_owner_key_id,
                            request_kind,
                            false,
                            None,
                            Some(failure_kind),
                        )
                        .await;
                    last_error = Some(ProxyError::Http(err));
                }
            }
        }

        let direct = {
            let manager = self.forward_proxy.lock().await;
            manager
                .endpoint_by_key(forward_proxy::FORWARD_PROXY_DIRECT_KEY)
                .filter(|endpoint| endpoint.is_selectable())
                .map(|endpoint| forward_proxy::SelectedForwardProxy::from_endpoint(&endpoint))
        };
        let Some(direct) = direct else {
            return Err(last_error.unwrap_or_else(|| {
                ProxyError::Other("no selectable forward proxy endpoints available".to_string())
            }));
        };
        let client = self.forward_proxy_clients.direct_client();
        let started = Instant::now();
        match build(client).send().await {
            Ok(response) => {
                let _ = self
                    .record_forward_proxy_attempt(
                        &direct.key,
                        affinity_owner_key_id,
                        request_kind,
                        true,
                        Some(started.elapsed().as_secs_f64() * 1000.0),
                        None,
                    )
                    .await;
                Ok((response, direct))
            }
            Err(err) => {
                let _ = self
                    .record_forward_proxy_attempt(
                        &direct.key,
                        affinity_owner_key_id,
                        request_kind,
                        false,
                        None,
                        Some(forward_proxy::failure_kind_from_http_error(&err)),
                    )
                    .await;
                Err(last_error.unwrap_or(ProxyError::Http(err)))
            }
        }
    }

    pub(crate) async fn send_with_forward_proxy<F>(
        &self,
        api_key_id: &str,
        request_kind: &str,
        build: F,
    ) -> Result<(reqwest::Response, forward_proxy::SelectedForwardProxy), ProxyError>
    where
        F: FnMut(Client) -> reqwest::RequestBuilder,
    {
        let plan = self
            .build_proxy_attempt_plan(api_key_id)
            .await
            .unwrap_or_default();
        self.send_with_forward_proxy_plan(api_key_id, Some(api_key_id), request_kind, plan, build)
            .await
    }

    pub(crate) async fn send_with_forward_proxy_affinity<F>(
        &self,
        subject: &str,
        request_kind: &str,
        affinity: &forward_proxy::ForwardProxyAffinityRecord,
        build: F,
    ) -> Result<(reqwest::Response, forward_proxy::SelectedForwardProxy), ProxyError>
    where
        F: FnMut(Client) -> reqwest::RequestBuilder,
    {
        let plan = self
            .build_proxy_attempt_plan_for_record(subject, affinity, false)
            .await
            .unwrap_or_default();
        self.send_with_forward_proxy_plan(subject, None, request_kind, plan, build)
            .await
    }

    pub(crate) async fn billing_subject_for_token(
        &self,
        token_id: &str,
    ) -> Result<String, ProxyError> {
        Ok(
            match self.key_store.find_user_id_by_token_fresh(token_id).await? {
                Some(user_id) => QuotaSubject::Account(user_id).billing_subject(),
                None => QuotaSubject::Token(token_id.to_string()).billing_subject(),
            },
        )
    }

    pub(crate) async fn reconcile_pending_billing_for_subject(
        &self,
        billing_subject: &str,
    ) -> Result<(), ProxyError> {
        let pending = self
            .key_store
            .list_pending_billing_log_ids(billing_subject)
            .await?;
        for log_id in pending {
            // `lock_token_billing()` already holds the per-subject lock at this point, so a
            // retry-later miss here is unexpected. We retry once to tolerate edge timing around
            // SQLite statement visibility, then fail closed so stale pending charges cannot bypass
            // the quota precheck for the current request.
            let mut retry_later_attempts = 0;
            loop {
                match self.key_store.apply_pending_billing_log(log_id).await? {
                    PendingBillingSettleOutcome::Charged
                    | PendingBillingSettleOutcome::AlreadySettled => break,
                    PendingBillingSettleOutcome::RetryLater => {
                        retry_later_attempts += 1;
                        if retry_later_attempts >= 2 {
                            let msg = format!(
                                "pending billing claim miss for auth_token_logs.id={log_id}; blocking request until replay succeeds",
                            );
                            eprintln!("{msg}");
                            let _ = self.annotate_pending_billing_attempt(log_id, &msg).await;
                            return Err(ProxyError::Other(msg));
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) async fn lock_billing_subject(
        &self,
        billing_subject: &str,
    ) -> Result<TokenBillingGuard, ProxyError> {
        let lock = {
            let mut locks = self.token_billing_locks.lock().await;
            if locks.len() > 1024 {
                locks.retain(|_, lock| lock.strong_count() > 0);
            }

            if let Some(existing) = locks.get(billing_subject).and_then(|lock| lock.upgrade()) {
                existing
            } else {
                let lock = Arc::new(Mutex::new(()));
                locks.insert(billing_subject.to_string(), Arc::downgrade(&lock));
                lock
            }
        };
        let local_guard = lock.lock_owned().await;
        let lease = self
            .key_store
            .acquire_quota_subject_lock(
                billing_subject,
                Duration::from_secs(QUOTA_SUBJECT_LOCK_TTL_SECS),
                Duration::from_secs(QUOTA_SUBJECT_LOCK_ACQUIRE_TIMEOUT_SECS),
            )
            .await?;
        Ok(TokenBillingGuard {
            billing_subject: billing_subject.to_string(),
            _local: local_guard,
            _subject_lock: QuotaSubjectLockGuard::new(self.key_store.clone(), lease),
        })
    }

    /// Serialize quota/billing work per effective quota subject across both the local process
    /// and any other instances sharing the same SQLite database.
    pub async fn lock_token_billing(
        &self,
        token_id: &str,
    ) -> Result<TokenBillingGuard, ProxyError> {
        let current_subject = self.billing_subject_for_token(token_id).await?;
        let mut subjects = self
            .key_store
            .list_pending_billing_subjects_for_token(token_id)
            .await?;
        subjects.push(current_subject.clone());
        subjects.sort();
        subjects.dedup();

        let mut current_guard: Option<TokenBillingGuard> = None;
        let mut extra_guards: Vec<TokenBillingGuard> = Vec::new();
        for subject in subjects {
            let guard = self.lock_billing_subject(&subject).await?;
            self.reconcile_pending_billing_for_subject(guard.billing_subject())
                .await?;
            if subject == current_subject {
                current_guard = Some(guard);
            } else {
                extra_guards.push(guard);
            }
        }
        drop(extra_guards);

        current_guard.ok_or_else(|| {
            ProxyError::Other(format!(
                "failed to acquire billing guard for current subject {current_subject}",
            ))
        })
    }

    pub(crate) async fn lock_research_key_usage(
        &self,
        key_id: &str,
    ) -> Result<TokenBillingGuard, ProxyError> {
        let subject = format!("research-key:{key_id}");
        let lock = {
            let mut locks = self.research_key_locks.lock().await;
            if locks.len() > 256 {
                locks.retain(|_, lock| lock.strong_count() > 0);
            }

            if let Some(existing) = locks.get(&subject).and_then(|lock| lock.upgrade()) {
                existing
            } else {
                let lock = Arc::new(Mutex::new(()));
                locks.insert(subject.clone(), Arc::downgrade(&lock));
                lock
            }
        };
        let local_guard = lock.lock_owned().await;
        let lease = self
            .key_store
            .acquire_quota_subject_lock(
                &subject,
                Duration::from_secs(QUOTA_SUBJECT_LOCK_TTL_SECS),
                Duration::from_secs(QUOTA_SUBJECT_LOCK_ACQUIRE_TIMEOUT_SECS),
            )
            .await?;

        Ok(TokenBillingGuard {
            billing_subject: subject,
            _local: local_guard,
            _subject_lock: QuotaSubjectLockGuard::new(self.key_store.clone(), lease),
        })
    }

    async fn rebind_user_primary_affinity(
        &self,
        user_id: &str,
        old_key_id: Option<&str>,
    ) -> Result<ApiKeyLease, ProxyError> {
        let lease = self
            .key_store
            .acquire_active_key_excluding(old_key_id)
            .await?;
        self.key_store
            .sync_user_primary_api_key_affinity(user_id, &lease.id)
            .await?;
        self.key_store
            .revoke_mcp_sessions_for_user(user_id, "primary_api_key_rebound")
            .await?;
        Ok(lease)
    }

    async fn rebind_token_primary_affinity(
        &self,
        token_id: &str,
        old_key_id: Option<&str>,
    ) -> Result<ApiKeyLease, ProxyError> {
        let lease = self
            .key_store
            .acquire_active_key_excluding(old_key_id)
            .await?;
        self.key_store
            .set_token_primary_api_key_affinity(token_id, None, &lease.id)
            .await?;
        self.key_store
            .revoke_mcp_sessions_for_token(token_id, "primary_api_key_rebound")
            .await?;
        Ok(lease)
    }

    pub(crate) async fn acquire_key_for(
        &self,
        auth_token_id: Option<&str>,
    ) -> Result<ApiKeyLease, ProxyError> {
        let Some(token_id) = auth_token_id else {
            // No token id (e.g. certain internal or dev flows) → plain global scheduling.
            return self.key_store.acquire_key().await;
        };

        if let Some(user_id) = self.key_store.find_user_id_by_token(token_id).await? {
            let user_primary = self
                .key_store
                .get_user_primary_api_key_affinity(&user_id)
                .await?;
            let token_primary = self
                .key_store
                .get_token_primary_api_key_affinity(token_id)
                .await?;
            let legacy_primary = if user_primary.is_none() && token_primary.is_none() {
                self.key_store
                    .find_recent_primary_candidate_for_user(&user_id)
                    .await?
            } else {
                None
            };

            let mut candidates = Vec::new();
            if let Some(user_primary) = user_primary.as_ref() {
                candidates.push(user_primary.clone());
            }
            if let Some(token_primary) = token_primary.as_ref()
                && token_primary.user_id.as_deref() == Some(user_id.as_str())
                && !candidates
                    .iter()
                    .any(|candidate| candidate == &token_primary.api_key_id)
            {
                candidates.push(token_primary.api_key_id.clone());
            }
            if let Some(legacy_primary) = legacy_primary.as_ref()
                && !candidates
                    .iter()
                    .any(|candidate| candidate == legacy_primary)
            {
                candidates.push(legacy_primary.clone());
            }

            for key_id in candidates {
                if let Some(lease) = self.key_store.try_acquire_specific_key(&key_id).await? {
                    self.key_store
                        .sync_user_primary_api_key_affinity(&user_id, &lease.id)
                        .await?;
                    return Ok(lease);
                }
            }

            if user_primary.is_some() || token_primary.is_some() {
                return self
                    .rebind_user_primary_affinity(&user_id, user_primary.as_deref())
                    .await;
            }

            let lease = self.key_store.acquire_key().await?;
            self.key_store
                .sync_user_primary_api_key_affinity(&user_id, &lease.id)
                .await?;
            return Ok(lease);
        }

        if let Some(token_primary) = self
            .key_store
            .get_token_primary_api_key_affinity(token_id)
            .await?
        {
            if let Some(lease) = self
                .key_store
                .try_acquire_specific_key(&token_primary.api_key_id)
                .await?
            {
                self.key_store
                    .set_token_primary_api_key_affinity(token_id, None, &lease.id)
                    .await?;
                return Ok(lease);
            }

            return self
                .rebind_token_primary_affinity(token_id, Some(&token_primary.api_key_id))
                .await;
        }

        let lease = self.key_store.acquire_key().await?;
        self.key_store
            .set_token_primary_api_key_affinity(token_id, None, &lease.id)
            .await?;
        Ok(lease)
    }

    pub(crate) async fn acquire_key_for_research_request(
        &self,
        auth_token_id: Option<&str>,
        research_request_id: Option<&str>,
    ) -> Result<ApiKeyLease, ProxyError> {
        let now = Utc::now().timestamp();

        if let Some(request_id) = research_request_id {
            let mut candidate_key_id = {
                let mut state = self.research_request_affinity.lock().await;
                state.get_candidate(request_id, now)
            };

            if candidate_key_id.is_none()
                && let Some((key_id, owner_token_id)) = self
                    .key_store
                    .get_research_request_affinity(request_id, now)
                    .await?
            {
                self.populate_research_request_affinity_caches(
                    request_id,
                    &key_id,
                    &owner_token_id,
                    now,
                )
                .await;
                candidate_key_id = Some(key_id);
            }

            if let Some(key_id) = candidate_key_id {
                if let Some(lease) = self.key_store.try_acquire_specific_key(&key_id).await? {
                    return Ok(lease);
                }
                return Err(ProxyError::NoAvailableKeys);
            }
        }

        self.acquire_key_for(auth_token_id).await
    }

    pub(crate) async fn populate_research_request_affinity_caches(
        &self,
        request_id: &str,
        key_id: &str,
        token_id: &str,
        now: i64,
    ) {
        {
            let mut state = self.research_request_affinity.lock().await;
            state.record_mapping(request_id, key_id, now);
        }
        let mut owner_state = self.research_request_owner_affinity.lock().await;
        owner_state.record_mapping(request_id, token_id, now);
    }

    pub(crate) async fn record_research_request_affinity(
        &self,
        request_id: &str,
        key_id: &str,
        token_id: &str,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        self.populate_research_request_affinity_caches(request_id, key_id, token_id, now)
            .await;
        self.key_store
            .save_research_request_affinity(
                request_id,
                key_id,
                token_id,
                now + RESEARCH_REQUEST_AFFINITY_TTL_SECS,
            )
            .await
    }

    pub async fn is_research_request_owned_by(
        &self,
        request_id: &str,
        token_id: Option<&str>,
    ) -> Result<bool, ProxyError> {
        let Some(token_id) = token_id else {
            return Ok(false);
        };

        let now = Utc::now().timestamp();
        if let Some(owner) = {
            let mut state = self.research_request_owner_affinity.lock().await;
            state.get_candidate(request_id, now)
        } {
            return Ok(owner == token_id);
        }

        match self
            .key_store
            .get_research_request_affinity(request_id, now)
            .await
        {
            Ok(Some((key_id, owner_token_id))) => {
                self.populate_research_request_affinity_caches(
                    request_id,
                    &key_id,
                    &owner_token_id,
                    now,
                )
                .await;
                Ok(owner_token_id == token_id)
            }
            Ok(None) => Ok(false),
            Err(err) => Err(err),
        }
    }

    pub(crate) async fn reconcile_key_health(
        &self,
        lease: &ApiKeyLease,
        source: &str,
        analysis: &AttemptAnalysis,
        auth_token_id: Option<&str>,
    ) -> Result<KeyEffect, ProxyError> {
        match &analysis.key_health_action {
            KeyHealthAction::None => {
                if analysis.status != OUTCOME_SUCCESS {
                    return Ok(KeyEffect::none());
                }
                let before = self.key_store.fetch_key_state_snapshot(&lease.id).await?;
                let changed = self.key_store.restore_active_status(&lease.secret).await?;
                if !changed {
                    return Ok(KeyEffect::none());
                }
                let after = self.key_store.fetch_key_state_snapshot(&lease.id).await?;
                self.key_store
                    .insert_api_key_maintenance_record(ApiKeyMaintenanceRecord {
                        id: nanoid!(12),
                        key_id: lease.id.clone(),
                        source: MAINTENANCE_SOURCE_SYSTEM.to_string(),
                        operation_code: MAINTENANCE_OP_AUTO_RESTORE_ACTIVE.to_string(),
                        operation_summary: "自动恢复为 active".to_string(),
                        reason_code: None,
                        reason_summary: Some("成功请求触发从 exhausted 恢复".to_string()),
                        reason_detail: Some(format!("source={source}")),
                        request_log_id: None,
                        auth_token_log_id: None,
                        auth_token_id: auth_token_id.map(str::to_string),
                        actor_user_id: None,
                        actor_display_name: None,
                        status_before: before.status,
                        status_after: after.status,
                        quarantine_before: before.quarantined,
                        quarantine_after: after.quarantined,
                        created_at: Utc::now().timestamp(),
                    })
                    .await?;
                Ok(KeyEffect::new(
                    KEY_EFFECT_RESTORED_ACTIVE,
                    "The system automatically restored this exhausted key to active",
                ))
            }
            KeyHealthAction::MarkExhausted => {
                let before = self.key_store.fetch_key_state_snapshot(&lease.id).await?;
                let changed = self.key_store.mark_quota_exhausted(&lease.secret).await?;
                if !changed {
                    return Ok(KeyEffect::none());
                }
                let after = self.key_store.fetch_key_state_snapshot(&lease.id).await?;
                self.key_store
                    .insert_api_key_maintenance_record(ApiKeyMaintenanceRecord {
                        id: nanoid!(12),
                        key_id: lease.id.clone(),
                        source: MAINTENANCE_SOURCE_SYSTEM.to_string(),
                        operation_code: MAINTENANCE_OP_AUTO_MARK_EXHAUSTED.to_string(),
                        operation_summary: "自动标记为 exhausted".to_string(),
                        reason_code: Some("quota_exhausted".to_string()),
                        reason_summary: Some("上游额度耗尽".to_string()),
                        reason_detail: Some(format!("source={source}")),
                        request_log_id: None,
                        auth_token_log_id: None,
                        auth_token_id: auth_token_id.map(str::to_string),
                        actor_user_id: None,
                        actor_display_name: None,
                        status_before: before.status,
                        status_after: after.status,
                        quarantine_before: before.quarantined,
                        quarantine_after: after.quarantined,
                        created_at: Utc::now().timestamp(),
                    })
                    .await?;
                Ok(KeyEffect::new(
                    KEY_EFFECT_MARKED_EXHAUSTED,
                    "The system automatically marked this key as exhausted",
                ))
            }
            KeyHealthAction::Quarantine(decision) => {
                let before = self.key_store.fetch_key_state_snapshot(&lease.id).await?;
                let inserted = self
                    .key_store
                    .quarantine_key_by_id(
                        &lease.id,
                        source,
                        &decision.reason_code,
                        &decision.reason_summary,
                        &decision.reason_detail,
                    )
                    .await?;
                if !inserted {
                    return Ok(KeyEffect::none());
                }
                let after = self.key_store.fetch_key_state_snapshot(&lease.id).await?;
                self.key_store
                    .insert_api_key_maintenance_record(ApiKeyMaintenanceRecord {
                        id: nanoid!(12),
                        key_id: lease.id.clone(),
                        source: MAINTENANCE_SOURCE_SYSTEM.to_string(),
                        operation_code: MAINTENANCE_OP_AUTO_QUARANTINE.to_string(),
                        operation_summary: "自动隔离 Key".to_string(),
                        reason_code: Some(decision.reason_code.clone()),
                        reason_summary: Some(decision.reason_summary.clone()),
                        reason_detail: Some(decision.reason_detail.clone()),
                        request_log_id: None,
                        auth_token_log_id: None,
                        auth_token_id: auth_token_id.map(str::to_string),
                        actor_user_id: None,
                        actor_display_name: None,
                        status_before: before.status,
                        status_after: after.status,
                        quarantine_before: before.quarantined,
                        quarantine_after: after.quarantined,
                        created_at: Utc::now().timestamp(),
                    })
                    .await?;
                Ok(KeyEffect::new(
                    KEY_EFFECT_QUARANTINED,
                    "The system automatically quarantined this key",
                ))
            }
        }
    }

    pub(crate) async fn maybe_quarantine_usage_error(
        &self,
        key_id: &str,
        source: &str,
        err: &ProxyError,
    ) -> Result<(), ProxyError> {
        let ProxyError::UsageHttp { status, body } = err else {
            return Ok(());
        };
        let Some(decision) =
            classify_quarantine_reason(Some(status.as_u16() as i64), body.as_bytes())
        else {
            return Ok(());
        };
        let before = self.key_store.fetch_key_state_snapshot(key_id).await?;
        let inserted = self
            .key_store
            .quarantine_key_by_id(
                key_id,
                source,
                &decision.reason_code,
                &decision.reason_summary,
                &decision.reason_detail,
            )
            .await?;
        if inserted {
            let after = self.key_store.fetch_key_state_snapshot(key_id).await?;
            self.key_store
                .insert_api_key_maintenance_record(ApiKeyMaintenanceRecord {
                    id: nanoid!(12),
                    key_id: key_id.to_string(),
                    source: MAINTENANCE_SOURCE_SYSTEM.to_string(),
                    operation_code: MAINTENANCE_OP_AUTO_QUARANTINE.to_string(),
                    operation_summary: "自动隔离 Key".to_string(),
                    reason_code: Some(decision.reason_code),
                    reason_summary: Some(decision.reason_summary),
                    reason_detail: Some(decision.reason_detail),
                    request_log_id: None,
                    auth_token_log_id: None,
                    auth_token_id: None,
                    actor_user_id: None,
                    actor_display_name: None,
                    status_before: before.status,
                    status_after: after.status,
                    quarantine_before: before.quarantined,
                    quarantine_after: after.quarantined,
                    created_at: Utc::now().timestamp(),
                })
                .await?;
        }
        Ok(())
    }

    /// 将请求透传到 Tavily upstream 并记录日志。
    pub async fn proxy_request(&self, request: ProxyRequest) -> Result<ProxyResponse, ProxyError> {
        let lease = if let Some(key_id) = request.pinned_api_key_id.as_deref() {
            let Some(lease) = self.key_store.try_acquire_specific_key(key_id).await? else {
                return Err(ProxyError::PinnedMcpSessionUnavailable);
            };
            lease
        } else {
            self.acquire_key_for(request.auth_token_id.as_deref())
                .await?
        };

        let mut url = self.upstream.clone();
        url.set_path(request.path.as_str());

        {
            let mut pairs = url.query_pairs_mut();
            if let Some(existing) = request.query.as_ref() {
                for (key, value) in form_urlencoded::parse(existing.as_bytes()) {
                    pairs.append_pair(&key, &value);
                }
            }
            pairs.append_pair("tavilyApiKey", lease.secret.as_str());
        }

        drop(url.query_pairs_mut());

        let sanitized_headers = self.sanitize_headers(&request.headers, &request.path);
        let request_method = request.method.clone();
        let request_body = request.body.clone();
        let request_url = url.clone();
        let tavily_secret = lease.secret.clone();
        let response = self
            .send_with_forward_proxy(&lease.id, "mcp", |client| {
                let mut builder = client.request(request_method.clone(), request_url.clone());
                for (name, value) in sanitized_headers.headers.iter() {
                    if name == HOST || name == CONTENT_LENGTH {
                        continue;
                    }
                    builder = builder.header(name, value);
                }
                builder
                    .header("Tavily-Api-Key", tavily_secret.as_str())
                    .body(request_body.clone())
            })
            .await;

        match response {
            Ok((response, _selected_proxy)) => {
                let status = response.status();
                let headers = response.headers().clone();
                let body_bytes = response.bytes().await.map_err(ProxyError::Http)?;
                let outcome = analyze_attempt(status, &body_bytes);

                log_success(
                    &lease.secret,
                    &request.method,
                    &request.path,
                    request.query.as_deref(),
                    status,
                );

                let key_effect = self
                    .reconcile_key_health(
                        &lease,
                        request.path.as_str(),
                        &outcome,
                        request.auth_token_id.as_deref(),
                    )
                    .await?;

                let request_log_id = self
                    .key_store
                    .log_attempt(AttemptLog {
                        key_id: Some(&lease.id),
                        auth_token_id: request.auth_token_id.as_deref(),
                        method: &request.method,
                        path: request.path.as_str(),
                        query: request.query.as_deref(),
                        status: Some(status),
                        tavily_status_code: outcome.tavily_status_code,
                        error: None,
                        request_body: &request.body,
                        response_body: &body_bytes,
                        outcome: outcome.status,
                        failure_kind: outcome.failure_kind.as_deref(),
                        key_effect_code: key_effect.code.as_str(),
                        key_effect_summary: key_effect.summary.as_deref(),
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;

                Ok(ProxyResponse {
                    status,
                    headers,
                    body: body_bytes,
                    api_key_id: Some(lease.id.clone()),
                    request_log_id: Some(request_log_id),
                    key_effect_code: key_effect.code,
                    key_effect_summary: key_effect.summary,
                })
            }
            Err(err) => {
                log_proxy_error(
                    &lease.secret,
                    &request.method,
                    &request.path,
                    request.query.as_deref(),
                    &err,
                );
                self.key_store
                    .log_attempt(AttemptLog {
                        key_id: Some(&lease.id),
                        auth_token_id: request.auth_token_id.as_deref(),
                        method: &request.method,
                        path: request.path.as_str(),
                        query: request.query.as_deref(),
                        status: None,
                        tavily_status_code: None,
                        error: Some(&err.to_string()),
                        request_body: &request.body,
                        response_body: &[],
                        outcome: OUTCOME_ERROR,
                        failure_kind: None,
                        key_effect_code: KEY_EFFECT_NONE,
                        key_effect_summary: None,
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                Err(err)
            }
        }
    }

    /// Generic helper to proxy a Tavily HTTP JSON endpoint (e.g. `/search`, `/extract`).
    /// It injects the Tavily key into the `api_key` field, performs header sanitization,
    /// records request logs with sensitive fields redacted, and updates key quota state.
    #[allow(clippy::too_many_arguments)]
    pub async fn proxy_http_json_endpoint(
        &self,
        usage_base: &str,
        upstream_path: &str,
        auth_token_id: Option<&str>,
        method: &Method,
        display_path: &str,
        options: Value,
        original_headers: &HeaderMap,
        inject_upstream_bearer_auth: bool,
    ) -> Result<(ProxyResponse, AttemptAnalysis), ProxyError> {
        let lease = self.acquire_key_for(auth_token_id).await?;

        let base = Url::parse(usage_base).map_err(|source| ProxyError::InvalidEndpoint {
            endpoint: usage_base.to_owned(),
            source,
        })?;
        let origin = origin_from_url(&base);

        let mut url = base.clone();
        url.set_path(upstream_path);

        let sanitized_headers = sanitize_headers_inner(original_headers, &base, &origin);

        // Build upstream request body by injecting Tavily key into api_key field.
        let mut upstream_options = options;
        if let Value::Object(ref mut map) = upstream_options {
            // Remove any existing api_key field (case-insensitive) before inserting the Tavily key.
            let keys_to_remove: Vec<String> = map
                .keys()
                .filter(|k| k.eq_ignore_ascii_case("api_key"))
                .cloned()
                .collect();
            for key in keys_to_remove {
                map.remove(&key);
            }
            map.insert("api_key".to_string(), Value::String(lease.secret.clone()));
        } else {
            // Unexpected payload shape; wrap it so we still send a valid JSON object upstream.
            let mut map = serde_json::Map::new();
            map.insert("api_key".to_string(), Value::String(lease.secret.clone()));
            map.insert("payload".to_string(), upstream_options);
            upstream_options = Value::Object(map);
        }

        // Force Tavily to return usage for predictable endpoints so we can charge credits 1:1.
        // Tavily does not document/support this on `/research` (we use /usage diff for that).
        if matches!(upstream_path, "/search" | "/extract" | "/crawl" | "/map")
            && let Value::Object(ref mut map) = upstream_options
        {
            map.insert("include_usage".to_string(), Value::Bool(true));
        }

        let request_body =
            serde_json::to_vec(&upstream_options).map_err(|e| ProxyError::Other(e.to_string()))?;
        let redacted_request_body = redact_api_key_bytes(&request_body);

        let request_method = method.clone();
        let request_url = url.clone();
        let upstream_secret = lease.secret.clone();
        let response = self
            .send_with_forward_proxy(&lease.id, upstream_path.trim_start_matches('/'), |client| {
                let mut builder = client.request(request_method.clone(), request_url.clone());
                for (name, value) in sanitized_headers.headers.iter() {
                    if name == HOST || name == CONTENT_LENGTH {
                        continue;
                    }
                    builder = builder.header(name, value);
                }
                if inject_upstream_bearer_auth {
                    builder =
                        builder.header("Authorization", format!("Bearer {}", upstream_secret));
                }
                builder.body(request_body.clone())
            })
            .await;

        match response {
            Ok((response, _selected_proxy)) => {
                let status = response.status();
                let headers = response.headers().clone();
                let body_bytes = response.bytes().await.map_err(ProxyError::Http)?;

                let mut analysis = analyze_http_attempt(status, &body_bytes);
                analysis.api_key_id = Some(lease.id.clone());
                if analysis.failure_kind.is_none() && analysis.status == OUTCOME_ERROR {
                    analysis.failure_kind = classify_failure_kind(
                        display_path,
                        Some(status.as_u16() as i64),
                        analysis.tavily_status_code,
                        None,
                        &body_bytes,
                    );
                }
                let redacted_response_body = redact_api_key_bytes(&body_bytes);
                if status.is_success()
                    && upstream_path == "/research"
                    && let Some(request_id) = extract_research_request_id(&body_bytes)
                    && let Some(token_id) = auth_token_id
                {
                    self.record_research_request_affinity(&request_id, &lease.id, token_id)
                        .await?;
                }

                let key_effect = self
                    .reconcile_key_health(&lease, display_path, &analysis, auth_token_id)
                    .await?;

                let request_log_id = self
                    .key_store
                    .log_attempt(AttemptLog {
                        key_id: Some(&lease.id),
                        auth_token_id,
                        method,
                        path: display_path,
                        query: None,
                        status: Some(status),
                        tavily_status_code: analysis.tavily_status_code,
                        error: None,
                        request_body: &redacted_request_body,
                        response_body: &redacted_response_body,
                        outcome: analysis.status,
                        failure_kind: analysis.failure_kind.as_deref(),
                        key_effect_code: key_effect.code.as_str(),
                        key_effect_summary: key_effect.summary.as_deref(),
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                analysis.key_effect = key_effect.clone();

                Ok((
                    ProxyResponse {
                        status,
                        headers,
                        body: body_bytes,
                        api_key_id: Some(lease.id.clone()),
                        request_log_id: Some(request_log_id),
                        key_effect_code: key_effect.code,
                        key_effect_summary: key_effect.summary,
                    },
                    analysis,
                ))
            }
            Err(err) => {
                log_proxy_error(&lease.secret, method, display_path, None, &err);
                let redacted_empty: Vec<u8> = Vec::new();
                self.key_store
                    .log_attempt(AttemptLog {
                        key_id: Some(&lease.id),
                        auth_token_id,
                        method,
                        path: display_path,
                        query: None,
                        status: None,
                        tavily_status_code: None,
                        error: Some(&err.to_string()),
                        request_body: &redacted_request_body,
                        response_body: &redacted_empty,
                        outcome: OUTCOME_ERROR,
                        failure_kind: None,
                        key_effect_code: KEY_EFFECT_NONE,
                        key_effect_summary: None,
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                Err(err)
            }
        }
    }

    /// Proxy Tavily `/research` while charging credits via `/usage` (research_usage) diff.
    ///
    /// Tavily research responses do not include `usage.credits`, so we probe
    /// `GET {usage_base}/usage` before and after the call using the *same* upstream key.
    ///
    /// Returns the usage delta when both probes succeed; otherwise `None`.
    #[allow(clippy::too_many_arguments)]
    pub async fn proxy_http_research_with_usage_diff(
        &self,
        usage_base: &str,
        auth_token_id: Option<&str>,
        method: &Method,
        display_path: &str,
        options: Value,
        original_headers: &HeaderMap,
        inject_upstream_bearer_auth: bool,
    ) -> Result<(ProxyResponse, AttemptAnalysis, Option<i64>), ProxyError> {
        let lease = self.acquire_key_for(auth_token_id).await?;
        // Research billing uses /usage diff of a key-scoped counter; protect it from concurrent
        // research calls sharing the same upstream key, otherwise deltas can be misattributed.
        let _key_guard = self.lock_research_key_usage(&lease.id).await?;

        let before_usage = match self
            .fetch_research_usage_for_secret_with_retries(
                &lease.secret,
                usage_base,
                Some(&lease.id),
                "research_usage_before",
            )
            .await
        {
            Ok(usage) => usage,
            Err(err) => {
                self.maybe_quarantine_usage_error(&lease.id, "/api/tavily/research#usage", &err)
                    .await?;
                return Err(err);
            }
        };

        let base = Url::parse(usage_base).map_err(|source| ProxyError::InvalidEndpoint {
            endpoint: usage_base.to_owned(),
            source,
        })?;
        let origin = origin_from_url(&base);

        let mut url = base.clone();
        url.set_path("/research");

        let sanitized_headers = sanitize_headers_inner(original_headers, &base, &origin);

        // Build upstream request body by injecting Tavily key into api_key field.
        let mut upstream_options = options;
        if let Value::Object(ref mut map) = upstream_options {
            let keys_to_remove: Vec<String> = map
                .keys()
                .filter(|k| k.eq_ignore_ascii_case("api_key"))
                .cloned()
                .collect();
            for key in keys_to_remove {
                map.remove(&key);
            }
            map.insert("api_key".to_string(), Value::String(lease.secret.clone()));
        } else {
            let mut map = serde_json::Map::new();
            map.insert("api_key".to_string(), Value::String(lease.secret.clone()));
            map.insert("payload".to_string(), upstream_options);
            upstream_options = Value::Object(map);
        }

        let request_body =
            serde_json::to_vec(&upstream_options).map_err(|e| ProxyError::Other(e.to_string()))?;
        let redacted_request_body = redact_api_key_bytes(&request_body);

        let request_method = method.clone();
        let request_url = url.clone();
        let upstream_secret = lease.secret.clone();
        let response = self
            .send_with_forward_proxy(&lease.id, "research", |client| {
                let mut builder = client.request(request_method.clone(), request_url.clone());
                for (name, value) in sanitized_headers.headers.iter() {
                    if name == HOST || name == CONTENT_LENGTH {
                        continue;
                    }
                    builder = builder.header(name, value);
                }
                if inject_upstream_bearer_auth {
                    builder =
                        builder.header("Authorization", format!("Bearer {}", upstream_secret));
                }
                builder.body(request_body.clone())
            })
            .await;

        match response {
            Ok((response, _selected_proxy)) => {
                let status = response.status();
                let headers = response.headers().clone();
                let body_bytes = response.bytes().await.map_err(ProxyError::Http)?;

                let mut analysis = analyze_http_attempt(status, &body_bytes);
                analysis.api_key_id = Some(lease.id.clone());
                if analysis.failure_kind.is_none() && analysis.status == OUTCOME_ERROR {
                    analysis.failure_kind = classify_failure_kind(
                        display_path,
                        Some(status.as_u16() as i64),
                        analysis.tavily_status_code,
                        None,
                        &body_bytes,
                    );
                }
                let redacted_response_body = redact_api_key_bytes(&body_bytes);
                if status.is_success()
                    && let Some(request_id) = extract_research_request_id(&body_bytes)
                    && let Some(token_id) = auth_token_id
                {
                    self.record_research_request_affinity(&request_id, &lease.id, token_id)
                        .await?;
                }

                let key_effect = self
                    .reconcile_key_health(&lease, display_path, &analysis, auth_token_id)
                    .await?;

                let request_log_id = self
                    .key_store
                    .log_attempt(AttemptLog {
                        key_id: Some(&lease.id),
                        auth_token_id,
                        method,
                        path: display_path,
                        query: None,
                        status: Some(status),
                        tavily_status_code: analysis.tavily_status_code,
                        error: None,
                        request_body: &redacted_request_body,
                        response_body: &redacted_response_body,
                        outcome: analysis.status,
                        failure_kind: analysis.failure_kind.as_deref(),
                        key_effect_code: key_effect.code.as_str(),
                        key_effect_summary: key_effect.summary.as_deref(),
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                analysis.key_effect = key_effect.clone();

                let after_usage = match self
                    .fetch_research_usage_for_secret_with_retries(
                        &lease.secret,
                        usage_base,
                        Some(&lease.id),
                        "research_usage_after",
                    )
                    .await
                {
                    Ok(usage) => Some(usage),
                    Err(err) => {
                        self.maybe_quarantine_usage_error(
                            &lease.id,
                            "/api/tavily/research#usage_after",
                            &err,
                        )
                        .await?;
                        None
                    }
                };
                let delta = match after_usage {
                    Some(after) if after >= before_usage => Some(after - before_usage),
                    _ => None,
                };

                Ok((
                    ProxyResponse {
                        status,
                        headers,
                        body: body_bytes,
                        api_key_id: Some(lease.id.clone()),
                        request_log_id: Some(request_log_id),
                        key_effect_code: key_effect.code,
                        key_effect_summary: key_effect.summary,
                    },
                    analysis,
                    delta,
                ))
            }
            Err(err) => {
                log_proxy_error(&lease.secret, method, display_path, None, &err);
                let redacted_empty: Vec<u8> = Vec::new();
                self.key_store
                    .log_attempt(AttemptLog {
                        key_id: Some(&lease.id),
                        auth_token_id,
                        method,
                        path: display_path,
                        query: None,
                        status: None,
                        tavily_status_code: None,
                        error: Some(&err.to_string()),
                        request_body: &redacted_request_body,
                        response_body: &redacted_empty,
                        outcome: OUTCOME_ERROR,
                        failure_kind: None,
                        key_effect_code: KEY_EFFECT_NONE,
                        key_effect_summary: None,
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                Err(err)
            }
        }
    }

    /// Generic helper to proxy a Tavily HTTP endpoint with no request body
    /// (for example `GET /research/{request_id}`).
    #[allow(clippy::too_many_arguments)]
    pub async fn proxy_http_get_endpoint(
        &self,
        usage_base: &str,
        upstream_path: &str,
        auth_token_id: Option<&str>,
        method: &Method,
        display_path: &str,
        original_headers: &HeaderMap,
        inject_upstream_bearer_auth: bool,
    ) -> Result<(ProxyResponse, AttemptAnalysis), ProxyError> {
        let research_request_id = extract_research_request_id_from_path(upstream_path);
        let lease = self
            .acquire_key_for_research_request(auth_token_id, research_request_id.as_deref())
            .await?;

        let base = Url::parse(usage_base).map_err(|source| ProxyError::InvalidEndpoint {
            endpoint: usage_base.to_owned(),
            source,
        })?;
        let origin = origin_from_url(&base);

        let mut url = base.clone();
        url.set_path(upstream_path);

        let sanitized_headers = sanitize_headers_inner(original_headers, &base, &origin);

        let redacted_request_body: Vec<u8> = Vec::new();
        let request_method = method.clone();
        let request_url = url.clone();
        let upstream_secret = lease.secret.clone();
        let response = self
            .send_with_forward_proxy(&lease.id, "research_result", |client| {
                let mut builder = client.request(request_method.clone(), request_url.clone());
                for (name, value) in sanitized_headers.headers.iter() {
                    if name == HOST || name == CONTENT_LENGTH {
                        continue;
                    }
                    builder = builder.header(name, value);
                }
                if inject_upstream_bearer_auth {
                    builder =
                        builder.header("Authorization", format!("Bearer {}", upstream_secret));
                }
                builder
            })
            .await;

        match response {
            Ok((response, _selected_proxy)) => {
                let status = response.status();
                let headers = response.headers().clone();
                let body_bytes = response.bytes().await.map_err(ProxyError::Http)?;

                let mut analysis = analyze_http_attempt(status, &body_bytes);
                analysis.api_key_id = Some(lease.id.clone());
                if analysis.failure_kind.is_none() && analysis.status == OUTCOME_ERROR {
                    analysis.failure_kind = classify_failure_kind(
                        display_path,
                        Some(status.as_u16() as i64),
                        analysis.tavily_status_code,
                        None,
                        &body_bytes,
                    );
                }
                let redacted_response_body = redact_api_key_bytes(&body_bytes);
                if status.is_success()
                    && let Some(request_id) = research_request_id.as_deref()
                    && let Some(token_id) = auth_token_id
                {
                    self.record_research_request_affinity(request_id, &lease.id, token_id)
                        .await?;
                }

                let key_effect = self
                    .reconcile_key_health(&lease, display_path, &analysis, auth_token_id)
                    .await?;

                let request_log_id = self
                    .key_store
                    .log_attempt(AttemptLog {
                        key_id: Some(&lease.id),
                        auth_token_id,
                        method,
                        path: display_path,
                        query: None,
                        status: Some(status),
                        tavily_status_code: analysis.tavily_status_code,
                        error: None,
                        request_body: &redacted_request_body,
                        response_body: &redacted_response_body,
                        outcome: analysis.status,
                        failure_kind: analysis.failure_kind.as_deref(),
                        key_effect_code: key_effect.code.as_str(),
                        key_effect_summary: key_effect.summary.as_deref(),
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                analysis.key_effect = key_effect.clone();

                Ok((
                    ProxyResponse {
                        status,
                        headers,
                        body: body_bytes,
                        api_key_id: Some(lease.id.clone()),
                        request_log_id: Some(request_log_id),
                        key_effect_code: key_effect.code,
                        key_effect_summary: key_effect.summary,
                    },
                    analysis,
                ))
            }
            Err(err) => {
                log_proxy_error(&lease.secret, method, display_path, None, &err);
                let redacted_empty: Vec<u8> = Vec::new();
                self.key_store
                    .log_attempt(AttemptLog {
                        key_id: Some(&lease.id),
                        auth_token_id,
                        method,
                        path: display_path,
                        query: None,
                        status: None,
                        tavily_status_code: None,
                        error: Some(&err.to_string()),
                        request_body: &redacted_request_body,
                        response_body: &redacted_empty,
                        outcome: OUTCOME_ERROR,
                        failure_kind: None,
                        key_effect_code: KEY_EFFECT_NONE,
                        key_effect_summary: None,
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                Err(err)
            }
        }
    }

    /// Proxy a Tavily HTTP `/search` call via the usage base URL, performing key rotation
    /// and recording request logs with sensitive fields redacted.
    pub async fn proxy_http_search(
        &self,
        usage_base: &str,
        auth_token_id: Option<&str>,
        method: &Method,
        display_path: &str,
        options: Value,
        original_headers: &HeaderMap,
    ) -> Result<(ProxyResponse, AttemptAnalysis), ProxyError> {
        self.proxy_http_json_endpoint(
            usage_base,
            "/search",
            auth_token_id,
            method,
            display_path,
            options,
            original_headers,
            true,
        )
        .await
    }

    /// 获取全部 API key 的统计信息，按状态与最近使用时间排序。
    pub async fn list_api_key_metrics(&self) -> Result<Vec<ApiKeyMetrics>, ProxyError> {
        self.key_store.fetch_api_key_metrics(false).await
    }

    /// Admin: list API key metrics with pagination and optional filters.
    pub async fn list_api_key_metrics_paged(
        &self,
        page: i64,
        per_page: i64,
        groups: &[String],
        statuses: &[String],
        registration_ip: Option<&str>,
        regions: &[String],
    ) -> Result<PaginatedApiKeyMetrics, ProxyError> {
        self.key_store
            .fetch_api_key_metrics_page(page, per_page, groups, statuses, registration_ip, regions)
            .await
    }

    /// 获取单个 API key 的完整统计信息，包含隔离详情。
    pub async fn get_api_key_metric(
        &self,
        key_id: &str,
    ) -> Result<Option<ApiKeyMetrics>, ProxyError> {
        self.key_store.fetch_api_key_metric_by_id(key_id).await
    }

    /// 获取最近的请求日志，按时间倒序排列。
    pub async fn recent_request_logs(
        &self,
        limit: usize,
    ) -> Result<Vec<RequestLogRecord>, ProxyError> {
        self.key_store.fetch_recent_logs(limit).await
    }

    /// Admin: recent request logs with simple pagination and optional result_status filter.
    pub async fn recent_request_logs_page(
        &self,
        result_status: Option<&str>,
        operational_class: Option<&str>,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<RequestLogRecord>, i64), ProxyError> {
        self.key_store
            .fetch_recent_logs_page(result_status, operational_class, page, per_page)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn request_logs_page(
        &self,
        request_kinds: &[String],
        result_status: Option<&str>,
        key_effect_code: Option<&str>,
        auth_token_id: Option<&str>,
        key_id: Option<&str>,
        operational_class: Option<&str>,
        page: i64,
        per_page: i64,
    ) -> Result<RequestLogsPage, ProxyError> {
        self.key_store
            .fetch_request_logs_page(
                None,
                None,
                request_kinds,
                result_status,
                key_effect_code,
                auth_token_id,
                key_id,
                operational_class,
                page,
                per_page,
                true,
                true,
            )
            .await
    }

    pub async fn request_log_bodies(
        &self,
        log_id: i64,
    ) -> Result<Option<RequestLogBodiesRecord>, ProxyError> {
        self.key_store.fetch_request_log_bodies(log_id).await
    }

    /// Rebuild API-key request buckets from visible request logs.
    pub async fn rebuild_api_key_usage_buckets(&self) -> Result<(), ProxyError> {
        self.key_store.rebuild_api_key_usage_buckets().await
    }

    /// 获取指定 key 在起始时间以来的汇总。
    pub async fn key_summary_since(
        &self,
        key_id: &str,
        since: i64,
    ) -> Result<ProxySummary, ProxyError> {
        self.key_store.fetch_key_summary_since(key_id, since).await
    }

    /// 获取指定 key 的最近日志（可选起始时间过滤）。
    pub async fn key_recent_logs(
        &self,
        key_id: &str,
        limit: usize,
        since: Option<i64>,
    ) -> Result<Vec<RequestLogRecord>, ProxyError> {
        self.key_store.fetch_key_logs(key_id, limit, since).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn key_logs_page(
        &self,
        key_id: &str,
        since: Option<i64>,
        request_kinds: &[String],
        result_status: Option<&str>,
        key_effect_code: Option<&str>,
        auth_token_id: Option<&str>,
        page: i64,
        per_page: i64,
    ) -> Result<RequestLogsPage, ProxyError> {
        self.key_store
            .fetch_request_logs_page(
                Some(key_id),
                since,
                request_kinds,
                result_status,
                key_effect_code,
                auth_token_id,
                None,
                None,
                page,
                per_page,
                true,
                false,
            )
            .await
    }

    pub async fn key_request_log_bodies(
        &self,
        key_id: &str,
        log_id: i64,
    ) -> Result<Option<RequestLogBodiesRecord>, ProxyError> {
        self.key_store
            .fetch_key_request_log_bodies(key_id, log_id)
            .await
    }

    pub async fn key_sticky_users_paged(
        &self,
        key_id: &str,
        page: i64,
        per_page: i64,
    ) -> Result<PaginatedApiKeyStickyUsers, ProxyError> {
        self.key_store
            .fetch_key_sticky_users_page(key_id, page, per_page)
            .await
    }

    pub async fn key_sticky_nodes(
        &self,
        key_id: &str,
    ) -> Result<ApiKeyStickyNodesResponse, ProxyError> {
        let record = self.resolve_proxy_affinity_record(key_id, false).await?;
        let manager = self.forward_proxy.lock().await.clone();
        let live =
            forward_proxy::build_forward_proxy_live_stats_response(&self.key_store.pool, &manager)
                .await?;
        let mut nodes = Vec::new();
        for (role, proxy_key) in [
            ("primary", record.primary_proxy_key.as_deref()),
            ("secondary", record.secondary_proxy_key.as_deref()),
        ] {
            let Some(proxy_key) = proxy_key else {
                continue;
            };
            if let Some(node) = live.nodes.iter().find(|node| node.key == proxy_key) {
                nodes.push(ApiKeyStickyNode {
                    role,
                    node: node.clone(),
                });
            }
        }
        Ok(ApiKeyStickyNodesResponse {
            range_start: live.range_start,
            range_end: live.range_end,
            bucket_seconds: live.bucket_seconds,
            nodes,
        })
    }

    // ----- Public auth token management API -----

    /// Validate an access token in format `th-<id>-<secret>` and record usage.
    /// Returns true if valid and enabled.
    pub async fn validate_access_token(&self, token: &str) -> Result<bool, ProxyError> {
        self.key_store.validate_access_token(token).await
    }

    /// Admin: create a new access token with optional note.
    pub async fn create_access_token(
        &self,
        note: Option<&str>,
    ) -> Result<AuthTokenSecret, ProxyError> {
        self.key_store.create_access_token(note).await
    }

    /// Admin: batch create access tokens with required group name.
    pub async fn create_access_tokens_batch(
        &self,
        group: &str,
        count: usize,
        note: Option<&str>,
    ) -> Result<Vec<AuthTokenSecret>, ProxyError> {
        self.key_store
            .create_access_tokens_batch(group, count, note)
            .await
    }

    /// Admin: list tokens for management.
    pub async fn list_access_tokens(&self) -> Result<Vec<AuthToken>, ProxyError> {
        let mut tokens = self.key_store.list_access_tokens().await?;
        self.populate_token_quota(&mut tokens).await?;
        Ok(tokens)
    }

    /// Admin: list tokens paginated.
    pub async fn list_access_tokens_paged(
        &self,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<AuthToken>, i64), ProxyError> {
        let (mut tokens, total) = self
            .key_store
            .list_access_tokens_paged(page, per_page)
            .await?;
        self.populate_token_quota(&mut tokens).await?;
        Ok((tokens, total))
    }

    pub(crate) async fn populate_token_quota(
        &self,
        tokens: &mut [AuthToken],
    ) -> Result<(), ProxyError> {
        if tokens.is_empty() {
            return Ok(());
        }
        let ids: Vec<String> = tokens.iter().map(|t| t.id.clone()).collect();
        let verdicts = self.token_quota.snapshot_many(&ids).await?;
        let token_bindings = self.key_store.list_user_bindings_for_tokens(&ids).await?;
        let now = Utc::now();
        let now_ts = now.timestamp();
        let minute_bucket = now_ts - (now_ts % 60);
        let hour_bucket = now_ts - (now_ts % SECS_PER_HOUR);
        let hour_window_start = minute_bucket - 59 * 60;
        let day_window_start = hour_bucket - 23 * SECS_PER_HOUR;
        let token_hourly_oldest = self
            .key_store
            .earliest_usage_bucket_since_bulk(&ids, GRANULARITY_MINUTE, hour_window_start)
            .await?;
        let token_daily_oldest = self
            .key_store
            .earliest_usage_bucket_since_bulk(&ids, GRANULARITY_HOUR, day_window_start)
            .await?;
        let mut user_ids: Vec<String> = token_bindings.values().cloned().collect();
        user_ids.sort_unstable();
        user_ids.dedup();
        let account_hourly_oldest = self
            .key_store
            .earliest_account_usage_bucket_since_bulk(
                &user_ids,
                GRANULARITY_MINUTE,
                hour_window_start,
            )
            .await?;
        let account_daily_oldest = self
            .key_store
            .earliest_account_usage_bucket_since_bulk(&user_ids, GRANULARITY_HOUR, day_window_start)
            .await?;
        let month_start = start_of_month(now);
        let next_month_reset = start_of_next_month(month_start).timestamp();
        for token in tokens.iter_mut() {
            if let Some(verdict) = verdicts.get(&token.id) {
                let hourly_oldest = if let Some(user_id) = token_bindings.get(&token.id) {
                    account_hourly_oldest.get(user_id).copied()
                } else {
                    token_hourly_oldest.get(&token.id).copied()
                };
                let daily_oldest = if let Some(user_id) = token_bindings.get(&token.id) {
                    account_daily_oldest.get(user_id).copied()
                } else {
                    token_daily_oldest.get(&token.id).copied()
                };
                token.quota_hourly_reset_at = if verdict.hourly_used > 0 {
                    hourly_oldest.map(|bucket| bucket + SECS_PER_HOUR)
                } else {
                    None
                };
                token.quota_daily_reset_at = if verdict.daily_used > 0 {
                    daily_oldest.map(|bucket| bucket + SECS_PER_DAY)
                } else {
                    None
                };
                token.quota_monthly_reset_at = if verdict.monthly_used > 0 {
                    Some(next_month_reset)
                } else {
                    None
                };
                token.quota = Some(verdict.clone());
            }
        }
        Ok(())
    }

    /// Admin: delete a token by id code.
    pub async fn delete_access_token(&self, id: &str) -> Result<(), ProxyError> {
        self.key_store.delete_access_token(id).await
    }

    /// Admin: set token enabled/disabled.
    pub async fn set_access_token_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<(), ProxyError> {
        self.key_store.set_access_token_enabled(id, enabled).await
    }

    /// Admin: update token note.
    pub async fn update_access_token_note(&self, id: &str, note: &str) -> Result<(), ProxyError> {
        self.key_store.update_access_token_note(id, note).await
    }

    /// Admin: get full token string for copy.
    pub async fn get_access_token_secret(
        &self,
        id: &str,
    ) -> Result<Option<AuthTokenSecret>, ProxyError> {
        self.key_store.get_access_token_secret(id).await
    }

    /// Admin: rotate token secret while keeping the same token id.
    /// Returns the new full token string (th-<id>-<secret>).
    pub async fn rotate_access_token_secret(
        &self,
        id: &str,
    ) -> Result<AuthTokenSecret, ProxyError> {
        self.key_store.rotate_access_token_secret(id).await
    }

    /// Create a one-time OAuth login state with TTL for CSRF/replay protection.
    pub async fn create_oauth_login_state(
        &self,
        provider: &str,
        redirect_to: Option<&str>,
        ttl_secs: i64,
    ) -> Result<String, ProxyError> {
        self.create_oauth_login_state_with_binding_and_token(
            provider,
            redirect_to,
            ttl_secs,
            None,
            None,
        )
        .await
    }

    /// Create a one-time OAuth login state bound to optional browser context hash.
    pub async fn create_oauth_login_state_with_binding(
        &self,
        provider: &str,
        redirect_to: Option<&str>,
        ttl_secs: i64,
        binding_hash: Option<&str>,
    ) -> Result<String, ProxyError> {
        self.create_oauth_login_state_with_binding_and_token(
            provider,
            redirect_to,
            ttl_secs,
            binding_hash,
            None,
        )
        .await
    }

    /// Create a one-time OAuth login state bound to optional browser context hash and token id.
    pub async fn create_oauth_login_state_with_binding_and_token(
        &self,
        provider: &str,
        redirect_to: Option<&str>,
        ttl_secs: i64,
        binding_hash: Option<&str>,
        bind_token_id: Option<&str>,
    ) -> Result<String, ProxyError> {
        self.key_store
            .insert_oauth_login_state(provider, redirect_to, ttl_secs, binding_hash, bind_token_id)
            .await
    }

    /// Consume and invalidate an OAuth login state. Returns redirect target when valid.
    pub async fn consume_oauth_login_state(
        &self,
        provider: &str,
        state: &str,
    ) -> Result<Option<Option<String>>, ProxyError> {
        Ok(self
            .consume_oauth_login_state_with_binding_and_token(provider, state, None)
            .await?
            .map(|payload| payload.redirect_to))
    }

    /// Consume and invalidate an OAuth login state bound to optional browser context hash.
    pub async fn consume_oauth_login_state_with_binding(
        &self,
        provider: &str,
        state: &str,
        binding_hash: Option<&str>,
    ) -> Result<Option<Option<String>>, ProxyError> {
        Ok(self
            .consume_oauth_login_state_with_binding_and_token(provider, state, binding_hash)
            .await?
            .map(|payload| payload.redirect_to))
    }

    /// Consume and invalidate an OAuth login state and return all payload fields.
    pub async fn consume_oauth_login_state_with_binding_and_token(
        &self,
        provider: &str,
        state: &str,
        binding_hash: Option<&str>,
    ) -> Result<Option<OAuthLoginStatePayload>, ProxyError> {
        self.key_store
            .consume_oauth_login_state(provider, state, binding_hash)
            .await
    }

    /// Upsert local user identity from third-party OAuth profile.
    pub async fn upsert_oauth_account(
        &self,
        profile: &OAuthAccountProfile,
    ) -> Result<UserIdentity, ProxyError> {
        self.key_store.upsert_oauth_account(profile).await
    }

    /// Check whether a third-party account already exists locally.
    pub async fn oauth_account_exists(
        &self,
        provider: &str,
        provider_user_id: &str,
    ) -> Result<bool, ProxyError> {
        self.key_store
            .oauth_account_exists(provider, provider_user_id)
            .await
    }

    /// Read whether first-time third-party registration is enabled.
    pub async fn allow_registration(&self) -> Result<bool, ProxyError> {
        self.key_store.allow_registration().await
    }

    /// Persist whether first-time third-party registration is enabled.
    pub async fn set_allow_registration(&self, allow: bool) -> Result<bool, ProxyError> {
        self.key_store.set_allow_registration(allow).await
    }

    /// Ensure one-to-one user token binding exists, creating a token only when missing.
    pub async fn ensure_user_token_binding(
        &self,
        user_id: &str,
        note: Option<&str>,
    ) -> Result<AuthTokenSecret, ProxyError> {
        self.key_store
            .ensure_user_token_binding(user_id, note)
            .await
    }

    /// Ensure binding with an optional preferred token id. Falls back to default behavior.
    pub async fn ensure_user_token_binding_with_preferred(
        &self,
        user_id: &str,
        note: Option<&str>,
        preferred_token_id: Option<&str>,
    ) -> Result<AuthTokenSecret, ProxyError> {
        self.key_store
            .ensure_user_token_binding_with_preferred(user_id, note, preferred_token_id)
            .await
    }

    /// Fetch current user token by user_id. Does not auto-recreate when unavailable.
    pub async fn get_user_token(&self, user_id: &str) -> Result<UserTokenLookup, ProxyError> {
        self.key_store.get_user_token(user_id).await
    }

    /// List tokens bound to the specified user.
    pub async fn list_user_tokens(&self, user_id: &str) -> Result<Vec<AuthToken>, ProxyError> {
        let mut tokens = self.key_store.list_user_tokens(user_id).await?;
        self.populate_token_quota(&mut tokens).await?;
        Ok(tokens)
    }

    /// Verify whether a token belongs to the specified user.
    pub async fn is_user_token_bound(
        &self,
        user_id: &str,
        token_id: &str,
    ) -> Result<bool, ProxyError> {
        self.key_store.is_user_token_bound(user_id, token_id).await
    }

    /// Get a token secret only when the token belongs to the specified user.
    pub async fn get_user_token_secret(
        &self,
        user_id: &str,
        token_id: &str,
    ) -> Result<Option<AuthTokenSecret>, ProxyError> {
        self.key_store
            .get_user_token_secret(user_id, token_id)
            .await
    }

    /// User-level quota and usage summary for dashboard.
    pub async fn user_dashboard_summary(
        &self,
        user_id: &str,
    ) -> Result<UserDashboardSummary, ProxyError> {
        let mut summaries = self
            .user_dashboard_summaries_for_users(&[user_id.to_string()])
            .await?;
        Ok(summaries.remove(user_id).unwrap_or(UserDashboardSummary {
            hourly_any_used: 0,
            hourly_any_limit: 0,
            quota_hourly_used: 0,
            quota_hourly_limit: 0,
            quota_daily_used: 0,
            quota_daily_limit: 0,
            quota_monthly_used: 0,
            quota_monthly_limit: 0,
            daily_success: 0,
            daily_failure: 0,
            monthly_success: 0,
            monthly_failure: 0,
            last_activity: None,
        }))
    }

    /// Admin: resolve dashboard summaries for many users without N+1 queries.
    pub async fn user_dashboard_summaries_for_users(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, UserDashboardSummary>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let now = Utc::now();
        let now_ts = now.timestamp();
        let minute_bucket = now_ts - (now_ts % SECS_PER_MINUTE);
        let hour_bucket = now_ts - (now_ts % SECS_PER_HOUR);
        let hour_window_start = minute_bucket - 59 * SECS_PER_MINUTE;
        let day_window_start = hour_bucket - 23 * SECS_PER_HOUR;
        let month_start = start_of_month(now).timestamp();

        let mut deduped_user_ids = user_ids.to_vec();
        deduped_user_ids.sort_unstable();
        deduped_user_ids.dedup();

        let account_limits = self
            .key_store
            .resolve_account_quota_limits_bulk(&deduped_user_ids)
            .await?;
        let hourly_any_totals = self
            .key_store
            .sum_account_usage_buckets_bulk(
                &deduped_user_ids,
                GRANULARITY_REQUEST_MINUTE,
                hour_window_start,
            )
            .await?;
        let hourly_totals = self
            .key_store
            .sum_account_usage_buckets_bulk(
                &deduped_user_ids,
                GRANULARITY_MINUTE,
                hour_window_start,
            )
            .await?;
        let daily_totals = self
            .key_store
            .sum_account_usage_buckets_bulk(&deduped_user_ids, GRANULARITY_HOUR, day_window_start)
            .await?;
        let monthly_totals = self
            .key_store
            .fetch_account_monthly_counts(&deduped_user_ids, month_start)
            .await?;
        let log_metrics = self
            .key_store
            .fetch_user_log_metrics_bulk(&deduped_user_ids)
            .await?;
        let default_limits = AccountQuotaLimits::zero_base();

        Ok(deduped_user_ids
            .into_iter()
            .map(|user_id| {
                let limits = account_limits
                    .get(&user_id)
                    .cloned()
                    .unwrap_or_else(|| default_limits.clone());
                let metrics = log_metrics.get(&user_id).cloned().unwrap_or_default();
                (
                    user_id.clone(),
                    UserDashboardSummary {
                        hourly_any_used: hourly_any_totals.get(&user_id).copied().unwrap_or(0),
                        hourly_any_limit: limits.hourly_any_limit,
                        quota_hourly_used: hourly_totals.get(&user_id).copied().unwrap_or(0),
                        quota_hourly_limit: limits.hourly_limit,
                        quota_daily_used: daily_totals.get(&user_id).copied().unwrap_or(0),
                        quota_daily_limit: limits.daily_limit,
                        quota_monthly_used: monthly_totals.get(&user_id).copied().unwrap_or(0),
                        quota_monthly_limit: limits.monthly_limit,
                        daily_success: metrics.daily_success,
                        daily_failure: metrics.daily_failure,
                        monthly_success: metrics.monthly_success,
                        monthly_failure: metrics.monthly_failure,
                        last_activity: metrics.last_activity,
                    },
                )
            })
            .collect())
    }

    pub async fn token_log_metrics_for_tokens(
        &self,
        token_ids: &[String],
    ) -> Result<HashMap<String, TokenLogMetricsSummary>, ProxyError> {
        self.key_store.fetch_token_log_metrics_bulk(token_ids).await
    }

    pub async fn list_api_key_binding_counts_for_users(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, i64>, ProxyError> {
        self.key_store
            .list_api_key_binding_counts_for_users(user_ids)
            .await
    }

    async fn backfill_current_month_broken_key_subjects(&self) -> Result<(), ProxyError> {
        self.key_store
            .backfill_current_month_auto_subject_breakages()
            .await
    }

    pub async fn fetch_account_monthly_broken_limit(
        &self,
        user_id: &str,
    ) -> Result<i64, ProxyError> {
        self.key_store
            .fetch_account_monthly_broken_limit(user_id)
            .await
    }

    pub async fn fetch_account_monthly_broken_limits_bulk(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, i64>, ProxyError> {
        self.key_store
            .fetch_account_monthly_broken_limits_bulk(user_ids)
            .await
    }

    pub async fn update_account_monthly_broken_limit(
        &self,
        user_id: &str,
        monthly_broken_limit: i64,
    ) -> Result<bool, ProxyError> {
        self.key_store
            .update_account_monthly_broken_limit(user_id, monthly_broken_limit)
            .await
    }

    pub async fn fetch_monthly_broken_counts_for_users(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, i64>, ProxyError> {
        self.backfill_current_month_broken_key_subjects().await?;
        self.key_store
            .fetch_monthly_broken_counts_for_users(user_ids, start_of_month(Utc::now()).timestamp())
            .await
    }

    pub async fn fetch_monthly_broken_counts_for_tokens(
        &self,
        token_ids: &[String],
    ) -> Result<HashMap<String, i64>, ProxyError> {
        self.backfill_current_month_broken_key_subjects().await?;
        self.key_store
            .fetch_monthly_broken_counts_for_tokens(
                token_ids,
                start_of_month(Utc::now()).timestamp(),
            )
            .await
    }

    pub async fn list_monthly_broken_subjects_for_tokens(
        &self,
        token_ids: &[String],
    ) -> Result<HashSet<String>, ProxyError> {
        self.backfill_current_month_broken_key_subjects().await?;
        self.key_store
            .list_monthly_broken_subjects_for_tokens(
                token_ids,
                start_of_month(Utc::now()).timestamp(),
            )
            .await
    }

    pub async fn fetch_user_monthly_broken_keys(
        &self,
        user_id: &str,
        page: i64,
        per_page: i64,
    ) -> Result<PaginatedMonthlyBrokenKeys, ProxyError> {
        self.backfill_current_month_broken_key_subjects().await?;
        self.key_store
            .fetch_monthly_broken_keys_page(
                BROKEN_KEY_SUBJECT_USER,
                user_id,
                page,
                per_page,
                start_of_month(Utc::now()).timestamp(),
            )
            .await
    }

    pub async fn fetch_token_monthly_broken_keys(
        &self,
        token_id: &str,
        page: i64,
        per_page: i64,
    ) -> Result<PaginatedMonthlyBrokenKeys, ProxyError> {
        self.backfill_current_month_broken_key_subjects().await?;
        self.key_store
            .fetch_monthly_broken_keys_page(
                BROKEN_KEY_SUBJECT_TOKEN,
                token_id,
                page,
                per_page,
                start_of_month(Utc::now()).timestamp(),
            )
            .await
    }

    /// Admin: list users with pagination and optional fuzzy query.
    pub async fn list_admin_users_paged(
        &self,
        page: i64,
        per_page: i64,
        query: Option<&str>,
        tag_id: Option<&str>,
    ) -> Result<(Vec<AdminUserIdentity>, i64), ProxyError> {
        self.key_store
            .list_admin_users_paged(page, per_page, query, tag_id)
            .await
    }

    /// Admin: list the full filtered user set prior to sorting and pagination.
    pub async fn list_admin_users_filtered(
        &self,
        query: Option<&str>,
        tag_id: Option<&str>,
    ) -> Result<Vec<AdminUserIdentity>, ProxyError> {
        self.key_store
            .list_admin_users_filtered(query, tag_id)
            .await
    }

    /// Admin: get a single user identity by id.
    pub async fn get_admin_user_identity(
        &self,
        user_id: &str,
    ) -> Result<Option<AdminUserIdentity>, ProxyError> {
        self.key_store.get_admin_user_identity(user_id).await
    }

    /// Admin: resolve token owners in bulk for management views.
    pub async fn get_admin_token_owners(
        &self,
        token_ids: &[String],
    ) -> Result<HashMap<String, AdminUserIdentity>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let token_bindings = self
            .key_store
            .list_user_bindings_for_tokens(token_ids)
            .await?;
        if token_bindings.is_empty() {
            return Ok(HashMap::new());
        }

        let mut user_ids: Vec<String> = token_bindings.values().cloned().collect();
        user_ids.sort_unstable();
        user_ids.dedup();

        let user_map = self.key_store.get_admin_user_identities(&user_ids).await?;
        let mut owners = HashMap::with_capacity(token_bindings.len());
        for (token_id, user_id) in token_bindings {
            if let Some(identity) = user_map.get(&user_id) {
                owners.insert(token_id, identity.clone());
            }
        }
        Ok(owners)
    }

    /// Admin: upsert account quota limits for a user.
    pub async fn update_account_quota_limits(
        &self,
        user_id: &str,
        hourly_any_limit: i64,
        hourly_limit: i64,
        daily_limit: i64,
        monthly_limit: i64,
    ) -> Result<bool, ProxyError> {
        self.key_store
            .update_account_quota_limits(
                user_id,
                hourly_any_limit,
                hourly_limit,
                daily_limit,
                monthly_limit,
            )
            .await
    }

    /// Admin: list all user tag definitions.
    pub async fn list_user_tags(&self) -> Result<Vec<AdminUserTag>, ProxyError> {
        Ok(self
            .key_store
            .list_user_tags()
            .await?
            .into_iter()
            .map(|tag| to_admin_user_tag(&tag))
            .collect())
    }

    /// Admin: create a custom user tag.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_user_tag(
        &self,
        name: &str,
        display_name: &str,
        icon: Option<&str>,
        effect_kind: &str,
        hourly_any_delta: i64,
        hourly_delta: i64,
        daily_delta: i64,
        monthly_delta: i64,
    ) -> Result<AdminUserTag, ProxyError> {
        self.key_store
            .create_user_tag(
                name,
                display_name,
                icon,
                effect_kind,
                hourly_any_delta,
                hourly_delta,
                daily_delta,
                monthly_delta,
            )
            .await
            .map(|tag| to_admin_user_tag(&tag))
    }

    /// Admin: update an existing user tag definition.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_user_tag(
        &self,
        tag_id: &str,
        name: &str,
        display_name: &str,
        icon: Option<&str>,
        effect_kind: &str,
        hourly_any_delta: i64,
        hourly_delta: i64,
        daily_delta: i64,
        monthly_delta: i64,
    ) -> Result<Option<AdminUserTag>, ProxyError> {
        self.key_store
            .update_user_tag(
                tag_id,
                name,
                display_name,
                icon,
                effect_kind,
                hourly_any_delta,
                hourly_delta,
                daily_delta,
                monthly_delta,
            )
            .await
            .map(|tag| tag.map(|it| to_admin_user_tag(&it)))
    }

    /// Admin: delete a custom user tag definition.
    pub async fn delete_user_tag(&self, tag_id: &str) -> Result<bool, ProxyError> {
        self.key_store.delete_user_tag(tag_id).await
    }

    /// Admin: bind a custom tag to a user.
    pub async fn bind_user_tag_to_user(
        &self,
        user_id: &str,
        tag_id: &str,
    ) -> Result<bool, ProxyError> {
        self.key_store.bind_user_tag_to_user(user_id, tag_id).await
    }

    /// Admin: unbind a tag from a user.
    pub async fn unbind_user_tag_from_user(
        &self,
        user_id: &str,
        tag_id: &str,
    ) -> Result<bool, ProxyError> {
        self.key_store
            .unbind_user_tag_from_user(user_id, tag_id)
            .await
    }

    /// Admin: list tag bindings for a set of users.
    pub async fn list_user_tag_bindings_for_users(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, Vec<AdminUserTagBinding>>, ProxyError> {
        let bindings = self
            .key_store
            .list_user_tag_bindings_for_users(user_ids)
            .await?;
        Ok(bindings
            .into_iter()
            .map(|(user_id, items)| {
                (
                    user_id,
                    items
                        .into_iter()
                        .map(|binding| to_admin_user_tag_binding(&binding))
                        .collect(),
                )
            })
            .collect())
    }

    /// Admin: resolve base/effective quota and breakdown for a user.
    pub async fn get_admin_user_quota_details(
        &self,
        user_id: &str,
    ) -> Result<Option<AdminUserQuotaDetails>, ProxyError> {
        let Some(_) = self.key_store.get_admin_user_identity(user_id).await? else {
            return Ok(None);
        };
        let resolution = self
            .key_store
            .resolve_account_quota_resolution(user_id)
            .await?;
        Ok(Some(AdminUserQuotaDetails {
            base: to_admin_quota_limit_set(&resolution.base),
            effective: to_admin_quota_limit_set(&resolution.effective),
            breakdown: resolution
                .breakdown
                .iter()
                .map(to_admin_quota_breakdown_entry)
                .collect(),
            tags: resolution
                .tags
                .iter()
                .map(to_admin_user_tag_binding)
                .collect(),
        }))
    }

    /// Create persisted user session.
    pub async fn create_user_session(
        &self,
        user: &UserIdentity,
        session_max_age_secs: i64,
    ) -> Result<UserSession, ProxyError> {
        self.key_store
            .create_user_session(user, session_max_age_secs)
            .await
    }

    /// Lookup valid user session from cookie token.
    pub async fn get_user_session(&self, token: &str) -> Result<Option<UserSession>, ProxyError> {
        self.key_store.get_user_session(token).await
    }

    /// Revoke persisted user session token.
    pub async fn revoke_user_session(&self, token: &str) -> Result<(), ProxyError> {
        self.key_store.revoke_user_session(token).await
    }

    /// Record a token usage log. Intended for /mcp proxy handler.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_local_request_log_without_key(
        &self,
        auth_token_id: Option<&str>,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: StatusCode,
        mcp_status: Option<i64>,
        request_body: &[u8],
        response_body: &[u8],
        result_status: &str,
        failure_kind: Option<&str>,
        forwarded_headers: &[String],
        dropped_headers: &[String],
    ) -> Result<i64, ProxyError> {
        self.key_store
            .log_attempt(AttemptLog {
                key_id: None,
                auth_token_id,
                method,
                path,
                query,
                status: Some(http_status),
                tavily_status_code: mcp_status,
                error: None,
                request_body,
                response_body,
                outcome: result_status,
                failure_kind,
                key_effect_code: KEY_EFFECT_NONE,
                key_effect_summary: None,
                forwarded_headers,
                dropped_headers,
            })
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_token_attempt(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
    ) -> Result<(), ProxyError> {
        self.record_token_attempt_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            None,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_token_attempt_metadata(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
    ) -> Result<(), ProxyError> {
        self.record_token_attempt_request_log_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_token_attempt_request_log_metadata(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
        request_log_id: Option<i64>,
    ) -> Result<(), ProxyError> {
        let request_kind = classify_token_request_kind(path, None);
        self.record_token_attempt_with_kind_request_log_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            &request_kind,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            request_log_id,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_token_attempt_with_kind(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        request_kind: &TokenRequestKind,
    ) -> Result<(), ProxyError> {
        self.record_token_attempt_with_kind_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            request_kind,
            None,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_token_attempt_with_kind_metadata(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        request_kind: &TokenRequestKind,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
    ) -> Result<(), ProxyError> {
        self.record_token_attempt_with_kind_request_log_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            request_kind,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_token_attempt_with_kind_request_log_metadata(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        request_kind: &TokenRequestKind,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
        request_log_id: Option<i64>,
    ) -> Result<(), ProxyError> {
        self.key_store
            .insert_token_log(
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                counts_business_quota,
                result_status,
                error_message,
                request_kind,
                failure_kind,
                key_effect_code.unwrap_or(KEY_EFFECT_NONE),
                key_effect_summary,
                request_log_id,
            )
            .await
    }

    /// Persist a billable attempt before quota counters are charged, so it can be replayed if the
    /// process crashes after the upstream call succeeds.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        api_key_id: Option<&str>,
    ) -> Result<i64, ProxyError> {
        self.record_pending_billing_attempt_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            api_key_id,
            None,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_metadata(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        api_key_id: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
    ) -> Result<i64, ProxyError> {
        self.record_pending_billing_attempt_request_log_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            api_key_id,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_request_log_metadata(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        api_key_id: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
        request_log_id: Option<i64>,
    ) -> Result<i64, ProxyError> {
        let request_kind = classify_token_request_kind(path, None);
        self.record_pending_billing_attempt_with_kind_request_log_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            &request_kind,
            api_key_id,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            request_log_id,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_with_kind(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        request_kind: &TokenRequestKind,
        api_key_id: Option<&str>,
    ) -> Result<i64, ProxyError> {
        self.record_pending_billing_attempt_with_kind_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            request_kind,
            api_key_id,
            None,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_with_kind_metadata(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        request_kind: &TokenRequestKind,
        api_key_id: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
    ) -> Result<i64, ProxyError> {
        self.record_pending_billing_attempt_with_kind_request_log_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            request_kind,
            api_key_id,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_with_kind_request_log_metadata(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        request_kind: &TokenRequestKind,
        api_key_id: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
        request_log_id: Option<i64>,
    ) -> Result<i64, ProxyError> {
        let billing_subject = self.billing_subject_for_token(token_id).await?;
        self.record_pending_billing_attempt_for_subject_with_kind_request_log(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            &billing_subject,
            request_kind,
            api_key_id,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            request_log_id,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_for_subject(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        billing_subject: &str,
        api_key_id: Option<&str>,
    ) -> Result<i64, ProxyError> {
        self.record_pending_billing_attempt_for_subject_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            billing_subject,
            api_key_id,
            None,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_for_subject_metadata(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        billing_subject: &str,
        api_key_id: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
    ) -> Result<i64, ProxyError> {
        self.record_pending_billing_attempt_for_subject_request_log_metadata(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            billing_subject,
            api_key_id,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_for_subject_request_log_metadata(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        billing_subject: &str,
        api_key_id: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
        request_log_id: Option<i64>,
    ) -> Result<i64, ProxyError> {
        let request_kind = classify_token_request_kind(path, None);
        self.record_pending_billing_attempt_for_subject_with_kind_request_log(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            billing_subject,
            &request_kind,
            api_key_id,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            request_log_id,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_for_subject_with_kind(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        billing_subject: &str,
        request_kind: &TokenRequestKind,
        api_key_id: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
    ) -> Result<i64, ProxyError> {
        self.record_pending_billing_attempt_for_subject_with_kind_request_log(
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            counts_business_quota,
            result_status,
            error_message,
            business_credits,
            billing_subject,
            request_kind,
            api_key_id,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_pending_billing_attempt_for_subject_with_kind_request_log(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        billing_subject: &str,
        request_kind: &TokenRequestKind,
        api_key_id: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: Option<&str>,
        key_effect_summary: Option<&str>,
        request_log_id: Option<i64>,
    ) -> Result<i64, ProxyError> {
        self.key_store
            .insert_token_log_pending_billing(
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                counts_business_quota,
                result_status,
                error_message,
                business_credits,
                billing_subject,
                request_kind,
                api_key_id,
                failure_kind,
                key_effect_code.unwrap_or(KEY_EFFECT_NONE),
                key_effect_summary,
                request_log_id,
            )
            .await
    }

    pub async fn settle_pending_billing_attempt(
        &self,
        log_id: i64,
    ) -> Result<PendingBillingSettleOutcome, ProxyError> {
        self.key_store.apply_pending_billing_log(log_id).await
    }

    pub async fn annotate_pending_billing_attempt(
        &self,
        log_id: i64,
        message: &str,
    ) -> Result<(), ProxyError> {
        self.key_store
            .annotate_pending_billing_log(log_id, message)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn force_pending_billing_claim_miss_once(&self, log_id: i64) {
        let mut forced = self
            .key_store
            .forced_pending_claim_miss_log_ids
            .lock()
            .await;
        forced.insert(log_id);
    }

    #[doc(hidden)]
    #[allow(dead_code)]
    pub fn force_quota_subject_lock_loss_once_for_subject(&self, billing_subject: &str) {
        let mut forced = self
            .key_store
            .forced_quota_subject_lock_loss_subjects
            .lock()
            .expect("forced quota subject lock loss mutex poisoned");
        forced.insert(billing_subject.to_string());
    }

    /// Token summary since a timestamp
    pub async fn token_summary_since(
        &self,
        token_id: &str,
        since: i64,
        until: Option<i64>,
    ) -> Result<TokenSummary, ProxyError> {
        self.key_store
            .fetch_token_summary_since(token_id, since, until)
            .await
    }

    /// Token recent logs with optional before-id pagination
    pub async fn token_recent_logs(
        &self,
        token_id: &str,
        limit: usize,
        before_id: Option<i64>,
    ) -> Result<Vec<TokenLogRecord>, ProxyError> {
        self.key_store
            .fetch_token_logs(token_id, limit, before_id)
            .await
    }

    /// Check and update quota usage for a token. Returns the latest counts and verdict.
    pub async fn check_token_quota(&self, token_id: &str) -> Result<TokenQuotaVerdict, ProxyError> {
        self.token_quota.check(token_id).await
    }

    /// Read-only snapshot of the current business quota usage for a token (hour/day/month).
    /// This does NOT increment any counters.
    pub async fn peek_token_quota(&self, token_id: &str) -> Result<TokenQuotaVerdict, ProxyError> {
        let now = Utc::now();
        self.token_quota.snapshot_for_token(token_id, now).await
    }

    /// Read-only snapshot for a locked billing subject. Use this when a request must keep the
    /// same quota subject from precheck through charge even if token bindings change mid-flight.
    pub async fn peek_token_quota_for_subject(
        &self,
        billing_subject: &str,
    ) -> Result<TokenQuotaVerdict, ProxyError> {
        let now = Utc::now();
        self.token_quota
            .snapshot_for_billing_subject(billing_subject, now)
            .await
    }

    /// Charge business quota usage for a token by Tavily credits (1:1).
    /// `credits <= 0` is treated as a no-op.
    pub async fn charge_token_quota(&self, token_id: &str, credits: i64) -> Result<(), ProxyError> {
        self.token_quota.charge(token_id, credits).await
    }

    /// Check and update the hourly *raw request* usage for a token.
    /// This limiter counts every authenticated request (regardless of MCP method)
    /// within the last rolling hour and enforces `TOKEN_HOURLY_REQUEST_LIMIT`.
    pub async fn check_token_hourly_requests(
        &self,
        token_id: &str,
    ) -> Result<TokenHourlyRequestVerdict, ProxyError> {
        self.token_request_limit.check(token_id).await
    }

    /// Read-only snapshot of hourly raw request usage for a set of tokens.
    /// Used by dashboards / leaderboards; does not increment counters.
    pub async fn token_hourly_any_snapshot(
        &self,
        token_ids: &[String],
    ) -> Result<HashMap<String, TokenHourlyRequestVerdict>, ProxyError> {
        self.token_request_limit.snapshot_many(token_ids).await
    }

    /// Read-only snapshot of current token quota usage (hour / day / month).
    pub async fn token_quota_snapshot(
        &self,
        token_id: &str,
    ) -> Result<Option<TokenQuotaVerdict>, ProxyError> {
        let now = Utc::now();
        let verdict = self.token_quota.snapshot_for_token(token_id, now).await?;
        Ok(Some(verdict))
    }

    /// Token logs (page-based pagination)
    #[allow(clippy::too_many_arguments)]
    pub async fn token_logs_page(
        &self,
        token_id: &str,
        page: usize,
        per_page: usize,
        since: i64,
        until: Option<i64>,
        request_kinds: &[String],
        result_status: Option<&str>,
        key_effect_code: Option<&str>,
        key_id: Option<&str>,
        operational_class: Option<&str>,
    ) -> Result<TokenLogsPage, ProxyError> {
        self.key_store
            .fetch_token_logs_page(
                token_id,
                page,
                per_page,
                since,
                until,
                request_kinds,
                result_status,
                key_effect_code,
                key_id,
                operational_class,
            )
            .await
    }

    pub async fn token_request_log_bodies(
        &self,
        token_id: &str,
        log_id: i64,
    ) -> Result<Option<RequestLogBodiesRecord>, ProxyError> {
        self.key_store
            .fetch_token_log_bodies(token_id, log_id)
            .await
    }

    pub async fn token_log_request_kind_options(
        &self,
        token_id: &str,
        since: i64,
        until: Option<i64>,
    ) -> Result<Vec<TokenRequestKindOption>, ProxyError> {
        self.key_store
            .fetch_token_log_request_kind_options(token_id, since, until)
            .await
    }

    /// Hourly breakdown for recent N hours (success + non-success aggregated as error).
    pub async fn token_hourly_breakdown(
        &self,
        token_id: &str,
        hours: i64,
    ) -> Result<Vec<TokenHourlyBucket>, ProxyError> {
        self.key_store
            .fetch_token_hourly_breakdown(token_id, hours)
            .await
    }

    /// Generic usage series for arbitrary window and granularity.
    pub async fn token_usage_series(
        &self,
        token_id: &str,
        since: i64,
        until: i64,
        bucket_secs: i64,
    ) -> Result<Vec<TokenUsageBucket>, ProxyError> {
        self.key_store
            .fetch_token_usage_series(token_id, since, until, bucket_secs)
            .await
    }

    /// 根据 ID 获取真实 API key，仅供管理员调用。
    pub async fn get_api_key_secret(&self, key_id: &str) -> Result<Option<String>, ProxyError> {
        self.key_store.fetch_api_key_secret(key_id).await
    }

    /// Admin: add or undelete an API key. Returns the key ID.
    pub async fn add_or_undelete_key(&self, api_key: &str) -> Result<String, ProxyError> {
        self.key_store.add_or_undelete_key(api_key).await
    }

    /// Admin: add or undelete an API key and optionally assign it to a group.
    pub async fn add_or_undelete_key_in_group(
        &self,
        api_key: &str,
        group: Option<&str>,
    ) -> Result<String, ProxyError> {
        self.key_store
            .add_or_undelete_key_in_group(api_key, group)
            .await
    }

    /// Admin: add/undelete an API key and return the upsert status.
    pub async fn add_or_undelete_key_with_status(
        &self,
        api_key: &str,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        self.key_store
            .add_or_undelete_key_with_status(api_key)
            .await
    }

    /// Admin: add/undelete an API key in the provided group and return the upsert status.
    pub async fn add_or_undelete_key_with_status_in_group(
        &self,
        api_key: &str,
        group: Option<&str>,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        self.key_store
            .add_or_undelete_key_with_status_in_group(api_key, group)
            .await
    }

    /// Admin: add/undelete an API key in the provided group and refresh registration metadata
    /// when the caller provides a new registration IP.
    pub async fn add_or_undelete_key_with_status_in_group_and_registration(
        &self,
        api_key: &str,
        group: Option<&str>,
        registration_ip: Option<&str>,
        registration_region: Option<&str>,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        self.key_store
            .add_or_undelete_key_with_status_in_group_and_registration(
                api_key,
                group,
                registration_ip,
                registration_region,
                None,
                false,
            )
            .await
    }

    /// Admin: add/undelete an API key, then bind it to the most relevant forward proxy node
    /// based on registration IP/region before persisting the affinity.
    pub async fn add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity(
        &self,
        api_key: &str,
        group: Option<&str>,
        registration_ip: Option<&str>,
        registration_region: Option<&str>,
        geo_origin: &str,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        self.add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            api_key,
            group,
            registration_ip,
            registration_region,
            geo_origin,
            None,
        )
        .await
    }

    /// Admin: add/undelete an API key and persist the caller-selected proxy node when provided.
    pub async fn add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
        &self,
        api_key: &str,
        group: Option<&str>,
        registration_ip: Option<&str>,
        registration_region: Option<&str>,
        geo_origin: &str,
        preferred_primary_proxy_key: Option<&str>,
    ) -> Result<(String, ApiKeyUpsertStatus), ProxyError> {
        let has_fresh_registration_metadata =
            registration_ip.is_some() || registration_region.is_some();
        let is_hint_only_affinity =
            !has_fresh_registration_metadata && preferred_primary_proxy_key.is_some();
        let proxy_affinity = if has_fresh_registration_metadata {
            Some(
                self.select_proxy_affinity_for_registration_with_hint(
                    api_key,
                    geo_origin,
                    registration_ip,
                    registration_region,
                    preferred_primary_proxy_key,
                )
                .await?,
            )
        } else if let Some(preferred_primary_proxy_key) = preferred_primary_proxy_key {
            Some(
                self.select_proxy_affinity_for_hint_only(
                    api_key,
                    geo_origin,
                    preferred_primary_proxy_key,
                )
                .await?,
            )
        } else {
            None
        };
        let result = self
            .key_store
            .add_or_undelete_key_with_status_in_group_and_registration(
                api_key,
                group,
                registration_ip,
                registration_region,
                proxy_affinity.as_ref(),
                is_hint_only_affinity,
            )
            .await?;
        self.remove_proxy_affinity_record_from_cache(&result.0)
            .await;
        Ok(result)
    }

    /// Admin: soft delete a key by ID.
    pub async fn soft_delete_key_by_id(&self, key_id: &str) -> Result<(), ProxyError> {
        self.key_store.soft_delete_key_by_id(key_id).await
    }

    /// Admin: disable a key by ID.
    pub async fn disable_key_by_id(&self, key_id: &str) -> Result<(), ProxyError> {
        self.key_store.disable_key_by_id(key_id).await
    }

    /// Admin: enable a key by ID (from disabled/exhausted -> active).
    pub async fn enable_key_by_id(&self, key_id: &str) -> Result<(), ProxyError> {
        self.key_store.enable_key_by_id(key_id).await
    }

    /// Admin: clear the active quarantine record for a key.
    pub async fn clear_key_quarantine_by_id(&self, key_id: &str) -> Result<(), ProxyError> {
        self.clear_key_quarantine_by_id_with_actor(key_id, MaintenanceActor::default())
            .await
    }

    /// Admin: clear the active quarantine record for a key and append an audit record when changed.
    pub async fn clear_key_quarantine_by_id_with_actor(
        &self,
        key_id: &str,
        actor: MaintenanceActor,
    ) -> Result<(), ProxyError> {
        let before = self.key_store.fetch_key_state_snapshot(key_id).await?;
        let changed = self.key_store.clear_key_quarantine_by_id(key_id).await?;
        if changed {
            let after = self.key_store.fetch_key_state_snapshot(key_id).await?;
            self.key_store
                .insert_api_key_maintenance_record(ApiKeyMaintenanceRecord {
                    id: nanoid!(12),
                    key_id: key_id.to_string(),
                    source: MAINTENANCE_SOURCE_ADMIN.to_string(),
                    operation_code: MAINTENANCE_OP_MANUAL_CLEAR_QUARANTINE.to_string(),
                    operation_summary: "管理员手动解除隔离".to_string(),
                    reason_code: None,
                    reason_summary: Some("管理员解除当前 quarantine".to_string()),
                    reason_detail: None,
                    request_log_id: None,
                    auth_token_log_id: None,
                    auth_token_id: actor.auth_token_id,
                    actor_user_id: actor.actor_user_id,
                    actor_display_name: actor.actor_display_name,
                    status_before: before.status,
                    status_after: after.status,
                    quarantine_before: before.quarantined,
                    quarantine_after: after.quarantined,
                    created_at: Utc::now().timestamp(),
                })
                .await?;
        }
        Ok(())
    }

    /// 获取整体运行情况汇总。
    pub async fn summary(&self) -> Result<ProxySummary, ProxyError> {
        self.key_store.fetch_summary().await
    }

    /// Admin dashboard period summary windows based on server-local day/month boundaries.
    pub async fn summary_windows(&self) -> Result<SummaryWindows, ProxyError> {
        self.summary_windows_at(Local::now()).await
    }

    pub(crate) async fn summary_windows_at(
        &self,
        now: chrono::DateTime<Local>,
    ) -> Result<SummaryWindows, ProxyError> {
        let today_start = start_of_local_day_utc_ts(now);
        let yesterday_start = previous_local_day_start_utc_ts(now);
        let month_start = start_of_local_month_utc_ts(now);
        let today_end = now.with_timezone(&Utc).timestamp().saturating_add(1);
        let yesterday_same_time_end = previous_local_same_time_utc_ts(now).saturating_add(1);

        self.key_store
            .fetch_summary_windows(
                today_start,
                today_end,
                yesterday_start,
                yesterday_same_time_end,
                month_start,
            )
            .await
    }

    /// Public metrics: successful requests today and this month.
    pub async fn success_breakdown(&self) -> Result<SuccessBreakdown, ProxyError> {
        let now = Local::now();
        let month_start = start_of_local_month_utc_ts(now);
        let day_start = start_of_local_day_utc_ts(now);
        self.key_store
            .fetch_success_breakdown(month_start, day_start)
            .await
    }

    /// Token-scoped success/failure breakdown.
    pub async fn token_success_breakdown(
        &self,
        token_id: &str,
    ) -> Result<(i64, i64, i64), ProxyError> {
        let now = Utc::now();
        let month_start = start_of_month(now).timestamp();
        let day_start = start_of_day(now).timestamp();
        self.key_store
            .fetch_token_success_failure(token_id, month_start, day_start)
            .await
    }

    pub(crate) fn sanitize_headers(&self, headers: &HeaderMap, path: &str) -> SanitizedHeaders {
        if path.starts_with("/mcp") {
            sanitize_mcp_headers_inner(headers)
        } else {
            sanitize_headers_inner(headers, &self.upstream, &self.upstream_origin)
        }
    }

    pub async fn find_user_id_by_token(
        &self,
        token_id: &str,
    ) -> Result<Option<String>, ProxyError> {
        self.key_store.find_user_id_by_token(token_id).await
    }

    pub async fn get_active_mcp_session(
        &self,
        proxy_session_id: &str,
    ) -> Result<Option<McpSessionBinding>, ProxyError> {
        self.key_store
            .get_active_mcp_session(proxy_session_id, Utc::now().timestamp())
            .await
    }

    pub async fn create_mcp_session(
        &self,
        upstream_session_id: &str,
        upstream_key_id: &str,
        auth_token_id: Option<&str>,
        user_id: Option<&str>,
        protocol_version: Option<&str>,
        last_event_id: Option<&str>,
    ) -> Result<String, ProxyError> {
        let now = Utc::now().timestamp();
        let proxy_session_id = nanoid!(24);
        self.key_store
            .create_or_replace_mcp_session(&McpSessionBinding {
                proxy_session_id: proxy_session_id.clone(),
                upstream_session_id: upstream_session_id.to_string(),
                upstream_key_id: upstream_key_id.to_string(),
                auth_token_id: auth_token_id.map(str::to_string),
                user_id: user_id.map(str::to_string),
                protocol_version: protocol_version.map(str::to_string),
                last_event_id: last_event_id.map(str::to_string),
                created_at: now,
                updated_at: now,
                expires_at: now + MCP_SESSION_IDLE_TTL_SECS,
                revoked_at: None,
                revoke_reason: None,
            })
            .await?;
        Ok(proxy_session_id)
    }

    pub async fn touch_mcp_session(
        &self,
        proxy_session_id: &str,
        protocol_version: Option<&str>,
        last_event_id: Option<&str>,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        self.key_store
            .touch_mcp_session(
                proxy_session_id,
                protocol_version,
                last_event_id,
                now,
                now + MCP_SESSION_IDLE_TTL_SECS,
            )
            .await
    }

    pub async fn update_mcp_session_upstream_identity(
        &self,
        proxy_session_id: &str,
        upstream_session_id: &str,
        protocol_version: Option<&str>,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        self.key_store
            .update_mcp_session_upstream_identity(
                proxy_session_id,
                upstream_session_id,
                protocol_version,
                now,
                now + MCP_SESSION_IDLE_TTL_SECS,
            )
            .await
    }

    pub async fn revoke_mcp_session(
        &self,
        proxy_session_id: &str,
        reason: &str,
    ) -> Result<(), ProxyError> {
        self.key_store
            .revoke_mcp_session(proxy_session_id, reason)
            .await
    }
}

impl TokenQuota {
    pub(crate) fn new(store: Arc<KeyStore>) -> Self {
        Self {
            store,
            cleanup: Arc::new(Mutex::new(CleanupState::default())),
            hourly_limit: effective_token_hourly_limit(),
            daily_limit: effective_token_daily_limit(),
            monthly_limit: effective_token_monthly_limit(),
        }
    }

    pub(crate) async fn resolve_subject(&self, token_id: &str) -> Result<QuotaSubject, ProxyError> {
        if let Some(user_id) = self.store.find_user_id_by_token_fresh(token_id).await? {
            Ok(QuotaSubject::Account(user_id))
        } else {
            Ok(QuotaSubject::Token(token_id.to_string()))
        }
    }

    pub(crate) async fn check(&self, token_id: &str) -> Result<TokenQuotaVerdict, ProxyError> {
        let now = Utc::now();
        let now_ts = now.timestamp();
        let minute_bucket = now_ts - (now_ts % SECS_PER_MINUTE);
        let hour_bucket = now_ts - (now_ts % SECS_PER_HOUR);

        let hour_window_start = minute_bucket - 59 * SECS_PER_MINUTE;
        let day_window_start = hour_bucket - 23 * SECS_PER_HOUR;
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
                        .store
                        .sum_account_usage_buckets(&user_id, GRANULARITY_HOUR, day_window_start)
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
                        .increment_account_usage_bucket(&user_id, hour_bucket, GRANULARITY_HOUR)
                        .await?;
                    let hourly_used = self
                        .store
                        .sum_account_usage_buckets(&user_id, GRANULARITY_MINUTE, hour_window_start)
                        .await?;
                    let daily_used = self
                        .store
                        .sum_account_usage_buckets(&user_id, GRANULARITY_HOUR, day_window_start)
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
                    .increment_usage_bucket(&token_id, hour_bucket, GRANULARITY_HOUR)
                    .await?;

                let hourly_used = self
                    .store
                    .sum_usage_buckets(&token_id, GRANULARITY_MINUTE, hour_window_start)
                    .await?;
                let daily_used = self
                    .store
                    .sum_usage_buckets(&token_id, GRANULARITY_HOUR, day_window_start)
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

        let now = Utc::now();
        let now_ts = now.timestamp();
        let minute_bucket = now_ts - (now_ts % SECS_PER_MINUTE);
        let hour_bucket = now_ts - (now_ts % SECS_PER_HOUR);
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
                        hour_bucket,
                        GRANULARITY_HOUR,
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
                    .increment_usage_bucket_by(&token_id, hour_bucket, GRANULARITY_HOUR, credits)
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
        let hour_bucket = now_ts - (now_ts % SECS_PER_HOUR);
        let hour_window_start = minute_bucket - 59 * SECS_PER_MINUTE;
        let day_window_start = hour_bucket - 23 * SECS_PER_HOUR;
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
                    .store
                    .sum_account_usage_buckets(user_id, GRANULARITY_HOUR, day_window_start)
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
                    .store
                    .sum_usage_buckets(token_id, GRANULARITY_HOUR, day_window_start)
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
        let now = Utc::now();
        let now_ts = now.timestamp();
        let minute_bucket = now_ts - (now_ts % SECS_PER_MINUTE);
        let hour_bucket = now_ts - (now_ts % SECS_PER_HOUR);
        let hour_window_start = minute_bucket - 59 * SECS_PER_MINUTE;
        let day_window_start = hour_bucket - 23 * SECS_PER_HOUR;
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
            .sum_usage_buckets_bulk(&token_subjects, GRANULARITY_HOUR, day_window_start)
            .await?;
        let token_monthly_totals = self
            .store
            .fetch_monthly_counts(&token_subjects, month_start)
            .await?;

        let mut verdicts = HashMap::new();
        for token_id in token_subjects {
            let hourly_used = token_hourly_totals.get(&token_id).copied().unwrap_or(0);
            let daily_used = token_daily_totals.get(&token_id).copied().unwrap_or(0);
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
                    GRANULARITY_HOUR,
                    day_window_start,
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
                let daily_used = account_daily_totals.get(&user_id).copied().unwrap_or(0);
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
                .delete_old_account_usage_buckets(GRANULARITY_MINUTE, threshold)
                .await?;
            self.store
                .delete_old_account_usage_buckets(GRANULARITY_HOUR, threshold)
                .await?;
        }

        Ok(())
    }
}

impl TokenRequestLimit {
    pub(crate) fn new(store: Arc<KeyStore>) -> Self {
        Self {
            store,
            cleanup: Arc::new(Mutex::new(CleanupState::default())),
            hourly_limit: effective_token_hourly_request_limit(),
        }
    }

    pub(crate) async fn check(
        &self,
        token_id: &str,
    ) -> Result<TokenHourlyRequestVerdict, ProxyError> {
        let now_ts = Utc::now().timestamp();
        let minute_bucket = now_ts - (now_ts % SECS_PER_MINUTE);
        let hour_window_start = minute_bucket - 59 * SECS_PER_MINUTE;
        let verdict =
            if let Some(user_id) = self.store.find_user_id_by_token_fresh(token_id).await? {
                let limits = self
                    .store
                    .resolve_account_quota_resolution(&user_id)
                    .await?
                    .effective;
                if limits.hourly_any_limit <= 0 {
                    let hourly_used = self
                        .store
                        .sum_account_usage_buckets(
                            &user_id,
                            GRANULARITY_REQUEST_MINUTE,
                            hour_window_start,
                        )
                        .await?;
                    TokenHourlyRequestVerdict::new(hourly_used, limits.hourly_any_limit)
                } else {
                    self.store
                        .increment_account_usage_bucket(
                            &user_id,
                            minute_bucket,
                            GRANULARITY_REQUEST_MINUTE,
                        )
                        .await?;
                    let hourly_used = self
                        .store
                        .sum_account_usage_buckets(
                            &user_id,
                            GRANULARITY_REQUEST_MINUTE,
                            hour_window_start,
                        )
                        .await?;
                    TokenHourlyRequestVerdict::new(hourly_used, limits.hourly_any_limit)
                }
            } else {
                // Increment per-minute raw request bucket for this token.
                self.store
                    .increment_usage_bucket(token_id, minute_bucket, GRANULARITY_REQUEST_MINUTE)
                    .await?;

                let hourly_used = self
                    .store
                    .sum_usage_buckets(token_id, GRANULARITY_REQUEST_MINUTE, hour_window_start)
                    .await?;
                TokenHourlyRequestVerdict::new(hourly_used, self.hourly_limit)
            };

        self.maybe_cleanup(now_ts).await?;
        Ok(verdict)
    }

    /// Read-only snapshot of hourly raw request usage for a set of tokens.
    /// This does NOT increment counters and is intended for dashboards / leaderboards.
    pub(crate) async fn snapshot_many(
        &self,
        token_ids: &[String],
    ) -> Result<HashMap<String, TokenHourlyRequestVerdict>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let now_ts = Utc::now().timestamp();
        let minute_bucket = now_ts - (now_ts % SECS_PER_MINUTE);
        let hour_window_start = minute_bucket - 59 * SECS_PER_MINUTE;

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

        let mut map = HashMap::new();
        let token_totals = self
            .store
            .sum_usage_buckets_bulk(
                &token_subjects,
                GRANULARITY_REQUEST_MINUTE,
                hour_window_start,
            )
            .await?;
        for token_id in token_subjects {
            let used = token_totals.get(&token_id).copied().unwrap_or(0);
            map.insert(
                token_id,
                TokenHourlyRequestVerdict::new(used, self.hourly_limit),
            );
        }

        if !account_user_ids.is_empty() {
            let account_limits = self
                .store
                .resolve_account_quota_limits_bulk(&account_user_ids)
                .await?;
            let account_totals = self
                .store
                .sum_account_usage_buckets_bulk(
                    &account_user_ids,
                    GRANULARITY_REQUEST_MINUTE,
                    hour_window_start,
                )
                .await?;
            let default_hourly_any_limit = AccountQuotaLimits::zero_base().hourly_any_limit;
            for (token_id, user_id) in account_subjects {
                let used = account_totals.get(&user_id).copied().unwrap_or(0);
                let limit = account_limits
                    .get(&user_id)
                    .map(|limits| limits.hourly_any_limit)
                    .unwrap_or(default_hourly_any_limit);
                map.insert(token_id, TokenHourlyRequestVerdict::new(used, limit));
            }
        }
        Ok(map)
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
                .delete_old_usage_buckets(GRANULARITY_REQUEST_MINUTE, threshold)
                .await?;
            self.store
                .delete_old_account_usage_buckets(GRANULARITY_REQUEST_MINUTE, threshold)
                .await?;
        }

        Ok(())
    }
}

impl TavilyProxy {
    /// List keys whose quota hasn't been synced within `older_than_secs` seconds (or never).
    pub async fn list_keys_pending_quota_sync(
        &self,
        older_than_secs: i64,
    ) -> Result<Vec<String>, ProxyError> {
        self.key_store
            .list_keys_pending_quota_sync(older_than_secs)
            .await
    }

    pub async fn list_keys_pending_hot_quota_sync(
        &self,
        active_within_secs: i64,
        stale_after_secs: i64,
    ) -> Result<Vec<String>, ProxyError> {
        self.key_store
            .list_keys_pending_hot_quota_sync(active_within_secs, stale_after_secs)
            .await
    }

    /// Sync usage/quota for specific key via Tavily Usage API base (e.g., https://api.tavily.com).
    pub async fn sync_key_quota(
        &self,
        key_id: &str,
        usage_base: &str,
        source: &str,
    ) -> Result<(i64, i64), ProxyError> {
        let Some(secret) = self.key_store.fetch_api_key_secret(key_id).await? else {
            return Err(ProxyError::Database(sqlx::Error::RowNotFound));
        };
        let (limit, remaining) = match self
            .fetch_usage_quota_for_secret(
                &secret,
                usage_base,
                None,
                Some(key_id),
                None,
                "quota_sync",
            )
            .await
        {
            Ok(quota) => quota,
            Err(err) => {
                self.maybe_quarantine_usage_error(key_id, "/api/tavily/usage", &err)
                    .await?;
                return Err(err);
            }
        };
        let now = Utc::now().timestamp();
        self.key_store
            .record_quota_sync_sample(key_id, limit, remaining, now, source)
            .await?;
        Ok((limit, remaining))
    }

    /// Probe usage/quota for an API key secret via Tavily Usage API base (e.g., https://api.tavily.com).
    /// This performs *no* database mutation and is safe to use for admin validation flows.
    pub async fn probe_api_key_quota(
        &self,
        api_key: &str,
        usage_base: &str,
    ) -> Result<(i64, i64), ProxyError> {
        self.fetch_usage_quota_for_secret(
            api_key,
            usage_base,
            Some(Duration::from_secs(USAGE_PROBE_TIMEOUT_SECS)),
            None,
            None,
            "quota_probe",
        )
        .await
    }

    pub async fn probe_api_key_quota_with_registration(
        &self,
        api_key: &str,
        usage_base: &str,
        registration_ip: Option<&str>,
        registration_region: Option<&str>,
        geo_origin: &str,
    ) -> Result<(i64, i64, Option<ForwardProxyAssignmentPreview>), ProxyError> {
        let (proxy_affinity, assigned_proxy) =
            if registration_ip.is_some() || registration_region.is_some() {
                let (record, preview) = self
                    .select_proxy_affinity_preview_for_registration_with_hint(
                        &format!("validate:{api_key}"),
                        geo_origin,
                        registration_ip,
                        registration_region,
                        None,
                    )
                    .await?;
                (Some(record), preview)
            } else {
                (None, None)
            };
        let (limit, remaining) = self
            .fetch_usage_quota_for_secret(
                api_key,
                usage_base,
                Some(Duration::from_secs(USAGE_PROBE_TIMEOUT_SECS)),
                None,
                proxy_affinity.as_ref().map(|record| (api_key, record)),
                "quota_probe",
            )
            .await?;
        Ok((limit, remaining, assigned_proxy))
    }

    /// Admin: mark a key as quota-exhausted by its secret string.
    pub async fn mark_key_quota_exhausted_by_secret(
        &self,
        api_key: &str,
    ) -> Result<bool, ProxyError> {
        self.mark_key_quota_exhausted_by_secret_with_actor(api_key, MaintenanceActor::default())
            .await
    }

    pub async fn mark_key_quota_exhausted_by_secret_with_actor(
        &self,
        api_key: &str,
        actor: MaintenanceActor,
    ) -> Result<bool, ProxyError> {
        let Some(key_id) = self.key_store.fetch_api_key_id_by_secret(api_key).await? else {
            return Ok(false);
        };
        let before = self.key_store.fetch_key_state_snapshot(&key_id).await?;
        let changed = self.key_store.mark_quota_exhausted(api_key).await?;
        if changed {
            let created_at = Utc::now().timestamp();
            let after = self.key_store.fetch_key_state_snapshot(&key_id).await?;
            self.key_store
                .insert_api_key_maintenance_record(ApiKeyMaintenanceRecord {
                    id: nanoid!(12),
                    key_id: key_id.clone(),
                    source: MAINTENANCE_SOURCE_ADMIN.to_string(),
                    operation_code: MAINTENANCE_OP_MANUAL_MARK_EXHAUSTED.to_string(),
                    operation_summary: "管理员手动标记 exhausted".to_string(),
                    reason_code: Some("manual_mark_exhausted".to_string()),
                    reason_summary: Some("确认该 Key 额度耗尽".to_string()),
                    reason_detail: None,
                    request_log_id: None,
                    auth_token_log_id: None,
                    auth_token_id: actor.auth_token_id.clone(),
                    actor_user_id: actor.actor_user_id.clone(),
                    actor_display_name: actor.actor_display_name.clone(),
                    status_before: before.status,
                    status_after: after.status,
                    quarantine_before: before.quarantined,
                    quarantine_after: after.quarantined,
                    created_at,
                })
                .await?;
            self.key_store
                .record_manual_key_breakage_fanout(
                    &key_id,
                    STATUS_EXHAUSTED,
                    Some("manual_mark_exhausted"),
                    Some("确认该 Key 额度耗尽"),
                    &actor,
                    created_at,
                )
                .await?;
        }
        Ok(changed)
    }

    pub(crate) async fn fetch_usage_quota_for_secret(
        &self,
        secret: &str,
        usage_base: &str,
        timeout: Option<Duration>,
        api_key_id: Option<&str>,
        proxy_affinity: Option<(&str, &forward_proxy::ForwardProxyAffinityRecord)>,
        request_kind: &str,
    ) -> Result<(i64, i64), ProxyError> {
        let base = Url::parse(usage_base).map_err(|e| ProxyError::InvalidEndpoint {
            endpoint: usage_base.to_string(),
            source: e,
        })?;
        let mut url = base.clone();
        url.set_path("/usage");

        let secret_header = secret.to_string();
        let request_url = url.clone();
        let resp = match (api_key_id, proxy_affinity) {
            (Some(api_key_id), _) => self
                .send_with_forward_proxy(api_key_id, request_kind, |client| {
                    let mut req = client
                        .get(request_url.clone())
                        .header("Authorization", format!("Bearer {}", secret_header));
                    if let Some(timeout) = timeout {
                        req = req.timeout(timeout);
                    }
                    req
                })
                .await
                .map(|(response, _)| response)?,
            (None, Some((subject, proxy_affinity))) => self
                .send_with_forward_proxy_affinity(subject, request_kind, proxy_affinity, |client| {
                    let mut req = client
                        .get(request_url.clone())
                        .header("Authorization", format!("Bearer {}", secret_header));
                    if let Some(timeout) = timeout {
                        req = req.timeout(timeout);
                    }
                    req
                })
                .await
                .map(|(response, _)| response)?,
            (None, None) => {
                let mut req = self
                    .client
                    .get(request_url.clone())
                    .header("Authorization", format!("Bearer {}", secret_header));
                if let Some(timeout) = timeout {
                    req = req.timeout(timeout);
                }
                req.send().await.map_err(ProxyError::Http)?
            }
        };
        let status = resp.status();
        let bytes = resp.bytes().await.map_err(ProxyError::Http)?;
        if !status.is_success() {
            let body = String::from_utf8_lossy(&bytes).into_owned();
            return Err(ProxyError::UsageHttp { status, body });
        }
        let json: Value = serde_json::from_slice(&bytes)
            .map_err(|e| ProxyError::Other(format!("invalid usage json: {}", e)))?;
        let key_limit = json
            .get("key")
            .and_then(|k| k.get("limit"))
            .and_then(|v| v.as_i64());
        let key_usage = json
            .get("key")
            .and_then(|k| k.get("usage"))
            .and_then(|v| v.as_i64());
        let acc_limit = json
            .get("account")
            .and_then(|a| a.get("plan_limit"))
            .and_then(|v| v.as_i64());
        let acc_usage = json
            .get("account")
            .and_then(|a| a.get("plan_usage"))
            .and_then(|v| v.as_i64());
        let limit = key_limit.or(acc_limit).unwrap_or(0);
        let used = key_usage.or(acc_usage).unwrap_or(0);
        if limit <= 0 && used <= 0 {
            return Err(ProxyError::QuotaDataMissing {
                reason: "missing key/account usage fields".to_owned(),
            });
        }
        let remaining = (limit - used).max(0);
        Ok((limit, remaining))
    }

    pub(crate) async fn fetch_research_usage_for_secret(
        &self,
        secret: &str,
        usage_base: &str,
        timeout: Option<Duration>,
        api_key_id: Option<&str>,
        request_kind: &str,
    ) -> Result<i64, ProxyError> {
        let base = Url::parse(usage_base).map_err(|e| ProxyError::InvalidEndpoint {
            endpoint: usage_base.to_string(),
            source: e,
        })?;
        let mut url = base.clone();
        url.set_path("/usage");

        let secret_header = secret.to_string();
        let request_url = url.clone();
        let resp = match api_key_id {
            Some(api_key_id) => self
                .send_with_forward_proxy(api_key_id, request_kind, |client| {
                    let mut req = client
                        .get(request_url.clone())
                        .header("Authorization", format!("Bearer {}", secret_header));
                    if let Some(timeout) = timeout {
                        req = req.timeout(timeout);
                    }
                    req
                })
                .await
                .map(|(response, _)| response)?,
            None => {
                let mut req = self
                    .client
                    .get(request_url.clone())
                    .header("Authorization", format!("Bearer {}", secret_header));
                if let Some(timeout) = timeout {
                    req = req.timeout(timeout);
                }
                req.send().await.map_err(ProxyError::Http)?
            }
        };
        let status = resp.status();
        let bytes = resp.bytes().await.map_err(ProxyError::Http)?;
        if !status.is_success() {
            let body = String::from_utf8_lossy(&bytes).into_owned();
            return Err(ProxyError::UsageHttp { status, body });
        }

        let json: Value = serde_json::from_slice(&bytes)
            .map_err(|e| ProxyError::Other(format!("invalid usage json: {}", e)))?;
        let usage = json
            .get("key")
            .and_then(|k| k.get("research_usage"))
            .and_then(parse_credits_value);
        usage.ok_or_else(|| ProxyError::QuotaDataMissing {
            reason: "missing key.research_usage field".to_owned(),
        })
    }

    pub(crate) async fn fetch_research_usage_for_secret_with_retries(
        &self,
        secret: &str,
        usage_base: &str,
        api_key_id: Option<&str>,
        request_kind: &str,
    ) -> Result<i64, ProxyError> {
        let mut last_error: Option<ProxyError> = None;
        for attempt in 0..USAGE_PROBE_RETRY_ATTEMPTS {
            match self
                .fetch_research_usage_for_secret(
                    secret,
                    usage_base,
                    Some(Duration::from_secs(USAGE_PROBE_TIMEOUT_SECS)),
                    api_key_id,
                    request_kind,
                )
                .await
            {
                Ok(usage) => return Ok(usage),
                Err(err) => last_error = Some(err),
            }

            if attempt + 1 < USAGE_PROBE_RETRY_ATTEMPTS {
                tokio::time::sleep(Duration::from_millis(USAGE_PROBE_RETRY_DELAY_MS)).await;
            }
        }

        Err(last_error.unwrap_or_else(|| {
            ProxyError::Other("research usage probe failed without error".to_owned())
        }))
    }

    /// Aggregate per-token usage logs into token_usage_stats for UI metrics.
    /// Used by background schedulers to keep usage charts up to date.
    pub async fn rollup_token_usage_stats(&self) -> Result<(i64, Option<i64>), ProxyError> {
        let mut retry_idx = 0usize;
        loop {
            match self.key_store.rollup_token_usage_stats().await {
                Ok(result) => return Ok(result),
                Err(err)
                    if is_transient_sqlite_write_error(&err)
                        && retry_idx < TOKEN_USAGE_ROLLUP_TRANSIENT_RETRY_BACKOFF_MS.len() =>
                {
                    let backoff_ms = TOKEN_USAGE_ROLLUP_TRANSIENT_RETRY_BACKOFF_MS[retry_idx];
                    retry_idx += 1;
                    eprintln!(
                        "token usage rollup transient sqlite error (attempt={}, backoff={}ms): {}",
                        retry_idx, backoff_ms, err
                    );
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                }
                Err(err) => return Err(err),
            }
        }
    }

    pub async fn rebuild_token_usage_stats_for_tokens(
        &self,
        token_ids: &[String],
    ) -> Result<i64, ProxyError> {
        let mut retry_idx = 0usize;
        loop {
            match self
                .key_store
                .rebuild_token_usage_stats_for_tokens(token_ids)
                .await
            {
                Ok(result) => return Ok(result),
                Err(err)
                    if is_transient_sqlite_write_error(&err)
                        && retry_idx < TOKEN_USAGE_ROLLUP_TRANSIENT_RETRY_BACKOFF_MS.len() =>
                {
                    let backoff_ms = TOKEN_USAGE_ROLLUP_TRANSIENT_RETRY_BACKOFF_MS[retry_idx];
                    retry_idx += 1;
                    eprintln!(
                        "token usage rebuild transient sqlite error (attempt={}, backoff={}ms): {}",
                        retry_idx, backoff_ms, err
                    );
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                }
                Err(err) => return Err(err),
            }
        }
    }

    /// Time-based garbage collection for per-token access logs.
    /// This uses a fixed retention window and never looks at token status,
    /// to avoid impacting auditability.
    pub async fn gc_auth_token_logs(&self) -> Result<i64, ProxyError> {
        let now_ts = Utc::now().timestamp();
        let threshold = now_ts - AUTH_TOKEN_LOG_RETENTION_SECS;
        self.key_store.delete_old_auth_token_logs(threshold).await
    }

    /// Time-based garbage collection for request_logs (online recent logs only).
    /// Retention is defined by local-day boundaries and enforced via environment variables.
    pub async fn gc_request_logs(&self) -> Result<i64, ProxyError> {
        let retention_days = effective_request_logs_retention_days();
        let threshold = request_logs_retention_threshold_utc_ts(retention_days);
        self.key_store.delete_old_request_logs(threshold).await
    }

    /// Job logging helpers
    pub async fn scheduled_job_start(
        &self,
        job_type: &str,
        key_id: Option<&str>,
        attempt: i64,
    ) -> Result<i64, ProxyError> {
        self.key_store
            .scheduled_job_start(job_type, key_id, attempt)
            .await
    }

    pub async fn scheduled_job_finish(
        &self,
        job_id: i64,
        status: &str,
        message: Option<&str>,
    ) -> Result<(), ProxyError> {
        self.key_store
            .scheduled_job_finish(job_id, status, message)
            .await
    }

    pub async fn list_recent_jobs(&self, limit: usize) -> Result<Vec<JobLog>, ProxyError> {
        self.key_store.list_recent_jobs(limit).await
    }

    pub async fn list_recent_jobs_paginated(
        &self,
        group: &str,
        page: usize,
        per_page: usize,
    ) -> Result<(Vec<JobLog>, i64), ProxyError> {
        self.key_store
            .list_recent_jobs_paginated(group, page, per_page)
            .await
    }
}
