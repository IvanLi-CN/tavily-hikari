fn random_delay_secs(max_inclusive: u64) -> u64 {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    rng.gen_range(0..=max_inclusive)
}

fn twenty_four_hours_secs() -> i64 {
    24 * 60 * 60
}

fn two_hours_secs() -> i64 {
    2 * 60 * 60
}

fn fifteen_minutes_secs() -> i64 {
    15 * 60
}

fn forward_proxy_geo_refresh_recheck_secs() -> i64 {
    60
}

fn request_logs_gc_catchup_recheck_secs() -> u64 {
    300
}

fn scheduled_request_logs_gc_options() -> RequestLogsGcOptions {
    RequestLogsGcOptions {
        batch_size: 100,
        max_batches: 5,
        max_runtime_secs: 20,
        inter_batch_sleep_ms: 0,
    }
}

const LINUXDO_USER_STATUS_SYNC_JOB_TYPE: &str = "linuxdo_user_status_sync";
const LINUXDO_USER_TAG_BINDING_REFRESH_JOB_TYPE: &str = "linuxdo_user_tag_binding_refresh";
const TRIGGER_SOURCE_SCHEDULER: &str = "scheduler";
const TRIGGER_SOURCE_MANUAL: &str = "manual";
const TRIGGER_SOURCE_AUTO: &str = "auto";

struct ClaimedScheduledJob {
    job_id: i64,
    _job_execution_gate: Option<OwnedMutexGuard<()>>,
}

async fn claim_scheduled_job_with_gate(
    state: &AppState,
    job_type: &str,
    key_id: Option<&str>,
    trigger_source: &str,
) -> Result<Option<ClaimedScheduledJob>, ProxyError> {
    let job_execution_gate = acquire_db_job_execution_gate_for_state(state).await;
    let _maintenance = acquire_db_maintenance_read_gate().await;
    match state
        .proxy
        .scheduled_job_claim(job_type, trigger_source, key_id, 1)
        .await
    {
        Ok(Some(job_id)) => Ok(Some(ClaimedScheduledJob {
            job_id,
            _job_execution_gate: Some(job_execution_gate),
        })),
        Ok(None) => Ok(None),
        Err(err) => Err(err),
    }
}

async fn claim_scheduled_job(
    state: &AppState,
    job_type: &str,
    key_id: Option<&str>,
    trigger_source: &str,
    log_prefix: &str,
) -> Option<ClaimedScheduledJob> {
    match claim_scheduled_job_with_gate(state, job_type, key_id, trigger_source).await {
        Ok(Some(job)) => Some(job),
        Ok(None) => {
            eprintln!("{log_prefix}: job already running; skip trigger");
            None
        }
        Err(err) => {
            eprintln!("{log_prefix}: start job error: {err}");
            None
        }
    }
}

