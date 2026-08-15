static LAST_RECONCILIATION_SUMMARY_LOG_AT: AtomicI64 = AtomicI64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReconciliationOutcome {
    Settled,
    NoAdjustment,
    Upstream429,
    TransportFailure,
    SemanticFailure,
    LocalPressure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub enum ClaimedReconciliationRunOutcome {
    Completed { settled: i64 },
    Deferred { reason: &'static str },
}

struct ReconciliationEngine;

struct ReconciliationRunResult {
    settled: i64,
    completed: i64,
    no_adjustment: i64,
    transport_failure_windows: i64,
    semantic_failure_windows: i64,
    settled_recent: i64,
    settled_backlog: i64,
    upstream_429_retry_windows: i64,
    local_usage_rate_limit_windows: i64,
    other_retry_windows: i64,
    key_backoff_window_count: i64,
    skipped_by_key_backoff: i64,
    attempted_candidate_count: i64,
    budget_exhausted: bool,
    remote_attempt_limit_reached: bool,
    max_retry_after_until: Option<i64>,
}

impl ReconciliationEngine {
    const MAX_REMOTE_ATTEMPTS: i64 = 2;
    // The scheduler consumes `Deferred` directly and persists a durable retry.
    // This compatibility one-shot API has no representative job, so it may wait
    // briefly for a transient local maintenance slice instead of reporting it as
    // a successful zero-settlement run.
    const ONE_SHOT_ADMISSION_WAIT: std::time::Duration = std::time::Duration::from_millis(250);

    fn outcome(
        settled: i64,
        no_adjustment: i64,
        upstream_429: bool,
        local_pressure: bool,
        transport_failure: bool,
        semantic_failure: bool,
    ) -> Option<ReconciliationOutcome> {
        if upstream_429 {
            Some(ReconciliationOutcome::Upstream429)
        } else if transport_failure {
            Some(ReconciliationOutcome::TransportFailure)
        } else if semantic_failure {
            Some(ReconciliationOutcome::SemanticFailure)
        } else if local_pressure {
            Some(ReconciliationOutcome::LocalPressure)
        } else if settled > no_adjustment {
            Some(ReconciliationOutcome::Settled)
        } else if no_adjustment > 0 {
            Some(ReconciliationOutcome::NoAdjustment)
        } else {
            None
        }
    }

    fn is_transport_failure(err: &ProxyError) -> bool {
        matches!(err, ProxyError::Http(_) | ProxyError::Database(_))
    }

    fn clears_local_pressure(outcome: ReconciliationOutcome) -> bool {
        matches!(
            outcome,
            ReconciliationOutcome::Settled | ReconciliationOutcome::NoAdjustment
        )
    }

    fn clears_upstream_429(outcome: ReconciliationOutcome) -> bool {
        matches!(
            outcome,
            ReconciliationOutcome::Settled | ReconciliationOutcome::NoAdjustment
        )
    }
}

impl ReconciliationOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Settled => RECONCILIATION_OUTCOME_SETTLED,
            Self::NoAdjustment => RECONCILIATION_OUTCOME_NO_ADJUSTMENT,
            Self::Upstream429 => RECONCILIATION_OUTCOME_UPSTREAM_429,
            Self::TransportFailure => RECONCILIATION_OUTCOME_TRANSPORT_FAILURE,
            Self::SemanticFailure => RECONCILIATION_OUTCOME_SEMANTIC_FAILURE,
            Self::LocalPressure => RECONCILIATION_OUTCOME_LOCAL_PRESSURE,
        }
    }
}


fn should_emit_reconciliation_summary_at(last_emitted_at: &AtomicI64, now: i64) -> bool {
    let mut previous = last_emitted_at.load(Ordering::Relaxed);
    loop {
        if previous > 0 && now.saturating_sub(previous) < 60 {
            return false;
        }
        match last_emitted_at.compare_exchange(
            previous,
            now,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return true,
            Err(observed) => previous = observed,
        }
    }
}

fn should_emit_reconciliation_summary(now: i64) -> bool {
    should_emit_reconciliation_summary_at(&LAST_RECONCILIATION_SUMMARY_LOG_AT, now)
}

async fn await_reconciliation_post_process<T>(
    deadline: std::time::Instant,
    operation: impl std::future::Future<Output = Result<T, ProxyError>>,
) -> Result<T, ProxyError> {
    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
    if remaining.is_zero() {
        return Err(ProxyError::Other(
            "reconciliation post-processing deadline exceeded".to_string(),
        ));
    }
    tokio::time::timeout(remaining, operation)
        .await
        .map_err(|_| {
            ProxyError::Other("reconciliation post-processing timed out".to_string())
        })?
}

