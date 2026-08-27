static LAST_RECONCILIATION_SUMMARY_LOG_AT: AtomicI64 = AtomicI64::new(0);

include!("reconciliation_engine.rs");

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

impl TavilyProxy {
    const RECONCILIATION_BACKOFF_SCOPE: &'static str = "period_reconciliation";
    // Candidate hydration is deliberately bounded so the first main settlement
    // request cannot be displaced by the terminal-research sweep. Research is
    // allowed to use the remainder of the scheduler's 20 second budget.
    const RECONCILIATION_MAIN_PREP_BUDGET_SECS: u64 = 2;
    const RECONCILIATION_TOTAL_BUDGET_SECS: u64 = 20;
    const RECONCILIATION_FINALIZATION_HEADROOM_SECS: u64 = 2;
    const RECONCILIATION_POST_PROCESS_HEADROOM_SECS: u64 = 2;
    const RECONCILIATION_RETRY_BOOKKEEPING_HEADROOM_SECS: u64 = 2;
    const RECONCILIATION_OUTER_TIMEOUT_MARGIN_SECS: u64 = 1;
    // Research records are observability follow-up, not settlement work. Keep
    // them bounded even when the main durable projection is empty so a slow
    // remote terminal probe cannot occupy the maintenance worker's full run.
    const RECONCILIATION_RESEARCH_SWEEP_BUDGET_SECS: u64 = 2;
    const RESEARCH_SWEEP_LIMIT: usize = 20;
    const RESEARCH_SWEEP_PER_KEY_LIMIT: usize = 4;

    #[cfg(test)]
    pub(crate) fn fail_next_reconciliation_research_read_for_test(&self) {
        self.key_store
            .sqlite_runtime
            .fail_next_reconciliation_research_read_for_test();
    }

    pub async fn upstream_reconciliation_shadow_compare_active_with_settings(
        &self,
        settings: &SystemSettings,
    ) -> Result<bool, ProxyError> {
        self.key_store
            .upstream_reconciliation_shadow_compare_active_with_settings(settings)
            .await
    }

    pub async fn upstream_privacy_status(&self) -> Result<UpstreamPrivacyStatus, ProxyError> {
        let mut session = self.key_store.begin_admin_privacy_read_session().await?;
        let result = self
            .key_store
            .upstream_privacy_status_from_snapshot(&mut session)
            .await;
        let close = session.close_after_query(result.as_ref().err()).await;
        match (result, close) {
            (Ok(status), Ok(())) => Ok(status),
            (Err(error), _) | (_, Err(error)) => Err(error),
        }
    }

