use super::*;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::str::FromStr;

async fn single_connection_runtime() -> SqliteRuntime {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::from_str("sqlite::memory:")
                .expect("SQLite options")
                .create_if_missing(true),
        )
        .await
        .expect("single connection pool");
    SqliteRuntime::with_max_connections(pool, 1)
}

async fn three_connection_runtime() -> SqliteRuntime {
    let pool = SqlitePoolOptions::new()
        .min_connections(3)
        .max_connections(3)
        .connect_with(
            SqliteConnectOptions::from_str("sqlite::memory:")
                .expect("SQLite options")
                .create_if_missing(true),
        )
        .await
        .expect("three connection pool");
    SqliteRuntime::with_max_connections(pool, 3)
}

#[test]
fn workload_io_parsers_extract_only_write_bytes() {
    assert_eq!(
        parse_process_write_bytes("rchar: 99\nwrite_bytes: 1234\ncancelled_write_bytes: 5\n"),
        Some(1234)
    );
    assert_eq!(
        parse_cgroup_write_bytes(
            "8:0 rbytes=10 wbytes=20 rios=1 wios=2\n8:16 rbytes=30 wbytes=40 rios=3 wios=4\n"
        ),
        Some(60)
    );
    assert_eq!(parse_cgroup_write_bytes("8:0 rbytes=10 rios=1\n"), None);
}

#[tokio::test]
async fn workload_window_emits_once_then_starts_a_new_bounded_window() {
    let runtime = SqliteRuntime::new(SqlitePool::connect_lazy("sqlite::memory:").unwrap());
    {
        let mut window = runtime.inner.workload.lock().unwrap();
        window.started_at = Instant::now() - Duration::from_secs(61);
    }
    runtime.record_success(
        SqliteOperation::HaEventsRead,
        Duration::from_millis(2),
        Duration::from_millis(3),
        Duration::from_millis(4),
        0,
    );
    runtime.record_success(
        SqliteOperation::HaEventsRead,
        Duration::from_millis(5),
        Duration::from_millis(6),
        Duration::from_millis(7),
        0,
    );

    let window = runtime.inner.workload.lock().unwrap();
    assert!(window.started_at.elapsed() < Duration::from_secs(1));
    assert_eq!(window.operations.len(), 1);
    let metrics = &window.operations[&SqliteOperation::HaEventsRead];
    assert_eq!(metrics.calls, 1);
    assert_eq!(metrics.retries, 0);
    assert_eq!(metrics.pool_wait_ms, 5);
    assert_eq!(metrics.begin_wait_ms, 6);
    assert_eq!(metrics.hold_ms, 7);
    assert_eq!(transaction_hold_p95_ms(&metrics.hold_histogram), 10);
}

#[test]
fn transaction_hold_histogram_reports_the_fixed_p95_bucket() {
    let mut histogram = [0; TRANSACTION_HOLD_BUCKET_UPPER_MS.len()];
    histogram[0] = 94;
    histogram[3] = 5;
    histogram[5] = 1;
    assert_eq!(transaction_hold_p95_ms(&histogram), 100);
}

#[test]
fn production_runtime_transactions_use_the_sqlite_runtime_boundary() {
    fn visit(dir: &std::path::Path, violations: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("read source directory") {
            let path = entry.expect("source entry").path();
            if path.is_dir() {
                visit(&path, violations);
                continue;
            }
            if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
                continue;
            }
            let relative = path
                .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                .expect("repository-relative source path")
                .to_string_lossy()
                .replace('\\', "/");
            let allowed = relative.starts_with("src/tests/")
                || relative.starts_with("src/server/tests/")
                || relative.ends_with("/tests.rs")
                || relative == "src/forward_proxy/tests.rs"
                || relative.starts_with("src/bin/")
                || matches!(
                    relative.as_str(),
                    "src/store/sqlite_runtime.rs"
                        | "src/store/sqlite_runtime_cooperative.rs"
                        | "src/store/immediate_transaction.rs"
                        | "src/store/key_store_bootstrap.rs"
                        | "src/store/key_store_migrations_a.rs"
                        | "src/store/key_store_migrations_b.rs"
                        | "src/store/key_store_admin_passkey_schema.rs"
                        | "src/store/key_store_quota_schema_semantic_migration.rs"
                );
            if allowed {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("read Rust source");
            let compact = source
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>();
            if ["BEGIN", "BEGINIMMEDIATE", "COMMIT", "ROLLBACK"]
                .iter()
                .any(|statement| {
                    compact.contains(&format!("sqlx::query(\"{statement}"))
                        || compact.contains(&format!("sqlx::query(r#\"{statement}"))
                })
            {
                violations.push(relative);
            }
        }
    }

    let mut violations = Vec::new();
    visit(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut violations,
    );
    assert!(
        violations.is_empty(),
        "manual production transactions must use SqliteRuntime:\n{}",
        violations.join("\n")
    );
}

