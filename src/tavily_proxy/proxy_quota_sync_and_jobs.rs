impl TavilyProxy {
    pub async fn upstream_reconciliation_shadow_compare_active_with_settings(
        &self,
        settings: &SystemSettings,
    ) -> Result<bool, ProxyError> {
        self.key_store
            .upstream_reconciliation_shadow_compare_active_with_settings(settings)
            .await
    }

    pub async fn upstream_privacy_status(&self) -> Result<UpstreamPrivacyStatus, ProxyError> {
        let now = self.backend_time.now_ts();
        let settings = self.key_store.get_system_settings().await?;
        let active_upstream_mcp_sessions = self
            .key_store
            .count_active_upstream_mcp_sessions(now)
            .await?;
        let period = business_period_for_timestamp(now);
        let stored_epoch = self
            .key_store
            .get_meta_i64(META_KEY_UPSTREAM_RECONCILIATION_READY_AFTER_V1)
            .await?
            .unwrap_or(0);
        let mode_ready = settings.upstream_project_id_mode == UpstreamProjectIdMode::AccessToken;
        let api_ready = settings.api_rebalance_enabled;
        let mcp_ready = settings.rebalance_mcp_enabled;
        let shadow_ready = mode_ready && api_ready && mcp_ready;
        let sessions_ready = active_upstream_mcp_sessions == 0;
        let gates = vec![
            UpstreamPrivacyGate {
                key: "accessTokenMode".to_string(),
                ready: mode_ready,
                detail: format!("{:?}", settings.upstream_project_id_mode),
            },
            UpstreamPrivacyGate {
                key: "apiRebalance".to_string(),
                ready: api_ready,
                detail: if api_ready { "enabled" } else { "disabled" }.to_string(),
            },
            UpstreamPrivacyGate {
                key: "mcpRebalance".to_string(),
                ready: mcp_ready,
                detail: if mcp_ready { "enabled" } else { "disabled" }.to_string(),
            },
            UpstreamPrivacyGate {
                key: "controlSessionsDrained".to_string(),
                ready: sessions_ready,
                detail: active_upstream_mcp_sessions.to_string(),
            },
        ];
        let completed_gates = gates.iter().filter(|gate| gate.ready).count() as i64;
        let total_gates = gates.len() as i64;
        let (pending_research, queued_settlements, degraded_settlements) = self
            .key_store
            .upstream_reconciliation_queue_counts()
            .await?;
        let retry_buckets = self
            .key_store
            .upstream_reconciliation_retry_buckets()
            .await?;
        let (current_period_bound_users_by_key, current_period_pending_project_ids_by_key) = self
            .key_store
            .current_period_reconciliation_key_activity(&period.code)
            .await?;
        let (
            last_reconciliation_run_at,
            last_shadow_adjustment_at,
            last_reconciliation_enqueue_error_at,
        ) = self
            .key_store
            .upstream_reconciliation_runtime_markers()
            .await?;
        let next_epoch_at = if shadow_ready && settings.upstream_precise_reconciliation_enabled && sessions_ready {
            Some(if stored_epoch > 0 {
                stored_epoch
            } else {
                period.ends_at
            })
        } else {
            None
        };
        let phase = if degraded_settlements > 0 {
            "degraded"
        } else if !shadow_ready {
            "configured"
        } else if !settings.upstream_precise_reconciliation_enabled || !sessions_ready {
            "compare"
        } else if next_epoch_at.is_some_and(|epoch| now < epoch) {
            "pending"
        } else {
            "active"
        };
        Ok(UpstreamPrivacyStatus {
            phase: phase.to_string(),
            configured_project_id_mode: settings.upstream_project_id_mode,
            effective_project_id_mode: settings.upstream_project_id_mode,
            fixed_project_id_configured: !settings.upstream_project_id_fixed_value.is_empty(),
            configured_mcp_user_agent: settings.upstream_mcp_user_agent.clone(),
            effective_mcp_user_agent: (!settings.upstream_mcp_user_agent.is_empty())
                .then_some(settings.upstream_mcp_user_agent),
            upstream_precise_reconciliation_enabled: settings.upstream_precise_reconciliation_enabled,
            http_allowed_headers: vec![
                "accept".to_string(),
                "accept-encoding".to_string(),
                "content-type".to_string(),
                "x-project-id (policy injected)".to_string(),
            ],
            control_mcp_allowed_headers: vec![
                "accept".to_string(),
                "accept-encoding".to_string(),
                "cache-control".to_string(),
                "content-type".to_string(),
                "last-event-id".to_string(),
                "mcp-protocol-version".to_string(),
                "mcp-session-id".to_string(),
                "pragma".to_string(),
                "user-agent (configured only)".to_string(),
            ],
            gates,
            completed_gates,
            total_gates,
            active_upstream_mcp_sessions,
            current_period_code: period.code,
            current_period_ends_at: period.ends_at,
            next_epoch_at,
            pending_research,
            queued_settlements,
            degraded_settlements,
            last_reconciliation_run_at,
            last_shadow_adjustment_at,
            last_reconciliation_enqueue_error_at,
            retry_buckets,
            current_period_bound_users_by_key,
            current_period_pending_project_ids_by_key,
            recent_adjustments: self
                .key_store
                .recent_reconciliation_adjustments(10)
                .await?,
            generated_at: now,
        })
    }

    pub async fn record_upstream_reconciliation_usage(
        &self,
        token_id: &str,
        key_id: &str,
        billing_subject: &str,
        research_request_id: Option<&str>,
    ) -> Result<Option<BusinessPeriod>, ProxyError> {
        self.key_store
            .record_upstream_reconciliation_usage(
                token_id,
                key_id,
                billing_subject,
                research_request_id,
            )
            .await
    }

    pub async fn mark_upstream_reconciliation_research_terminal(
        &self,
        request_id: &str,
    ) -> Result<bool, ProxyError> {
        self.key_store
            .mark_upstream_reconciliation_research_terminal(request_id)
            .await
    }

    pub async fn shadow_daily_reconciled_usage_for_accounts(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, i64>, ProxyError> {
        let now = self.backend_time.now_utc().with_timezone(&Local);
        let window = server_local_day_window_utc(now);
        self.key_store
            .shadow_daily_reconciled_usage_for_accounts(user_ids, window.start, window.end)
            .await
    }

    pub async fn shadow_daily_projection_for_accounts(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, AccountShadowDailyProjection>, ProxyError> {
        let now = self.backend_time.now_utc().with_timezone(&Local);
        let window = server_local_day_window_utc(now);
        self.key_store
            .shadow_daily_projection_for_accounts(user_ids, window.start, window.end)
            .await
    }

    pub async fn upstream_reconciliation_queue_counts(&self) -> Result<(i64, i64, i64), ProxyError> {
        self.key_store.upstream_reconciliation_queue_counts().await
    }

    pub async fn mark_upstream_reconciliation_enqueue_error_at(
        &self,
        timestamp: i64,
    ) -> Result<(), ProxyError> {
        self.key_store
            .mark_upstream_reconciliation_enqueue_error_at(timestamp)
            .await
    }

    async fn fetch_upstream_project_usage(
        &self,
        key_id: &str,
        usage_base: &str,
        project_id: &str,
    ) -> Result<i64, (ProxyError, Option<i64>)> {
        let secret = self
            .key_store
            .fetch_api_key_secret(key_id)
            .await
            .map_err(|err| (err, None))?
            .ok_or_else(|| (ProxyError::Database(sqlx::Error::RowNotFound), None))?;
        let base = Url::parse(usage_base).map_err(|source| {
            (
                ProxyError::InvalidEndpoint {
                    endpoint: usage_base.to_string(),
                    source,
                },
                None,
            )
        })?;
        let url = build_path_prefixed_url(&base, "/usage");
        let response = self
            .send_with_forward_proxy(key_id, "period_reconciliation", |client| {
                client
                    .get(url.clone())
                    .header("Authorization", format!("Bearer {secret}"))
                    .header("X-Project-ID", project_id)
                    .timeout(Duration::from_secs(QUOTA_SYNC_FETCH_TIMEOUT_SECS))
            })
            .await
            .map(|(response, _)| response)
            .map_err(|err| (err, None))?;
        let status = response.status();
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<i64>().ok())
            .map(|seconds| self.backend_time.now_ts().saturating_add(seconds.max(1)));
        let bytes = response
            .bytes()
            .await
            .map_err(|err| (ProxyError::Http(err), retry_after))?;
        if !status.is_success() {
            return Err((
                ProxyError::UsageHttp {
                    status,
                    body: String::from_utf8_lossy(&bytes).into_owned(),
                },
                retry_after,
            ));
        }
        let json: Value = serde_json::from_slice(&bytes)
            .map_err(|err| (ProxyError::Other(format!("invalid usage json: {err}")), None))?;
        json.get("key")
            .and_then(|key| key.get("usage"))
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                (
                    ProxyError::QuotaDataMissing {
                        reason: "missing key.usage for reconciliation".to_string(),
                    },
                    None,
                )
            })
    }

    pub async fn run_upstream_reconciliation_once(
        &self,
        usage_base: &str,
    ) -> Result<i64, ProxyError> {
        let started_at = std::time::Instant::now();
        let (pending_research_before, queued_settlements_before, degraded_settlements_before) =
            self.key_store.upstream_reconciliation_queue_counts().await?;
        let settings = self.key_store.get_system_settings().await?;
        let shadow_ready = settings.upstream_project_id_mode == UpstreamProjectIdMode::AccessToken
            && settings.api_rebalance_enabled
            && settings.rebalance_mcp_enabled;
        if !shadow_ready {
            tracing::info!(
                component = "reconciliation",
                event = "run_started",
                elapsed_ms = 0_u64,
                job_type = "upstream_reconciliation",
                candidate_count = 0_i64,
                pending_research = pending_research_before,
                queued_settlements = queued_settlements_before,
                degraded_settlements = degraded_settlements_before,
            );
            self.key_store
                .mark_upstream_reconciliation_run_completed_at(self.backend_time.now_ts())
                .await?;
            tracing::info!(
                component = "reconciliation",
                event = "run_completed",
                elapsed_ms = started_at.elapsed().as_millis() as u64,
                job_type = "upstream_reconciliation",
                candidate_count = 0_i64,
                settled_count = 0_i64,
                pending_research = pending_research_before,
                queued_settlements = queued_settlements_before,
                degraded_settlements = degraded_settlements_before,
            );
            return Ok(0);
        }
        let candidate_batch = self
            .key_store
            .next_upstream_reconciliation_candidates(20)
            .await?;
        let recent_candidate_count = candidate_batch.recent_candidate_count;
        let backlog_candidate_count = candidate_batch.backlog_candidate_count;
        let recent_lane_budget = candidate_batch.recent_lane_budget;
        let backlog_lane_budget = candidate_batch.backlog_lane_budget;
        let candidates = candidate_batch.candidates;
        let candidate_count = candidates.len() as i64;
        tracing::info!(
            component = "reconciliation",
            event = "run_started",
            elapsed_ms = 0_u64,
            job_type = "upstream_reconciliation",
            candidate_count,
            recent_lane_budget,
            backlog_lane_budget,
            candidate_recent_count = recent_candidate_count,
            candidate_backlog_count = backlog_candidate_count,
            pending_research = pending_research_before,
            queued_settlements = queued_settlements_before,
            degraded_settlements = degraded_settlements_before,
        );
        let result = async {
            let mut settled = 0_i64;
            let mut settled_recent = 0_i64;
            let mut settled_backlog = 0_i64;
            let mut upstream_429_retry_windows = 0_i64;
            let mut local_usage_rate_limit_windows = 0_i64;
            let mut other_retry_windows = 0_i64;
            let mut key_backoff_window_count = 0_i64;
            let mut skipped_by_key_backoff = 0_i64;
            let mut cooling_keys = HashSet::<String>::new();
            for (index, candidate) in candidates.into_iter().enumerate() {
                let in_recent_lane = index < recent_candidate_count as usize;
                let key_ids = self
                    .key_store
                    .reconciliation_key_ids(&candidate.token_id, &candidate.period_code)
                    .await?;
                if key_ids.iter().any(|key_id| cooling_keys.contains(key_id)) {
                    skipped_by_key_backoff += 1;
                    continue;
                }
                let mut upstream_usage = 0_i64;
                let mut retry_at = None;
                let mut retry_reason = None;
                let mut retry_key_id = None;
                for key_id in key_ids {
                    match self
                        .key_store
                        .reserve_upstream_usage_attempt(&key_id)
                        .await?
                    {
                        Ok(()) => {}
                        Err(next_attempt_at) => {
                            retry_at = Some(next_attempt_at);
                            retry_reason =
                                Some(RECONCILIATION_RETRY_REASON_LOCAL_USAGE_RATE_LIMIT.to_string());
                            retry_key_id = Some(key_id.clone());
                            break;
                        }
                    }
                    match self
                        .fetch_upstream_project_usage(&key_id, usage_base, &candidate.project_id)
                        .await
                    {
                        Ok(usage) => upstream_usage = upstream_usage.saturating_add(usage),
                        Err((err, upstream_retry_at)) => {
                            retry_at = Some(
                                upstream_retry_at
                                    .unwrap_or_else(|| self.backend_time.now_ts().saturating_add(60)),
                            );
                            retry_reason = Some(err.to_string());
                            retry_key_id = Some(key_id.clone());
                            break;
                        }
                    }
                }
                match (retry_at, retry_reason, retry_key_id) {
                    (Some(next_attempt_at), Some(retry_reason), Some(retry_key_id)) => {
                        let reason_kind =
                            classify_reconciliation_retry_reason(Some(retry_reason.as_str()));
                        let changed = self
                            .key_store
                            .mark_reconciliation_key_retry(
                                &retry_key_id,
                                next_attempt_at,
                                Some(retry_reason.as_str()),
                            )
                            .await?;
                        let affected_window_count = if changed > 0 {
                            changed
                        } else {
                            self.key_store
                                .mark_reconciliation_retry(
                                    &candidate,
                                    RECONCILIATION_STATUS_RATE_LIMITED,
                                    next_attempt_at,
                                    Some(retry_reason.as_str()),
                                )
                                .await?;
                            1
                        };
                        match reason_kind {
                            RECONCILIATION_RETRY_REASON_UPSTREAM_429 => {
                                upstream_429_retry_windows += affected_window_count;
                            }
                            RECONCILIATION_RETRY_REASON_LOCAL_USAGE_RATE_LIMIT => {
                                local_usage_rate_limit_windows += affected_window_count;
                            }
                            _ => {
                                other_retry_windows += affected_window_count;
                            }
                        }
                        key_backoff_window_count += affected_window_count;
                        cooling_keys.insert(retry_key_id.clone());
                        tracing::warn!(
                            component = "reconciliation",
                            event = "key_backoff_applied",
                            elapsed_ms = started_at.elapsed().as_millis() as u64,
                            job_type = "upstream_reconciliation",
                            key_id = %retry_key_id,
                            period_code = %candidate.period_code,
                            reason_kind,
                            next_attempt_at,
                            affected_window_count,
                        );
                        continue;
                    }
                    (Some(next_attempt_at), reason, _) => {
                        self.key_store
                            .mark_reconciliation_retry(
                                &candidate,
                                RECONCILIATION_STATUS_RATE_LIMITED,
                                next_attempt_at,
                                reason.as_deref(),
                            )
                            .await?;
                        continue;
                    }
                    _ => {}
                }
                let local_billed = self
                    .key_store
                    .reconciliation_local_billed_credits(&candidate)
                    .await?;
                let did_settle = if candidate.settlement_mode == "shadow" {
                    self.key_store
                        .settle_upstream_reconciliation_shadow(
                            &candidate,
                            upstream_usage,
                            local_billed,
                        )
                        .await?
                } else {
                    self.key_store
                        .settle_upstream_reconciliation(&candidate, upstream_usage, local_billed)
                        .await?
                };
                if did_settle {
                    settled += 1;
                    if in_recent_lane {
                        settled_recent += 1;
                    } else {
                        settled_backlog += 1;
                    }
                }
            }
            Ok::<(i64, i64, i64, i64, i64, i64, i64, i64), ProxyError>((
                settled,
                settled_recent,
                settled_backlog,
                upstream_429_retry_windows,
                local_usage_rate_limit_windows,
                other_retry_windows,
                key_backoff_window_count,
                skipped_by_key_backoff,
            ))
        }
        .await;
        self.key_store
            .mark_upstream_reconciliation_run_completed_at(self.backend_time.now_ts())
            .await?;
        let (pending_research_after, queued_settlements_after, degraded_settlements_after) =
            self.key_store.upstream_reconciliation_queue_counts().await?;
        match result {
            Ok((
                settled,
                settled_recent,
                settled_backlog,
                upstream_429_retry_windows,
                local_usage_rate_limit_windows,
                other_retry_windows,
                key_backoff_window_count,
                skipped_by_key_backoff,
            )) => {
                tracing::info!(
                    component = "reconciliation",
                    event = "run_completed",
                    elapsed_ms = started_at.elapsed().as_millis() as u64,
                    job_type = "upstream_reconciliation",
                    candidate_count,
                    settled_count = settled,
                    recent_lane_budget,
                    backlog_lane_budget,
                    candidate_recent_count = recent_candidate_count,
                    candidate_backlog_count = backlog_candidate_count,
                    settled_recent_count = settled_recent,
                    settled_backlog_count = settled_backlog,
                    pending_research = pending_research_after,
                    queued_settlements = queued_settlements_after,
                    degraded_settlements = degraded_settlements_after,
                    rate_limited_429_count = upstream_429_retry_windows,
                    rate_limited_local_usage_count = local_usage_rate_limit_windows,
                    rate_limited_other_count = other_retry_windows,
                    key_backoff_window_count,
                    skipped_by_key_backoff,
                );
                Ok(settled)
            }
            Err(err) => {
                tracing::warn!(
                    component = "reconciliation",
                    event = "run_completed",
                    elapsed_ms = started_at.elapsed().as_millis() as u64,
                    job_type = "upstream_reconciliation",
                    candidate_count,
                    settled_count = 0_i64,
                    recent_lane_budget,
                    backlog_lane_budget,
                    candidate_recent_count = recent_candidate_count,
                    candidate_backlog_count = backlog_candidate_count,
                    settled_recent_count = 0_i64,
                    settled_backlog_count = 0_i64,
                    pending_research = pending_research_after,
                    queued_settlements = queued_settlements_after,
                    degraded_settlements = degraded_settlements_after,
                    rate_limited_429_count = 0_i64,
                    rate_limited_local_usage_count = 0_i64,
                    rate_limited_other_count = 0_i64,
                    key_backoff_window_count = 0_i64,
                    skipped_by_key_backoff = 0_i64,
                    err = %err,
                );
                Err(err)
            }
        }
    }

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
                Some(Duration::from_secs(QUOTA_SYNC_FETCH_TIMEOUT_SECS)),
                Some(key_id),
                None,
                "quota_sync",
            )
            .await
        {
            Ok(quota) => quota,
            Err(err) => {
                let err = normalize_quota_sync_fetch_error(err);
                self.maybe_quarantine_usage_error(key_id, "/api/tavily/usage", &err)
                    .await?;
                return Err(err);
            }
        };
        let now = self.backend_time.now_ts();
        self.key_store
            .record_quota_sync_sample(key_id, limit, remaining, now, source)
            .await?;
        self.clear_transient_backoffs_after_success(key_id, source, None)
            .await?;
        Ok((limit, remaining))
    }

    pub async fn quota_sync_api_key_secret(&self, key_id: &str) -> Result<String, ProxyError> {
        self.key_store
            .fetch_api_key_secret(key_id)
            .await?
            .ok_or_else(|| ProxyError::Database(sqlx::Error::RowNotFound))
    }

    pub async fn fetch_usage_quota_for_sync_secret(
        &self,
        secret: &str,
        usage_base: &str,
        key_id: &str,
    ) -> Result<(i64, i64), ProxyError> {
        self.fetch_usage_quota_for_secret(
            secret,
            usage_base,
            Some(Duration::from_secs(QUOTA_SYNC_FETCH_TIMEOUT_SECS)),
            Some(key_id),
            None,
            "quota_sync",
        )
        .await
        .map_err(normalize_quota_sync_fetch_error)
    }

    pub async fn record_quota_sync_usage_error(
        &self,
        key_id: &str,
        err: &ProxyError,
    ) -> Result<(), ProxyError> {
        self.maybe_quarantine_usage_error(key_id, "/api/tavily/usage", err)
            .await
    }

    pub async fn record_quota_sync_result(
        &self,
        key_id: &str,
        limit: i64,
        remaining: i64,
        source: &str,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        self.key_store
            .record_quota_sync_sample(key_id, limit, remaining, now, source)
            .await?;
        self.clear_transient_backoffs_after_success(key_id, source, None)
            .await?;
        Ok(())
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
            let created_at = self.backend_time.now_ts();
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
        let url = build_path_prefixed_url(&base, "/usage");

        let secret_header = secret.to_string();
        let request_url = url.clone();
        let (resp, _relay_lease) = match (api_key_id, proxy_affinity) {
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
                .map(|(response, relay_lease)| (response, Some(relay_lease)))?,
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
                .map(|(response, relay_lease)| (response, Some(relay_lease)))?,
            (None, None) => {
                let mut req = self
                    .client
                    .get(request_url.clone())
                    .header("Authorization", format!("Bearer {}", secret_header));
                if let Some(timeout) = timeout {
                    req = req.timeout(timeout);
                }
                (req.send().await.map_err(ProxyError::Http)?, None)
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
                    self.backend_time
                        .sleep(Duration::from_millis(backoff_ms))
                        .await;
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
                    self.backend_time
                        .sleep(Duration::from_millis(backoff_ms))
                        .await;
                }
                Err(err) => return Err(err),
            }
        }
    }

    /// Time-based garbage collection for per-token access logs.
    /// This uses a fixed retention window and never looks at token status,
    /// to avoid impacting auditability.
    pub async fn gc_auth_token_logs(&self) -> Result<i64, ProxyError> {
        let current_local_day_start = local_day_bucket_start_utc_ts(self.backend_time.now_ts());
        let retention_days = self
            .key_store
            .effective_auth_token_log_retention_days()
            .await?;
        let threshold = shift_local_day_start_utc_ts(
            current_local_day_start,
            -((retention_days - 1) as i32),
        );
        let deleted = self.key_store.delete_old_auth_token_logs(threshold).await?;
        if deleted > 0 {
            // Keep the scheduler path lightweight: reclaim WAL frames opportunistically without
            // escalating into a blocking shrink/compaction pass.
            let _checkpoint = self.key_store.checkpoint_sqlite_wal_passive().await?;
        }
        Ok(deleted)
    }

    /// Time-based garbage collection for request_logs (online recent logs only).
    /// Retention is defined by local-day boundaries and enforced via Admin settings.
    pub async fn gc_request_logs(&self) -> Result<i64, ProxyError> {
        let report = self
            .gc_request_logs_with_options(RequestLogsGcOptions {
                batch_size: 5_000,
                max_batches: i64::MAX,
                max_runtime_secs: 24 * 60 * 60,
                inter_batch_sleep_ms: 0,
            })
            .await?;
        if !report.completed {
            return Err(ProxyError::Other(format!(
                "request_logs_gc incomplete after legacy full pass: cleaned_bodies={} deleted_rows={} rollup_deleted={} batches={} retention_days={}",
                report.cleaned_request_log_bodies,
                report.deleted_request_logs,
                report.deleted_rollups,
                report.batches,
                report.retention_days
            )));
        }
        Ok(report.deleted_request_logs)
    }

    pub async fn gc_request_logs_with_options(
        &self,
        options: RequestLogsGcOptions,
    ) -> Result<RequestLogsGcReport, ProxyError> {
        let settings = self.key_store.get_system_settings().await?;
        let retention_days = settings.request_log_retention.max_log_retention_days;
        let threshold = configured_request_logs_retention_threshold_utc_ts_at(
            retention_days,
            self.backend_time.local_now(),
        );
        self.key_store
            .delete_old_request_logs_bounded(
                threshold,
                options,
                retention_days,
                &settings.request_log_retention,
            )
            .await
    }

    pub async fn gc_mcp_sessions(&self) -> Result<i64, ProxyError> {
        let now = self.backend_time.now_ts();
        self.key_store
            .delete_stale_mcp_sessions(now, now - MCP_SESSION_RETENTION_SECS)
            .await
    }

    pub async fn gc_mcp_session_init_backoffs(&self) -> Result<i64, ProxyError> {
        self.key_store
            .delete_expired_api_key_transient_backoffs(self.backend_time.now_ts())
            .await
    }

    pub async fn linuxdo_user_tag_binding_refresh_wait_secs(&self, max_age_secs: i64) -> i64 {
        match self
            .key_store
            .linuxdo_user_tag_binding_refresh_wait_secs(max_age_secs)
            .await
        {
            Ok(wait_secs) => wait_secs,
            Err(err) => {
                eprintln!("linuxdo tag binding refresh: read schedule error: {err}");
                max_age_secs.max(0)
            }
        }
    }

    pub async fn linuxdo_user_tag_binding_refresh_due(&self, max_age_secs: i64) -> bool {
        match self
            .key_store
            .linuxdo_user_tag_binding_refresh_due(max_age_secs)
            .await
        {
            Ok(due) => due,
            Err(err) => {
                eprintln!("linuxdo tag binding refresh: due check error: {err}");
                false
            }
        }
    }

    pub async fn refresh_linuxdo_user_tag_bindings(&self) -> Result<i64, ProxyError> {
        self.key_store.refresh_linuxdo_user_tag_bindings().await
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

    pub async fn scheduled_job_start_with_source(
        &self,
        job_type: &str,
        trigger_source: &str,
        key_id: Option<&str>,
        attempt: i64,
    ) -> Result<i64, ProxyError> {
        self.key_store
            .scheduled_job_start_with_source(job_type, trigger_source, key_id, attempt)
            .await
    }

    pub async fn scheduled_job_claim(
        &self,
        job_type: &str,
        trigger_source: &str,
        key_id: Option<&str>,
        attempt: i64,
    ) -> Result<Option<i64>, ProxyError> {
        self.key_store
            .scheduled_job_claim(job_type, trigger_source, key_id, attempt)
            .await
    }

    pub async fn scheduled_job_enqueue(
        &self,
        job_type: &str,
        trigger_source: &str,
        key_id: Option<&str>,
        attempt: i64,
    ) -> Result<ScheduledJobEnqueueResult, ProxyError> {
        self.key_store
            .scheduled_job_enqueue(job_type, trigger_source, key_id, attempt)
            .await
    }

    pub async fn fetch_queued_scheduled_jobs(
        &self,
        limit: usize,
    ) -> Result<Vec<QueuedScheduledJob>, ProxyError> {
        self.key_store.fetch_queued_scheduled_jobs(limit).await
    }

    pub async fn scheduled_job_mark_running(
        &self,
        job_id: i64,
    ) -> Result<Option<JobLog>, ProxyError> {
        self.key_store.scheduled_job_mark_running(job_id).await
    }

    pub async fn scheduled_job_by_id(&self, job_id: i64) -> Result<Option<JobLog>, ProxyError> {
        self.key_store.scheduled_job_by_id(job_id).await
    }

    pub async fn abandon_running_scheduled_jobs(&self) -> Result<u64, ProxyError> {
        self.key_store.abandon_running_scheduled_jobs().await
    }

    pub async fn abandon_active_scheduled_jobs(&self) -> Result<u64, ProxyError> {
        self.key_store.abandon_active_scheduled_jobs().await
    }

    pub async fn sqlite_db_stats(&self) -> Result<SqliteDbStats, ProxyError> {
        self.key_store.sqlite_db_stats().await
    }

    pub async fn compact_sqlite_database(&self) -> Result<SqliteDbStats, ProxyError> {
        self.key_store.compact_sqlite_database().await
    }

    pub fn sqlite_database_path(&self) -> &str {
        &self.key_store.database_path
    }

    pub fn sqlite_observability_database_path(&self) -> Option<&str> {
        self.key_store.observability_database_path.as_deref()
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

    pub async fn scheduled_job_update_message(
        &self,
        job_id: i64,
        message: Option<&str>,
    ) -> Result<(), ProxyError> {
        self.key_store
            .scheduled_job_update_message(job_id, message)
            .await
    }

    pub async fn list_recent_jobs(&self, limit: usize) -> Result<Vec<JobLog>, ProxyError> {
        self.key_store.list_recent_jobs(limit).await
    }

    pub async fn list_recent_job_signatures(
        &self,
        limit: usize,
    ) -> Result<Vec<(i64, String, Option<i64>)>, ProxyError> {
        self.key_store.list_recent_job_signatures(limit).await
    }

    pub async fn list_recent_jobs_paginated(
        &self,
        group: &str,
        page: usize,
        per_page: usize,
    ) -> Result<(Vec<JobLog>, i64, JobGroupCounts), ProxyError> {
        self.key_store
            .list_recent_jobs_paginated(group, page, per_page)
            .await
    }
}

fn normalize_quota_sync_fetch_error(err: ProxyError) -> ProxyError {
    match err {
        ProxyError::Http(http_err) if http_err.is_timeout() => ProxyError::Other(format!(
            "quota_sync fetch timed out after {}s",
            QUOTA_SYNC_FETCH_TIMEOUT_SECS
        )),
        other => other,
    }
}