impl TavilyProxy {
    const RECONCILIATION_BACKOFF_SCOPE: &'static str = "period_reconciliation";
    // Candidate hydration is deliberately bounded so the first main settlement
    // request cannot be displaced by the terminal-research sweep. Research is
    // allowed to use the remainder of the scheduler's 20 second budget.
    const RECONCILIATION_MAIN_PREP_BUDGET_SECS: u64 = 2;
    const RECONCILIATION_TOTAL_BUDGET_SECS: u64 = 20;
    const RECONCILIATION_FINALIZATION_HEADROOM_SECS: u64 = 1;
    const RECONCILIATION_POST_PROCESS_HEADROOM_SECS: u64 = 2;
    const RECONCILIATION_RETRY_BOOKKEEPING_HEADROOM_SECS: u64 = 2;
    const RECONCILIATION_OUTER_TIMEOUT_MARGIN_SECS: u64 = 1;
    // Research records are observability follow-up, not settlement work. Keep
    // them bounded even when the main durable projection is empty so a slow
    // remote terminal probe cannot occupy the maintenance worker's full run.
    const RECONCILIATION_RESEARCH_SWEEP_BUDGET_SECS: u64 = 2;
    const RESEARCH_SWEEP_LIMIT: usize = 20;
    const RESEARCH_SWEEP_PER_KEY_LIMIT: usize = 4;

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
        let reconciliation_observation = self
            .key_store
            .upstream_reconciliation_observation()
            .await?;
        let retry_buckets = self
            .key_store
            .upstream_reconciliation_retry_buckets()
            .await?;
        let (current_period_bound_users_by_key, current_period_pending_project_ids_by_key) = self
            .key_store
            .current_period_reconciliation_key_activity(&period.code)
            .await?;
        let (daily_reconciliation_progress, daily_reconciliation_by_key) = self
            .key_store
            .daily_reconciliation_progress()
            .await?;
        let pending_research = Some(daily_reconciliation_progress.research_pending);
        let queued_settlements = reconciliation_observation.queue_estimate;
        let (degraded_settlements, degraded_settlements_capped) = self
            .key_store
            .upstream_reconciliation_degraded_estimate()
            .await?;
        let has_degraded_settlements = self
            .key_store
            .upstream_reconciliation_degraded_exists()
            .await?;
        let (
            last_reconciliation_run_at,
            last_shadow_adjustment_at,
            last_reconciliation_enqueue_error_at,
            last_research_sweep_at,
            last_research_terminal_at,
        ) = self
            .key_store
            .upstream_reconciliation_runtime_markers()
            .await?;
        let (
            reconciliation_pressure_streak,
            reconciliation_backoff_level,
            reconciliation_backoff_until,
        ) = self
            .key_store
            .upstream_reconciliation_global_backoff_state()
            .await?;
        let (
            reconciliation_local_pressure_streak,
            reconciliation_local_backoff_level,
            reconciliation_local_backoff_until,
        ) = self
            .key_store
            .upstream_reconciliation_local_backoff_state()
            .await?;
        let reconciliation_local_last_recovered_at = self
            .key_store
            .upstream_reconciliation_local_last_recovered_at()
            .await?;
        let (
            reconciliation_last_duration_ms,
            reconciliation_last_attempted,
            reconciliation_last_settled,
            reconciliation_last_no_adjustment,
            reconciliation_last_upstream_429,
            reconciliation_last_budget_exhausted,
        ) = self
            .key_store
            .upstream_reconciliation_last_run_stats()
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
        let phase = if has_degraded_settlements {
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
            degraded_settlements_capped,
            last_reconciliation_run_at,
            last_shadow_adjustment_at,
            last_reconciliation_enqueue_error_at,
            last_research_sweep_at,
            last_research_terminal_at,
            reconciliation_pressure_streak,
            reconciliation_backoff_level,
            reconciliation_backoff_until: (reconciliation_backoff_until > now)
                .then_some(reconciliation_backoff_until),
            reconciliation_last_duration_ms,
            reconciliation_last_attempted,
            reconciliation_last_settled,
            reconciliation_last_no_adjustment,
            reconciliation_last_upstream_429,
            reconciliation_last_budget_exhausted,
            reconciliation_observation,
            reconciliation_local_backoff: ReconciliationLocalBackoff {
                pressure_streak: reconciliation_local_pressure_streak,
                level: reconciliation_local_backoff_level,
                available_at: (reconciliation_local_backoff_until > now)
                    .then_some(reconciliation_local_backoff_until),
                last_recovered_at: reconciliation_local_last_recovered_at,
            },
            retry_buckets,
            current_period_bound_users_by_key,
            current_period_pending_project_ids_by_key,
            daily_reconciliation_progress,
            daily_reconciliation_by_key,
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

    async fn fetch_upstream_research_terminal(
        &self,
        key_id: &str,
        usage_base: &str,
        request_id: &str,
    ) -> Result<bool, (ProxyError, Option<i64>)> {
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
        let path = format!("/research/{}", urlencoding::encode(request_id));
        let url = build_path_prefixed_url(&base, &path);
        let response = self
            .send_with_forward_proxy(key_id, "period_reconciliation", |client| {
                client
                    .get(url.clone())
                    .header("Authorization", format!("Bearer {secret}"))
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
        let body = response
            .bytes()
            .await
            .map_err(|err| (ProxyError::Http(err), retry_after))?;
        if !status.is_success() {
            return Err((
                ProxyError::UsageHttp {
                    status,
                    body: String::from_utf8_lossy(&body).into_owned(),
                },
                retry_after,
            ));
        }
        Ok(research_response_is_terminal(&body))
    }

    async fn reconciliation_cooldown_until(
        &self,
        key_id: &str,
        now: i64,
    ) -> Result<Option<i64>, ProxyError> {
        Ok(self
            .key_store
            .list_active_api_key_transient_backoffs(
                &[key_id.to_string()],
                Self::RECONCILIATION_BACKOFF_SCOPE,
                now,
            )
            .await?
            .get(key_id)
            .map(|state| state.cooldown_until))
    }

    async fn arm_reconciliation_backoff(
        &self,
        key_id: &str,
        requested_until: Option<i64>,
        reason: &str,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<i64, ProxyError> {
        let now = self.backend_time.now_ts();
        let prior_retry_after_secs = self
            .reconciliation_cooldown_until(key_id, now)
            .await?
            .map(|until| until.saturating_sub(now))
            .or(self
                .key_store
                .api_key_transient_backoff_state(key_id, Self::RECONCILIATION_BACKOFF_SCOPE)
                .await?
                .map(|state| state.retry_after_secs));
        let retry_after_secs = requested_until
            .map(|until| until.saturating_sub(now).max(1))
            .unwrap_or_else(|| match prior_retry_after_secs {
                None | Some(0) => 300,
                Some(1..=300) => 600,
                Some(301..=600) => 1200,
                _ => 1800,
            });
        let cooldown_until = now.saturating_add(retry_after_secs);
        let arm = ApiKeyTransientBackoffArm {
            key_id,
            scope: Self::RECONCILIATION_BACKOFF_SCOPE,
            cooldown_until,
            retry_after_secs,
            reason_code: Some(classify_reconciliation_retry_reason(Some(reason))),
            source_request_log_id: None,
            now,
        };
        let armed = match claimed_job {
            Some((job_id, claim_generation)) => {
                self.key_store
                    .arm_api_key_transient_backoff_claimed(arm, job_id, claim_generation)
                    .await?
            }
            None => self.key_store.arm_api_key_transient_backoff(arm).await?,
        };
        Ok(armed.map(|state| state.cooldown_until).unwrap_or(cooldown_until))
    }

    async fn run_research_terminal_sweep(
        &self,
        usage_base: &str,
        started_at: &std::time::Instant,
        request_start_budget_secs: u64,
        request_deadline: std::time::Instant,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<(i64, i64, i64, i64, i64, bool), ProxyError> {
        let now = self.backend_time.now_ts();
        let remaining = request_deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Ok((0, 0, 0, 0, 0, true));
        }
        let candidates = match tokio::time::timeout(
            remaining,
            self.key_store
                .next_upstream_reconciliation_research_candidates(80),
        )
        .await
        {
            Ok(candidates) => candidates?,
            Err(_) => return Ok((0, 0, 0, 0, 0, true)),
        };
        let candidate_count = candidates.len() as i64;
        tracing::debug!(
            component = "reconciliation",
            event = "research_sweep_started",
            elapsed_ms = started_at.elapsed().as_millis() as u64,
            job_type = "upstream_reconciliation",
            candidate_count,
        );
        let mut selected_per_key = HashMap::<String, usize>::new();
        let mut cooling_keys = HashSet::<String>::new();
        let mut polled = 0_i64;
        let mut terminal = 0_i64;
        let mut pending = 0_i64;
        let mut retries = 0_i64;
        let mut skipped_cooldown = 0_i64;
        let mut budget_exhausted = false;
        for candidate in candidates {
            if polled as usize >= Self::RESEARCH_SWEEP_LIMIT {
                break;
            }
            if started_at.elapsed() >= std::time::Duration::from_secs(request_start_budget_secs) {
                budget_exhausted = true;
                break;
            }
            let cooldown_remaining = request_deadline.saturating_duration_since(std::time::Instant::now());
            if cooldown_remaining.is_zero() {
                budget_exhausted = true;
                break;
            }
            let cooldown_active = tokio::time::timeout(
                cooldown_remaining,
                self.reconciliation_cooldown_until(&candidate.key_id, self.backend_time.now_ts()),
            )
            .await;
            let cooldown_active = match cooldown_active {
                Ok(result) => result?.is_some(),
                Err(_) => {
                    budget_exhausted = true;
                    break;
                }
            };
            if cooling_keys.contains(&candidate.key_id) || cooldown_active {
                skipped_cooldown += 1;
                continue;
            }
            if started_at.elapsed() >= std::time::Duration::from_secs(request_start_budget_secs) {
                budget_exhausted = true;
                break;
            }
            let selected = selected_per_key.entry(candidate.key_id.clone()).or_default();
            if *selected >= Self::RESEARCH_SWEEP_PER_KEY_LIMIT {
                continue;
            }
            *selected += 1;
            polled += 1;
            let remaining = request_deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                budget_exhausted = true;
                break;
            }
            let research_result = tokio::time::timeout(
                remaining,
                self.fetch_upstream_research_terminal(
                    &candidate.key_id,
                    usage_base,
                    &candidate.request_id,
                ),
            )
            .await;
            if let Some((job_id, claim_generation)) = claimed_job
                && !self
                    .key_store
                    .scheduled_job_claim_is_current(job_id, claim_generation)
                    .await?
            {
                return Err(ProxyError::StaleClaim {
                    job_id,
                    claim_generation,
                });
            }
            match research_result {
                Err(_) => {
                    budget_exhausted = true;
                    break;
                }
                Ok(Ok(true)) => {
                    let remaining = request_deadline.saturating_duration_since(std::time::Instant::now());
                    if remaining.is_zero() {
                        budget_exhausted = true;
                        break;
                    }
                    let marker = match claimed_job {
                        Some((job_id, claim_generation)) => {
                            tokio::time::timeout(
                                remaining,
                                self.key_store
                                    .mark_upstream_reconciliation_research_terminal_claimed(
                                        &candidate.request_id,
                                        job_id,
                                        claim_generation,
                                    ),
                            )
                            .await
                        }
                        None => {
                            tokio::time::timeout(
                                remaining,
                                self.key_store
                                    .mark_upstream_reconciliation_research_terminal(
                                        &candidate.request_id,
                                    ),
                            )
                            .await
                        }
                    };
                    match marker {
                        Ok(result) => {
                            result?;
                        }
                        Err(_) => {
                            budget_exhausted = true;
                            break;
                        }
                    }
                    terminal += 1;
                    tracing::debug!(
                        component = "reconciliation",
                        event = "research_terminal_observed",
                        elapsed_ms = started_at.elapsed().as_millis() as u64,
                        job_type = "upstream_reconciliation",
                        period_code = %candidate.period_code,
                    );
                }
                Ok(Ok(false)) => {
                    let remaining = request_deadline.saturating_duration_since(std::time::Instant::now());
                    if remaining.is_zero() {
                        budget_exhausted = true;
                        break;
                    }
                    let marker = match claimed_job {
                        Some((job_id, claim_generation)) => {
                            tokio::time::timeout(
                                remaining,
                                self.key_store
                                    .record_upstream_reconciliation_research_poll_claimed(
                                        &candidate.request_id,
                                        now.saturating_add(120),
                                        "pending",
                                        None,
                                        job_id,
                                        claim_generation,
                                    ),
                            )
                            .await
                        }
                        None => {
                            tokio::time::timeout(
                                remaining,
                                self.key_store.record_upstream_reconciliation_research_poll(
                                    &candidate.request_id,
                                    now.saturating_add(120),
                                    "pending",
                                    None,
                                ),
                            )
                            .await
                        }
                    };
                    match marker {
                        Ok(result) => {
                            result?;
                        }
                        Err(_) => {
                            budget_exhausted = true;
                            break;
                        }
                    }
                    pending += 1;
                }
                Ok(Err((err, retry_after))) => {
                    let reason = if matches!(
                        &err,
                        ProxyError::UsageHttp { status, .. }
                            if *status == reqwest::StatusCode::TOO_MANY_REQUESTS
                    ) {
                        RECONCILIATION_RETRY_REASON_UPSTREAM_429
                    } else {
                        RECONCILIATION_RETRY_REASON_OTHER
                    };
                    let next_poll_at = if reason == RECONCILIATION_RETRY_REASON_UPSTREAM_429 {
                        let remaining = request_deadline.saturating_duration_since(std::time::Instant::now());
                        if remaining.is_zero() {
                            budget_exhausted = true;
                            break;
                        }
                        let until = match tokio::time::timeout(
                            remaining,
                            self.arm_reconciliation_backoff(
                                &candidate.key_id,
                                retry_after,
                                reason,
                                claimed_job,
                            ),
                        )
                        .await
                        {
                            Ok(result) => result?,
                            Err(_) => {
                                budget_exhausted = true;
                                break;
                            }
                        };
                        cooling_keys.insert(candidate.key_id.clone());
                        tracing::debug!(
                            component = "reconciliation",
                            event = "research_key_cooldown_applied",
                            elapsed_ms = started_at.elapsed().as_millis() as u64,
                            job_type = "upstream_reconciliation",
                            key_id = %candidate.key_id,
                            reason_kind = reason,
                            cooldown_until = until,
                        );
                        until
                    } else {
                        now.saturating_add(match candidate.poll_attempt_count {
                            0..=1 => 60,
                            2..=3 => 120,
                            4..=5 => 300,
                            6..=7 => 600,
                            _ => 1800,
                        })
                    };
                    let remaining = request_deadline.saturating_duration_since(std::time::Instant::now());
                    if remaining.is_zero() {
                        budget_exhausted = true;
                        break;
                    }
                    let outcome = if reason == RECONCILIATION_RETRY_REASON_UPSTREAM_429 {
                        "rate_limited"
                    } else {
                        "retry"
                    };
                    let marker = match claimed_job {
                        Some((job_id, claim_generation)) => {
                            tokio::time::timeout(
                                remaining,
                                self.key_store
                                    .record_upstream_reconciliation_research_poll_claimed(
                                        &candidate.request_id,
                                        next_poll_at,
                                        outcome,
                                        Some(reason),
                                        job_id,
                                        claim_generation,
                                    ),
                            )
                            .await
                        }
                        None => {
                            tokio::time::timeout(
                                remaining,
                                self.key_store.record_upstream_reconciliation_research_poll(
                                    &candidate.request_id,
                                    next_poll_at,
                                    outcome,
                                    Some(reason),
                                ),
                            )
                            .await
                        }
                    };
                    match marker {
                        Ok(result) => {
                            result?;
                        }
                        Err(_) => {
                            budget_exhausted = true;
                            break;
                        }
                    }
                    retries += 1;
                }
            }
        }
        let remaining = request_deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            budget_exhausted = true;
        } else {
            if let Some((job_id, claim_generation)) = claimed_job
                && !self
                    .key_store
                    .scheduled_job_claim_is_current(job_id, claim_generation)
                    .await?
            {
                return Err(ProxyError::StaleClaim {
                    job_id,
                    claim_generation,
                });
            }
            let marker = match claimed_job {
                Some((job_id, claim_generation)) => {
                    tokio::time::timeout(
                        remaining,
                        self.key_store
                            .mark_upstream_reconciliation_research_sweep_at_claimed(
                                now,
                                job_id,
                                claim_generation,
                            ),
                    )
                    .await
                }
                None => {
                    tokio::time::timeout(
                        remaining,
                        self.key_store.mark_upstream_reconciliation_research_sweep_at(now),
                    )
                    .await
                }
            };
            match marker {
                Ok(result) => {
                    result?;
                }
                Err(_) => {
                    budget_exhausted = true;
                }
            }
        }
        tracing::debug!(
            component = "reconciliation",
            event = "research_sweep_completed",
            elapsed_ms = started_at.elapsed().as_millis() as u64,
            job_type = "upstream_reconciliation",
            candidate_count,
            polled_count = polled,
            terminal_count = terminal,
            pending_count = pending,
            retry_count = retries,
            skipped_cooldown_count = skipped_cooldown,
            budget_exhausted,
        );
        Ok((
            polled,
            terminal,
            pending,
            retries,
            skipped_cooldown,
            budget_exhausted,
        ))
    }

    pub async fn run_upstream_reconciliation_once(
        &self,
        usage_base: &str,
    ) -> Result<i64, ProxyError> {
        let deadline = std::time::Instant::now() + ReconciliationEngine::ONE_SHOT_ADMISSION_WAIT;
        loop {
            match self
                .run_upstream_reconciliation_once_inner(usage_base, None)
                .await?
            {
                ClaimedReconciliationRunOutcome::Completed { settled } => return Ok(settled),
                ClaimedReconciliationRunOutcome::Deferred { reason } => {
                    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                    if remaining.is_zero() {
                        return Err(ProxyError::Other(format!(
                            "upstream reconciliation local preparation remained deferred for {}ms: {reason}",
                            ReconciliationEngine::ONE_SHOT_ADMISSION_WAIT.as_millis(),
                        )));
                    }
                    tokio::time::sleep(remaining.min(std::time::Duration::from_millis(25))).await;
                }
            }
        }
    }

    #[doc(hidden)]
    pub async fn run_upstream_reconciliation_once_claimed(
        &self,
        usage_base: &str,
        job_id: i64,
        claim_generation: i64,
    ) -> Result<i64, ProxyError> {
        self.run_upstream_reconciliation_once_claimed_outcome(usage_base, job_id, claim_generation)
            .await
            .map(|outcome| match outcome {
                ClaimedReconciliationRunOutcome::Completed { settled } => settled,
                ClaimedReconciliationRunOutcome::Deferred { .. } => 0,
            })
    }

    #[doc(hidden)]
    pub async fn run_upstream_reconciliation_once_claimed_outcome(
        &self,
        usage_base: &str,
        job_id: i64,
        claim_generation: i64,
    ) -> Result<ClaimedReconciliationRunOutcome, ProxyError> {
        self.run_upstream_reconciliation_once_inner(
            usage_base,
            Some((job_id, claim_generation)),
        )
        .await
    }

    async fn run_upstream_reconciliation_once_inner(
        &self,
        usage_base: &str,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<ClaimedReconciliationRunOutcome, ProxyError> {
        let started_at = std::time::Instant::now();
        let local_admission = match self.admit_upstream_reconciliation_projection() {
            SqliteAdmissionOutcome::Admitted(admission) => admission,
            SqliteAdmissionOutcome::Deferred { reason } => {
                tracing::debug!(
                    component = "reconciliation",
                    event = "local_preparation_deferred",
                    defer_reason = reason,
                    "reconciliation skipped local candidate preparation before SQLite connection acquisition"
                );
                return Ok(ClaimedReconciliationRunOutcome::Deferred { reason });
            }
        };
        let run_admission_state = match self
            .key_store
            .upstream_reconciliation_run_admission_state(claimed_job)
            .await
        {
            Ok(state) => state,
            Err(err) if is_transient_sqlite_write_error(&err) => {
                return Ok(ClaimedReconciliationRunOutcome::Deferred {
                    reason: "pool_pressure",
                });
            }
            Err(err) => return Err(err),
        };
        if claimed_job.is_some() && !run_admission_state.claim_current {
            tracing::debug!(
                component = "reconciliation",
                event = "stale_claim_rejected",
                job_id = claimed_job.map(|(job_id, _)| job_id),
                claim_generation = claimed_job.map(|(_, claim_generation)| claim_generation),
            );
            return Ok(ClaimedReconciliationRunOutcome::Completed { settled: 0 });
        }
        if !run_admission_state.shadow_ready {
            tracing::debug!(
                component = "reconciliation",
                event = "run_started",
                elapsed_ms = 0_u64,
                job_type = "upstream_reconciliation",
                candidate_count = 0_i64,
            );
            self.key_store
                .mark_upstream_reconciliation_run_completed_at(self.backend_time.now_ts())
                .await?;
            tracing::debug!(
                component = "reconciliation",
                event = "run_completed",
                elapsed_ms = started_at.elapsed().as_millis() as u64,
                job_type = "upstream_reconciliation",
                candidate_count = 0_i64,
                settled_count = 0_i64,
            );
            return Ok(ClaimedReconciliationRunOutcome::Completed { settled: 0 });
        }
        let now = self.backend_time.now_ts();
        let global_backoff_level = run_admission_state.global_backoff_level;
        let global_backoff_until = run_admission_state.global_backoff_until;
        if global_backoff_until > now {
            self.key_store
                .mark_upstream_reconciliation_run_completed_at(now)
                .await?;
            tracing::debug!(
                component = "reconciliation",
                event = "global_backoff_active",
                job_type = "upstream_reconciliation",
                backoff_level = global_backoff_level,
                backoff_until = global_backoff_until,
            );
            return Ok(ClaimedReconciliationRunOutcome::Completed { settled: 0 });
        }
        let local_backoff_level = run_admission_state.local_backoff_level;
        let local_backoff_until = run_admission_state.local_backoff_until;
        if local_backoff_until > now {
            self.key_store
                .mark_upstream_reconciliation_run_completed_at(now)
                .await?;
            tracing::debug!(
                component = "reconciliation",
                event = "local_backoff_active",
                job_type = "upstream_reconciliation",
                backoff_level = local_backoff_level,
                backoff_until = local_backoff_until,
            );
            return Ok(ClaimedReconciliationRunOutcome::Completed { settled: 0 });
        }
        let preparation_deadline = started_at
            + std::time::Duration::from_secs(Self::RECONCILIATION_MAIN_PREP_BUDGET_SECS);
        // Keep the deadlines nested so the outer 20-second scheduler timeout
        // still has time for durable markers and backoff state writes.
        let remote_request_deadline = started_at
            + std::time::Duration::from_secs(
                Self::RECONCILIATION_TOTAL_BUDGET_SECS
                    .saturating_sub(Self::RECONCILIATION_FINALIZATION_HEADROOM_SECS)
                    .saturating_sub(Self::RECONCILIATION_POST_PROCESS_HEADROOM_SECS)
                    .saturating_sub(Self::RECONCILIATION_RETRY_BOOKKEEPING_HEADROOM_SECS),
            );
        let finalization_deadline = started_at
            + std::time::Duration::from_secs(
                Self::RECONCILIATION_TOTAL_BUDGET_SECS
                    .saturating_sub(Self::RECONCILIATION_POST_PROCESS_HEADROOM_SECS),
            );
        let post_process_deadline = started_at
            + std::time::Duration::from_secs(
                Self::RECONCILIATION_TOTAL_BUDGET_SECS
                    .saturating_sub(Self::RECONCILIATION_OUTER_TIMEOUT_MARGIN_SECS),
            );
        let research_start_budget_secs = Self::RECONCILIATION_TOTAL_BUDGET_SECS
            .saturating_sub(QUOTA_SYNC_FETCH_TIMEOUT_SECS)
            .saturating_sub(Self::RECONCILIATION_FINALIZATION_HEADROOM_SECS)
            .saturating_sub(Self::RECONCILIATION_POST_PROCESS_HEADROOM_SECS);
        let remote_request_start_budget_secs = research_start_budget_secs;
        let empty_candidate_batch = || UpstreamReconciliationCandidateBatch {
            candidates: Vec::new(),
            work_generation_by_candidate: std::collections::HashMap::new(),
            recent_lane_budget: 0,
            backlog_lane_budget: 0,
            recent_candidate_count: 0,
            backlog_candidate_count: 0,
        };
        let mut preparation_budget_exhausted = false;
        let candidate_remaining = preparation_deadline
            .saturating_duration_since(std::time::Instant::now());
        let candidate_batch;
        if candidate_remaining.is_zero() {
            preparation_budget_exhausted = true;
            candidate_batch = empty_candidate_batch();
        } else {
            candidate_batch = match tokio::time::timeout(
                candidate_remaining,
                self.key_store.next_upstream_reconciliation_candidates(20),
            )
            .await
            {
                Ok(batch) => batch?,
                Err(_) => {
                    preparation_budget_exhausted = true;
                    empty_candidate_batch()
                }
            };
        }
        preparation_budget_exhausted |=
            std::time::Instant::now() >= preparation_deadline;
        if candidate_batch.candidates.is_empty() && !preparation_budget_exhausted {
            // The projection is a compatibility bootstrap for usage written
            // before the durable work triggers existed. It must never sit in
            // front of already-projectable work: otherwise its aggregate write
            // can consume the entire main-settlement preparation budget. A
            // projected page is settled by the next representative run so this
            // run remains a bounded local maintenance slice.
            let bootstrap_budget = preparation_deadline
                .saturating_duration_since(std::time::Instant::now())
                .min(Duration::from_millis(250));
            if bootstrap_budget.is_zero() {
                preparation_budget_exhausted = true;
            } else {
                match tokio::time::timeout(
                    bootstrap_budget,
                    self.key_store.advance_upstream_reconciliation_work_projection(),
                )
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) if crate::store::is_transient_sqlite_write_error(&err) => {
                        preparation_budget_exhausted = true;
                    }
                    Ok(Err(err)) => return Err(err),
                    Err(_) => preparation_budget_exhausted = true,
                }
            }
        }
        let candidate_hydration_deadline = preparation_deadline;
        let recent_candidate_count = candidate_batch.recent_candidate_count;
        let backlog_candidate_count = candidate_batch.backlog_candidate_count;
        let recent_lane_budget = candidate_batch.recent_lane_budget;
        let backlog_lane_budget = candidate_batch.backlog_lane_budget;
        let work_generation_by_candidate = candidate_batch.work_generation_by_candidate;
        let candidates = candidate_batch.candidates;
        let candidate_count = candidates.len() as i64;
        let candidate_keys = candidates
            .iter()
            .map(|candidate| (candidate.token_id.clone(), candidate.period_code.clone()))
            .collect::<Vec<_>>();
        let key_ids_by_candidate = if preparation_budget_exhausted {
            std::collections::HashMap::new()
        } else {
            let remaining = candidate_hydration_deadline
                .saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                preparation_budget_exhausted = true;
                std::collections::HashMap::new()
            } else {
                match tokio::time::timeout(
                    remaining,
                    self.key_store.reconciliation_key_ids_batch(&candidate_keys),
                )
                .await
                {
                    Ok(result) => result?,
                    Err(_) => {
                        preparation_budget_exhausted = true;
                        std::collections::HashMap::new()
                    }
                }
            }
        };
        let all_key_ids = key_ids_by_candidate
            .values()
            .flat_map(|key_ids| key_ids.iter().cloned())
            .collect::<std::collections::HashSet<_>>();
        let active_key_cooldowns = if preparation_budget_exhausted {
            std::collections::HashMap::new()
        } else {
            let remaining = candidate_hydration_deadline
                .saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                preparation_budget_exhausted = true;
                std::collections::HashMap::new()
            } else {
                match tokio::time::timeout(
                    remaining,
                    self.key_store.list_active_api_key_transient_backoffs(
                        &all_key_ids.into_iter().collect::<Vec<_>>(),
                        Self::RECONCILIATION_BACKOFF_SCOPE,
                        self.backend_time.now_ts(),
                    ),
                )
                .await
                {
                    Ok(result) => result?,
                    Err(_) => {
                        preparation_budget_exhausted = true;
                        std::collections::HashMap::new()
                    }
                }
            }
        };
        preparation_budget_exhausted |= std::time::Instant::now() >= candidate_hydration_deadline;
        tracing::debug!(
            component = "reconciliation",
            event = "run_started",
            elapsed_ms = 0_u64,
            job_type = "upstream_reconciliation",
            candidate_count,
            recent_lane_budget,
            backlog_lane_budget,
            candidate_recent_count = recent_candidate_count,
            candidate_backlog_count = backlog_candidate_count,
            research_polled_count = 0_i64,
            research_terminal_count = 0_i64,
            research_pending_count = 0_i64,
            research_retry_count = 0_i64,
            research_skipped_cooldown_count = 0_i64,
        );
        // Remote I/O and durable settlement must never retain the local bulk
        // permit. The permit protects only candidate selection, hydration and
        // the optional legacy projection above.
        drop(local_admission);
        let result = async {
            let mut settled = 0_i64;
            let mut completed = 0_i64;
            let mut no_adjustment = 0_i64;
            let mut transport_failure_windows = 0_i64;
            let mut semantic_failure_windows = 0_i64;
            let mut settled_recent = 0_i64;
            let mut settled_backlog = 0_i64;
            let mut upstream_429_retry_windows = 0_i64;
            let mut local_usage_rate_limit_windows = 0_i64;
            let mut other_retry_windows = 0_i64;
            let mut key_backoff_window_count = 0_i64;
            let mut skipped_by_key_backoff = 0_i64;
            let mut attempted_candidate_count = 0_i64;
            let mut remote_request_count = 0_i64;
            let mut budget_exhausted = preparation_budget_exhausted;
            let mut max_retry_after_until = None::<i64>;
            let mut cooling_keys = HashSet::<String>::new();
            let mut remote_request_started = false;
            let mut remote_attempt_limit_reached = false;
            let mut observed_candidates = Vec::<(
                UpstreamReconciliationCandidate,
                i64,
                bool,
                i64,
            )>::new();
            'candidates: for (index, candidate) in candidates.iter().cloned().enumerate() {
                if budget_exhausted {
                    break;
                }
                if started_at.elapsed()
                    >= std::time::Duration::from_secs(remote_request_start_budget_secs)
                {
                    budget_exhausted = true;
                    break;
                }
                let in_recent_lane = index < recent_candidate_count as usize;
                let work_generation = work_generation_by_candidate
                    .get(&(candidate.token_id.clone(), candidate.period_code.clone()))
                    .copied()
                    .ok_or_else(|| {
                        ProxyError::Other(
                            "missing reconciliation work generation for candidate".to_string(),
                        )
                    })?;
                let key_ids = key_ids_by_candidate
                    .get(&(candidate.token_id.clone(), candidate.period_code.clone()))
                    .cloned()
                    .unwrap_or_default();
                if key_ids.iter().any(|key_id| cooling_keys.contains(key_id)) {
                    skipped_by_key_backoff += 1;
                    continue;
                }
                let mut key_in_cooldown = false;
                for key_id in &key_ids {
                    if active_key_cooldowns.contains_key(key_id) {
                        cooling_keys.insert(key_id.clone());
                        key_in_cooldown = true;
                        break;
                    }
                }
                if key_in_cooldown {
                    skipped_by_key_backoff += 1;
                    continue;
                }
                let mut upstream_usage = 0_i64;
                let mut retry_at = None;
                let mut retry_reason = None;
                let mut retry_key_id = None;
                let mut retry_outcome = None;
                let mut candidate_attempted = false;
                let key_count = key_ids.len();
                let mut successful_key_count = 0_usize;
                if key_count == 0 {
                    continue;
                }
                for key_id in key_ids {
                    if remote_request_count >= ReconciliationEngine::MAX_REMOTE_ATTEMPTS {
                        remote_attempt_limit_reached = true;
                        budget_exhausted = true;
                        break 'candidates;
                    }
                    // The two-second limit applies only to local candidate
                    // preparation. Once a remote request has started, allow
                    // the bounded request timeout to finish and use the
                    // separate total-budget deadline for subsequent requests.
                    let reservation_deadline = if candidate_attempted || remote_request_started {
                        remote_request_deadline
                    } else {
                        preparation_deadline
                    };
                    let remote_remaining = remote_request_deadline
                        .saturating_duration_since(std::time::Instant::now());
                    let request_timeout = Duration::from_secs(QUOTA_SYNC_FETCH_TIMEOUT_SECS);
                    if remote_remaining < request_timeout {
                        budget_exhausted = true;
                        break;
                    }
                    let reservation_budget = reservation_deadline
                        .saturating_duration_since(std::time::Instant::now())
                        .min(remote_remaining.saturating_sub(request_timeout));
                    if reservation_budget.is_zero() {
                        budget_exhausted = true;
                        break;
                    }
                    let reservation_result = tokio::time::timeout(
                        reservation_budget,
                        self.key_store.reserve_upstream_usage_attempt(&key_id),
                    )
                    .await;
                    let reservation = match reservation_result {
                        Ok(result) => result?,
                        Err(_) => {
                            budget_exhausted = true;
                            break;
                        }
                    };
                    match reservation {
                        Ok(()) => {}
                        Err(next_attempt_at) => {
                            retry_at = Some(next_attempt_at);
                            retry_reason =
                                Some(RECONCILIATION_RETRY_REASON_LOCAL_USAGE_RATE_LIMIT.to_string());
                            retry_key_id = Some(key_id.clone());
                            retry_outcome = Some(ReconciliationOutcome::LocalPressure);
                            break;
                        }
                    }
                    if !candidate_attempted {
                        attempted_candidate_count += 1;
                        candidate_attempted = true;
                    }
                    let remaining = remote_request_deadline
                        .saturating_duration_since(std::time::Instant::now());
                    remote_request_started = true;
                    remote_request_count += 1;
                    let usage_result = tokio::time::timeout(
                        remaining.max(Duration::from_millis(1)),
                        self.fetch_upstream_project_usage(
                            &key_id,
                            usage_base,
                            &candidate.project_id,
                        ),
                    )
                    .await;
                    match usage_result {
                        Err(_) => {
                            transport_failure_windows += 1;
                            retry_reason = Some("upstream transport timeout".to_string());
                            retry_key_id = Some(key_id.clone());
                            retry_outcome = Some(ReconciliationOutcome::TransportFailure);
                            break;
                        }
                        Ok(Ok(usage)) => {
                            upstream_usage = upstream_usage.saturating_add(usage);
                            successful_key_count += 1;
                        }
                        Ok(Err((err, upstream_retry_at))) => {
                            let outcome = if matches!(
                                &err,
                                ProxyError::UsageHttp { status, .. }
                                    if *status == reqwest::StatusCode::TOO_MANY_REQUESTS
                            ) {
                                ReconciliationOutcome::Upstream429
                            } else if ReconciliationEngine::is_transport_failure(&err) {
                                transport_failure_windows += 1;
                                ReconciliationOutcome::TransportFailure
                            } else if !matches!(&err, ProxyError::UsageHttp { status, .. } if *status == reqwest::StatusCode::TOO_MANY_REQUESTS) {
                                semantic_failure_windows += 1;
                                ReconciliationOutcome::SemanticFailure
                            } else {
                                ReconciliationOutcome::SemanticFailure
                            };
                            retry_at = upstream_retry_at;
                            retry_reason = Some(err.to_string());
                            retry_key_id = Some(key_id.clone());
                            retry_outcome = Some(outcome);
                            break;
                        }
                    }
                }
                if budget_exhausted {
                    break;
                }
                    if let (Some(_retry_reason), Some(retry_key_id)) = (retry_reason, retry_key_id) {
                        if let Some((job_id, claim_generation)) = claimed_job
                            && !self
                                .key_store
                                .scheduled_job_claim_is_current(job_id, claim_generation)
                                .await?
                        {
                            return Err(ProxyError::StaleClaim {
                                job_id,
                                claim_generation,
                            });
                        }
                        let retry_outcome = retry_outcome.unwrap_or(ReconciliationOutcome::SemanticFailure);
                        let reason_kind = match retry_outcome {
                            ReconciliationOutcome::Upstream429 => {
                                RECONCILIATION_RETRY_REASON_UPSTREAM_429
                            }
                            ReconciliationOutcome::LocalPressure => {
                                RECONCILIATION_RETRY_REASON_LOCAL_USAGE_RATE_LIMIT
                            }
                            _ => RECONCILIATION_RETRY_REASON_OTHER,
                        };
                        if reason_kind == RECONCILIATION_RETRY_REASON_UPSTREAM_429
                            && let Some(retry_after_until) = retry_at
                        {
                            max_retry_after_until = Some(
                                max_retry_after_until
                                    .unwrap_or_default()
                                    .max(retry_after_until),
                            );
                        }
                        let retry_bookkeeping_deadline = started_at
                            + std::time::Duration::from_secs(
                                Self::RECONCILIATION_TOTAL_BUDGET_SECS
                                    .saturating_sub(Self::RECONCILIATION_POST_PROCESS_HEADROOM_SECS),
                            );
                        let remaining = retry_bookkeeping_deadline
                            .saturating_duration_since(std::time::Instant::now());
                        if remaining.is_zero() {
                            return Err(ProxyError::Other(
                                "reconciliation retry bookkeeping deadline exceeded".to_string(),
                            ));
                        }
                        let cooldown_until = if reason_kind
                            == RECONCILIATION_RETRY_REASON_UPSTREAM_429
                        {
                            tokio::time::timeout(
                                remaining,
                                self.arm_reconciliation_backoff(
                                    &retry_key_id,
                                    retry_at,
                                    reason_kind,
                                    claimed_job,
                                ),
                            )
                            .await
                            .map_err(|_| {
                                ProxyError::Other(
                                    "reconciliation retry backoff persistence timed out".to_string(),
                                )
                            })??
                        } else {
                            retry_at.unwrap_or_else(|| self.backend_time.now_ts().saturating_add(300))
                        };
                        let remaining = retry_bookkeeping_deadline
                            .saturating_duration_since(std::time::Instant::now());
                        if remaining.is_zero() {
                            return Err(ProxyError::Other(
                                "reconciliation retry bookkeeping deadline exceeded".to_string(),
                            ));
                        }
                        tokio::time::timeout(
                            remaining,
                            self.key_store.mark_reconciliation_retry(
                                &candidate,
                                if reason_kind == RECONCILIATION_RETRY_REASON_UPSTREAM_429 {
                                    RECONCILIATION_STATUS_RATE_LIMITED
                                } else {
                                    "waiting"
                                },
                                cooldown_until,
                                Some(reason_kind),
                                retry_outcome.as_str(),
                                Some(ReconciliationWorkFence {
                                    work_generation,
                                    claimed_job,
                                }),
                            ),
                        )
                        .await
                        .map_err(|_| {
                            ProxyError::Other(
                                "reconciliation retry marker persistence timed out".to_string(),
                            )
                        })??;
                        let affected_window_count = 1_i64;
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
                        if reason_kind == RECONCILIATION_RETRY_REASON_UPSTREAM_429 {
                            key_backoff_window_count += affected_window_count;
                            cooling_keys.insert(retry_key_id.clone());
                        }
                        tracing::debug!(
                            component = "reconciliation",
                            event = "key_backoff_applied",
                            elapsed_ms = started_at.elapsed().as_millis() as u64,
                            job_type = "upstream_reconciliation",
                            key_id = %retry_key_id,
                            period_code = %candidate.period_code,
                            reason_kind,
                            cooldown_until,
                            affected_window_count,
                        );
                        continue;
                }
                if successful_key_count != key_count {
                    budget_exhausted = true;
                    break;
                }
                observed_candidates.push((candidate, upstream_usage, in_recent_lane, work_generation));
            }

