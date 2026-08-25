use super::*;

#[tokio::test]
async fn request_stats_shutdown_drains_pending_rollups() {
    let db_path = temp_db_path("request-stats-shutdown-drain");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create proxy");
    let created_at = proxy.backend_time().now_ts();

    proxy
        .key_store
        .enqueue_request_stats_rollup_for_test(None, created_at, OUTCOME_SUCCESS)
        .await;
    proxy
        .shutdown_request_stats_coalescer(Duration::from_secs(2))
        .await
        .expect("drain request stats coalescer");

    let persisted: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(total_requests), 0) FROM dashboard_request_rollup_buckets WHERE bucket_secs = 60",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read drained minute rollup");
    assert_eq!(persisted, 1);
}

#[tokio::test]
async fn repair_barrier_discards_fenced_rollups_and_requeues_newer_changes() {
    let coalescer = RequestStatsCoalescer::default();
    let created_at = Utc::now().timestamp() - SECS_PER_FIVE_MINUTES;
    let range_start = created_at - created_at.rem_euclid(SECS_PER_FIVE_MINUTES);
    let range_end = range_start + SECS_PER_FIVE_MINUTES;
    let counts = DashboardRequestRollupCounts {
        total_requests: 1,
        success_count: 1,
        valuable_success_count: 1,
        api_billable: 1,
        ..DashboardRequestRollupCounts::default()
    };

    coalescer
        .begin_dashboard_rollup_repair(range_start, range_end, 10)
        .await;
    coalescer
        .enqueue_request_log_rollups(crate::store::RequestLogRollupInput {
            api_key_id: None,
            auth_token_id: "test-auth-token",
            request_user_id: None,
            request_log_id: Some(10),
            created_at,
            dashboard_counts: counts,
            request_log_catalog_key: None,
        })
        .await;
    assert!(
        !coalescer
            .finish_dashboard_rollup_repair(range_start, true)
            .await
    );
    assert!(
        coalescer
            .state
            .lock()
            .await
            .pending_dashboard_rollups
            .is_empty(),
        "the source replacement already includes a fenced request"
    );

    coalescer
        .begin_dashboard_rollup_repair(range_start, range_end, 10)
        .await;
    coalescer
        .enqueue_request_log_rollups(crate::store::RequestLogRollupInput {
            api_key_id: None,
            auth_token_id: "test-auth-token",
            request_user_id: None,
            request_log_id: Some(11),
            created_at,
            dashboard_counts: counts,
            request_log_catalog_key: None,
        })
        .await;
    assert!(
        coalescer
            .finish_dashboard_rollup_repair(range_start, true)
            .await
    );
    let pending_total: i64 = coalescer
        .state
        .lock()
        .await
        .pending_dashboard_rollups
        .values()
        .map(|value| value.total_requests)
        .sum();
    assert_eq!(pending_total, 2, "minute and day deltas must both requeue");
}

#[tokio::test]
async fn dashboard_window_marks_every_bucket_unverified_before_the_first_audit_state() {
    let db_path = temp_db_path("dashboard-rollup-integrity-initial-gap");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create proxy");

    let window = proxy
        .dashboard_hourly_request_window_at(Utc::now())
        .await
        .expect("load dashboard window before integrity scheduler state");

    assert_eq!(
        window.unverified_bucket_starts.len() as i64,
        window.retained_buckets,
        "a missing integrity state must never render zero-valued buckets as verified"
    );
}

#[tokio::test]
async fn integrity_audits_work_when_daily_seal_verification_is_due() {
    let db_path = temp_db_path("dashboard-rollup-integrity-seal-does-not-starve-audit");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create proxy");
    let now = proxy.backend_time().now_ts();
    let latest_closed = now - now.rem_euclid(SECS_PER_FIVE_MINUTES);
    let range_start = latest_closed - SECS_PER_FIVE_MINUTES;
    insert_visible_dashboard_log(&proxy, range_start + 60).await;

    sqlx::query(
        r#"
        INSERT INTO dashboard_rollup_integrity_state (
            id, hot_cursor, hot_fence, hot_reaudit_cursor, history_cursor,
            last_history_attempt_at, last_seal_attempt_at, updated_at
        ) VALUES (1, ?, ?, ?, ?, ?, NULL, ?)
        "#,
    )
    .bind(range_start)
    .bind(range_start)
    .bind(latest_closed)
    .bind(range_start)
    .bind(now)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("make a seal check due before the next hot slice");

    let result = proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("run due seal verification and audit work together");
    assert_eq!(result.state, "repaired");
    let repaired: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(total_requests), 0) FROM dashboard_request_rollup_buckets WHERE bucket_secs = 60 AND bucket_start >= ? AND bucket_start < ?",
    )
    .bind(range_start)
    .bind(latest_closed)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read repaired hot bucket");
    assert_eq!(repaired, 1);
}