async fn sync_key_quota_with_db_job_gate(
    state: &AppState,
    key_id: &str,
    source: &str,
) -> Result<(i64, i64), ProxyError> {
    let secret = {
        let _maintenance = acquire_db_maintenance_read_gate().await;
        state.proxy.quota_sync_api_key_secret(key_id).await?
    };
    let result = tokio::time::timeout(
        Duration::from_secs(QUOTA_SYNC_JOB_TIMEOUT_SECS),
        state
            .proxy
            .fetch_usage_quota_for_sync_secret(&secret, &state.usage_base, key_id),
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

fn next_local_daily_run_after(now: DateTime<Local>, hour: u32, minute: u32) -> DateTime<Local> {
    let today = now.date_naive();
    let scheduled_naive = today
        .and_hms_opt(hour, minute, 0)
        .unwrap_or_else(|| today.and_hms_opt(6, 20, 0).expect("valid default time"));
    let scheduled_today = match Local.from_local_datetime(&scheduled_naive) {
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(dt, _) => dt,
        chrono::LocalResult::None => now,
    };
    if scheduled_today > now {
        return scheduled_today;
    }

    let tomorrow = today.succ_opt().unwrap_or_else(|| {
        today
            .checked_add_days(chrono::Days::new(1))
            .unwrap_or(today)
    });
    let next_naive = tomorrow
        .and_hms_opt(hour, minute, 0)
        .unwrap_or_else(|| tomorrow.and_hms_opt(6, 20, 0).expect("valid default time"));
    match Local.from_local_datetime(&next_naive) {
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(dt, _) => dt,
        chrono::LocalResult::None => now + ChronoDuration::hours(24),
    }
}

fn duration_until_next_local_daily_run(now: DateTime<Local>, hour: u32, minute: u32) -> Duration {
    (next_local_daily_run_after(now, hour, minute) - now)
        .to_std()
        .unwrap_or_else(|_| Duration::from_secs(0))
}

fn spawn_quota_sync_scheduler(state: Arc<AppState>) {
    let cold_state = state.clone();
    tokio::spawn(async move {
        loop {
            let keys = {
                let _maintenance = acquire_db_maintenance_read_gate().await;
                match cold_state
                    .proxy
                    .list_keys_pending_quota_sync(twenty_four_hours_secs())
                    .await
                {
                    Ok(list) => list,
                    Err(err) => {
                        eprintln!("quota-sync: list pending error: {err}");
                        vec![]
                    }
                }
            };

            for key_id in keys {
                let delay = random_delay_secs(300);
                tokio::time::sleep(Duration::from_secs(delay)).await;
                let Some(ClaimedScheduledJob {
                    job_id,
                    _job_execution_gate,
                }) = claim_scheduled_job(
                    cold_state.as_ref(),
                    "quota_sync",
                    Some(&key_id),
                    TRIGGER_SOURCE_SCHEDULER,
                    "quota-sync",
                )
                .await
                else {
                    continue;
                };
                drop(_job_execution_gate);
                match sync_key_quota_with_db_job_gate(cold_state.as_ref(), &key_id, "quota_sync")
                    .await
                {
                    Ok((limit, remaining)) => {
                        let msg = format!("limit={limit} remaining={remaining}");
                        let _ = cold_state
                            .proxy
                            .scheduled_job_finish(job_id, "success", Some(&msg))
                            .await;
                    }
                    Err(ProxyError::QuotaDataMissing { reason }) => {
                        let msg = format!("quota_data_missing: {reason}");
                        let _ = cold_state
                            .proxy
                            .scheduled_job_finish(job_id, "error", Some(&msg))
                            .await;
                    }
                    Err(ProxyError::UsageHttp { status, body }) => {
                        let msg = format!("usage_http {status}: {body}");
                        let _ = cold_state
                            .proxy
                            .scheduled_job_finish(job_id, "error", Some(&msg))
                            .await;
                    }
                    Err(err) => {
                        let _ = cold_state
                            .proxy
                            .scheduled_job_finish(job_id, "error", Some(&err.to_string()))
                            .await;
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });

    let hot_state = state;
    tokio::spawn(async move {
        loop {
            let keys = {
                let _maintenance = acquire_db_maintenance_read_gate().await;
                match hot_state
                    .proxy
                    .list_keys_pending_hot_quota_sync(two_hours_secs(), fifteen_minutes_secs())
                    .await
                {
                    Ok(list) => list,
                    Err(err) => {
                        eprintln!("quota-sync-hot: list pending error: {err}");
                        vec![]
                    }
                }
            };

            for key_id in keys {
                let delay = random_delay_secs(60);
                tokio::time::sleep(Duration::from_secs(delay)).await;
                let Some(ClaimedScheduledJob {
                    job_id,
                    _job_execution_gate,
                }) = claim_scheduled_job(
                    hot_state.as_ref(),
                    "quota_sync/hot",
                    Some(&key_id),
                    TRIGGER_SOURCE_SCHEDULER,
                    "quota-sync-hot",
                )
                .await
                else {
                    continue;
                };
                drop(_job_execution_gate);
                match sync_key_quota_with_db_job_gate(hot_state.as_ref(), &key_id, "quota_sync/hot")
                    .await
                {
                    Ok((limit, remaining)) => {
                        let msg = format!("limit={limit} remaining={remaining}");
                        let _ = hot_state
                            .proxy
                            .scheduled_job_finish(job_id, "success", Some(&msg))
                            .await;
                    }
                    Err(ProxyError::QuotaDataMissing { reason }) => {
                        let msg = format!("quota_data_missing: {reason}");
                        let _ = hot_state
                            .proxy
                            .scheduled_job_finish(job_id, "error", Some(&msg))
                            .await;
                    }
                    Err(ProxyError::UsageHttp { status, body }) => {
                        let msg = format!("usage_http {status}: {body}");
                        let _ = hot_state
                            .proxy
                            .scheduled_job_finish(job_id, "error", Some(&msg))
                            .await;
                    }
                    Err(err) => {
                        let _ = hot_state
                            .proxy
                            .scheduled_job_finish(job_id, "error", Some(&err.to_string()))
                            .await;
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(300)).await;
        }
    });
}

fn spawn_token_usage_rollup_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            let Some(ClaimedScheduledJob {
                job_id,
                _job_execution_gate,
            }) = claim_scheduled_job(
                state.as_ref(),
                "token_usage_rollup",
                None,
                TRIGGER_SOURCE_SCHEDULER,
                "token-usage-rollup",
            )
            .await
            else {
                tokio::time::sleep(Duration::from_secs(300)).await;
                continue;
            };

            let _maintenance = acquire_db_maintenance_read_gate().await;
            match state.proxy.rollup_token_usage_stats().await {
                Ok((rows, last_ts)) => {
                    let msg = match last_ts {
                        Some(ts) => format!("rows={rows} last_rollup_ts={ts}"),
                        None => format!("rows={rows} last_rollup_ts=none"),
                    };
                    let _ = state
                        .proxy
                        .scheduled_job_finish(job_id, "success", Some(&msg))
                        .await;
                }
                Err(err) => {
                    let _ = state
                        .proxy
                        .scheduled_job_finish(job_id, "error", Some(&err.to_string()))
                        .await;
                }
            }
            drop(_maintenance);

            // Run rollup every 5 minutes to keep charts reasonably fresh
            tokio::time::sleep(Duration::from_secs(300)).await;
        }
    });
}

fn spawn_auth_token_logs_gc_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            let Some(ClaimedScheduledJob {
                job_id,
                _job_execution_gate,
            }) = claim_scheduled_job(
                state.as_ref(),
                "auth_token_logs_gc",
                None,
                TRIGGER_SOURCE_SCHEDULER,
                "auth-token-logs-gc",
            )
            .await
            else {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                continue;
            };

            let _maintenance = acquire_db_maintenance_read_gate().await;
            match state.proxy.gc_auth_token_logs().await {
                Ok(deleted) => {
                    let msg = format!("deleted_rows={deleted}");
                    let _ = state
                        .proxy
                        .scheduled_job_finish(job_id, "success", Some(&msg))
                        .await;
                }
                Err(err) => {
                    let _ = state
                        .proxy
                        .scheduled_job_finish(job_id, "error", Some(&err.to_string()))
                        .await;
                }
            }
            drop(_maintenance);

            // Run GC once per hour; retention window is enforced inside the proxy.
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
}

fn spawn_mcp_sessions_gc_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            let Some(ClaimedScheduledJob {
                job_id,
                _job_execution_gate,
            }) = claim_scheduled_job(
                state.as_ref(),
                "mcp_sessions_gc",
                None,
                TRIGGER_SOURCE_SCHEDULER,
                "mcp-sessions-gc",
            )
            .await
            else {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                continue;
            };

            let _maintenance = acquire_db_maintenance_read_gate().await;
            match state.proxy.gc_mcp_sessions().await {
                Ok(deleted) => {
                    let msg = format!("deleted_rows={deleted}");
                    let _ = state
                        .proxy
                        .scheduled_job_finish(job_id, "success", Some(&msg))
                        .await;
                }
                Err(err) => {
                    let _ = state
                        .proxy
                        .scheduled_job_finish(job_id, "error", Some(&err.to_string()))
                        .await;
                }
            }
            drop(_maintenance);

            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
}

fn spawn_mcp_session_init_backoffs_gc_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            let Some(ClaimedScheduledJob {
                job_id,
                _job_execution_gate,
            }) = claim_scheduled_job(
                state.as_ref(),
                "mcp_session_init_backoffs_gc",
                None,
                TRIGGER_SOURCE_SCHEDULER,
                "mcp-session-init-backoffs-gc",
            )
            .await
            else {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                continue;
            };

            let _maintenance = acquire_db_maintenance_read_gate().await;
            match state.proxy.gc_mcp_session_init_backoffs().await {
                Ok(deleted) => {
                    let msg = format!("deleted_rows={deleted}");
                    let _ = state
                        .proxy
                        .scheduled_job_finish(job_id, "success", Some(&msg))
                        .await;
                }
                Err(err) => {
                    let _ = state
                        .proxy
                        .scheduled_job_finish(job_id, "error", Some(&err.to_string()))
                        .await;
                }
            }
            drop(_maintenance);

            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
}

fn spawn_request_logs_gc_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        // Schedule: daily at configured local time.
        loop {
            let (hour, minute) = effective_request_logs_gc_at();
            tokio::time::sleep(duration_until_next_local_daily_run(Local::now(), hour, minute))
                .await;

            // After we reach the scheduled time, keep running bounded passes until the backlog
            // is cleared or a pass errors out. Each pass is a separate scheduled_jobs row so
            // operators can aggregate daily cleanup throughput from job history directly.
            loop {
                let Some(claimed_job) = claim_scheduled_job(
                    state.as_ref(),
                    "request_logs_gc",
                    None,
                    TRIGGER_SOURCE_SCHEDULER,
                    "request-logs-gc",
                )
                .await
                else {
                    tokio::time::sleep(Duration::from_secs(300)).await;
                    continue;
                };

                let completed =
                    run_request_logs_gc_catchup_claimed_job(state.clone(), claimed_job).await;
                if completed {
                    break;
                }

                tokio::time::sleep(Duration::from_secs(
                    request_logs_gc_catchup_recheck_secs(),
                ))
                .await;
            }
        }
    });
}