#[tokio::test]
async fn cancelled_read_snapshot_never_returns_an_open_transaction_to_pool() {
    let runtime = single_connection_runtime().await;
    let task_runtime = runtime.clone();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _snapshot = task_runtime
            .begin_read_snapshot(SqliteOperation::HaBaselineRead)
            .await
            .expect("read snapshot");
        ready_tx.send(()).ok();
        std::future::pending::<()>().await;
    });
    ready_rx.await.expect("snapshot started");
    task.abort();
    let _ = task.await;

    let transaction = tokio::time::timeout(
        Duration::from_secs(1),
        runtime.begin_immediate(SqliteOperation::DashboardIntegrityWrite),
    )
    .await
    .expect("pool should replace discarded connection")
    .expect("new immediate transaction");
    transaction.rollback().await.expect("rollback");
}

#[tokio::test]
async fn cancelled_immediate_transaction_rolls_back_without_discarding_connection() {
    let runtime = single_connection_runtime().await;
    let task_runtime = runtime.clone();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _transaction = task_runtime
            .begin_immediate(SqliteOperation::DashboardIntegrityWrite)
            .await
            .expect("immediate transaction");
        ready_tx.send(()).ok();
        std::future::pending::<()>().await;
    });
    ready_rx.await.expect("transaction started");
    task.abort();
    let _ = task.await;

    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(
        runtime.discarded_connections_for_test(SqliteOperation::DashboardIntegrityWrite),
        0,
        "caller cancellation must hand the transaction to the owned rollback task"
    );

    let next = tokio::time::timeout(
        Duration::from_secs(1),
        runtime.begin_immediate(SqliteOperation::DashboardIntegrityWrite),
    )
    .await
    .expect("owned rollback should return the pooled connection")
    .expect("next immediate transaction");
    next.rollback().await.expect("rollback");
}

#[tokio::test]
async fn cancelled_finish_keeps_the_owned_commit_boundary_alive() {
    let runtime = single_connection_runtime().await;
    let pause = install_owned_finish_pause_for_test();
    let task_runtime = runtime.clone();
    let task = tokio::spawn(async move {
        let mut transaction = task_runtime
            .begin_immediate(SqliteOperation::DashboardIntegrityWrite)
            .await
            .expect("immediate transaction");
        sqlx::query("CREATE TABLE owned_finish_probe (id INTEGER PRIMARY KEY)")
            .execute(&mut *transaction)
            .await
            .expect("write inside transaction");
        transaction.finish(Ok(())).await
    });
    pause.arrived.notified().await;
    task.abort();
    let _ = task.await;
    pause.release.notify_one();

    let next = tokio::time::timeout(
        Duration::from_secs(1),
        runtime.begin_immediate(SqliteOperation::DashboardIntegrityWrite),
    )
    .await
    .expect("owned commit should return the pooled connection")
    .expect("next immediate transaction");
    next.rollback().await.expect("rollback");
    assert_eq!(
        runtime.discarded_connections_for_test(SqliteOperation::DashboardIntegrityWrite),
        0,
        "caller cancellation during commit must not detach the connection"
    );
}

#[tokio::test]
async fn explicit_read_close_and_write_error_leave_the_single_connection_clean() {
    let runtime = single_connection_runtime().await;
    runtime
        .begin_read_snapshot(SqliteOperation::HaEventsRead)
        .await
        .expect("read snapshot")
        .close()
        .await
        .expect("close read snapshot");

    let mut transaction = runtime
        .begin_immediate(SqliteOperation::DashboardIntegrityWrite)
        .await
        .expect("immediate transaction");
    let err = transaction
        .finish(Err(ProxyError::Other(
            "synthetic write failure".to_string(),
        )))
        .await
        .expect_err("write failure is preserved");
    assert!(matches!(err, ProxyError::Other(message) if message == "synthetic write failure"));
    drop(transaction);

    let next = tokio::time::timeout(
        Duration::from_secs(1),
        runtime.begin_immediate(SqliteOperation::DashboardIntegrityWrite),
    )
    .await
    .expect("single pooled connection remains usable")
    .expect("next immediate transaction");
    next.rollback().await.expect("rollback");
}

