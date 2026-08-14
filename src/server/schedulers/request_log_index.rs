async fn run_request_logs_body_gc_index_ensure_claimed_job(
    state: Arc<AppState>,
    claimed_job: ClaimedScheduledJob,
) -> bool {
    let ClaimedScheduledJob {
        job_id,
        claim_generation,
        _job_execution_gate,
    } = claimed_job;
    drop(_job_execution_gate);
    let _bulk_admission = match state.proxy.admit_request_logs_gc() {
        tavily_hikari::SqliteAdmissionOutcome::Admitted(permit) => permit,
        tavily_hikari::SqliteAdmissionOutcome::Deferred { reason } => {
            let available_at = state
                .proxy
                .backend_time()
                .now_ts()
                .saturating_add(REQUEST_LOGS_BODY_GC_INDEX_ENSURE_RETRY_DELAY_SECS);
            let message = format!("deferred={reason}");
            match state
                .proxy
                .scheduled_job_finish_and_enqueue_auto_at(
                    job_id,
                    claim_generation,
                    REQUEST_LOGS_BODY_GC_INDEX_ENSURE_JOB_TYPE,
                    None,
                    1,
                    Some(&message),
                    available_at,
                )
                .await
            {
                Ok(retry) => {
                    maintenance_worker_wake_for_state(state.as_ref()).notify_one();
                    tracing::debug!(
                        component = "request_logs_gc",
                        event = "body_gc_index_deferred",
                        job_id,
                        retry_job_id = retry.job_id,
                        defer_reason = reason,
                        available_at,
                        "request-log body GC index build deferred before SQLite connection acquisition"
                    );
                }
                Err(err) => tracing::warn!(
                    component = "request_logs_gc",
                    event = "body_gc_index_defer_handoff_failed",
                    job_id,
                    defer_reason = reason,
                    available_at,
                    err = %err,
                    "request-log body GC index defer handoff could not be persisted; stale recovery remains eligible"
                ),
            }
            return false;
        }
    };
    let _schema_admission = match state.proxy.admit_request_logs_gc_schema() {
        tavily_hikari::SqliteSchemaAdmissionOutcome::Admitted(admission) => admission,
        tavily_hikari::SqliteSchemaAdmissionOutcome::Deferred { reason } => {
            drop(_bulk_admission);
            return defer_request_logs_body_gc_index_ensure(
                state,
                job_id,
                claim_generation,
                reason,
            )
            .await;
        }
    };
    if let Some(reason) = state.proxy.request_logs_gc_continue_defer_reason() {
        drop(_schema_admission);
        drop(_bulk_admission);
        return defer_request_logs_body_gc_index_ensure(
            state,
            job_id,
            claim_generation,
            reason,
        )
        .await;
    }
    let result = state.proxy.ensure_request_log_body_gc_cursor_index().await;
    drop(_schema_admission);
    drop(_bulk_admission);

    match result {
        Ok(()) => {
            let _ = state
                .proxy
                .scheduled_job_finish_claimed(
                    job_id,
                    claim_generation,
                    "success",
                    Some("partial_index_ready"),
                )
                .await;
            true
        }
        Err(err) => {
            let available_at = state
                .proxy
                .backend_time()
                .now_ts()
                .saturating_add(REQUEST_LOGS_BODY_GC_INDEX_ENSURE_RETRY_DELAY_SECS);
            match state
                .proxy
                .scheduled_job_finish_and_enqueue_auto_at(
                    job_id,
                    claim_generation,
                    REQUEST_LOGS_BODY_GC_INDEX_ENSURE_JOB_TYPE,
                    None,
                    1,
                    Some(&err.to_string()),
                    available_at,
                )
                .await
            {
                Ok(retry) => {
                    maintenance_worker_wake_for_state(state.as_ref()).notify_one();
                    tracing::warn!(
                        component = "request_logs_gc",
                        event = "body_gc_index_retry_queued",
                        failed_job_id = job_id,
                        retry_job_id = retry.job_id,
                        retry_delay_secs = REQUEST_LOGS_BODY_GC_INDEX_ENSURE_RETRY_DELAY_SECS,
                        available_at,
                        err = %err,
                        "request-log body GC index build failed; retry persisted atomically"
                    );
                }
                Err(handoff_err) => tracing::warn!(
                    component = "request_logs_gc",
                    event = "body_gc_index_retry_handoff_failed",
                    failed_job_id = job_id,
                    retry_delay_secs = REQUEST_LOGS_BODY_GC_INDEX_ENSURE_RETRY_DELAY_SECS,
                    available_at,
                    err = %err,
                    handoff_err = %handoff_err,
                    "request-log body GC index build retry handoff failed; stale recovery remains eligible"
                ),
            }
            false
        }
    }
}

async fn defer_request_logs_body_gc_index_ensure(
    state: Arc<AppState>,
    job_id: i64,
    claim_generation: i64,
    reason: &str,
) -> bool {
    let available_at = state
        .proxy
        .backend_time()
        .now_ts()
        .saturating_add(REQUEST_LOGS_BODY_GC_INDEX_ENSURE_RETRY_DELAY_SECS);
    let message = format!("deferred={reason}");
    match state
        .proxy
        .scheduled_job_finish_and_enqueue_auto_at(
            job_id,
            claim_generation,
            REQUEST_LOGS_BODY_GC_INDEX_ENSURE_JOB_TYPE,
            None,
            1,
            Some(&message),
            available_at,
        )
        .await
    {
        Ok(retry) => {
            maintenance_worker_wake_for_state(state.as_ref()).notify_one();
            tracing::debug!(
                component = "request_logs_gc",
                event = "body_gc_index_deferred",
                job_id,
                retry_job_id = retry.job_id,
                defer_reason = reason,
                available_at,
                "request-log body GC index build deferred before SQLite schema work"
            );
        }
        Err(err) => tracing::warn!(
            component = "request_logs_gc",
            event = "body_gc_index_defer_handoff_failed",
            job_id,
            defer_reason = reason,
            available_at,
            err = %err,
            "request-log body GC index defer handoff could not be persisted; stale recovery remains eligible"
        ),
    }
    false
}