async fn run_request_logs_gc_catchup_claimed_job(
    state: Arc<AppState>,
    claimed_job: ClaimedScheduledJob,
) -> bool {
    let ClaimedScheduledJob {
        job_id,
        _job_execution_gate,
    } = claimed_job;
    drop(_job_execution_gate);
    let _job_execution_gate = acquire_db_job_execution_gate_for_state(state.as_ref()).await;
    let _maintenance = acquire_db_maintenance_read_gate().await;
    let result = state
        .proxy
        .gc_request_logs_with_options(scheduled_request_logs_gc_options())
        .await;
    drop(_maintenance);
    drop(_job_execution_gate);

    match result {
        Ok(report) => {
            let msg = format_request_logs_gc_report_message(&report, 1);
            let _ = state
                .proxy
                .scheduled_job_update_message(job_id, Some(&msg))
                .await;
            let _ = state
                .proxy
                .scheduled_job_finish(job_id, "success", Some(&msg))
                .await;
            report.completed
        }
        Err(err) => {
            let _ = state
                .proxy
                .scheduled_job_finish(job_id, "error", Some(&err.to_string()))
                .await;
            false
        }
    }
}

async fn record_linuxdo_user_sync_failure(
    state: &AppState,
    provider_user_id: &str,
    attempted_at: i64,
    error: &str,
) {
    let _job_execution_gate = acquire_db_job_execution_gate_for_state(state).await;
    let _maintenance = acquire_db_maintenance_read_gate().await;
    record_linuxdo_user_sync_failure_in_db_window(state, provider_user_id, attempted_at, error)
        .await;
}

