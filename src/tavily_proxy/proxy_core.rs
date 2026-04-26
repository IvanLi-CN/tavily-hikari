impl TavilyProxy {
    fn affinity_subject_score(subject: &str, key_id: &str) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(subject.as_bytes());
        digest.update(b":");
        digest.update(key_id.as_bytes());
        digest.finalize().into()
    }

    fn sha256_hex(value: &str) -> String {
        let digest: [u8; 32] = Sha256::digest(value.as_bytes()).into();
        let mut hex = String::with_capacity(digest.len() * 2);
        for byte in digest {
            use std::fmt::Write as _;
            let _ = write!(&mut hex, "{byte:02x}");
        }
        hex
    }

    fn mcp_session_affinity_subject(user_id: Option<&str>, token_id: &str) -> String {
        match user_id {
            Some(user_id) => format!("user:{user_id}"),
            None => format!("token:{token_id}"),
        }
    }

    fn mcp_session_affinity_score(subject: &str, key_id: &str) -> [u8; 32] {
        Self::affinity_subject_score(subject, key_id)
    }

    fn http_project_affinity_subject(owner_subject: &str, project_id_hash: &str) -> String {
        format!("{owner_subject}:project:{project_id_hash}")
    }

    fn http_project_affinity_reused_effect() -> KeyEffect {
        KeyEffect::new(
            KEY_EFFECT_HTTP_PROJECT_AFFINITY_REUSED,
            "HTTP project affinity reused the existing upstream key binding",
        )
    }

    fn http_project_affinity_bound_effect() -> KeyEffect {
        KeyEffect::new(
            KEY_EFFECT_HTTP_PROJECT_AFFINITY_BOUND,
            "HTTP project affinity created a new upstream key binding",
        )
    }

    fn http_project_affinity_rebound_effect() -> KeyEffect {
        KeyEffect::new(
            KEY_EFFECT_HTTP_PROJECT_AFFINITY_REBOUND,
            "HTTP project affinity rebound the project onto a different upstream key",
        )
    }

    fn primary_request_effect(
        key_effect: &KeyEffect,
        binding_effect: &KeyEffect,
        selection_effect: &KeyEffect,
    ) -> KeyEffect {
        if key_effect.code != KEY_EFFECT_NONE {
            key_effect.clone()
        } else if selection_effect.code != KEY_EFFECT_NONE {
            selection_effect.clone()
        } else if binding_effect.code != KEY_EFFECT_NONE {
            binding_effect.clone()
        } else {
            KeyEffect::none()
        }
    }

    fn mcp_session_init_lock_subject(user_id: Option<&str>, token_id: &str) -> String {
        format!(
            "mcp-init:{}",
            Self::mcp_session_affinity_subject(user_id, token_id)
        )
    }

    fn mcp_session_request_lock_subject(proxy_session_id: &str) -> String {
        format!("mcp-session:{proxy_session_id}")
    }

    pub(crate) fn parse_retry_after_secs_value(value: &str, now_ts: i64) -> Option<i64> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }

        if let Ok(seconds) = trimmed.parse::<i64>() {
            return Some(seconds.max(0));
        }

        let parsed = httpdate::parse_http_date(trimmed).ok()?;
        let now_secs = now_ts.max(0) as u64;
        let now = std::time::UNIX_EPOCH.checked_add(Duration::from_secs(now_secs))?;
        match parsed.duration_since(now) {
            Ok(delta) => Some(delta.as_secs().min(i64::MAX as u64) as i64),
            Err(_) => Some(0),
        }
    }

    fn parse_retry_after_secs(headers: &HeaderMap, now_ts: i64) -> Option<i64> {
        headers
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| Self::parse_retry_after_secs_value(value, now_ts))
    }

    pub(crate) fn mcp_session_init_retry_after_secs(headers: &HeaderMap, now_ts: i64) -> i64 {
        Self::parse_retry_after_secs(headers, now_ts)
            .unwrap_or(MCP_SESSION_INIT_BACKOFF_DEFAULT_SECS)
            .clamp(
                MCP_SESSION_INIT_BACKOFF_MIN_SECS,
                MCP_SESSION_INIT_BACKOFF_MAX_SECS,
            )
    }

    pub(crate) fn order_mcp_session_init_candidates(candidates: &mut [McpSessionInitCandidate]) {
        candidates.sort_by(|left, right| {
            left.cooldown_until
                .is_some()
                .cmp(&right.cooldown_until.is_some())
                .then_with(|| {
                    left.cooldown_until
                        .unwrap_or_default()
                        .cmp(&right.cooldown_until.unwrap_or_default())
                })
                .then_with(|| {
                    left.recent_rate_limited_count
                        .cmp(&right.recent_rate_limited_count)
                })
                .then_with(|| {
                    left.recent_billable_request_count
                        .cmp(&right.recent_billable_request_count)
                })
                .then_with(|| left.active_session_count.cmp(&right.active_session_count))
                .then_with(|| left.last_used_at.cmp(&right.last_used_at))
                .then_with(|| left.stable_rank_index.cmp(&right.stable_rank_index))
                .then_with(|| left.key_id.cmp(&right.key_id))
        });
    }

    pub(crate) fn mcp_session_init_selection_effect(
        ordered: &[McpSessionInitCandidate],
    ) -> KeyEffect {
        let Some(selected) = ordered.first() else {
            return KeyEffect::none();
        };
        if selected.stable_rank_index == 0 {
            return KeyEffect::none();
        }
        let Some(stable_front) = ordered
            .iter()
            .find(|candidate| candidate.stable_rank_index == 0)
        else {
            return KeyEffect::none();
        };

        if stable_front.cooldown_until.is_some()
            && (selected.cooldown_until.is_none()
                || selected.cooldown_until < stable_front.cooldown_until)
        {
            return KeyEffect::new(
                KEY_EFFECT_MCP_SESSION_INIT_COOLDOWN_AVOIDED,
                "MCP initialize skipped a cooled key inside the affinity pool",
            );
        }

        if selected.recent_rate_limited_count < stable_front.recent_rate_limited_count {
            return KeyEffect::new(
                KEY_EFFECT_MCP_SESSION_INIT_RATE_LIMIT_AVOIDED,
                "MCP initialize skipped a recently rate-limited key inside the affinity pool",
            );
        }

        if selected.recent_billable_request_count < stable_front.recent_billable_request_count
            || selected.active_session_count < stable_front.active_session_count
            || selected.last_used_at < stable_front.last_used_at
        {
            return KeyEffect::new(
                KEY_EFFECT_MCP_SESSION_INIT_PRESSURE_AVOIDED,
                "MCP initialize skipped a hotter key inside the affinity pool",
            );
        }

        KeyEffect::none()
    }

    pub(crate) fn order_http_project_affinity_candidates(
        candidates: &mut [HttpProjectAffinityCandidate],
    ) {
        candidates.sort_by(|left, right| {
            left.cooldown_until
                .is_some()
                .cmp(&right.cooldown_until.is_some())
                .then_with(|| {
                    left.cooldown_until
                        .unwrap_or_default()
                        .cmp(&right.cooldown_until.unwrap_or_default())
                })
                .then_with(|| {
                    left.recent_rate_limited_count
                        .cmp(&right.recent_rate_limited_count)
                })
                .then_with(|| {
                    left.recent_billable_request_count
                        .cmp(&right.recent_billable_request_count)
                })
                .then_with(|| left.last_used_at.cmp(&right.last_used_at))
                .then_with(|| left.stable_rank_index.cmp(&right.stable_rank_index))
                .then_with(|| left.key_id.cmp(&right.key_id))
        });
    }

    pub(crate) fn http_project_affinity_selection_effect(
        ordered: &[HttpProjectAffinityCandidate],
    ) -> KeyEffect {
        let Some(selected) = ordered.first() else {
            return KeyEffect::none();
        };
        if selected.stable_rank_index == 0 {
            return KeyEffect::none();
        }
        let Some(stable_front) = ordered
            .iter()
            .find(|candidate| candidate.stable_rank_index == 0)
        else {
            return KeyEffect::none();
        };

        if stable_front.cooldown_until.is_some()
            && (selected.cooldown_until.is_none()
                || selected.cooldown_until < stable_front.cooldown_until)
        {
            return KeyEffect::new(
                KEY_EFFECT_HTTP_PROJECT_AFFINITY_COOLDOWN_AVOIDED,
                "HTTP project affinity skipped a cooled key inside the project pool",
            );
        }

        if selected.recent_rate_limited_count < stable_front.recent_rate_limited_count {
            return KeyEffect::new(
                KEY_EFFECT_HTTP_PROJECT_AFFINITY_RATE_LIMIT_AVOIDED,
                "HTTP project affinity skipped a recently rate-limited key inside the project pool",
            );
        }

        if selected.recent_billable_request_count < stable_front.recent_billable_request_count
            || selected.last_used_at < stable_front.last_used_at
        {
            return KeyEffect::new(
                KEY_EFFECT_HTTP_PROJECT_AFFINITY_PRESSURE_AVOIDED,
                "HTTP project affinity skipped a hotter key inside the project pool",
            );
        }

        KeyEffect::none()
    }

    fn mcp_session_init_backoff_effect() -> KeyEffect {
        KeyEffect::new(
            KEY_EFFECT_MCP_SESSION_INIT_BACKOFF_SET,
            "The system temporarily cooled this key for future MCP session placement",
        )
    }

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
        let system_settings = key_store.get_system_settings().await?;
        token_request_limit.set_request_limit(system_settings.request_rate_limit);
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
            summary_windows_cache: Arc::new(Mutex::new(SummaryWindowsCacheState::default())),
            dashboard_hourly_request_window_cache: Arc::new(Mutex::new(
                DashboardHourlyRequestWindowCacheState::default(),
            )),
            token_billing_locks: Arc::new(Mutex::new(HashMap::new())),
            mcp_session_init_locks: Arc::new(Mutex::new(HashMap::new())),
            mcp_session_request_locks: Arc::new(Mutex::new(HashMap::new())),
            research_key_locks: Arc::new(Mutex::new(HashMap::new())),
            low_quota_depletion_threshold: options.low_quota_depletion_threshold,
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

    pub async fn get_system_settings(&self) -> Result<SystemSettings, ProxyError> {
        self.key_store.get_system_settings().await
    }

    pub async fn set_system_settings(
        &self,
        settings: &SystemSettings,
    ) -> Result<SystemSettings, ProxyError> {
        let updated = self.key_store.set_system_settings(settings).await?;
        self.token_request_limit
            .set_request_limit(updated.request_rate_limit);
        Ok(updated)
    }

    pub async fn set_mcp_session_affinity_key_count(
        &self,
        count: i64,
    ) -> Result<SystemSettings, ProxyError> {
        self.key_store
            .set_mcp_session_affinity_key_count(count)
            .await
    }

    pub async fn seed_user_primary_api_key_affinity_for_test(
        &self,
        user_id: &str,
        key_id: &str,
    ) -> Result<(), ProxyError> {
        self.key_store
            .sync_user_primary_api_key_affinity(user_id, key_id)
            .await
    }

    pub async fn acquire_key_id_for_test(
        &self,
        auth_token_id: Option<&str>,
    ) -> Result<String, ProxyError> {
        self.acquire_key_for(auth_token_id)
            .await
            .map(|lease| lease.id)
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
        let mut validation_leases = Vec::with_capacity(probe_total);
        let validation_result = async {
            let mut resolved_endpoints = Vec::with_capacity(probe_total);

            for endpoint in &endpoints {
                ensure_forward_proxy_not_cancelled(cancellation)?;
                let (resolved_endpoint, relay_lease) = self
                    .resolve_forward_proxy_validation_endpoint(endpoint)
                    .await?;
                validation_leases.push(relay_lease);
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
        for relay_lease in validation_leases {
            relay_lease.release().await;
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

}
