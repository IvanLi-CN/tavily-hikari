static LAST_RESEARCH_DRAIN_SUMMARY_LOG_AT: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);
static RESEARCH_DRAIN_POLLS: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);
static RESEARCH_DRAIN_TERMINAL: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);
static RESEARCH_DRAIN_PENDING: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);
static RESEARCH_DRAIN_RETRIES: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);
static RESEARCH_DRAIN_COOLDOWN_SKIPS: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);
static RESEARCH_DRAIN_DEFERS: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);
static RESEARCH_DRAIN_RESUMES: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);
static RESEARCH_DRAIN_BACKLOG_OPEN: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);
static RESEARCH_DRAIN_UNAVAILABLE: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);
static RESEARCH_DRAIN_CREDENTIALS: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimedResearchDrainOutcome {
    Persisted {
        polled: i64,
        terminal: i64,
        pending: i64,
        retries: i64,
        unavailable: i64,
        credentials_cooling: i64,
    },
    Completed {
        polled: i64,
        terminal: i64,
        pending: i64,
        retries: i64,
        next_at: Option<i64>,
    },
    Deferred {
        reason: &'static str,
        retry_at: i64,
    },
    StaleClaim,
}

impl TavilyProxy {
    const RESEARCH_DRAIN_INTERVAL_SECS: i64 = 5;
    const RESEARCH_DRAIN_DEFER_SECS: i64 = 30;
    const RESEARCH_CREDENTIALS_BACKOFF_SCOPE: &'static str =
        "reconciliation_research_credentials";
    const RESEARCH_CREDENTIALS_COOLDOWN_SECS: i64 = 6 * 60 * 60;

    fn observe_research_drain_outcome(
        now: i64,
        outcome: ClaimedResearchDrainOutcome,
        cooldown_skips: i64,
    ) -> ClaimedResearchDrainOutcome {
        use std::sync::atomic::Ordering;

        let (polled, terminal, pending, retries, deferred, backlog_open) = match &outcome {
            ClaimedResearchDrainOutcome::Persisted {
                polled,
                terminal,
                pending,
                retries,
                unavailable,
                credentials_cooling,
            } => {
                RESEARCH_DRAIN_UNAVAILABLE.fetch_add(*unavailable, Ordering::Relaxed);
                RESEARCH_DRAIN_CREDENTIALS.fetch_add(*credentials_cooling, Ordering::Relaxed);
                (*polled, *terminal, *pending, *retries, 0, 1)
            }
            ClaimedResearchDrainOutcome::Completed {
                polled,
                terminal,
                pending,
                retries,
                next_at,
            } => (*polled, *terminal, *pending, *retries, 0, i64::from(next_at.is_some())),
            ClaimedResearchDrainOutcome::Deferred { .. } => (0, 0, 0, 0, 1, 1),
            ClaimedResearchDrainOutcome::StaleClaim => (0, 0, 0, 0, 0, 1),
        };
        RESEARCH_DRAIN_POLLS.fetch_add(polled, Ordering::Relaxed);
        RESEARCH_DRAIN_TERMINAL.fetch_add(terminal, Ordering::Relaxed);
        RESEARCH_DRAIN_PENDING.fetch_add(pending, Ordering::Relaxed);
        RESEARCH_DRAIN_RETRIES.fetch_add(retries, Ordering::Relaxed);
        RESEARCH_DRAIN_COOLDOWN_SKIPS.fetch_add(cooldown_skips, Ordering::Relaxed);
        RESEARCH_DRAIN_DEFERS.fetch_add(deferred, Ordering::Relaxed);
        RESEARCH_DRAIN_RESUMES.fetch_add(i64::from(polled > 0), Ordering::Relaxed);
        RESEARCH_DRAIN_BACKLOG_OPEN.store(backlog_open, Ordering::Relaxed);
        if should_emit_reconciliation_summary_at(&LAST_RESEARCH_DRAIN_SUMMARY_LOG_AT, now) {
            tracing::info!(
                component = "reconciliation_research_drain",
                event = "research_drain_window",
                polls = RESEARCH_DRAIN_POLLS.swap(0, Ordering::Relaxed),
                terminal = RESEARCH_DRAIN_TERMINAL.swap(0, Ordering::Relaxed),
                pending = RESEARCH_DRAIN_PENDING.swap(0, Ordering::Relaxed),
                retries = RESEARCH_DRAIN_RETRIES.swap(0, Ordering::Relaxed),
                cooldown_skips = RESEARCH_DRAIN_COOLDOWN_SKIPS.swap(0, Ordering::Relaxed),
                unavailable = RESEARCH_DRAIN_UNAVAILABLE.swap(0, Ordering::Relaxed),
                credentials_cooling = RESEARCH_DRAIN_CREDENTIALS.swap(0, Ordering::Relaxed),
                defers = RESEARCH_DRAIN_DEFERS.swap(0, Ordering::Relaxed),
                resumes = RESEARCH_DRAIN_RESUMES.swap(0, Ordering::Relaxed),
                backlog_open = RESEARCH_DRAIN_BACKLOG_OPEN.load(Ordering::Relaxed),
                "reconciliation Research drain window"
            );
        }
        outcome
    }