async fn record_linuxdo_user_sync_failure_in_db_window(
    state: &AppState,
    provider_user_id: &str,
    attempted_at: i64,
    error: &str,
) {
    if let Err(mark_err) = state
        .proxy
        .record_oauth_account_profile_sync_failure(
            "linuxdo",
            provider_user_id,
            attempted_at,
            error,
        )
        .await
    {
        eprintln!(
            "linuxdo-user-sync: record failure metadata error for {}: {}",
            provider_user_id, mark_err
        );
    }
}

async fn finish_scheduled_job_with_db_gate(
    state: &AppState,
    job_id: i64,
    status: &str,
    message: &str,
) {
    let _job_execution_gate = acquire_db_job_execution_gate_for_state(state).await;
    let _maintenance = acquire_db_maintenance_read_gate().await;
    let _ = state
        .proxy
        .scheduled_job_finish(job_id, status, Some(message))
        .await;
}

async fn run_linuxdo_user_status_sync_job(state: Arc<AppState>) {
    run_linuxdo_user_status_sync_job_with_source(state, TRIGGER_SOURCE_SCHEDULER).await;
}

async fn run_linuxdo_user_status_sync_job_with_source(
    state: Arc<AppState>,
    trigger_source: &'static str,
) {
    let Some(claimed_job) = claim_scheduled_job(
        state.as_ref(),
        LINUXDO_USER_STATUS_SYNC_JOB_TYPE,
        None,
        trigger_source,
        "linuxdo-user-sync",
    )
    .await
    else {
        return;
    };

    run_linuxdo_user_status_sync_claimed_job(state, claimed_job).await;
}

