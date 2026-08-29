#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReconciliationOutcome {
    Settled,
    NoAdjustment,
    Observed,
    Upstream429,
    TransportFailure,
    SemanticFailure,
    LocalPressure,
}

struct ReconciliationRunResult {
    settled: i64,
    completed: i64,
    no_adjustment: i64,
    observed: i64,
    transport_failure_windows: i64,
    last_transport_kind: Option<&'static str>,
    semantic_failure_windows: i64,
    settled_recent: i64,
    settled_backlog: i64,
    upstream_429_retry_windows: i64,
    local_usage_rate_limit_windows: i64,
    other_retry_windows: i64,
    key_backoff_window_count: i64,
    skipped_by_key_backoff: i64,
    attempted_candidate_count: i64,
    main_remote_request_count: i64,
    budget_exhausted: bool,
    remote_attempt_limit_reached: bool,
    max_retry_after_until: Option<i64>,
    hydrate_ms: i64,
    first_remote_ms: Option<i64>,
    remote_ms: i64,
}

struct ReconciliationFinalizationResult {
    settled: i64,
    completed: i64,
    no_adjustment: i64,
    observed: i64,
    settled_recent: i64,
    settled_backlog: i64,
    budget_exhausted: bool,
}

enum ResearchCursorAcceptance {
    Accepted,
    LocalPressure,
    StaleClaim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub enum ClaimedReconciliationRunOutcome {
    Completed {
        settled: i64,
        no_adjustment: i64,
        observed: i64,
    },
    Deferred { reason: &'static str, retry_at: i64 },
    StaleClaim,
}

pub(crate) struct ReconciliationEngine;

#[derive(Clone, Copy)]
struct ReconciliationRemoteBudget {
    main_remote_deadline: std::time::Instant,
    main_finalization_deadline: std::time::Instant,
}

impl ReconciliationRemoteBudget {
    fn with_due_research_reserve(
        research_reservation_required: bool,
        remote_request_deadline: std::time::Instant,
        finalization_deadline: std::time::Instant,
        research_sweep_budget: std::time::Duration,
        finalization_headroom: std::time::Duration,
    ) -> Self {
        if research_reservation_required {
            Self {
                main_remote_deadline: remote_request_deadline
                    - research_sweep_budget
                    - finalization_headroom,
                main_finalization_deadline: remote_request_deadline - research_sweep_budget,
            }
        } else {
            Self {
                main_remote_deadline: remote_request_deadline,
                main_finalization_deadline: finalization_deadline,
            }
        }
    }
}

impl TavilyProxy {
    async fn persist_main_reconciliation_marker(&self) -> Result<bool, ProxyError> {
        match self
            .key_store
            .mark_upstream_reconciliation_run_completed_at(self.backend_time.now_ts())
            .await
        {
            Ok(()) => Ok(true),
            Err(err) if is_transient_sqlite_write_error(&err) => {
                tracing::debug!(
                    component = "reconciliation",
                    event = "run_marker_deferred",
                    reason = "local_pressure",
                    err = %err,
                );
                Ok(false)
            }
            Err(err) => Err(err),
        }
    }

    async fn accept_research_cursor_if_ready(
        &self,
        cursor: Option<&crate::store::UpstreamReconciliationResearchCursor>,
        wrapped: bool,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<ResearchCursorAcceptance, ProxyError> {
        match self
            .key_store
            .accept_upstream_reconciliation_research_page(cursor, wrapped, claimed_job, true)
            .await
        {
            Ok(()) => Ok(ResearchCursorAcceptance::Accepted),
            Err(ProxyError::StaleClaim { .. }) => Ok(ResearchCursorAcceptance::StaleClaim),
            Err(err) if is_transient_sqlite_write_error(&err) => {
                Ok(ResearchCursorAcceptance::LocalPressure)
            }
            Err(err) => Err(err),
        }
    }

    async fn finish_research_cursor(
        &self,
        page: &crate::store::UpstreamReconciliationResearchCandidatePage,
        claimed_job: Option<(i64, i64)>,
        defer_reason: &mut Option<&'static str>,
    ) -> Result<bool, ProxyError> {
        Ok(match self
            .accept_research_cursor_if_ready(page.next_cursor.as_ref(), page.wrapped, claimed_job)
            .await?
        {
            ResearchCursorAcceptance::Accepted => false,
            ResearchCursorAcceptance::LocalPressure => {
                *defer_reason = Some("research_local_pressure");
                false
            }
            ResearchCursorAcceptance::StaleClaim => true,
        })
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
}

#[derive(Clone, Copy)]
struct ReconciliationRemoteAttemptContext<'a> {
    remote_attempt_admission: Option<&'a Arc<RemoteAttemptAdmissionController>>,
    reconciliation_turn: Option<&'a crate::ReconciliationTurn>,
    manual_remote_attempt: bool,
}

/// A stable, non-sensitive classification for failures before a reconciliation
/// response can be interpreted. The category is durable diagnostics only: it
/// never changes settlement or billing semantics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransportFailureKind {
    Connect,
    Timeout,
    ResponseBody,
    InvalidEndpoint,
    CredentialsOrDatabase,
    Unknown,
}

impl TransportFailureKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Connect => "connect",
            Self::Timeout => "timeout",
            Self::ResponseBody => "response_body",
            Self::InvalidEndpoint => "invalid_endpoint",
            Self::CredentialsOrDatabase => "credentials_or_database",
            Self::Unknown => "unknown",
        }
    }

    pub(crate) fn from_proxy_error(error: &ProxyError) -> Self {
        match error {
            ProxyError::InvalidEndpoint { .. } => Self::InvalidEndpoint,
            ProxyError::Database(_) => Self::CredentialsOrDatabase,
            ProxyError::Http(error) if error.is_timeout() => Self::Timeout,
            ProxyError::Http(error) if error.is_connect() => Self::Connect,
            ProxyError::Http(_) => Self::ResponseBody,
            _ => Self::Unknown,
        }
    }
}

