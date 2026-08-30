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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimedResearchDrainOutcome {
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

    fn observe_research_drain_outcome(
        now: i64,
        outcome: ClaimedResearchDrainOutcome,
        cooldown_skips: i64,
    ) -> ClaimedResearchDrainOutcome {
        use std::sync::atomic::Ordering;

        let (polled, terminal, pending, retries, deferred, backlog_open) = match &outcome {
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
                defers = RESEARCH_DRAIN_DEFERS.swap(0, Ordering::Relaxed),
                resumes = RESEARCH_DRAIN_RESUMES.swap(0, Ordering::Relaxed),
                backlog_open = RESEARCH_DRAIN_BACKLOG_OPEN.load(Ordering::Relaxed),
                "reconciliation Research drain window"
            );
        }
        outcome
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
                return Ok(Self::observe_research_drain_outcome(
                    now,
                    ClaimedResearchDrainOutcome::Deferred {
                        reason: "research_drain_budget",
                        retry_at: now.saturating_add(Self::RESEARCH_DRAIN_DEFER_SECS),
                    },
                    0,
                ));
            }
            Err(error) => return Err(error),
        };
        if page.candidates.is_empty() && page.cooled_due_count > 0 {
            let retry_at = page
                .earliest_cooldown_until
                .unwrap_or_else(|| now.saturating_add(Self::RESEARCH_DRAIN_INTERVAL_SECS));
            return Ok(Self::observe_research_drain_outcome(
                now,
                ClaimedResearchDrainOutcome::Deferred {
                    reason: "key_cooldown",
                    retry_at,
                },
                page.cooled_due_count,
            ));
        }
        if page.candidates.is_empty() {
            let minimum_next_at = now.saturating_add(Self::RESEARCH_DRAIN_INTERVAL_SECS);
            let next_at = self
                .key_store
                .upstream_reconciliation_research_drain_available_at()
                .await?
                .map(|available_at| available_at.max(minimum_next_at));
            return Ok(Self::observe_research_drain_outcome(
                now,
                ClaimedResearchDrainOutcome::Completed {
                    polled: 0,
                    terminal: 0,
                    pending: 0,
                    retries: 0,
                    next_at,
                },
                0,
            ));
        }

        let key_ids = page
            .candidates
            .iter()
            .map(|candidate| candidate.key_id.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let cooldowns = self
            .key_store
            .list_active_api_key_transient_backoffs(
                &key_ids,
                Self::RECONCILIATION_BACKOFF_SCOPE,
                now,
            )
            .await?;
        let earliest_cooldown = cooldowns.values().map(|state| state.cooldown_until).min();
        let Some(candidate) = page
            .candidates
            .iter()
            .find(|candidate| !cooldowns.contains_key(&candidate.key_id))
        else {
            return Ok(Self::observe_research_drain_outcome(
                now,
                ClaimedResearchDrainOutcome::Deferred {
                    reason: "key_cooldown",
                    retry_at: earliest_cooldown
                        .unwrap_or_else(|| now.saturating_add(Self::RESEARCH_DRAIN_INTERVAL_SECS)),
                },
                cooldowns.len() as i64,
            ));
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
            .fetch_upstream_research_terminal(
                &candidate.key_id,
                usage_base,
                &candidate.request_id,
                remote_context,
            )
            .await;
        let mut key_backoff = None;
        let (poll, terminal, pending, retries) = match result {
            Ok(true) => (
                crate::store::UpstreamReconciliationResearchDrainPoll::Terminal,
                1,
                0,
                0,
            ),
            Ok(false) => (
                crate::store::UpstreamReconciliationResearchDrainPoll::Pending {
                    next_poll_at: now.saturating_add(120),
                    outcome: "pending",
                    error_kind: None,
                },
                0,
                1,
                0,
            ),
            Err((error, _)) if ReconciliationEngine::remote_attempt_is_deferred(&error) => {
                if ReconciliationEngine::remote_attempt_is_stale(&error) {
                    return Ok(Self::observe_research_drain_outcome(
                        now,
                        ClaimedResearchDrainOutcome::StaleClaim,
                        0,
                    ));
                }
                return Ok(Self::observe_research_drain_outcome(
                    now,
                    ClaimedResearchDrainOutcome::Deferred {
                        reason: "research_drain_budget",
                        retry_at: now.saturating_add(Self::RESEARCH_DRAIN_DEFER_SECS),
                    },
                    0,
                ));
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
                let (next_poll_at, error_kind) = if ReconciliationEngine::is_remote_request_timeout(&error) {
                    (now.saturating_add(Self::research_retry_delay_secs(candidate.poll_attempt_count)), "timeout")
                } else {
                    (now.saturating_add(Self::research_retry_delay_secs(candidate.poll_attempt_count)), RECONCILIATION_RETRY_REASON_OTHER)
                };
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
        let accepted = self
            .key_store
            .commit_upstream_reconciliation_research_drain(
                crate::store::UpstreamReconciliationResearchDrainCommit {
                    request_id: &candidate.request_id,
                    expected_cursor: &page.start_cursor,
                    accepted_cursor,
                    wrapped: page.wrapped,
                    poll,
                    key_backoff,
                    job_id,
                    claim_generation,
                },
            )
            .await;
        match accepted {
            Ok(true) => {}
            Ok(false) => {
                return Ok(Self::observe_research_drain_outcome(
                    now,
                    ClaimedResearchDrainOutcome::Deferred {
                        reason: "research_drain_budget",
                        retry_at: now.saturating_add(Self::RESEARCH_DRAIN_DEFER_SECS),
                    },
                    0,
                ));
            }
            Err(ProxyError::StaleClaim { .. }) => {
                return Ok(Self::observe_research_drain_outcome(
                    now,
                    ClaimedResearchDrainOutcome::StaleClaim,
                    0,
                ));
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
        let next_at = self
            .key_store
            .upstream_reconciliation_research_drain_available_at()
            .await?
            .map(|available_at| {
                available_at.max(now.saturating_add(Self::RESEARCH_DRAIN_INTERVAL_SECS))
            });
        Ok(Self::observe_research_drain_outcome(
            now,
            ClaimedResearchDrainOutcome::Completed {
                polled: 1,
                terminal,
                pending,
                retries,
                next_at,
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
