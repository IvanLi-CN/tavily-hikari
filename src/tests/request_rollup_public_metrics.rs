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
async fn public_success_breakdown_flushes_pending_request_stats_for_current_window() {
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

    assert_eq!(public.monthly_success, 1);
    assert_eq!(public.daily_success, 1);

    let persisted_flush = proxy
        .key_store
        .get_meta_i64(META_KEY_REQUEST_STATS_LAST_FLUSHED_AT_V1)
        .await
        .expect("read request stats flush watermark");
    assert!(persisted_flush.unwrap_or_default() > 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn public_success_breakdown_flushes_when_newer_pending_rollup_is_inside_window() {
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

    assert_eq!(public.monthly_success, 2);
    assert_eq!(public.daily_success, 1);

    let persisted_flush = proxy
        .key_store
        .get_meta_i64(META_KEY_REQUEST_STATS_LAST_FLUSHED_AT_V1)
        .await
        .expect("read request stats flush watermark");
    assert!(persisted_flush.unwrap_or_default() >= now.saturating_sub(10));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn public_success_breakdown_flushes_when_pending_rollup_is_outside_day_but_inside_month() {
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

    assert_eq!(public.monthly_success, 1);
    assert_eq!(public.daily_success, 0);

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

    tokio::time::timeout(Duration::from_secs(1), pause.arrived.notified())
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
async fn public_success_breakdown_waits_for_inflight_flush_before_serving_metrics() {
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

    tokio::select! {
        early = &mut done_rx => {
            panic!("public metrics returned before inflight flush completed: {early:?}");
        }
        _ = tokio::time::sleep(Duration::from_millis(100)) => {}
    }

    sqlx::query("ROLLBACK")
        .execute(&mut lock_conn)
        .await
        .expect("release writer lock");
    lock_conn.close().await.expect("close lock connection");

    flush_handle
        .await
        .expect("flush join")
        .expect("flush request stats");

    let public = done_rx
        .await
        .expect("public metrics result channel")
        .expect("public success breakdown");
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

    let summary_after = proxy.summary().await.expect("summary after lock release");
    assert_eq!(summary_after.total_requests, 1);
    assert_eq!(summary_after.success_count, 1);

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
async fn admin_user_rankings_snapshot_returns_promptly_when_flush_hits_write_lock() {
    let db_path = temp_db_path("admin-user-rankings-write-lock-fallback");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = public_metrics_proxy(&db_str, "tvly-admin-rankings-lock").await;

    proxy
        .key_store
        .enqueue_request_stats_rollup_for_test(None, proxy.backend_time().now_ts(), OUTCOME_SUCCESS)
        .await;

    let mut lock_conn = open_write_lock_connection(&db_str).await;
    let snapshot = tokio::time::timeout(Duration::from_secs(1), proxy.user_rankings_snapshot())
        .await
        .expect("rankings snapshot should return promptly under write contention")
        .expect("rankings fallback should succeed");
    assert!(snapshot.last24h.primary_success_top.is_empty());
    assert!(snapshot.last24h.business_credits_top.is_empty());
    assert!(snapshot.last24h.unique_ip_top.is_empty());

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
