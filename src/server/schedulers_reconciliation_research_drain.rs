fn scheduled_job_uses_db_execution_gate(job_type: &str) -> bool {
    !matches!(
        job_type,
        "upstream_reconciliation" | RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE
    )
}

fn reconciliation_turn_eligible_since(job: &QueuedScheduledJob) -> i64 {
    if job.job_type == RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE {
        job.queued_at
    } else {
        job.available_at
    }
}

async fn select_next_scheduled_candidate(
    state: &AppState,
    candidates: &[QueuedScheduledJob],
) -> Result<(Option<QueuedScheduledJob>, Option<ReconciliationTurn>), ProxyError> {
    let controller = remote_attempt_admission_for_state(state);
    let mut selected = candidates
        .iter()
        .find(|candidate| {
            scheduled_job_uses_remote_io(&candidate.job_type)
                && scheduled_job_is_manual_remote(candidate)
        })
        .cloned();
    let mut reconciliation_turn = None;

    if selected.is_none() {
        let aged_main = state
            .proxy
            .fetch_aged_queued_scheduled_job_by_type(
                "upstream_reconciliation",
                RECONCILIATION_REMOTE_TURN_WAIT_SECS,
            )
            .await?;
        let aged_research = state
            .proxy
            .fetch_aged_queued_scheduled_job_by_type(
                RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE,
                RECONCILIATION_REMOTE_TURN_WAIT_SECS,
            )
            .await?;
        let resumed_kind = controller.resumable_reconciliation_turn_kind();
        let aged = match (resumed_kind, aged_main, aged_research) {
            (Some(ReconciliationTurnKind::ResearchDrain), _, Some(research)) => {
                Some((research, ReconciliationTurnKind::ResearchDrain))
            }
            (Some(ReconciliationTurnKind::Main), Some(main), _) => {
                Some((main, ReconciliationTurnKind::Main))
            }
            (Some(_), _, _) => None,
            (None, Some(main), Some(research))
                if reconciliation_turn_eligible_since(&research)
                    < reconciliation_turn_eligible_since(&main) =>
            {
                Some((research, ReconciliationTurnKind::ResearchDrain))
            }
            (None, Some(main), _) => Some((main, ReconciliationTurnKind::Main)),
            (None, None, Some(research)) => {
                Some((research, ReconciliationTurnKind::ResearchDrain))
            }
            (None, None, None) => None,
        };
        if let Some((aged_job, kind)) = aged {
            let turn = match kind {
                ReconciliationTurnKind::Main => controller.reserve_aged_reconciliation_turn(),
                ReconciliationTurnKind::ResearchDrain => {
                    controller.reserve_aged_research_drain_turn()
                }
            };
            if let Some(turn) = turn {
                reconciliation_turn = Some(turn);
                selected = Some(aged_job);
            }
        }
    }

    if selected.is_none() && !controller.reconciliation_turn_required() {
        let main_available = candidates
            .iter()
            .any(|candidate| candidate.job_type == "upstream_reconciliation");
        let research_available = candidates
            .iter()
            .any(|candidate| candidate.job_type == RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE);
        if let Some(turn) = controller
            .reserve_next_automatic_reconciliation_turn(main_available, research_available)
        {
            let selected_job = candidates.iter().find(|candidate| match turn.kind() {
                ReconciliationTurnKind::Main => candidate.job_type == "upstream_reconciliation",
                ReconciliationTurnKind::ResearchDrain => {
                    candidate.job_type == RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE
                }
            });
            if let Some(selected_job) = selected_job {
                reconciliation_turn = Some(turn);
                selected = Some(selected_job.clone());
            }
        }
    }

    if selected.is_none() {
        for candidate in candidates {
            if scheduled_job_uses_remote_io(&candidate.job_type)
                && controller.reconciliation_turn_required()
            {
                let reserved_kind = reconciliation_turn.as_ref().map(|turn| turn.kind());
                let is_reconciliation = matches!(
                    candidate.job_type.as_str(),
                    "upstream_reconciliation" | RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE
                );
                let matches_reservation = match reserved_kind {
                    Some(ReconciliationTurnKind::Main) => {
                        candidate.job_type == "upstream_reconciliation"
                    }
                    Some(ReconciliationTurnKind::ResearchDrain) => {
                        candidate.job_type == RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE
                    }
                    None => false,
                };
                if is_reconciliation && !matches_reservation {
                    continue;
                }
            }
            selected = Some(candidate.clone());
            break;
        }
    }

    if selected.is_none() {
        selected = state
            .proxy
            .fetch_next_queued_scheduled_job_excluding_types(&REMOTE_IO_SCHEDULED_JOB_TYPES)
            .await?;
    }
    Ok((selected, reconciliation_turn))
}

#[cfg(test)]
#[test]
fn upstream_reconciliation_does_not_wait_for_db_execution_gate() {
    assert!(scheduled_job_uses_remote_io("upstream_reconciliation"));
    assert!(!scheduled_job_uses_db_execution_gate(
        "upstream_reconciliation"
    ));
    assert!(!scheduled_job_uses_db_execution_gate(
        RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE
    ));
    assert!(scheduled_job_uses_db_execution_gate("ha_outbox_gc"));
}