#[tokio::test]
async fn integrity_keeps_initial_hot_scan_ahead_of_history() {
    let db_path = temp_db_path("dashboard-rollup-integrity-hot-before-history");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create proxy");
    let now = proxy.backend_time().now_ts();
    let hot_range_start =
        (now - 10 * SECS_PER_MINUTE).div_euclid(SECS_PER_FIVE_MINUTES) * SECS_PER_FIVE_MINUTES;
    let hot_range_end = hot_range_start + SECS_PER_FIVE_MINUTES;
    let history_cursor = hot_range_start - 10 * SECS_PER_HOUR;
    let history_created_at = history_cursor - 5 * SECS_PER_HOUR;
    insert_visible_dashboard_log(&proxy, hot_range_start + 60).await;
    insert_visible_dashboard_log(&proxy, history_created_at).await;
    sqlx::query(
        r#"
        INSERT INTO dashboard_rollup_integrity_state (
            id, hot_cursor, hot_fence, hot_reaudit_cursor, history_cursor,
            last_history_attempt_at, updated_at
        ) VALUES (1, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(hot_range_start)
    .bind(hot_range_end)
    .bind(hot_range_start)
    .bind(history_cursor)
    .bind(now - 61)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed an overdue history scan while the first hot scan is incomplete");

    let result = proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("run initial hot slice");
    assert_eq!(result.state, "repaired");
    let hot_rollup: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(total_requests), 0) FROM dashboard_request_rollup_buckets WHERE bucket_secs = 60 AND bucket_start >= ? AND bucket_start < ?",
    )
    .bind(hot_range_start)
    .bind(hot_range_end)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read hot repair result");
    assert_eq!(hot_rollup, 1);
    let history_rollup: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(total_requests), 0) FROM dashboard_request_rollup_buckets WHERE bucket_secs = 60 AND bucket_start = ?",
    )
    .bind(history_created_at.div_euclid(SECS_PER_MINUTE) * SECS_PER_MINUTE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read untouched history rollup");
    assert_eq!(history_rollup, 0);
}

#[tokio::test]
async fn integrity_resets_persisted_checkpoint_after_restart() {
    let db_path = temp_db_path("dashboard-rollup-integrity-restart-checkpoint");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create initial proxy");
    let now = proxy.backend_time().now_ts();
    let range_start =
        (now - 10 * SECS_PER_MINUTE).div_euclid(SECS_PER_FIVE_MINUTES) * SECS_PER_FIVE_MINUTES;
    let range_end = range_start + SECS_PER_FIVE_MINUTES;
    pin_integrity_hot_work(&proxy, range_start, range_end).await;
    for offset in 0..501_i64 {
        insert_visible_dashboard_log(
            &proxy,
            range_start + offset.rem_euclid(SECS_PER_FIVE_MINUTES),
        )
        .await;
    }
    let checkpointed = proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("persist a source aggregation checkpoint");
    assert_eq!(checkpointed.state, "deferred");
    sqlx::query(
        "UPDATE request_logs SET business_credits = 9 WHERE created_at >= ? AND created_at < ?",
    )
    .bind(range_start)
    .bind(range_end)
    .execute(&proxy.key_store.pool)
    .await
    .expect("settle source credits during the hard-stop gap");
    proxy
        .shutdown_request_stats_coalescer(Duration::from_secs(2))
        .await
        .expect("stop initial proxy worker");
    drop(proxy);

    let restarted = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("restart proxy and reset persisted checkpoint");
    let first = restarted
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("restart source pagination");
    assert_eq!(first.state, "deferred");
    let repaired = restarted
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("repair from the refreshed source checkpoint");
    assert_eq!(repaired.state, "repaired");
    let credits: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(local_estimated_credits), 0) FROM dashboard_request_rollup_buckets WHERE bucket_secs = 60 AND bucket_start >= ? AND bucket_start < ?",
    )
    .bind(range_start)
    .bind(range_end)
    .fetch_one(&restarted.key_store.pool)
    .await
    .expect("read restarted repair credits");
    assert_eq!(credits, 501 * 9);
}

#[tokio::test]
async fn integrity_ignores_an_inflight_source_mutation_in_another_slice() {
    let db_path = temp_db_path("dashboard-rollup-integrity-range-source-version");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create proxy");
    let now = proxy.backend_time().now_ts();
    let range_start =
        (now - 15 * SECS_PER_MINUTE).div_euclid(SECS_PER_FIVE_MINUTES) * SECS_PER_FIVE_MINUTES;
    let range_end = range_start + SECS_PER_FIVE_MINUTES;
    insert_visible_dashboard_log(&proxy, range_start + 60).await;
    pin_integrity_hot_work(&proxy, range_start, range_end).await;

    let unrelated_mutation = proxy
        .key_store
        .request_stats_coalescer
        .begin_dashboard_rollup_source_mutation(range_end);
    let result = proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("audit a stable slice beside unrelated traffic");
    drop(unrelated_mutation);

    assert_eq!(result.state, "repaired");
}