    async fn record_one_shot_research_credentials(
        &self,
        candidate: &crate::models::UpstreamReconciliationResearchCandidate,
        outcome: ResearchPollOutcome,
        now: i64,
        cooling_keys: &mut std::collections::HashSet<String>,
        earliest_cooldown_until: &mut Option<i64>,
    ) -> Result<(), ProxyError> {
        let error_kind = if matches!(outcome, ResearchPollOutcome::MissingLocalSecret) {
            "missing_local_secret"
        } else {
            "credentials"
        };
        let cooldown_until = now.saturating_add(Self::RESEARCH_CREDENTIALS_COOLDOWN_SECS);
        self.key_store
            .arm_api_key_transient_backoff(crate::store::ApiKeyTransientBackoffArm {
                key_id: &candidate.key_id,
                scope: Self::RESEARCH_CREDENTIALS_BACKOFF_SCOPE,
                cooldown_until,
                retry_after_secs: Self::RESEARCH_CREDENTIALS_COOLDOWN_SECS,
                reason_code: Some(error_kind),
                source_request_log_id: None,
                now,
            })
            .await?;
        cooling_keys.insert(candidate.key_id.clone());
        *earliest_cooldown_until = Some(
            earliest_cooldown_until
                .map_or(cooldown_until, |current| current.min(cooldown_until)),
        );
        self.key_store
            .record_upstream_reconciliation_research_poll(
                &candidate.request_id,
                cooldown_until,
                "retry",
                Some(error_kind),
            )
            .await
    }

