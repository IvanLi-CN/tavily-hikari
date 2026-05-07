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
