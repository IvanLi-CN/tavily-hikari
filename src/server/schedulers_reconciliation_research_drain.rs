fn scheduled_job_uses_db_execution_gate(job_type: &str) -> bool {
    !matches!(
        job_type,
        "upstream_reconciliation" | RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE
    )
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

async fn persist_claimed_research_drain(
    state: Arc<AppState>,
    job_id: i64,
    claim_generation: i64,
    run_result: Result<ClaimedResearchDrainOutcome, ProxyError>,
) -> bool {
    match run_result {
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
        Ok(ClaimedResearchDrainOutcome::Deferred { reason, retry_at }) => state
            .proxy
            .scheduled_job_finish_and_enqueue_auto_at(
                job_id,
                claim_generation,
                RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE,
                None,
                1,
                Some(&format!("deferred={reason}")),
                retry_at,
            )
            .await
            .is_ok(),
        Ok(ClaimedResearchDrainOutcome::StaleClaim) => false,
        Err(error) if tavily_hikari::is_transient_sqlite_write_error(&error) => state
            .proxy
            .scheduled_job_finish_and_enqueue_auto_at(
                job_id,
                claim_generation,
                RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE,
                None,
                1,
                Some("deferred=research_drain_budget"),
                state.proxy.backend_time().now_ts().saturating_add(30),
            )
            .await
            .is_ok(),
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