fn spawn_upstream_reconciliation_startup_resume(state: Arc<AppState>) {
    tokio::spawn(async move {
        match state
            .proxy
            .ensure_upstream_reconciliation_representative_job()
            .await
        {
            Ok(()) => maintenance_worker_wake_for_state(state.as_ref()).notify_one(),
            Err(err) if tavily_hikari::is_transient_sqlite_write_error(&err) => tracing::debug!(
                component = "reconciliation",
                event = "startup_resume_enqueue_deferred",
                defer_reason = "sqlite_contention",
                retry_via = "stale_reaper",
                err = %err,
            ),
            Err(err) => tracing::error!(
                component = "reconciliation",
                event = "startup_resume_enqueue_failed",
                err = %err,
            ),
        }
        match state
            .proxy
            .ensure_upstream_reconciliation_research_drain_job()
            .await
        {
            Ok(()) => maintenance_worker_wake_for_state(state.as_ref()).notify_one(),
            Err(err) if tavily_hikari::is_transient_sqlite_write_error(&err) => tracing::debug!(
                component = "reconciliation_research_drain",
                event = "startup_resume_enqueue_deferred",
                defer_reason = "sqlite_contention",
                retry_via = "stale_reaper",
                err = %err,
            ),
            Err(err) => tracing::error!(
                component = "reconciliation_research_drain",
                event = "startup_resume_enqueue_failed",
                err = %err,
            ),
        }
    });
}

async fn run_reconciliation_research_drain_claimed_job(
    state: Arc<AppState>,
    job_id: i64,
    claim_generation: i64,
    reconciliation_turn: Option<ReconciliationTurn>,
) -> bool {
    let foreground_rps = state.proxy.foreground_activity_rps();
    let aged_research_turn = reconciliation_turn
        .as_ref()
        .is_some_and(|turn| turn.kind() == ReconciliationTurnKind::ResearchDrain && turn.is_aged());
    if foreground_rps > tavily_hikari::HA_OUTBOX_GC_LOW_PRESSURE_RPS && !aged_research_turn {
        let retry_at = state.proxy.backend_time().now_ts().saturating_add(30);
        return persist_claimed_research_drain(
            state,
            job_id,
            claim_generation,
            Ok(ClaimedResearchDrainOutcome::Deferred {
                reason: tavily_hikari::ResearchDrainDeferReason::ForegroundPressure,
                retry_at,
            }),
        )
        .await;
    }

    let remote_attempt_admission = remote_attempt_admission_for_state(state.as_ref());
    let run_result = state
        .proxy
        .run_upstream_reconciliation_research_drain_claimed_with_turn(
            &state.usage_base,
            job_id,
            claim_generation,
            remote_attempt_admission,
            reconciliation_turn.as_ref(),
        )
        .await;
    let retain_research_turn_after_commit = reconciliation_turn.as_ref().is_some()
        && matches!(
            &run_result,
            Ok(ClaimedResearchDrainOutcome::Deferred {
                reason: tavily_hikari::ResearchDrainDeferReason::RemoteLease,
                ..
            })
        );
    let accepted = persist_claimed_research_drain(state, job_id, claim_generation, run_result).await;
    if accepted
        && retain_research_turn_after_commit
        && let Some(turn) = reconciliation_turn.as_ref()
    {
        turn.retain_for_continuation();
    }
    accepted
}

async fn persist_claimed_research_drain(
    state: Arc<AppState>,
    job_id: i64,
    claim_generation: i64,
    run_result: Result<ClaimedResearchDrainOutcome, ProxyError>,
) -> bool {
    match run_result {
        Ok(ClaimedResearchDrainOutcome::Persisted { .. }) => true,
        Ok(ClaimedResearchDrainOutcome::Completed {
            polled,
            terminal,
            pending,
            retries,
            next_at,
        }) => {
            let message = format!(
                "polled={polled} terminal={terminal} pending={pending} retries={retries}"
            );
            match next_at {
                Some(available_at) => state
                    .proxy
                    .scheduled_job_finish_and_enqueue_auto_at(
                        job_id,
                        claim_generation,
                        RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE,
                        None,
                        1,
                        Some(&message),
                        available_at,
                    )
                    .await
                    .is_ok(),
                None => state
                    .proxy
                    .scheduled_job_finish_claimed(
                        job_id,
                        claim_generation,
                        "success",
                        Some(&message),
                    )
                    .await
                    .is_ok(),
            }
        }
        Ok(ClaimedResearchDrainOutcome::Deferred { reason, retry_at }) => {
            let accepted = state
                .proxy
                .scheduled_job_finish_and_enqueue_auto_at(
                    job_id,
                    claim_generation,
                    RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE,
                    None,
                    1,
                    Some(reason.scheduled_job_message()),
                    retry_at,
                )
                .await
                .is_ok();
            if accepted {
                TavilyProxy::observe_research_drain_defer(reason);
            }
            accepted
        }
        Ok(ClaimedResearchDrainOutcome::StaleClaim) => false,
        Err(error) if tavily_hikari::is_transient_sqlite_write_error(&error) => {
            let accepted = state
                .proxy
                .scheduled_job_finish_and_enqueue_auto_at(
                    job_id,
                    claim_generation,
                    RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE,
                    None,
                    1,
                    Some(
                        tavily_hikari::ResearchDrainDeferReason::ControlDefer
                            .scheduled_job_message(),
                    ),
                    state.proxy.backend_time().now_ts().saturating_add(30),
                )
                .await
                .is_ok();
            if accepted {
                TavilyProxy::observe_research_drain_defer(
                    tavily_hikari::ResearchDrainDeferReason::ControlDefer,
                );
            }
            accepted
        }
        Err(error) => state
            .proxy
            .scheduled_job_finish_claimed(
                job_id,
                claim_generation,
                "error",
                Some(&error.to_string()),
            )
            .await
            .is_ok(),
    }
}
