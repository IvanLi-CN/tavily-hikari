impl SelectedForwardProxy {
    pub fn from_endpoint(endpoint: &ForwardProxyEndpoint) -> Self {
        Self {
            key: endpoint.key.clone(),
            source: endpoint.source.clone(),
            display_name: endpoint.display_name.clone(),
            kind: endpoint.protocol.as_str().to_string(),
            endpoint_url: endpoint.endpoint_url.clone(),
            endpoint_url_raw: endpoint.raw_url.clone(),
            uses_local_relay: endpoint.uses_local_relay,
            relay_handle: endpoint.relay_handle(),
        }
    }

    pub(crate) fn uses_local_relay(&self) -> bool {
        self.uses_local_relay
    }
}

#[derive(Debug)]
pub struct ForwardProxyRelayLease {
    supervisor: Arc<Mutex<XraySupervisor>>,
    relay_handle: Option<Arc<SharedXrayRelayHandle>>,
}

impl ForwardProxyRelayLease {
    pub fn new(supervisor: Arc<Mutex<XraySupervisor>>) -> Self {
        Self {
            supervisor,
            relay_handle: None,
        }
    }

    pub(crate) fn from_acquired_handle(
        supervisor: Arc<Mutex<XraySupervisor>>,
        relay_handle: Arc<SharedXrayRelayHandle>,
    ) -> Self {
        Self {
            supervisor,
            relay_handle: Some(relay_handle),
        }
    }

    pub async fn acquire_for_selection(
        supervisor: Arc<Mutex<XraySupervisor>>,
        candidate: &SelectedForwardProxy,
    ) -> Option<Self> {
        Self::acquire_selection(supervisor, candidate).await
    }

    pub async fn acquire_for_endpoint(
        supervisor: Arc<Mutex<XraySupervisor>>,
        endpoint: &ForwardProxyEndpoint,
    ) -> Option<Self> {
        Self::acquire(
            supervisor,
            endpoint.uses_local_relay,
            endpoint.relay_handle(),
        )
        .await
    }

    async fn acquire_selection(
        supervisor: Arc<Mutex<XraySupervisor>>,
        candidate: &SelectedForwardProxy,
    ) -> Option<Self> {
        Self::acquire(
            supervisor,
            candidate.uses_local_relay,
            candidate.relay_handle.clone(),
        )
        .await
    }

    async fn acquire(
        supervisor: Arc<Mutex<XraySupervisor>>,
        uses_local_relay: bool,
        relay_handle: Option<Arc<SharedXrayRelayHandle>>,
    ) -> Option<Self> {
        if !uses_local_relay {
            return Some(Self::new(supervisor));
        }
        let relay_handle = relay_handle?;
        {
            let mut locked = supervisor.lock().await;
            locked.reset_if_shared_process_exited();
        }
        if !relay_handle.try_acquire_lease() {
            return None;
        }
        Some(Self {
            supervisor,
            relay_handle: Some(relay_handle),
        })
    }

    pub async fn release(mut self) {
        if let Some(relay_handle) = self.relay_handle.take() {
            relay_handle.release_lease();
            drop(relay_handle);
            self.supervisor.lock().await.reap_retired_handles().await;
        }
    }
}

impl Drop for ForwardProxyRelayLease {
    fn drop(&mut self) {
        let Some(relay_handle) = self.relay_handle.take() else {
            return;
        };
        let supervisor = Arc::clone(&self.supervisor);
        tokio::spawn(async move {
            relay_handle.release_lease();
            drop(relay_handle);
            supervisor.lock().await.reap_retired_handles().await;
        });
    }
}

#[derive(Debug, Clone)]
pub struct ForwardProxyClientPool {
    direct_client: Client,
    clients: Arc<RwLock<HashMap<String, Client>>>,
    egress_clients: Arc<RwLock<HashMap<String, Client>>>,
}

