async fn hold_sqlite_write_lock_for_test(
    pool: &SqlitePool,
) -> tokio::task::JoinHandle<()> {
    hold_sqlite_write_lock_for_test_for(pool, Duration::from_millis(120)).await
}

async fn begin_held_sqlite_write_lock_for_test(pool: &SqlitePool) -> sqlx::pool::PoolConnection<sqlx::Sqlite> {
    begin_immediate_sqlite_connection(pool)
        .await
        .expect("begin immediate transaction")
}

async fn hold_sqlite_write_lock_for_test_for(
    pool: &SqlitePool,
    hold_for: Duration,
) -> tokio::task::JoinHandle<()> {
    hold_sqlite_write_lock_for_test_for_with_release(pool, hold_for, None).await
}

async fn hold_sqlite_write_lock_for_test_for_with_release(
    pool: &SqlitePool,
    hold_for: Duration,
    release: Option<crate::ManualBackendTime>,
) -> tokio::task::JoinHandle<()> {
    let mut immediate_conn = begin_immediate_sqlite_connection(pool)
        .await
        .expect("begin immediate transaction");
    tokio::spawn(async move {
        if let Some(release) = release {
            release.advance(hold_for).await;
        } else {
            tokio::time::sleep(hold_for).await;
        }
        sqlx::query("ROLLBACK")
            .execute(&mut *immediate_conn)
            .await
            .expect("rollback immediate transaction");
    })
}

async fn ensure_quota_subject_lock_schema_for_test(pool: &SqlitePool) {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS quota_subject_locks (
            subject TEXT PRIMARY KEY,
            owner TEXT NOT NULL,
            expires_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await
    .expect("create quota_subject_locks table");
    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_quota_subject_locks_expires_at
           ON quota_subject_locks(expires_at)"#,
    )
    .execute(pool)
    .await
    .expect("create quota_subject_locks expires index");
}