async fn run_linuxdo_user_status_sync_claimed_job(
    state: Arc<AppState>,
    mut claimed_job: ClaimedScheduledJob,
) -> bool {
    if claimed_job._job_execution_gate.is_none() {
        claimed_job._job_execution_gate =
            Some(acquire_db_job_execution_gate_for_state(state.as_ref()).await);
    }

    let job_id = claimed_job.job_id;
    let cfg = &state.linuxdo_oauth;

    let records = {
        let _job_execution_gate = claimed_job
            ._job_execution_gate
            .take()
            .expect("claimed linuxdo job has execution gate");
        let _maintenance = acquire_db_maintenance_read_gate().await;
        if !cfg.is_enabled_and_configured() {
            let _ = state
                .proxy
                .scheduled_job_finish(
                    job_id,
                    "success",
                    Some("attempted=0 success=0 skipped=0 failure=0 reason=linuxdo_oauth_not_configured"),
                )
                .await;
            return true;
        }
        if !cfg.has_refresh_token_crypt_key() {
            let _ = state
                .proxy
                .scheduled_job_finish(
                    job_id,
                    "success",
                    Some("attempted=0 success=0 skipped=0 failure=0 reason=missing_refresh_token_crypt_key"),
                )
                .await;
            return true;
        }

        let records = match state
            .proxy
            .list_oauth_accounts_with_refresh_token("linuxdo")
            .await
        {
            Ok(records) => records,
            Err(err) => {
                let _ = state
                    .proxy
                    .scheduled_job_finish(job_id, "error", Some(&err.to_string()))
                    .await;
                return false;
            }
        };

        if records.is_empty() {
            let _ = state
                .proxy
                .scheduled_job_finish(
                    job_id,
                    "success",
                    Some("attempted=0 success=0 skipped=0 failure=0 reason=no_eligible_accounts"),
                )
                .await;
            return true;
        }

        records
    };

    let client = reqwest::Client::new();
    let attempted = records.len();
    let mut success = 0usize;
    let skipped = 0usize;
    let mut failure = 0usize;
    let mut first_failure: Option<String> = None;

    for record in records {
        let attempted_at = Utc::now().timestamp();
        let record_label = record
            .username
            .as_deref()
            .or(record.name.as_deref())
            .unwrap_or(record.provider_user_id.as_str())
            .to_string();
        let refresh_token = match decrypt_linuxdo_refresh_token(
            cfg,
            &record.refresh_token_ciphertext,
            &record.refresh_token_nonce,
        ) {
            Ok(refresh_token) => refresh_token,
            Err(err) => {
                let message = err.to_string();
                failure += 1;
                first_failure
                    .get_or_insert_with(|| format!("{record_label}: {message}"));
                record_linuxdo_user_sync_failure(
                    state.as_ref(),
                    &record.provider_user_id,
                    attempted_at,
                    &message,
                )
                .await;
                continue;
            }
        };
        let (profile, token_payload) =
            match fetch_linuxdo_profile_from_refresh_token(&client, cfg, &refresh_token).await {
                Ok(result) => result,
                Err(err) => {
                    let message = err.to_string();
                    failure += 1;
                    first_failure
                        .get_or_insert_with(|| format!("{record_label}: {message}"));
                    record_linuxdo_user_sync_failure(
                        state.as_ref(),
                        &record.provider_user_id,
                        attempted_at,
                        &message,
                    )
                    .await;
                    continue;
                }
            };

        if profile.provider_user_id != record.provider_user_id {
            let message = LinuxDoSyncError::ProviderUserIdMismatch {
                expected: record.provider_user_id.clone(),
                actual: profile.provider_user_id.clone(),
            }
            .to_string();
            failure += 1;
            first_failure.get_or_insert_with(|| format!("{record_label}: {message}"));
            record_linuxdo_user_sync_failure(
                state.as_ref(),
                &record.provider_user_id,
                attempted_at,
                &message,
            )
            .await;
            continue;
        }

        let mut upsert_failure_message = None;
        {
            let _job_execution_gate = acquire_db_job_execution_gate_for_state(state.as_ref()).await;
            let _maintenance = acquire_db_maintenance_read_gate().await;
            let upsert_result = if let Some(rotated_refresh_token) = token_payload
                .refresh_token
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                match encrypt_linuxdo_refresh_token(cfg, rotated_refresh_token) {
                    Ok(Some((refresh_token_ciphertext, refresh_token_nonce))) => {
                        state
                            .proxy
                            .refresh_oauth_account_profile_with_refresh_token(
                                &profile,
                                &refresh_token_ciphertext,
                                &refresh_token_nonce,
                            )
                            .await
                    }
                    Ok(None) => state.proxy.refresh_oauth_account_profile(&profile).await,
                    Err(err) => {
                        let message = format!("encrypt rotated refresh token error: {err}");
                        failure += 1;
                        first_failure.get_or_insert_with(|| format!("{record_label}: {message}"));
                        record_linuxdo_user_sync_failure_in_db_window(
                            state.as_ref(),
                            &record.provider_user_id,
                            attempted_at,
                            &message,
                        )
                        .await;
                        continue;
                    }
                }
            } else {
                state.proxy.refresh_oauth_account_profile(&profile).await
            };

            if let Err(err) = upsert_result {
                let mut message = format!("upsert oauth account error: {err}");
                if !profile.active
                    && let Err(deactivate_err) = state
                        .proxy
                        .set_user_active_status(&record.user_id, false)
                        .await
                {
                    message.push_str(&format!(
                        "; deactivate local user error: {deactivate_err}"
                    ));
                }
                record_linuxdo_user_sync_failure_in_db_window(
                    state.as_ref(),
                    &record.provider_user_id,
                    attempted_at,
                    &message,
                )
                .await;
                upsert_failure_message = Some(message);
            }
        }

        if let Some(message) = upsert_failure_message {
            failure += 1;
            first_failure.get_or_insert_with(|| format!("{record_label}: {message}"));
            continue;
        }

        {
            let _job_execution_gate = acquire_db_job_execution_gate_for_state(state.as_ref()).await;
            let _maintenance = acquire_db_maintenance_read_gate().await;
            if let Err(err) = state
                .proxy
                .record_oauth_account_profile_sync_success(
                    "linuxdo",
                    &record.provider_user_id,
                    attempted_at,
                )
                .await
            {
                eprintln!(
                    "linuxdo-user-sync: record success metadata error for {} (user_id={}): {}",
                    record.provider_user_id, record.user_id, err
                );
            }
        }

        success += 1;
    }

    let mut message =
        format!("attempted={attempted} success={success} skipped={skipped} failure={failure}");
    if let Some(first_failure) = first_failure {
        message.push_str(&format!(" first_failure={first_failure}"));
    }
    let final_status = if failure > 0 { "error" } else { "success" };
    finish_scheduled_job_with_db_gate(state.as_ref(), job_id, final_status, &message).await;
    final_status == "success"
}

