use super::*;

#[tokio::test]
async fn scheduled_job_start_respects_control_transaction_budget_under_sqlite_write_lock() {
    let db_path = temp_db_path("scheduled-job-start-control-budget-sqlite-lock");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let release =
        hold_sqlite_write_lock_for_test_for(&proxy.key_store.pool, Duration::from_millis(500))
            .await;
    let job_type = format!(
        "sqlite_lock_retry_test_{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );

    let started_at = std::time::Instant::now();
    let err = proxy
        .scheduled_job_start(&job_type, None, 1)
        .await
        .expect_err("control transaction should defer instead of retrying behind a writer lock");
    assert!(
        is_transient_sqlite_write_error(&err),
        "expected bounded SQLite writer contention error, got {err}"
    );
    assert!(
        started_at.elapsed() < Duration::from_millis(250),
        "control transaction must not wait behind bulk SQLite work (elapsed={:?})",
        started_at.elapsed()
    );
    release.await.expect("release task");

    let job_id = proxy
        .scheduled_job_start(&job_type, None, 1)
        .await
        .expect("durable job can be submitted after the writer lock releases");
    assert!(job_id > 0);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn scheduled_job_enqueue_reuses_ha_gc_representative_under_writer_lock() {
    let db_path = temp_db_path("scheduled-job-enqueue-ha-gc-writer-lock");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let queued = proxy
        .scheduled_job_enqueue("ha_outbox_gc", "scheduler", None, 1)
        .await
        .expect("enqueue HA GC representative");
    let mut immediate_conn = begin_held_sqlite_write_lock_for_test(&proxy.key_store.pool).await;

    let started = Instant::now();
    let reused = proxy
        .scheduled_job_enqueue("ha_outbox_gc", "manual", None, 1)
        .await
        .expect("manual HA GC trigger reuses durable representative under lock");
    assert!(
        started.elapsed() < Duration::from_millis(250),
        "manual coalesce must not wait for the writer lock (elapsed={:?})",
        started.elapsed()
    );
    assert_eq!(reused.job_id, queued.job_id);
    assert!(!reused.created);
    assert!(!reused.promoted);
    assert_eq!(reused.status, "queued");
    assert_eq!(reused.trigger_source, "scheduler");

    sqlx::query("ROLLBACK")
        .execute(&mut *immediate_conn)
        .await
        .expect("release writer lock");

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}
