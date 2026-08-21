use super::*;
use axum::http::{Method, StatusCode};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

async fn wait_for_server_pressure_totals(
    proxy: &TavilyProxy,
    expected_success: i64,
    expected_failure: i64,
) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let (success_count, failure_count): (i64, i64) = sqlx::query_as(
                r#"
                SELECT COALESCE(SUM(success_count), 0), COALESCE(SUM(failure_count), 0)
                FROM observability.server_pressure_buckets
                WHERE bucket_kind = 'five_minute'
                "#,
            )
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read deferred pressure totals");
            if (success_count, failure_count) == (expected_success, expected_failure) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("deferred pressure buckets should converge within the test budget");
}

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
async fn analysis_pressure_rebuild_waits_for_overflow_coverage_loss_or_sustained_staleness() {
    let db_path = temp_db_path("analysis-pressure-rebuild-hysteresis");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
    )
    .await
    .expect("create proxy");

    assert!(
        !proxy.spawn_server_pressure_buckets_rebuild_once(),
        "a writable tenure alone must not rebuild pressure buckets"
    );
    assert!(!proxy.server_pressure_rebuild_is_active_for_test());

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn analysis_pressure_rebuild_source_slice_uses_the_time_cursor_index() {
    let db_path = temp_db_path("analysis-pressure-rebuild-time-cursor");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
    )
    .await
    .expect("create proxy");

    let plan = sqlx::query(
        r#"
        EXPLAIN QUERY PLAN
        SELECT id, created_at, result_status
        FROM observability.request_logs INDEXED BY idx_request_logs_time
        WHERE created_at >= 0
          AND (created_at > 0 OR (created_at = 0 AND id > 0))
          AND id <= 9223372036854775807
          AND visibility = 'visible'
          AND request_user_id IS NOT NULL
          AND counts_business_quota = 1
          AND upstream_operation IS NOT NULL
          AND result_status != 'quota_exhausted'
        ORDER BY created_at ASC, id ASC
        LIMIT 500
        "#,
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("explain server pressure rebuild source slice");
    let details = plan
        .iter()
        .map(|row| {
            row.try_get::<String, _>("detail")
                .expect("query-plan detail")
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        details.contains("idx_request_logs_time"),
        "expected time-keyset source scan, got query plan:\n{details}"
    );
    assert!(
        !details.contains("USE TEMP B-TREE"),
        "time-keyset source scan must not sort the historical log range:\n{details}"
    );

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
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
    wait_for_server_pressure_totals(&proxy, 2, 1).await;
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
async fn analysis_pressure_snapshot_live_local_mcp_logs_update_server_buckets() {
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_200_000);
    let db_path = temp_db_path("analysis-pressure-local-mcp-live");
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
            provider_user_id: "analysis-pressure-local-mcp".to_string(),
            username: Some("local-mcp".to_string()),
            name: Some("Local MCP".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("analysis-pressure-local-mcp"))
        .await
        .expect("bind token");
    let now = manual_clock.now_ts();
    manual_clock.set_now_ts(now);
    let headers: [String; 0] = [];
    let pressure_flush_complete = proxy
        .server_pressure_flush_completed_notifier_for_test()
        .await
        .notified_owned();

    let request_log_id = proxy
        .record_local_request_log_without_key_with_diagnostics(
            Some(&token.id),
            &Method::POST,
            "/mcp",
            None,
            StatusCode::OK,
            None,
            br#"{"jsonrpc":"2.0","method":"ping","id":1}"#,
            br#"{"jsonrpc":"2.0","result":{},"id":1}"#,
            OUTCOME_SUCCESS,
            None,
            Some(MCP_GATEWAY_MODE_REBALANCE),
            Some(MCP_EXPERIMENT_VARIANT_REBALANCE),
            Some("analysis-pressure-local-mcp-session"),
            None,
            Some("mcp"),
            None,
            &headers,
            &headers,
            None,
        )
        .await
        .expect("record local mcp request log");

    tokio::time::timeout(Duration::from_secs(3), pressure_flush_complete)
        .await
        .expect("deferred pressure writer should drain the live local mcp log");

    let canonical_rows: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM observability.request_logs
        WHERE id = ?
          AND visibility = ?
          AND request_user_id IS NOT NULL
          AND counts_business_quota = 1
          AND upstream_operation IS NOT NULL
          AND result_status != ?
        "#,
    )
    .bind(request_log_id)
    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
    .bind(OUTCOME_QUOTA_EXHAUSTED)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count canonical pressure request logs");
    assert_eq!(canonical_rows, 1);

    let five_minute_pressure: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(success_count + failure_count), 0)
        FROM observability.server_pressure_buckets
        WHERE bucket_kind = 'five_minute'
        "#,
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("sum live five-minute pressure");
    assert_eq!(five_minute_pressure, 1);

    let hour_pressure: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(success_count + failure_count), 0)
        FROM observability.server_pressure_buckets
        WHERE bucket_kind = 'hour'
        "#,
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("sum live hour pressure");
    assert_eq!(hour_pressure, 1);

    let snapshot = proxy
        .analysis_pressure_snapshot()
        .await
        .expect("analysis pressure snapshot after live local mcp log");
    assert_eq!(
        snapshot
            .server_24h
            .current
            .last()
            .expect("latest current pressure point")
            .pressure,
        1
    );
    assert_eq!(snapshot.current_user_distribution.rows.len(), 1);
    assert_eq!(
        snapshot.current_user_distribution.rows[0].user_id,
        user.user_id
    );
    assert_eq!(snapshot.current_user_distribution.rows[0].pressure, 1);

    proxy
        .key_store
        .rebuild_server_pressure_buckets_with_cancel(|| true)
        .await
        .expect("rebuild server pressure buckets");

    let five_minute_pressure_after_rebuild: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(success_count + failure_count), 0)
        FROM observability.server_pressure_buckets
        WHERE bucket_kind = 'five_minute'
        "#,
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("sum rebuilt five-minute pressure");
    assert_eq!(five_minute_pressure_after_rebuild, 1);

    let hour_pressure_after_rebuild: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(success_count + failure_count), 0)
        FROM observability.server_pressure_buckets
        WHERE bucket_kind = 'hour'
        "#,
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("sum rebuilt hour pressure");
    assert_eq!(hour_pressure_after_rebuild, 1);

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
        reopened.force_server_pressure_buckets_rebuild_once_for_test(),
        "reopened proxy should schedule exactly one background rebuild"
    );
    assert!(
        !reopened.force_server_pressure_buckets_rebuild_once_for_test(),
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
        reopened.force_server_pressure_buckets_rebuild_once_for_test(),
        "first background rebuild attempt should schedule"
    );
    assert!(
        !reopened.force_server_pressure_buckets_rebuild_once_for_test(),
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn analysis_pressure_rebuild_does_not_hold_writer_during_source_aggregation() {
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_390_000);
    let db_path = temp_db_path("analysis-pressure-read-before-write");
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
    let now = manual_clock.now_ts();
    sqlx::query(
        r#"
        INSERT INTO observability.request_logs (
            method, path, status_code, tavily_status_code, result_status,
            request_kind_key, request_kind_label, counts_business_quota,
            request_user_id, upstream_operation, created_at
        ) VALUES ('POST', '/api/tavily/search', 200, 200, 'success',
                  'api:search', 'API | search', 1, 'pressure-reader', 'search', ?)
        "#,
    )
    .bind(now - 120)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed pressure request log");

    let aggregation_reached = Arc::new(Barrier::new(2));
    let release_aggregation = Arc::new(Barrier::new(2));
    let checkpoints = Arc::new(AtomicUsize::new(0));
    let rebuild_store = proxy.key_store.clone();
    let rebuild_reached = aggregation_reached.clone();
    let rebuild_release = release_aggregation.clone();
    let rebuild_checkpoints = checkpoints.clone();
    let rebuild = tokio::spawn(async move {
        rebuild_store
            .rebuild_server_pressure_buckets_with_cancel(|| {
                if rebuild_checkpoints.fetch_add(1, Ordering::SeqCst) == 1 {
                    rebuild_reached.wait();
                    rebuild_release.wait();
                }
                true
            })
            .await
    });
    tokio::task::spawn_blocking(move || aggregation_reached.wait())
        .await
        .expect("observe completed source aggregation");

    let foreground_write = tokio::time::timeout(
        Duration::from_millis(250),
        sqlx::query("UPDATE meta SET value = value WHERE key = 'schema_version'")
            .execute(&proxy.key_store.pool),
    )
    .await;
    tokio::task::spawn_blocking(move || release_aggregation.wait())
        .await
        .expect("release pressure rebuild");
    foreground_write
        .expect("source aggregation must not block a foreground writer")
        .expect("foreground writer succeeds during source aggregation");
    rebuild
        .await
        .expect("pressure rebuild task joins")
        .expect("pressure rebuild succeeds");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn analysis_pressure_rebuild_never_deletes_the_live_generation() {
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_395_000);
    let db_path = temp_db_path("analysis-pressure-staged-generation");
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
    let now = manual_clock.now_ts();
    sqlx::query(
        r#"
        INSERT INTO observability.request_logs (
            method, path, status_code, tavily_status_code, result_status,
            request_kind_key, request_kind_label, counts_business_quota,
            request_user_id, upstream_operation, created_at
        ) VALUES ('POST', '/api/tavily/search', 200, 200, 'success',
                  'api:search', 'API | search', 1, 'staged-generation', 'search', ?)
        "#,
    )
    .bind(now - 120)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed pressure request log");
    proxy
        .key_store
        .ensure_server_pressure_bucket_schema()
        .await
        .expect("initialize pressure generation schema");
    for bucket_start in 0..26 {
        sqlx::query(
            r#"
            INSERT INTO observability.server_pressure_buckets (
                bucket_kind, bucket_start, bucket_secs, success_count,
                failure_count, updated_at, generation
            ) VALUES ('five_minute', ?, 300, 1, 0, ?, 0)
            "#,
        )
        .bind(bucket_start)
        .bind(now)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed old live generation");
    }
    sqlx::query(
        "CREATE TABLE observability.server_pressure_delete_counter (count INTEGER NOT NULL)",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("create delete counter");
    sqlx::query("INSERT INTO observability.server_pressure_delete_counter (count) VALUES (0)")
        .execute(&proxy.key_store.pool)
        .await
        .expect("initialize delete counter");
    sqlx::query(
        r#"
        CREATE TRIGGER observability.forbid_server_pressure_generation_delete
        BEFORE DELETE ON server_pressure_buckets
        BEGIN
            UPDATE server_pressure_delete_counter SET count = count + 1;
            SELECT CASE WHEN (SELECT count FROM server_pressure_delete_counter) > 25
                THEN RAISE(ABORT, 'live pressure cleanup exceeded its bounded slice') END;
        END
        "#,
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("forbid destructive pressure publication");

    proxy
        .key_store
        .rebuild_server_pressure_buckets_with_cancel(|| true)
        .await
        .expect("staged rebuild should publish without deleting live buckets");
    let total: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(success_count + failure_count), 0)
        FROM observability.server_pressure_buckets
        WHERE generation = (
            SELECT active_generation FROM observability.server_pressure_rebuild_state WHERE singleton = 1
        )
        "#,
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read staged pressure generation");
    assert_eq!(total, 2, "both staged bucket resolutions are published");

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
        reopened.force_server_pressure_buckets_rebuild_once_for_test(),
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

    reopened
        .key_store
        .sqlite_runtime
        .mark_recent_contention_for_test();
    assert!(
        reopened.force_server_pressure_buckets_rebuild_once_for_test(),
        "serving promotion should reschedule after cancellation through the contention cooldown"
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
async fn observability_deferred_writer_requeues_pressure_deltas_after_writer_contention() {
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_500_000);
    let db_path = temp_db_path("observability-deferred-writer");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time,
    )
    .await
    .expect("create proxy");
    let now = manual_clock.now_ts();
    let lock_handle =
        hold_sqlite_write_lock_for_test_for(&proxy.key_store.pool, Duration::from_millis(350))
            .await;

    let received_at = std::time::Instant::now();
    proxy
        .record_server_pressure_event(None, now, OUTCOME_SUCCESS)
        .await
        .expect("enqueue pressure event without waiting for writer");
    assert!(
        received_at.elapsed() < Duration::from_millis(250),
        "request-path pressure observation must not wait for SQLite writer"
    );

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let (queued, stale, _, _, _) = proxy
                .observability_deferred_writer_snapshot_for_test()
                .await;
            if queued >= 2 && stale {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("transient writer contention requeues bounded pressure deltas");
    lock_handle
        .await
        .expect("held SQLite writer lock releases cleanly");

    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let bucket_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM observability.server_pressure_buckets WHERE bucket_kind IN ('five_minute', 'hour')",
            )
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("count asynchronously flushed pressure buckets");
            if bucket_count == 2 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("requeued deltas flush after the writer becomes available");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn observability_deferred_writer_source_fence_does_not_double_count_rebuilds() {
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_500_000);
    let db_path = temp_db_path("observability-deferred-writer-source-fence");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time,
    )
    .await
    .expect("create proxy");
    let now = manual_clock.now_ts();
    let request_log_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO observability.request_logs (
            method, path, status_code, tavily_status_code, result_status,
            request_kind_key, request_kind_label, counts_business_quota,
            request_user_id, upstream_operation, created_at
        ) VALUES ('POST', '/mcp', 200, 200, ?, 'mcp:tool', 'MCP | tool', 1, 'source-fence-user', 'mcp', ?)
        RETURNING id
        "#,
    )
    .bind(OUTCOME_SUCCESS)
    .bind(now)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("seed rebuild source row");

    let held = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match proxy.admit_dashboard_rollup_integrity() {
                SqliteAdmissionOutcome::Admitted(permit) => return permit,
                SqliteAdmissionOutcome::Deferred { reason } => {
                    assert_eq!(
                        reason, "pool_pressure",
                        "test setup must not mask a real bulk-admission defer"
                    );
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }
    })
    .await
    .expect("test must acquire the shared bulk permit after lazy pool startup");
    proxy
        .record_server_pressure_event(Some(request_log_id), now, OUTCOME_SUCCESS)
        .await
        .expect("queue source-backed pressure delta");
    proxy
        .record_server_pressure_event(None, now, OUTCOME_ERROR)
        .await
        .expect("queue unsourced pressure delta");
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let (queued, stale, _, _, _) = proxy
                .observability_deferred_writer_snapshot_for_test()
                .await;
            if queued == 2 && stale {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("source-backed delta should wait behind the held bulk permit");
    drop(held);

    assert!(
        proxy.force_server_pressure_buckets_rebuild_once_for_test(),
        "source rebuild should schedule while the old delta remains queued"
    );
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if !proxy.server_pressure_rebuild_is_active_for_test() {
                let (success_count, failure_count): (i64, i64) = sqlx::query_as(
                    r#"
                    SELECT COALESCE(SUM(success_count), 0), COALESCE(SUM(failure_count), 0)
                    FROM observability.server_pressure_buckets
                    WHERE bucket_kind = 'five_minute'
                    "#,
                )
                .fetch_one(&proxy.key_store.pool)
                .await
                .expect("read rebuilt source-backed totals");
                if (success_count, failure_count) == (1, 1) {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("source-backed delta must not be replayed and unsourced delta must survive rebuild");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn rebalance_audit_writer_is_best_effort_and_payload_bounded() {
    let db_path = temp_db_path("rebalance-audit-deferred-writer");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create proxy");
    let entry = RebalanceAuditEntry {
        auth_token_id: None,
        method: Method::POST,
        path: "/mcp".to_string(),
        request_body: br#"{"jsonrpc":"2.0"}"#.to_vec(),
        response_status: StatusCode::OK,
        tavily_status_code: Some(200),
        response_body: br#"{"result":{}}"#.to_vec(),
        result_status: OUTCOME_SUCCESS.to_string(),
        failure_kind: None,
        proxy_session_id: Some("rebalance-audit-test".to_string()),
        routing_subject_hash: None,
        fallback_reason: Some("affinity_rebalanced".to_string()),
    };
    let received_at = std::time::Instant::now();
    assert!(proxy.enqueue_rebalance_audit(entry).await);
    assert!(
        received_at.elapsed() < Duration::from_millis(250),
        "MCP completion must not wait for the best-effort audit write"
    );

    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let written: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM observability.request_logs WHERE gateway_mode = 'rebalance'",
            )
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("count rebalance audit records");
            if written == 1 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("background audit writer persists its bounded entry");

    let oversized = RebalanceAuditEntry {
        auth_token_id: None,
        method: Method::POST,
        path: "/mcp".to_string(),
        request_body: vec![b'x'; 1024 * 1024 + 1],
        response_status: StatusCode::OK,
        tavily_status_code: Some(200),
        response_body: Vec::new(),
        result_status: OUTCOME_SUCCESS.to_string(),
        failure_kind: None,
        proxy_session_id: None,
        routing_subject_hash: None,
        fallback_reason: None,
    };
    assert!(
        !proxy.enqueue_rebalance_audit(oversized).await,
        "oversized best-effort audit is rejected without allocating an unbounded queue"
    );
    let (_, _, _, _, audit_stale) = proxy
        .observability_deferred_writer_snapshot_for_test()
        .await;
    assert!(audit_stale, "dropped audit coverage is explicit");

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
        reopened.force_server_pressure_buckets_rebuild_once_for_test(),
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
            if reopened.force_server_pressure_buckets_rebuild_once_for_test() {
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
            let (success_count, failure_count): (i64, i64) = sqlx::query_as(
                r#"
                SELECT COALESCE(SUM(success_count), 0), COALESCE(SUM(failure_count), 0)
                FROM observability.server_pressure_buckets
                WHERE bucket_kind = 'five_minute'
                "#,
            )
            .fetch_one(&reopened.key_store.pool)
            .await
            .expect("read rebuilt server pressure outcomes after reschedule");
            if success_count >= 1 && failure_count >= 1 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("rescheduled rebuild should include the newly imported failure");

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
        reopened.force_server_pressure_buckets_rebuild_once_for_test(),
        "first background rebuild attempt should schedule"
    );
    reopened
        .inject_server_pressure_buffered_event_for_test(Some(9_999), now - 30, OUTCOME_ERROR)
        .await;
    reopened.cancel_server_pressure_buckets_rebuild().await;

    assert!(
        reopened.force_server_pressure_buckets_rebuild_once_for_test(),
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
async fn analysis_pressure_rebuild_drains_events_arriving_during_tail_replay() {
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_470_000);
    let db_path = temp_db_path("analysis-pressure-live-tail-drain");
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
    let now = manual_clock.now_ts();

    // Seed the first replay batch before the background task can drain it.
    // The concurrent producer below then exercises the live handoff rather
    // than relying on an unlocked test-only append racing the task itself.
    for index in 0..250_i64 {
        proxy
            .inject_server_pressure_buffered_event_for_test(
                Some(10_000 + index),
                now - 30,
                OUTCOME_SUCCESS,
            )
            .await;
    }
    proxy.pause_server_pressure_tail_replay_for_test();
    assert!(proxy.force_server_pressure_buckets_rebuild_once_for_test());
    tokio::time::timeout(
        Duration::from_secs(2),
        proxy.wait_for_server_pressure_tail_replay_for_test(),
    )
    .await
    .expect("rebuild should enter the tail replay before the live producer starts");
    let producer = {
        let proxy = proxy.clone();
        tokio::spawn(async move {
            for index in 0..100_i64 {
                proxy
                    .record_server_pressure_event(Some(20_000 + index), now - 30, OUTCOME_ERROR)
                    .await
                    .expect("record event while replay drains");
                tokio::task::yield_now().await;
            }
        })
    };
    producer.await.expect("tail producer joins");
    proxy.resume_server_pressure_tail_replay_for_test();
    tokio::time::timeout(Duration::from_secs(10), async {
        while proxy.server_pressure_rebuild_is_active_for_test() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("pressure rebuild drains its live tail");

    wait_for_server_pressure_totals(&proxy, 250, 100).await;

    let (success_count, failure_count): (i64, i64) = sqlx::query_as(
        r#"
        SELECT COALESCE(SUM(success_count), 0), COALESCE(SUM(failure_count), 0)
        FROM observability.server_pressure_buckets
        WHERE bucket_kind = 'five_minute'
        "#,
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read rebuilt pressure totals");
    assert_eq!(success_count, 250);
    assert_eq!(failure_count, 100);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn analysis_pressure_rebuild_releases_phase_after_tail_replay_error() {
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_475_000);
    let db_path = temp_db_path("analysis-pressure-tail-replay-error");
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
    let now = manual_clock.now_ts();

    proxy
        .inject_server_pressure_buffered_event_for_test(Some(9_999), now - 30, OUTCOME_ERROR)
        .await;
    proxy.pause_server_pressure_tail_replay_for_test();
    assert!(proxy.force_server_pressure_buckets_rebuild_once_for_test());
    tokio::time::timeout(
        Duration::from_secs(2),
        proxy.wait_for_server_pressure_tail_replay_for_test(),
    )
    .await
    .expect("rebuild should enter the tail replay before forcing its write failure");

    proxy.fail_next_server_pressure_tail_replay_upsert_for_test();
    proxy.resume_server_pressure_tail_replay_for_test();

    tokio::time::timeout(Duration::from_secs(2), async {
        while proxy.server_pressure_rebuild_is_active_for_test() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("tail replay write error should release the rebuild phase");
    assert!(
        proxy.force_server_pressure_buckets_rebuild_once_for_test(),
        "a failed tail replay must leave the next rebuild schedulable"
    );
    proxy.cancel_server_pressure_buckets_rebuild().await;

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
    wait_for_server_pressure_totals(&proxy, 2, 0).await;
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