#[tokio::test]
async fn integrity_restarts_after_a_cancelled_existing_source_mutation() {
    let db_path = temp_db_path("dashboard-rollup-integrity-cancelled-source-mutation");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create proxy");
    let now = proxy.backend_time().now_ts();
    let range_start =
        (now - 10 * SECS_PER_MINUTE).div_euclid(SECS_PER_FIVE_MINUTES) * SECS_PER_FIVE_MINUTES;
    let range_end = range_start + SECS_PER_FIVE_MINUTES;
    insert_visible_dashboard_log(&proxy, range_start + 60).await;
    let source_fence: i64 = sqlx::query_scalar("SELECT MAX(id) FROM request_logs")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read source fence");
    sqlx::query(
        r#"
        INSERT INTO dashboard_rollup_integrity_work_items (
            range_start, range_end, source_fence, source_version, cursor_created_at, cursor_id,
            counts_json, status, updated_at
        ) VALUES (?, ?, ?, 0, NULL, NULL, '{}', 'pending', ?)
        "#,
    )
    .bind(range_start)
    .bind(range_end)
    .bind(source_fence)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed integrity work before source mutation");

    let mutation = proxy
        .key_store
        .request_stats_coalescer
        .begin_dashboard_rollup_source_mutation(range_start);
    sqlx::query("UPDATE request_logs SET business_credits = 9 WHERE id = ?")
        .bind(source_fence)
        .execute(&proxy.key_store.pool)
        .await
        .expect("commit source update before cancellation");
    drop(mutation);

    let result = proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("observe cancelled source mutation");
    assert_eq!(result.state, "deferred");
    let restarted_version: i64 = sqlx::query_scalar(
        "SELECT source_version FROM dashboard_rollup_integrity_work_items WHERE range_start = ?",
    )
    .bind(range_start)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read restarted source version");
    assert_eq!(restarted_version, 1);
}

