use super::*;
use axum::http::Method;

#[allow(clippy::too_many_arguments)]
async fn seed_pressure_attempt(
    proxy: &TavilyProxy,
    manual_clock: &crate::ManualBackendTime,
    now: i64,
    token_id: &str,
    user_id: &str,
    created_at: i64,
    result_status: &str,
    upstream_operation: Option<&str>,
    request_kind: &TokenRequestKind,
) {
    let request_log_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO observability.request_logs (
            method,
            path,
            status_code,
            tavily_status_code,
            result_status,
            request_kind_key,
            request_kind_label,
            counts_business_quota,
            request_user_id,
            upstream_operation,
            created_at
        ) VALUES ('POST', '/api/tavily/search', 200, 200, ?, 'api:search', 'API | search', 1, ?, ?, ?)
        RETURNING id
        "#,
    )
    .bind(result_status)
    .bind(user_id)
    .bind(upstream_operation)
    .bind(created_at)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("insert pressure request log");

    manual_clock.set_now_ts(now);
    proxy
        .record_token_attempt_with_kind_request_log_metadata(
            token_id,
            &Method::POST,
            "/api/tavily/search",
            Some("q=pressure"),
            Some(if result_status == OUTCOME_SUCCESS {
                200
            } else {
                500
            }),
            Some(if result_status == OUTCOME_SUCCESS {
                200
            } else {
                500
            }),
            true,
            result_status,
            None,
            request_kind,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(request_log_id),
        )
        .await
        .expect("record pressure attempt");
}

