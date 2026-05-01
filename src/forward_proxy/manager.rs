impl ForwardProxyManager {
    pub fn new(
        settings: ForwardProxySettings,
        runtime_rows: Vec<ForwardProxyRuntimeState>,
    ) -> Self {
        let runtime = runtime_rows
            .into_iter()
            .map(|entry| (entry.proxy_key.clone(), entry))
            .collect::<HashMap<_, _>>();
        let mut manager = Self {
            settings,
            endpoints: Vec::new(),
            runtime,
            selection_counter: 0,
            requests_since_probe: 0,
            probe_in_flight: false,
            last_probe_at: Utc::now().timestamp() - FORWARD_PROXY_PROBE_INTERVAL_SECS,
            last_subscription_refresh_at: None,
            window_stats_cache: Arc::new(RwLock::new(None)),
        };
        manager.rebuild_endpoints(Vec::new());
        manager
    }

    pub fn apply_settings(&mut self, settings: ForwardProxySettings) {
        self.settings = settings;
        self.rebuild_endpoints(Vec::new());
    }

    pub fn update_settings_only(&mut self, settings: ForwardProxySettings) {
        self.settings = settings;
    }

    pub fn apply_subscription_refresh(
        &mut self,
        subscription_proxy_urls: &HashMap<String, Vec<String>>,
    ) {
        let mut subscription_endpoints = Vec::new();
        for (subscription_source, proxy_urls) in subscription_proxy_urls {
            subscription_endpoints.extend(normalize_subscription_endpoints_from_urls(
                proxy_urls,
                subscription_source,
            ));
        }
        self.rebuild_endpoints(subscription_endpoints);
        self.last_subscription_refresh_at = Some(Utc::now().timestamp());
    }

    pub fn rebuild_endpoints(&mut self, subscription_endpoints: Vec<ForwardProxyEndpoint>) {
        let manual = normalize_proxy_endpoints_from_urls(&self.settings.proxy_urls);
        let mut merged: Vec<ForwardProxyEndpoint> = Vec::new();
        let mut positions: HashMap<String, usize> = HashMap::new();
        for endpoint in manual.into_iter().chain(subscription_endpoints.into_iter()) {
            if let Some(index) = positions.get(&endpoint.key).copied() {
                merged[index].absorb_duplicate(endpoint);
            } else {
                positions.insert(endpoint.key.clone(), merged.len());
                merged.push(endpoint);
            }
        }
        if self.settings.insert_direct {
            merged.push(ForwardProxyEndpoint::direct());
        }
        if merged.is_empty()
            && self.settings.proxy_urls.is_empty()
            && self.settings.subscription_urls.is_empty()
        {
            merged.push(ForwardProxyEndpoint::direct());
        }
        self.endpoints = merged;

        for endpoint in &self.endpoints {
            match self.runtime.entry(endpoint.key.clone()) {
                std::collections::hash_map::Entry::Occupied(mut occupied) => {
                    let runtime = occupied.get_mut();
                    runtime.display_name = endpoint.display_name.clone();
                    runtime.source = endpoint.source.clone();
                    runtime.kind = endpoint.protocol.as_str().to_string();
                    runtime.endpoint_url = endpoint
                        .endpoint_url
                        .as_ref()
                        .map(Url::to_string)
                        .or_else(|| endpoint.raw_url.clone());
                    runtime.available = endpoint.is_selectable();
                    runtime.last_error = if endpoint.is_selectable() {
                        None
                    } else {
                        Some("xray_missing".to_string())
                    };
                }
                std::collections::hash_map::Entry::Vacant(vacant) => {
                    vacant.insert(ForwardProxyRuntimeState::default_for_endpoint(endpoint));
                }
            }
        }
        self.ensure_non_zero_weight();
    }

    pub fn apply_incremental_settings(
        &mut self,
        settings: ForwardProxySettings,
        fetched_subscriptions: &HashMap<String, Vec<String>>,
    ) -> Vec<ForwardProxyEndpoint> {
        let previous_keys = self
            .endpoints
            .iter()
            .map(|endpoint| endpoint.key.clone())
            .collect::<HashSet<_>>();
        self.settings = settings.clone();

        let manual_by_key = normalize_proxy_endpoints_from_urls(&settings.proxy_urls)
            .into_iter()
            .map(|endpoint| (endpoint.key.clone(), endpoint))
            .collect::<HashMap<_, _>>();
        let desired_subscription_sources = settings
            .subscription_urls
            .iter()
            .cloned()
            .collect::<HashSet<_>>();

        let mut merged = Vec::new();
        let mut seen = HashSet::new();

        for mut endpoint in self.endpoints.clone() {
            if endpoint.is_direct() {
                continue;
            }
            endpoint.manual_present = manual_by_key.contains_key(&endpoint.key);
            endpoint
                .subscription_sources
                .retain(|source| desired_subscription_sources.contains(source));
            if endpoint.manual_present || endpoint.is_subscription_backed() {
                endpoint.refresh_source();
                if seen.insert(endpoint.key.clone()) {
                    merged.push(endpoint);
                }
            }
        }

        for (key, manual_endpoint) in &manual_by_key {
            if let Some(existing) = merged.iter_mut().find(|endpoint| endpoint.key == *key) {
                existing.manual_present = true;
                existing.display_name = manual_endpoint.display_name.clone();
                existing.protocol = manual_endpoint.protocol;
                existing.raw_url = manual_endpoint.raw_url.clone();
                existing.refresh_source();
                continue;
            }
            if seen.insert(key.clone()) {
                merged.push(manual_endpoint.clone());
            }
        }

        for (subscription_source, proxy_urls) in fetched_subscriptions {
            for mut endpoint in
                normalize_subscription_endpoints_from_urls(proxy_urls, subscription_source)
            {
                if let Some(existing) = merged
                    .iter_mut()
                    .find(|candidate| candidate.key == endpoint.key)
                {
                    existing
                        .subscription_sources
                        .append(&mut endpoint.subscription_sources);
                    existing.refresh_source();
                    continue;
                }
                if seen.insert(endpoint.key.clone()) {
                    merged.push(endpoint);
                }
            }
        }

        if !fetched_subscriptions.is_empty() {
            self.last_subscription_refresh_at = Some(Utc::now().timestamp());
        }

        if settings.insert_direct {
            merged.push(ForwardProxyEndpoint::direct());
        }
        if merged.is_empty()
            && settings.proxy_urls.is_empty()
            && settings.subscription_urls.is_empty()
        {
            merged.push(ForwardProxyEndpoint::direct());
        }

        self.endpoints = merged;
        for endpoint in &self.endpoints {
            match self.runtime.entry(endpoint.key.clone()) {
                std::collections::hash_map::Entry::Occupied(mut occupied) => {
                    let runtime = occupied.get_mut();
                    runtime.display_name = endpoint.display_name.clone();
                    runtime.source = endpoint.source.clone();
                    runtime.kind = endpoint.protocol.as_str().to_string();
                    runtime.endpoint_url = endpoint
                        .endpoint_url
                        .as_ref()
                        .map(Url::to_string)
                        .or_else(|| endpoint.raw_url.clone());
                    runtime.available = endpoint.is_selectable();
                    runtime.last_error = if endpoint.is_selectable() {
                        None
                    } else {
                        Some("xray_missing".to_string())
                    };
                }
                std::collections::hash_map::Entry::Vacant(vacant) => {
                    vacant.insert(ForwardProxyRuntimeState::default_for_endpoint(endpoint));
                }
            }
        }
        self.ensure_non_zero_weight();

        self.endpoints
            .iter()
            .filter(|endpoint| !previous_keys.contains(&endpoint.key))
            .cloned()
            .collect()
    }

    pub fn ensure_non_zero_weight(&mut self) {
        let selectable_keys = self.selectable_endpoint_keys();
        let mut positive_count = self
            .runtime
            .values()
            .filter(|entry| {
                selectable_keys.contains(entry.proxy_key.as_str()) && entry.weight > 0.0
            })
            .count();
        if positive_count >= 1 {
            return;
        }
        let mut candidates = self
            .runtime
            .values()
            .filter(|entry| selectable_keys.contains(entry.proxy_key.as_str()))
            .map(|entry| (entry.proxy_key.clone(), entry.weight))
            .collect::<Vec<_>>();
        candidates.sort_by(|lhs, rhs| rhs.1.total_cmp(&lhs.1));
        for (proxy_key, _) in candidates {
            if let Some(entry) = self.runtime.get_mut(&proxy_key)
                && entry.weight <= 0.0
            {
                entry.weight = FORWARD_PROXY_PROBE_RECOVERY_WEIGHT;
                positive_count += 1;
            }
            if positive_count >= 1 {
                break;
            }
        }
    }

    fn selectable_endpoint_keys(&self) -> HashSet<&str> {
        self.endpoints
            .iter()
            .filter(|endpoint| endpoint.is_selectable())
            .map(|endpoint| endpoint.key.as_str())
            .collect::<HashSet<_>>()
    }

    pub fn snapshot_runtime(&self) -> Vec<ForwardProxyRuntimeState> {
        self.endpoints
            .iter()
            .filter_map(|endpoint| self.runtime.get(&endpoint.key).cloned())
            .collect()
    }

    pub fn endpoint_by_key(&self, key: &str) -> Option<ForwardProxyEndpoint> {
        self.endpoints
            .iter()
            .find(|endpoint| endpoint.key == key)
            .cloned()
    }

    pub fn endpoint(&self, key: &str) -> Option<&ForwardProxyEndpoint> {
        self.endpoints.iter().find(|endpoint| endpoint.key == key)
    }

    pub fn runtime(&self, key: &str) -> Option<&ForwardProxyRuntimeState> {
        self.runtime.get(key)
    }

    pub fn select_proxy(&mut self) -> SelectedForwardProxy {
        self.selection_counter = self.selection_counter.wrapping_add(1);
        self.note_request();
        self.ensure_non_zero_weight();

        let mut candidates = Vec::new();
        let mut total_weight = 0.0f64;
        for endpoint in &self.endpoints {
            if !endpoint.is_selectable() {
                continue;
            }
            if let Some(runtime) = self.runtime.get(&endpoint.key)
                && runtime.weight > 0.0
                && runtime.weight.is_finite()
            {
                total_weight += runtime.weight;
                candidates.push((endpoint, runtime.weight));
            }
        }

        if candidates.is_empty() {
            let fallback = self
                .endpoints
                .iter()
                .find(|endpoint| endpoint.protocol == ForwardProxyProtocol::Direct)
                .cloned()
                .or_else(|| {
                    self.endpoints
                        .iter()
                        .find(|endpoint| endpoint.is_selectable())
                        .cloned()
                })
                .or_else(|| self.endpoints.first().cloned())
                .unwrap_or_else(ForwardProxyEndpoint::direct);
            return SelectedForwardProxy::from_endpoint(&fallback);
        }

        let random = deterministic_unit_f64(self.selection_counter);
        let mut threshold = random * total_weight;
        let mut last_candidate = candidates[0].0;
        for (endpoint, weight) in candidates {
            last_candidate = endpoint;
            if threshold <= weight {
                return SelectedForwardProxy::from_endpoint(endpoint);
            }
            threshold -= weight;
        }
        SelectedForwardProxy::from_endpoint(last_candidate)
    }

    pub fn note_request(&mut self) {
        self.requests_since_probe = self.requests_since_probe.saturating_add(1);
    }

    pub fn record_attempt(
        &mut self,
        proxy_key: &str,
        success: bool,
        latency_ms: Option<f64>,
        failure_kind: Option<&str>,
    ) {
        if !self
            .endpoints
            .iter()
            .any(|endpoint| endpoint.key == proxy_key)
        {
            return;
        }
        let Some(runtime) = self.runtime.get_mut(proxy_key) else {
            return;
        };
        update_runtime_ema(runtime, success, latency_ms);
        if success {
            runtime.consecutive_failures = 0;
            runtime.available = true;
            runtime.last_error = None;
            let latency_penalty = runtime
                .latency_ema_ms
                .map(|value| (value / 2500.0).min(0.6))
                .unwrap_or(0.0);
            runtime.weight += FORWARD_PROXY_WEIGHT_SUCCESS_BONUS - latency_penalty;
            if runtime.weight <= 0.0 {
                runtime.weight = FORWARD_PROXY_PROBE_RECOVERY_WEIGHT;
            }
        } else {
            runtime.consecutive_failures = runtime.consecutive_failures.saturating_add(1);
            runtime.available = false;
            runtime.last_error = failure_kind.map(ToOwned::to_owned);
            let failure_penalty = FORWARD_PROXY_WEIGHT_FAILURE_PENALTY_BASE
                + f64::from(runtime.consecutive_failures.saturating_sub(1))
                    * FORWARD_PROXY_WEIGHT_FAILURE_PENALTY_STEP;
            runtime.weight -= failure_penalty;
        }
        runtime.weight = runtime
            .weight
            .clamp(FORWARD_PROXY_WEIGHT_MIN, FORWARD_PROXY_WEIGHT_MAX);
        if success && runtime.weight < FORWARD_PROXY_WEIGHT_RECOVERY {
            runtime.weight = runtime.weight.max(FORWARD_PROXY_WEIGHT_RECOVERY * 0.5);
        }
        self.ensure_non_zero_weight();
    }

    pub fn should_refresh_subscriptions(&self) -> bool {
        if self.settings.subscription_urls.is_empty() {
            return false;
        }
        let Some(last_refresh_at) = self.last_subscription_refresh_at else {
            return true;
        };
        let interval =
            i64::try_from(self.settings.subscription_update_interval_secs).unwrap_or(i64::MAX);
        (Utc::now().timestamp() - last_refresh_at) >= interval
    }

    pub fn should_probe_penalized_proxy(&self) -> bool {
        let selectable_keys = self.selectable_endpoint_keys();
        let has_penalized = self.runtime.values().any(|entry| {
            selectable_keys.contains(entry.proxy_key.as_str()) && entry.is_penalized()
        });
        if !has_penalized || self.probe_in_flight {
            return false;
        }
        self.requests_since_probe >= FORWARD_PROXY_PROBE_EVERY_REQUESTS
            || (Utc::now().timestamp() - self.last_probe_at) >= FORWARD_PROXY_PROBE_INTERVAL_SECS
    }

    pub fn mark_probe_started(&mut self) -> Option<SelectedForwardProxy> {
        if !self.should_probe_penalized_proxy() {
            return None;
        }
        let selectable_keys = self.selectable_endpoint_keys();
        let selected = self
            .runtime
            .values()
            .filter(|entry| {
                entry.is_penalized() && selectable_keys.contains(entry.proxy_key.as_str())
            })
            .max_by(|lhs, rhs| lhs.weight.total_cmp(&rhs.weight))
            .and_then(|entry| {
                self.endpoints
                    .iter()
                    .find(|item| item.key == entry.proxy_key)
            })
            .cloned()?;
        self.probe_in_flight = true;
        self.requests_since_probe = 0;
        self.last_probe_at = Utc::now().timestamp();
        Some(SelectedForwardProxy::from_endpoint(&selected))
    }

    pub fn mark_probe_finished(&mut self) {
        self.probe_in_flight = false;
        self.last_probe_at = Utc::now().timestamp();
    }

    pub fn rank_candidates_for_subject(
        &self,
        subject: &str,
        exclude: &HashSet<String>,
        allow_direct: bool,
        limit: usize,
    ) -> Vec<ForwardProxyEndpoint> {
        let seed = stable_hash_u64(subject);
        let mut candidates = self
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.is_selectable())
            .filter(|endpoint| allow_direct || !endpoint.is_direct())
            .filter(|endpoint| !exclude.contains(&endpoint.key))
            .filter_map(|endpoint| {
                let runtime = self.runtime.get(&endpoint.key)?;
                if !runtime.available || !runtime.weight.is_finite() {
                    return None;
                }
                let score = runtime.weight + runtime.success_ema * 4.0
                    - runtime
                        .latency_ema_ms
                        .map(|latency| (latency / 1000.0).min(1.5))
                        .unwrap_or(0.0)
                    - if endpoint.is_direct() { 50.0 } else { 0.0 }
                    + deterministic_unit_f64(seed ^ stable_hash_u64(&endpoint.key)) * 0.05;
                Some((score, endpoint.clone()))
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|lhs, rhs| rhs.0.total_cmp(&lhs.0));
        candidates
            .into_iter()
            .take(limit.max(1))
            .map(|(_, endpoint)| endpoint)
            .collect()
    }
}

fn update_runtime_ema(
    runtime: &mut ForwardProxyRuntimeState,
    success: bool,
    latency_ms: Option<f64>,
) {
    runtime.success_ema = runtime.success_ema * 0.9 + if success { 0.1 } else { 0.0 };
    if let Some(latency_ms) = latency_ms.filter(|value| value.is_finite() && *value >= 0.0) {
        runtime.latency_ema_ms = Some(match runtime.latency_ema_ms {
            Some(previous) => previous * 0.8 + latency_ms * 0.2,
            None => latency_ms,
        });
    }
}

#[derive(Debug, Clone)]
pub struct SelectedForwardProxy {
    pub key: String,
    pub source: String,
    pub display_name: String,
    pub kind: String,
    pub endpoint_url: Option<Url>,
    pub endpoint_url_raw: Option<String>,
    uses_local_relay: bool,
    relay_handle: Option<Arc<SharedXrayRelayHandle>>,
}