            // Billing can become charged while the remote request is in flight. Re-read
            // all observed candidates in one batch immediately before settlement so the
            // persisted adjustment uses the post-observation ledger state without
            // reintroducing a per-candidate hydration query.
            if !observed_candidates.is_empty() {
                let observed = observed_candidates
                    .iter()
                    .map(|(candidate, _, _, _)| candidate.clone())
                    .collect::<Vec<_>>();
                let remaining = finalization_deadline
                    .saturating_duration_since(std::time::Instant::now());
                let fresh_local_billed_by_candidate = if remaining.is_zero() {
                    budget_exhausted = true;
                    None
                } else {
                    match tokio::time::timeout(
                        finalization_deadline
                            .saturating_duration_since(std::time::Instant::now()),
                        self.key_store.reconciliation_local_billed_credits_batch(&observed),
                    )
                    .await
                    {
                        Ok(result) => Some(result?),
                        Err(_) => {
                            budget_exhausted = true;
                            None
                        }
                    }
                };
                // A later remote request may exhaust the main budget after an earlier
                // request has already succeeded. Do not let that later timeout discard
                // observations that still fit the bounded finalization window.
                if let Some(fresh_local_billed_by_candidate) = fresh_local_billed_by_candidate {
                    for (candidate, upstream_usage, in_recent_lane, work_generation) in observed_candidates {
                        let remaining = finalization_deadline
                            .saturating_duration_since(std::time::Instant::now());
                        if remaining.is_zero() {
                            budget_exhausted = true;
                            break;
                        }
                        let local_billed = fresh_local_billed_by_candidate
                            .get(&(candidate.token_id.clone(), candidate.period_code.clone()))
                            .copied()
                            .unwrap_or(0);
                        let did_settle = match tokio::time::timeout(remaining, async {
                            if candidate.settlement_mode == "shadow" {
                                self.key_store
                                    .settle_upstream_reconciliation_shadow(
                                        &candidate,
                                        upstream_usage,
                                        local_billed,
                                        Some(ReconciliationWorkFence {
                                            work_generation,
                                            claimed_job,
                                        }),
                                    )
                                    .await
                            } else {
                                self.key_store
                                    .settle_upstream_reconciliation(
                                        &candidate,
                                        upstream_usage,
                                        local_billed,
                                        Some(ReconciliationWorkFence {
                                            work_generation,
                                            claimed_job,
                                        }),
                                    )
                                    .await
                            }
                        })
                        .await
                        {
                            Ok(result) => result?,
                            Err(_) => {
                                budget_exhausted = true;
                                break;
                            }
                        };
                        if did_settle {
                            completed += 1;
                            if upstream_usage == local_billed {
                                no_adjustment += 1;
                            }
                            settled += 1;
                            if in_recent_lane {
                                settled_recent += 1;
                            } else {
                                settled_backlog += 1;
                            }
                        }
                    }
                }
            }
            Ok::<ReconciliationRunResult, ProxyError>(ReconciliationRunResult {
                settled,
                completed,
                no_adjustment,
                transport_failure_windows,
                semantic_failure_windows,
                settled_recent,
                settled_backlog,
                upstream_429_retry_windows,
                local_usage_rate_limit_windows,
                other_retry_windows,
                key_backoff_window_count,
                skipped_by_key_backoff,
                attempted_candidate_count,
                budget_exhausted,
                remote_attempt_limit_reached,
                max_retry_after_until,
            })
        }
        .await;
        if let Some((job_id, claim_generation)) = claimed_job
            && !self
                .key_store
                .scheduled_job_claim_is_current(job_id, claim_generation)
                .await?
        {
            tracing::debug!(
                component = "reconciliation",
                event = "stale_claim_rejected",
                job_id,
                claim_generation,
            );
            return Ok(ClaimedReconciliationRunOutcome::Completed { settled: 0 });
        }
        let main_budget_exhausted = result
            .as_ref()
            .map(|value| value.budget_exhausted)
            .unwrap_or(true);
        let (
            research_polled_count,
            research_terminal_count,
            research_pending_count,
            research_retry_count,
            research_skipped_cooldown_count,
            research_budget_exhausted,
        ) = if result.is_ok() {
            let research_deadline = remote_request_deadline.min(
                std::time::Instant::now()
                    + std::time::Duration::from_secs(Self::RECONCILIATION_RESEARCH_SWEEP_BUDGET_SECS),
            );
            match self
                .run_research_terminal_sweep(
                    usage_base,
                    &started_at,
                    research_start_budget_secs,
                    research_deadline,
                    claimed_job,
                )
                .await
            {
                Ok(research) => research,
                Err(ProxyError::StaleClaim {
                    job_id,
                    claim_generation,
                }) => {
                    tracing::debug!(
                        component = "reconciliation",
                        event = "stale_claim_rejected",
                        job_id,
                        claim_generation,
                    );
                    return Ok(ClaimedReconciliationRunOutcome::Completed { settled: 0 });
                }
                Err(err) => return Err(err),
            }
        } else {
            (0, 0, 0, 0, 0, main_budget_exhausted)
        };
        await_reconciliation_post_process(
            post_process_deadline,
            self.key_store
                .mark_upstream_reconciliation_run_completed_at(self.backend_time.now_ts()),
        )
        .await?;
        match result {
            Ok(ReconciliationRunResult {
                settled,
                completed,
                no_adjustment,
                transport_failure_windows,
                semantic_failure_windows,
                settled_recent,
                settled_backlog,
                upstream_429_retry_windows,
                local_usage_rate_limit_windows,
                other_retry_windows,
                key_backoff_window_count,
                skipped_by_key_backoff,
                attempted_candidate_count,
                mut budget_exhausted,
                remote_attempt_limit_reached,
                max_retry_after_until,
            }) => {
                // The cap stops partial settlement, but it is not a time or local
                // preparation budget exhaustion in the persisted observation.
                // The research sweep has its own small post-settlement budget;
                // exhausting it must not pretend that primary work was starved.
                budget_exhausted &= !remote_attempt_limit_reached;
                tracing::debug!(
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
                    rate_limited_429_count = upstream_429_retry_windows,
                    rate_limited_local_usage_count = local_usage_rate_limit_windows,
                    rate_limited_other_count = other_retry_windows,
                    key_backoff_window_count,
                    skipped_by_key_backoff,
                    research_polled_count,
                    research_terminal_count,
                    research_pending_count,
                    research_retry_count,
                    research_skipped_cooldown_count,
                    research_budget_exhausted,
                    budget_exhausted,
                );
                let (
                    _previous_duration_ms,
                    _previous_attempted,
                    _previous_settled,
                    _previous_no_adjustment,
                    _previous_upstream_429,
                    previous_budget_exhausted,
                ) = await_reconciliation_post_process(
                    post_process_deadline,
                    self.key_store.upstream_reconciliation_last_run_stats(),
                )
                .await?;
                await_reconciliation_post_process(
                    post_process_deadline,
                    self.key_store.record_upstream_reconciliation_run_stats(
                        started_at.elapsed().as_millis().min(i64::MAX as u128) as i64,
                        attempted_candidate_count,
                        settled,
                        no_adjustment,
                        upstream_429_retry_windows,
                        budget_exhausted,
                    ),
                )
                .await?;
                let summary_now = self.backend_time.now_ts();
                if budget_exhausted && !previous_budget_exhausted {
                    tracing::warn!(
                        component = "reconciliation",
                        event = "budget_exhausted",
                        job_type = "upstream_reconciliation",
                        elapsed_ms = started_at.elapsed().as_millis() as u64,
                        candidate_count,
                        attempted_candidate_count,
                    );
                } else if !budget_exhausted && previous_budget_exhausted {
                    tracing::warn!(
                        component = "reconciliation",
                        event = "budget_recovered",
                        job_type = "upstream_reconciliation",
                        elapsed_ms = started_at.elapsed().as_millis() as u64,
                    );
                }
                if should_emit_reconciliation_summary(summary_now) {
                    tracing::info!(
                        component = "reconciliation",
                        event = "run_summary",
                        job_type = "upstream_reconciliation",
                        elapsed_ms = started_at.elapsed().as_millis() as u64,
                        candidate_count,
                        attempted_candidate_count,
                        settled_count = settled,
                        completed_count = completed,
                        no_adjustment_count = no_adjustment,
                        rate_limited_429_count = upstream_429_retry_windows,
                        budget_exhausted,
                    );
                }
                let upstream_429_observed = upstream_429_retry_windows > 0;
                let qualified_remote_pressure = upstream_429_observed
                    && completed == 0
                    && upstream_429_retry_windows.saturating_mul(2)
                        >= attempted_candidate_count.max(1);
                let local_pressure = local_usage_rate_limit_windows > 0
                    || ((candidate_count > 0 || preparation_budget_exhausted)
                        && attempted_candidate_count == 0
                        && budget_exhausted);
                let reconciliation_outcome = ReconciliationEngine::outcome(
                    completed,
                    no_adjustment,
                    upstream_429_observed,
                    local_pressure,
                    transport_failure_windows > 0,
                    semantic_failure_windows > 0,
                );
                let (_, previous_local_backoff_level, _) = await_reconciliation_post_process(
                    post_process_deadline,
                    self.key_store.upstream_reconciliation_local_backoff_state(),
                )
                .await?;
                let (local_pressure_streak, local_backoff_level, local_backoff_until) =
                    if local_pressure {
                        let now = self.backend_time.now_ts();
                        match claimed_job {
                            Some((job_id, claim_generation)) => {
                                await_reconciliation_post_process(
                                    post_process_deadline,
                                    self.key_store
                                        .update_upstream_reconciliation_local_backoff_claimed(
                                            true,
                                            now,
                                            job_id,
                                            claim_generation,
                                        ),
                                )
                                .await?
                            }
                            None => {
                                await_reconciliation_post_process(
                                    post_process_deadline,
                                    self.key_store
                                        .update_upstream_reconciliation_local_backoff(true, now),
                                )
                                .await?
                            }
                        }
                    } else if reconciliation_outcome
                        .is_some_and(ReconciliationEngine::clears_local_pressure)
                    {
                        let now = self.backend_time.now_ts();
                        match claimed_job {
                            Some((job_id, claim_generation)) => {
                                await_reconciliation_post_process(
                                    post_process_deadline,
                                    self.key_store
                                        .update_upstream_reconciliation_local_backoff_claimed(
                                            false,
                                            now,
                                            job_id,
                                            claim_generation,
                                        ),
                                )
                                .await?
                            }
                            None => {
                                await_reconciliation_post_process(
                                    post_process_deadline,
                                    self.key_store.update_upstream_reconciliation_local_backoff(
                                        false, now,
                                    ),
                                )
                                .await?
                            }
                        }
                    } else {
                        self.key_store.upstream_reconciliation_local_backoff_state().await?
                    };
                if local_backoff_level > previous_local_backoff_level {
                    tracing::warn!(
                        component = "reconciliation",
                        event = "local_backoff_applied",
                        job_type = "upstream_reconciliation",
                        pressure_streak = local_pressure_streak,
                        backoff_level = local_backoff_level,
                        backoff_until = local_backoff_until,
                        candidate_count,
                        budget_exhausted,
                    );
                } else if previous_local_backoff_level > 0 && local_backoff_level == 0 {
                    tracing::warn!(
                        component = "reconciliation",
                        event = "local_backoff_recovered",
                        job_type = "upstream_reconciliation",
                    );
                }
                let (_, previous_backoff_level, _) = await_reconciliation_post_process(
                    post_process_deadline,
                    self.key_store.upstream_reconciliation_global_backoff_state(),
                )
                .await?;
                let (pressure_streak, backoff_level, backoff_until) = if qualified_remote_pressure {
                    let now = self.backend_time.now_ts();
                    match claimed_job {
                        Some((job_id, claim_generation)) => {
                            await_reconciliation_post_process(
                                post_process_deadline,
                                self.key_store
                                    .update_upstream_reconciliation_global_backoff_claimed(
                                        true,
                                        now,
                                        max_retry_after_until,
                                        job_id,
                                        claim_generation,
                                    ),
                            )
                            .await?
                        }
                        None => {
                            await_reconciliation_post_process(
                                post_process_deadline,
                                self.key_store.update_upstream_reconciliation_global_backoff(
                                    true,
                                    now,
                                    max_retry_after_until,
                                ),
                            )
                            .await?
                        }
                    }
                } else if reconciliation_outcome
                    .is_some_and(ReconciliationEngine::clears_upstream_429)
                {
                    let now = self.backend_time.now_ts();
                    match claimed_job {
                        Some((job_id, claim_generation)) => {
                            await_reconciliation_post_process(
                                post_process_deadline,
                                self.key_store
                                    .update_upstream_reconciliation_global_backoff_claimed(
                                        false,
                                        now,
                                        None,
                                        job_id,
                                        claim_generation,
                                    ),
                            )
                            .await?
                        }
                        None => {
                            await_reconciliation_post_process(
                                post_process_deadline,
                                self.key_store.update_upstream_reconciliation_global_backoff(
                                    false, now, None,
                                ),
                            )
                            .await?
                        }
                    }
                } else {
                    self.key_store.upstream_reconciliation_global_backoff_state().await?
                };
                if backoff_level > previous_backoff_level {
                    tracing::warn!(
                        component = "reconciliation",
                        event = "global_backoff_applied",
                        job_type = "upstream_reconciliation",
                        pressure_streak,
                        backoff_level,
                        backoff_until,
                        rate_limited_429_count = upstream_429_retry_windows,
                        candidate_count,
                        attempted_candidate_count,
                    );
                } else if previous_backoff_level > 0 && backoff_level == 0 {
                    tracing::warn!(
                        component = "reconciliation",
                        event = "global_backoff_recovered",
                        job_type = "upstream_reconciliation",
                        previous_backoff_level,
                    );
                }
                Ok(ClaimedReconciliationRunOutcome::Completed { settled })
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
                    rate_limited_429_count = 0_i64,
                    rate_limited_local_usage_count = 0_i64,
                    rate_limited_other_count = 0_i64,
                    key_backoff_window_count = 0_i64,
                    skipped_by_key_backoff = 0_i64,
                    research_polled_count,
                    research_terminal_count,
                    research_pending_count,
                    research_retry_count,
                    research_skipped_cooldown_count,
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
                    tracing::debug!(
                        component = "usage_rollup",
                        event = "sqlite_retry",
                        operation = "token_usage_rollup",
                        attempt = retry_idx,
                        backoff_ms,
                        err = %err,
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
                    tracing::debug!(
                        component = "usage_rollup",
                        event = "sqlite_retry",
                        operation = "token_usage_rebuild",
                        attempt = retry_idx,
                        backoff_ms,
                        err = %err,
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
                tracing::debug!(
                    component = "linuxdo_user_tags",
                    event = "refresh_schedule_read_failed",
                    err = %err,
                );
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
                tracing::debug!(
                    component = "linuxdo_user_tags",
                    event = "refresh_due_check_failed",
                    err = %err,
                );
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

    pub async fn scheduled_job_enqueue_foreground(
        &self,
        job_type: &str,
        trigger_source: &str,
        key_id: Option<&str>,
        attempt: i64,
    ) -> Result<ScheduledJobEnqueueResult, ProxyError> {
        self.key_store
            .scheduled_job_enqueue_foreground(job_type, trigger_source, key_id, attempt)
            .await
    }

    pub async fn scheduled_job_enqueue_at(
        &self,
        job_type: &str,
        trigger_source: &str,
        key_id: Option<&str>,
        attempt: i64,
        available_at: i64,
    ) -> Result<ScheduledJobEnqueueResult, ProxyError> {
        self.key_store
            .scheduled_job_enqueue_at(job_type, trigger_source, key_id, attempt, available_at)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn scheduled_job_finish_and_enqueue_auto_at(
        &self,
        job_id: i64,
        claim_generation: i64,
        job_type: &str,
        key_id: Option<&str>,
        attempt: i64,
        message: Option<&str>,
        available_at: i64,
    ) -> Result<ScheduledJobEnqueueResult, ProxyError> {
        self.key_store
            .scheduled_job_finish_and_enqueue_auto_at(
                job_id,
                claim_generation,
                job_type,
                key_id,
                attempt,
                message,
                available_at,
            )
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn scheduled_job_finish_and_enqueue_auto_at_with_status(
        &self,
        job_id: i64,
        claim_generation: i64,
        status: &str,
        job_type: &str,
        key_id: Option<&str>,
        attempt: i64,
        message: Option<&str>,
        available_at: i64,
    ) -> Result<ScheduledJobEnqueueResult, ProxyError> {
        self.key_store
            .scheduled_job_finish_and_enqueue_auto_at_with_status(
                job_id,
                claim_generation,
                status,
                job_type,
                key_id,
                attempt,
                message,
                available_at,
            )
            .await
    }

    pub async fn scheduled_job_finish_claimed(
        &self,
        job_id: i64,
        claim_generation: i64,
        status: &str,
        message: Option<&str>,
    ) -> Result<(), ProxyError> {
        self.key_store
            .scheduled_job_finish_claimed(job_id, claim_generation, status, message)
            .await
    }

    pub async fn ensure_auth_token_logs_alert_time_index(&self) -> Result<(), ProxyError> {
        self.key_store
            .ensure_auth_token_logs_alert_time_index()
            .await
    }

    pub async fn upstream_reconciliation_backoff_until(&self) -> Result<i64, ProxyError> {
        let (_, _, global_backoff_until) = self
            .key_store
            .upstream_reconciliation_global_backoff_state()
            .await?;
        let (_, _, local_backoff_until) = self
            .key_store
            .upstream_reconciliation_local_backoff_state()
            .await?;
        Ok(global_backoff_until.max(local_backoff_until))
    }

    pub async fn upstream_reconciliation_continuation_at(
        &self,
    ) -> Result<Option<i64>, ProxyError> {
        self.key_store.upstream_reconciliation_continuation_at().await
    }

    pub async fn upstream_reconciliation_representative_available_at(
        &self,
    ) -> Result<Option<i64>, ProxyError> {
        self.key_store
            .upstream_reconciliation_representative_available_at()
            .await
    }

    pub async fn ensure_upstream_reconciliation_representative_job(
        &self,
    ) -> Result<(), ProxyError> {
        self.key_store
            .ensure_upstream_reconciliation_representative_job()
            .await
    }

    pub async fn record_upstream_reconciliation_budget_exhausted(
        &self,
        duration_ms: i64,
    ) -> Result<(), ProxyError> {
        self.key_store
            .record_upstream_reconciliation_run_stats(duration_ms, 0, 0, 0, 0, true)
            .await
    }

    pub async fn fetch_queued_scheduled_jobs(
        &self,
        limit: usize,
    ) -> Result<Vec<QueuedScheduledJob>, ProxyError> {
        self.key_store.fetch_queued_scheduled_jobs(limit).await
    }

    pub async fn fetch_next_queued_scheduled_job_excluding_types(
        &self,
        excluded_job_types: &[&str],
    ) -> Result<Option<QueuedScheduledJob>, ProxyError> {
        self.key_store
            .fetch_next_queued_scheduled_job_excluding_types(excluded_job_types)
            .await
    }

    pub async fn next_queued_scheduled_job_available_at(&self) -> Result<Option<i64>, ProxyError> {
        self.key_store.next_queued_scheduled_job_available_at().await
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

    pub async fn recover_stale_scheduled_jobs(&self) -> Result<u64, ProxyError> {
        self.key_store.recover_stale_scheduled_jobs().await
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

#[cfg(test)]
mod reconciliation_engine_tests {
    use std::sync::atomic::AtomicI64;

    use super::{
        ReconciliationEngine, ReconciliationOutcome, should_emit_reconciliation_summary_at,
    };

    #[test]
    fn reconciliation_summary_logging_is_limited_to_one_per_minute() {
        let last_emitted_at = AtomicI64::new(0);

        assert!(should_emit_reconciliation_summary_at(&last_emitted_at, 1_000));
        assert!(!should_emit_reconciliation_summary_at(&last_emitted_at, 1_059));
        assert!(should_emit_reconciliation_summary_at(&last_emitted_at, 1_060));
    }

    #[test]
    fn non_success_outcomes_do_not_clear_upstream_429_state() {
        assert!(!ReconciliationEngine::clears_upstream_429(
            ReconciliationOutcome::TransportFailure
        ));
        assert!(!ReconciliationEngine::clears_upstream_429(
            ReconciliationOutcome::SemanticFailure
        ));
        assert!(!ReconciliationEngine::clears_upstream_429(
            ReconciliationOutcome::LocalPressure
        ));
        assert!(ReconciliationEngine::clears_upstream_429(
            ReconciliationOutcome::Settled
        ));
    }

    #[test]
    fn successful_terminal_outcomes_clear_both_backoffs() {
        assert!(ReconciliationEngine::clears_local_pressure(
            ReconciliationOutcome::Settled
        ));
        assert!(ReconciliationEngine::clears_local_pressure(
            ReconciliationOutcome::NoAdjustment
        ));
        assert!(ReconciliationEngine::clears_upstream_429(
            ReconciliationOutcome::NoAdjustment
        ));
    }

    #[test]
    fn failure_outcomes_prevent_a_same_round_success_from_clearing_429() {
        assert_eq!(
            ReconciliationEngine::outcome(1, 1, false, false, true, false),
            Some(ReconciliationOutcome::TransportFailure)
        );
        assert_eq!(
            ReconciliationEngine::outcome(1, 0, false, false, false, true),
            Some(ReconciliationOutcome::SemanticFailure)
        );
        assert_eq!(
            ReconciliationEngine::outcome(1, 1, false, true, false, false),
            Some(ReconciliationOutcome::LocalPressure)
        );
    }
}
