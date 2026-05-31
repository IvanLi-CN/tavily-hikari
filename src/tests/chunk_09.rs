async fn hold_sqlite_write_lock_for_test(
    pool: &SqlitePool,
) -> tokio::task::JoinHandle<()> {
    let mut immediate_conn = begin_immediate_sqlite_connection(pool)
        .await
        .expect("begin immediate transaction");
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(5_200)).await;
        sqlx::query("ROLLBACK")
            .execute(&mut *immediate_conn)
            .await
            .expect("rollback immediate transaction");
    })
}

#[tokio::test]
async fn quota_subject_lock_retries_transient_sqlite_write_lock() {
    let db_path = temp_db_path("quota-subject-lock-retries-sqlite-lock");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let release = hold_sqlite_write_lock_for_test(&proxy.key_store.pool).await;

    let lease = proxy
        .key_store
        .acquire_quota_subject_lock(
            "test:quota-subject-lock-retry",
            Duration::from_secs(20),
            Duration::from_secs(30),
        )
        .await
        .expect("acquire lock after transient sqlite write lock");
    assert_eq!(lease.subject, "test:quota-subject-lock-retry");
    proxy
        .key_store
        .release_quota_subject_lock(&lease)
        .await
        .expect("release lock");
    release.await.expect("release task");

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn scheduled_job_start_retries_transient_sqlite_write_lock() {
    let db_path = temp_db_path("scheduled-job-start-retries-sqlite-lock");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let release = hold_sqlite_write_lock_for_test(&proxy.key_store.pool).await;

    let job_id = proxy
        .scheduled_job_start("sqlite_lock_retry_test", None, 1)
        .await
        .expect("scheduled job starts after transient sqlite write lock");
    assert!(job_id > 0);
    release.await.expect("release task");

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

async fn seed_request_log_for_gc(pool: &SqlitePool, created_at: i64, path: &str) -> i64 {
    sqlx::query_scalar(
        r#"
        INSERT INTO request_logs (
            api_key_id,
            auth_token_id,
            method,
            path,
            result_status,
            visibility,
            created_at
        )
        VALUES (NULL, NULL, 'POST', ?, 'success', ?, ?)
        RETURNING id
        "#,
    )
    .bind(path)
    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
    .bind(created_at)
    .fetch_one(pool)
    .await
    .expect("seed request log")
}

async fn seed_request_log_rollup_for_gc(pool: &SqlitePool, bucket_start: i64) {
    sqlx::query(
        r#"
        INSERT INTO request_log_catalog_rollups (
            bucket_start,
            request_kind_key,
            request_kind_label,
            result_bucket,
            key_effect_code,
            binding_effect_code,
            selection_effect_code,
            auth_token_id,
            api_key_id,
            operational_class,
            request_count,
            updated_at
        )
        VALUES (?, 'search', 'Search', 'success', 'none', 'none', 'none', '', '', 'api', 1, ?)
        "#,
    )
    .bind(bucket_start)
    .bind(Utc::now().timestamp())
    .execute(pool)
    .await
    .expect("seed request log rollup");
}

async fn seed_auth_token_log_reference_for_gc(
    pool: &SqlitePool,
    token_id: &str,
    request_log_id: i64,
    created_at: i64,
) {
    sqlx::query(
        r#"
        INSERT INTO auth_tokens (id, secret, enabled, note, total_requests, created_at)
        VALUES (?, 'secret', 1, 'gc reference test', 0, ?)
        "#,
    )
    .bind(token_id)
    .bind(created_at)
    .execute(pool)
    .await
    .expect("seed auth token");

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id,
            method,
            path,
            result_status,
            request_log_id,
            created_at
        )
        VALUES (?, 'POST', '/mcp', 'success', ?, ?)
        "#,
    )
    .bind(token_id)
    .bind(request_log_id)
    .bind(created_at)
    .execute(pool)
    .await
    .expect("seed auth token log request reference");
}

struct RequestLogsRetentionEnvGuard {
    prev: Option<String>,
}

impl RequestLogsRetentionEnvGuard {
    fn set_32_days() -> Self {
        let prev = std::env::var("REQUEST_LOGS_RETENTION_DAYS").ok();
        unsafe {
            std::env::set_var("REQUEST_LOGS_RETENTION_DAYS", "32");
        }
        Self { prev }
    }
}

impl Drop for RequestLogsRetentionEnvGuard {
    fn drop(&mut self) {
        unsafe {
            if let Some(prev) = self.prev.take() {
                std::env::set_var("REQUEST_LOGS_RETENTION_DAYS", prev);
            } else {
                std::env::remove_var("REQUEST_LOGS_RETENTION_DAYS");
            }
        }
    }
}