#[tokio::test]
async fn integrity_prioritizes_hot_slices_over_sealed_day_reaudits() {
    let db_path = temp_db_path("dashboard-rollup-integrity-hot-priority");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create proxy");
    let now = proxy.backend_time().now_ts();
    let hot_fence = now - now.rem_euclid(SECS_PER_FIVE_MINUTES);
    let hot_cursor = hot_fence - SECS_PER_FIVE_MINUTES;
    let day_start = local_day_bucket_start_utc_ts(now - 3 * SECS_PER_DAY);
    let day_end = next_local_day_start_utc_ts(day_start);
    sqlx::query(
        r#"
        INSERT INTO dashboard_rollup_integrity_state (
            id, hot_cursor, hot_fence, hot_reaudit_cursor, history_cursor, updated_at
        ) VALUES (1, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(hot_cursor)
    .bind(hot_fence)
    .bind(hot_cursor)
    .bind(hot_cursor)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed a hot backlog");
    sqlx::query(
        r#"
        INSERT INTO dashboard_rollup_integrity_day_reaudits (
            bucket_start, bucket_end, cursor, status, updated_at
        ) VALUES (?, ?, ?, 'pending', ?)
        "#,
    )
    .bind(day_start)
    .bind(day_end)
    .bind(day_start)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed a sealed day re-audit");

    proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("run hot-priority integrity slice");
    let day_cursor: i64 = sqlx::query_scalar(
        "SELECT cursor FROM dashboard_rollup_integrity_day_reaudits WHERE bucket_start = ?",
    )
    .bind(day_start)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read unchanged sealed day cursor");
    assert_eq!(day_cursor, day_start);
    let advanced_hot_cursor: i64 =
        sqlx::query_scalar("SELECT hot_cursor FROM dashboard_rollup_integrity_state WHERE id = 1")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read advanced hot cursor");
    assert_eq!(advanced_hot_cursor, hot_fence);
}

#[tokio::test]
async fn dashboard_window_marks_unscanned_history_as_unverified() {
    let db_path = temp_db_path("dashboard-rollup-integrity-history-gap");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create proxy");
    let now = proxy.backend_time().now_ts();
    let latest_closed = now - now.rem_euclid(SECS_PER_FIVE_MINUTES);
    let historical_created_at = latest_closed - 30 * SECS_PER_HOUR + 60;
    let history_cursor = latest_closed - SECS_PER_DAY;
    insert_visible_dashboard_log(&proxy, historical_created_at).await;
    sqlx::query(
        r#"
        INSERT INTO dashboard_rollup_integrity_state (
            id, hot_cursor, hot_fence, hot_reaudit_cursor, history_cursor,
            last_history_attempt_at, updated_at
        ) VALUES (1, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(latest_closed)
    .bind(latest_closed)
    .bind(latest_closed)
    .bind(history_cursor)
    .bind(now)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed an unscanned history interval");

    let window = proxy
        .dashboard_hourly_request_window_at(Utc::now())
        .await
        .expect("load dashboard window");
    let historical_bucket =
        historical_created_at.div_euclid(SECS_PER_FIVE_MINUTES) * SECS_PER_FIVE_MINUTES;
    assert!(
        window.unverified_bucket_starts.contains(&historical_bucket),
        "the history range below its cursor must not render as verified"
    );
}

async fn insert_visible_dashboard_log(proxy: &TavilyProxy, created_at: i64) {
    sqlx::query(
        r#"
        INSERT INTO request_logs (
            auth_token_id, method, path, query, status_code, tavily_status_code,
            error_message, result_status, request_kind_key, counts_business_quota,
            business_credits, request_body, response_body, forwarded_headers,
            dropped_headers, visibility, created_at
        ) VALUES (
            NULL, 'GET', '/api/tavily/search', NULL, 200, 200,
            NULL, 'success', 'api:search', 1,
            3, NULL, NULL, '[]', '[]', 'visible', ?
        )
        "#,
    )
    .bind(created_at)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert visible dashboard log");
}

async fn insert_rebalance_recovery_log(proxy: &TavilyProxy, created_at: i64) {
    sqlx::query(
        r#"
        INSERT INTO request_logs (
            auth_token_id, method, path, query, status_code, tavily_status_code,
            error_message, result_status, request_kind_key, counts_business_quota,
            business_credits, gateway_mode, experiment_variant, upstream_operation,
            request_body, response_body, forwarded_headers, dropped_headers,
            visibility, created_at
        ) VALUES (
            NULL, 'POST', '/mcp', NULL, 200, 200,
            NULL, 'success', NULL, NULL,
            NULL, ?, 'rebalance', 'mcp',
            '{"jsonrpc":"2.0","method":"tools/call"}', '{"result":{}}', '[]', '[]',
            'visible', ?
        )
        "#,
    )
    .bind(MCP_GATEWAY_MODE_REBALANCE)
    .bind(created_at)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert rebalance recovery source log");
}

async fn pin_integrity_after_hot_window(proxy: &TavilyProxy, now: i64, history_cursor: i64) {
    let latest_closed = now - now.rem_euclid(SECS_PER_FIVE_MINUTES);
    sqlx::query(
        r#"
        INSERT INTO dashboard_rollup_integrity_state (
            id, hot_cursor, hot_fence, hot_reaudit_cursor, history_cursor,
            last_history_attempt_at, last_day_reaudit_attempt_at, last_seal_attempt_at,
            seal_cursor, updated_at
        ) VALUES (1, ?, ?, ?, ?, ?, ?, ?, NULL, ?)
        ON CONFLICT(id) DO UPDATE SET
            hot_cursor = excluded.hot_cursor,
            hot_fence = excluded.hot_fence,
            hot_reaudit_cursor = excluded.hot_reaudit_cursor,
            history_cursor = excluded.history_cursor,
            last_history_attempt_at = excluded.last_history_attempt_at,
            last_day_reaudit_attempt_at = excluded.last_day_reaudit_attempt_at,
            last_seal_attempt_at = excluded.last_seal_attempt_at,
            seal_cursor = NULL,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(latest_closed)
    .bind(latest_closed)
    .bind(latest_closed)
    .bind(history_cursor)
    .bind(now)
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("pin integrity after hot window");
}

#[tokio::test]
async fn rebalance_rollup_recovery_is_fenced_and_resumable() {
    let db_path = temp_db_path("rebalance-rollup-recovery-fenced");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create proxy");
    let now = proxy.backend_time().now_ts();
    let range_start = local_day_bucket_start_utc_ts(now - 2 * SECS_PER_DAY);
    let recovery_start = range_start + SECS_PER_FIVE_MINUTES;
    for offset in 0..501_i64 {
        insert_rebalance_recovery_log(
            &proxy,
            recovery_start + offset.rem_euclid(SECS_PER_FIVE_MINUTES),
        )
        .await;
    }
    pin_integrity_after_hot_window(&proxy, now, range_start).await;
    sqlx::query(
        r#"
        INSERT INTO dashboard_rollup_rebalance_recovery (
            id, version, status, range_start, range_end, source_fence, cursor,
            last_error, completed_at, updated_at
        ) VALUES (1, 1, 'complete', NULL, NULL, 0, NULL, NULL, ?, ?)
        "#,
    )
    .bind(now)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed the previously false-complete recovery state");
    let matched: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM request_logs WHERE gateway_mode = ? AND experiment_variant = 'rebalance' AND upstream_operation = 'mcp' AND request_kind_key IS NOT NULL",
    )
    .bind(MCP_GATEWAY_MODE_REBALANCE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count matching recovery source logs");
    assert_eq!(matched, 501);

    let first = proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("start fenced recovery slice");
    assert_eq!(first.state, "deferred");
    let integrity_status = proxy
        .key_store
        .dashboard_rollup_integrity_status()
        .await
        .expect("read recovery integrity status");
    assert_eq!(integrity_status.state, "repairing");
    assert!(integrity_status.unverified_bucket_count > 0);
    let recovery_version: i64 =
        sqlx::query_scalar("SELECT version FROM dashboard_rollup_rebalance_recovery WHERE id = 1")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read recovery version");
    assert_eq!(recovery_version, 2);
    let second = proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("resume fenced recovery slice");
    assert_eq!(second.state, "repaired");
    let recovery_status_after_minute_repair: String =
        sqlx::query_scalar("SELECT status FROM dashboard_rollup_rebalance_recovery WHERE id = 1")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read recovery status before day re-audit");
    assert_eq!(recovery_status_after_minute_repair, "pending");
    let third = proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("complete recovery operation");
    assert!(matches!(third.state.as_str(), "verified" | "repaired"));

    let recovered_total: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(total_requests), 0) FROM dashboard_request_rollup_buckets WHERE bucket_secs = 60 AND bucket_start >= ? AND bucket_start < ?",
    )
    .bind(recovery_start)
    .bind(recovery_start + SECS_PER_FIVE_MINUTES)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read recovered rebalance rollup");
    assert_eq!(recovered_total, 501);
    let recovery_status: String =
        sqlx::query_scalar("SELECT status FROM dashboard_rollup_rebalance_recovery WHERE id = 1")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read recovery operation status");
    assert_eq!(recovery_status, "complete");

    proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("verify completed recovery is idempotent");
    let idempotent_total: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(total_requests), 0) FROM dashboard_request_rollup_buckets WHERE bucket_secs = 60 AND bucket_start >= ? AND bucket_start < ?",
    )
    .bind(recovery_start)
    .bind(recovery_start + SECS_PER_FIVE_MINUTES)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read idempotent recovery rollup");
    assert_eq!(idempotent_total, 501);

    drop(proxy);
    let restarted = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("restart proxy after recovery completion");
    restarted
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("verify completed recovery remains resumable after restart");
    let restarted_total: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(total_requests), 0) FROM dashboard_request_rollup_buckets WHERE bucket_secs = 60 AND bucket_start >= ? AND bucket_start < ?",
    )
    .bind(recovery_start)
    .bind(recovery_start + SECS_PER_FIVE_MINUTES)
    .fetch_one(&restarted.key_store.pool)
    .await
    .expect("read restarted recovery rollup");
    assert_eq!(restarted_total, 501);
}