#[tokio::test]
async fn successful_short_write_restores_busy_timeout_before_pool_return() {
    let runtime = single_connection_runtime().await;
    let mut transaction = runtime
        .begin_immediate(SqliteOperation::DashboardIntegrityWrite)
        .await
        .expect("immediate transaction");
    sqlx::query("CREATE TABLE runtime_guard_probe (id INTEGER PRIMARY KEY)")
        .execute(&mut *transaction)
        .await
        .expect("write inside transaction");
    transaction
        .finish(Ok(()))
        .await
        .expect("commit transaction");
    drop(transaction);

    let mut conn = runtime
        .inner
        .pool
        .acquire()
        .await
        .expect("pooled connection");
    let busy_timeout_ms: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
        .fetch_one(&mut *conn)
        .await
        .expect("read restored busy timeout");
    assert_eq!(busy_timeout_ms, DEFAULT_BUSY_TIMEOUT_MS);
}

#[tokio::test]
async fn failed_short_write_restores_busy_timeout_before_pool_return() {
    let runtime = single_connection_runtime().await;
    let mut transaction = runtime
        .begin_immediate(SqliteOperation::DashboardIntegrityWrite)
        .await
        .expect("immediate transaction");
    let err = transaction
        .finish(Err(ProxyError::Other(
            "synthetic write failure".to_string(),
        )))
        .await
        .expect_err("synthetic write failure remains visible");
    assert!(matches!(err, ProxyError::Other(message) if message == "synthetic write failure"));

    let mut conn = runtime
        .inner
        .pool
        .acquire()
        .await
        .expect("pooled connection after rollback");
    let busy_timeout_ms: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
        .fetch_one(&mut *conn)
        .await
        .expect("read restored busy timeout");
    assert_eq!(busy_timeout_ms, DEFAULT_BUSY_TIMEOUT_MS);
}

#[tokio::test]
async fn reconciliation_projection_uses_and_restores_short_busy_timeout() {
    let runtime = single_connection_runtime().await;
    let mut transaction = runtime
        .begin_immediate(SqliteOperation::ReconciliationProjection)
        .await
        .expect("projection transaction");
    let busy_timeout_ms: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
        .fetch_one(&mut *transaction)
        .await
        .expect("read projection busy timeout");
    assert_eq!(busy_timeout_ms, 100);
    transaction.rollback().await.expect("rollback projection");

    let mut conn = runtime
        .inner
        .pool
        .acquire()
        .await
        .expect("pooled connection after projection");
    let busy_timeout_ms: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
        .fetch_one(&mut *conn)
        .await
        .expect("read restored busy timeout");
    assert_eq!(busy_timeout_ms, DEFAULT_BUSY_TIMEOUT_MS);
}

#[tokio::test]
async fn scheduled_job_control_begin_respects_a_short_deadline() {
    let runtime = single_connection_runtime().await;
    let held = runtime
        .inner
        .pool
        .acquire()
        .await
        .expect("hold the only connection");
    let started = Instant::now();
    let error = runtime
        .begin_scheduled_job_control()
        .await
        .expect_err("scheduled-job control must yield under pool pressure");
    assert!(is_transient_sqlite_write_error(&error));
    assert!(started.elapsed() < Duration::from_millis(250));
    drop(held);
}

#[tokio::test]
async fn alert_projection_read_snapshot_uses_and_restores_short_busy_timeout() {
    let runtime = single_connection_runtime().await;
    let mut snapshot = runtime
        .begin_read_snapshot(SqliteOperation::AlertProjection)
        .await
        .expect("alert projection read snapshot");
    let busy_timeout_ms: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
        .fetch_one(&mut *snapshot)
        .await
        .expect("read alert projection busy timeout");
    assert_eq!(busy_timeout_ms, 100);
    snapshot.close().await.expect("close read snapshot");

    let mut conn = runtime
        .inner
        .pool
        .acquire()
        .await
        .expect("pooled connection after alert projection snapshot");
    let busy_timeout_ms: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
        .fetch_one(&mut *conn)
        .await
        .expect("read restored busy timeout");
    assert_eq!(busy_timeout_ms, DEFAULT_BUSY_TIMEOUT_MS);
}