#[tokio::test]
async fn request_logs_gc_bounded_deletes_old_rows_and_preserves_recent_rows() {
    let lock = env_lock();
    let _env_lock = lock.lock().await;
    let _retention_guard = RequestLogsRetentionEnvGuard::set_32_days();
    let db_path = temp_db_path("request-logs-gc-bounded-preserves-recent");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let now = Utc::now().timestamp();
    let old_ts = now - 40 * 24 * 60 * 60;
    let recent_ts = now - 2 * 24 * 60 * 60;
    let old_id = seed_request_log_for_gc(&proxy.key_store.pool, old_ts, "/api/tavily/search").await;
    let recent_id =
        seed_request_log_for_gc(&proxy.key_store.pool, recent_ts, "/api/tavily/search").await;
    seed_auth_token_log_reference_for_gc(&proxy.key_store.pool, "tok-gc-ref", old_id, recent_ts)
        .await;
    seed_request_log_rollup_for_gc(&proxy.key_store.pool, old_ts).await;
    seed_request_log_rollup_for_gc(&proxy.key_store.pool, recent_ts).await;

    let report = proxy
        .gc_request_logs_with_options(RequestLogsGcOptions {
            batch_size: 10,
            max_batches: 5,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("run request logs gc");

    assert!(report.completed);
    assert_eq!(report.deleted_request_logs, 1);
    assert_eq!(report.deleted_rollups, 2);
    let old_exists: Option<i64> = sqlx::query_scalar("SELECT id FROM request_logs WHERE id = ?")
        .bind(old_id)
        .fetch_optional(&proxy.key_store.pool)
        .await
        .expect("query old log");
    let recent_exists: Option<i64> = sqlx::query_scalar("SELECT id FROM request_logs WHERE id = ?")
        .bind(recent_id)
        .fetch_optional(&proxy.key_store.pool)
        .await
        .expect("query recent log");
    assert!(old_exists.is_none());
    assert_eq!(recent_exists, Some(recent_id));
    let retained_auth_log_ref: Option<i64> = sqlx::query_scalar(
        "SELECT request_log_id FROM auth_token_logs WHERE token_id = 'tok-gc-ref'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("query retained auth token log reference");
    assert_eq!(retained_auth_log_ref, None);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn request_logs_gc_bounded_reports_partial_and_resumes() {
    let lock = env_lock();
    let _env_lock = lock.lock().await;
    let _retention_guard = RequestLogsRetentionEnvGuard::set_32_days();
    let db_path = temp_db_path("request-logs-gc-bounded-partial");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let old_ts = Utc::now().timestamp() - 40 * 24 * 60 * 60;
    for idx in 0..3 {
        seed_request_log_for_gc(&proxy.key_store.pool, old_ts + idx, "/mcp").await;
        seed_request_log_rollup_for_gc(&proxy.key_store.pool, old_ts + idx).await;
    }

    let first = proxy
        .gc_request_logs_with_options(RequestLogsGcOptions {
            batch_size: 1,
            max_batches: 1,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("run first request logs gc pass");
    assert!(!first.completed);
    assert!(first.has_more);
    assert_eq!(first.deleted_request_logs, 1);
    assert_eq!(first.deleted_rollups, 1);

    let second = proxy
        .gc_request_logs_with_options(RequestLogsGcOptions {
            batch_size: 10,
            max_batches: 5,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("run second request logs gc pass");
    assert!(second.completed);
    assert_eq!(second.deleted_request_logs, 2);
    assert_eq!(second.deleted_rollups, 5);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn request_logs_gc_retries_transient_sqlite_write_lock() {
    let lock = env_lock();
    let _env_lock = lock.lock().await;
    let _retention_guard = RequestLogsRetentionEnvGuard::set_32_days();
    let db_path = temp_db_path("request-logs-gc-retries-sqlite-lock");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let old_ts = Utc::now().timestamp() - 40 * 24 * 60 * 60;
    let old_id = seed_request_log_for_gc(&proxy.key_store.pool, old_ts, "/mcp").await;
    let threshold = request_logs_retention_threshold_utc_ts(effective_request_logs_retention_days());
    assert!(old_ts < threshold);

    let release = hold_sqlite_write_lock_for_test(&proxy.key_store.pool).await;
    let report = proxy
        .gc_request_logs_with_options(RequestLogsGcOptions {
            batch_size: 10,
            max_batches: 5,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("request logs gc retries after transient sqlite write lock");
    release.await.expect("release task");

    assert!(report.completed);
    let old_exists: Option<i64> = sqlx::query_scalar("SELECT id FROM request_logs WHERE id = ?")
        .bind(old_id)
        .fetch_optional(&proxy.key_store.pool)
        .await
        .expect("query old log after locked gc");
    assert!(old_exists.is_none());

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}