async fn pin_integrity_hot_work(proxy: &TavilyProxy, range_start: i64, range_end: i64) {
    sqlx::query(
        r#"
        INSERT INTO dashboard_rollup_integrity_state (
            id, hot_cursor, hot_fence, history_cursor, seal_cursor, updated_at
        ) VALUES (1, ?, ?, ?, NULL, ?)
        ON CONFLICT(id) DO UPDATE SET
            hot_cursor = excluded.hot_cursor,
            hot_fence = excluded.hot_fence,
            history_cursor = excluded.history_cursor,
            seal_cursor = NULL,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(range_start)
    .bind(range_end)
    .bind(range_start)
    .bind(range_end)
    .execute(&proxy.key_store.pool)
    .await
    .expect("pin integrity work");
}

#[tokio::test]
async fn integrity_pages_dense_source_without_partial_rollup_writes() {
    let db_path = temp_db_path("dashboard-rollup-integrity-pagination");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create proxy");
    let now = proxy.backend_time().now_ts();
    let range_start =
        (now - 10 * SECS_PER_MINUTE).div_euclid(SECS_PER_FIVE_MINUTES) * SECS_PER_FIVE_MINUTES;
    let range_end = range_start + SECS_PER_FIVE_MINUTES;
    pin_integrity_hot_work(&proxy, range_start, range_end).await;
    for offset in 0..501_i64 {
        insert_visible_dashboard_log(
            &proxy,
            range_start + offset.rem_euclid(SECS_PER_FIVE_MINUTES),
        )
        .await;
    }

    let first = proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("first integrity slice");
    assert_eq!(first.state, "deferred");
    let partial_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM dashboard_request_rollup_buckets WHERE bucket_secs = 60 AND bucket_start >= ? AND bucket_start < ?",
    )
    .bind(range_start)
    .bind(range_end)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count partial rollups");
    assert_eq!(
        partial_rows, 0,
        "paged aggregation must not write partial rollups"
    );

    let second = proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("second integrity slice");
    assert_eq!(second.state, "repaired");
    let repaired_total: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(total_requests), 0) FROM dashboard_request_rollup_buckets WHERE bucket_secs = 60 AND bucket_start >= ? AND bucket_start < ?",
    )
    .bind(range_start)
    .bind(range_end)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read repaired total");
    assert_eq!(repaired_total, 501);
}

#[tokio::test]
async fn integrity_restarts_when_an_existing_source_row_changes_during_a_slice() {
    let db_path = temp_db_path("dashboard-rollup-integrity-source-version");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create proxy");
    let now = proxy.backend_time().now_ts();
    let range_start =
        (now - 10 * SECS_PER_MINUTE).div_euclid(SECS_PER_FIVE_MINUTES) * SECS_PER_FIVE_MINUTES;
    let range_end = range_start + SECS_PER_FIVE_MINUTES;
    pin_integrity_hot_work(&proxy, range_start, range_end).await;
    insert_visible_dashboard_log(&proxy, range_start + 60).await;

    let source_mutation = proxy
        .key_store
        .request_stats_coalescer
        .begin_dashboard_rollup_source_mutation(range_start);
    sqlx::query(
        "UPDATE request_logs SET business_credits = 9 WHERE created_at >= ? AND created_at < ?",
    )
    .bind(range_start)
    .bind(range_end)
    .execute(&proxy.key_store.pool)
    .await
    .expect("amend source credits while the audit slice is active");

    let deferred = proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("defer unstable source slice");
    assert_eq!(deferred.state, "deferred");
    let written_minutes: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM dashboard_request_rollup_buckets WHERE bucket_secs = 60 AND bucket_start >= ? AND bucket_start < ?",
    )
    .bind(range_start)
    .bind(range_end)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("inspect uncommitted repair");
    assert_eq!(written_minutes, 0);

    source_mutation.commit().await;
    let refreshed = proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("refresh source-version fence after the mutation commits");
    assert_eq!(refreshed.state, "deferred");
    let repaired = proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("re-read amended source row");
    assert_eq!(repaired.state, "repaired");
    let credits: i64 = sqlx::query_scalar(
        "SELECT local_estimated_credits FROM dashboard_request_rollup_buckets WHERE bucket_secs = 60 AND bucket_start = ?",
    )
    .bind(range_start + 60 - (range_start + 60).rem_euclid(SECS_PER_MINUTE))
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read repaired credit rollup");
    assert_eq!(credits, 9);
    let remaining_gaps: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM dashboard_rollup_integrity_gaps WHERE range_start = ?",
    )
    .bind(range_start)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count stale integrity gaps");
    assert_eq!(
        remaining_gaps, 0,
        "successful verification must clear the gap"
    );
}

