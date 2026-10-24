use super::jobs_and_request_log_retention::{
    RequestLogsRetentionEnvGuard, seed_request_log_for_gc,
};
use super::*;

#[tokio::test]
async fn request_logs_gc_scans_bodies_without_online_schema_work() {
    let db_path = temp_db_path("request-logs-gc-body-index-free-scan");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let index_before: Option<String> = sqlx::query_scalar(
        "SELECT name FROM observability.sqlite_master WHERE type = 'index' AND name = ?",
    )
    .bind("idx_request_logs_body_gc_cursor")
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("read optional body index before online GC");
    assert!(index_before.is_none());
    sqlx::query(
        r#"
        INSERT INTO observability.request_logs (
            method, path, result_status, visibility, created_at, request_body
        ) VALUES ('POST', '/api/tavily/search', 'success', ?, ?, ?)
        "#,
    )
    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
    .bind(Utc::now().timestamp())
    .bind(br#"{"query":"index-pending"}"#.as_slice())
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed body-bearing request log");

    let report = proxy
        .gc_request_logs_with_options(RequestLogsGcOptions {
            batch_size: 1,
            max_batches: 1,
            max_runtime_secs: 1,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("online body GC must use its bounded time-index scan");

    assert!(report.scanned_body_candidates >= 1);
    let index_after: Option<String> = sqlx::query_scalar(
        "SELECT name FROM observability.sqlite_master WHERE type = 'index' AND name = ?",
    )
    .bind("idx_request_logs_body_gc_cursor")
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("read optional body index after online GC");
    assert!(
        index_after.is_none(),
        "online GC must not build schema indexes"
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn request_logs_gc_does_not_hydrate_bodyless_cursor_windows() {
    let db_path = temp_db_path("request-logs-gc-bodyless-cursor-window");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    sqlx::query(
        r#"
        WITH RECURSIVE candidates(id) AS (
            VALUES(1)
            UNION ALL
            SELECT id + 1 FROM candidates WHERE id < 64
        )
        INSERT INTO observability.request_logs (
            method, path, result_status, visibility, created_at
        )
        SELECT 'POST', '/api/tavily/search', 'success', ?, ? + id
        FROM candidates
        "#,
    )
    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
    .bind(Utc::now().timestamp())
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed bodyless cursor window");

    let report = proxy
        .gc_request_logs_with_options(RequestLogsGcOptions {
            batch_size: 1,
            max_batches: 1,
            max_runtime_secs: 1,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("bounded body scan completes without user retention reads");

    assert_eq!(report.scanned_body_candidates, 64);
    assert!(report.body_scan_cursor_advanced);
    assert_eq!(report.unique_retention_users, 0);
    assert_eq!(report.retention_context_cache_hits, 0);
    assert_eq!(report.cleaned_request_log_bodies, 0);
    assert!(
        report.has_more,
        "the cursor records bounded bodyless progress"
    );

    let resumed = proxy
        .gc_request_logs_with_options(RequestLogsGcOptions {
            batch_size: 1,
            max_batches: 1,
            max_runtime_secs: 1,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("cursor resumes after the bounded bodyless window");
    assert_eq!(resumed.scanned_body_candidates, 0);
    assert!(!resumed.body_scan_cursor_advanced);
    assert!(resumed.completed);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn request_logs_gc_completed_bodyless_scan_is_not_durable_progress_when_blocked() {
    let lock = env_lock();
    let _env_lock = lock.lock().await;
    let _retention_guard = RequestLogsRetentionEnvGuard::set_32_days();
    let db_path = temp_db_path("request-logs-gc-blocked-bodyless-scan");
    let proxy = TavilyProxy::with_endpoint(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_path.to_string_lossy(),
    )
    .await
    .expect("create proxy");
    let now = Utc::now().timestamp();
    seed_request_log_for_gc(
        &proxy.key_store.pool,
        now - 40 * SECS_PER_DAY,
        "/api/tavily/search",
    )
    .await;
    sqlx::query("INSERT INTO request_logs (method, path, result_status, visibility, created_at) VALUES ('POST', '/api/tavily/search', 'success', 'visible', ?)")
        .bind(now).execute(&proxy.key_store.pool).await.expect("seed retained bodyless log");
    for _ in 0..2 {
        let report = proxy
            .gc_request_logs_with_options(RequestLogsGcOptions {
                batch_size: 100,
                max_batches: 5,
                max_runtime_secs: 20,
                inter_batch_sleep_ms: 0,
            })
            .await
            .expect("scan blocked GC pass");
        assert_eq!(report.scanned_body_candidates, 1);
        assert!(
            !report.body_scan_cursor_advanced,
            "a repeated terminal scan does not advance a retained cursor"
        );
        assert_eq!(
            report.cleaned_request_log_bodies
                + report.deleted_request_logs
                + report.deleted_rollups,
            0
        );
        assert_eq!(report.progress_status, "incomplete_blocked_integrity");
        let cursor: Option<String> = sqlx::query_scalar(
            "SELECT value FROM meta WHERE key = 'request_log_body_gc_cursor_v1'",
        )
        .fetch_optional(&proxy.key_store.pool)
        .await
        .expect("read retained cursor");
        assert!(cursor.is_none());
    }
}

#[tokio::test]
async fn request_logs_gc_stops_after_an_unsealed_day_without_repeating_work() {
    let lock = env_lock();
    let _env_lock = lock.lock().await;
    let _retention_guard = RequestLogsRetentionEnvGuard::set_32_days();
    let db_path = temp_db_path("request-logs-gc-unsealed-day-single-slice");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let old_ts = Utc::now().timestamp() - 40 * SECS_PER_DAY;
    let old_id = seed_request_log_for_gc(&proxy.key_store.pool, old_ts, "/api/tavily/search").await;
    seed_request_log_rollup_for_gc(&proxy.key_store.pool, old_ts).await;

    let report = proxy
        .gc_request_logs_with_options(RequestLogsGcOptions {
            batch_size: 1,
            max_batches: 5,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("run request logs gc against an unsealed day");

    assert_eq!(report.batches, 1, "an unsealed day ends the current slice");
    assert_eq!(report.progress_status, "incomplete_blocked_integrity");
    assert!(report.has_more);
    assert!(!report.completed);
    assert_eq!(report.deleted_request_logs, 0);
    assert_eq!(report.deleted_rollups, 0);
    let retained_id: Option<i64> =
        sqlx::query_scalar("SELECT id FROM observability.request_logs WHERE id = ?")
            .bind(old_id)
            .fetch_optional(&proxy.key_store.pool)
            .await
            .expect("read retained unsealed request log");
    assert_eq!(retained_id, Some(old_id));

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn request_logs_gc_remains_queued_after_process_restart() {
    let db_path = temp_db_path("request-logs-gc-restart-recovery");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let job_id = proxy
        .scheduled_job_claim("request_logs_gc", "auto", None, 1)
        .await
        .expect("claim request-log GC job")
        .expect("request-log GC job created");

    proxy
        .abandon_active_scheduled_jobs()
        .await
        .expect("apply process-restart recovery");

    let row: (String, Option<i64>, Option<i64>, Option<String>) = sqlx::query_as(
        "SELECT status, started_at, finished_at, message FROM scheduled_jobs WHERE id = ?",
    )
    .bind(job_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read request-log GC restart state");
    assert_eq!(row.0, "queued");
    assert!(row.1.is_none(), "requeued job must not retain started_at");
    assert!(row.2.is_none(), "requeued job must not become terminal");
    assert!(
        row.3
            .as_deref()
            .is_some_and(|message| message.contains("process_restart")),
        "requeued job should retain a restart deferral marker: {:?}",
        row.3
    );

    sqlx::query("UPDATE scheduled_jobs SET available_at = 0 WHERE id = ?")
        .bind(job_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("let restart backoff elapse");
    let queued = proxy
        .fetch_queued_scheduled_jobs(16)
        .await
        .expect("fetch requeued request-log GC job");
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].id, job_id);
    assert_eq!(queued[0].job_type, "request_logs_gc");

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn request_logs_gc_admission_retry_uses_controlled_backend_clock() {
    let now = 1_700_010_123_i64;
    let (backend_time, clock) = BackendTime::manual_from_ts(now);
    let db_path = temp_db_path("request-logs-gc-admission-controlled-clock");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time,
    )
    .await
    .expect("create controlled-clock request-log GC proxy");

    let initial = proxy
        .scheduled_job_enqueue("request_logs_gc", "auto", None, 1)
        .await
        .expect("enqueue request-log GC");
    let claim = proxy
        .scheduled_job_mark_running(initial.job_id)
        .await
        .expect("claim request-log GC")
        .expect("request-log GC is due");
    let continuation = proxy
        .scheduled_job_finish_and_enqueue_auto_at(
            claim.id,
            claim.claim_generation,
            "request_logs_gc",
            None,
            1,
            Some("deferred=pool_pressure next_retry_secs=5"),
            now + 5,
        )
        .await
        .expect("queue the short admission retry");
    let queued: (i64, i64) =
        sqlx::query_as("SELECT queued_at, available_at FROM scheduled_jobs WHERE id = ?")
            .bind(continuation.job_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read controlled-clock continuation");
    assert_eq!(queued, (now, now + 5));

    clock.set_now_ts(now + 4);
    assert!(
        proxy
            .scheduled_job_mark_running(continuation.job_id)
            .await
            .expect("check early admission retry")
            .is_none(),
        "the five-second admission retry must not run early"
    );
    clock.set_now_ts(now + 5);
    let retry = proxy
        .scheduled_job_mark_running(continuation.job_id)
        .await
        .expect("claim due admission retry")
        .expect("five-second admission retry is due at the controlled boundary");
    proxy
        .scheduled_job_finish_claimed(retry.id, retry.claim_generation, "success", None)
        .await
        .expect("finish controlled-clock retry");

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn blocked_integrity_range_does_not_advance_pending_work_before_probe() {
    let db_path = temp_db_path("integrity-blocked-range-eligibility");
    let proxy = TavilyProxy::with_endpoint(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_path.to_string_lossy(),
    )
    .await
    .expect("create blocked-range eligibility fixture");
    let now = proxy.backend_time().now_ts();
    let day_start = local_day_bucket_start_utc_ts(now - 3 * SECS_PER_DAY);
    let day_end = next_local_day_start_utc_ts(day_start);
    let next_probe_at = now + SECS_PER_FIVE_MINUTES;
    sqlx::query("INSERT INTO dashboard_rollup_integrity_blocked_ranges (day_start, day_end, reason, next_probe_at, source_fence, source_revision, probe_generation, updated_at) VALUES (?, ?, 'source_baseline', ?, NULL, 0, 0, ?)")
        .bind(day_start).bind(day_end).bind(next_probe_at).bind(now).execute(&proxy.key_store.pool).await
        .expect("seed not-yet-due blocked range");
    sqlx::query("INSERT INTO dashboard_rollup_integrity_day_reaudits (bucket_start, bucket_end, cursor, status, updated_at, gc_blocking) VALUES (?, ?, ?, 'pending', ?, 1)")
        .bind(day_start).bind(day_end).bind(day_start).bind(now).execute(&proxy.key_store.pool).await
        .expect("seed blocked day reaudit");
    sqlx::query("INSERT INTO dashboard_rollup_integrity_work_items (range_start, range_end, source_fence, source_version, counts_json, status, priority, updated_at) VALUES (?, ?, 1, 0, '{}', 'pending', 3, ?)")
        .bind(day_start).bind(day_start + SECS_PER_FIVE_MINUTES).bind(now).execute(&proxy.key_store.pool).await
        .expect("seed blocked pending work");

    proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("skip blocked range before its probe is due");
    let work_status: String = sqlx::query_scalar(
        "SELECT status FROM dashboard_rollup_integrity_work_items WHERE range_start = ?",
    )
    .bind(day_start)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read blocked pending work");
    assert_eq!(work_status, "pending");
    let reaudit: (i64, String) = sqlx::query_as(
        "SELECT cursor, status FROM dashboard_rollup_integrity_day_reaudits WHERE bucket_start = ?",
    )
    .bind(day_start)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read blocked day reaudit");
    assert_eq!(reaudit, (day_start, "pending".to_string()));
    let blocked: (i64, i64) = sqlx::query_as("SELECT next_probe_at, probe_generation FROM dashboard_rollup_integrity_blocked_ranges WHERE day_start = ?")
        .bind(day_start).fetch_one(&proxy.key_store.pool).await.expect("read unchanged blocked range");
    assert_eq!(blocked, (next_probe_at, 0));
}

#[tokio::test]
async fn recovery_baseline_deficit_registers_blocked_range() {
    let db_path = temp_db_path("integrity-baseline-deficit-eligibility");
    let db_str = db_path.to_string_lossy().to_string();
    let (backend_time, _) = BackendTime::manual_from_ts(1_700_000_123);
    let store = crate::store::KeyStore::new_with_time(&db_str, backend_time.clone())
        .await
        .expect("create baseline deficit fixture");
    let now = backend_time.now_ts();
    let day_start = local_day_bucket_start_utc_ts(now - 3 * SECS_PER_DAY);
    let counts = DashboardRequestRollupCounts {
        total_requests: 1,
        ..Default::default()
    };
    sqlx::query(
        "INSERT INTO dashboard_rollup_daily_seals (bucket_start, counts_json, verified_at, source_fence, source_version, durable_source_version) VALUES (?, ?, ?, 0, 0, 0)",
    )
    .bind(day_start)
    .bind(serde_json::to_string(&counts).expect("serialize baseline deficit seal"))
    .bind(now)
    .execute(&store.pool)
    .await
    .expect("seed incomplete source baseline");
    sqlx::query(
        "INSERT INTO dashboard_rollup_integrity_blocked_ranges (day_start, day_end, reason, next_probe_at, source_fence, source_revision, probe_generation, updated_at) VALUES (?, ?, 'reaudit_pending', ?, NULL, 0, 0, ?)",
    )
    .bind(day_start)
    .bind(next_local_day_start_utc_ts(day_start))
    .bind(now + 60)
    .bind(now)
    .execute(&store.pool)
    .await
    .expect("seed prior nonbaseline block");

    let source_fence = store
        .prepare_request_statistics_recovery_target(day_start)
        .await
        .expect("prepare baseline deficit target");
    let result = store
        .run_request_statistics_recovery_integrity_slice(
            day_start,
            source_fence,
            tokio::time::Instant::now() + Duration::from_secs(5),
        )
        .await
        .expect("defer incomplete source baseline");
    assert!(matches!(
        result,
        crate::store::DashboardRollupIntegritySlice::Deferred {
            reason: "deleted_source_baseline_unavailable",
            ..
        }
    ));
    let blocked: (String, i64, Option<i64>, i64) = sqlx::query_as(
        "SELECT reason, next_probe_at, source_fence, source_revision FROM dashboard_rollup_integrity_blocked_ranges WHERE day_start = ?",
    )
    .bind(day_start)
    .fetch_one(&store.pool)
    .await
    .expect("read persisted baseline eligibility");
    assert_eq!(blocked.0, "deleted_source_baseline_unavailable");
    assert!(blocked.1 >= now + SECS_PER_FIVE_MINUTES);
    assert_eq!(blocked.2, None);
    assert_eq!(blocked.3, 0);
    drop(store);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn integrity_selects_recovery_work_inside_a_closed_fence() {
    let db_path = temp_db_path("integrity-recovery-non_boundary-fence");
    let db_str = db_path.to_string_lossy().to_string();
    let (backend_time, _) = BackendTime::manual_from_ts(1_700_000_123);
    let store = crate::store::KeyStore::new_with_time(&db_str, backend_time.clone())
        .await
        .expect("create non-boundary recovery fixture");
    let now = backend_time.now_ts();
    let closed = now - now.rem_euclid(SECS_PER_FIVE_MINUTES);
    let range_start = local_day_bucket_start_utc_ts(now - 3 * SECS_PER_DAY);
    sqlx::query(
        "INSERT INTO dashboard_rollup_integrity_state (id, hot_cursor, hot_fence, hot_reaudit_cursor, history_cursor, updated_at) VALUES (1, ?, ?, ?, ?, ?)",
    )
    .bind(closed)
    .bind(closed)
    .bind(closed)
    .bind(closed)
    .bind(now)
    .execute(&store.pool)
    .await
    .expect("pin caught-up integrity state");
    sqlx::query(
        "INSERT INTO dashboard_rollup_integrity_work_items (range_start, range_end, source_fence, source_version, counts_json, status, priority, recovery, updated_at) VALUES (?, ?, 0, 0, '{}', 'pending', 3, 1, ?)",
    )
    .bind(range_start)
    .bind(range_start + SECS_PER_FIVE_MINUTES)
    .bind(now)
    .execute(&store.pool)
    .await
    .expect("seed recovery work");

    store
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("process recovery work after a non-boundary now");
    let status: String = sqlx::query_scalar(
        "SELECT status FROM dashboard_rollup_integrity_work_items WHERE range_start = ?",
    )
    .bind(range_start)
    .fetch_one(&store.pool)
    .await
    .expect("read recovery work status");
    assert_eq!(status, "done");
    drop(store);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn integrity_skips_blocked_reaudit_and_advances_the_next_eligible_day() {
    let db_path = temp_db_path("integrity-blocked-reaudit-selection");
    let db_str = db_path.to_string_lossy().to_string();
    let (backend_time, _) = BackendTime::manual_from_ts(1_700_000_123);
    let store = crate::store::KeyStore::new_with_time(&db_str, backend_time.clone())
        .await
        .expect("create blocked reaudit fixture");
    let now = backend_time.now_ts();
    let closed = now - now.rem_euclid(SECS_PER_FIVE_MINUTES);
    let blocked_day = local_day_bucket_start_utc_ts(now - 4 * SECS_PER_DAY);
    let eligible_day = blocked_day + SECS_PER_DAY;
    sqlx::query(
        "INSERT INTO dashboard_rollup_integrity_state (id, hot_cursor, hot_fence, hot_reaudit_cursor, history_cursor, last_day_reaudit_attempt_at, updated_at) VALUES (1, ?, ?, ?, ?, ?, ?)",
    )
    .bind(closed)
    .bind(closed)
    .bind(closed)
    .bind(closed)
    .bind(now - 60)
    .bind(now)
    .execute(&store.pool)
    .await
    .expect("pin caught-up state for reaudit selection");
    sqlx::query(
        "INSERT INTO dashboard_rollup_integrity_blocked_ranges (day_start, day_end, reason, next_probe_at, source_fence, source_revision, probe_generation, updated_at) VALUES (?, ?, 'source_baseline', ?, NULL, 0, 0, ?)",
    )
    .bind(blocked_day)
    .bind(next_local_day_start_utc_ts(blocked_day))
    .bind(now + SECS_PER_FIVE_MINUTES)
    .bind(now)
    .execute(&store.pool)
    .await
    .expect("seed not-yet-due blocked day");
    for (day, updated_at) in [(blocked_day, now - 2), (eligible_day, now - 1)] {
        sqlx::query(
            "INSERT INTO dashboard_rollup_integrity_day_reaudits (bucket_start, bucket_end, cursor, status, updated_at, gc_blocking) VALUES (?, ?, ?, 'pending', ?, 1)",
        )
        .bind(day)
        .bind(next_local_day_start_utc_ts(day))
        .bind(day)
        .bind(updated_at)
        .execute(&store.pool)
        .await
        .expect("seed pending day reaudit");
    }

    store
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("advance eligible reaudit after blocked day");
    let cursors: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT cursor FROM dashboard_rollup_integrity_day_reaudits WHERE bucket_start = ?), (SELECT cursor FROM dashboard_rollup_integrity_day_reaudits WHERE bucket_start = ?)",
    )
    .bind(blocked_day)
    .bind(eligible_day)
    .fetch_one(&store.pool)
    .await
    .expect("read blocked and eligible cursors");
    assert_eq!(cursors.0, blocked_day);
    assert!(cursors.1 > eligible_day);
    drop(store);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}