fn spawn_linuxdo_user_status_sync_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            let (hour, minute) = state.linuxdo_oauth.user_sync_time();
            tokio::time::sleep(duration_until_next_local_daily_run(Local::now(), hour, minute))
                .await;
            run_linuxdo_user_status_sync_job(state.clone()).await;
        }
    });
}

async fn run_linuxdo_user_tag_binding_refresh_job(state: Arc<AppState>) {
    run_linuxdo_user_tag_binding_refresh_job_with_source(state, TRIGGER_SOURCE_SCHEDULER).await;
}

async fn run_linuxdo_user_tag_binding_refresh_job_with_source(
    state: Arc<AppState>,
    trigger_source: &'static str,
) {
    let Some(ClaimedScheduledJob {
        job_id,
        _job_execution_gate,
    }) = claim_scheduled_job(
        state.as_ref(),
        LINUXDO_USER_TAG_BINDING_REFRESH_JOB_TYPE,
        None,
        trigger_source,
        "linuxdo-tag-binding-refresh",
    )
    .await
    else {
        return;
    };

    let _maintenance = acquire_db_maintenance_read_gate().await;
    match state.proxy.refresh_linuxdo_user_tag_bindings().await {
        Ok(refreshed) => {
            let msg = format!("refreshed={refreshed}");
            let _ = state
                .proxy
                .scheduled_job_finish(job_id, "success", Some(&msg))
                .await;
        }
        Err(err) => {
            let _ = state
                .proxy
                .scheduled_job_finish(job_id, "error", Some(&err.to_string()))
                .await;
        }
    }
}

fn spawn_linuxdo_user_tag_binding_refresh_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            let wait_secs = {
                let _maintenance = acquire_db_maintenance_read_gate().await;
                state
                    .proxy
                    .linuxdo_user_tag_binding_refresh_wait_secs(twenty_four_hours_secs())
                    .await
            };
            if wait_secs <= 0 {
                let due = {
                    let _maintenance = acquire_db_maintenance_read_gate().await;
                    state
                        .proxy
                        .linuxdo_user_tag_binding_refresh_due(twenty_four_hours_secs())
                        .await
                };
                if due {
                    run_linuxdo_user_tag_binding_refresh_job(state.clone()).await;
                }
                tokio::time::sleep(Duration::from_secs(fifteen_minutes_secs() as u64)).await;
                continue;
            }

            let sleep_secs = wait_secs.min(fifteen_minutes_secs()) as u64;
            tokio::time::sleep(Duration::from_secs(sleep_secs)).await;
        }
    });
}

async fn run_forward_proxy_geo_refresh_job(state: Arc<AppState>) {
    run_forward_proxy_geo_refresh_job_with_source(state, TRIGGER_SOURCE_SCHEDULER).await;
}

async fn run_forward_proxy_geo_refresh_job_with_source(
    state: Arc<AppState>,
    trigger_source: &'static str,
) {
    let Some(ClaimedScheduledJob {
        job_id,
        _job_execution_gate,
    }) = claim_scheduled_job(
        state.as_ref(),
        "forward_proxy_geo_refresh",
        None,
        trigger_source,
        "forward-proxy-geo-refresh",
    )
    .await
    else {
        return;
    };

    let _maintenance = acquire_db_maintenance_read_gate().await;
    match state
        .proxy
        .refresh_forward_proxy_geo_metadata(&state.api_key_ip_geo_origin, true)
        .await
    {
        Ok(refreshed) => {
            let msg = format!("refreshed_candidates={refreshed}");
            let _ = state
                .proxy
                .scheduled_job_finish(job_id, "success", Some(&msg))
                .await;
        }
        Err(err) => {
            let _ = state
                .proxy
                .scheduled_job_finish(job_id, "error", Some(&err.to_string()))
                .await;
        }
    }
}

