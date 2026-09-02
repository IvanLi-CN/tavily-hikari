fn scheduled_job_uses_remote_io(job_type: &str) -> bool {
    REMOTE_IO_SCHEDULED_JOB_TYPES.contains(&job_type)
}

fn scheduled_job_is_manual_remote(job: &QueuedScheduledJob) -> bool {
    job.trigger_source == TRIGGER_SOURCE_MANUAL
}

async fn sync_key_quota_with_db_job_gate(
    state: &AppState,
    key_id: &str,
    source: &str,
    manual_remote_attempt: bool,
) -> Result<(i64, i64), ProxyError> {
    let secret = {
        let _maintenance = acquire_db_maintenance_read_gate().await;
        state.proxy.quota_sync_api_key_secret(key_id).await?
    };
    let remote_attempt = (if manual_remote_attempt {
        remote_attempt_admission_for_state(state)
            .acquire_manual_attempt()
            .await
    } else {
        remote_attempt_admission_for_state(state).acquire_attempt().await
    })
    .map_err(|reason| ProxyError::Other(reason.to_string()))?;
    let result = tokio::time::timeout(
        Duration::from_secs(QUOTA_SYNC_JOB_TIMEOUT_SECS),
        state
            .proxy
            .fetch_usage_quota_for_sync_secret(&secret, &state.usage_base, key_id),
    )
    .await;
    drop(remote_attempt);

    let (limit, remaining) = match result {
        Ok(Ok(quota)) => quota,
        Ok(Err(err)) => {
            let _job_execution_gate = acquire_db_job_execution_gate_for_state(state).await;
            let _maintenance = acquire_db_maintenance_read_gate().await;
            state.proxy.record_quota_sync_usage_error(key_id, &err).await?;
            return Err(err);
        }
        Err(_) => {
            let err = ProxyError::Other(format!(
                "quota_sync timed out after {}s",
                QUOTA_SYNC_JOB_TIMEOUT_SECS
            ));
            let _job_execution_gate = acquire_db_job_execution_gate_for_state(state).await;
            let _maintenance = acquire_db_maintenance_read_gate().await;
            state.proxy.record_quota_sync_usage_error(key_id, &err).await?;
            return Err(err);
        }
    };

    let _job_execution_gate = acquire_db_job_execution_gate_for_state(state).await;
    let _maintenance = acquire_db_maintenance_read_gate().await;
    state
        .proxy
        .record_quota_sync_result(key_id, limit, remaining, source)
        .await?;

    Ok((limit, remaining))
}