impl ReconciliationRemoteAttemptContext<'_> {
    async fn acquire(self) -> Result<Option<crate::RemoteAttemptLease>, &'static str> {
        match (self.reconciliation_turn, self.remote_attempt_admission) {
        (Some(turn), _) => turn.acquire_attempt().await.map(Some),
        (None, Some(controller)) if self.manual_remote_attempt => {
            controller.acquire_manual_attempt().await.map(Some)
        }
        (None, Some(controller)) => controller.acquire_reconciliation_attempt().await.map(Some),
        (None, None) => Ok(None),
        }
    }
}

impl ReconciliationEngine {
    const MAX_REMOTE_ATTEMPTS: i64 = 2;
    const DEFER_RETRY_DELAY_SECS: i64 = 30;
    // The compatibility one-shot API has no durable representative job.
    const ONE_SHOT_ADMISSION_WAIT: std::time::Duration = std::time::Duration::from_millis(250);

    fn deferred(proxy: &TavilyProxy, reason: &'static str) -> ClaimedReconciliationRunOutcome {
        ClaimedReconciliationRunOutcome::Deferred {
            reason,
            retry_at: proxy
                .backend_time()
                .now_ts()
                .saturating_add(Self::DEFER_RETRY_DELAY_SECS),
        }
    }

    fn projection_read_budget_is_deferred(error: &ProxyError) -> bool {
        matches!(
            error,
            ProxyError::Deferred { operation, reason }
                if *operation == "reconciliation_projection" && reason == "projection_read_budget"
        )
    }

    fn post_process_exhaustion_is_deferred(error: &ProxyError) -> bool {
        matches!(
            error,
            ProxyError::Other(message)
                if message.contains("reconciliation post-processing deadline exceeded")
                    || message.contains("reconciliation retry bookkeeping deadline exceeded")
        )
    }