#[tokio::test]
async fn request_stats_shutdown_waits_for_an_active_repair_barrier() {
    let db_path = temp_db_path("request-stats-shutdown-repair-barrier");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create proxy");
    let now = proxy.backend_time().now_ts();
    let range_start = now - now.rem_euclid(SECS_PER_FIVE_MINUTES);
    let range_end = range_start + SECS_PER_FIVE_MINUTES;
    let coalescer = proxy.key_store.request_stats_coalescer.clone();
    coalescer
        .begin_dashboard_rollup_repair(range_start, range_end, 0)
        .await;

    let shutdown_proxy = proxy.clone();
    let shutdown = tokio::spawn(async move {
        shutdown_proxy
            .shutdown_request_stats_coalescer(Duration::from_secs(2))
            .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !shutdown.is_finished(),
        "the worker must not stop while a repair holds deferred deltas"
    );

    coalescer
        .finish_dashboard_rollup_repair(range_start, false)
        .await;
    shutdown
        .await
        .expect("join shutdown task")
        .expect("stop worker after repair barrier releases");
}

#[tokio::test]
async fn integrity_defers_when_a_sqlite_writer_holds_the_sidecar() {
    let db_path = temp_db_path("dashboard-rollup-integrity-write-lock");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create proxy");
    let now = proxy.backend_time().now_ts();
    let range_start =
        (now - 10 * SECS_PER_MINUTE).div_euclid(SECS_PER_FIVE_MINUTES) * SECS_PER_FIVE_MINUTES;
    let range_end = range_start + SECS_PER_FIVE_MINUTES;
    pin_integrity_hot_work(&proxy, range_start, range_end).await;
    for offset in 0..501_i64 {
        insert_visible_dashboard_log(
            &proxy,
            range_start + offset.rem_euclid(SECS_PER_FIVE_MINUTES),
        )
        .await;
    }
    proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("persist checkpoint before lock contention");

    let mut lock_conn = proxy
        .key_store
        .pool
        .acquire()
        .await
        .expect("acquire lock connection");
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *lock_conn)
        .await
        .expect("hold sqlite writer lock");
    let started = std::time::Instant::now();
    let result = proxy.run_dashboard_rollup_integrity_slice().await;
    let elapsed = started.elapsed();
    sqlx::query("ROLLBACK")
        .execute(&mut *lock_conn)
        .await
        .expect("release sqlite writer lock");

    assert!(
        result.is_err(),
        "maintenance must defer instead of waiting for a writer"
    );
    assert!(
        elapsed < Duration::from_millis(500),
        "write contention must honor the bounded retry budget; elapsed={elapsed:?}"
    );
    let pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM dashboard_rollup_integrity_work_items WHERE range_start = ? AND status = 'pending'",
    )
    .bind(range_start)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read deferred work item");
    assert_eq!(pending, 1, "failed maintenance must retain its checkpoint");
}

#[tokio::test]
async fn request_log_gc_requires_a_daily_seal_before_deleting_source_rows() {
    let db_path = temp_db_path("dashboard-rollup-integrity-gc-seal");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create proxy");
    let now = proxy.backend_time().now_ts();
    let old_created_at = now - 100 * SECS_PER_DAY;
    let threshold = now - 32 * SECS_PER_DAY;
    insert_visible_dashboard_log(&proxy, old_created_at).await;

    assert_eq!(
        proxy
            .key_store
            .dashboard_rollup_integrity_request_log_gc_cutoff(threshold)
            .await
            .expect("inspect unsealed source day"),
        None
    );

    let day_start = local_day_bucket_start_utc_ts(old_created_at);
    let empty_seal = serde_json::to_string(&DashboardRequestRollupCounts::default())
        .expect("serialize empty day seal");
    sqlx::query(
        "INSERT INTO dashboard_rollup_daily_seals (bucket_start, counts_json, verified_at) VALUES (?, ?, ?)",
    )
    .bind(day_start)
    .bind(empty_seal)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seal source day");
    assert_eq!(
        proxy
            .key_store
            .dashboard_rollup_integrity_request_log_gc_cutoff(threshold)
            .await
            .expect("inspect sealed source day"),
        Some(next_local_day_start_utc_ts(day_start))
    );
}

