use super::*;

const PUBLIC_METRICS_TEST_NOW: i64 = 1_700_000_000;

async fn public_metrics_proxy(db_str: &str, key: &str) -> TavilyProxy {
    let (backend_time, _) = BackendTime::manual_from_ts(PUBLIC_METRICS_TEST_NOW);
    TavilyProxy::with_options_and_time(
        vec![key.to_string()],
        DEFAULT_UPSTREAM,
        db_str,
        TavilyProxyOptions::from_database_path(db_str),
        backend_time,
    )
    .await
    .expect("proxy created")
}

async fn seed_public_metrics_request_log_floor(proxy: &TavilyProxy, month_start: i64) {
    sqlx::query(
        r#"
        INSERT INTO request_logs (
            api_key_id,
            auth_token_id,
            method,
            path,
            query,
            status_code,
            tavily_status_code,
            error_message,
            result_status,
            request_kind_key,
            request_kind_label,
            request_body,
            response_body,
            forwarded_headers,
            dropped_headers,
            visibility,
            created_at
        ) VALUES (
            NULL,
            NULL,
            'GET',
            '/api/tavily/search',
            NULL,
            500,
            500,
            'floor',
            'error',
            'api:search',
            'API | search',
            NULL,
            NULL,
            '[]',
            '[]',
            'visible',
            ?
        )
        "#,
    )
    .bind(month_start)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert public metrics request log floor");
}

async fn open_write_lock_connection(db_str: &str) -> sqlx::SqliteConnection {
    let lock_options = SqliteConnectOptions::new()
        .filename(db_str)
        .create_if_missing(false)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let mut lock_conn = sqlx::SqliteConnection::connect_with(&lock_options)
        .await
        .expect("open lock connection");
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut lock_conn)
        .await
        .expect("lock writer");
    lock_conn
}

