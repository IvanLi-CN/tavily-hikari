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

#[tokio::test]
async fn foreground_manual_enqueue_waits_for_short_pool_pressure() {
    let db_path = temp_db_path("foreground-manual-enqueue-pool-pressure");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let first = proxy
        .key_store
        .pool
        .acquire()
        .await
        .expect("hold first pool connection");
    let second = proxy
        .key_store
        .pool
        .acquire()
        .await
        .expect("hold second pool connection");
    let third = proxy
        .key_store
        .pool
        .acquire()
        .await
        .expect("hold third pool connection");
    let release = async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        drop((first, second, third));
    };

    let started = Instant::now();
    let enqueue = proxy.scheduled_job_enqueue_foreground("ha_outbox_gc", "manual", None, 1);
    let (job, ()) = tokio::join!(enqueue, release);
    let job = job.expect("foreground manual enqueue after short pool pressure");
    assert!(job.created);
    assert!(
        started.elapsed() < Duration::from_millis(250),
        "foreground enqueue must fit its bounded acquisition window (elapsed={:?})",
        started.elapsed()
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn scheduler_queue_reads_defer_before_waiting_behind_a_saturated_pool() {
    let db_path = temp_db_path("scheduled-job-queue-read-admission");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let first = proxy
        .key_store
        .pool
        .acquire()
        .await
        .expect("first connection");
    let second = proxy
        .key_store
        .pool
        .acquire()
        .await
        .expect("second connection");
    let third = proxy
        .key_store
        .pool
        .acquire()
        .await
        .expect("third connection");

    let started = Instant::now();
    let err = proxy
        .fetch_queued_scheduled_jobs(16)
        .await
        .expect_err("scheduler dequeue must defer before becoming a long-lived pool waiter");
    assert!(is_transient_sqlite_write_error(&err));
    assert!(
        started.elapsed() < Duration::from_millis(250),
        "control queue read exceeded its admission budget: {:?}",
        started.elapsed()
    );
    let next_wake_started = Instant::now();
    let next_wake_err = proxy
        .next_queued_scheduled_job_available_at()
        .await
        .expect_err("scheduler next-wake read must use the same bounded control admission");
    assert!(is_transient_sqlite_write_error(&next_wake_err));
    assert!(
        next_wake_started.elapsed() < Duration::from_millis(250),
        "next-wake control read exceeded its admission budget: {:?}",
        next_wake_started.elapsed()
    );
    drop((third, second, first));

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn reconciliation_candidate_preparation_defers_before_a_saturated_pool() {
    let db_path = temp_db_path("reconciliation-preparation-admission");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let queued = proxy
        .scheduled_job_enqueue("upstream_reconciliation", "scheduler", None, 1)
        .await
        .expect("enqueue durable reconciliation representative");
    let claimed = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim durable reconciliation representative")
        .expect("representative became running");
    let first = proxy
        .key_store
        .pool
        .acquire()
        .await
        .expect("first connection");
    let second = proxy
        .key_store
        .pool
        .acquire()
        .await
        .expect("second connection");
    let third = proxy
        .key_store
        .pool
        .acquire()
        .await
        .expect("third connection");

    let started = Instant::now();
    let outcome = proxy
        .run_upstream_reconciliation_once_claimed_outcome(
            DEFAULT_UPSTREAM,
            claimed.id,
            claimed.claim_generation,
        )
        .await
        .expect("admission defer is a typed reconciliation outcome");
    assert!(matches!(
        outcome,
        crate::tavily_proxy::ClaimedReconciliationRunOutcome::Deferred {
            reason: "pool_pressure"
        }
    ));
    assert!(
        started.elapsed() < Duration::from_millis(250),
        "reconciliation must defer before candidate reads wait on foreground pool capacity: {:?}",
        started.elapsed()
    );
    drop((third, second, first));
    let status: String = sqlx::query_scalar("SELECT status FROM scheduled_jobs WHERE id = ?")
        .bind(claimed.id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read claimed representative after typed defer");
    assert_eq!(
        status, "running",
        "the engine must not finish an unexecuted representative; scheduler owns its durable defer handoff"
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn reconciliation_claim_fence_and_run_marker_use_control_budget() {
    let db_path = temp_db_path("reconciliation-control-marker-admission");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let queued = proxy
        .scheduled_job_enqueue("upstream_reconciliation", "scheduler", None, 1)
        .await
        .expect("enqueue durable reconciliation representative");
    let claimed = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim durable reconciliation representative")
        .expect("representative became running");
    let first = proxy
        .key_store
        .pool
        .acquire()
        .await
        .expect("first connection");
    let second = proxy
        .key_store
        .pool
        .acquire()
        .await
        .expect("second connection");
    let third = proxy
        .key_store
        .pool
        .acquire()
        .await
        .expect("third connection");

    let claim_started = Instant::now();
    let claim_err = proxy
        .key_store
        .scheduled_job_claim_is_current(claimed.id, claimed.claim_generation)
        .await
        .expect_err("claim fence must not wait behind foreground pool saturation");
    assert!(is_transient_sqlite_write_error(&claim_err));
    assert!(
        claim_started.elapsed() < Duration::from_millis(250),
        "claim fence exceeded its control budget: {:?}",
        claim_started.elapsed()
    );

    let marker_started = Instant::now();
    let marker_err = proxy
        .key_store
        .mark_upstream_reconciliation_run_completed_at(Utc::now().timestamp())
        .await
        .expect_err("run marker must not wait behind foreground pool saturation");
    assert!(is_transient_sqlite_write_error(&marker_err));
    assert!(
        marker_started.elapsed() < Duration::from_millis(250),
        "run marker exceeded its control budget: {:?}",
        marker_started.elapsed()
    );

    let stats_started = Instant::now();
    let stats_err = proxy
        .key_store
        .record_upstream_reconciliation_run_stats(1, 1, 0, 0, 0, false)
        .await
        .expect_err("run stats must not wait behind foreground pool saturation");
    assert!(is_transient_sqlite_write_error(&stats_err));
    assert!(
        stats_started.elapsed() < Duration::from_millis(250),
        "run stats exceeded its control budget: {:?}",
        stats_started.elapsed()
    );

    let continuation_started = Instant::now();
    let continuation_err = proxy
        .key_store
        .upstream_reconciliation_continuation_at()
        .await
        .expect_err("continuation discovery must not wait behind foreground pool saturation");
    assert!(is_transient_sqlite_write_error(&continuation_err));
    assert!(
        continuation_started.elapsed() < Duration::from_millis(250),
        "continuation discovery exceeded its control budget: {:?}",
        continuation_started.elapsed()
    );

    let projection_started = Instant::now();
    let projection_err = proxy
        .key_store
        .advance_upstream_reconciliation_work_projection()
        .await
        .expect_err("legacy projection must not wait behind foreground pool saturation");
    assert!(is_transient_sqlite_write_error(&projection_err));
    assert!(
        projection_started.elapsed() < Duration::from_millis(250),
        "legacy projection exceeded its bulk acquisition budget: {:?}",
        projection_started.elapsed()
    );
    drop((third, second, first));

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}
