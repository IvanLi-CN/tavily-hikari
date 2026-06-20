use super::*;

#[tokio::test]
async fn public_success_breakdown_skips_flush_when_no_pending_request_stats() {
    let db_path = temp_db_path("public-success-breakdown-no-pending-flush");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-public-success-no-pending".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    proxy
        .key_store
        .set_meta_i64(
            META_KEY_REQUEST_STATS_LAST_FLUSHED_AT_V1,
            Utc::now().timestamp(),
        )
        .await
        .expect("set request stats flush watermark");

    let now = Utc::now().timestamp();
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

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-public-success-pending".to_string()],
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
    let now = Utc::now().timestamp();
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

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-public-success-mixed-pending".to_string()],
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
    let now = Utc::now().timestamp();
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

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-public-success-month-only-pending".to_string()],
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
    let now = Utc::now();
    let month_start = start_of_month(now).timestamp();
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

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-public-success-concurrent-pending".to_string()],
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
    let now = Utc::now().timestamp();
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

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-public-success-inflight-wait".to_string()],
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
    let now = Utc::now().timestamp();
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