#[tokio::test]
async fn admitted_maintenance_work_keeps_the_configured_busy_timeout() {
    let runtime = single_connection_runtime().await;
    for operation in [
        SqliteOperation::ScheduledJobControl,
        SqliteOperation::RequestStatsFlush,
        SqliteOperation::HaOutboxGc,
        SqliteOperation::RequestLogsGc,
    ] {
        runtime
            .begin_immediate(operation)
            .await
            .expect("admitted maintenance transaction")
            .rollback()
            .await
            .expect("rollback maintenance transaction");

        let mut conn = runtime
            .inner
            .pool
            .acquire()
            .await
            .expect("pooled connection");
        let busy_timeout_ms: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
            .fetch_one(&mut *conn)
            .await
            .expect("read configured busy timeout");
        assert_eq!(busy_timeout_ms, DEFAULT_BUSY_TIMEOUT_MS, "{operation}");
    }
}

#[tokio::test]
async fn sqlite_runtime_foreground_preempts_bulk_work() {
    let runtime = three_connection_runtime().await;
    let first = runtime
        .try_admit_maintenance_bulk(SqliteOperation::HaOutboxGc)
        .expect("first bulk workload is admitted");
    let second = runtime
        .try_admit_maintenance_bulk(SqliteOperation::HaOutboxGc)
        .expect_err("second bulk workload must preserve foreground capacity");
    assert_eq!(second, SqliteAdmissionDeferReason::BulkBusy);

    let foreground = tokio::time::timeout(Duration::from_millis(250), runtime.inner.pool.acquire())
        .await
        .expect("foreground request must not wait behind deferred bulk work")
        .expect("foreground pool acquisition");
    drop(foreground);
    drop(first);
}

#[tokio::test]
async fn admin_privacy_read_session_is_bounded_and_independent_of_bulk_admission() {
    let runtime = three_connection_runtime().await;
    let bulk = runtime
        .try_admit_maintenance_bulk(SqliteOperation::HaOutboxGc)
        .expect("hold unrelated bulk admission");
    let session = runtime
        .begin_read_snapshot(SqliteOperation::AdminPrivacyRead)
        .await
        .expect("privacy read does not require the bulk permit");
    session.close().await.expect("close privacy session");
    drop(bulk);

    let first = runtime
        .inner
        .pool
        .acquire()
        .await
        .expect("hold first connection");
    let second = runtime
        .inner
        .pool
        .acquire()
        .await
        .expect("hold second connection");
    let third = runtime
        .inner
        .pool
        .acquire()
        .await
        .expect("hold third connection");
    let started = Instant::now();
    let error = runtime
        .begin_read_snapshot(SqliteOperation::AdminPrivacyRead)
        .await
        .expect_err("pool exhaustion must reject the cold privacy read");
    assert!(matches!(
        error,
        ProxyError::Database(sqlx::Error::PoolTimedOut)
    ));
    assert!(started.elapsed() < Duration::from_millis(150));
    drop((third, second, first));
}

#[tokio::test]
async fn admin_privacy_read_run_budget_interrupts_before_discarding_its_session() {
    let runtime = single_connection_runtime().await;
    let mut session = runtime
        .begin_read_snapshot(SqliteOperation::AdminPrivacyRead)
        .await
        .expect("privacy read session");
    assert_eq!(
        session.cooperative_run_budget_for_test(),
        Some(ADMIN_PRIVACY_READ_RUN_BUDGET),
        "admin privacy snapshots install the production two-second run budget"
    );
    session
        .arm_cooperative_run_budget(Duration::ZERO)
        .await
        .expect("install immediate read budget");
    let query_error = sqlx::query_scalar::<_, i64>(
            "WITH RECURSIVE counter(value) AS (VALUES(1) UNION ALL SELECT value + 1 FROM counter WHERE value < 1000000) SELECT SUM(value) FROM counter",
        )
        .fetch_one(&mut *session)
        .await
        .expect_err("the SQLite progress handler must interrupt an expired read budget");
    let query_error = ProxyError::Database(query_error);
    session
        .close_after_query(Some(&query_error))
        .await
        .expect("interrupted session closes explicitly");
    assert_eq!(
        runtime.discarded_connections_for_test(SqliteOperation::AdminPrivacyRead),
        0,
        "cooperative interruption must not discard the SQLite connection"
    );
    assert_eq!(
        runtime.operation_errors_for_test(SqliteOperation::AdminPrivacyRead),
        1,
        "an interrupted query must enter runtime workload error metrics"
    );
    runtime
        .begin_read_snapshot(SqliteOperation::AdminPrivacyRead)
        .await
        .expect("next privacy session is clean")
        .close()
        .await
        .expect("close next privacy session");
}

