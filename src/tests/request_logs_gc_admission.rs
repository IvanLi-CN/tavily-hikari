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
    assert!(resumed.completed);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
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