#[tokio::test]
async fn quota_subject_lock_retries_transient_sqlite_write_lock() {
    let db_path = temp_db_path("quota-subject-lock-retries-sqlite-lock");
    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_millis(1));
    let pool = SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(5)
        .connect_with(options)
        .await
        .expect("busy-test pool");
    let store = KeyStore {
        database_path: db_path.to_string_lossy().into_owned(),
        observability_database_path: None,
        pool,
        backend_time: BackendTime::system(),
        token_binding_cache: RwLock::new(std::collections::HashMap::new()),
        account_quota_resolution_cache: RwLock::new(std::collections::HashMap::new()),
        request_logs_catalog_cache: RwLock::new(std::collections::HashMap::new()),
        request_log_retention_cache: RwLock::new(None),
        user_debug_info_shared_cache: RwLock::new(std::collections::HashMap::new()),
        request_stats_coalescer: RequestStatsCoalescer::default(),
        admin_heavy_read_semaphore: Semaphore::new(ADMIN_HEAVY_READ_CONCURRENCY),
        #[cfg(test)]
        forced_pending_claim_miss_log_ids: Mutex::new(std::collections::HashSet::new()),
        forced_quota_subject_lock_loss_subjects: std::sync::Mutex::new(
            std::collections::HashSet::new(),
        ),
    };
    ensure_quota_subject_lock_schema_for_test(&store.pool).await;
    let release = hold_sqlite_write_lock_for_test_for(&store.pool, Duration::from_millis(120)).await;

    let lease = store
        .acquire_quota_subject_lock(
            "test:quota-subject-lock-retry",
            Duration::from_secs(20),
            Duration::from_secs(30),
        )
        .await
        .expect("acquire lock after transient sqlite write lock");
    assert_eq!(lease.subject, "test:quota-subject-lock-retry");
    store
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
    let job_type = format!(
        "sqlite_lock_retry_test_{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );

    let job_id = proxy
        .scheduled_job_start(&job_type, None, 1)
        .await
        .expect("scheduled job starts after transient sqlite write lock");
    assert!(job_id > 0);
    release.await.expect("release task");

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn abandon_running_scheduled_jobs_retries_transient_sqlite_write_lock() {
    let db_path = temp_db_path("scheduled-job-abandon-retries-sqlite-lock");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let job_id = proxy
        .scheduled_job_start("sqlite_lock_retry_test", None, 1)
        .await
        .expect("scheduled job starts");
    let release = hold_sqlite_write_lock_for_test(&proxy.key_store.pool).await;

    let abandoned = proxy
        .abandon_running_scheduled_jobs()
        .await
        .expect("abandon retries after transient sqlite write lock");
    assert_eq!(abandoned, 1);
    release.await.expect("release task");

    let status: String = sqlx::query_scalar("SELECT status FROM scheduled_jobs WHERE id = ?")
        .bind(job_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read job status");
    assert_eq!(status, "abandoned");

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn scheduled_job_claim_records_trigger_source_and_rejects_duplicate_running_job() {
    let db_path = temp_db_path("scheduled-job-claim-trigger-source");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let job_id = proxy
        .scheduled_job_claim("request_logs_gc", "manual", None, 1)
        .await
        .expect("claim manual job")
        .expect("manual job claimed");
    let duplicate = proxy
        .scheduled_job_claim("request_logs_gc", "manual", None, 1)
        .await
        .expect("duplicate claim checked");
    assert!(duplicate.is_none());

    let trigger_source: String =
        sqlx::query_scalar("SELECT trigger_source FROM scheduled_jobs WHERE id = ?")
            .bind(job_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read trigger source");
    assert_eq!(trigger_source, "manual");

    proxy
        .scheduled_job_finish(job_id, "success", Some("done"))
        .await
        .expect("finish job");
    let after_finish = proxy
        .scheduled_job_claim("request_logs_gc", "manual", None, 1)
        .await
        .expect("claim after finish")
        .expect("job claimed after finish");
    assert!(after_finish > job_id);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn scheduled_job_claim_abandons_stale_quota_sync_running_job() {
    let db_path = temp_db_path("scheduled-job-claim-abandons-stale-quota-sync");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-stale-quota-sync".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
        .await
        .expect("proxy created");
    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list api key metrics")
        .into_iter()
        .next()
        .expect("seeded key exists")
        .id;
    let stale_started_at = Utc::now()
        .timestamp()
        .saturating_sub(QUOTA_SYNC_STALE_RUNNING_SECS + 5);

    sqlx::query(
        r#"
        INSERT INTO scheduled_jobs (
            job_type,
            trigger_source,
            key_id,
            status,
            attempt,
            queued_at,
            started_at
        )
        VALUES ('quota_sync', 'scheduler', ?, 'running', 1, ?, ?)
        "#,
    )
    .bind(&key_id)
    .bind(stale_started_at)
    .bind(stale_started_at)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert stale quota sync job");

    let job_id = proxy
        .scheduled_job_claim("quota_sync", "manual", Some(&key_id), 1)
        .await
        .expect("claim after stale running row")
        .expect("new quota sync job claimed");

    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT status, message FROM scheduled_jobs WHERE key_id = ? ORDER BY id ASC",
    )
    .bind(&key_id)
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("fetch quota sync rows");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, "abandoned");
    assert!(
        rows[0]
            .1
            .as_deref()
            .is_some_and(|message| message.contains("quota_sync timeout window"))
    );
    assert_eq!(rows[1].0, "running");
    assert!(job_id > 0);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn scheduled_job_claim_keeps_fresh_quota_sync_running_job() {
    let db_path = temp_db_path("scheduled-job-claim-keeps-fresh-quota-sync");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-fresh-quota-sync".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
        .await
        .expect("proxy created");
    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list api key metrics")
        .into_iter()
        .next()
        .expect("seeded key exists")
        .id;

    proxy
        .scheduled_job_start("quota_sync", Some(&key_id), 1)
        .await
        .expect("start fresh quota sync job");

    let duplicate = proxy
        .scheduled_job_claim("quota_sync", "manual", Some(&key_id), 1)
        .await
        .expect("duplicate claim checked");
    assert!(duplicate.is_none());

    let running_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM scheduled_jobs WHERE job_type = 'quota_sync' AND status = 'running' AND key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count fresh running jobs");
    assert_eq!(running_count, 1);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn scheduled_job_claim_abandons_stale_hot_quota_sync_running_job() {
    let db_path = temp_db_path("scheduled-job-claim-abandons-stale-hot-quota-sync");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-stale-hot-quota-sync".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list api key metrics")
        .into_iter()
        .next()
        .expect("seeded key exists")
        .id;
    let stale_started_at = Utc::now()
        .timestamp()
        .saturating_sub(QUOTA_SYNC_STALE_RUNNING_SECS + 5);

    sqlx::query(
        r#"
        INSERT INTO scheduled_jobs (
            job_type,
            trigger_source,
            key_id,
            status,
            attempt,
            queued_at,
            started_at
        )
        VALUES ('quota_sync/hot', 'scheduler', ?, 'running', 1, ?, ?)
        "#,
    )
    .bind(&key_id)
    .bind(stale_started_at)
    .bind(stale_started_at)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert stale hot quota sync job");

    let job_id = proxy
        .scheduled_job_claim("quota_sync/hot", "auto", Some(&key_id), 1)
        .await
        .expect("claim after stale hot running row")
        .expect("new hot quota sync job claimed");

    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT status, message FROM scheduled_jobs WHERE key_id = ? ORDER BY id ASC",
    )
    .bind(&key_id)
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("fetch hot quota sync rows");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, "abandoned");
    assert!(
        rows[0]
            .1
            .as_deref()
            .is_some_and(|message| message.contains("quota_sync timeout window"))
    );
    assert_eq!(rows[1].0, "running");
    assert!(job_id > 0);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn scheduled_job_claim_reclaims_only_quota_sync_job_types() {
    let db_path = temp_db_path("scheduled-job-claim-reclaims-only-quota-sync");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-quota-sync-scope".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list api key metrics")
        .into_iter()
        .next()
        .expect("seeded key exists")
        .id;
    let stale_started_at = Utc::now()
        .timestamp()
        .saturating_sub(QUOTA_SYNC_STALE_RUNNING_SECS + 5);

    sqlx::query(
        r#"
        INSERT INTO scheduled_jobs (
            job_type,
            trigger_source,
            status,
            attempt,
            queued_at,
            started_at
        )
        VALUES ('request_logs_gc', 'scheduler', 'running', 1, ?, ?)
        "#,
    )
    .bind(stale_started_at)
    .bind(stale_started_at)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert unrelated stale running job");

    proxy
        .scheduled_job_claim("quota_sync", "manual", Some(&key_id), 1)
        .await
        .expect("claim quota sync job")
        .expect("quota sync job claimed");

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT job_type, status FROM scheduled_jobs ORDER BY id ASC",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("fetch scheduled job rows");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], ("request_logs_gc".to_string(), "running".to_string()));
    assert_eq!(rows[1], ("quota_sync".to_string(), "running".to_string()));

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn scheduled_job_claim_serializes_concurrent_duplicate_triggers() {
    let db_path = temp_db_path("scheduled-job-claim-concurrent");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let results = futures_util::future::join_all((0..8).map(|_| {
        proxy.scheduled_job_claim("db_compaction", "manual", None, 1)
    }))
    .await;
    let claimed: Vec<i64> = results
        .into_iter()
        .filter_map(|result| result.expect("claim should not error"))
        .collect();
    assert_eq!(claimed.len(), 1);

    let running_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM scheduled_jobs WHERE job_type = 'db_compaction' AND status = 'running'")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("count running jobs");
    assert_eq!(running_count, 1);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn scheduled_job_enqueue_coalesces_duplicate_queue_and_promotes_manual_source() {
    let db_path = temp_db_path("scheduled-job-enqueue-coalesce");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let first = proxy
        .scheduled_job_enqueue("request_logs_gc", "scheduler", None, 1)
        .await
        .expect("enqueue scheduler job");
    assert!(first.created);
    assert!(!first.promoted);

    let second = proxy
        .scheduled_job_enqueue("request_logs_gc", "manual", None, 1)
        .await
        .expect("enqueue manual duplicate");
    assert_eq!(second.job_id, first.job_id);
    assert!(!second.created);
    assert!(second.promoted);
    assert_eq!(second.status, "queued");
    assert_eq!(second.trigger_source, "manual");

    let queued_jobs = proxy
        .fetch_queued_scheduled_jobs(10)
        .await
        .expect("list queued jobs");
    assert_eq!(queued_jobs.len(), 1);
    assert_eq!(queued_jobs[0].id, first.job_id);
    assert_eq!(queued_jobs[0].trigger_source, "manual");

    let row: (String, Option<i64>, i64) = sqlx::query_as(
        "SELECT trigger_source, started_at, queued_at FROM scheduled_jobs WHERE id = ?",
    )
    .bind(first.job_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("fetch queued row");
    assert_eq!(row.0, "manual");
    assert!(row.1.is_none());
    assert!(row.2 > 0);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn scheduled_job_enqueue_coalesces_running_job_and_promotes_manual_source() {
    let db_path = temp_db_path("scheduled-job-enqueue-running-coalesce");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let running_job_id = proxy
        .scheduled_job_claim("db_compaction", "scheduler", None, 1)
        .await
        .expect("claim scheduler compaction job")
        .expect("scheduler compaction job created");

    let manual = proxy
        .scheduled_job_enqueue("db_compaction", "manual", None, 1)
        .await
        .expect("enqueue manual duplicate for running job");
    assert_eq!(manual.job_id, running_job_id);
    assert!(!manual.created);
    assert!(manual.promoted);
    assert_eq!(manual.status, "running");
    assert_eq!(manual.trigger_source, "manual");

    let row: (String, String) = sqlx::query_as(
        "SELECT status, trigger_source FROM scheduled_jobs WHERE id = ?",
    )
    .bind(running_job_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("fetch running row after manual coalesce");
    assert_eq!(row.0, "running");
    assert_eq!(row.1, "manual");

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn scheduled_job_enqueue_coalesces_running_manual_job_without_waiting_for_write_lock() {
    let db_path = temp_db_path("scheduled-job-enqueue-running-manual-fast-path");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let running_job_id = proxy
        .scheduled_job_claim("request_logs_gc", "manual", None, 1)
        .await
        .expect("claim manual gc job")
        .expect("manual gc job created");
    let mut immediate_conn = begin_held_sqlite_write_lock_for_test(&proxy.key_store.pool).await;

    let started = Instant::now();
    let manual = proxy
        .scheduled_job_enqueue("request_logs_gc", "manual", None, 1)
        .await
        .expect("coalesce running manual job without write lock");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "expected fast-path coalesce, elapsed={:?}",
        started.elapsed()
    );
    assert_eq!(manual.job_id, running_job_id);
    assert!(!manual.created);
    assert!(!manual.promoted);
    assert_eq!(manual.status, "running");
    assert_eq!(manual.trigger_source, "manual");

    sqlx::query("ROLLBACK")
        .execute(&mut *immediate_conn)
        .await
        .expect("rollback immediate transaction");

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn scheduled_job_mark_running_sets_started_at_after_queue_time() {
    let db_path = temp_db_path("scheduled-job-mark-running");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let queued = proxy
        .scheduled_job_enqueue("db_compaction", "manual", None, 1)
        .await
        .expect("enqueue manual compaction");
    let row = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("mark running")
        .expect("queued row claimed by worker");
    assert_eq!(row.status, "running");
    assert!(row.started_at.is_some());
    assert!(row.started_at.expect("started_at") >= row.queued_at);

    let after: (String, i64, i64) = sqlx::query_as(
        "SELECT status, queued_at, started_at FROM scheduled_jobs WHERE id = ?",
    )
    .bind(queued.job_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("fetch running row");
    assert_eq!(after.0, "running");
    assert!(after.2 >= after.1);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn abandon_active_scheduled_jobs_abandons_queued_and_running_rows() {
    let db_path = temp_db_path("scheduled-job-abandon-active");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let queued = proxy
        .scheduled_job_enqueue("request_logs_gc", "manual", None, 1)
        .await
        .expect("enqueue manual job");
    let running = proxy
        .scheduled_job_claim("db_compaction", "auto", None, 1)
        .await
        .expect("claim running job")
        .expect("running job created");

    let abandoned = proxy
        .abandon_active_scheduled_jobs()
        .await
        .expect("abandon active jobs");
    assert_eq!(abandoned, 2);

    let rows: Vec<(i64, String, Option<i64>)> = sqlx::query_as(
        "SELECT id, status, finished_at FROM scheduled_jobs ORDER BY id ASC",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("fetch abandoned rows");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, queued.job_id);
    assert_eq!(rows[0].1, "abandoned");
    assert!(rows[0].2.is_some());
    assert_eq!(rows[1].0, running);
    assert_eq!(rows[1].1, "abandoned");
    assert!(rows[1].2.is_some());

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn abandoned_running_scheduled_jobs_unblocks_future_claims() {
    let db_path = temp_db_path("scheduled-job-abandon-running");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let stale_job_id = proxy
        .scheduled_job_claim("db_compaction", "auto", None, 1)
        .await
        .expect("claim stale job")
        .expect("stale job claimed");
    let abandoned = proxy
        .abandon_running_scheduled_jobs()
        .await
        .expect("abandon stale jobs");
    assert_eq!(abandoned, 1);

    let status: String = sqlx::query_scalar("SELECT status FROM scheduled_jobs WHERE id = ?")
        .bind(stale_job_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read abandoned status");
    assert_eq!(status, "abandoned");

    let next_job_id = proxy
        .scheduled_job_claim("db_compaction", "manual", None, 1)
        .await
        .expect("claim after abandoned")
        .expect("new job claimed");
    assert!(next_job_id > stale_job_id);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn sqlite_db_stats_reports_reclaimable_shape() {
    let db_path = temp_db_path("sqlite-db-stats-shape");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let stats = proxy.sqlite_db_stats().await.expect("db stats");
    assert!(stats.page_size > 0);
    assert!(stats.page_count > 0);
    assert!(stats.database_bytes > 0);
    assert!(stats.reclaimable_ratio >= 0.0);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn db_compaction_once_skips_when_reclaimable_space_is_below_threshold() {
    let db_path = temp_db_path("db-compaction-once-skips-below-threshold");
    let db_str = db_path.to_string_lossy().to_string();
    let _proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let report = run_db_compaction_once(&db_str, false)
        .await
        .expect("db compaction report");
    assert!(report.skipped);
    assert!(!report.forced);
    assert!(report.reason.is_some());
    assert_eq!(report.before.database_bytes, report.after.database_bytes);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn db_compaction_once_force_runs_even_below_threshold() {
    let db_path = temp_db_path("db-compaction-once-force");
    let db_str = db_path.to_string_lossy().to_string();
    let _proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let report = run_db_compaction_once(&db_str, true)
        .await
        .expect("forced db compaction report");
    assert!(!report.skipped);
    assert!(report.forced);
    assert!(report.reason.is_none());
    assert!(report.after.database_bytes > 0);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn startup_linuxdo_tag_backfill_does_not_rewrite_correct_binding() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("linuxdo-system-tags-backfill-marker");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "linuxdo-backfill-marker-user".to_string(),
            username: Some("linuxdo_backfill_marker_user".to_string()),
            name: Some("LinuxDo Backfill Marker User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert linuxdo user");
    let binding_updated_at_before: i64 = sqlx::query_scalar(
        r#"SELECT b.updated_at
           FROM user_tag_bindings b
           JOIN user_tags t ON t.id = b.tag_id
           WHERE b.user_id = ? AND t.system_key = 'linuxdo_l2'
           LIMIT 1"#,
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read binding timestamp before restart");
    let snapshot_count_before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM account_quota_limit_snapshots WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read snapshot count before restart");
    drop(proxy);

    let proxy_after = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened");
    let binding_updated_at_after: i64 = sqlx::query_scalar(
        r#"SELECT b.updated_at
           FROM user_tag_bindings b
           JOIN user_tags t ON t.id = b.tag_id
           WHERE b.user_id = ? AND t.system_key = 'linuxdo_l2'
           LIMIT 1"#,
    )
    .bind(&user.user_id)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read binding timestamp after restart");
    let snapshot_count_after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM account_quota_limit_snapshots WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read snapshot count after restart");
    assert_eq!(binding_updated_at_after, binding_updated_at_before);
    assert_eq!(snapshot_count_after, snapshot_count_before);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn linuxdo_tag_binding_refresh_rewrites_correct_binding_periodically() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("linuxdo-system-tags-periodic-refresh");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "linuxdo-periodic-refresh-user".to_string(),
            username: Some("linuxdo_periodic_refresh_user".to_string()),
            name: Some("LinuxDo Periodic Refresh User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert linuxdo user");

    let old_updated_at = Utc::now().timestamp() - 86_400;
    sqlx::query(
        r#"UPDATE user_tag_bindings
           SET updated_at = ?
           WHERE user_id = ?
             AND tag_id IN (SELECT id FROM user_tags WHERE system_key = 'linuxdo_l2')"#,
    )
    .bind(old_updated_at)
    .bind(&user.user_id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("backdate binding timestamp");
    sqlx::query("DELETE FROM account_quota_limit_snapshots WHERE user_id = ?")
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear snapshots before refresh");
    let snapshot_count_before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM account_quota_limit_snapshots WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read snapshot count before refresh");

    let refreshed = proxy
        .key_store
        .refresh_linuxdo_user_tag_bindings()
        .await
        .expect("refresh linuxdo tag bindings");
    assert_eq!(refreshed, 1);

    let binding_updated_at_after: i64 = sqlx::query_scalar(
        r#"SELECT b.updated_at
           FROM user_tag_bindings b
           JOIN user_tags t ON t.id = b.tag_id
           WHERE b.user_id = ? AND t.system_key = 'linuxdo_l2'
           LIMIT 1"#,
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read binding timestamp after refresh");
    let snapshot_count_after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM account_quota_limit_snapshots WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read snapshot count after refresh");
    assert!(binding_updated_at_after > old_updated_at);
    assert_eq!(snapshot_count_before, 0);
    assert_eq!(snapshot_count_after, 1);
    assert!(
        !proxy
            .key_store
            .linuxdo_user_tag_binding_refresh_due(86_400)
            .await
            .expect("refresh due after refresh")
    );

    let _ = std::fs::remove_file(db_path);
}

async fn seed_request_log_for_gc(pool: &SqlitePool, created_at: i64, path: &str) -> i64 {
    sqlx::query_scalar(
        r#"
        INSERT INTO observability.request_logs (
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
        INSERT INTO observability.request_log_catalog_rollups (
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
    fn set_days(days: &str) -> Self {
        let prev = std::env::var("REQUEST_LOGS_RETENTION_DAYS").ok();
        unsafe {
            std::env::set_var("REQUEST_LOGS_RETENTION_DAYS", days);
        }
        Self { prev }
    }

    fn set_32_days() -> Self {
        Self::set_days("32")
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
async fn request_log_retention_settings_clamp_days_to_max_and_reject_bad_threshold() {
    let db_path = temp_db_path("request-log-retention-settings-clamp");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let mut settings = proxy
        .get_system_settings()
        .await
        .expect("load system settings");
    proxy
        .key_store
        .set_meta_string("request_log_body_gc_cursor_v1", "1:2:9999999999")
        .await
        .expect("seed body gc cursor");
    settings.request_log_retention.max_log_retention_days = 7;
    settings.request_log_retention.global.business_body_days = 92;
    settings.request_log_retention.debug_shared.non_success_body_days = 32;
    let saved = proxy
        .set_system_settings(&settings)
        .await
        .expect("save clamped retention settings");
    assert_eq!(saved.request_log_retention.global.business_body_days, 7);
    assert_eq!(saved.request_log_retention.debug_shared.non_success_body_days, 7);
    assert!(
        proxy
            .key_store
            .get_meta_string("request_log_body_gc_cursor_v1")
            .await
            .expect("read body gc cursor")
            .is_none()
    );

    let mut invalid = saved.clone();
    invalid.request_log_retention.heavy_usage_threshold_percent = 85;
    assert!(proxy.set_system_settings(&invalid).await.is_err());

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_log_retention_settings_default_max_days_uses_env_until_saved() {
    let lock = env_lock();
    let _lock = lock.lock().await;
    let _env_guard = RequestLogsRetentionEnvGuard::set_days("60");
    let db_path = temp_db_path("request-log-retention-settings-env-default");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let settings = proxy
        .get_system_settings()
        .await
        .expect("load system settings");
    assert_eq!(settings.request_log_retention.max_log_retention_days, 60);

    let mut saved_settings = settings;
    saved_settings.request_log_retention.max_log_retention_days = 32;
    proxy
        .set_system_settings(&saved_settings)
        .await
        .expect("persist explicit settings");
    unsafe {
        std::env::set_var("REQUEST_LOGS_RETENTION_DAYS", "70");
    }
    let settings = proxy
        .get_system_settings()
        .await
        .expect("reload system settings");
    assert_eq!(settings.request_log_retention.max_log_retention_days, 32);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_log_policy_drops_non_business_body_but_keeps_metadata() {
    let db_path = temp_db_path("request-log-retention-nonbusiness-body");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(vec!["tvly-body-policy".to_string()], DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let key_id: String = sqlx::query_scalar("SELECT id FROM api_keys LIMIT 1")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("fetch key id");

    let request_body = br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    let response_body = br#"{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"search"}]}}"#;
    let log_id = proxy
        .key_store
        .log_attempt(AttemptLog {
            key_id: Some(&key_id),
            auth_token_id: None,
            method: &Method::POST,
            path: "/mcp",
            query: None,
            status: Some(StatusCode::OK),
            tavily_status_code: Some(200),
            error: None,
            request_body,
            response_body,
            outcome: OUTCOME_SUCCESS,
            failure_kind: None,
            key_effect_code: KEY_EFFECT_NONE,
            key_effect_summary: None,
            binding_effect_code: KEY_EFFECT_NONE,
            binding_effect_summary: None,
            selection_effect_code: KEY_EFFECT_NONE,
            selection_effect_summary: None,
            gateway_mode: None,
            experiment_variant: None,
            proxy_session_id: None,
            routing_subject_hash: None,
            upstream_operation: None,
            fallback_reason: None,
            forwarded_headers: &[],
            dropped_headers: &[],
            client_ip: None,
        })
        .await
        .expect("log non-business attempt");

    type BodyMetadataRow = (
        Option<Vec<u8>>,
        Option<Vec<u8>>,
        Option<i64>,
        Option<String>,
        Option<String>,
    );
    let read_pool = connect_sqlite_test_pool(&db_str).await;
    let row: BodyMetadataRow = sqlx::query_as(
        r#"
            SELECT request_body, response_body, request_body_bytes, request_body_sha256, body_cleaned_reason
            FROM observability.request_logs WHERE id = ?
            "#,
    )
    .bind(log_id)
    .fetch_one(&read_pool)
    .await
    .expect("fetch request log body metadata");
    assert!(row.0.is_none());
    assert!(row.1.is_none());
    assert_eq!(row.2, Some(request_body.len() as i64));
    assert_eq!(row.3.as_deref(), Some(sha256_hex_bytes(request_body).as_str()));
    assert_eq!(row.4.as_deref(), Some(REQUEST_LOG_BODY_CLEANED_REASON_POLICY_ZERO));

    read_pool.close().await;
    proxy.key_store.pool.close().await;
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_log_policy_preserves_batch_non_business_classification_without_body() {
    let db_path = temp_db_path("request-log-retention-batch-classification-without-body");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(vec!["tvly-body-policy-batch".to_string()], DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let key_id: String = sqlx::query_scalar("SELECT id FROM api_keys LIMIT 1")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("fetch key id");

    let request_body =
        br#"[{"jsonrpc":"2.0","id":1,"method":"initialize"},{"jsonrpc":"2.0","id":2,"method":"tools/list"}]"#;
    let response_body =
        br#"[{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05"}},{"jsonrpc":"2.0","id":2,"result":{"tools":[]}}]"#;
    let log_id = proxy
        .key_store
        .log_attempt(AttemptLog {
            key_id: Some(&key_id),
            auth_token_id: None,
            method: &Method::POST,
            path: "/mcp",
            query: None,
            status: Some(StatusCode::OK),
            tavily_status_code: Some(200),
            error: None,
            request_body,
            response_body,
            outcome: OUTCOME_SUCCESS,
            failure_kind: None,
            key_effect_code: KEY_EFFECT_NONE,
            key_effect_summary: None,
            binding_effect_code: KEY_EFFECT_NONE,
            binding_effect_summary: None,
            selection_effect_code: KEY_EFFECT_NONE,
            selection_effect_summary: None,
            gateway_mode: None,
            experiment_variant: None,
            proxy_session_id: None,
            routing_subject_hash: None,
            upstream_operation: None,
            fallback_reason: None,
            forwarded_headers: &[],
            dropped_headers: &[],
            client_ip: None,
        })
        .await
        .expect("log non-business batch attempt");

    let stored: (Option<Vec<u8>>, Option<i64>, String) = sqlx::query_as(
        "SELECT request_body, counts_business_quota, request_kind_key FROM observability.request_logs WHERE id = ?",
    )
    .bind(log_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("fetch stored batch request log");
    assert!(stored.0.is_none());
    assert_eq!(stored.1, Some(0));
    assert_eq!(stored.2, "mcp:batch");

    let direct_logs = proxy
        .key_store
        .fetch_recent_logs(10, None)
        .await
        .expect("fetch direct recent logs");
    let direct_log = direct_logs
        .iter()
        .find(|log| log.id == log_id)
        .expect("direct recent log exists");
    assert_eq!(direct_log.request_kind_billing_group, "non_billable");
    assert_eq!(direct_log.operational_class, "neutral");

    let (page_logs, _) = proxy
        .key_store
        .fetch_recent_logs_page(None, None, 1, 10)
        .await
        .expect("fetch paged recent logs");
    let page_log = page_logs
        .iter()
        .find(|log| log.id == log_id)
        .expect("paged recent log exists");
    assert_eq!(page_log.request_kind_billing_group, "non_billable");
    assert_eq!(page_log.operational_class, "neutral");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_log_policy_keeps_debug_shared_business_body() {
    let lock = env_lock();
    let _env_lock = lock.lock().await;
    let _retention_guard = RequestLogsRetentionEnvGuard::set_32_days();
    let db_path = temp_db_path("request-log-retention-debug-shared-body");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "request-log-retention-debug-shared".to_string(),
            username: Some("request_log_retention_debug_shared".to_string()),
            name: Some("Request Log Retention Debug Shared".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("request-log-retention-debug-shared"))
        .await
        .expect("bind token");
    proxy
        .key_store
        .set_meta_string("request_log_body_gc_cursor_v1", "1:2:9999999999")
        .await
        .expect("seed body gc cursor");
    proxy
        .set_user_debug_info_shared(&user.user_id, true)
        .await
        .expect("enable debug sharing");
    assert!(
        proxy
            .key_store
            .get_meta_string("request_log_body_gc_cursor_v1")
            .await
            .expect("read body gc cursor")
            .is_none()
    );
    let mut settings = proxy
        .get_system_settings()
        .await
        .expect("load settings");
    settings.request_log_retention.global.business_body_days = 0;
    settings.request_log_retention.debug_shared.business_body_days = 14;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("save retention settings");

    let request_body = br#"{"query":"debug shared"}"#;
    let response_body = br#"{"answer":"retained"}"#;
    let log_id = proxy
        .key_store
        .log_attempt(AttemptLog {
            key_id: None,
            auth_token_id: Some(&token.id),
            method: &Method::POST,
            path: "/api/tavily/search",
            query: None,
            status: Some(StatusCode::OK),
            tavily_status_code: Some(200),
            error: None,
            request_body,
            response_body,
            outcome: OUTCOME_SUCCESS,
            failure_kind: None,
            key_effect_code: KEY_EFFECT_NONE,
            key_effect_summary: None,
            binding_effect_code: KEY_EFFECT_NONE,
            binding_effect_summary: None,
            selection_effect_code: KEY_EFFECT_NONE,
            selection_effect_summary: None,
            gateway_mode: None,
            experiment_variant: None,
            proxy_session_id: None,
            routing_subject_hash: None,
            upstream_operation: None,
            fallback_reason: None,
            forwarded_headers: &[],
            dropped_headers: &[],
            client_ip: None,
        })
        .await
        .expect("log debug shared attempt");

    let row: (Option<Vec<u8>>, Option<String>) =
        sqlx::query_as(
            "SELECT request_body, body_cleaned_reason FROM observability.request_logs WHERE id = ?",
        )
        .bind(log_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("fetch request log body");
    assert_eq!(row.0.as_deref(), Some(request_body.as_slice()));
    assert!(row.1.is_none());

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_log_policy_applies_heavy_usage_business_body_days() {
    let lock = env_lock();
    let _env_lock = lock.lock().await;
    let _retention_guard = RequestLogsRetentionEnvGuard::set_32_days();
    let db_path = temp_db_path("request-log-retention-heavy-usage-body");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "request-log-retention-heavy-usage".to_string(),
            username: Some("request_log_retention_heavy_usage".to_string()),
            name: Some("Request Log Retention Heavy Usage".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("request-log-retention-heavy-usage"))
        .await
        .expect("bind token");
    proxy
        .update_account_business_quota_limits(&user.user_id, 100, 100, 10_000)
        .await
        .expect("set account quota limits");
    let day_bucket = start_of_local_day_utc_ts(Utc::now().with_timezone(&Local));
    sqlx::query(
        r#"
        INSERT INTO account_usage_buckets (user_id, bucket_start, granularity, count)
        VALUES (?, ?, ?, ?)
        "#,
    )
    .bind(&user.user_id)
    .bind(day_bucket)
    .bind(GRANULARITY_DAY)
    .bind(80_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed heavy usage bucket");
    let mut settings = proxy
        .get_system_settings()
        .await
        .expect("load settings");
    settings.request_log_retention.global.business_body_days = 7;
    settings.request_log_retention.heavy_usage.business_body_days = 0;
    settings.request_log_retention.heavy_usage_threshold_percent = 80;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("save retention settings");

    let request_body = br#"{"query":"heavy usage"}"#;
    let response_body = br#"{"answer":"dropped"}"#;
    let log_id = proxy
        .key_store
        .log_attempt(AttemptLog {
            key_id: None,
            auth_token_id: Some(&token.id),
            method: &Method::POST,
            path: "/api/tavily/search",
            query: None,
            status: Some(StatusCode::OK),
            tavily_status_code: Some(200),
            error: None,
            request_body,
            response_body,
            outcome: OUTCOME_SUCCESS,
            failure_kind: None,
            key_effect_code: KEY_EFFECT_NONE,
            key_effect_summary: None,
            binding_effect_code: KEY_EFFECT_NONE,
            binding_effect_summary: None,
            selection_effect_code: KEY_EFFECT_NONE,
            selection_effect_summary: None,
            gateway_mode: None,
            experiment_variant: None,
            proxy_session_id: None,
            routing_subject_hash: None,
            upstream_operation: None,
            fallback_reason: None,
            forwarded_headers: &[],
            dropped_headers: &[],
            client_ip: None,
        })
        .await
        .expect("log heavy usage attempt");

    let row: (Option<Vec<u8>>, Option<i64>, Option<String>) = sqlx::query_as(
        "SELECT request_body, request_body_bytes, body_cleaned_reason FROM observability.request_logs WHERE id = ?",
    )
    .bind(log_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("fetch request log body metadata");
    assert!(row.0.is_some());
    assert_eq!(row.1, Some(request_body.len() as i64));
    assert!(row.2.is_none());

    let report = proxy
        .gc_request_logs_with_options(RequestLogsGcOptions {
            batch_size: 10,
            max_batches: 1,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("run heavy usage gc");
    assert_eq!(report.cleaned_request_log_bodies, 1);

    let row: (Option<Vec<u8>>, Option<String>) =
        sqlx::query_as(
            "SELECT request_body, body_cleaned_reason FROM observability.request_logs WHERE id = ?",
        )
            .bind(log_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("fetch cleaned heavy usage body");
    assert!(row.0.is_none());
    assert_eq!(
        row.1.as_deref(),
        Some(REQUEST_LOG_BODY_CLEANED_REASON_POLICY_ZERO)
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_logs_gc_clears_expired_body_without_deleting_visible_row() {
    let lock = env_lock();
    let _env_lock = lock.lock().await;
    let _retention_guard = RequestLogsRetentionEnvGuard::set_32_days();
    let db_path = temp_db_path("request-log-retention-gc-clears-body");
    let db_str = db_path.to_string_lossy().to_string();
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_000_000);
    let proxy = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        crate::TavilyProxyOptions::from_database_path(&db_str),
        backend_time,
    )
        .await
        .expect("proxy created");
    let mut settings = proxy
        .get_system_settings()
        .await
        .expect("load settings");
    settings.request_log_retention.max_log_retention_days = 32;
    settings.request_log_retention.global.business_body_days = 1;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("save retention settings");

    manual_clock.set_now_ts(1_700_000_000);
    let old_ts = manual_clock.now_ts() - 2 * SECS_PER_DAY;
    let log_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO observability.request_logs (
            method, path, result_status, request_kind_key, request_body, response_body,
            visibility, created_at
        ) VALUES ('POST', '/api/tavily/search', 'success', 'api:search', ?, ?, ?, ?)
        RETURNING id
        "#,
    )
    .bind(br#"{"query":"old"}"#.as_slice())
    .bind(br#"{"ok":true}"#.as_slice())
    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
    .bind(old_ts)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("seed old request log body");

    let report = proxy
        .gc_request_logs_with_options(RequestLogsGcOptions {
            batch_size: 10,
            max_batches: 1,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("run request logs gc");
    assert_eq!(report.cleaned_request_log_bodies, 1);
    assert_eq!(report.deleted_request_logs, 0);

    let row: (Option<Vec<u8>>, Option<Vec<u8>>, Option<String>) = sqlx::query_as(
        "SELECT request_body, response_body, body_cleaned_reason FROM observability.request_logs WHERE id = ?",
    )
    .bind(log_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("fetch cleaned request log row");
    assert!(row.0.is_none());
    assert!(row.1.is_none());
    assert_eq!(row.2.as_deref(), Some(REQUEST_LOG_BODY_CLEANED_REASON_RETENTION_EXPIRED));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_logs_gc_skips_body_cleanup_for_rows_past_row_retention() {
    let db_path = temp_db_path("request-log-retention-gc-skips-body-cleanup-for-old-row");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let mut settings = proxy
        .get_system_settings()
        .await
        .expect("load settings");
    settings.request_log_retention.max_log_retention_days = 32;
    settings.request_log_retention.global.business_body_days = 0;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("save retention settings");

    let old_ts = Utc::now().timestamp() - 40 * SECS_PER_DAY;
    let log_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO observability.request_logs (
            method, path, result_status, request_kind_key, request_body, response_body,
            visibility, created_at
        ) VALUES ('POST', '/api/tavily/search', 'success', 'api:search', ?, NULL, ?, ?)
        RETURNING id
        "#,
    )
    .bind(br#"{"query":"row retention wins"}"#.as_slice())
    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
    .bind(old_ts)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("seed request log beyond row retention");

    let report = proxy
        .gc_request_logs_with_options(RequestLogsGcOptions {
            batch_size: 10,
            max_batches: 1,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("run request logs gc");
    assert_eq!(report.cleaned_request_log_bodies, 0);
    assert_eq!(report.deleted_request_logs, 1);

    let remaining: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM observability.request_logs WHERE id = ?")
            .bind(log_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("count old request log rows");
    assert_eq!(remaining, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn user_debug_info_shared_caches_enabled_lookup_until_local_update() {
    let db_path = temp_db_path("request-log-retention-debug-sharing-true-cache");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "request-log-debug-sharing-cache".to_string(),
            username: Some("request_log_debug_sharing_cache".to_string()),
            name: Some("Request Log Debug Sharing Cache".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    proxy
        .set_user_debug_info_shared(&user.user_id, true)
        .await
        .expect("enable debug sharing");
    assert!(
        proxy
            .key_store
            .user_debug_info_shared(&user.user_id)
            .await
            .expect("load debug sharing from cache")
    );

    sqlx::query("UPDATE users SET debug_info_shared = 0 WHERE id = ?")
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("simulate remote debug sharing opt-out");
    assert!(
        proxy
            .key_store
            .user_debug_info_shared(&user.user_id)
            .await
            .expect("debug sharing true is cached briefly")
    );

    proxy
        .set_user_debug_info_shared(&user.user_id, false)
        .await
        .expect("disable debug sharing locally");
    assert!(
        !proxy
            .key_store
            .user_debug_info_shared(&user.user_id)
            .await
            .expect("local update refreshes debug sharing cache")
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_logs_gc_reevaluates_persisted_body_retention_days_after_policy_change() {
    let lock = env_lock();
    let _env_lock = lock.lock().await;
    let _retention_guard = RequestLogsRetentionEnvGuard::set_32_days();
    let db_path = temp_db_path("request-log-retention-gc-policy-change");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let mut settings = proxy
        .get_system_settings()
        .await
        .expect("load settings");
    settings.request_log_retention.global.business_body_days = 7;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("save retention settings");

    let now = Utc::now().timestamp();
    let log_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO observability.request_logs (
            method, path, result_status, request_kind_key,
            request_body, response_body, body_retention_days, body_retention_profile,
            visibility, created_at
        ) VALUES ('POST', '/api/tavily/search', 'success', 'api:search', ?, ?, 7, 'global', ?, ?)
        RETURNING id
        "#,
    )
    .bind(br#"{"query":"policy lowered"}"#.as_slice())
    .bind(br#"{"ok":true}"#.as_slice())
    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
    .bind(now)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("seed persisted old-policy body");

    settings.request_log_retention.global.business_body_days = 0;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("lower retention settings");

    let mut row: (Option<Vec<u8>>, Option<String>, Option<i64>, Option<String>) =
        (Some(Vec::new()), None, None, None);
    let mut cleaned_reports = 0_i64;
    for _ in 0..3 {
        let report = proxy
            .gc_request_logs_with_options(RequestLogsGcOptions {
                batch_size: 10,
                max_batches: 1,
                max_runtime_secs: 30,
                inter_batch_sleep_ms: 0,
            })
            .await
            .expect("run request logs gc");
        cleaned_reports += report.cleaned_request_log_bodies;
        row = sqlx::query_as(
            "SELECT request_body, body_cleaned_reason, body_retention_days, body_retention_profile FROM observability.request_logs WHERE id = ?",
        )
        .bind(log_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("fetch cleaned body");
        if row.0.is_none() {
            break;
        }
    }
    assert_eq!(cleaned_reports, 1);
    assert!(row.0.is_none());
    assert_eq!(
        row.1.as_deref(),
        Some(REQUEST_LOG_BODY_CLEANED_REASON_POLICY_ZERO)
    );
    assert_eq!(row.2, Some(0));
    assert_eq!(row.3.as_deref(), Some("global"));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_logs_gc_honors_debug_sharing_opt_out_for_persisted_debug_profile() {
    let lock = env_lock();
    let _env_lock = lock.lock().await;
    let _retention_guard = RequestLogsRetentionEnvGuard::set_32_days();
    let db_path = temp_db_path("request-log-retention-gc-debug-opt-out");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "request-log-gc-debug-opt-out".to_string(),
            username: Some("request_log_gc_debug_opt_out".to_string()),
            name: Some("Request Log GC Debug Opt Out".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    proxy
        .set_user_debug_info_shared(&user.user_id, true)
        .await
        .expect("enable debug sharing");

    let mut settings = proxy
        .get_system_settings()
        .await
        .expect("load settings");
    settings.request_log_retention.global.business_body_days = 0;
    settings.request_log_retention.debug_shared.business_body_days = 14;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("save retention settings");

    let now = Utc::now().timestamp();
    let log_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO observability.request_logs (
            method, path, result_status, request_kind_key, request_user_id,
            request_body, response_body, body_retention_days, body_retention_profile,
            visibility, created_at
        ) VALUES ('POST', '/api/tavily/search', 'success', 'api:search', ?, ?, ?, 14, 'debug_shared', ?, ?)
        RETURNING id
        "#,
    )
    .bind(&user.user_id)
    .bind(br#"{"query":"debug opt out"}"#.as_slice())
    .bind(br#"{"ok":true}"#.as_slice())
    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
    .bind(now)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("seed debug-profile body");

    proxy
        .set_user_debug_info_shared(&user.user_id, false)
        .await
        .expect("disable debug sharing");

    let report = proxy
        .gc_request_logs_with_options(RequestLogsGcOptions {
            batch_size: 10,
            max_batches: 1,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("run request logs gc");
    assert_eq!(report.cleaned_request_log_bodies, 1);

    let row: (Option<Vec<u8>>, Option<String>) =
        sqlx::query_as(
            "SELECT request_body, body_cleaned_reason FROM observability.request_logs WHERE id = ?",
        )
        .bind(log_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("fetch cleaned opt-out row");
    assert!(row.0.is_none());
    assert_eq!(
        row.1.as_deref(),
        Some(REQUEST_LOG_BODY_CLEANED_REASON_POLICY_ZERO)
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_logs_gc_scans_past_unexpired_body_to_clear_later_expired_body() {
    let lock = env_lock();
    let _env_lock = lock.lock().await;
    let _retention_guard = RequestLogsRetentionEnvGuard::set_32_days();
    let db_path = temp_db_path("request-log-retention-gc-scans-past-unexpired");
    let db_str = db_path.to_string_lossy().to_string();
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_000_000);
    let proxy = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        crate::TavilyProxyOptions::from_database_path(&db_str),
        backend_time,
    )
    .await
    .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "request-log-gc-debug-unexpired".to_string(),
            username: Some("request_log_gc_debug_unexpired".to_string()),
            name: Some("Request Log GC Debug Unexpired".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    proxy
        .set_user_debug_info_shared(&user.user_id, true)
        .await
        .expect("enable debug sharing");
    let mut settings = proxy
        .get_system_settings()
        .await
        .expect("load settings");
    settings.request_log_retention.max_log_retention_days = 32;
    settings.request_log_retention.global.business_body_days = 7;
    settings.request_log_retention.global.non_business_body_days = 0;
    settings.request_log_retention.debug_shared.business_body_days = 14;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("save retention settings");
    manual_clock.set_now_ts(1_700_000_000);

    let now = manual_clock.now_ts();
    let unexpired_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO observability.request_logs (
            method, path, result_status, request_kind_key, request_user_id,
            request_body, response_body, visibility, created_at
        ) VALUES ('POST', '/api/tavily/search', 'success', 'api:search', ?, ?, ?, ?, ?)
        RETURNING id
        "#,
    )
    .bind(&user.user_id)
    .bind(br#"{"query":"debug still retained"}"#.as_slice())
    .bind(br#"{"ok":true}"#.as_slice())
    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
    .bind(now - SECS_PER_DAY)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("seed unexpired debug body");
    let expired_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO observability.request_logs (
            method, path, result_status, request_kind_key,
            request_body, response_body, visibility, created_at
        ) VALUES ('POST', '/mcp', 'success', 'mcp:tools/list', ?, ?, ?, ?)
        RETURNING id
        "#,
    )
    .bind(br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#.as_slice())
    .bind(br#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#.as_slice())
    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
    .bind(now)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("seed later expired non-business body");

    let report = proxy
        .gc_request_logs_with_options(RequestLogsGcOptions {
            batch_size: 10,
            max_batches: 1,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("run request logs gc");
    assert_eq!(report.cleaned_request_log_bodies, 1);

    let unexpired_body: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT request_body FROM observability.request_logs WHERE id = ?")
            .bind(unexpired_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("fetch unexpired body");
    let expired: (Option<Vec<u8>>, Option<String>) =
        sqlx::query_as(
            "SELECT request_body, body_cleaned_reason FROM observability.request_logs WHERE id = ?",
        )
        .bind(expired_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("fetch expired body");
    assert!(unexpired_body.is_some());
    assert!(expired.0.is_none());
    assert_eq!(
        expired.1.as_deref(),
        Some(REQUEST_LOG_BODY_CLEANED_REASON_POLICY_ZERO)
    );

    let second_expired_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO observability.request_logs (
            method, path, result_status, request_kind_key,
            request_body, response_body, visibility, created_at
        ) VALUES ('POST', '/mcp', 'success', 'mcp:tools/list', ?, ?, ?, ?)
        RETURNING id
        "#,
    )
    .bind(br#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#.as_slice())
    .bind(br#"{"jsonrpc":"2.0","id":2,"result":{"tools":[]}}"#.as_slice())
    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
    .bind(now + SECS_PER_MINUTE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("seed another expired non-business body");
    manual_clock.advance_wall(Duration::from_secs(SECS_PER_MINUTE as u64 + 1));
    let second_report = proxy
        .gc_request_logs_with_options(RequestLogsGcOptions {
            batch_size: 10,
            max_batches: 1,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("run request logs gc with fresh body left behind");
    assert_eq!(second_report.cleaned_request_log_bodies, 1);
    assert!(second_report.completed);
    assert!(!second_report.has_more);
    let second_expired_body: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT request_body FROM observability.request_logs WHERE id = ?")
            .bind(second_expired_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("fetch second expired body");
    assert!(second_expired_body.is_none());

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_logs_gc_resumes_body_scan_after_unexpired_scan_limit() {
    let lock = env_lock();
    let _env_lock = lock.lock().await;
    let _retention_guard = RequestLogsRetentionEnvGuard::set_32_days();
    let db_path = temp_db_path("request-log-retention-gc-body-cursor");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "request-log-gc-cursor-user".to_string(),
            username: Some("request_log_gc_cursor_user".to_string()),
            name: Some("Request Log GC Cursor User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    proxy
        .set_user_debug_info_shared(&user.user_id, true)
        .await
        .expect("enable debug sharing");
    let mut settings = proxy
        .get_system_settings()
        .await
        .expect("load settings");
    settings.request_log_retention.global.non_business_body_days = 0;
    settings.request_log_retention.debug_shared.business_body_days = 14;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("save retention settings");

    let now = Utc::now().timestamp();
    for idx in 0..64 {
        sqlx::query(
            r#"
            INSERT INTO observability.request_logs (
                method, path, result_status, request_kind_key, request_user_id,
                request_body, response_body, visibility, created_at
            ) VALUES ('POST', '/api/tavily/search', 'success', 'api:search', ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&user.user_id)
        .bind(format!(r#"{{"query":"debug retained {idx}"}}"#).as_bytes().to_vec())
        .bind(br#"{"ok":true}"#.as_slice())
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(now - SECS_PER_DAY + idx)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed unexpired debug body");
    }

    let expired_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO observability.request_logs (
            method, path, result_status, request_kind_key,
            request_body, response_body, visibility, created_at
        ) VALUES ('POST', '/mcp', 'success', 'mcp:tools/list', ?, ?, ?, ?)
        RETURNING id
        "#,
    )
    .bind(br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#.as_slice())
    .bind(br#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#.as_slice())
    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
    .bind(now + 1)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("seed expired non-business body");

    let first_report = proxy
        .gc_request_logs_with_options(RequestLogsGcOptions {
            batch_size: 1,
            max_batches: 1,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("run first request logs gc");
    assert_eq!(first_report.cleaned_request_log_bodies, 0);
    assert!(first_report.has_more);

    let retained_body: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT request_body FROM observability.request_logs WHERE id = ?")
            .bind(expired_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("fetch expired body before resumed gc");
    assert!(retained_body.is_some());

    let second_report = proxy
        .gc_request_logs_with_options(RequestLogsGcOptions {
            batch_size: 10,
            max_batches: 1,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("run resumed request logs gc");
    assert_eq!(second_report.cleaned_request_log_bodies, 1);
    assert!(second_report.completed);
    assert!(!second_report.has_more);

    let cleaned_body: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT request_body FROM observability.request_logs WHERE id = ?")
            .bind(expired_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("fetch expired body after resumed gc");
    assert!(cleaned_body.is_none());

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_logs_gc_restarts_body_scan_when_cursor_restart_time_is_due() {
    let lock = env_lock();
    let _env_lock = lock.lock().await;
    let _retention_guard = RequestLogsRetentionEnvGuard::set_32_days();
    let db_path = temp_db_path("request-log-retention-gc-body-cursor-restart");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let now = Utc::now().timestamp();
    let expired_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO observability.request_logs (
            method, path, result_status, request_kind_key,
            request_body, response_body, visibility, created_at
        ) VALUES ('POST', '/mcp', 'success', 'mcp:tools/list', ?, ?, ?, ?)
        RETURNING id
        "#,
    )
    .bind(br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#.as_slice())
    .bind(br#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#.as_slice())
    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
    .bind(now)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("seed zero-day body before stale cursor");

    proxy
        .key_store
        .set_meta_string(
            "request_log_body_gc_cursor_v1",
            &format!("{now}:{expired_id}:{}", now - 1),
        )
        .await
        .expect("seed due body gc cursor");

    let report = proxy
        .gc_request_logs_with_options(RequestLogsGcOptions {
            batch_size: 10,
            max_batches: 1,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("run request logs gc with due cursor restart");
    assert_eq!(report.cleaned_request_log_bodies, 1);
    assert!(report.completed);
    assert!(!report.has_more);

    let row: (Option<Vec<u8>>, Option<String>) =
        sqlx::query_as(
            "SELECT request_body, body_cleaned_reason FROM observability.request_logs WHERE id = ?",
        )
        .bind(expired_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("fetch restarted cursor row");
    assert!(row.0.is_none());
    assert_eq!(
        row.1.as_deref(),
        Some(REQUEST_LOG_BODY_CLEANED_REASON_POLICY_ZERO)
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_logs_gc_continues_when_body_scan_only_advances_cursor() {
    let lock = env_lock();
    let _env_lock = lock.lock().await;
    let _retention_guard = RequestLogsRetentionEnvGuard::set_32_days();
    let db_path = temp_db_path("request-log-retention-gc-body-cursor-continues");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "request-log-gc-cursor-continue-user".to_string(),
            username: Some("request_log_gc_cursor_continue_user".to_string()),
            name: Some("Request Log GC Cursor Continue User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    proxy
        .set_user_debug_info_shared(&user.user_id, true)
        .await
        .expect("enable debug sharing");
    let mut settings = proxy
        .get_system_settings()
        .await
        .expect("load settings");
    settings.request_log_retention.global.non_business_body_days = 0;
    settings.request_log_retention.debug_shared.business_body_days = 14;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("save retention settings");

    let now = Utc::now().timestamp();
    for idx in 0..64 {
        sqlx::query(
            r#"
            INSERT INTO observability.request_logs (
                method, path, result_status, request_kind_key, request_user_id,
                request_body, response_body, visibility, created_at
            ) VALUES ('POST', '/api/tavily/search', 'success', 'api:search', ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&user.user_id)
        .bind(format!(r#"{{"query":"debug retained {idx}"}}"#).as_bytes().to_vec())
        .bind(br#"{"ok":true}"#.as_slice())
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(now - SECS_PER_DAY + idx)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed unexpired debug body");
    }

    let expired_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO observability.request_logs (
            method, path, result_status, request_kind_key,
            request_body, response_body, visibility, created_at
        ) VALUES ('POST', '/mcp', 'success', 'mcp:tools/list', ?, ?, ?, ?)
        RETURNING id
        "#,
    )
    .bind(br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#.as_slice())
    .bind(br#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#.as_slice())
    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
    .bind(now + 1)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("seed expired non-business body");

    let report = proxy
        .gc_request_logs_with_options(RequestLogsGcOptions {
            batch_size: 1,
            max_batches: 3,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("run multi-batch request logs gc");
    assert_eq!(report.cleaned_request_log_bodies, 1);
    assert!(report.completed);
    assert!(!report.has_more);

    let cleaned_body: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT request_body FROM observability.request_logs WHERE id = ?")
            .bind(expired_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("fetch expired body after continued gc");
    assert!(cleaned_body.is_none());

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_logs_gc_preserves_cursor_until_retained_bodies_expire() {
    let lock = env_lock();
    let _env_lock = lock.lock().await;
    let _retention_guard = RequestLogsRetentionEnvGuard::set_32_days();
    let db_path = temp_db_path("request-log-retention-gc-body-cursor-preserved");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "request-log-gc-cursor-preserve-user".to_string(),
            username: Some("request_log_gc_cursor_preserve_user".to_string()),
            name: Some("Request Log GC Cursor Preserve User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    proxy
        .set_user_debug_info_shared(&user.user_id, true)
        .await
        .expect("enable debug sharing");
    let mut settings = proxy
        .get_system_settings()
        .await
        .expect("load settings");
    settings.request_log_retention.debug_shared.business_body_days = 14;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("save retention settings");

    let now = Utc::now().timestamp();
    let mut first_created_at = 0_i64;
    let mut last_created_at = 0_i64;
    for idx in 0..3 {
        last_created_at = now - SECS_PER_DAY + idx;
        if idx == 0 {
            first_created_at = last_created_at;
        }
        let _: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO observability.request_logs (
                method, path, result_status, request_kind_key, request_user_id,
                request_body, response_body, visibility, created_at
            ) VALUES ('POST', '/api/tavily/search', 'success', 'api:search', ?, ?, ?, ?, ?)
            RETURNING id
            "#,
        )
        .bind(&user.user_id)
        .bind(format!(r#"{{"query":"retained {idx}"}}"#).as_bytes().to_vec())
        .bind(br#"{"ok":true}"#.as_slice())
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(last_created_at)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("seed retained body");
    }

    let report = proxy
        .gc_request_logs_with_options(RequestLogsGcOptions {
            batch_size: 10,
            max_batches: 1,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("run request logs gc over retained bodies");
    assert_eq!(report.cleaned_request_log_bodies, 0);
    assert!(report.completed);
    assert!(!report.has_more);

    let cursor = proxy
        .key_store
        .get_meta_string("request_log_body_gc_cursor_v1")
        .await
        .expect("fetch body gc cursor")
        .expect("retained bodies should keep restart cursor");
    let parts = cursor.split(':').collect::<Vec<_>>();
    assert_eq!(parts.len(), 3);
    let cursor_created_at = parts[0].parse::<i64>().expect("cursor created_at");
    assert!(cursor_created_at >= first_created_at);
    assert!(cursor_created_at <= last_created_at);
    let cursor_id = parts[1].parse::<i64>().expect("cursor id");
    assert!(cursor_id > 0);
    let restart_at = parts[2].parse::<i64>().expect("cursor restart_at");
    assert!(restart_at > now);
    assert_eq!(
        restart_at,
        shift_local_day_start_utc_ts(local_day_bucket_start_utc_ts(cursor_created_at), 14)
            .max(now)
    );

    let second_report = proxy
        .gc_request_logs_with_options(RequestLogsGcOptions {
            batch_size: 10,
            max_batches: 1,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("run request logs gc before retained bodies expire");
    assert_eq!(second_report.cleaned_request_log_bodies, 0);
    assert!(second_report.completed);
    assert!(!second_report.has_more);
    let next_cursor = proxy
        .key_store
        .get_meta_string("request_log_body_gc_cursor_v1")
        .await
        .expect("refetch body gc cursor")
        .expect("retained bodies should keep restart cursor after another pass");
    let next_parts = next_cursor.split(':').collect::<Vec<_>>();
    assert_eq!(next_parts.len(), 3);
    assert!(
        next_parts[2]
            .parse::<i64>()
            .expect("next cursor restart_at")
            > now
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn standalone_request_logs_gc_upgrades_legacy_body_metadata_columns() {
    let db_path = temp_db_path("request-log-retention-standalone-gc-upgrades-body-columns");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    for column in [
        "request_body_bytes",
        "response_body_bytes",
        "request_body_sha256",
        "response_body_sha256",
        "body_cleaned_reason",
        "body_cleaned_at",
    ] {
        sqlx::query(&format!("ALTER TABLE request_logs DROP COLUMN {column}"))
            .execute(&proxy.key_store.pool)
            .await
            .expect("drop request log body metadata column");
    }
    let old_ts = Utc::now().timestamp() - SECS_PER_DAY;
    let log_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO request_logs (
            method, path, result_status, request_kind_key,
            request_body, response_body, visibility, created_at
        ) VALUES ('POST', '/mcp', 'success', 'mcp:tools/list', ?, ?, ?, ?)
        RETURNING id
        "#,
    )
    .bind(br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#.as_slice())
    .bind(br#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#.as_slice())
    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
    .bind(old_ts)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("seed legacy request log body");
    sqlx::query("DROP TRIGGER IF EXISTS trg_request_logs_canonical_request_kind_update")
        .execute(&proxy.key_store.pool)
        .await
        .expect("drop canonical request kind update trigger");
    sqlx::query("DROP TRIGGER IF EXISTS trg_request_logs_canonical_request_kind_insert")
        .execute(&proxy.key_store.pool)
        .await
        .expect("drop canonical request kind insert trigger");
    sqlx::query(
        "UPDATE request_logs SET request_kind_key = 'mcp:raw:legacy-tools-list',
            request_kind_label = 'Legacy tools list', request_kind_detail = NULL WHERE id = ?",
    )
    .bind(log_id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("mark request log as legacy request kind");
    drop(proxy);

    let report = run_request_logs_gc_once(
        &db_str,
        RequestLogsGcOptions {
            batch_size: 10,
            max_batches: 1,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        },
    )
    .await
    .expect("standalone request logs gc upgrades schema and cleans body");
    assert_eq!(report.cleaned_request_log_bodies, 1);

    let pool = open_sqlite_pool_with_observability(
        &layout.core_database_path,
        layout.observability_database_path.as_deref(),
        true,
        false,
    )
        .await
        .expect("open sqlite pool");
    let row: (Option<Vec<u8>>, Option<i64>, Option<String>, String) = sqlx::query_as(
        "SELECT request_body, request_body_bytes, body_cleaned_reason, request_kind_key FROM request_logs WHERE id = ?",
    )
    .bind(log_id)
    .fetch_one(&pool)
    .await
    .expect("fetch upgraded cleaned request log");
    assert!(row.0.is_none());
    assert!(row.1.is_some());
    assert_eq!(row.2.as_deref(), Some(REQUEST_LOG_BODY_CLEANED_REASON_POLICY_ZERO));
    assert_eq!(row.3, "mcp:tools/list");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn standalone_request_logs_gc_initializes_meta_for_legacy_table() {
    let db_path = temp_db_path("request-log-retention-standalone-gc-legacy-meta");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("open legacy sqlite pool");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            method TEXT,
            path TEXT,
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create legacy request logs table");
    sqlx::query("INSERT INTO request_logs (method, path, created_at) VALUES ('POST', '/mcp', 0)")
        .execute(&pool)
        .await
        .expect("seed legacy request log");
    let meta_exists_before: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'meta'")
            .fetch_optional(&pool)
            .await
            .expect("check legacy meta table");
    assert!(meta_exists_before.is_none());
    drop(pool);

    let report = run_request_logs_gc_once(
        &db_str,
        RequestLogsGcOptions {
            batch_size: 10,
            max_batches: 1,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        },
    )
    .await
    .expect("standalone request logs gc initializes meta");
    assert_eq!(report.deleted_request_logs, 1);

    let pool = open_sqlite_pool_with_observability(
        &layout.core_database_path,
        layout.observability_database_path.as_deref(),
        true,
        false,
    )
        .await
        .expect("reopen sqlite pool");
    let meta_exists_after: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'meta'")
            .fetch_optional(&pool)
            .await
            .expect("check initialized meta table");
    assert_eq!(meta_exists_after, Some(1));
    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM request_logs")
        .fetch_one(&pool)
        .await
        .expect("count remaining request logs");
    assert_eq!(remaining, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn standalone_request_logs_gc_uses_large_legacy_single_db_layout() {
    let db_path = temp_db_path("request-log-retention-standalone-gc-large-legacy-layout");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("open legacy sqlite pool");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            method TEXT,
            path TEXT,
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create legacy request logs table");
    sqlx::query("INSERT INTO request_logs (method, path, created_at) VALUES ('POST', '/mcp', 0)")
        .execute(&pool)
        .await
        .expect("seed legacy request log");
    drop(pool);

    std::fs::OpenOptions::new()
        .write(true)
        .open(&db_path)
        .expect("open sqlite file for resize")
        .set_len(LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES + 4096)
        .expect("expand sqlite file");

    let report = run_request_logs_gc_once(
        &db_str,
        RequestLogsGcOptions {
            batch_size: 10,
            max_batches: 1,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        },
    )
    .await
    .expect("standalone request logs gc initializes meta against large legacy layout");
    assert_eq!(report.deleted_request_logs, 1);

    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(false),
    )
    .await
    .expect("reopen legacy sqlite pool");
    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM request_logs")
        .fetch_one(&pool)
        .await
        .expect("count remaining legacy request logs");
    assert_eq!(remaining, 0);
    let observability_path = layout
        .observability_database_path
        .as_deref()
        .expect("sidecar path should still be derivable");
    assert!(
        !std::path::Path::new(observability_path).exists(),
        "large legacy GC should not create a sidecar file"
    );

    let _ = std::fs::remove_file(db_path);
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
    assert_eq!(report.deleted_rollups, 1);
    let old_exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM observability.request_logs WHERE id = ?")
            .bind(old_id)
            .fetch_optional(&proxy.key_store.pool)
            .await
            .expect("query old log");
    let recent_exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM observability.request_logs WHERE id = ?")
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
    assert_eq!(second.deleted_rollups, 2);

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
    let old_exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM observability.request_logs WHERE id = ?")
            .bind(old_id)
            .fetch_optional(&proxy.key_store.pool)
            .await
            .expect("query old log after locked gc");
    assert!(old_exists.is_none());

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn request_logs_gc_body_cleanup_retries_transient_sqlite_write_lock() {
    let lock = env_lock();
    let _env_lock = lock.lock().await;
    let db_path = temp_db_path("request-logs-gc-body-cleanup-retries-sqlite-lock");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let mut settings = proxy
        .get_system_settings()
        .await
        .expect("load settings");
    settings.request_log_retention.max_log_retention_days = 32;
    settings.request_log_retention.global.business_body_days = 0;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("save retention settings");

    let log_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO observability.request_logs (
            method, path, result_status, request_kind_key, request_body, response_body,
            visibility, created_at
        ) VALUES ('POST', '/api/tavily/search', 'success', 'api:search', ?, ?, ?, ?)
        RETURNING id
        "#,
    )
    .bind(br#"{"query":"body cleanup lock retry"}"#.as_slice())
    .bind(br#"{"ok":true}"#.as_slice())
    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
    .bind(Utc::now().timestamp())
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("seed request log body");

    let release = hold_sqlite_write_lock_for_test(&proxy.key_store.pool).await;
    let report = proxy
        .gc_request_logs_with_options(RequestLogsGcOptions {
            batch_size: 10,
            max_batches: 1,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("request log body cleanup retries after transient sqlite write lock");
    release.await.expect("release task");

    assert_eq!(report.deleted_request_logs, 0);
    let body: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT request_body FROM observability.request_logs WHERE id = ?")
            .bind(log_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("fetch cleaned request log body");
    assert!(body.is_none());
    assert!(report.cleaned_request_log_bodies <= 1);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}