    async fn finalize_observed_candidates(
        proxy: &TavilyProxy,
        observed_candidates: Vec<(UpstreamReconciliationCandidate, i64, bool, i64)>,
        finalization_deadline: std::time::Instant,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<ReconciliationFinalizationResult, ProxyError> {
        let mut result = ReconciliationFinalizationResult {
            settled: 0,
            completed: 0,
            no_adjustment: 0,
            observed: 0,
            settled_recent: 0,
            settled_backlog: 0,
            budget_exhausted: false,
        };
        for (candidate, upstream_usage, in_recent_lane, work_generation) in observed_candidates {
            if finalization_deadline <= std::time::Instant::now() {
                result.budget_exhausted = true;
                break;
            }
            let local_billed = proxy
                .key_store
                .reconciliation_local_billed_credits_for_finalization(&candidate)
                .await?;
            let settlement_result = if candidate.settlement_mode == "shadow" {
                proxy
                    .key_store
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
                proxy
                    .key_store
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
            };
            let did_settle = match settlement_result {
                Ok(did_settle) => did_settle,
                Err(error) => {
                    Self::pause_active_settlement_integrity_failure(
                        proxy,
                        &candidate.settlement_mode,
                        &error,
                    )
                    .await?;
                    return Err(error);
                }
            };
            if !did_settle {
                continue;
            }
            result.completed += 1;
            if upstream_usage == local_billed {
                result.no_adjustment += 1;
            } else if candidate.settlement_mode == "shadow" {
                result.observed += 1;
            } else {
                result.settled += 1;
                if in_recent_lane {
                    result.settled_recent += 1;
                } else {
                    result.settled_backlog += 1;
                }
            }
        }
        Ok(result)
    }

    async fn run_claimed(
        proxy: &TavilyProxy,
        usage_base: &str,
        job_id: i64,
        claim_generation: i64,
        remote_attempt_admission: Option<Arc<RemoteAttemptAdmissionController>>,
        reconciliation_turn: Option<crate::ReconciliationTurn>,
        manual_remote_attempt: bool,
    ) -> Result<ClaimedReconciliationRunOutcome, ProxyError> {
        let proxy = proxy.clone();
        let finalization_proxy = proxy.clone();
        let usage_base = usage_base.to_string();
        let run_result = tokio::spawn(async move {
            let Some(_run_lease) = proxy.key_store.sqlite_runtime.try_start_maintenance_run() else {
                return Ok(Self::deferred(&proxy, "shutdown"));
            };
            proxy
                .run_upstream_reconciliation_once_inner(
                    &usage_base,
                    Some((job_id, claim_generation)),
                    remote_attempt_admission,
                    reconciliation_turn.as_ref(),
                    manual_remote_attempt,
                )
                .await
        })
        .await;
        match run_result {
            Ok(Ok(outcome)) => Ok(outcome),
            Ok(Err(ProxyError::StaleClaim { .. })) => Ok(ClaimedReconciliationRunOutcome::StaleClaim),
            Ok(Err(error)) if Self::post_process_exhaustion_is_deferred(&error) => {
                tracing::warn!(
                    component = "reconciliation",
                    event = "claimed_run_deferred",
                    defer_reason = "local_pressure",
                    err = %error,
                    "claimed reconciliation exhausted its reserved durable finalization window"
                );
                Ok(Self::deferred(&finalization_proxy, "local_pressure"))
            }
            Ok(Err(error)) => Err(error),
            Err(error) => Err(ProxyError::Other(format!(
                "claimed reconciliation task join failed: {error}"
            ))),
        }
    }

    fn outcome(
        settled: i64,
        no_adjustment: i64,
        observed: i64,
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
        } else if observed > 0 {
            Some(ReconciliationOutcome::Observed)
        } else if no_adjustment > 0 {
            Some(ReconciliationOutcome::NoAdjustment)
        } else {
            None
        }
    }

    fn is_transport_failure(err: &ProxyError) -> bool {
        matches!(
            err,
            ProxyError::Http(_) | ProxyError::Database(_) | ProxyError::InvalidEndpoint { .. }
        )
    }

    fn active_settlement_integrity_reason(err: &ProxyError) -> Option<&'static str> {
        let ProxyError::Other(message) = err else {
            return None;
        };
        if message.contains("invalid reconciliation billing subject") {
            Some("invalid_billing_subject")
        } else if message.contains("unsupported reconciliation billing subject") {
            Some("unsupported_billing_subject")
        } else {
            None
        }
    }

    async fn pause_active_settlement_integrity_failure(
        proxy: &TavilyProxy,
        settlement_mode: &str,
        error: &ProxyError,
    ) -> Result<(), ProxyError> {
        if settlement_mode != "shadow"
            && let Some(reason) = Self::active_settlement_integrity_reason(error)
        {
            proxy
                .key_store
                .pause_upstream_reconciliation_for_integrity(reason)
                .await?;
        }
        Ok(())
    }

    pub(crate) fn projection_defer_exhausts_preparation(candidate_count: usize) -> bool {
        candidate_count == 0
    }

    fn clears_local_pressure(outcome: ReconciliationOutcome) -> bool {
        matches!(
            outcome,
            ReconciliationOutcome::Settled
                | ReconciliationOutcome::NoAdjustment
                | ReconciliationOutcome::Observed
        )
    }

    fn clears_upstream_429(outcome: ReconciliationOutcome) -> bool {
        Self::clears_local_pressure(outcome)
    }
}

impl TavilyProxy {
    #[doc(hidden)]
    pub async fn run_upstream_reconciliation_once_claimed_outcome_with_remote_attempt_turn(
        &self,
        usage_base: &str,
        job_id: i64,
        claim_generation: i64,
        remote_attempt_admission: Option<Arc<RemoteAttemptAdmissionController>>,
        reconciliation_turn: Option<crate::ReconciliationTurn>,
        manual_remote_attempt: bool,
    ) -> Result<ClaimedReconciliationRunOutcome, ProxyError> {
        ReconciliationEngine::run_claimed(
            self,
            usage_base,
            job_id,
            claim_generation,
            remote_attempt_admission,
            reconciliation_turn,
            manual_remote_attempt,
        )
        .await
    }
}