#[tokio::test]
async fn request_log_gc_ignores_non_visible_days_when_selecting_a_seal() {
    let db_path = temp_db_path("dashboard-rollup-integrity-gc-suppressed-only-day");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create proxy");
    let now = proxy.backend_time().now_ts();
    let threshold = now - 32 * SECS_PER_DAY;
    let suppressed_created_at = threshold - 2 * SECS_PER_DAY;
    let visible_created_at = threshold - SECS_PER_DAY;
    sqlx::query(
        r#"
        INSERT INTO request_logs (
            auth_token_id, method, path, query, status_code, tavily_status_code,
            error_message, result_status, request_kind_key, counts_business_quota,
            business_credits, request_body, response_body, forwarded_headers,
            dropped_headers, visibility, created_at
        ) VALUES (
            NULL, 'GET', '/api/tavily/search', NULL, 200, 200,
            NULL, 'success', 'api:search', 1,
            3, NULL, NULL, '[]', '[]', 'suppressed_retry_shadow', ?
        )
        "#,
    )
    .bind(suppressed_created_at)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert suppressed-only day source log");
    insert_visible_dashboard_log(&proxy, visible_created_at).await;
    let visible_day_start = local_day_bucket_start_utc_ts(visible_created_at);
    let empty_seal = serde_json::to_string(&DashboardRequestRollupCounts::default())
        .expect("serialize visible day seal");
    sqlx::query(
        "INSERT INTO dashboard_rollup_daily_seals (bucket_start, counts_json, verified_at) VALUES (?, ?, ?)",
    )
    .bind(visible_day_start)
    .bind(empty_seal)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seal visible source day");

    assert_eq!(
        proxy
            .key_store
            .dashboard_rollup_integrity_request_log_gc_cutoff(threshold)
            .await
            .expect("select visible day seal for GC"),
        Some(next_local_day_start_utc_ts(visible_day_start))
    );
}

#[tokio::test]
async fn integrity_reaudits_retained_source_when_a_sealed_day_minute_rollup_diverges() {
    let db_path = temp_db_path("dashboard-rollup-integrity-retained-seal-reaudit");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create proxy");
    let now = proxy.backend_time().now_ts();
    let today = local_day_bucket_start_utc_ts(now);
    let day_start = local_day_bucket_start_utc_ts(today - SECS_PER_DAY);
    let day_end = next_local_day_start_utc_ts(day_start);
    let final_range_start = day_end - SECS_PER_FIVE_MINUTES;
    let corrupted_minute_start = final_range_start + SECS_PER_MINUTE;
    pin_integrity_hot_work(&proxy, final_range_start, day_end).await;
    for bucket_start in (day_start..day_end).step_by(SECS_PER_FIVE_MINUTES as usize) {
        if bucket_start == final_range_start {
            continue;
        }
        sqlx::query(
            r#"
            INSERT INTO dashboard_rollup_integrity_work_items (
                range_start, range_end, source_fence, cursor_created_at, cursor_id, counts_json, status, updated_at
            ) VALUES (?, ?, 0, NULL, NULL, '{}', 'done', ?)
            "#,
        )
        .bind(bucket_start)
        .bind(bucket_start + SECS_PER_FIVE_MINUTES)
        .bind(now)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed verified source slices for a closed day");
    }
    insert_visible_dashboard_log(&proxy, corrupted_minute_start).await;
    let sealed_result = proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("seal the source-verified day");
    assert_eq!(sealed_result.state, "repaired");
    let seals: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM dashboard_rollup_daily_seals WHERE bucket_start = ?",
    )
    .bind(day_start)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("confirm the day was sealed before corruption");
    assert_eq!(seals, 1);
    let minute_before_corruption: i64 = sqlx::query_scalar(
        "SELECT total_requests FROM dashboard_request_rollup_buckets WHERE bucket_secs = 60 AND bucket_start = ?",
    )
    .bind(corrupted_minute_start)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read the source-verified minute before corruption");
    assert_eq!(minute_before_corruption, 1);

    sqlx::query(
        "UPDATE dashboard_request_rollup_buckets SET total_requests = 999 WHERE bucket_secs = 60 AND bucket_start = ?",
    )
    .bind(corrupted_minute_start)
    .execute(&proxy.key_store.pool)
    .await
    .expect("corrupt a retained-source minute rollup");
    let latest_closed = now - now.rem_euclid(SECS_PER_FIVE_MINUTES);
    sqlx::query(
        r#"
        UPDATE dashboard_rollup_integrity_state
        SET hot_cursor = ?, hot_fence = ?, history_cursor = ?, last_seal_attempt_at = NULL,
            seal_cursor = NULL
        WHERE id = 1
        "#,
    )
    .bind(latest_closed)
    .bind(latest_closed)
    .bind(day_start)
    .execute(&proxy.key_store.pool)
    .await
    .expect("force a seal verification pass");

    proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("enqueue the retained-source day re-audit");
    let reaudits = sqlx::query(
        "SELECT bucket_start, bucket_end, cursor, status FROM dashboard_rollup_integrity_day_reaudits",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("inspect retained-source day re-audit rows");
    assert!(
        !reaudits.is_empty(),
        "seal verification did not retain a day re-audit row"
    );
    let cursor: i64 = sqlx::query_scalar(
        "SELECT cursor FROM dashboard_rollup_integrity_day_reaudits WHERE bucket_start = ? AND status = 'pending'",
    )
    .bind(day_start)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read the durable day re-audit checkpoint");
    assert_eq!(cursor, day_start + SECS_PER_FIVE_MINUTES);
}

