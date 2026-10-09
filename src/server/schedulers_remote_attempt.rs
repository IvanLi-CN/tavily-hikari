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
    let (secret, provider) = {
        let _maintenance = acquire_db_maintenance_read_gate().await;
        let secret = state.proxy.quota_sync_api_key_secret(key_id).await?;
        let provider = state.proxy.quota_sync_key_provider(key_id).await?;
        (secret, provider)
    };
    // M4 hard guard (plan todo 19): the Tavily quota_sync family
    // (quota_sync | quota_sync/manual | quota_sync/hot) FAILS CLOSED on any
    // stored non-Tavily key BEFORE admission/egress — a third-party secret
    // can never reach TAVILY_USAGE_BASE /usage even if a job was enqueued
    // against it by hand. Missing key rows keep the legacy RowNotFound path.
    if let Some(provider) = provider
        && provider != tavily_hikari::providers::ProviderId::Tavily
    {
        return Err(ProxyError::Other(format!(
            "provider_guard_rejected: tavily quota_sync lane is tavily-only, key provider is {provider}"
        )));
    }
    let admission = remote_attempt_admission_for_state(state);
    let result = tokio::time::timeout(
        Duration::from_secs(QUOTA_SYNC_JOB_TIMEOUT_SECS),
        state
            .proxy
            .fetch_usage_quota_for_sync_secret_with_admission(
                &secret,
                &state.usage_base,
                key_id,
                admission,
                manual_remote_attempt,
            ),
    )
    .await;

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