#[tokio::test]
async fn analysis_pressure_snapshot_uses_rolling_1h_and_excludes_non_upstream_events() {
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_000_000);
    let db_path = temp_db_path("analysis-pressure-snapshot-live");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time,
    )
    .await
    .expect("proxy created");

    let alpha = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "analysis-pressure-alpha".to_string(),
            username: Some("alpha".to_string()),
            name: Some("Alpha".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert alpha");
    let beta = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "analysis-pressure-beta".to_string(),
            username: Some("beta".to_string()),
            name: Some("Beta".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert beta");
    let alpha_token = proxy
        .ensure_user_token_binding(&alpha.user_id, Some("analysis-pressure-alpha"))
        .await
        .expect("bind alpha token");
    let beta_token = proxy
        .ensure_user_token_binding(&beta.user_id, Some("analysis-pressure-beta"))
        .await
        .expect("bind beta token");
    let request_kind = TokenRequestKind::new("api:search", "API | search", None);
    let now = manual_clock.now_ts();

    seed_pressure_attempt(
        &proxy,
        &manual_clock,
        now,
        &alpha_token.id,
        &alpha.user_id,
        now - 50 * 60,
        OUTCOME_SUCCESS,
        Some("http_search"),
        &request_kind,
    )
    .await;
    seed_pressure_attempt(
        &proxy,
        &manual_clock,
        now,
        &alpha_token.id,
        &alpha.user_id,
        now - 15 * 60,
        "error",
        Some("http_search"),
        &request_kind,
    )
    .await;
    seed_pressure_attempt(
        &proxy,
        &manual_clock,
        now,
        &beta_token.id,
        &beta.user_id,
        now - 10 * 60,
        OUTCOME_SUCCESS,
        Some("http_search"),
        &request_kind,
    )
    .await;
    seed_pressure_attempt(
        &proxy,
        &manual_clock,
        now,
        &beta_token.id,
        &beta.user_id,
        now - 5 * 60,
        OUTCOME_QUOTA_EXHAUSTED,
        Some("http_search"),
        &request_kind,
    )
    .await;
    seed_pressure_attempt(
        &proxy,
        &manual_clock,
        now,
        &beta_token.id,
        &beta.user_id,
        now - 2 * 60,
        "blocked",
        None,
        &request_kind,
    )
    .await;

    manual_clock.set_now_ts(now);
    let snapshot = proxy
        .analysis_pressure_snapshot()
        .await
        .expect("analysis pressure snapshot");

    assert_eq!(snapshot.server_24h.current.len(), 288);
    assert_eq!(snapshot.server_24h.previous.len(), 288);
    assert_eq!(snapshot.server_7d.points.len(), 168);
    assert_eq!(snapshot.server_7d.moving_averages.len(), 2);
    assert_eq!(snapshot.server_7d.moving_averages[0].window_hours, 6);
    assert_eq!(snapshot.server_7d.moving_averages[0].points.len(), 168);
    assert_eq!(snapshot.server_7d.moving_averages[1].window_hours, 24);
    assert_eq!(snapshot.server_7d.moving_averages[1].points.len(), 168);

    let current_point = snapshot
        .server_24h
        .current
        .last()
        .expect("latest current pressure point");
    assert_eq!(current_point.pressure, 3);
    assert_eq!(current_point.success_count, 2);
    assert_eq!(current_point.failure_count, 1);

    let distribution = &snapshot.current_user_distribution;
    assert_eq!(distribution.rows.len(), 2);
    assert_eq!(distribution.rows[0].user_id, alpha.user_id);
    assert_eq!(distribution.rows[0].pressure, 2);
    assert_eq!(distribution.rows[1].user_id, beta.user_id);
    assert_eq!(distribution.rows[1].pressure, 1);
    assert_eq!(distribution.summary.current_pressure, 3);
    assert_eq!(distribution.summary.active_users, 2);
    assert_eq!(distribution.summary.zero_pressure_users, 0);
    assert_eq!(distribution.summary.peak, 2);
    assert_eq!(distribution.summary.median, 1);
    assert_eq!(distribution.summary.p90, 1);

    let latest_hour = snapshot
        .server_7d
        .points
        .last()
        .expect("latest hourly pressure point");
    assert_eq!(latest_hour.pressure, 1);
    assert_eq!(latest_hour.success_count, 1);
    assert_eq!(latest_hour.failure_count, 0);
    assert_eq!(
        snapshot.server_7d.moving_averages[0]
            .points
            .last()
            .expect("latest 6h moving average")
            .value,
        0
    );
    assert_eq!(
        snapshot.server_7d.moving_averages[1]
            .points
            .last()
            .expect("latest 24h moving average")
            .value,
        0
    );

    let previous_last = snapshot
        .server_24h
        .previous
        .last()
        .expect("latest previous pressure point");
    assert_eq!(
        previous_last
            .display_bucket_start
            .saturating_sub(previous_last.bucket_start),
        SECS_PER_DAY
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn analysis_pressure_snapshot_background_rebuild_rehydrates_server_pressure_buckets() {
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_300_000);
    let db_path = temp_db_path("analysis-pressure-snapshot-backfill");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time.clone(),
    )
    .await
    .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "analysis-pressure-backfill".to_string(),
            username: Some("backfill".to_string()),
            name: Some("Backfill".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let now = manual_clock.now_ts();

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
            counts_business_quota,
            request_user_id,
            upstream_operation,
            created_at
        ) VALUES
            ('POST', '/api/tavily/search', 200, 200, 'success', 'api:search', 'API | search', 1, ?, 'http_search', ?),
            ('POST', '/api/tavily/search', 500, 500, 'error', 'api:search', 'API | search', 1, ?, 'http_search', ?),
            ('POST', '/api/tavily/search', 429, 429, 'quota_exhausted', 'api:search', 'API | search', 1, ?, 'http_search', ?),
            ('POST', '/api/tavily/search', 429, 429, 'blocked', 'api:search', 'API | search', 1, ?, NULL, ?)
        "#,
    )
    .bind(&user.user_id)
    .bind(now - 40 * 60)
    .bind(&user.user_id)
    .bind(now - 8 * 60)
    .bind(&user.user_id)
    .bind(now - 4 * 60)
    .bind(&user.user_id)
    .bind(now - 2 * 60)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed pressure request logs");
    drop(proxy);

    manual_clock.set_now_ts(now);
    let reopened = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time,
    )
    .await
    .expect("reopen proxy");

    let initial_snapshot = reopened
        .analysis_pressure_snapshot()
        .await
        .expect("analysis pressure snapshot before background rebuild");
    assert_eq!(
        initial_snapshot
            .server_24h
            .current
            .last()
            .expect("latest current pressure point before rebuild")
            .pressure,
        0
    );

    assert!(
        reopened.spawn_server_pressure_buckets_rebuild_once(),
        "reopened proxy should schedule exactly one background rebuild"
    );
    assert!(
        !reopened.spawn_server_pressure_buckets_rebuild_once(),
        "background rebuild scheduling should be idempotent"
    );
    assert!(
        reopened.spawn_user_business_calls_1h_backfill_once(),
        "reopened proxy should also schedule one business-call backfill"
    );
    assert!(
        !reopened.spawn_user_business_calls_1h_backfill_once(),
        "business-call backfill scheduling should be idempotent"
    );

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let bucket_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM observability.server_pressure_buckets WHERE bucket_kind = 'five_minute'",
            )
            .fetch_one(&reopened.key_store.pool)
            .await
            .expect("count rebuilt server pressure buckets");
            if bucket_count >= 2 {
                return bucket_count;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("background rebuild should complete in time");

    let bucket_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM observability.server_pressure_buckets WHERE bucket_kind = 'five_minute'",
    )
    .fetch_one(&reopened.key_store.pool)
    .await
    .expect("count rebuilt server pressure buckets");
    assert!(
        bucket_count >= 2,
        "expected rebuilt server pressure buckets, got {bucket_count}"
    );

    let snapshot = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let snapshot = reopened
                .analysis_pressure_snapshot()
                .await
                .expect("analysis pressure snapshot after backfill");
            if snapshot.current_user_distribution.rows.len() == 1
                && snapshot.current_user_distribution.rows[0].pressure == 2
            {
                return snapshot;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("analysis pressure user distribution should recover after background tasks");
    let current_point = snapshot
        .server_24h
        .current
        .last()
        .expect("latest current pressure point");
    assert_eq!(current_point.pressure, 2);
    assert_eq!(current_point.success_count, 1);
    assert_eq!(current_point.failure_count, 1);
    assert_eq!(snapshot.current_user_distribution.rows.len(), 1);
    assert_eq!(snapshot.current_user_distribution.rows[0].pressure, 2);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn analysis_pressure_background_rebuild_retries_after_transient_failure() {
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_350_000);
    let db_path = temp_db_path("analysis-pressure-background-retry");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time.clone(),
    )
    .await
    .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "analysis-pressure-retry".to_string(),
            username: Some("retry".to_string()),
            name: Some("Retry".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert retry user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("analysis-pressure-retry"))
        .await
        .expect("bind retry token");
    let now = manual_clock.now_ts();
    let request_kind = TokenRequestKind::new("api:search", "API | search", None);

    seed_pressure_attempt(
        &proxy,
        &manual_clock,
        now,
        &token.id,
        &user.user_id,
        now - 120,
        OUTCOME_SUCCESS,
        Some("search"),
        &request_kind,
    )
    .await;
    drop(proxy);

    manual_clock.set_now_ts(now);
    let reopened = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time,
    )
    .await
    .expect("reopen proxy");

    let lock_handle =
        hold_sqlite_write_lock_for_test_for(&reopened.key_store.pool, Duration::from_secs(6)).await;
    assert!(
        reopened.spawn_server_pressure_buckets_rebuild_once(),
        "first background rebuild attempt should schedule"
    );
    assert!(
        !reopened.spawn_server_pressure_buckets_rebuild_once(),
        "concurrent background rebuild attempts should still dedupe"
    );
    lock_handle
        .await
        .expect("held sqlite write lock should release cleanly");

    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let bucket_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM observability.server_pressure_buckets WHERE bucket_kind = 'five_minute'",
            )
            .fetch_one(&reopened.key_store.pool)
            .await
            .expect("count rebuilt server pressure buckets after automatic retry");
            if bucket_count >= 1 {
                return bucket_count;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("background rebuild should retry automatically after transient failure");

    let snapshot = reopened
        .analysis_pressure_snapshot()
        .await
        .expect("analysis pressure snapshot after retry rebuild");
    let current_point = snapshot
        .server_24h
        .current
        .last()
        .expect("latest current pressure point after retry rebuild");
    assert_eq!(current_point.pressure, 1);
    assert_eq!(current_point.success_count, 1);
    assert_eq!(current_point.failure_count, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn analysis_pressure_background_rebuild_cancels_and_can_be_rescheduled() {
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_400_000);
    let db_path = temp_db_path("analysis-pressure-background-cancel");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time.clone(),
    )
    .await
    .expect("proxy created");
    let now = manual_clock.now_ts();
    sqlx::query(
        r#"
        INSERT INTO observability.request_logs (
            method,
            path,
            status_code,
            tavily_status_code,
            result_status,
            request_kind_key,
            request_kind_label,
            counts_business_quota,
            request_user_id,
            upstream_operation,
            created_at
        ) VALUES ('POST', '/api/tavily/search', 200, 200, 'success', 'api:search', 'API | search', 1, ?, 'search', ?)
        "#,
    )
    .bind("analysis-pressure-cancel-user")
    .bind(now - 120)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed pressure request log");
    drop(proxy);

    manual_clock.set_now_ts(now);
    let reopened = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time,
    )
    .await
    .expect("reopen proxy");

    let lock_handle =
        hold_sqlite_write_lock_for_test_for(&reopened.key_store.pool, Duration::from_secs(6)).await;
    assert!(
        reopened.spawn_server_pressure_buckets_rebuild_once(),
        "first background rebuild attempt should schedule"
    );
    reopened.cancel_server_pressure_buckets_rebuild().await;
    lock_handle
        .await
        .expect("held sqlite write lock should release cleanly");

    tokio::time::sleep(Duration::from_secs(2)).await;

    let cancelled_bucket_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM observability.server_pressure_buckets WHERE bucket_kind = 'five_minute'",
    )
    .fetch_one(&reopened.key_store.pool)
    .await
    .expect("count buckets after cancelling rebuild");
    assert_eq!(
        cancelled_bucket_count, 0,
        "cancelled rebuild must not keep writing after role demotion"
    );

    assert!(
        reopened.spawn_server_pressure_buckets_rebuild_once(),
        "serving promotion should be able to reschedule rebuild after cancellation"
    );

    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let bucket_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM observability.server_pressure_buckets WHERE bucket_kind = 'five_minute'",
            )
            .fetch_one(&reopened.key_store.pool)
            .await
            .expect("count rebuilt buckets after reschedule");
            if bucket_count >= 1 {
                return bucket_count;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("rescheduled rebuild should eventually repopulate server pressure buckets");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn analysis_pressure_background_rebuild_releases_latch_after_success() {
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_450_000);
    let db_path = temp_db_path("analysis-pressure-background-success-reschedule");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time.clone(),
    )
    .await
    .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "analysis-pressure-success-reschedule".to_string(),
            username: Some("success-reschedule".to_string()),
            name: Some("Success Reschedule".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert success reschedule user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("analysis-pressure-success-reschedule"))
        .await
        .expect("bind success reschedule token");
    let now = manual_clock.now_ts();
    let request_kind = TokenRequestKind::new("api:search", "API | search", None);

    seed_pressure_attempt(
        &proxy,
        &manual_clock,
        now,
        &token.id,
        &user.user_id,
        now - 120,
        OUTCOME_SUCCESS,
        Some("search"),
        &request_kind,
    )
    .await;
    drop(proxy);

    manual_clock.set_now_ts(now);
    let reopened = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time,
    )
    .await
    .expect("reopen proxy");

    assert!(
        reopened.spawn_server_pressure_buckets_rebuild_once(),
        "first background rebuild attempt should schedule"
    );

    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let total_pressure: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(success_count + failure_count), 0) FROM observability.server_pressure_buckets WHERE bucket_kind = 'five_minute'",
            )
            .fetch_one(&reopened.key_store.pool)
            .await
            .expect("sum rebuilt server pressure buckets after first success");
            if total_pressure >= 1 {
                return total_pressure;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("first background rebuild should complete in time");

    seed_pressure_attempt(
        &reopened,
        &manual_clock,
        now,
        &token.id,
        &user.user_id,
        now - 60,
        OUTCOME_ERROR,
        Some("search"),
        &request_kind,
    )
    .await;

    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if reopened.spawn_server_pressure_buckets_rebuild_once() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("successful rebuild should release the latch so a later serving promotion can reschedule it");
    reopened.cancel_server_pressure_buckets_rebuild().await;

    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let total_pressure: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(success_count + failure_count), 0) FROM observability.server_pressure_buckets WHERE bucket_kind = 'five_minute'",
            )
            .fetch_one(&reopened.key_store.pool)
            .await
            .expect("sum rebuilt server pressure buckets after reschedule");
            if total_pressure >= 2 {
                return total_pressure;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("rescheduled rebuild should include newly imported request logs");

    let snapshot = reopened
        .analysis_pressure_snapshot()
        .await
        .expect("analysis pressure snapshot after successful reschedule");
    let current_point = snapshot
        .server_24h
        .current
        .last()
        .expect("latest current pressure point after reschedule");
    assert!(current_point.pressure >= 2);
    assert!(current_point.success_count >= 1);
    assert!(current_point.failure_count >= 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn analysis_pressure_cancel_requeues_buffered_events_for_next_generation() {
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_460_000);
    let db_path = temp_db_path("analysis-pressure-cancel-requeues-buffered-events");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time.clone(),
    )
    .await
    .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "analysis-pressure-cancel-requeue".to_string(),
            username: Some("cancel-requeue".to_string()),
            name: Some("Cancel Requeue".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert cancel requeue user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("analysis-pressure-cancel-requeue"))
        .await
        .expect("bind cancel requeue token");
    let request_kind = TokenRequestKind::new("api:search", "API | search", None);
    let now = manual_clock.now_ts();

    seed_pressure_attempt(
        &proxy,
        &manual_clock,
        now,
        &token.id,
        &user.user_id,
        now - 180,
        OUTCOME_SUCCESS,
        Some("search"),
        &request_kind,
    )
    .await;
    drop(proxy);

    let reopened = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time,
    )
    .await
    .expect("reopen proxy");

    assert!(
        reopened.spawn_server_pressure_buckets_rebuild_once(),
        "first background rebuild attempt should schedule"
    );
    reopened
        .inject_server_pressure_buffered_event_for_test(Some(9_999), now - 30, OUTCOME_ERROR)
        .await;
    reopened.cancel_server_pressure_buckets_rebuild().await;

    assert!(
        reopened.spawn_server_pressure_buckets_rebuild_once(),
        "next serving generation should be able to reschedule rebuild"
    );

    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let total_pressure: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(success_count + failure_count), 0) FROM observability.server_pressure_buckets WHERE bucket_kind = 'five_minute'",
            )
            .fetch_one(&reopened.key_store.pool)
            .await
            .expect("sum rebuilt server pressure buckets after cancelled replay");
            if total_pressure >= 2 {
                return total_pressure;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("requeued buffered pressure event should be replayed by the next rebuild generation");

    let snapshot = reopened
        .analysis_pressure_snapshot()
        .await
        .expect("analysis pressure snapshot after requeued replay");
    let current_point = snapshot
        .server_24h
        .current
        .last()
        .expect("latest current pressure point after requeued replay");
    assert_eq!(current_point.pressure, 2);
    assert_eq!(current_point.success_count, 1);
    assert_eq!(current_point.failure_count, 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn analysis_pressure_snapshot_warms_up_24h_rolling_window_edges() {
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_500_000);
    let db_path = temp_db_path("analysis-pressure-snapshot-warmup");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time,
    )
    .await
    .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "analysis-pressure-warmup".to_string(),
            username: Some("warmup".to_string()),
            name: Some("Warmup".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("analysis-pressure-warmup"))
        .await
        .expect("bind token");
    let request_kind = TokenRequestKind::new("api:search", "API | search", None);
    let now = manual_clock.now_ts();
    let current_bucket_start = now - now.rem_euclid(SECS_PER_FIVE_MINUTES);
    let current_24h_start = current_bucket_start - 287 * SECS_PER_FIVE_MINUTES;
    let previous_24h_start = current_24h_start - SECS_PER_DAY;

    seed_pressure_attempt(
        &proxy,
        &manual_clock,
        now,
        &token.id,
        &user.user_id,
        current_24h_start - 5 * SECS_PER_MINUTE,
        OUTCOME_SUCCESS,
        Some("http_search"),
        &request_kind,
    )
    .await;
    seed_pressure_attempt(
        &proxy,
        &manual_clock,
        now,
        &token.id,
        &user.user_id,
        previous_24h_start - 5 * SECS_PER_MINUTE,
        OUTCOME_SUCCESS,
        Some("http_search"),
        &request_kind,
    )
    .await;

    manual_clock.set_now_ts(now);
    let snapshot = proxy
        .analysis_pressure_snapshot()
        .await
        .expect("analysis pressure snapshot");

    let current_first = snapshot
        .server_24h
        .current
        .first()
        .expect("first current pressure point");
    assert_eq!(current_first.bucket_start, current_24h_start);
    assert_eq!(current_first.pressure, 1);
    assert_eq!(current_first.success_count, 1);
    assert_eq!(current_first.failure_count, 0);

    let previous_first = snapshot
        .server_24h
        .previous
        .first()
        .expect("first previous pressure point");
    assert_eq!(previous_first.bucket_start, previous_24h_start);
    assert_eq!(previous_first.pressure, 1);
    assert_eq!(previous_first.success_count, 1);
    assert_eq!(previous_first.failure_count, 0);
    assert_eq!(snapshot.server_7d.moving_averages.len(), 2);
    assert_eq!(snapshot.server_7d.moving_averages[0].points.len(), 168);
    assert_eq!(snapshot.server_7d.moving_averages[1].points.len(), 168);

    let _ = std::fs::remove_file(db_path);
}