#[tokio::test]
async fn public_success_breakdown_skips_flush_when_no_pending_request_stats() {
    let db_path = temp_db_path("public-success-breakdown-no-pending-flush");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = public_metrics_proxy(&db_str, "tvly-public-success-no-pending").await;

    proxy
        .key_store
        .set_meta_i64(
            META_KEY_REQUEST_STATS_LAST_FLUSHED_AT_V1,
            proxy.backend_time().now_ts(),
        )
        .await
        .expect("set request stats flush watermark");

    let now = proxy.backend_time().now_ts();
    let window = TimeRangeUtc {
        start: now.saturating_sub(300),
        end: now.saturating_add(60),
    };
    let public = proxy
        .success_breakdown(Some(window))
        .await
        .expect("public success breakdown");

    assert_eq!(public.monthly_success, 0);
    assert_eq!(public.daily_success, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn public_success_breakdown_serves_durable_data_before_background_flush() {
    let db_path = temp_db_path("public-success-breakdown-pending-flush");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = public_metrics_proxy(&db_str, "tvly-public-success-pending").await;

    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list key metrics")
        .into_iter()
        .next()
        .expect("seeded key")
        .id;
    let now = proxy.backend_time().now_ts();
    let month_start = start_of_month(proxy.backend_time().now_utc()).timestamp();
    seed_public_metrics_request_log_floor(&proxy, month_start).await;
    let window = TimeRangeUtc {
        start: now.saturating_sub(300),
        end: now.saturating_add(60),
    };

    proxy
        .key_store
        .enqueue_request_stats_rollup_for_test(
            Some(&key_id),
            now.saturating_sub(10),
            OUTCOME_SUCCESS,
        )
        .await;
    let public = proxy
        .success_breakdown(Some(window))
        .await
        .expect("public success breakdown");

    assert_eq!(public.monthly_success, 0);
    assert_eq!(public.daily_success, 0);

    proxy
        .key_store
        .flush_request_stats_writes()
        .await
        .expect("background-equivalent test flush");
    let converged = proxy
        .success_breakdown(Some(window))
        .await
        .expect("durable success breakdown after flush");
    assert_eq!(converged.monthly_success, 1);
    assert_eq!(converged.daily_success, 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn public_request_stats_flush_uses_injected_time_for_persisted_rollups() {
    let db_path = temp_db_path("request-stats-flush-injected-time");
    let db_str = db_path.to_string_lossy().to_string();
    let (backend_time, manual_clock) = BackendTime::manual_from_ts(PUBLIC_METRICS_TEST_NOW);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-request-stats-injected-time".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time,
    )
    .await
    .expect("proxy created");
    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list key metrics")
        .into_iter()
        .next()
        .expect("seeded key")
        .id;

    proxy
        .key_store
        .enqueue_request_stats_rollup_for_test(
            Some(&key_id),
            PUBLIC_METRICS_TEST_NOW - 10,
            OUTCOME_SUCCESS,
        )
        .await;
    let flushed_at = PUBLIC_METRICS_TEST_NOW + 123;
    manual_clock.set_now_ts(flushed_at);
    proxy
        .key_store
        .flush_request_stats_writes()
        .await
        .expect("flush request stats");

    let persisted_updated_at: i64 =
        sqlx::query_scalar("SELECT MAX(updated_at) FROM dashboard_request_rollup_buckets")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read persisted rollup timestamp");
    assert_eq!(persisted_updated_at, flushed_at);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn public_success_breakdown_serves_durable_data_until_mixed_pending_rollups_flush() {
    let db_path = temp_db_path("public-success-breakdown-mixed-pending-window");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = public_metrics_proxy(&db_str, "tvly-public-success-mixed-pending").await;

    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list key metrics")
        .into_iter()
        .next()
        .expect("seeded key")
        .id;
    let now = proxy.backend_time().now_ts();
    let month_start = start_of_month(proxy.backend_time().now_utc()).timestamp();
    seed_public_metrics_request_log_floor(&proxy, month_start).await;
    let day_start = now.saturating_sub(300);
    let window = TimeRangeUtc {
        start: day_start,
        end: now.saturating_add(60),
    };

    proxy
        .key_store
        .set_meta_i64(
            META_KEY_REQUEST_STATS_LAST_FLUSHED_AT_V1,
            day_start.saturating_sub(5),
        )
        .await
        .expect("set request stats flush watermark");

    proxy
        .key_store
        .enqueue_request_stats_rollup_for_test(
            Some(&key_id),
            day_start.saturating_sub(120),
            OUTCOME_SUCCESS,
        )
        .await;
    proxy
        .key_store
        .enqueue_request_stats_rollup_for_test(
            Some(&key_id),
            now.saturating_sub(10),
            OUTCOME_SUCCESS,
        )
        .await;

    let public = proxy
        .success_breakdown(Some(window))
        .await
        .expect("public success breakdown");

    assert_eq!(public.monthly_success, 0);
    assert_eq!(public.daily_success, 0);

    proxy
        .key_store
        .flush_request_stats_writes()
        .await
        .expect("background-equivalent test flush");
    let converged = proxy
        .success_breakdown(Some(window))
        .await
        .expect("durable success breakdown after flush");
    assert_eq!(converged.monthly_success, 2);
    assert_eq!(converged.daily_success, 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn public_success_breakdown_defers_month_only_pending_rollup_to_background_flush() {
    let db_path = temp_db_path("public-success-breakdown-month-only-pending");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = public_metrics_proxy(&db_str, "tvly-public-success-month-only-pending").await;

    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list key metrics")
        .into_iter()
        .next()
        .expect("seeded key")
        .id;
    let now = proxy.backend_time().now_utc();
    let month_start = start_of_month(now).timestamp();
    seed_public_metrics_request_log_floor(&proxy, month_start).await;
    let day_window = server_local_day_window_utc(now.with_timezone(&Local));
    let pending_created_at = day_window.start.saturating_sub(120);
    assert!(pending_created_at >= month_start);

    proxy
        .key_store
        .enqueue_request_stats_rollup_for_test(Some(&key_id), pending_created_at, OUTCOME_SUCCESS)
        .await;

    let public = proxy
        .success_breakdown(Some(day_window))
        .await
        .expect("public success breakdown");

    assert_eq!(public.monthly_success, 0);
    assert_eq!(public.daily_success, 0);

    proxy
        .key_store
        .flush_request_stats_writes()
        .await
        .expect("background-equivalent test flush");
    let converged = proxy
        .success_breakdown(Some(day_window))
        .await
        .expect("durable success breakdown after flush");
    assert_eq!(converged.monthly_success, 1);
    assert_eq!(converged.daily_success, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn public_success_breakdown_flushes_pending_rollup_enqueued_during_flush() {
    let db_path = temp_db_path("public-success-breakdown-concurrent-pending-flush");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = public_metrics_proxy(&db_str, "tvly-public-success-concurrent-pending").await;

    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list key metrics")
        .into_iter()
        .next()
        .expect("seeded key")
        .id;
    let now = proxy.backend_time().now_ts();
    let month_start = start_of_month(proxy.backend_time().now_utc()).timestamp();
    seed_public_metrics_request_log_floor(&proxy, month_start).await;
    let first_created_at = now.saturating_sub(60);
    let second_created_at = now.saturating_sub(10);
    let window = TimeRangeUtc {
        start: now.saturating_sub(300),
        end: now.saturating_add(60),
    };

    proxy
        .key_store
        .enqueue_request_stats_rollup_for_test(Some(&key_id), first_created_at, OUTCOME_SUCCESS)
        .await;

    let store = proxy.key_store.clone();
    let pause = store
        .request_stats_coalescer
        .install_post_flush_pause()
        .await;
    let flush_handle = tokio::spawn(async move { store.flush_request_stats_writes().await });

    tokio::time::timeout(Duration::from_secs(2), pause.arrived.notified())
        .await
        .expect("flush reached post-flush pause");

    assert_eq!(
        proxy
            .key_store
            .request_stats_coalescer
            .pending_oldest_created_at()
            .await,
        None
    );

    proxy
        .key_store
        .enqueue_request_stats_rollup_for_test(Some(&key_id), second_created_at, OUTCOME_SUCCESS)
        .await;

    assert_eq!(
        proxy
            .key_store
            .request_stats_coalescer
            .pending_oldest_created_at()
            .await,
        Some(second_created_at)
    );
    assert_eq!(
        proxy
            .key_store
            .request_stats_coalescer
            .pending_newest_created_at()
            .await,
        Some(second_created_at)
    );

    pause
        .released
        .store(true, std::sync::atomic::Ordering::SeqCst);
    pause.release.notify_waiters();

    flush_handle
        .await
        .expect("flush join")
        .expect("flush request stats");

    let public = proxy
        .success_breakdown(Some(window))
        .await
        .expect("public success breakdown");

    assert_eq!(public.monthly_success, 2);
    assert_eq!(public.daily_success, 2);
    assert_eq!(
        proxy
            .key_store
            .request_stats_coalescer
            .pending_oldest_created_at()
            .await,
        None
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn public_success_breakdown_returns_durable_data_while_flush_is_inflight() {
    let db_path = temp_db_path("public-success-breakdown-inflight-flush-wait");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = public_metrics_proxy(&db_str, "tvly-public-success-inflight-wait").await;

    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list key metrics")
        .into_iter()
        .next()
        .expect("seeded key")
        .id;
    let now = proxy.backend_time().now_ts();
    let month_start = start_of_month(proxy.backend_time().now_utc()).timestamp();
    seed_public_metrics_request_log_floor(&proxy, month_start).await;
    let created_at = now.saturating_sub(10);
    let window = TimeRangeUtc {
        start: now.saturating_sub(300),
        end: now.saturating_add(60),
    };

    proxy
        .key_store
        .enqueue_request_stats_rollup_for_test(Some(&key_id), created_at, OUTCOME_SUCCESS)
        .await;

    let lock_options = SqliteConnectOptions::new()
        .filename(&db_str)
        .create_if_missing(false)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let mut lock_conn = sqlx::SqliteConnection::connect_with(&lock_options)
        .await
        .expect("open lock connection");
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut lock_conn)
        .await
        .expect("lock writer");

    let store = proxy.key_store.clone();
    let flush_handle = tokio::spawn(async move { store.flush_request_stats_writes().await });

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let is_inflight = {
                let state = proxy.key_store.request_stats_coalescer.state.lock().await;
                state.flushing
                    && state.oldest_pending_created_at.is_none()
                    && state.flushing_oldest_created_at == Some(created_at)
                    && state.flushing_newest_created_at == Some(created_at)
            };
            if is_inflight {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("flush entered inflight state");

    let proxy_for_read = proxy.clone();
    let (done_tx, mut done_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let result = proxy_for_read.success_breakdown(Some(window)).await;
        let _ = done_tx.send(result);
    });

    let durable_before_flush = tokio::time::timeout(Duration::from_millis(250), &mut done_rx)
        .await
        .expect("public metrics must not wait for an inflight write")
        .expect("public metrics result channel")
        .expect("durable public success breakdown");
    assert_eq!(durable_before_flush.monthly_success, 0);
    assert_eq!(durable_before_flush.daily_success, 0);

    sqlx::query("ROLLBACK")
        .execute(&mut lock_conn)
        .await
        .expect("release writer lock");
    lock_conn.close().await.expect("close lock connection");

    flush_handle
        .await
        .expect("flush join")
        .expect("flush request stats");

    let public = proxy
        .success_breakdown(Some(window))
        .await
        .expect("durable public success breakdown after flush");
    assert_eq!(public.monthly_success, 1);
    assert_eq!(public.daily_success, 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn admin_summary_falls_back_to_durable_data_when_flush_hits_write_lock() {
    let db_path = temp_db_path("admin-summary-write-lock-fallback");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = public_metrics_proxy(&db_str, "tvly-admin-summary-lock").await;

    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list key metrics")
        .into_iter()
        .next()
        .expect("seeded key")
        .id;
    let created_at = proxy.backend_time().now_ts().saturating_sub(1);
    proxy
        .key_store
        .enqueue_request_stats_rollup_for_test(Some(&key_id), created_at, OUTCOME_SUCCESS)
        .await;

    let mut lock_conn = open_write_lock_connection(&db_str).await;
    let summary = tokio::time::timeout(Duration::from_secs(1), proxy.summary())
        .await
        .expect("summary should return promptly under write contention")
        .expect("summary fallback should succeed");
    assert_eq!(
        summary.total_requests, 0,
        "fallback should serve durable data before the blocked flush commits"
    );

    sqlx::query("ROLLBACK")
        .execute(&mut lock_conn)
        .await
        .expect("release writer lock");
    lock_conn.close().await.expect("close lock connection");

    let flushed = proxy.key_store.request_stats_coalescer.flushed.clone();
    let flush_complete = flushed.notified();
    proxy.nudge_request_stats_flush().await;
    // The write lock deliberately enters the runtime's recent-contention
    // protection window. The background writer must yield while that window
    // drains, rather than racing a foreground transaction; the no-contention
    // tests retain the nominal two-second persistence contract.
    tokio::time::timeout(Duration::from_secs(8), flush_complete)
        .await
        .expect("background flush should persist after bounded contention recovery");
    let summary_after = proxy.summary().await.expect("summary after lock release");
    assert_eq!(summary_after.total_requests, 1);
    assert_eq!(summary_after.success_count, 1);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn admin_summary_never_flushes_pending_request_stats_from_the_read_path() {
    let db_path = temp_db_path("admin-summary-durable-read-only");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = public_metrics_proxy(&db_str, "tvly-admin-summary-read-only").await;
    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list key metrics")
        .into_iter()
        .next()
        .expect("seeded key")
        .id;
    let pause = proxy
        .key_store
        .request_stats_coalescer
        .install_post_flush_pause()
        .await;
    proxy
        .key_store
        .enqueue_request_stats_rollup_for_test(
            Some(&key_id),
            proxy.backend_time().now_ts().saturating_sub(1),
            OUTCOME_SUCCESS,
        )
        .await;

    let summary = tokio::time::timeout(Duration::from_millis(250), proxy.summary())
        .await
        .expect("durable summary read budget")
        .expect("durable summary");
    assert_eq!(summary.total_requests, 0);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), pause.arrived.notified())
            .await
            .is_err(),
        "the read path must not start a request-stats flush"
    );

    proxy.nudge_request_stats_flush().await;
    tokio::time::timeout(Duration::from_secs(2), pause.arrived.notified())
        .await
        .expect("background flush starts within its nominal cadence");
    pause
        .released
        .store(true, std::sync::atomic::Ordering::SeqCst);
    pause.release.notify_waiters();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if proxy
                .summary_without_flush()
                .await
                .expect("durable summary after background flush")
                .total_requests
                == 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("background flush persists the pending delta");

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn admin_summary_returns_promptly_while_full_budget_flush_is_inflight() {
    let db_path = temp_db_path("admin-summary-inflight-flush-fallback");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = public_metrics_proxy(&db_str, "tvly-admin-summary-inflight").await;

    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list key metrics")
        .into_iter()
        .next()
        .expect("seeded key")
        .id;
    let created_at = proxy.backend_time().now_ts().saturating_sub(1);
    proxy
        .key_store
        .enqueue_request_stats_rollup_for_test(Some(&key_id), created_at, OUTCOME_SUCCESS)
        .await;

    let mut lock_conn = open_write_lock_connection(&db_str).await;
    let store = proxy.key_store.clone();
    let flush_handle = tokio::spawn(async move { store.flush_request_stats_writes().await });

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let is_inflight = {
                let state = proxy.key_store.request_stats_coalescer.state.lock().await;
                state.flushing
                    && state.oldest_pending_created_at.is_none()
                    && state.flushing_oldest_created_at == Some(created_at)
                    && state.flushing_newest_created_at == Some(created_at)
            };
            if is_inflight {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("flush entered inflight state");

    let summary = tokio::time::timeout(Duration::from_secs(1), proxy.summary())
        .await
        .expect("summary should return promptly while full-budget flush is inflight")
        .expect("summary fallback should succeed");
    assert_eq!(
        summary.total_requests, 0,
        "fallback should serve durable data while the inflight full-budget flush is blocked"
    );

    sqlx::query("ROLLBACK")
        .execute(&mut lock_conn)
        .await
        .expect("release writer lock");
    lock_conn.close().await.expect("close lock connection");

    flush_handle
        .await
        .expect("flush join")
        .expect("flush request stats");

    let summary_after = proxy.summary().await.expect("summary after lock release");
    assert_eq!(summary_after.total_requests, 1);
    assert_eq!(summary_after.success_count, 1);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn admin_summary_does_not_start_a_slow_background_flush() {
    let db_path = temp_db_path("admin-summary-slow-successful-flush-fallback");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = public_metrics_proxy(&db_str, "tvly-admin-summary-slow-flush").await;

    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list key metrics")
        .into_iter()
        .next()
        .expect("seeded key")
        .id;
    let created_at = proxy.backend_time().now_ts().saturating_sub(1);
    proxy
        .key_store
        .enqueue_request_stats_rollup_for_test(Some(&key_id), created_at, OUTCOME_SUCCESS)
        .await;

    let pause = proxy
        .key_store
        .request_stats_coalescer
        .install_post_flush_pause()
        .await;
    let summary = tokio::time::timeout(Duration::from_millis(250), proxy.summary())
        .await
        .expect("summary should return durable data promptly")
        .expect("summary read should succeed");
    assert_eq!(summary.total_requests, 0);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), pause.arrived.notified())
            .await
            .is_err(),
        "summary reads must not start the background flush"
    );

    proxy.nudge_request_stats_flush().await;
    tokio::time::timeout(Duration::from_secs(1), pause.arrived.notified())
        .await
        .expect("explicit background flush reached post-flush pause");

    pause
        .released
        .store(true, std::sync::atomic::Ordering::SeqCst);
    pause.release.notify_waiters();

    let summary_after = proxy
        .summary()
        .await
        .expect("summary after background flush");
    assert_eq!(summary_after.total_requests, 1);
    assert_eq!(summary_after.success_count, 1);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn expired_flush_budget_keeps_pending_deltas_for_the_admitted_worker() {
    let db_path = temp_db_path("admin-read-budget-expiry-drain-detach");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = public_metrics_proxy(&db_str, "tvly-admin-read-budget-expiry").await;

    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list key metrics")
        .into_iter()
        .next()
        .expect("seeded key")
        .id;
    proxy
        .key_store
        .enqueue_request_stats_rollup_for_test(
            Some(&key_id),
            proxy.backend_time().now_ts().saturating_sub(1),
            OUTCOME_SUCCESS,
        )
        .await;

    let err = proxy
        .key_store
        .flush_request_stats_writes_with_wait_policy_for_test(
            Duration::from_millis(250),
            Some(tokio::time::Instant::now()),
        )
        .await
        .expect_err("expired admin read budget should return a wait-budget error");
    assert!(
        matches!(err, ProxyError::Other(ref message) if message == "request stats flush wait budget exhausted"),
        "unexpected error after budget expiry: {err}"
    );

    let state = proxy.key_store.request_stats_coalescer.state.lock().await;
    assert!(
        !state.flushing,
        "rejected flush must not leave the coalescer stuck in flushing=true"
    );
    assert!(
        RequestStatsCoalescer::pending_key_count(&state) > 0,
        "expired flush budgets must keep every derived delta for the next admitted worker"
    );

    drop(state);
    proxy
        .key_store
        .flush_request_stats_writes()
        .await
        .expect("admitted flush persists the retained delta");
    let summary_after = proxy
        .summary_without_flush()
        .await
        .expect("summary after admitted flush");
    assert_eq!(summary_after.total_requests, 1);
    assert_eq!(summary_after.success_count, 1);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn request_stats_background_flush_defers_before_pool_acquire_and_preserves_pending_delta() {
    let db_path = temp_db_path("request-stats-background-admission");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = public_metrics_proxy(&db_str, "tvly-request-stats-background-admission").await;
    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list key metrics")
        .into_iter()
        .next()
        .expect("seeded key")
        .id;
    proxy
        .key_store
        .flush_request_stats_writes()
        .await
        .expect("drain setup coalescer state");
    proxy
        .key_store
        .enqueue_request_stats_rollup_for_test(
            Some(&key_id),
            proxy.backend_time().now_ts().saturating_sub(1),
            OUTCOME_SUCCESS,
        )
        .await;
    let pending_before_defer = {
        let state = proxy.key_store.request_stats_coalescer.state.lock().await;
        RequestStatsCoalescer::pending_key_count(&state)
    };

    let first_foreground_connection = proxy
        .key_store
        .pool
        .acquire()
        .await
        .expect("first foreground connection");
    let second_foreground_connection = proxy
        .key_store
        .pool
        .acquire()
        .await
        .expect("second foreground connection");

    let outcome = proxy
        .key_store
        .flush_request_stats_writes_in_background()
        .await
        .expect("bulk admission decision");
    assert!(
        matches!(
            outcome,
            RequestStatsBackgroundFlushOutcome::Deferred(SqliteAdmissionDeferReason::PoolPressure)
        ),
        "bulk persistence must defer before taking the final foreground pool slot"
    );
    let pending = proxy.key_store.request_stats_coalescer.state.lock().await;
    assert_eq!(
        RequestStatsCoalescer::pending_key_count(&pending),
        pending_before_defer,
        "deferred flush must preserve every logical delta already queued in the coalescer"
    );
    drop(pending);
    drop(second_foreground_connection);
    drop(first_foreground_connection);

    let outcome = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let outcome = proxy
                .key_store
                .flush_request_stats_writes_in_background()
                .await
                .expect("background flush admission decision after foreground capacity returns");
            if matches!(outcome, RequestStatsBackgroundFlushOutcome::Flushed) {
                break outcome;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("background flush should resume once foreground capacity returns");
    assert_eq!(outcome, RequestStatsBackgroundFlushOutcome::Flushed);
    let summary = proxy
        .summary_without_flush()
        .await
        .expect("durable summary after admitted flush");
    assert_eq!(summary.total_requests, 1);
    assert_eq!(summary.success_count, 1);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn request_stats_background_flush_defers_on_writer_lock_and_requeues_exact_delta() {
    let db_path = temp_db_path("request-stats-background-writer-lock");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-request-stats-background-writer-lock".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list key metrics")
        .into_iter()
        .next()
        .expect("seeded key")
        .id;
    proxy
        .key_store
        .flush_request_stats_writes()
        .await
        .expect("drain setup coalescer state");
    proxy
        .key_store
        .enqueue_request_stats_rollup_for_test(
            Some(&key_id),
            proxy.backend_time().now_ts().saturating_sub(1),
            OUTCOME_SUCCESS,
        )
        .await;
    let pending_before_defer = {
        let state = proxy.key_store.request_stats_coalescer.state.lock().await;
        RequestStatsCoalescer::pending_key_count(&state)
    };

    let mut lock_conn = open_write_lock_connection(&db_str).await;
    let started = Instant::now();
    let outcome = proxy
        .key_store
        .flush_request_stats_writes_in_background()
        .await
        .expect("writer contention is a typed background defer");
    assert!(
        matches!(
            outcome,
            RequestStatsBackgroundFlushOutcome::Deferred(
                SqliteAdmissionDeferReason::RecentContention
            )
        ),
        "unexpected background flush outcome: {outcome:?}"
    );
    assert!(
        started.elapsed() < Duration::from_millis(250),
        "background flush must yield before the writer timeout (elapsed={:?})",
        started.elapsed()
    );
    let pending = proxy.key_store.request_stats_coalescer.state.lock().await;
    assert_eq!(
        RequestStatsCoalescer::pending_key_count(&pending),
        pending_before_defer,
        "a deferred chunk must return every internal rollup delta to the coalescer"
    );
    drop(pending);

    sqlx::query("ROLLBACK")
        .execute(&mut lock_conn)
        .await
        .expect("release writer lock");
    lock_conn.close().await.expect("close lock connection");
    proxy
        .key_store
        .flush_request_stats_writes()
        .await
        .expect("requeued delta persists after the writer lock releases");
    let summary = proxy
        .summary_without_flush()
        .await
        .expect("read durable summary");
    assert_eq!(summary.total_requests, 1);
    assert_eq!(summary.success_count, 1);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn request_stats_chunked_flush_preserves_exact_deltas() {
    let db_path = temp_db_path("request-stats-chunked-flush-exact-deltas");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = public_metrics_proxy(&db_str, "tvly-request-stats-chunked-flush").await;
    let created_at = proxy.backend_time().now_ts().saturating_sub(1);

    for index in 0..501 {
        let bucket_created_at = created_at + (i64::from(index) * SECS_PER_FIVE_MINUTES);
        proxy
            .key_store
            .enqueue_request_stats_rollup_for_test(None, bucket_created_at, OUTCOME_SUCCESS)
            .await;
    }

    proxy
        .key_store
        .flush_request_stats_writes()
        .await
        .expect("chunked request stats flush");

    let durable_total: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(total_requests), 0)
        FROM dashboard_request_rollup_buckets
        WHERE bucket_secs = ?
        "#,
    )
    .bind(SECS_PER_MINUTE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read chunked durable rollups");
    assert_eq!(
        durable_total, 501,
        "every logical delta persists exactly once"
    );

    let state = proxy.key_store.request_stats_coalescer.state.lock().await;
    assert_eq!(
        RequestStatsCoalescer::pending_key_count(&state),
        0,
        "committed chunks must not be replayed from the coalescer"
    );
    drop(state);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn request_stats_background_slice_bounds_work_and_requeues_the_tail() {
    const BACKGROUND_SLICE_MAX_KEYS: i64 = 250;
    let db_path = temp_db_path("request-stats-background-slice-tail");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = public_metrics_proxy(&db_str, "tvly-request-stats-background-slice").await;
    let created_at = proxy.backend_time().now_ts().saturating_sub(1);

    for index in 0..501 {
        let bucket_created_at = created_at + (i64::from(index) * SECS_PER_FIVE_MINUTES);
        proxy
            .key_store
            .enqueue_request_stats_rollup_for_test(None, bucket_created_at, OUTCOME_SUCCESS)
            .await;
    }
    let pending_before_slice = {
        let pending = proxy.key_store.request_stats_coalescer.state.lock().await;
        RequestStatsCoalescer::pending_key_count(&pending)
    };

    proxy
        .key_store
        .flush_request_stats_background_slice_for_test()
        .await
        .expect("bounded background admission flushes a finite transaction group");

    let durable_total: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(total_requests), 0) FROM dashboard_request_rollup_buckets WHERE bucket_secs = ?",
    )
    .bind(SECS_PER_MINUTE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read committed background slice");
    assert!(
        (1..=BACKGROUND_SLICE_MAX_KEYS).contains(&durable_total),
        "the background slice commits no more than its bounded logical-key budget",
    );

    let pending = proxy.key_store.request_stats_coalescer.state.lock().await;
    assert!(
        (1..pending_before_slice).contains(&RequestStatsCoalescer::pending_key_count(&pending)),
        "the uncommitted tail remains durable in the coalescer for the next tick",
    );
    drop(pending);

    proxy
        .key_store
        .flush_request_stats_writes()
        .await
        .expect("the explicit drain persists the returned tail");
    let final_total: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(total_requests), 0) FROM dashboard_request_rollup_buckets WHERE bucket_secs = ?",
    )
    .bind(SECS_PER_MINUTE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read fully drained rollups");
    assert_eq!(final_total, 501, "each logical delta persists exactly once");

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn admitted_flush_drains_followup_batches_before_reporting_fresh() {
    let db_path = temp_db_path("admin-read-followup-batch-fresh");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = public_metrics_proxy(&db_str, "tvly-admin-read-followup-batch").await;

    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list key metrics")
        .into_iter()
        .next()
        .expect("seeded key")
        .id;
    let now = proxy.backend_time().now_ts();
    let first_created_at = now.saturating_sub(60);
    let second_created_at = now.saturating_sub(10);
    proxy
        .key_store
        .enqueue_request_stats_rollup_for_test(Some(&key_id), first_created_at, OUTCOME_SUCCESS)
        .await;

    let pause = proxy
        .key_store
        .request_stats_coalescer
        .install_post_flush_pause()
        .await;
    let store = proxy.key_store.clone();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    let flush_handle = tokio::spawn(async move {
        store
            .flush_request_stats_writes_with_wait_policy_for_test(
                Duration::from_millis(250),
                Some(deadline),
            )
            .await
    });

    tokio::time::timeout(Duration::from_secs(1), pause.arrived.notified())
        .await
        .expect("first drained batch reached post-flush pause");

    proxy
        .key_store
        .enqueue_request_stats_rollup_for_test(Some(&key_id), second_created_at, OUTCOME_SUCCESS)
        .await;

    pause
        .released
        .store(true, std::sync::atomic::Ordering::SeqCst);
    pause.release.notify_waiters();

    tokio::time::timeout(Duration::from_secs(2), flush_handle)
        .await
        .expect("admitted flush should finish within the bounded worker window")
        .expect("admitted flush join")
        .expect("admitted flush should drain the follow-up batch too");

    let summary_after = proxy
        .summary_without_flush()
        .await
        .expect("summary after draining follow-up batch");
    assert_eq!(summary_after.total_requests, 2);
    assert_eq!(summary_after.success_count, 2);
    assert_eq!(
        proxy
            .key_store
            .request_stats_coalescer
            .pending_oldest_created_at()
            .await,
        None
    );
    assert_eq!(
        proxy
            .key_store
            .request_stats_coalescer
            .pending_newest_created_at()
            .await,
        None
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn admin_user_rankings_snapshot_returns_promptly_when_flush_hits_write_lock() {
    let db_path = temp_db_path("admin-user-rankings-write-lock-fallback");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = public_metrics_proxy(&db_str, "tvly-admin-rankings-lock").await;
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "user-rankings-fallback".to_string(),
            username: Some("rankings_fallback".to_string()),
            name: Some("Rankings Fallback".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("create rankings user");
    let created_at = proxy
        .backend_time()
        .now_ts()
        .saturating_sub(SECS_PER_FIVE_MINUTES + 1);

    sqlx::query(
        r#"
        INSERT INTO request_logs (
            method,
            path,
            status_code,
            tavily_status_code,
            result_status,
            request_kind_key,
            request_kind_label,
            response_body,
            created_at,
            request_user_id,
            visibility,
            remote_addr,
            client_ip,
            client_ip_source,
            client_ip_trusted,
            ip_headers
        ) VALUES (
            'POST', '/api/tavily/search', 200, 200, 'success', 'api:search', 'HTTP | search', NULL, ?, ?, ?, NULL, ?, 'cf-connecting-ip', 1, NULL
        )
        "#,
    )
    .bind(created_at)
    .bind(&user.user_id)
    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
    .bind("198.51.100.24")
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert durable request log for rankings fallback");

    let (cached_snapshot, cached_stale) = proxy
        .user_rankings_snapshot_with_stale_flag()
        .await
        .expect("initial rankings snapshot");
    assert!(!cached_stale);
    assert_eq!(
        cached_snapshot
            .last24h
            .unique_ip_top
            .first()
            .map(|row| (row.user.user_id.as_str(), row.value)),
        Some((user.user_id.as_str(), 1)),
        "initial cache warm should include the durable unique-ip ranking"
    );

    proxy
        .key_store
        .enqueue_request_stats_rollup_for_user_for_test(&user.user_id, created_at, OUTCOME_SUCCESS)
        .await;

    let mut lock_conn = open_write_lock_connection(&db_str).await;
    let (snapshot, stale) = tokio::time::timeout(
        Duration::from_secs(1),
        proxy.user_rankings_snapshot_with_stale_flag(),
    )
    .await
    .expect("rankings snapshot should return promptly under write contention")
    .expect("rankings fallback should succeed");
    assert!(stale);
    assert!(snapshot.generated_at > 0);
    assert!(snapshot.last24h.primary_success_top.is_empty());
    assert!(snapshot.last24h.business_credits_top.is_empty());
    assert_eq!(
        snapshot
            .last24h
            .unique_ip_top
            .first()
            .map(|row| (row.user.user_id.as_str(), row.value)),
        Some((user.user_id.as_str(), 1)),
        "durable fallback should still surface already committed unique-ip rankings"
    );

    sqlx::query("ROLLBACK")
        .execute(&mut lock_conn)
        .await
        .expect("release writer lock");
    lock_conn.close().await.expect("close lock connection");

    proxy
        .key_store
        .flush_request_stats_writes()
        .await
        .expect("flush request stats after lock release");

    let refreshed = proxy
        .user_rankings_snapshot()
        .await
        .expect("rankings should refresh immediately after contention clears");
    assert_eq!(
        refreshed
            .last24h
            .primary_success_top
            .first()
            .map(|row| (row.user.user_id.as_str(), row.value)),
        Some((user.user_id.as_str(), 1)),
        "fallback snapshots must not remain cached after the durable flush succeeds"
    );
    assert_eq!(
        refreshed
            .last24h
            .unique_ip_top
            .first()
            .map(|row| (row.user.user_id.as_str(), row.value)),
        Some((user.user_id.as_str(), 1)),
        "fresh snapshots should surface the durable request log once contention clears"
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn analysis_pressure_snapshot_returns_promptly_when_flush_hits_write_lock() {
    let db_path = temp_db_path("analysis-pressure-write-lock-fallback");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = public_metrics_proxy(&db_str, "tvly-analysis-pressure-lock").await;

    proxy
        .key_store
        .enqueue_request_stats_rollup_for_test(None, proxy.backend_time().now_ts(), OUTCOME_SUCCESS)
        .await;

    let mut lock_conn = open_write_lock_connection(&db_str).await;
    let snapshot = tokio::time::timeout(Duration::from_secs(1), proxy.analysis_pressure_snapshot())
        .await
        .expect("analysis pressure should return promptly under write contention")
        .expect("analysis pressure fallback should succeed");
    assert_eq!(snapshot.current_user_distribution.summary.active_users, 0);
    assert_eq!(
        snapshot.current_user_distribution.summary.current_pressure,
        0
    );

    sqlx::query("ROLLBACK")
        .execute(&mut lock_conn)
        .await
        .expect("release writer lock");
    lock_conn.close().await.expect("close lock connection");

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn admin_user_list_stats_returns_durable_data_until_rollups_flush() {
    let db_path = temp_db_path("admin-user-list-stats-fresh-rollups");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = public_metrics_proxy(&db_str, "tvly-admin-user-list-stats-fresh").await;

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "admin-user-list-stats".to_string(),
            username: Some("admin_user_list_stats".to_string()),
            name: Some("Admin User List Stats".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert active user");

    proxy
        .key_store
        .enqueue_request_stats_rollup_for_user_for_test(
            &user.user_id,
            proxy.backend_time().now_ts(),
            OUTCOME_SUCCESS,
        )
        .await;

    let mut lock_conn = open_write_lock_connection(&db_str).await;
    let release_writer = async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        sqlx::query("ROLLBACK")
            .execute(&mut lock_conn)
            .await
            .expect("release writer lock");
        lock_conn.close().await.expect("close lock connection");
    };
    let (stats, ()) = tokio::join!(proxy.get_admin_user_list_stats(), release_writer);
    let stats = stats.expect("admin user list stats");

    assert_eq!(stats.total_users, 1);
    assert_eq!(stats.active_users_90d, 0);
    assert_eq!(stats.window_days, 90);

    proxy
        .key_store
        .flush_request_stats_writes()
        .await
        .expect("background-equivalent test flush");
    let converged = proxy
        .get_admin_user_list_stats()
        .await
        .expect("durable user list stats after flush");
    assert_eq!(converged.total_users, 1);
    assert_eq!(converged.active_users_90d, 1);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}