    pub async fn run_upstream_reconciliation_research_drain_claimed(
        &self,
        usage_base: &str,
        job_id: i64,
        claim_generation: i64,
        remote_attempt_admission: Arc<RemoteAttemptAdmissionController>,
    ) -> Result<ClaimedResearchDrainOutcome, ProxyError> {
        let now = self.backend_time.now_ts();
        let page = match self
            .key_store
            .next_upstream_reconciliation_research_candidates(80)
            .await
        {
            Ok(page) => page,
            Err(error)
                if ReconciliationEngine::projection_read_budget_is_deferred(&error)
                    || is_transient_sqlite_write_error(&error) =>
            {
                return Ok(ClaimedResearchDrainOutcome::Deferred {
                        reason: "research_drain_budget",
                        retry_at: now.saturating_add(Self::RESEARCH_DRAIN_DEFER_SECS),
                    });
            }
            Err(error) => return Err(error),
        };
        if page.candidates.is_empty() && page.cooled_due_count > 0 {
            let retry_at = page
                .earliest_cooldown_until
                .unwrap_or_else(|| now.saturating_add(Self::RESEARCH_DRAIN_INTERVAL_SECS));
            return Ok(ClaimedResearchDrainOutcome::Deferred {
                    reason: "key_cooldown",
                    retry_at,
                });
        }
        if page.candidates.is_empty() {
            let minimum_next_at = now.saturating_add(Self::RESEARCH_DRAIN_INTERVAL_SECS);
            let next_at = self
                .key_store
                .upstream_reconciliation_research_drain_available_at()
                .await?
                .map(|available_at| available_at.max(minimum_next_at));
            return Ok(ClaimedResearchDrainOutcome::Completed {
                    polled: 0,
                    terminal: 0,
                    pending: 0,
                    retries: 0,
                    next_at,
                });
        }

        let key_ids = page
            .candidates
            .iter()
            .map(|candidate| candidate.key_id.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let mut cooldowns = self
            .key_store
            .list_active_api_key_transient_backoffs(
                &key_ids,
                Self::RECONCILIATION_BACKOFF_SCOPE,
                now,
            )
            .await?;
        for (key_id, state) in self
            .key_store
            .list_active_api_key_transient_backoffs(
                &key_ids,
                Self::RESEARCH_CREDENTIALS_BACKOFF_SCOPE,
                now,
            )
            .await?
        {
            cooldowns
                .entry(key_id)
                .and_modify(|current| {
                    if state.cooldown_until > current.cooldown_until {
                        *current = state;
                    }
                })
                .or_insert(state);
        }
        let earliest_cooldown = cooldowns.values().map(|state| state.cooldown_until).min();
        let Some(candidate) = page
            .candidates
            .iter()
            .find(|candidate| !cooldowns.contains_key(&candidate.key_id))
        else {
            return Ok(ClaimedResearchDrainOutcome::Deferred {
                    reason: "key_cooldown",
                    retry_at: earliest_cooldown
                        .unwrap_or_else(|| now.saturating_add(Self::RESEARCH_DRAIN_INTERVAL_SECS)),
                });
        };
        let accepted_cursor = page
            .candidate_cursors
            .get(&candidate.request_id)
            .ok_or_else(|| ProxyError::Other("missing Research candidate cursor".to_string()))?;
        let request_deadline = std::time::Instant::now()
            + std::time::Duration::from_secs(Self::RECONCILIATION_RESEARCH_SWEEP_BUDGET_SECS);
        let remote_context = ReconciliationRemoteAttemptContext {
            remote_attempt_admission: Some(&remote_attempt_admission),
            reconciliation_turn: None,
            manual_remote_attempt: false,
            attempt_deadline: Some(request_deadline),
        };
        let result = self
            .fetch_upstream_research_poll(
                &candidate.key_id,
                usage_base,
                &candidate.request_id,
                remote_context,
            )
            .await;
        let mut key_backoff = None;
        let mut clear_key_backoff_scope = None;
        let (poll, terminal, pending, retries) = match result {
            Ok(ResearchPollOutcome::Terminal) => {
                clear_key_backoff_scope = Some(Self::RESEARCH_CREDENTIALS_BACKOFF_SCOPE);
                (
                crate::store::UpstreamReconciliationResearchDrainPoll::Terminal,
                1,
                0,
                0,
                )
            }
            Ok(ResearchPollOutcome::Pending) => {
                clear_key_backoff_scope = Some(Self::RESEARCH_CREDENTIALS_BACKOFF_SCOPE);
                (
                crate::store::UpstreamReconciliationResearchDrainPoll::Pending {
                    next_poll_at: now.saturating_add(120),
                    outcome: "pending",
                    error_kind: None,
                },
                0,
                1,
                0,
                )
            }
            Ok(ResearchPollOutcome::Unavailable) => (
                crate::store::UpstreamReconciliationResearchDrainPoll::Unavailable {
                    error_kind: "not_found",
                },
                0,
                0,
                0,
            ),
            Ok(outcome @ (ResearchPollOutcome::Credentials | ResearchPollOutcome::MissingLocalSecret)) => {
                let missing_secret = matches!(outcome, ResearchPollOutcome::MissingLocalSecret);
                let error_kind = if missing_secret {
                    "missing_local_secret"
                } else {
                    "credentials"
                };
                let cooldown_until = now.saturating_add(Self::RESEARCH_CREDENTIALS_COOLDOWN_SECS);
                key_backoff = Some(crate::store::ApiKeyTransientBackoffArm {
                    key_id: &candidate.key_id,
                    scope: Self::RESEARCH_CREDENTIALS_BACKOFF_SCOPE,
                    cooldown_until,
                    retry_after_secs: Self::RESEARCH_CREDENTIALS_COOLDOWN_SECS,
                    reason_code: Some(error_kind),
                    source_request_log_id: None,
                    now,
                });
                (
                    crate::store::UpstreamReconciliationResearchDrainPoll::Pending {
                        next_poll_at: cooldown_until,
                        outcome: "retry",
                        error_kind: Some(error_kind),
                    },
                    0,
                    0,
                    1,
                )
            }
            Err((error, _)) if ReconciliationEngine::remote_attempt_is_deferred(&error) => {
                if ReconciliationEngine::remote_attempt_is_stale(&error) {
                    return Ok(ClaimedResearchDrainOutcome::StaleClaim);
                }
                return Ok(ClaimedResearchDrainOutcome::Deferred {
                        reason: "research_drain_budget",
                        retry_at: now.saturating_add(Self::RESEARCH_DRAIN_DEFER_SECS),
                    });
            }
            Err((ProxyError::UsageHttp { status, .. }, retry_after))
                if status == reqwest::StatusCode::TOO_MANY_REQUESTS =>
            {
                let prior_retry_after_secs = self
                    .key_store
                    .api_key_transient_backoff_state(
                        &candidate.key_id,
                        Self::RECONCILIATION_BACKOFF_SCOPE,
                    )
                    .await?
                    .map(|state| state.retry_after_secs);
                let retry_after_secs = ReconciliationEngine::reconciliation_retry_delay_secs(
                    prior_retry_after_secs,
                    retry_after.map(|until| until.saturating_sub(now)),
                );
                let cooldown_until = now.saturating_add(retry_after_secs);
                key_backoff = Some(crate::store::ApiKeyTransientBackoffArm {
                    key_id: &candidate.key_id,
                    scope: Self::RECONCILIATION_BACKOFF_SCOPE,
                    cooldown_until,
                    retry_after_secs,
                    reason_code: Some("upstream429"),
                    source_request_log_id: None,
                    now,
                });
                (
                    crate::store::UpstreamReconciliationResearchDrainPoll::Pending {
                        next_poll_at: cooldown_until,
                        outcome: "rate_limited",
                        error_kind: Some(RECONCILIATION_RETRY_REASON_UPSTREAM_429),
                    },
                    0,
                    0,
                    1,
                )
            }
            Err((error, _)) => {
                let error_kind = if ReconciliationEngine::is_remote_request_timeout(&error) {
                    "timeout"
                } else if matches!(&error, ProxyError::UsageHttp { status, .. } if status.is_server_error()) {
                    "response_body"
                } else {
                    TransportFailureKind::from_proxy_error(&error).as_str()
                };
                let next_poll_at = now.saturating_add(Self::research_retry_delay_secs(candidate.poll_attempt_count));
                (
                    crate::store::UpstreamReconciliationResearchDrainPoll::Pending {
                        next_poll_at,
                        outcome: "retry",
                        error_kind: Some(error_kind),
                    },
                    0,
                    0,
                    1,
                )
            }
        };
        let unavailable = matches!(
            &poll,
            crate::store::UpstreamReconciliationResearchDrainPoll::Unavailable { .. }
        );
        let credentials_backoff = key_backoff.is_some();
        let receipt = self
            .key_store
            .commit_upstream_reconciliation_research_drain(
                crate::store::UpstreamReconciliationResearchDrainCommit {
                    request_id: &candidate.request_id,
                    expected_cursor: &page.start_cursor,
                    accepted_cursor,
                    wrapped: page.wrapped,
                    poll,
                    key_backoff,
                    clear_key_backoff_scope,
                    job_id,
                    claim_generation,
                },
            )
            .await;
        match receipt {
            Ok(crate::store::ResearchDrainCommitReceipt::Accepted { .. }) => {}
            Ok(crate::store::ResearchDrainCommitReceipt::Deferred { retry_at }) => {
                return Ok(ClaimedResearchDrainOutcome::Deferred {
                        reason: "research_drain_budget",
                        retry_at,
                    });
            }
            Ok(crate::store::ResearchDrainCommitReceipt::StaleClaim) | Err(ProxyError::StaleClaim { .. }) => {
                return Ok(ClaimedResearchDrainOutcome::StaleClaim);
            }
            Err(error) => return Err(error),
        }
        tracing::debug!(
            component = "reconciliation_research_drain",
            event = "research_poll_persisted",
            terminal,
            pending,
            retries,
        );
        Ok(Self::observe_research_drain_outcome(
            now,
            ClaimedResearchDrainOutcome::Persisted {
                polled: 1,
                terminal,
                pending,
                retries,
                unavailable: i64::from(unavailable),
                credentials_cooling: i64::from(credentials_backoff),
            },
            cooldowns.len() as i64,
        ))
    }

    fn research_retry_delay_secs(poll_attempt_count: i64) -> i64 {
        match poll_attempt_count {
            0..=1 => 60,
            2..=3 => 120,
            4..=5 => 300,
            6..=7 => 600,
            _ => 1800,
        }
    }
}