async fn run_manual_claimed_job(
    state: Arc<AppState>,
    job_type: String,
    key_id: Option<String>,
    mut claimed_job: ClaimedScheduledJob,
) -> bool {
    if job_type == "request_logs_gc" {
        return run_request_logs_gc_catchup_claimed_job(state, claimed_job).await;
    }
    if job_type == LINUXDO_USER_STATUS_SYNC_JOB_TYPE {
        return run_linuxdo_user_status_sync_claimed_job(state, claimed_job).await;
    }

    if claimed_job._job_execution_gate.is_none() {
        claimed_job._job_execution_gate =
            Some(acquire_db_job_execution_gate_for_state(state.as_ref()).await);
    }

    let ClaimedScheduledJob {
        job_id,
        _job_execution_gate,
    } = claimed_job;
    let finish = |state: Arc<AppState>, status: &'static str, message: String| async move {
        let succeeded = status == "success";
        let _ = state
            .proxy
            .scheduled_job_finish(job_id, status, Some(&message))
            .await;
        succeeded
    };

    match job_type.as_str() {
        "quota_sync" => {
            let Some(key_id) = key_id else {
                return finish(state, "error", "missing key_id".to_string()).await;
            };
            drop(_job_execution_gate);
            match sync_key_quota_with_db_job_gate(state.as_ref(), &key_id, "quota_sync/manual")
                .await
            {
                Ok((limit, remaining)) => {
                    finish(state, "success", format!("limit={limit} remaining={remaining}")).await
                }
                Err(ProxyError::QuotaDataMissing { reason }) => {
                    finish(state, "error", format!("quota_data_missing: {reason}")).await
                }
                Err(ProxyError::UsageHttp { status, body }) => {
                    finish(state, "error", format!("usage_http {status}: {body}")).await
                }
                Err(err) => finish(state, "error", err.to_string()).await,
            }
        }
        "token_usage_rollup" => {
            let _maintenance = acquire_db_maintenance_read_gate().await;
            match state.proxy.rollup_token_usage_stats().await {
                Ok((rows, last_ts)) => {
                    let msg = match last_ts {
                        Some(ts) => format!("rows={rows} last_rollup_ts={ts}"),
                        None => format!("rows={rows} last_rollup_ts=none"),
                    };
                    finish(state, "success", msg).await
                }
                Err(err) => finish(state, "error", err.to_string()).await,
            }
        }
        "auth_token_logs_gc" => {
            let _maintenance = acquire_db_maintenance_read_gate().await;
            match state.proxy.gc_auth_token_logs().await {
                Ok(deleted) => finish(state, "success", format!("deleted_rows={deleted}")).await,
                Err(err) => finish(state, "error", err.to_string()).await,
            }
        }
        "mcp_sessions_gc" => {
            let _maintenance = acquire_db_maintenance_read_gate().await;
            match state.proxy.gc_mcp_sessions().await {
                Ok(deleted) => finish(state, "success", format!("deleted_rows={deleted}")).await,
                Err(err) => finish(state, "error", err.to_string()).await,
            }
        }
        "mcp_session_init_backoffs_gc" => {
            let _maintenance = acquire_db_maintenance_read_gate().await;
            match state.proxy.gc_mcp_session_init_backoffs().await {
                Ok(deleted) => finish(state, "success", format!("deleted_rows={deleted}")).await,
                Err(err) => finish(state, "error", err.to_string()).await,
            }
        },
        "request_logs_gc" => unreachable!("request_logs_gc handled above"),
        "linuxdo_user_status_sync" => unreachable!("linuxdo_user_status_sync handled above"),
        "linuxdo_user_tag_binding_refresh" => {
            let _maintenance = acquire_db_maintenance_read_gate().await;
            match state.proxy.refresh_linuxdo_user_tag_bindings().await {
                Ok(refreshed) => finish(state, "success", format!("refreshed={refreshed}")).await,
                Err(err) => finish(state, "error", err.to_string()).await,
            }
        },
        "forward_proxy_geo_refresh" => {
            let _maintenance = acquire_db_maintenance_read_gate().await;
            match state
                .proxy
                .refresh_forward_proxy_geo_metadata(&state.api_key_ip_geo_origin, true)
                .await
            {
                Ok(refreshed) => {
                    finish(state, "success", format!("refreshed_candidates={refreshed}")).await
                }
                Err(err) => finish(state, "error", err.to_string()).await,
            }
        },
        "db_compaction" => run_db_compaction_claimed_job(state, job_id).await,
        _ => finish(state, "error", format!("unsupported manual job type: {job_type}")).await,
    }
}