impl ForwardProxyClientPool {
    pub fn new() -> Result<Self, ProxyError> {
        let direct_client = Client::builder()
            .pool_idle_timeout(Duration::from_secs(90))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(ProxyError::Http)?;
        Ok(Self {
            direct_client,
            clients: Arc::new(RwLock::new(HashMap::new())),
            egress_clients: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub fn direct_client(&self) -> Client {
        self.direct_client.clone()
    }

    pub async fn client_for(&self, endpoint_url: Option<&Url>) -> Result<Client, ProxyError> {
        let Some(endpoint_url) = endpoint_url else {
            return Ok(self.direct_client());
        };
        self.cached_client_for_proxy_url(endpoint_url, &self.clients)
            .await
    }

    pub async fn direct_client_via_egress(
        &self,
        egress_socks5_url: Option<&Url>,
    ) -> Result<Client, ProxyError> {
        let Some(egress_socks5_url) = egress_socks5_url else {
            return Ok(self.direct_client());
        };
        self.cached_client_for_proxy_url(egress_socks5_url, &self.egress_clients)
            .await
    }

    async fn cached_client_for_proxy_url(
        &self,
        proxy_url: &Url,
        cache: &Arc<RwLock<HashMap<String, Client>>>,
    ) -> Result<Client, ProxyError> {
        let key = proxy_url.as_str().to_string();
        if let Some(client) = cache.read().await.get(&key).cloned() {
            return Ok(client);
        }
        let built = Client::builder()
            .pool_idle_timeout(Duration::from_secs(90))
            .redirect(reqwest::redirect::Policy::none())
            .proxy(Proxy::all(proxy_url.as_str()).map_err(|err| {
                ProxyError::Other(format!("invalid forward proxy endpoint {proxy_url}: {err}"))
            })?)
            .build()
            .map_err(ProxyError::Http)?;
        cache.write().await.insert(key, built.clone());
        Ok(built)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ForwardProxyRuntimeConfig {
    pub xray_binary: &'static str,
}

#[derive(Debug, Clone)]
pub struct ForwardProxyAssignmentCounts {
    pub primary: i64,
    pub secondary: i64,
}

#[derive(Debug, Clone)]
struct ForwardProxyKeyAffinity {
    primary_proxy_key: Option<String>,
    secondary_proxy_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ForwardProxyAttemptWindowStats {
    pub attempts: i64,
    pub success_count: i64,
    pub avg_latency_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxyWindowStatsResponse {
    pub attempts: i64,
    pub success_rate: Option<f64>,
    pub avg_latency_ms: Option<f64>,
}

impl From<ForwardProxyAttemptWindowStats> for ForwardProxyWindowStatsResponse {
    fn from(value: ForwardProxyAttemptWindowStats) -> Self {
        let success_rate = if value.attempts > 0 {
            Some((value.success_count as f64) / (value.attempts as f64))
        } else {
            None
        };
        Self {
            attempts: value.attempts,
            success_rate,
            avg_latency_ms: value.avg_latency_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxyStatsResponse {
    pub one_minute: ForwardProxyWindowStatsResponse,
    pub fifteen_minutes: ForwardProxyWindowStatsResponse,
    pub one_hour: ForwardProxyWindowStatsResponse,
    pub one_day: ForwardProxyWindowStatsResponse,
    pub seven_days: ForwardProxyWindowStatsResponse,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxyNodeResponse {
    pub key: String,
    pub source: String,
    pub display_name: String,
    pub endpoint_url: Option<String>,
    pub resolved_ips: Vec<String>,
    pub resolved_regions: Vec<String>,
    pub weight: f64,
    pub available: bool,
    pub last_error: Option<String>,
    pub penalized: bool,
    pub primary_assignment_count: i64,
    pub secondary_assignment_count: i64,
    pub stats: ForwardProxyStatsResponse,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxySettingsResponse {
    pub proxy_urls: Vec<String>,
    pub subscription_urls: Vec<String>,
    pub subscription_update_interval_secs: u64,
    pub insert_direct: bool,
    pub egress_socks5_enabled: bool,
    pub egress_socks5_url: String,
    pub nodes: Vec<ForwardProxyNodeResponse>,
}

#[derive(Debug, Clone, Default)]
struct ForwardProxyHourlyStatsPoint {
    success_count: i64,
    failure_count: i64,
}

#[derive(Debug, Clone)]
struct ForwardProxyWeightHourlyStatsPoint {
    sample_count: i64,
    min_weight: f64,
    max_weight: f64,
    avg_weight: f64,
    last_weight: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxyHourlyBucketResponse {
    pub bucket_start: String,
    pub bucket_end: String,
    pub success_count: i64,
    pub failure_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxyWeightHourlyBucketResponse {
    pub bucket_start: String,
    pub bucket_end: String,
    pub sample_count: i64,
    pub min_weight: f64,
    pub max_weight: f64,
    pub avg_weight: f64,
    pub last_weight: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxyLiveNodeResponse {
    pub key: String,
    pub source: String,
    pub display_name: String,
    pub endpoint_url: Option<String>,
    pub resolved_ips: Vec<String>,
    pub resolved_regions: Vec<String>,
    pub weight: f64,
    pub available: bool,
    pub last_error: Option<String>,
    pub penalized: bool,
    pub primary_assignment_count: i64,
    pub secondary_assignment_count: i64,
    pub stats: ForwardProxyStatsResponse,
    pub last24h: Vec<ForwardProxyHourlyBucketResponse>,
    pub weight24h: Vec<ForwardProxyWeightHourlyBucketResponse>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxyLiveStatsResponse {
    pub range_start: String,
    pub range_end: String,
    pub bucket_seconds: i64,
    pub nodes: Vec<ForwardProxyLiveNodeResponse>,
}

#[derive(Debug)]
struct SharedXrayProcess {
    api_server: String,
    api_port: u16,
    config_path: PathBuf,
    child: Child,
}

#[derive(Debug)]
struct ReservedLocalPort {
    port: u16,
    listener: Option<std::net::TcpListener>,
}

impl ReservedLocalPort {
    fn bind() -> Result<Self, ProxyError> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|err| {
            ProxyError::Other(format!(
                "failed to bind local socket for port allocation: {err}"
            ))
        })?;
        let port = listener
            .local_addr()
            .map_err(|err| {
                ProxyError::Other(format!(
                    "failed to read local address for allocated port: {err}"
                ))
            })?
            .port();
        Ok(Self {
            port,
            listener: Some(listener),
        })
    }

    fn port(&self) -> u16 {
        self.port
    }

    fn release(&mut self) {
        self.listener.take();
    }
}

#[derive(Debug)]
pub(crate) struct SharedXrayRelayHandle {
    relay_id: String,
    endpoint_key: Option<String>,
    route_key: String,
    local_proxy_url: Url,
    local_port: u16,
    inbound_tag: String,
    outbound_tags: Vec<String>,
    rule_tag: String,
    config_paths: Vec<PathBuf>,
    lease_count: AtomicUsize,
    invalidated: AtomicBool,
    temporary: bool,
}

impl SharedXrayRelayHandle {
    fn drop_runtime_files(&self) {
        cleanup_paths(&self.config_paths);
    }

    fn try_acquire_lease(&self) -> bool {
        if self.invalidated.load(Ordering::Acquire) {
            return false;
        }
        self.lease_count.fetch_add(1, Ordering::AcqRel);
        if self.invalidated.load(Ordering::Acquire) {
            self.release_lease();
            return false;
        }
        true
    }

    fn release_lease(&self) {
        let _ = self
            .lease_count
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                Some(count.saturating_sub(1))
            });
    }

    fn lease_count(&self) -> usize {
        self.lease_count.load(Ordering::Acquire)
    }

    fn invalidate(&self) {
        self.invalidated.store(true, Ordering::Release);
    }

    fn is_invalidated(&self) -> bool {
        self.invalidated.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone)]
#[cfg(test)]
pub struct XraySupervisorDebugSnapshot {
    pub shared_pid: Option<u32>,
    pub active_endpoint_handles: usize,
    pub total_handles: usize,
    pub retiring_handles: usize,
    pub runtime_files: Vec<String>,
}

#[derive(Debug, Default)]
pub struct XraySupervisor {
    pub binary: String,
    pub runtime_dir: PathBuf,
    shared: Option<SharedXrayProcess>,
    active_endpoint_handles: HashMap<String, String>,
    handles: HashMap<String, Arc<SharedXrayRelayHandle>>,
    retiring_handles: HashSet<String>,
    handle_by_url: HashMap<String, String>,
    handle_nonce: u64,
}

impl XraySupervisor {
    pub fn new(binary: String, runtime_dir: PathBuf) -> Self {
        Self {
            binary,
            runtime_dir,
            shared: None,
            active_endpoint_handles: HashMap::new(),
            handles: HashMap::new(),
            retiring_handles: HashSet::new(),
            handle_by_url: HashMap::new(),
            handle_nonce: 0,
        }
    }

    pub async fn sync_endpoints(
        &mut self,
        endpoints: &mut [ForwardProxyEndpoint],
        egress_socks5_url: Option<&Url>,
    ) -> Result<(), ProxyError> {
        let _ = fs::create_dir_all(&self.runtime_dir);
        self.reset_if_shared_process_exited();

        let mut desired_endpoint_keys = HashSet::new();
        let mut retire_ids = Vec::new();

        for endpoint in endpoints.iter_mut() {
            if !endpoint.needs_local_relay(egress_socks5_url) {
                endpoint.endpoint_url = endpoint_transport_url(endpoint);
                endpoint.uses_local_relay = false;
                endpoint.set_relay_handle(None);
                if let Some(old_id) = self.active_endpoint_handles.remove(&endpoint.key) {
                    retire_ids.push(old_id);
                }
                continue;
            }

            desired_endpoint_keys.insert(endpoint.key.clone());
            let route_key = build_xray_route_key(endpoint, egress_socks5_url);
            let existing_id = self.active_endpoint_handles.get(&endpoint.key).cloned();
            let reusable_id = existing_id.clone().filter(|relay_id| {
                self.handles
                    .get(relay_id)
                    .is_some_and(|handle| handle.route_key == route_key)
            });

            let relay_id = if let Some(relay_id) = reusable_id {
                relay_id
            } else {
                match self
                    .create_relay_handle(
                        Some(endpoint.key.clone()),
                        route_key.clone(),
                        endpoint,
                        egress_socks5_url,
                        false,
                    )
                    .await
                {
                    Ok(relay_id) => {
                        if let Some(previous_id) = self
                            .active_endpoint_handles
                            .insert(endpoint.key.clone(), relay_id.clone())
                            && previous_id != relay_id
                        {
                            retire_ids.push(previous_id);
                        }
                        relay_id
                    }
                    Err(_) => {
                        endpoint.endpoint_url = None;
                        endpoint.uses_local_relay = true;
                        endpoint.set_relay_handle(None);
                        if let Some(previous_id) =
                            self.active_endpoint_handles.remove(&endpoint.key)
                        {
                            retire_ids.push(previous_id);
                        }
                        continue;
                    }
                }
            };

            if let Some(handle) = self.handles.get(&relay_id).cloned() {
                endpoint.endpoint_url = Some(handle.local_proxy_url.clone());
                endpoint.uses_local_relay = true;
                endpoint.set_relay_handle(Some(&handle));
            } else {
                endpoint.endpoint_url = None;
                endpoint.uses_local_relay = true;
                endpoint.set_relay_handle(None);
            }
        }

        let stale_endpoint_keys = self
            .active_endpoint_handles
            .keys()
            .filter(|key| !desired_endpoint_keys.contains(*key))
            .cloned()
            .collect::<Vec<_>>();
        for endpoint_key in stale_endpoint_keys {
            if let Some(relay_id) = self.active_endpoint_handles.remove(&endpoint_key) {
                retire_ids.push(relay_id);
            }
        }

        retire_ids.sort();
        retire_ids.dedup();
        for relay_id in retire_ids {
            self.mark_handle_retiring(&relay_id).await;
        }
        self.reap_retired_handles().await;
        Ok(())
    }

    pub async fn shutdown_all(&mut self) {
        let relay_ids = self.handles.keys().cloned().collect::<Vec<_>>();
        for relay_id in relay_ids {
            self.force_remove_handle(&relay_id).await;
        }
        self.active_endpoint_handles.clear();
        self.retiring_handles.clear();
        self.handle_by_url.clear();
        self.shutdown_shared_process().await;
    }

    pub async fn resolve_validation_endpoint(
        &mut self,
        endpoint: &ForwardProxyEndpoint,
        egress_socks5_url: Option<&Url>,
    ) -> Result<ForwardProxyEndpoint, ProxyError> {
        self.reset_if_shared_process_exited();
        if endpoint.uses_local_relay
            && endpoint
                .relay_handle()
                .is_some_and(|handle| !handle.is_invalidated())
        {
            return Ok(endpoint.clone());
        }
        if !endpoint.needs_local_relay(egress_socks5_url) {
            return Ok(endpoint.clone());
        }

        let route_key = format!(
            "__validate_xray__{:016x}",
            stable_hash_u64(&format!(
                "{}|{}",
                endpoint
                    .raw_url
                    .as_deref()
                    .or_else(|| endpoint.endpoint_url.as_ref().map(Url::as_str))
                    .unwrap_or_default(),
                egress_socks5_url.map(Url::as_str).unwrap_or_default()
            ))
        );
        let relay_id = self
            .create_relay_handle(None, route_key, endpoint, egress_socks5_url, true)
            .await?;
        let Some(handle) = self.handles.get(&relay_id).cloned() else {
            return Err(ProxyError::Other(
                "shared xray relay handle disappeared before validation".to_string(),
            ));
        };
        let mut resolved = endpoint.clone();
        resolved.endpoint_url = Some(handle.local_proxy_url.clone());
        resolved.uses_local_relay = true;
        resolved.set_relay_handle(Some(&handle));
        Ok(resolved)
    }

    pub async fn release_relay_lease(&mut self, relay_id: &str) {
        if let Some(handle) = self.handles.get_mut(relay_id)
            && handle.lease_count() > 0
        {
            handle.release_lease();
        }
        self.reap_retired_handles().await;
    }

    pub async fn reap_retired_handles_now(&mut self) {
        self.reap_retired_handles().await;
    }

    #[cfg(test)]
    pub async fn debug_snapshot(&mut self) -> XraySupervisorDebugSnapshot {
        self.reset_if_shared_process_exited();
        let runtime_files = fs::read_dir(&self.runtime_dir)
            .ok()
            .into_iter()
            .flat_map(|entries| entries.filter_map(Result::ok))
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect::<Vec<_>>();
        XraySupervisorDebugSnapshot {
            shared_pid: self.shared.as_ref().and_then(|shared| shared.child.id()),
            active_endpoint_handles: self.active_endpoint_handles.len(),
            total_handles: self.handles.len(),
            retiring_handles: self.retiring_handles.len(),
            runtime_files,
        }
    }

    pub async fn acquire_relay_lease_by_url(
        &mut self,
        endpoint_url: Option<&Url>,
    ) -> Option<String> {
        self.reset_if_shared_process_exited();
        let endpoint_url = endpoint_url?;
        let relay_id = self.handle_by_url.get(endpoint_url.as_str())?.clone();
        let handle = self.handles.get(&relay_id)?.clone();
        handle.try_acquire_lease().then_some(relay_id)
    }

    fn acquire_relay_lease(&mut self, relay_id: &str) -> Option<String> {
        let handle = self.handles.get(relay_id)?.clone();
        handle.try_acquire_lease().then_some(relay_id.to_string())
    }

    pub(crate) fn acquire_relay_handle_for_endpoint(
        &mut self,
        endpoint: &ForwardProxyEndpoint,
    ) -> Option<Arc<SharedXrayRelayHandle>> {
        self.reset_if_shared_process_exited();
        if !endpoint.uses_local_relay {
            return None;
        }
        let relay_handle = endpoint.relay_handle()?;
        relay_handle.try_acquire_lease().then_some(relay_handle)
    }

    async fn create_relay_handle(
        &mut self,
        endpoint_key: Option<String>,
        route_key: String,
        endpoint: &ForwardProxyEndpoint,
        egress_socks5_url: Option<&Url>,
        temporary: bool,
    ) -> Result<String, ProxyError> {
        let relay_id = self.next_relay_id(&route_key, temporary);
        let mut local_port_reservation = reserve_unused_local_port()?;
        let local_port = local_port_reservation.port();
        let local_proxy_url =
            Url::parse(&format!("socks5h://127.0.0.1:{local_port}")).map_err(|err| {
                ProxyError::Other(format!("failed to build local xray socks endpoint: {err}"))
            })?;
        let inbound_tag = format!("relay-in::{relay_id}");
        let outbound_tag = format!("relay-out::{relay_id}");
        let egress_tag = egress_socks5_url.map(|_| format!("relay-egress::{relay_id}"));
        let rule_tag = format!("relay-rule::{relay_id}");
        let config_paths = vec![
            self.runtime_dir.join(format!("{relay_id}-inbounds.json")),
            self.runtime_dir.join(format!("{relay_id}-outbounds.json")),
            self.runtime_dir.join(format!("{relay_id}-rules.json")),
        ];
        let cleanup_handle = Arc::new(SharedXrayRelayHandle {
            relay_id: relay_id.clone(),
            endpoint_key: endpoint_key.clone(),
            route_key: route_key.clone(),
            local_proxy_url: local_proxy_url.clone(),
            local_port,
            inbound_tag: inbound_tag.clone(),
            outbound_tags: {
                let mut tags = vec![outbound_tag.clone()];
                if let Some(egress_tag) = egress_tag.clone() {
                    tags.push(egress_tag);
                }
                tags
            },
            rule_tag: rule_tag.clone(),
            config_paths: config_paths.clone(),
            lease_count: AtomicUsize::new(0),
            invalidated: AtomicBool::new(false),
            temporary,
        });

        let mut outbound = build_xray_outbound_for_endpoint(endpoint, egress_tag.as_deref())?;
        set_xray_outbound_tag(&mut outbound, &outbound_tag);
        let mut outbounds = vec![outbound];
        if let Some(egress_socks5_url) = egress_socks5_url
            && let Some(egress_outbound) =
                build_xray_egress_outbound(Some(egress_socks5_url), egress_tag.as_deref())
        {
            outbounds.push(egress_outbound);
        }
        let inbound_config = json!({
            "inbounds": [{
                "tag": inbound_tag,
                "listen": "127.0.0.1",
                "port": local_port,
                "protocol": "socks",
                "settings": { "auth": "noauth", "udp": false }
            }]
        });
        let outbound_config = json!({ "outbounds": outbounds });
        let rules_config = json!({
            "routing": {
                "domainStrategy": "AsIs",
                "rules": [{
                    "type": "field",
                    "inboundTag": [inbound_tag],
                    "outboundTag": outbound_tag,
                    "ruleTag": rule_tag,
                }]
            }
        });

        write_xray_runtime_json(&config_paths[0], &inbound_config)?;
        write_xray_runtime_json(&config_paths[1], &outbound_config)?;
        write_xray_runtime_json(&config_paths[2], &rules_config)?;
        self.ensure_shared_process_started().await?;
        local_port_reservation.release();

        if let Err(err) = self
            .run_shared_api_command("ado", &[config_paths[1].clone()], &[])
            .await
        {
            cleanup_handle.drop_runtime_files();
            self.shutdown_shared_process_if_idle().await;
            return Err(err);
        }
        if let Err(err) = self
            .run_shared_api_command("adi", &[config_paths[0].clone()], &[])
            .await
        {
            if let Err(cleanup_err) = self
                .run_shared_api_command(
                    "rmo",
                    &[],
                    &[outbound_tag.clone(), egress_tag.clone().unwrap_or_default()],
                )
                .await
            {
                cleanup_handle.drop_runtime_files();
                self.track_handle_for_retry(cleanup_handle);
                return Err(ProxyError::Other(format!(
                    "{err}; shared xray rollback failed after adi error: {cleanup_err}"
                )));
            }
            cleanup_handle.drop_runtime_files();
            self.shutdown_shared_process_if_idle().await;
            return Err(err);
        }
        if let Err(err) = self
            .run_shared_api_command(
                "adrules",
                &[config_paths[2].clone()],
                &["-append".to_string()],
            )
            .await
        {
            let inbound_cleanup = self
                .run_shared_api_command("rmi", &[], std::slice::from_ref(&inbound_tag))
                .await;
            let mut remove_outbound_args = vec![outbound_tag.clone()];
            if let Some(egress_tag) = egress_tag.clone() {
                remove_outbound_args.push(egress_tag);
            }
            let outbound_cleanup = self
                .run_shared_api_command("rmo", &[], &remove_outbound_args)
                .await;
            cleanup_handle.drop_runtime_files();
            if let Err(cleanup_err) = join_cleanup_errors([inbound_cleanup, outbound_cleanup]) {
                self.track_handle_for_retry(cleanup_handle);
                return Err(ProxyError::Other(format!(
                    "{err}; shared xray rollback failed after adrules error: {cleanup_err}"
                )));
            }
            self.shutdown_shared_process_if_idle().await;
            return Err(err);
        }
        if let Err(err) = wait_for_local_socks_ready(
            local_port,
            Duration::from_millis(XRAY_PROXY_READY_TIMEOUT_MS),
        )
        .await
        {
            let rule_cleanup = self
                .run_shared_api_command("rmrules", &[], std::slice::from_ref(&rule_tag))
                .await;
            let inbound_cleanup = self
                .run_shared_api_command("rmi", &[], std::slice::from_ref(&inbound_tag))
                .await;
            let mut remove_outbound_args = vec![outbound_tag.clone()];
            if let Some(egress_tag) = egress_tag.clone() {
                remove_outbound_args.push(egress_tag);
            }
            let outbound_cleanup = self
                .run_shared_api_command("rmo", &[], &remove_outbound_args)
                .await;
            cleanup_handle.drop_runtime_files();
            if let Err(cleanup_err) =
                join_cleanup_errors([rule_cleanup, inbound_cleanup, outbound_cleanup])
            {
                self.track_handle_for_retry(cleanup_handle);
                return Err(ProxyError::Other(format!(
                    "{err}; shared xray rollback failed after local port wait error: {cleanup_err}"
                )));
            }
            self.shutdown_shared_process_if_idle().await;
            return Err(err);
        }

        self.handle_by_url
            .insert(local_proxy_url.to_string(), relay_id.clone());
        self.handles
            .insert(relay_id.clone(), Arc::clone(&cleanup_handle));
        if temporary {
            self.retiring_handles.insert(relay_id.clone());
        }
        Ok(relay_id)
    }

    async fn mark_handle_retiring(&mut self, relay_id: &str) {
        if self.handles.contains_key(relay_id) {
            self.retiring_handles.insert(relay_id.to_string());
        }
        self.reap_retired_handles().await;
    }

    async fn reap_retired_handles(&mut self) {
        let removable = self
            .retiring_handles
            .iter()
            .filter_map(|relay_id| {
                self.handles
                    .get(relay_id)
                    .filter(|handle| handle.lease_count() == 0 && Arc::strong_count(handle) == 1)
                    .map(|_| relay_id.clone())
            })
            .collect::<Vec<_>>();
        for relay_id in removable {
            self.force_remove_handle(&relay_id).await;
        }
        self.shutdown_shared_process_if_idle().await;
    }

    async fn force_remove_handle(&mut self, relay_id: &str) {
        let Some(handle) = self.handles.get(relay_id).cloned() else {
            return;
        };
        handle.invalidate();
        self.active_endpoint_handles
            .retain(|_, active_relay_id| active_relay_id != relay_id);
        self.handle_by_url.remove(handle.local_proxy_url.as_str());
        self.retiring_handles.insert(relay_id.to_string());
        match self.cleanup_handle_from_shared_process(&handle).await {
            Ok(()) => {}
            Err(_) => {
                self.track_handle_for_retry(handle);
                return;
            }
        }
        self.retiring_handles.remove(relay_id);
        self.handles.remove(relay_id);
    }

    fn track_handle_for_retry(&mut self, handle: Arc<SharedXrayRelayHandle>) {
        handle.drop_runtime_files();
        self.handles
            .insert(handle.relay_id.clone(), Arc::clone(&handle));
        self.retiring_handles.insert(handle.relay_id.clone());
    }

    async fn cleanup_handle_from_shared_process(
        &mut self,
        handle: &SharedXrayRelayHandle,
    ) -> Result<(), ProxyError> {
        self.reset_if_shared_process_exited();
        if self.shared.is_none() {
            cleanup_paths(&handle.config_paths);
            return Ok(());
        }
        let rule_cleanup = self
            .run_shared_api_command("rmrules", &[], std::slice::from_ref(&handle.rule_tag))
            .await;
        let inbound_cleanup = self
            .run_shared_api_command("rmi", &[], std::slice::from_ref(&handle.inbound_tag))
            .await;
        let outbound_cleanup = self
            .run_shared_api_command("rmo", &[], &handle.outbound_tags)
            .await;
        join_cleanup_errors([rule_cleanup, inbound_cleanup, outbound_cleanup])?;
        cleanup_paths(&handle.config_paths);
        Ok(())
    }

    async fn ensure_shared_process_started(&mut self) -> Result<(), ProxyError> {
        self.reset_if_shared_process_exited();
        if self.shared.is_some() {
            return Ok(());
        }
        let _ = fs::create_dir_all(&self.runtime_dir);
        let mut api_port_reservation = reserve_unused_local_port()?;
        let api_port = api_port_reservation.port();
        let api_server = format!("127.0.0.1:{api_port}");
        let config_path = self.runtime_dir.join("shared-xray-base.json");
        let config = json!({
            "log": { "loglevel": "warning" },
            "api": {
                "tag": "api",
                "listen": api_server.clone(),
                "services": ["HandlerService", "RoutingService"]
            },
            "routing": {
                "domainStrategy": "AsIs",
                "rules": []
            },
            "outbounds": [{ "tag": "direct", "protocol": "freedom" }]
        });
        write_xray_runtime_json(&config_path, &config)?;
        api_port_reservation.release();

        let mut child = Command::new(&self.binary)
            .arg("run")
            .arg("-c")
            .arg(&config_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| {
                ProxyError::Other(format!(
                    "failed to start xray binary {}: {err}",
                    self.binary
                ))
            })?;

        if let Err(err) = wait_for_xray_api_ready(
            &mut child,
            api_port,
            Duration::from_millis(XRAY_PROXY_READY_TIMEOUT_MS),
        )
        .await
        {
            let _ = terminate_child_process(&mut child, Duration::from_secs(2)).await;
            let stderr_tail = child.wait_with_output().await.ok().and_then(|output| {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                if stderr.is_empty() {
                    None
                } else {
                    Some(
                        stderr
                            .lines()
                            .rev()
                            .take(3)
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect::<Vec<_>>()
                            .join(" | "),
                    )
                }
            });
            let _ = fs::remove_file(&config_path);
            return Err(if let Some(stderr_tail) = stderr_tail {
                ProxyError::Other(format!("{err} ({stderr_tail})"))
            } else {
                err
            });
        }

        self.shared = Some(SharedXrayProcess {
            api_server,
            api_port,
            config_path,
            child,
        });
        Ok(())
    }

    async fn shutdown_shared_process_if_idle(&mut self) {
        if !self.handles.is_empty() {
            return;
        }
        self.shutdown_shared_process().await;
    }

    async fn shutdown_shared_process(&mut self) {
        if let Some(mut shared) = self.shared.take() {
            let _ = terminate_child_process(&mut shared.child, Duration::from_secs(2)).await;
            let _ = fs::remove_file(&shared.config_path);
        }
    }

    fn reset_if_shared_process_exited(&mut self) {
        let Some(shared) = self.shared.as_mut() else {
            return;
        };
        let exited = match shared.child.try_wait() {
            Ok(None) => false,
            Ok(Some(_)) => true,
            Err(_) => true,
        };
        if !exited {
            return;
        }
        let _ = fs::remove_file(&shared.config_path);
        let runtime_paths = self
            .handles
            .values()
            .flat_map(|handle| handle.config_paths.iter().cloned())
            .collect::<Vec<_>>();
        cleanup_paths(&runtime_paths);
        for handle in self.handles.values() {
            handle.invalidate();
        }
        self.shared = None;
        self.active_endpoint_handles.clear();
        self.handles.clear();
        self.retiring_handles.clear();
        self.handle_by_url.clear();
    }

    async fn run_shared_api_command(
        &mut self,
        command: &str,
        config_paths: &[PathBuf],
        args: &[String],
    ) -> Result<(), ProxyError> {
        self.ensure_shared_process_started().await?;
        let shared = self.shared.as_ref().ok_or_else(|| {
            ProxyError::Other("shared xray process missing after startup".to_string())
        })?;
        let mut cmd = Command::new(&self.binary);
        cmd.arg("api")
            .arg(command)
            .arg(format!("--server={}", shared.api_server))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        for arg in args {
            if !arg.is_empty() {
                cmd.arg(arg);
            }
        }
        for config_path in config_paths {
            cmd.arg(config_path);
        }
        let output = cmd.output().await.map_err(|err| {
            ProxyError::Other(format!(
                "failed to run xray api {command} via {}: {err}",
                self.binary
            ))
        })?;
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let suffix = if stderr.is_empty() {
            format!("status {}", output.status)
        } else {
            stderr
        };
        Err(ProxyError::Other(format!(
            "xray api {command} failed: {suffix}"
        )))
    }

    fn next_relay_id(&mut self, route_key: &str, temporary: bool) -> String {
        self.handle_nonce = self.handle_nonce.wrapping_add(1);
        let prefix = if temporary { "temp" } else { "relay" };
        format!(
            "{prefix}-{:016x}-{:08x}",
            stable_hash_u64(route_key),
            self.handle_nonce,
        )
    }
}

