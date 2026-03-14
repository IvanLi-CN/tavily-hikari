fn random_delay_secs() -> u64 {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    rng.gen_range(0..=300)
}

fn twenty_four_hours_secs() -> i64 {
    24 * 60 * 60
}

fn spawn_quota_sync_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            // Initial cycle runs immediately on startup
            let keys = match state
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
                let delay = random_delay_secs();
                tokio::time::sleep(Duration::from_secs(delay)).await;
                let job_id = match state
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
                match state.proxy.sync_key_quota(&key_id, &state.usage_base).await {
                    Ok((limit, remaining)) => {
                        let msg = format!("limit={limit} remaining={remaining}");
                        let _ = state
                            .proxy
                            .scheduled_job_finish(job_id, "success", Some(&msg))
                            .await;
                    }
                    Err(ProxyError::QuotaDataMissing { reason }) => {
                        let msg = format!("quota_data_missing: {reason}");
                        let _ = state
                            .proxy
                            .scheduled_job_finish(job_id, "error", Some(&msg))
                            .await;
                    }
                    Err(ProxyError::UsageHttp { status, body }) => {
                        let msg = format!("usage_http {status}: {body}");
                        let _ = state
                            .proxy
                            .scheduled_job_finish(job_id, "error", Some(&msg))
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

            // Sleep one hour before next cycle
            tokio::time::sleep(Duration::from_secs(3600)).await;
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

fn spawn_request_logs_gc_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        // Schedule: daily at configured local time.
        loop {
            let (hour, minute) = effective_request_logs_gc_at();

            let now = Local::now();
            let today = now.date_naive();
            let scheduled_naive = today
                .and_hms_opt(hour, minute, 0)
                .unwrap_or_else(|| today.and_hms_opt(7, 0, 0).expect("valid default time"));
            let scheduled_today = match Local.from_local_datetime(&scheduled_naive) {
                chrono::LocalResult::Single(dt) => dt,
                chrono::LocalResult::Ambiguous(dt, _) => dt,
                chrono::LocalResult::None => now,
            };
            let scheduled_next = if scheduled_today > now {
                scheduled_today
            } else {
                // Next day at the configured time.
                let tomorrow = today.succ_opt().unwrap_or_else(|| {
                    today
                        .checked_add_days(chrono::Days::new(1))
                        .unwrap_or(today)
                });
                let next_naive = tomorrow
                    .and_hms_opt(hour, minute, 0)
                    .unwrap_or_else(|| tomorrow.and_hms_opt(7, 0, 0).expect("valid default time"));
                match Local.from_local_datetime(&next_naive) {
                    chrono::LocalResult::Single(dt) => dt,
                    chrono::LocalResult::Ambiguous(dt, _) => dt,
                    chrono::LocalResult::None => now + ChronoDuration::hours(24),
                }
            };

            let sleep_for = (scheduled_next - now)
                .to_std()
                .unwrap_or_else(|_| Duration::from_secs(0));
            tokio::time::sleep(sleep_for).await;

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