async fn finish_db_compaction_claimed_job(state: Arc<AppState>, job_id: i64) -> bool {
    let finish = |state: Arc<AppState>, status: &'static str, message: String| async move {
        let succeeded = status == "success";
        let _ = state
            .proxy
            .scheduled_job_finish(job_id, status, Some(&message))
            .await;
        succeeded
    };

    match run_db_compaction_once(state.proxy.sqlite_database_path(), false).await {
        Ok(report) => {
            let message = if report.skipped {
                format!(
                    "skipped=true forced={} reason={} database_bytes_before={} database_bytes_after={} wal_bytes_before={} wal_bytes_after={} reclaimable_bytes_before={} reclaimable_bytes_after={} freelist_before={} freelist_after={}",
                    report.forced,
                    report.reason.unwrap_or_else(|| "unknown".to_string()),
                    report.before.database_bytes,
                    report.after.database_bytes,
                    report.before.wal_bytes,
                    report.after.wal_bytes,
                    report.before.reclaimable_bytes,
                    report.after.reclaimable_bytes,
                    report.before.freelist_count,
                    report.after.freelist_count
                )
            } else {
                format!(
                    "skipped=false forced={} database_bytes_before={} database_bytes_after={} wal_bytes_before={} wal_bytes_after={} reclaimable_bytes_before={} reclaimable_bytes_after={} freelist_before={} freelist_after={}",
                    report.forced,
                    report.before.database_bytes,
                    report.after.database_bytes,
                    report.before.wal_bytes,
                    report.after.wal_bytes,
                    report.before.reclaimable_bytes,
                    report.after.reclaimable_bytes,
                    report.before.freelist_count,
                    report.after.freelist_count
                )
            };
            finish(state, "success", message).await
        }
        Err(err) => finish(state, "error", err.to_string()).await,
    }
}

async fn run_db_compaction_claimed_job(state: Arc<AppState>, job_id: i64) -> bool {
    let _maintenance = acquire_db_maintenance_write_gate().await;
    finish_db_compaction_claimed_job(state, job_id).await
}

fn spawn_db_compaction_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut next_allowed_at = Instant::now();
        loop {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            if Instant::now() < next_allowed_at {
                continue;
            }
            let _job_execution_gate = acquire_db_job_execution_gate_for_state(state.as_ref()).await;
            let _maintenance = acquire_db_maintenance_write_gate().await;
            let stats = match state.proxy.sqlite_db_stats().await {
                Ok(stats) => stats,
                Err(err) => {
                    eprintln!("db-compaction: stats error: {err}");
                    continue;
                }
            };
            if stats.reclaimable_bytes < DB_COMPACTION_MIN_RECLAIMABLE_BYTES
                || stats.reclaimable_ratio < DB_COMPACTION_MIN_RECLAIMABLE_RATIO
            {
                continue;
            }
            let job_id = match state
                .proxy
                .scheduled_job_claim("db_compaction", TRIGGER_SOURCE_AUTO, None, 1)
                .await
            {
                Ok(Some(id)) => id,
                Ok(None) => {
                    eprintln!("db-compaction: job already running; skip trigger");
                    continue;
                }
                Err(err) => {
                    eprintln!("db-compaction: start job error: {err}");
                    continue;
                }
            };
            let succeeded = finish_db_compaction_claimed_job(state.clone(), job_id).await;
            if succeeded {
                next_allowed_at = Instant::now() + Duration::from_secs(DB_COMPACTION_COOLDOWN_SECS);
            }
        }
    });
}

fn spawn_forward_proxy_geo_refresh_scheduler(state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let wait_secs = {
                let _maintenance = acquire_db_maintenance_read_gate().await;
                state
                    .proxy
                    .forward_proxy_geo_refresh_wait_secs(twenty_four_hours_secs())
                    .await
            };
            if wait_secs <= 0 {
                let due = {
                    let _maintenance = acquire_db_maintenance_read_gate().await;
                    state
                        .proxy
                        .forward_proxy_geo_refresh_due(twenty_four_hours_secs())
                        .await
                };
                if due {
                    run_forward_proxy_geo_refresh_job(state.clone()).await;
                }
                tokio::time::sleep(Duration::from_secs(
                    forward_proxy_geo_refresh_recheck_secs() as u64,
                ))
                .await;
                continue;
            }

            let sleep_secs = wait_secs.min(forward_proxy_geo_refresh_recheck_secs()) as u64;
            tokio::time::sleep(Duration::from_secs(sleep_secs)).await;
        }
    })
}

fn spawn_forward_proxy_maintenance_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            {
                let _maintenance = acquire_db_maintenance_read_gate().await;
                if let Err(err) = state.proxy.maybe_run_forward_proxy_maintenance().await {
                    eprintln!("forward-proxy-maintenance: {err}");
                }
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}
