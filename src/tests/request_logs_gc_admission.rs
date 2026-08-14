use super::jobs_and_request_log_retention::{
    RequestLogsRetentionEnvGuard, seed_request_log_for_gc,
};
use super::*;

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
