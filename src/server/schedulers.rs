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

const LINUXDO_USER_STATUS_SYNC_JOB_TYPE: &str = "linuxdo_user_status_sync";

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
            let keys = match cold_state
                .proxy
                .list_keys_pending_quota_sync(twenty_four_hours_secs())
                .await
            {
                Ok(list) => list,
                Err(err) => {
                    eprintln!("quota-sync: list pending error: {err}");
                    vec![]
                }
            };

            for key_id in keys {
                let delay = random_delay_secs(300);
                tokio::time::sleep(Duration::from_secs(delay)).await;
                let job_id = match cold_state
                    .proxy
                    .scheduled_job_start("quota_sync", Some(&key_id), 1)
                    .await
                {
                    Ok(id) => id,
                    Err(err) => {
                        eprintln!("quota-sync: start job error: {err}");
                        continue;
                    }
                };
                match cold_state
                    .proxy
                    .sync_key_quota(&key_id, &cold_state.usage_base, "quota_sync")
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
            let keys = match hot_state
                .proxy
                .list_keys_pending_hot_quota_sync(two_hours_secs(), fifteen_minutes_secs())
                .await
            {
                Ok(list) => list,
                Err(err) => {
                    eprintln!("quota-sync-hot: list pending error: {err}");
                    vec![]
                }
            };

            for key_id in keys {
                let delay = random_delay_secs(60);
                tokio::time::sleep(Duration::from_secs(delay)).await;
                let job_id = match hot_state
                    .proxy
                    .scheduled_job_start("quota_sync/hot", Some(&key_id), 1)
                    .await
                {
                    Ok(id) => id,
                    Err(err) => {
                        eprintln!("quota-sync-hot: start job error: {err}");
                        continue;
                    }
                };
                match hot_state
                    .proxy
                    .sync_key_quota(&key_id, &hot_state.usage_base, "quota_sync/hot")
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
            let job_id = match state
                .proxy
                .scheduled_job_start("token_usage_rollup", None, 1)
                .await
            {
                Ok(id) => id,
                Err(err) => {
                    eprintln!("token-usage-rollup: start job error: {err}");
                    tokio::time::sleep(Duration::from_secs(300)).await;
                    continue;
                }
            };

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

            // Run rollup every 5 minutes to keep charts reasonably fresh
            tokio::time::sleep(Duration::from_secs(300)).await;
        }
    });
}

fn spawn_auth_token_logs_gc_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            let job_id = match state
                .proxy
                .scheduled_job_start("auth_token_logs_gc", None, 1)
                .await
            {
                Ok(id) => id,
                Err(err) => {
                    eprintln!("auth-token-logs-gc: start job error: {err}");
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                    continue;
                }
            };

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

            // Run GC once per hour; retention window is enforced inside the proxy.
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
}

fn spawn_mcp_sessions_gc_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            let job_id = match state
                .proxy
                .scheduled_job_start("mcp_sessions_gc", None, 1)
                .await
            {
                Ok(id) => id,
                Err(err) => {
                    eprintln!("mcp-sessions-gc: start job error: {err}");
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                    continue;
                }
            };

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

            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
}

fn spawn_mcp_session_init_backoffs_gc_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            let job_id = match state
                .proxy
                .scheduled_job_start("mcp_session_init_backoffs_gc", None, 1)
                .await
            {
                Ok(id) => id,
                Err(err) => {
                    eprintln!("mcp-session-init-backoffs-gc: start job error: {err}");
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                    continue;
                }
            };

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

            // After we reach the scheduled time, keep retrying until we either run the job
            // successfully or record an error for this run window.
            loop {
                let retention_days = effective_request_logs_retention_days();
                let job_id = match state
                    .proxy
                    .scheduled_job_start("request_logs_gc", None, 1)
                    .await
                {
                    Ok(id) => id,
                    Err(err) => {
                        eprintln!("request-logs-gc: start job error: {err}");
                        tokio::time::sleep(Duration::from_secs(300)).await;
                        continue;
                    }
                };

                match state.proxy.gc_request_logs().await {
                    Ok(deleted) => {
                        let msg = format!("deleted_rows={deleted} retention_days={retention_days}");
                        let _ = state
                            .proxy
                            .scheduled_job_finish(job_id, "success", Some(&msg))
                            .await;
                        break;
                    }
                    Err(err) => {
                        let _ = state
                            .proxy
                            .scheduled_job_finish(job_id, "error", Some(&err.to_string()))
                            .await;
                        break;
                    }
                }
            }
        }
    });
}

async fn record_linuxdo_user_sync_failure(
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

async fn run_linuxdo_user_status_sync_job(state: Arc<AppState>) {
    let job_id = match state
        .proxy
        .scheduled_job_start(LINUXDO_USER_STATUS_SYNC_JOB_TYPE, None, 1)
        .await
    {
        Ok(id) => id,
        Err(err) => {
            eprintln!("linuxdo-user-sync: start job error: {err}");
            return;
        }
    };

    let cfg = &state.linuxdo_oauth;
    if !cfg.is_enabled_and_configured() {
        let _ = state
            .proxy
            .scheduled_job_finish(
                job_id,
                "success",
                Some("attempted=0 success=0 skipped=0 failure=0 reason=linuxdo_oauth_not_configured"),
            )
            .await;
        return;
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
        return;
    }

    let records = match state.proxy.list_oauth_accounts_with_refresh_token("linuxdo").await {
        Ok(records) => records,
        Err(err) => {
            let _ = state
                .proxy
                .scheduled_job_finish(job_id, "error", Some(&err.to_string()))
                .await;
            return;
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
        return;
    }

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
                    record_linuxdo_user_sync_failure(
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

        success += 1;
    }

    let mut message =
        format!("attempted={attempted} success={success} skipped={skipped} failure={failure}");
    if let Some(first_failure) = first_failure {
        message.push_str(&format!(" first_failure={first_failure}"));
    }
    let final_status = if failure > 0 { "error" } else { "success" };
    let _ = state
        .proxy
        .scheduled_job_finish(job_id, final_status, Some(&message))
        .await;
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

async fn run_forward_proxy_geo_refresh_job(state: Arc<AppState>) {
    let job_id = match state
        .proxy
        .scheduled_job_start("forward_proxy_geo_refresh", None, 1)
        .await
    {
        Ok(id) => id,
        Err(err) => {
            eprintln!("forward-proxy-geo-refresh: start job error: {err}");
            return;
        }
    };

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

fn spawn_forward_proxy_geo_refresh_scheduler(state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let wait_secs = state
                .proxy
                .forward_proxy_geo_refresh_wait_secs(twenty_four_hours_secs())
                .await;
            if wait_secs <= 0 {
                if state
                    .proxy
                    .forward_proxy_geo_refresh_due(twenty_four_hours_secs())
                    .await
                {
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
            if let Err(err) = state.proxy.maybe_run_forward_proxy_maintenance().await {
                eprintln!("forward-proxy-maintenance: {err}");
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}