    #[cfg(test)]
    #[doc(hidden)]
    pub async fn upstream_privacy_status_after_one_safe_boundary_for_test(
        &self,
    ) -> Result<UpstreamPrivacyStatus, ProxyError> {
        let mut session = self.key_store.begin_admin_privacy_read_session().await?;
        session.expire_cooperative_run_budget_after_check_for_test(1);
        let result = self
            .key_store
            .upstream_privacy_status_from_snapshot(&mut session)
            .await;
        let close = session.close_after_query(result.as_ref().err()).await;
        match (result, close) {
            (Ok(status), Ok(())) => Ok(status),
            (Err(error), _) | (_, Err(error)) => Err(error),
        }
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub async fn hold_sqlite_pool_until_for_test(
        &self,
        ready: tokio::sync::oneshot::Sender<()>,
        release: std::sync::Arc<tokio::sync::Notify>,
    ) -> Result<(), ProxyError> {
        self.key_store.hold_sqlite_pool_until_for_test(ready, release).await
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub async fn install_admin_privacy_read_pause_for_test(
        &self,
    ) -> crate::store::RequestStatsPostFlushPause {
        self.key_store.install_admin_privacy_read_pause().await
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn admin_privacy_read_discards_for_test(&self) -> u64 {
        self.key_store
            .sqlite_runtime
            .discarded_connections_for_test(crate::store::SqliteOperation::AdminPrivacyRead)
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn admin_privacy_read_errors_for_test(&self) -> u64 {
        self.key_store
            .sqlite_runtime
            .operation_errors_for_test(crate::store::SqliteOperation::AdminPrivacyRead)
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub async fn verify_admin_privacy_read_connection_clean_for_test(&self) -> Result<(), ProxyError> {
        self.key_store
            .sqlite_runtime
            .begin_immediate(crate::store::SqliteOperation::AdminPrivacyRead)
            .await?
            .rollback()
            .await
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
        remote_attempt: ReconciliationRemoteAttemptContext<'_>,
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
        let remote_attempt = remote_attempt
            .acquire()
            .await
            .map_err(|reason| (ProxyError::Other(reason.to_string()), None))?;
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
        drop(remote_attempt);
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
        remote_attempt: ReconciliationRemoteAttemptContext<'_>,
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
        let remote_attempt = remote_attempt
            .acquire()
            .await
            .map_err(|reason| (ProxyError::Other(reason.to_string()), None))?;
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
        drop(remote_attempt);
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
        remote_attempt: ReconciliationRemoteAttemptContext<'_>,
    ) -> Result<(i64, i64, i64, i64, i64, bool), ProxyError> {
        let now = self.backend_time.now_ts();
        let remaining = request_deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Ok((0, 0, 0, 0, 0, true));
        }
        let candidates = self
            .key_store
            .next_upstream_reconciliation_research_candidates(80)
            .await?;
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
                    remote_attempt,
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
                    match claimed_job {
                        Some((job_id, claim_generation)) => {
                            self.key_store
                                .mark_upstream_reconciliation_research_terminal_claimed(
                                    &candidate.request_id,
                                    job_id,
                                    claim_generation,
                                )
                                .await?;
                        }
                        None => {
                            self.key_store
                                .mark_upstream_reconciliation_research_terminal(
                                    &candidate.request_id,
                                )
                                .await?;
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
                    match claimed_job {
                        Some((job_id, claim_generation)) => {
                            self.key_store
                                .record_upstream_reconciliation_research_poll_claimed(
                                    &candidate.request_id,
                                    now.saturating_add(120),
                                    "pending",
                                    None,
                                    job_id,
                                    claim_generation,
                                )
                                .await?
                        }
                        None => {
                            self.key_store
                                .record_upstream_reconciliation_research_poll(
                                    &candidate.request_id,
                                    now.saturating_add(120),
                                    "pending",
                                    None,
                                )
                                .await?
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
                        let until = self
                            .arm_reconciliation_backoff(
                                &candidate.key_id,
                                retry_after,
                                reason,
                                claimed_job,
                            )
                            .await?;
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
                    match claimed_job {
                        Some((job_id, claim_generation)) => {
                            self.key_store
                                .record_upstream_reconciliation_research_poll_claimed(
                                    &candidate.request_id,
                                    next_poll_at,
                                    outcome,
                                    Some(reason),
                                    job_id,
                                    claim_generation,
                                )
                                .await?
                        }
                        None => {
                            self.key_store
                                .record_upstream_reconciliation_research_poll(
                                    &candidate.request_id,
                                    next_poll_at,
                                    outcome,
                                    Some(reason),
                                )
                                .await?
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
            match claimed_job {
                Some((job_id, claim_generation)) => {
                    self.key_store
                        .mark_upstream_reconciliation_research_sweep_at_claimed(
                            now,
                            job_id,
                            claim_generation,
                        )
                        .await?
                }
                None => {
                    self.key_store
                        .mark_upstream_reconciliation_research_sweep_at(now)
                        .await?
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
                .run_upstream_reconciliation_once_inner(usage_base, None, None, None, false)
                .await?
            {
                ClaimedReconciliationRunOutcome::Completed {
                    settled,
                    no_adjustment,
                    observed,
                } => return Ok(settled + no_adjustment + observed),
                ClaimedReconciliationRunOutcome::Deferred { reason, .. } => {
                    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                    if remaining.is_zero() {
                        return Err(ProxyError::Other(format!(
                            "upstream reconciliation local preparation remained deferred for {}ms: {reason}",
                            ReconciliationEngine::ONE_SHOT_ADMISSION_WAIT.as_millis(),
                        )));
                    }
                    tokio::time::sleep(remaining.min(std::time::Duration::from_millis(25))).await;
                }
                ClaimedReconciliationRunOutcome::StaleClaim => return Ok(0),
            }
        }
    }

    pub(crate) async fn advance_claimed_reconciliation_projection_safe(
        &self,
        job_id: i64,
        claim_generation: i64,
        admission: SqliteMaintenanceAdmission,
    ) -> Result<
        (
            ReconciliationProjectionSliceOutcome,
            SqliteMaintenanceAdmission,
        ),
        ProxyError,
    > {
        let store = Arc::clone(&self.key_store);
        tokio::spawn(async move {
            let outcome = store
                .advance_upstream_reconciliation_work_projection_claimed(
                    job_id,
                    claim_generation,
                )
                .await;
            outcome.map(|outcome| (outcome, admission))
        })
        .await
        .map_err(|err| ProxyError::Other(format!("reconciliation projection task failed: {err}")))?
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
                ClaimedReconciliationRunOutcome::Completed {
                    settled,
                    no_adjustment,
                    observed,
                } => settled + no_adjustment + observed,
                ClaimedReconciliationRunOutcome::Deferred { .. } => 0,
                ClaimedReconciliationRunOutcome::StaleClaim => 0,
            })
    }

    #[doc(hidden)]
    pub async fn run_upstream_reconciliation_once_claimed_outcome(
        &self,
        usage_base: &str,
        job_id: i64,
        claim_generation: i64,
    ) -> Result<ClaimedReconciliationRunOutcome, ProxyError> {
        self.run_upstream_reconciliation_once_claimed_outcome_with_remote_attempt_admission(
            usage_base,
            job_id,
            claim_generation,
            None,
        )
        .await
    }

    #[doc(hidden)]
    pub async fn run_upstream_reconciliation_once_claimed_outcome_with_remote_attempt_admission(
        &self,
        usage_base: &str,
        job_id: i64,
        claim_generation: i64,
        remote_attempt_admission: Option<Arc<RemoteAttemptAdmissionController>>,
    ) -> Result<ClaimedReconciliationRunOutcome, ProxyError> {
        ReconciliationEngine::run_claimed(
            self,
            usage_base,
            job_id,
            claim_generation,
            remote_attempt_admission,
            None,
            false,
        )
        .await
    }

    async fn run_upstream_reconciliation_once_inner(
        &self,
        usage_base: &str,
        claimed_job: Option<(i64, i64)>,
        remote_attempt_admission: Option<Arc<RemoteAttemptAdmissionController>>,
        reconciliation_turn: Option<&crate::ReconciliationTurn>,
        manual_remote_attempt: bool,
    ) -> Result<ClaimedReconciliationRunOutcome, ProxyError> {
        let started_at = std::time::Instant::now();
        let reconciliation_read_metrics_started = self
            .key_store
            .sqlite_runtime
            .operation_telemetry(SqliteOperation::ReconciliationProjection);
        let remote_attempt_metrics_started = remote_attempt_admission
            .as_ref()
            .map(|controller| controller.metrics());
        let remote_attempt_context = ReconciliationRemoteAttemptContext {
            remote_attempt_admission: remote_attempt_admission.as_ref(),
            reconciliation_turn,
            manual_remote_attempt,
        };
        let mut local_admission_outcome = self.admit_upstream_reconciliation_projection();
        if matches!(
            local_admission_outcome,
            SqliteAdmissionOutcome::Deferred {
                reason: "pool_pressure"
            }
        ) {
            self.prewarm_upstream_reconciliation_projection_capacity()
                .await;
            local_admission_outcome = self.admit_upstream_reconciliation_projection();
        }
        let mut local_admission = match local_admission_outcome {
            SqliteAdmissionOutcome::Admitted(admission) => admission,
            SqliteAdmissionOutcome::Deferred { reason } => {
                tracing::debug!(
                    component = "reconciliation",
                    event = "local_preparation_deferred",
                    defer_reason = reason,
                    "reconciliation skipped local candidate preparation before SQLite connection acquisition"
                );
                return Ok(ReconciliationEngine::deferred(self, reason));
            }
        };
        let run_admission_state = match self
            .key_store
            .upstream_reconciliation_run_admission_state(claimed_job)
            .await
        {
            Ok(state) => state,
            Err(err) if is_transient_sqlite_write_error(&err) => {
                return Ok(ReconciliationEngine::deferred(self, "pool_pressure"));
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
            return Ok(ClaimedReconciliationRunOutcome::StaleClaim);
        }
        if !run_admission_state.shadow_ready
            || run_admission_state.mode == ReconciliationMode::ActivePaused
        {
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
            self.key_store
                .record_upstream_reconciliation_engine_observation(
                    ReconciliationRunObservationWrite {
                        claimed_job,
                        mode: run_admission_state.mode.as_str(),
                        hydrate_ms: 0,
                        first_remote_ms: None,
                        remote_ms: 0,
                        finalization_ms: 0,
                        research_ms: 0,
                        settled: 0,
                        no_adjustment: 0,
                        observed: 0,
                        upstream_429: 0,
                        transport_failure: 0,
                        semantic_failure: 0,
                        local_pressure: 0,
                        last_transport_kind: None,
                        last_retryable_outcome: None,
                        continuation_reason: None,
                        next_retry_at: None,
                    },
                )
                .await?;
            tracing::debug!(
                component = "reconciliation",
                event = "run_completed",
                elapsed_ms = started_at.elapsed().as_millis() as u64,
                job_type = "upstream_reconciliation",
                candidate_count = 0_i64,
                settled_count = 0_i64,
            );
            return Ok(ClaimedReconciliationRunOutcome::Completed {
                settled: 0,
                no_adjustment: 0,
                observed: 0,
            });
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
            return Ok(ClaimedReconciliationRunOutcome::Completed {
                settled: 0,
                no_adjustment: 0,
                observed: 0,
            });
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
            return Ok(ClaimedReconciliationRunOutcome::Completed {
                settled: 0,
                no_adjustment: 0,
                observed: 0,
            });
        }
        let preparation_deadline = started_at
            + std::time::Duration::from_secs(Self::RECONCILIATION_MAIN_PREP_BUDGET_SECS);
        // Cooperative deadlines leave time for durable markers and backoff
        // state writes without cancelling an open SQLite transaction.
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
        let mut preparation_budget_exhausted = false;
        let mut candidate_batch;
        if preparation_deadline <= std::time::Instant::now() {
            return Ok(ReconciliationEngine::deferred(self, "local_pressure"));
        } else {
            candidate_batch = match self
                .key_store
                .next_upstream_reconciliation_candidates(20)
                .await
            {
                Ok(batch) => batch,
                Err(err) if ReconciliationEngine::projection_read_budget_is_deferred(&err) => {
                    return Ok(ReconciliationEngine::deferred(
                        self,
                        "projection_read_budget",
                    ));
                }
                Err(err) if is_transient_sqlite_write_error(&err) => {
                    return Ok(ReconciliationEngine::deferred(self, "local_pressure"));
                }
                Err(err) => return Err(err),
            };
        }
        preparation_budget_exhausted |=
            std::time::Instant::now() >= preparation_deadline;
        if !preparation_budget_exhausted {
            // The projection is a compatibility bootstrap for usage written
            // before the durable work triggers existed. Advance one bounded
            // slice while this run still owns its preparation permit so an
            // existing candidate backlog cannot starve historical projection.
            if preparation_deadline <= std::time::Instant::now() {
                preparation_budget_exhausted = true;
            } else {
                let projection = match claimed_job {
                    Some((job_id, claim_generation)) => {
                        let (outcome, admission) = self
                            .advance_claimed_reconciliation_projection_safe(
                                job_id,
                                claim_generation,
                                local_admission,
                            )
                            .await?;
                        local_admission = admission;
                        Ok(outcome)
                    }
                    None => {
                        self.key_store
                            .advance_upstream_reconciliation_work_projection()
                            .await
                    }
                };
                match projection {
                    Ok(ReconciliationProjectionSliceOutcome::Advanced { scanned_rows, .. }) => {
                        if candidate_batch.candidates.is_empty()
                            && scanned_rows > 0
                            && std::time::Instant::now() < preparation_deadline
                        {
                            candidate_batch = match self
                                .key_store
                                .next_upstream_reconciliation_candidates(20)
                                .await
                            {
                                Ok(batch) => batch,
                                Err(err)
                                    if ReconciliationEngine::projection_read_budget_is_deferred(
                                        &err,
                                    ) =>
                                {
                                    return Ok(ReconciliationEngine::deferred(
                                        self,
                                        "projection_read_budget",
                                    ));
                                }
                                Err(err) if is_transient_sqlite_write_error(&err) => {
                                    return Ok(ReconciliationEngine::deferred(
                                        self,
                                        "local_pressure",
                                    ));
                                }
                                Err(err) => return Err(err),
                            };
                        }
                    }
                    Ok(ReconciliationProjectionSliceOutcome::Deferred {
                        reason: "projection_read_budget",
                    }) => {
                        return Ok(ReconciliationEngine::deferred(
                            self,
                            "projection_read_budget",
                        ));
                    }
                    Ok(ReconciliationProjectionSliceOutcome::Deferred { .. }) => {
                        preparation_budget_exhausted |=
                            ReconciliationEngine::projection_defer_exhausts_preparation(
                                candidate_batch.candidates.len(),
                            );
                    }
                    Ok(ReconciliationProjectionSliceOutcome::StaleClaim) => {
                        return Ok(ClaimedReconciliationRunOutcome::StaleClaim);
                    }
                    Err(err) if crate::store::is_transient_sqlite_write_error(&err) => {
                        preparation_budget_exhausted |=
                            ReconciliationEngine::projection_defer_exhausts_preparation(
                                candidate_batch.candidates.len(),
                            );
                    }
                    Err(err) => return Err(err),
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
        let run_mode = run_admission_state.mode.as_str();
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
                return Ok(ReconciliationEngine::deferred(self, "local_pressure"));
            } else {
                match self
                    .key_store
                    .reconciliation_key_ids_batch(&candidate_keys)
                    .await
                {
                    Ok(result) => result,
                    Err(err) if ReconciliationEngine::projection_read_budget_is_deferred(&err) => {
                        return Ok(ReconciliationEngine::deferred(
                            self,
                            "projection_read_budget",
                        ));
                    }
                    Err(err) if is_transient_sqlite_write_error(&err) => {
                        return Ok(ReconciliationEngine::deferred(self, "local_pressure"));
                    }
                    Err(err) => return Err(err),
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
                return Ok(ReconciliationEngine::deferred(self, "local_pressure"));
            } else {
                match self
                    .key_store
                    .list_active_api_key_transient_backoffs(
                        &all_key_ids.into_iter().collect::<Vec<_>>(),
                        Self::RECONCILIATION_BACKOFF_SCOPE,
                        self.backend_time.now_ts(),
                    )
                    .await
                {
                    Ok(result) => result,
                    Err(err) if is_transient_sqlite_write_error(&err) => {
                        return Ok(ReconciliationEngine::deferred(self, "local_pressure"));
                    }
                    Err(err) => return Err(err),
                }
            }
        };
        if !preparation_budget_exhausted && !candidates.is_empty() {
            let remaining = candidate_hydration_deadline
                .saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Ok(ReconciliationEngine::deferred(self, "local_pressure"));
            }
            match self
                .key_store
                .reconciliation_local_billed_credits_batch(&candidates)
                .await
            {
                Ok(_) => {}
                Err(err) if ReconciliationEngine::projection_read_budget_is_deferred(&err) => {
                    return Ok(ReconciliationEngine::deferred(
                        self,
                        "projection_read_budget",
                    ));
                }
                Err(err) if is_transient_sqlite_write_error(&err) => {
                    return Ok(ReconciliationEngine::deferred(self, "local_pressure"));
                }
                Err(err) => return Err(err),
            }
        }
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
        let hydrate_ms = started_at.elapsed().as_millis().min(i64::MAX as u128) as i64;
        // Remote I/O and durable settlement must never retain the local bulk
        // permit. The permit protects only candidate selection, hydration and
        // the optional legacy projection above.
        drop(local_admission);
        let result = async {
            let mut settled = 0_i64;
            let mut completed = 0_i64;
            let mut no_adjustment = 0_i64;
            let mut observed_terminal = 0_i64;
            let mut transport_failure_windows = 0_i64;
            let mut last_transport_kind = None::<&'static str>;
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
            let mut first_remote_ms = None;
            let remote_phase_started = std::time::Instant::now();
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
                    semantic_failure_windows += 1;
                    other_retry_windows += 1;
                    self.key_store
                        .mark_reconciliation_retry(
                            &candidate,
                            "waiting",
                            self.backend_time.now_ts(),
                            Some("no eligible upstream key"),
                            RECONCILIATION_OUTCOME_SEMANTIC_FAILURE,
                            Some(ReconciliationWorkFence {
                                work_generation,
                                claimed_job,
                            }),
                        )
                        .await?;
                    continue;
                }
                let remaining_remote_attempts = ReconciliationEngine::MAX_REMOTE_ATTEMPTS
                    .saturating_sub(remote_request_count) as usize;
                if key_count > remaining_remote_attempts {
                    if remote_request_count > 0 {
                        remote_attempt_limit_reached = true;
                        budget_exhausted = true;
                        break 'candidates;
                    }
                    semantic_failure_windows += 1;
                    other_retry_windows += 1;
                    self.key_store
                        .mark_reconciliation_retry(
                            &candidate,
                            "waiting",
                            self.backend_time.now_ts(),
                            Some("candidate exceeds remote request limit"),
                            RECONCILIATION_OUTCOME_SEMANTIC_FAILURE,
                            Some(ReconciliationWorkFence {
                                work_generation,
                                claimed_job,
                            }),
                        )
                        .await?;
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
                    let reservation = self
                        .key_store
                        .reserve_upstream_usage_attempt(&key_id)
                        .await?;
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
                    if remote_request_count == 0 {
                        first_remote_ms = Some(
                            started_at.elapsed().as_millis().min(i64::MAX as u128) as i64,
                        );
                    }
                    remote_request_count += 1;
                    let usage_result = tokio::time::timeout(
                        remaining.max(Duration::from_millis(1)),
                        self.fetch_upstream_project_usage(
                            &key_id,
                            usage_base,
                            &candidate.project_id,
                            remote_attempt_context,
                        ),
                    )
                    .await;
                    match usage_result {
                        Err(_) => {
                            transport_failure_windows += 1;
                            last_transport_kind = Some(TransportFailureKind::Timeout.as_str());
                            retry_reason = Some("transport_failure".to_string());
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
                            if outcome == ReconciliationOutcome::TransportFailure {
                                last_transport_kind = Some(
                                    TransportFailureKind::from_proxy_error(&err).as_str(),
                                );
                                retry_reason = Some("transport_failure".to_string());
                            } else if outcome == ReconciliationOutcome::Upstream429 {
                                retry_reason =
                                    Some(RECONCILIATION_RETRY_REASON_UPSTREAM_429.to_string());
                            } else {
                                retry_reason = Some("semantic_failure".to_string());
                            }
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
                        ensure_reconciliation_post_process_reserve(retry_bookkeeping_deadline)?;
                        let cooldown_until = if reason_kind
                            == RECONCILIATION_RETRY_REASON_UPSTREAM_429
                        {
                            self.arm_reconciliation_backoff(
                                &retry_key_id,
                                retry_at,
                                reason_kind,
                                claimed_job,
                            )
                            .await?
                        } else {
                            retry_at.unwrap_or_else(|| self.backend_time.now_ts().saturating_add(300))
                        };
                        self.key_store
                            .mark_reconciliation_retry(
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
                            )
                            .await?;
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

            // Billing can become charged while the remote request is in flight. The
            // source-phase batch above proves that BillingHydrate fits its native
            // deadline before any HTTP starts. Re-read each observed candidate here
            // through the bounded finalization connection so settlement uses the
            // post-observation ledger state without a post-request source deadline.
            if !observed_candidates.is_empty() {
                // A later remote request may exhaust the main budget after an earlier
                // request has already succeeded. Do not let that later timeout discard
                // observations that still fit the bounded finalization window.
                let finalization = ReconciliationEngine::finalize_observed_candidates(
                    self,
                    observed_candidates,
                    finalization_deadline,
                    claimed_job,
                )
                .await?;
                settled += finalization.settled;
                completed += finalization.completed;
                no_adjustment += finalization.no_adjustment;
                observed_terminal += finalization.observed;
                settled_recent += finalization.settled_recent;
                settled_backlog += finalization.settled_backlog;
                budget_exhausted |= finalization.budget_exhausted;
            }
            Ok::<ReconciliationRunResult, ProxyError>(ReconciliationRunResult {
                settled,
                completed,
                no_adjustment,
                observed: observed_terminal,
                transport_failure_windows,
                last_transport_kind,
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
                hydrate_ms,
                first_remote_ms,
                remote_ms: remote_phase_started
                    .elapsed()
                    .as_millis()
                    .min(i64::MAX as u128) as i64,
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
            return Ok(ClaimedReconciliationRunOutcome::StaleClaim);
        }
        let main_budget_exhausted = result
            .as_ref()
            .map(|value| value.budget_exhausted)
            .unwrap_or(true);
        let research_started_at = std::time::Instant::now();
        let mut research_local_pressure = false;
        let (
            research_polled_count,
            research_terminal_count,
            research_pending_count,
            research_retry_count,
            research_skipped_cooldown_count,
            research_budget_exhausted,
        ) = if result.is_ok() && !self.sqlite_maintenance_runs_shutting_down() {
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
                    remote_attempt_context,
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
                    return Ok(ClaimedReconciliationRunOutcome::StaleClaim);
                }
                Err(err) if ReconciliationEngine::projection_read_budget_is_deferred(&err) => {
                    return Ok(ReconciliationEngine::deferred(
                        self,
                        "projection_read_budget",
                    ));
                }
                Err(err) if is_transient_sqlite_write_error(&err) => {
                    research_local_pressure = true;
                    tracing::debug!(
                        component = "reconciliation",
                        event = "research_sweep_deferred",
                        reason = "local_pressure",
                        err = %err,
                    );
                    (0, 0, 0, 0, 0, true)
                }
                Err(err) => return Err(err),
            }
        } else {
            (0, 0, 0, 0, 0, main_budget_exhausted)
        };
        let research_ms = research_started_at
            .elapsed()
            .as_millis()
            .min(i64::MAX as u128) as i64;
        // Do the only cooperative deadline check before any post-processing
        // write. Once durable work begins, do not turn a partially applied
        // sequence into a second local-pressure transition.
        ensure_reconciliation_post_process_reserve(post_process_deadline)?;
        let run_marker_result = self
            .key_store
            .mark_upstream_reconciliation_run_completed_at(self.backend_time.now_ts())
            .await;
        if let Err(err) = run_marker_result {
            if is_transient_sqlite_write_error(&err) {
                tracing::debug!(
                    component = "reconciliation",
                    event = "run_marker_deferred",
                    reason = "local_pressure",
                    err = %err,
                );
                return Ok(ReconciliationEngine::deferred(self, "local_pressure"));
            }
            return Err(err);
        }
        match result {
            Ok(ReconciliationRunResult {
                settled,
                completed,
                no_adjustment,
                observed,
                transport_failure_windows,
                last_transport_kind,
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
                hydrate_ms,
                first_remote_ms,
                remote_ms,
            }) => {
                let remote_attempt_metrics = remote_attempt_admission
                    .as_ref()
                    .map(|controller| controller.metrics());
                let remote_wait_ms = remote_attempt_metrics
                    .zip(remote_attempt_metrics_started)
                    .map(|(current, started)| {
                        current.total_wait_ms.saturating_sub(started.total_wait_ms)
                    })
                    .unwrap_or_default();
                let remote_hold_ms = remote_attempt_metrics
                    .zip(remote_attempt_metrics_started)
                    .map(|(current, started)| {
                        current.total_hold_ms.saturating_sub(started.total_hold_ms)
                    })
                    .unwrap_or_default();
                let reconciliation_read_metrics = self
                    .key_store
                    .sqlite_runtime
                    .operation_telemetry(SqliteOperation::ReconciliationProjection)
                    .delta_since(reconciliation_read_metrics_started);
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
                    remote_wait_ms,
                    remote_hold_ms,
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
                let observation_result = await_reconciliation_post_process(
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
                .await;
                match observation_result {
                    Ok(()) => {}
                    Err(ProxyError::StaleClaim { .. }) => {
                        return Ok(ClaimedReconciliationRunOutcome::StaleClaim);
                    }
                    Err(err) => return Err(err),
                }
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
                let upstream_429_observed = upstream_429_retry_windows > 0;
                let qualified_remote_pressure = upstream_429_observed
                    && completed == 0
                    && upstream_429_retry_windows.saturating_mul(2)
                        >= attempted_candidate_count.max(1);
                let local_pressure = local_usage_rate_limit_windows > 0
                    || research_local_pressure
                    || ((candidate_count > 0 || preparation_budget_exhausted)
                        && attempted_candidate_count == 0
                        && budget_exhausted);
                let reconciliation_outcome = ReconciliationEngine::outcome(
                    settled,
                    no_adjustment,
                    observed,
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
                let finalization_ms = started_at
                    .elapsed()
                    .as_millis()
                    .min(i64::MAX as u128) as i64
                    - hydrate_ms
                    - remote_ms
                    - research_ms;
                let next_retry_at = [local_backoff_until, backoff_until]
                    .into_iter()
                    .filter(|value| *value > self.backend_time.now_ts())
                    .max();
                await_reconciliation_post_process(
                    post_process_deadline,
                    self.key_store.record_upstream_reconciliation_engine_observation(
                        ReconciliationRunObservationWrite {
                            claimed_job,
                            mode: run_mode,
                            hydrate_ms,
                            first_remote_ms,
                            remote_ms,
                            finalization_ms: finalization_ms.max(0),
                            research_ms,
                            settled,
                            no_adjustment,
                            observed,
                            upstream_429: upstream_429_retry_windows,
                            transport_failure: transport_failure_windows,
                            semantic_failure: semantic_failure_windows,
                            local_pressure: i64::from(local_pressure),
                            last_transport_kind,
                            last_retryable_outcome: reconciliation_outcome
                                .filter(|outcome| {
                                    matches!(
                                        outcome,
                                        ReconciliationOutcome::Upstream429
                                            | ReconciliationOutcome::TransportFailure
                                            | ReconciliationOutcome::SemanticFailure
                                            | ReconciliationOutcome::LocalPressure
                                    )
                                })
                                .map(ReconciliationOutcome::as_str),
                            continuation_reason: reconciliation_outcome.map(|value| value.as_str()),
                            next_retry_at,
                        },
                    ),
                )
                .await?;
                if should_emit_reconciliation_summary(self.backend_time.now_ts()) {
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
                        observed_count = observed,
                        rate_limited_429_count = upstream_429_retry_windows,
                        transport_failure_count = transport_failure_windows,
                        semantic_failure_count = semantic_failure_windows,
                        local_pressure_count = i64::from(local_pressure),
                        hydrate_ms,
                        first_remote_ms,
                        remote_ms,
                        finalization_ms = finalization_ms.max(0),
                        research_ms,
                        reconciliation_source_read_ms = reconciliation_read_metrics.cooperative_read_elapsed_ms,
                        reconciliation_source_read_deadline_count = reconciliation_read_metrics.cooperative_read_deadlines,
                        reconciliation_operation_connection_cache_write_pages = %if reconciliation_read_metrics.connection_cache_write_sampled
                            && !reconciliation_read_metrics.connection_cache_write_sample_failed
                        {
                            reconciliation_read_metrics.connection_cache_write_pages.to_string()
                        } else {
                            "unknown".to_string()
                        },
                        remote_wait_ms,
                        remote_hold_ms,
                        remote_active_attempts = remote_attempt_metrics
                            .map(|metrics| metrics.active_attempts)
                            .unwrap_or_default(),
                        continuation_reason = reconciliation_outcome.map(|value| value.as_str()),
                        next_retry_at,
                        budget_exhausted,
                    );
                }
                Ok(ClaimedReconciliationRunOutcome::Completed {
                    settled,
                    no_adjustment,
                    observed,
                })
            }
            Err(err) if ReconciliationEngine::projection_read_budget_is_deferred(&err) => {
                tracing::debug!(
                    component = "reconciliation",
                    event = "preparation_deferred",
                    reason = "projection_read_budget",
                    "reconciliation source read reached its SQLite budget before a durable boundary"
                );
                Ok(ReconciliationEngine::deferred(
                    self,
                    "projection_read_budget",
                ))
            }
            Err(err) if is_transient_sqlite_write_error(&err) => {
                tracing::debug!(
                    component = "reconciliation",
                    event = "settlement_deferred",
                    reason = "local_pressure",
                    err = %err,
                );
                Ok(ReconciliationEngine::deferred(self, "local_pressure"))
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

    pub async fn finalize_deferred_upstream_reconciliation_claim(
        &self,
        job_id: i64,
        claim_generation: i64,
        reason: &'static str,
        retry_at: i64,
    ) -> Result<ScheduledJobEnqueueResult, ProxyError> {
        self.key_store
            .finalize_deferred_upstream_reconciliation_claim(
                job_id,
                claim_generation,
                reason,
                retry_at,
            )
            .await
    }

    #[doc(hidden)]
    pub async fn clear_upstream_reconciliation_local_backoff_claimed(
        &self,
        job_id: i64,
        claim_generation: i64,
    ) -> Result<(), ProxyError> {
        self.key_store
            .update_upstream_reconciliation_local_backoff_claimed(
                false,
                self.backend_time.now_ts(),
                job_id,
                claim_generation,
            )
            .await
            .map(|_| ())
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

    pub async fn fetch_aged_queued_scheduled_job_by_type(
        &self,
        job_type: &str,
        minimum_eligible_wait_secs: i64,
    ) -> Result<Option<QueuedScheduledJob>, ProxyError> {
        self.key_store
            .fetch_aged_queued_scheduled_job_by_type(job_type, minimum_eligible_wait_secs)
            .await
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
mod privacy_status_phase_budget_tests {
    use super::*;

    #[tokio::test]
    async fn privacy_status_stops_at_a_safe_boundary_and_closes_its_snapshot() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let db_path = temp_dir.path().join("privacy-status-phase-budget.db");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-privacy-status-phase-budget".to_string()],
            crate::DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let error = proxy
            .upstream_privacy_status_after_one_safe_boundary_for_test()
            .await
            .expect_err("the real status builder must stop before its second SQLite phase");
        assert!(matches!(
            error,
            ProxyError::Database(sqlx::Error::PoolTimedOut)
        ));
        assert_eq!(
            proxy.admin_privacy_read_discards_for_test(),
            0,
            "phase-bound refresh aborts must close the snapshot without discarding it"
        );
        proxy
            .verify_admin_privacy_read_connection_clean_for_test()
            .await
            .expect("the next privacy transaction is clean after a phase-bound refresh abort");
    }
}