#[tokio::test]
async fn reconciliation_read_sessions_interrupt_and_clean_each_read_kind() {
    let runtime = single_connection_runtime().await;
    for kind in ReconciliationReadKind::ALL {
        runtime.force_next_cooperative_query_deadline_for_test();
        let mut session = runtime
            .begin_reconciliation_read(kind)
            .await
            .expect("begin bounded reconciliation read");
        let query_result = sqlx::query_scalar::<_, i64>(
                "WITH RECURSIVE counter(value) AS (VALUES(1) UNION ALL SELECT value + 1 FROM counter WHERE value < 1000000) SELECT SUM(value) FROM counter",
            )
            .fetch_one(&mut *session)
            .await;
        assert!(matches!(
            session
                .complete_query(query_result)
                .await
                .expect("complete interrupted reconciliation read"),
            SqliteCooperativeQueryOutcome::DeadlineExceeded
        ));
    }

    assert_eq!(
        runtime.discarded_connections_for_test(SqliteOperation::ReconciliationProjection),
        0,
        "a cleaned deadline session remains reusable"
    );
    let mut completed_after_deadline = runtime
        .begin_reconciliation_read(ReconciliationReadKind::CandidateRecent)
        .await
        .expect("begin reconciliation read that completes at the boundary");
    let completed_result = sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&mut *completed_after_deadline)
        .await;
    completed_after_deadline.expire_deadline_after_query_for_test();
    assert!(matches!(
        completed_after_deadline
            .complete_query(completed_result)
            .await
            .expect("a result beyond the deadline becomes a typed defer"),
        SqliteCooperativeQueryOutcome::DeadlineExceeded
    ));
    let mut normal_session = runtime
        .begin_reconciliation_read(ReconciliationReadKind::CandidateRecent)
        .await
        .expect("next reconciliation session is clean");
    let normal_result = sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&mut *normal_session)
        .await;
    assert_eq!(
        normal_session
            .complete_query_or_defer(normal_result)
            .await
            .expect("normal session completes"),
        1
    );
}

#[tokio::test]
async fn sqlite_runtime_admits_bulk_from_a_lazy_three_connection_pool() {
    let runtime = SqliteRuntime::with_max_connections(
        SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(3)
            .connect_with(
                SqliteConnectOptions::from_str("sqlite::memory:")
                    .expect("SQLite options")
                    .create_if_missing(true),
            )
            .await
            .expect("lazy three connection pool"),
        3,
    );
    assert_eq!(runtime.inner.pool.num_idle(), 1);

    let bulk = runtime
        .try_admit_maintenance_bulk(SqliteOperation::HaOutboxGc)
        .expect("unopened capacity must satisfy the two-slot foreground reservation");
    let first_foreground =
        tokio::time::timeout(Duration::from_millis(250), runtime.inner.pool.acquire())
            .await
            .expect("first foreground acquisition stays bounded")
            .expect("first foreground connection");
    let second_foreground =
        tokio::time::timeout(Duration::from_millis(250), runtime.inner.pool.acquire())
            .await
            .expect("second foreground acquisition stays bounded")
            .expect("second foreground connection");

    drop((second_foreground, first_foreground, bulk));
}