#[cfg(test)]
mod reconciliation_engine_tests {
    use std::sync::atomic::AtomicI64;

    use crate::{ProxyError, TavilyProxy};

    use super::{
        ReconciliationEngine, ReconciliationOutcome, TransportFailureKind,
        should_emit_reconciliation_summary_at,
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
    fn claimed_runs_reserve_two_seconds_for_durable_finalization() {
        assert_eq!(
            TavilyProxy::RECONCILIATION_FINALIZATION_HEADROOM_SECS,
            2,
            "a remote attempt must leave a durable finalization reserve"
        );
    }

    #[test]
    fn post_process_exhaustion_is_a_durable_defer_not_a_terminal_error() {
        let error = ProxyError::Other("reconciliation post-processing deadline exceeded".to_string());
        assert!(ReconciliationEngine::post_process_exhaustion_is_deferred(&error));
        assert!(!ReconciliationEngine::post_process_exhaustion_is_deferred(
            &ProxyError::Other("unrelated reconciliation failure".to_string())
        ));
    }

    #[test]
    fn failure_outcomes_prevent_a_same_round_success_from_clearing_429() {
        assert_eq!(
            ReconciliationEngine::outcome(1, 1, 0, false, false, true, false),
            Some(ReconciliationOutcome::TransportFailure)
        );
        assert_eq!(
            ReconciliationEngine::outcome(1, 0, 0, false, false, false, true),
            Some(ReconciliationOutcome::SemanticFailure)
        );
        assert_eq!(
            ReconciliationEngine::outcome(1, 1, 0, false, true, false, false),
            Some(ReconciliationOutcome::LocalPressure)
        );
    }

    #[test]
    fn compare_observation_is_not_classified_as_a_settlement() {
        assert_eq!(
            ReconciliationEngine::outcome(0, 0, 1, false, false, false, false),
            Some(ReconciliationOutcome::Observed)
        );
    }

    #[test]
    fn actual_settlement_integrity_failures_are_pauseable_but_retryable_errors_are_not() {
        assert_eq!(
            ReconciliationEngine::active_settlement_integrity_reason(&ProxyError::Other(
                "invalid reconciliation billing subject".to_string(),
            )),
            Some("invalid_billing_subject")
        );
        assert_eq!(
            ReconciliationEngine::active_settlement_integrity_reason(&ProxyError::Other(
                "database is locked".to_string(),
            )),
            None
        );
        assert_eq!(
            ReconciliationEngine::active_settlement_integrity_reason(&ProxyError::StaleClaim {
                job_id: 1,
                claim_generation: 2,
            }),
            None
        );
    }

    #[test]
    fn transport_failure_categories_are_fixed_and_do_not_expose_error_text() {
        let invalid_endpoint = ProxyError::InvalidEndpoint {
            endpoint: "https://secret.invalid/path".to_string(),
            source: url::Url::parse("http://[").expect_err("invalid URL fixture"),
        };
        assert_eq!(
            TransportFailureKind::from_proxy_error(&invalid_endpoint).as_str(),
            "invalid_endpoint"
        );
        assert_eq!(
            TransportFailureKind::from_proxy_error(&ProxyError::Database(sqlx::Error::RowNotFound))
                .as_str(),
            "credentials_or_database"
        );
        assert_eq!(
            TransportFailureKind::from_proxy_error(&ProxyError::Other(
                "upstream body includes an access token".to_string(),
            ))
            .as_str(),
            "unknown"
        );
    }
}

impl ReconciliationOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Settled => RECONCILIATION_OUTCOME_SETTLED,
            Self::NoAdjustment => RECONCILIATION_OUTCOME_NO_ADJUSTMENT,
            Self::Observed => RECONCILIATION_OUTCOME_OBSERVED,
            Self::Upstream429 => RECONCILIATION_OUTCOME_UPSTREAM_429,
            Self::TransportFailure => RECONCILIATION_OUTCOME_TRANSPORT_FAILURE,
            Self::SemanticFailure => RECONCILIATION_OUTCOME_SEMANTIC_FAILURE,
            Self::LocalPressure => RECONCILIATION_OUTCOME_LOCAL_PRESSURE,
        }
    }
}
async fn await_reconciliation_post_process<T>(
    _deadline: std::time::Instant,
    operation: impl std::future::Future<Output = Result<T, ProxyError>>,
) -> Result<T, ProxyError> {
    operation.await
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

fn ensure_reconciliation_post_process_reserve(
    deadline: std::time::Instant,
) -> Result<(), ProxyError> {
    if deadline
        .saturating_duration_since(std::time::Instant::now())
        .is_zero()
    {
        return Err(ProxyError::Other(
            "reconciliation post-processing deadline exceeded".to_string(),
        ));
    }
    Ok(())
}