#[tokio::test]
async fn integrity_seal_restores_a_corrupted_daily_rollup() {
    let db_path = temp_db_path("dashboard-rollup-integrity-seal");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create proxy");
    let now = proxy.backend_time().now_ts();
    let today = local_day_bucket_start_utc_ts(now);
    let day_start = local_day_bucket_start_utc_ts(today - SECS_PER_DAY);
    let day_end = next_local_day_start_utc_ts(day_start);
    let range_start = day_end - SECS_PER_FIVE_MINUTES;
    pin_integrity_hot_work(&proxy, range_start, day_end).await;
    for bucket_start in (day_start..day_end).step_by(SECS_PER_FIVE_MINUTES as usize) {
        if bucket_start == range_start {
            continue;
        }
        sqlx::query(
            r#"
            INSERT INTO dashboard_rollup_integrity_work_items (
                range_start, range_end, source_fence, cursor_created_at, cursor_id, counts_json, status, updated_at
            ) VALUES (?, ?, 0, NULL, NULL, '{}', 'done', ?)
            "#,
        )
        .bind(bucket_start)
        .bind(bucket_start + SECS_PER_FIVE_MINUTES)
        .bind(now)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed completed day work");
    }
    insert_visible_dashboard_log(&proxy, range_start + 60).await;

    proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("seal day after verified minute work");
    let sealed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM dashboard_rollup_daily_seals WHERE bucket_start = ?",
    )
    .bind(day_start)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read day seal");
    assert_eq!(sealed, 1);

    pin_integrity_hot_work(&proxy, range_start, day_end).await;
    insert_visible_dashboard_log(&proxy, range_start + 120).await;
    proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("repair first slice of sealed day after late source data");
    let preserved_seal_json: String = sqlx::query_scalar(
        "SELECT counts_json FROM dashboard_rollup_daily_seals WHERE bucket_start = ?",
    )
    .bind(day_start)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read preserved day seal");
    let preserved_seal: DashboardRequestRollupCounts =
        serde_json::from_str(&preserved_seal_json).expect("parse preserved day seal");
    assert_eq!(preserved_seal.total_requests, 1);
    let reaudit_status: String = sqlx::query_scalar(
        "SELECT status FROM dashboard_rollup_integrity_day_reaudits WHERE bucket_start = ?",
    )
    .bind(day_start)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read queued day reaudit");
    assert_eq!(reaudit_status, "pending");

    sqlx::query(
        "UPDATE dashboard_rollup_integrity_day_reaudits SET cursor = ? WHERE bucket_start = ?",
    )
    .bind(day_end)
    .bind(day_start)
    .execute(&proxy.key_store.pool)
    .await
    .expect("complete retained day reaudit cursor");
    pin_integrity_hot_work(&proxy, range_start, day_end).await;
    proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("seal retained day after full reaudit");
    let refreshed: i64 = sqlx::query_scalar(
        "SELECT total_requests FROM dashboard_request_rollup_buckets WHERE bucket_secs = 86400 AND bucket_start = ?",
    )
    .bind(day_start)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read refreshed day rollup");
    assert_eq!(refreshed, 2);
    let refreshed_seal_json: String = sqlx::query_scalar(
        "SELECT counts_json FROM dashboard_rollup_daily_seals WHERE bucket_start = ?",
    )
    .bind(day_start)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read refreshed day seal");
    let refreshed_seal: DashboardRequestRollupCounts =
        serde_json::from_str(&refreshed_seal_json).expect("parse refreshed day seal");
    assert_eq!(refreshed_seal.total_requests, 2);

    sqlx::query("DELETE FROM request_logs WHERE created_at >= ? AND created_at < ?")
        .bind(day_start)
        .bind(day_end)
        .execute(&proxy.key_store.pool)
        .await
        .expect("expire source logs before seal-only recovery");

    sqlx::query(
        "UPDATE dashboard_request_rollup_buckets SET total_requests = 999 WHERE bucket_secs = 86400 AND bucket_start = ?",
    )
    .bind(day_start)
    .execute(&proxy.key_store.pool)
    .await
    .expect("corrupt day rollup");
    let latest_closed = now - now.rem_euclid(SECS_PER_FIVE_MINUTES);
    sqlx::query(
        "UPDATE dashboard_rollup_integrity_state SET hot_cursor = ?, hot_fence = ?, history_cursor = ? WHERE id = 1",
    )
    .bind(latest_closed)
    .bind(latest_closed)
    .bind(range_start)
    .execute(&proxy.key_store.pool)
    .await
    .expect("force seal verification pass");

    proxy
        .run_dashboard_rollup_integrity_slice()
        .await
        .expect("verify seal");
    let restored: i64 = sqlx::query_scalar(
        "SELECT total_requests FROM dashboard_request_rollup_buckets WHERE bucket_secs = 86400 AND bucket_start = ?",
    )
    .bind(day_start)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read restored day rollup");
    assert_eq!(restored, 2);
}
