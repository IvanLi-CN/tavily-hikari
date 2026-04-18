impl TavilyProxy {
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
    ) -> Result<
        (
            forward_proxy::ForwardProxyEndpoint,
            forward_proxy::ForwardProxyRelayLease,
        ),
        ProxyError,
    > {
        let egress_socks5_url = self.current_forward_proxy_egress_socks5_url().await;
        let (resolved, relay_lease) = {
            let mut supervisor = self.xray_supervisor.lock().await;
            let resolved = supervisor
                .resolve_validation_endpoint(endpoint, egress_socks5_url.as_ref())
                .await?;
            let relay_lease = if resolved.uses_local_relay {
                let relay_handle = supervisor
                    .acquire_relay_handle_for_endpoint(&resolved)
                    .ok_or_else(|| ProxyError::Other("xray_missing".to_string()))?;
                forward_proxy::ForwardProxyRelayLease::from_acquired_handle(
                    Arc::clone(&self.xray_supervisor),
                    relay_handle,
                )
            } else {
                forward_proxy::ForwardProxyRelayLease::new(Arc::clone(&self.xray_supervisor))
            };
            (resolved, relay_lease)
        };
        Ok((resolved, relay_lease))
    }

    async fn recover_forward_proxy_candidate(
        &self,
        proxy_key: &str,
    ) -> Result<Option<forward_proxy::SelectedForwardProxy>, ProxyError> {
        let egress_socks5_url = self.current_forward_proxy_egress_socks5_url().await;
        let mut manager = self.forward_proxy.lock().await;
        let Some(current_endpoint) = manager.endpoint_by_key(proxy_key) else {
            return Ok(None);
        };
        if !current_endpoint.uses_local_relay {
            return Ok(current_endpoint
                .is_selectable()
                .then(|| forward_proxy::SelectedForwardProxy::from_endpoint(&current_endpoint)));
        }
        {
            let mut xray = self.xray_supervisor.lock().await;
            xray.sync_endpoints(&mut manager.endpoints, egress_socks5_url.as_ref())
                .await?;
        }
        self.sync_forward_proxy_runtime_state(&mut manager).await?;
        Ok(manager
            .endpoint_by_key(proxy_key)
            .filter(|endpoint| endpoint.is_selectable())
            .map(|endpoint| forward_proxy::SelectedForwardProxy::from_endpoint(&endpoint)))
    }

    pub(crate) async fn probe_forward_proxy_endpoint(
        &self,
        endpoint: &forward_proxy::ForwardProxyEndpoint,
        timeout: Duration,
        probe_url: &Url,
        cancellation: Option<&ForwardProxyCancellation>,
    ) -> Result<f64, ProxyError> {
        ensure_forward_proxy_not_cancelled(cancellation)?;
        let (resolved, relay_lease) = self
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
        .await;
        relay_lease.release().await;
        result?
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
        let (resolved, relay_lease) = self
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
        relay_lease.release().await;
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
    ) -> Result<(reqwest::Response, forward_proxy::ForwardProxyRelayLease), ProxyError>
    where
        F: FnMut(Client) -> reqwest::RequestBuilder,
    {
        let result = async {
            let mut last_error: Option<ProxyError> = None;
            for candidate in plan {
                let mut candidate = candidate;
                let mut attempted_recovery = false;
                let relay_lease = loop {
                    if let Some(relay_lease) =
                        forward_proxy::ForwardProxyRelayLease::acquire_for_selection(
                            Arc::clone(&self.xray_supervisor),
                            &candidate,
                        )
                        .await
                    {
                        break Some(relay_lease);
                    }
                    if !candidate.uses_local_relay() || attempted_recovery {
                        break None;
                    }
                    attempted_recovery = true;
                    match self.recover_forward_proxy_candidate(&candidate.key).await {
                        Ok(Some(recovered_candidate)) => {
                            candidate = recovered_candidate;
                        }
                        Ok(None) => break None,
                        Err(err) => {
                            last_error = Some(err);
                            break None;
                        }
                    }
                };
                let Some(relay_lease) = relay_lease else {
                    let _ = self
                        .record_forward_proxy_attempt(
                            &candidate.key,
                            affinity_owner_key_id,
                            request_kind,
                            false,
                            None,
                            Some("xray_missing"),
                        )
                        .await;
                    if last_error.is_none() {
                        last_error = Some(ProxyError::Other("xray_missing".to_string()));
                    }
                    continue;
                };
                let client = match self
                    .forward_proxy_clients
                    .client_for(candidate.endpoint_url.as_ref())
                    .await
                {
                    Ok(client) => client,
                    Err(err) => {
                        drop(relay_lease);
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
                        return Ok((response, relay_lease));
                    }
                    Err(err) => {
                        drop(relay_lease);
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
                    Ok((
                        response,
                        forward_proxy::ForwardProxyRelayLease::new(Arc::clone(
                            &self.xray_supervisor,
                        )),
                    ))
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
        .await;
        self.xray_supervisor
            .lock()
            .await
            .reap_retired_handles_now()
            .await;
        result
    }

    pub(crate) async fn send_with_forward_proxy<F>(
        &self,
        api_key_id: &str,
        request_kind: &str,
        build: F,
    ) -> Result<(reqwest::Response, forward_proxy::ForwardProxyRelayLease), ProxyError>
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
    ) -> Result<(reqwest::Response, forward_proxy::ForwardProxyRelayLease), ProxyError>
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

}