#[tokio::test]
async fn reconciliation_projection_can_probe_a_partially_open_idle_pool() {
    let runtime = SqliteRuntime::with_max_connections(
        SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(3)
            .connect_with(
                SqliteConnectOptions::from_str("sqlite::memory:")
                    .expect("SQLite options")
                    .create_if_missing(true),
            )
            .await
            .expect("lazy three connection pool"),
        3,
    );
    let foreground = runtime.inner.pool.acquire().await.expect("foreground");
    let second = runtime.inner.pool.acquire().await.expect("grow pool");
    drop(second);
    tokio::time::timeout(Duration::from_secs(1), async {
        while runtime.inner.pool.num_idle() < 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("returned connection becomes idle");
    assert_eq!(runtime.inner.pool.size(), 2);
    assert_eq!(runtime.inner.pool.num_idle(), 1);

    assert_eq!(
        runtime
            .try_admit_maintenance_bulk(SqliteOperation::HaOutboxGc)
            .expect_err("ordinary bulk work still reserves two foreground slots"),
        SqliteAdmissionDeferReason::PoolPressure
    );
    runtime.prewarm_reconciliation_projection_capacity().await;
    assert_eq!(runtime.inner.pool.size(), 3);
    assert_eq!(runtime.inner.pool.num_idle(), 2);
    runtime.mark_recent_contention_for_test();
    let projection = runtime
        .try_admit_maintenance_bulk(SqliteOperation::ReconciliationProjection)
        .expect("a bounded projection slice may probe prewarmed capacity");
    let projection_tx = runtime
        .begin_immediate(SqliteOperation::ReconciliationProjection)
        .await
        .expect("bounded projection transaction");
    let second_foreground =
        tokio::time::timeout(Duration::from_millis(250), runtime.inner.pool.acquire())
            .await
            .expect("foreground can open the final reserved connection")
            .expect("foreground connection");

    drop(second_foreground);
    projection_tx.rollback().await.expect("rollback projection");
    drop((projection, foreground));
}

#[tokio::test]
async fn reconciliation_projection_prewarm_does_not_block_foreground_capacity() {
    let runtime = SqliteRuntime::with_max_connections(
        SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(3)
            .connect_with(
                SqliteConnectOptions::from_str("sqlite::memory:")
                    .expect("SQLite options")
                    .create_if_missing(true),
            )
            .await
            .expect("lazy three connection pool"),
        3,
    );
    let existing_foreground = runtime.inner.pool.acquire().await.expect("foreground");
    let grow = runtime.inner.pool.acquire().await.expect("grow pool");
    drop(grow);

    let prewarm_runtime = runtime.clone();
    let foreground_pool = runtime.inner.pool.clone();
    let started = Instant::now();
    let ((), foreground) = tokio::join!(
        prewarm_runtime.prewarm_reconciliation_projection_capacity(),
        async {
            tokio::time::timeout(Duration::from_millis(250), foreground_pool.acquire())
                .await
                .expect("foreground wait remains bounded during prewarm")
                .expect("foreground connection")
        }
    );
    assert!(started.elapsed() < Duration::from_millis(250));

    drop((foreground, existing_foreground));
}

#[tokio::test]
async fn sqlite_runtime_never_admits_bulk_from_a_single_connection_pool() {
    let runtime = single_connection_runtime().await;
    assert_eq!(
        runtime
            .try_admit_maintenance_bulk(SqliteOperation::HaOutboxGc)
            .expect_err("a one-connection pool cannot reserve two foreground slots"),
        SqliteAdmissionDeferReason::PoolPressure
    );
}

#[tokio::test]
async fn sqlite_runtime_rate_limits_process_wide_heap_trims() {
    let runtime = three_connection_runtime().await;
    assert!(runtime.bulk_heap_trim_due());
    assert!(
        !runtime.bulk_heap_trim_due(),
        "adjacent recovery slices must not repeatedly take the allocator lock"
    );
}

#[tokio::test]
async fn sqlite_runtime_defers_bulk_before_foreground_pool_capacity_is_exhausted() {
    let runtime = three_connection_runtime().await;
    let first_foreground = runtime
        .inner
        .pool
        .acquire()
        .await
        .expect("first foreground");
    let second_foreground = runtime
        .inner
        .pool
        .acquire()
        .await
        .expect("second foreground");

    assert_eq!(
        runtime
            .try_admit_maintenance_bulk(SqliteOperation::HaOutboxGc)
            .expect_err("bulk work must defer before taking the final foreground slot"),
        SqliteAdmissionDeferReason::PoolPressure
    );

    drop(second_foreground);
    drop(first_foreground);
    tokio::time::timeout(Duration::from_secs(1), async {
        while runtime.inner.pool.num_idle() < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("foreground connections return to the pool");
    let bulk = runtime
        .try_admit_maintenance_bulk(SqliteOperation::HaOutboxGc)
        .expect("bulk work resumes after foreground capacity returns");
    drop(bulk);
}

#[tokio::test]
async fn dashboard_read_admission_allows_a_lazy_idle_pool() {
    let runtime = SqliteRuntime::new(
        SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(3)
            .connect_with(
                SqliteConnectOptions::from_str("sqlite::memory:")
                    .expect("SQLite options")
                    .create_if_missing(true),
            )
            .await
            .expect("lazy foreground pool"),
    );
    assert_eq!(runtime.inner.pool.num_idle(), 1);
    assert_eq!(
        runtime.dashboard_read_defer_reason(),
        None,
        "a foreground dashboard read must not require bulk's two-idle reservation",
    );
}

#[tokio::test]
async fn maintenance_control_bypasses_bulk_without_retry_loop() {
    let runtime = three_connection_runtime().await;
    let bulk = runtime
        .try_admit_maintenance_bulk(SqliteOperation::HaOutboxGc)
        .expect("bulk permit");

    let transaction = tokio::time::timeout(
        Duration::from_millis(100),
        runtime.begin_scheduled_job_control(),
    )
    .await
    .expect("control transaction must not wait for the bulk permit")
    .expect("control transaction");
    transaction
        .rollback()
        .await
        .expect("rollback control transaction");
    drop(bulk);
}

#[tokio::test]
async fn maintenance_shutdown_waits_for_the_active_slice_and_blocks_new_bulk() {
    let runtime = three_connection_runtime().await;
    let bulk = runtime
        .try_admit_maintenance_bulk(SqliteOperation::ReconciliationProjection)
        .expect("active projection slice");
    let shutdown_runtime = runtime.clone();
    let shutdown = tokio::spawn(async move {
        shutdown_runtime
            .shutdown_maintenance_bulk(Duration::from_secs(1))
            .await
    });

    tokio::task::yield_now().await;
    assert!(!shutdown.is_finished());
    assert_eq!(
        runtime
            .try_admit_maintenance_bulk(SqliteOperation::HaOutboxGc)
            .expect_err("shutdown must reject new maintenance slices"),
        SqliteAdmissionDeferReason::BulkBusy,
    );

    drop(bulk);
    assert!(shutdown.await.expect("maintenance shutdown task"));
}

#[tokio::test]
async fn maintenance_shutdown_waits_for_an_active_run_without_reserving_bulk() {
    let runtime = three_connection_runtime().await;
    let run = runtime
        .try_start_maintenance_run()
        .expect("active reconciliation run");
    let bulk = runtime
        .try_admit_maintenance_bulk(SqliteOperation::HaOutboxGc)
        .expect("a run lease must not reserve the bulk permit");
    drop(bulk);

    runtime.begin_maintenance_run_shutdown();
    assert!(runtime.try_start_maintenance_run().is_none());
    let shutdown_runtime = runtime.clone();
    let shutdown = tokio::spawn(async move {
        shutdown_runtime
            .shutdown_maintenance_bulk(Duration::from_secs(1))
            .await
    });
    tokio::task::yield_now().await;
    assert!(!shutdown.is_finished());

    drop(run);
    assert!(shutdown.await.expect("maintenance shutdown task"));
}

#[tokio::test]
async fn maintenance_control_pool_timeout_is_a_typed_defer() {
    let runtime = single_connection_runtime().await;
    let held = runtime
        .inner
        .pool
        .acquire()
        .await
        .expect("hold the only connection");

    let err = runtime
        .acquire_operation_connection(SqliteOperation::ScheduledJobControl)
        .await
        .expect_err("control connection must time out within its budget");
    assert!(
        is_transient_sqlite_write_error(&err),
        "pool acquisition must remain a typed transient error, got {err}",
    );
    let window = runtime.inner.workload.lock().unwrap();
    let metrics = &window.operations[&SqliteOperation::ScheduledJobControl];
    assert_eq!(
        metrics.errors, 0,
        "control contention is a defer, not an error"
    );
    assert_eq!(metrics.deferred, 1);
    assert_eq!(
        metrics
            .deferred_by_reason
            .get(&SqliteAdmissionDeferReason::RecentContention),
        Some(&1)
    );
    drop(held);
}

#[tokio::test]
async fn request_stats_begin_respects_the_slice_deadline() {
    let runtime = single_connection_runtime().await;
    let held = runtime
        .inner
        .pool
        .acquire()
        .await
        .expect("hold the only connection");
    let started = Instant::now();
    let err = runtime
        .begin_immediate_before(
            SqliteOperation::RequestStatsFlush,
            Instant::now() + Duration::from_millis(50),
        )
        .await
        .expect_err("flush must yield when its caller deadline expires");
    assert!(is_transient_sqlite_write_error(&err));
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "caller deadline must bound pool acquisition and BEGIN"
    );
    drop(held);
}
