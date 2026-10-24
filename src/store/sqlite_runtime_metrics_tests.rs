use super::*;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::str::FromStr;
use std::sync::atomic::Ordering;

impl SqliteRuntime {
    #[cfg(test)]
    pub(crate) fn deferred_count_for_test(
        &self,
        operation: SqliteOperation,
        reason: SqliteAdmissionDeferReason,
    ) -> u64 {
        self.inner
            .workload
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .operations
            .get(&operation)
            .and_then(|metrics| metrics.deferred_by_reason.get(&reason))
            .copied()
            .unwrap_or_default()
    }
}

async fn file_runtime(max_connections: u32) -> (tempfile::TempDir, SqliteRuntime) {
    let directory = tempfile::tempdir().expect("create SQLite lock fixture directory");
    let database_path = directory.path().join("runtime-boundary.db");
    let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", database_path.display()))
        .expect("SQLite options")
        .create_if_missing(true)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(max_connections)
        .connect_with(options)
        .await
        .expect("open SQLite runtime boundary pool");
    sqlx::query("CREATE TABLE lock_probe (id INTEGER PRIMARY KEY, value INTEGER)")
        .execute(&pool)
        .await
        .expect("create SQLite lock fixture");
    sqlx::query("INSERT INTO lock_probe (id, value) VALUES (1, 1)")
        .execute(&pool)
        .await
        .expect("seed SQLite lock fixture");
    (
        directory,
        SqliteRuntime::with_max_connections(pool, max_connections),
    )
}

fn assert_typed_defer(
    runtime: &SqliteRuntime,
    operation: SqliteOperation,
    reason: SqliteAdmissionDeferReason,
    label: &str,
) {
    let window = runtime.inner.workload.lock().unwrap();
    let metrics = &window.operations[&operation];
    assert_eq!(metrics.errors, 0, "{label} must remain a typed defer");
    assert_eq!(metrics.deferred, 1, "{label} defer count");
    assert_eq!(
        metrics.deferred_by_reason.get(&reason),
        Some(&1),
        "{label} defer reason",
    );
    assert!(
        runtime
            .inner
            .last_contention_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some(),
        "{label} must preserve writer-contention state",
    );
    assert!(
        runtime
            .inner
            .contention_warning_active
            .load(Ordering::Acquire),
        "{label} must preserve the contention warning state",
    );
}

#[tokio::test]
async fn sqlite_runtime_contention_classification_respects_acquire_boundary() {
    {
        let (_directory, runtime) = file_runtime(1).await;
        let held = runtime
            .acquire_operation_connection(SqliteOperation::ScheduledJobControl)
            .await
            .expect("hold the only runtime pool connection");
        let error = runtime
            .acquire_operation_connection(SqliteOperation::ScheduledJobControl)
            .await
            .expect_err("runtime acquire must time out on an exhausted pool");
        assert!(
            matches!(error, ProxyError::Database(sqlx::Error::PoolTimedOut)),
            "pool exhaustion must remain a typed PoolTimedOut, got {error}"
        );
        assert_typed_defer(
            &runtime,
            SqliteOperation::ScheduledJobControl,
            SqliteAdmissionDeferReason::PoolPressure,
            "pool acquire timeout",
        );
        drop(held);
    }

    {
        let (_directory, runtime) = file_runtime(2).await;
        let mut holder = runtime
            .acquire_operation_connection(SqliteOperation::ScheduledJobControl)
            .await
            .expect("acquire the BEGIN timeout lock holder");
        let holder_transaction = holder
            .begin_immediate()
            .await
            .expect("hold a real BEGIN IMMEDIATE transaction");
        let error = runtime
            .begin_immediate(SqliteOperation::ScheduledJobControl)
            .await
            .expect_err("runtime BEGIN must time out behind the held writer");
        assert!(
            matches!(error, ProxyError::Database(sqlx::Error::PoolTimedOut)),
            "BEGIN timeout must remain a typed PoolTimedOut, got {error}"
        );
        assert_typed_defer(
            &runtime,
            SqliteOperation::ScheduledJobControl,
            SqliteAdmissionDeferReason::RecentContention,
            "BEGIN timeout",
        );
        holder_transaction
            .rollback()
            .await
            .expect("release the BEGIN timeout lock holder");
    }

    {
        let (_directory, runtime) = file_runtime(2).await;
        let mut holder = runtime
            .acquire_operation_connection(SqliteOperation::ScheduledJobControl)
            .await
            .expect("acquire the busy-query lock holder");
        let holder_transaction = holder
            .begin_immediate()
            .await
            .expect("hold a real SQLite write lock");
        let mut contender = runtime
            .acquire_operation_connection(SqliteOperation::DashboardIntegrityWrite)
            .await
            .expect("acquire the runtime busy-query contender");
        let query_result = sqlx::query("UPDATE lock_probe SET value = 2 WHERE id = 1")
            .execute(&mut *contender)
            .await;
        let error = contender
            .complete_query(query_result)
            .await
            .expect_err("runtime query must observe the held SQLite writer");
        assert!(
            is_transient_sqlite_write_error(&error),
            "SQLite busy/locked must remain transient, got {error}"
        );
        assert_typed_defer(
            &runtime,
            SqliteOperation::DashboardIntegrityWrite,
            SqliteAdmissionDeferReason::RecentContention,
            "SQLite busy/locked",
        );
        holder_transaction
            .rollback()
            .await
            .expect("release the busy-query lock holder");
    }
}

#[tokio::test]
async fn workload_window_records_typed_admission_defers_without_statement_text() {
    let runtime = SqliteRuntime::new(SqlitePool::connect_lazy("sqlite::memory:").unwrap());
    runtime.record_deferred(
        SqliteOperation::RequestStatsFlush,
        SqliteAdmissionDeferReason::PoolPressure,
    );

    let window = runtime.inner.workload.lock().unwrap();
    let metrics = &window.operations[&SqliteOperation::RequestStatsFlush];
    assert_eq!(metrics.calls, 1);
    assert_eq!(metrics.deferred, 1);
    assert_eq!(
        metrics
            .deferred_by_reason
            .get(&SqliteAdmissionDeferReason::PoolPressure),
        Some(&1)
    );
    let formatted = format_operation_window(&window.operations, &window.reconciliation_reads);
    assert!(formatted.contains("maintenance_bulk/request_stats_flush"));
    assert!(formatted.contains("defer_reasons=pool_pressure=1"));
    assert!(!formatted.contains("SELECT"));
    assert!(!formatted.contains("INSERT"));
}

#[tokio::test]
async fn discarded_transaction_connection_is_recorded_as_error() {
    let runtime = SqliteRuntime::new(SqlitePool::connect_lazy("sqlite::memory:").unwrap());
    runtime.record_discard(
        SqliteOperation::RequestStatsFlush,
        Duration::from_millis(2),
        Duration::from_millis(3),
        Duration::from_millis(4),
    );

    let window = runtime.inner.workload.lock().unwrap();
    let metrics = &window.operations[&SqliteOperation::RequestStatsFlush];
    assert_eq!(metrics.errors, 1);
    assert_eq!(metrics.discarded_connections, 1);
}

#[tokio::test]
async fn deferred_read_close_is_not_reported_as_a_database_error() {
    let runtime = SqliteRuntime::new(SqlitePool::connect_lazy("sqlite::memory:").unwrap());
    let error = ProxyError::Deferred {
        operation: "admin_alerts_read",
        reason: "read_budget".to_string(),
    };
    runtime.record_deferred_error(SqliteOperation::AdminAlertsCacheWarm, &error);

    let window = runtime.inner.workload.lock().unwrap();
    let metrics = &window.operations[&SqliteOperation::AdminAlertsCacheWarm];
    assert_eq!(metrics.errors, 0);
    assert_eq!(metrics.deferred, 1);
    assert_eq!(
        metrics
            .deferred_by_reason
            .get(&SqliteAdmissionDeferReason::QueryDeadline),
        Some(&1)
    );
}

#[tokio::test]
async fn workload_window_keeps_dashboard_quota_read_defers_out_of_alerts_warm_metrics() {
    let runtime = SqliteRuntime::new(SqlitePool::connect_lazy("sqlite::memory:").unwrap());
    let error = ProxyError::Deferred {
        operation: "dashboard_quota_read",
        reason: "read_budget".to_string(),
    };

    runtime.record_deferred_error(SqliteOperation::DashboardQuotaRead, &error);

    let window = runtime.inner.workload.lock().unwrap();
    let quota_metrics = &window.operations[&SqliteOperation::DashboardQuotaRead];
    assert_eq!(quota_metrics.deferred, 1);
    assert_eq!(
        quota_metrics
            .deferred_by_reason
            .get(&SqliteAdmissionDeferReason::QueryDeadline),
        Some(&1)
    );
    assert!(
        !window
            .operations
            .contains_key(&SqliteOperation::AdminAlertsCacheWarm),
        "Dashboard quota timeouts must not be attributed to canonical Alerts warming"
    );
}

#[test]
fn deferred_reason_observability_keeps_blocking_causes_distinct() {
    assert_eq!(
        SqliteAdmissionDeferReason::CoverageReadiness.observability_as_str(),
        "coverage_readiness"
    );
    assert_eq!(
        SqliteAdmissionDeferReason::SourceBaseline.observability_as_str(),
        "source_baseline"
    );
    assert_eq!(
        SqliteAdmissionDeferReason::QueryDeadline.observability_as_str(),
        "native_query_deadline"
    );
    assert_eq!(
        SqliteAdmissionDeferReason::PoolPressure.observability_as_str(),
        "pool_pressure"
    );
    assert_eq!(
        SqliteAdmissionDeferReason::RecentContention.observability_as_str(),
        "writer_contention"
    );
}

#[tokio::test]
async fn deferred_error_reason_mapping_does_not_mask_readiness_or_source_baseline() {
    for (reason, expected) in [
        (
            "groups_build_in_progress",
            SqliteAdmissionDeferReason::CoverageReadiness,
        ),
        (
            "deleted_source_baseline_unavailable",
            SqliteAdmissionDeferReason::SourceBaseline,
        ),
        ("read_budget", SqliteAdmissionDeferReason::QueryDeadline),
        (
            "native_query_deadline",
            SqliteAdmissionDeferReason::QueryDeadline,
        ),
        (
            "sqlite_contention",
            SqliteAdmissionDeferReason::RecentContention,
        ),
        (
            "writer_contention",
            SqliteAdmissionDeferReason::RecentContention,
        ),
    ] {
        let runtime = SqliteRuntime::new(SqlitePool::connect_lazy("sqlite::memory:").unwrap());
        let error = ProxyError::Deferred {
            operation: "test_operation",
            reason: reason.to_owned(),
        };
        runtime.record_deferred_error(SqliteOperation::RequestStatsFlush, &error);
        let window = runtime.inner.workload.lock().unwrap();
        assert_eq!(
            window.operations[&SqliteOperation::RequestStatsFlush]
                .deferred_by_reason
                .get(&expected),
            Some(&1)
        );
        drop(window);
    }
}

#[tokio::test]
async fn workload_window_reports_canonical_alerts_warm_events_without_sensitive_fields() {
    let runtime = SqliteRuntime::new(SqlitePool::connect_lazy("sqlite::memory:").unwrap());
    runtime.record_admin_alerts_warm_slice();
    runtime.record_admin_alerts_warm_publish();
    runtime.record_admin_alerts_warm_generation_discard();
    runtime.record_admin_alerts_warm_defer();
    runtime.record_admin_alerts_warm_cold_miss();
    runtime.record_admin_alerts_canonical_group_build_slice();
    runtime.record_admin_alerts_canonical_group_publish();
    runtime.record_admin_alerts_canonical_group_defer();

    let window = runtime
        .inner
        .workload
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let formatted = format_admin_alerts_warm_window(window.admin_alerts_warm);
    assert_eq!(
        formatted,
        "slices=1,publishes=1,generation_discards=1,defers=1,cold_misses=1,canonical_events_indexed_reads=0,canonical_catalog_payload_slices=0,canonical_group_build_slices=1,canonical_group_reduction_slices=0,canonical_group_publishes=1,canonical_group_defers=1"
    );
    assert!(!formatted.contains("SELECT"));
    assert!(!formatted.contains("token"));
}

#[tokio::test]
async fn sqlite_workload_window_reports_scoped_reconciliation_metrics() {
    let runtime = SqliteRuntime::new(SqlitePool::connect_lazy("sqlite::memory:").unwrap());
    runtime.record_connection_cache_write_pages(SqliteOperation::ReconciliationProjection, Some(7));
    runtime.record_cooperative_read(
        SqliteOperation::ReconciliationProjection,
        Duration::from_millis(42),
        true,
    );
    runtime.record_reconciliation_key_observation_identity(2, 1);

    let telemetry = runtime.operation_telemetry(SqliteOperation::ReconciliationProjection);
    assert_eq!(telemetry.connection_cache_write_pages, 7);
    assert_eq!(telemetry.cooperative_read_elapsed_ms, 42);
    assert_eq!(telemetry.cooperative_read_deadlines, 1);

    let window = runtime.inner.workload.lock().unwrap();
    let formatted = format_operation_window(&window.operations, &window.reconciliation_reads);
    assert!(formatted.contains("connection_cache_write_pages=7"));
    assert!(formatted.contains("cooperative_read_elapsed_ms=42"));
    assert!(formatted.contains("cooperative_read_deadlines=1"));
    assert!(formatted.contains("key_observation_reuses=2"));
    assert!(formatted.contains("key_observation_identity_misses=1"));
    assert!(!formatted.contains("SELECT"));
    assert!(!formatted.contains("token_"));
}

#[tokio::test]
async fn cache_write_sampling_failure_is_reported_as_unknown() {
    let runtime = SqliteRuntime::new(SqlitePool::connect_lazy("sqlite::memory:").unwrap());
    runtime.record_connection_cache_write_pages(SqliteOperation::ReconciliationProjection, Some(7));
    runtime.record_connection_cache_write_pages(SqliteOperation::ReconciliationProjection, None);

    let telemetry = runtime.operation_telemetry(SqliteOperation::ReconciliationProjection);
    assert!(telemetry.connection_cache_write_sampled);
    assert!(telemetry.connection_cache_write_sample_failed);

    let window = runtime.inner.workload.lock().unwrap();
    assert!(
        format_operation_window(&window.operations, &window.reconciliation_reads)
            .contains("connection_cache_write_pages=unknown")
    );
}

#[tokio::test]
async fn operation_telemetry_survives_workload_window_rotation() {
    let runtime = SqliteRuntime::new(SqlitePool::connect_lazy("sqlite::memory:").unwrap());
    runtime.record_connection_cache_write_pages(SqliteOperation::ReconciliationProjection, Some(7));
    runtime.record_cooperative_read(
        SqliteOperation::ReconciliationProjection,
        Duration::from_millis(42),
        true,
    );

    {
        let mut window = runtime
            .inner
            .workload
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *window = WorkloadWindow::default();
    }

    let telemetry = runtime.operation_telemetry(SqliteOperation::ReconciliationProjection);
    assert_eq!(telemetry.connection_cache_write_pages, 7);
    assert_eq!(telemetry.cooperative_read_elapsed_ms, 42);
    assert_eq!(telemetry.cooperative_read_deadlines, 1);
}

#[tokio::test]
async fn workload_window_reports_reconciliation_read_kinds_without_statement_text() {
    let runtime = SqliteRuntime::new(SqlitePool::connect_lazy("sqlite::memory:").unwrap());
    for kind in ReconciliationReadKind::ALL {
        let deadline = kind == ReconciliationReadKind::ResearchCandidates;
        runtime.record_reconciliation_read(
            kind,
            Duration::from_millis(42),
            deadline,
            deadline || kind == ReconciliationReadKind::CandidateRecent,
            false,
            Some(3),
        );
    }

    let window = runtime
        .inner
        .workload
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let formatted = format_operation_window(&window.operations, &window.reconciliation_reads);
    for kind in ReconciliationReadKind::ALL {
        assert!(formatted.contains(&format!("reconciliation_read/{}:calls=1", kind.as_str())));
    }
    assert!(formatted.contains(
        "reconciliation_read/candidate_recent:calls=1,elapsed_ms=42,deadlines=0,deferred=1"
    ));
    assert!(formatted.contains(
        "reconciliation_read/research_candidates:calls=1,elapsed_ms=42,deadlines=1,deferred=1"
    ));
    assert!(!formatted.contains("SELECT"));
}

#[test]
fn sqlite_file_state_sampling_reads_only_configured_database_paths() {
    let directory = tempfile::tempdir().expect("create database directory");
    let core = directory.path().join("core.db");
    let observability = directory.path().join("observability.db");
    std::fs::write(&core, [0_u8; 3]).expect("write core database fixture");
    std::fs::write(format!("{}-wal", core.display()), [0_u8; 5]).expect("write core WAL fixture");
    std::fs::write(&observability, [0_u8; 7]).expect("write observability fixture");

    let formatted = format_sqlite_file_state(&SqliteFileStatePaths {
        core: Some(core),
        observability: Some(observability),
    });
    assert!(formatted.contains("core_db_bytes=3"));
    assert!(formatted.contains("core_wal_bytes=5"));
    assert!(formatted.contains("observability_db_bytes=7"));
    assert!(formatted.contains("observability_wal_bytes=unknown"));
}

#[test]
fn maintenance_metrics_keep_fixed_classes_and_reset_only_their_window() {
    let metrics = Arc::new(SqliteMaintenanceMetrics::default());

    for metric in SqliteMaintenanceInternalMetric::ALL {
        metrics.record(metric, Duration::from_millis(3));
    }

    let permit = metrics.start_permit(SqliteMaintenanceClass::RequestLogsGc);
    metrics.record_committed_progress(true, 4, true);
    drop(permit);

    let snapshot = metrics.snapshot();
    for metric in SqliteMaintenanceInternalMetric::ALL {
        assert_eq!(snapshot.internal(metric).count, 1);
        assert_eq!(snapshot.internal(metric).elapsed_ms, 3);
    }
    assert_eq!(
        snapshot.permit(SqliteMaintenanceClass::RequestLogsGc).count,
        1
    );
    assert_eq!(snapshot.cursor_progress, 1);
    assert_eq!(snapshot.range_progress, 4);
    assert_eq!(snapshot.completed, 1);

    let formatted = metrics.format();
    assert!(formatted.contains("physical_bulk_busy"));
    assert!(formatted.contains("coordinator_wait"));
    assert!(formatted.contains("request_logs_gc"));
    assert!(formatted.contains("committed_cursor=1"));
    assert!(formatted.contains("committed_range=4"));
    assert!(formatted.contains("completed=1"));

    metrics.rotate();

    let rotated = metrics.snapshot();
    assert_eq!(rotated.cursor_progress, 0);
    assert_eq!(rotated.range_progress, 0);
    assert_eq!(rotated.completed, 0);
    for metric in SqliteMaintenanceInternalMetric::ALL {
        assert_eq!(rotated.internal(metric).count, 0);
    }
    assert_eq!(
        rotated.permit(SqliteMaintenanceClass::RequestLogsGc).count,
        0
    );
}

#[test]
fn disabled_maintenance_metrics_do_not_record_lifecycle_events() {
    let metrics = Arc::new(SqliteMaintenanceMetrics::default());
    metrics.set_recording_enabled_for_test(false);

    for metric in SqliteMaintenanceInternalMetric::ALL {
        metrics.record(metric, Duration::from_millis(3));
    }
    let permit = metrics.start_permit(SqliteMaintenanceClass::RequestLogsGc);
    metrics.record_committed_progress(true, 4, true);
    drop(permit);

    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.cursor_progress, 0);
    assert_eq!(snapshot.range_progress, 0);
    assert_eq!(snapshot.completed, 0);
    for metric in SqliteMaintenanceInternalMetric::ALL {
        let stats = snapshot.internal(metric);
        assert_eq!(stats.count, 0);
        assert_eq!(stats.elapsed_ms, 0);
    }
    let permit = snapshot.permit(SqliteMaintenanceClass::RequestLogsGc);
    assert_eq!(permit.count, 0);
    assert_eq!(permit.hold_ms, 0);
}

#[tokio::test]
async fn maintenance_metrics_rotate_with_the_workload_window() {
    let runtime = SqliteRuntime::new(SqlitePool::connect_lazy("sqlite::memory:").unwrap());
    let permit = runtime
        .try_admit_maintenance_bulk(SqliteOperation::RequestLogsGc)
        .expect("maintenance admission");
    drop(permit);
    runtime.record_committed_maintenance_progress(true, 4, true);
    {
        let mut window = runtime.inner.workload.lock().unwrap();
        window.started_at = Instant::now() - SQLITE_WORKLOAD_LOG_INTERVAL;
    }

    runtime.record_success(
        SqliteOperation::RequestLogsGc,
        Duration::ZERO,
        Duration::ZERO,
        Duration::ZERO,
        0,
    );

    let snapshot = runtime.maintenance_metrics_snapshot_for_test();
    assert_eq!(snapshot.cursor_progress, 0);
    assert_eq!(snapshot.range_progress, 0);
    assert_eq!(snapshot.completed, 0);
    assert_eq!(
        snapshot.permit(SqliteMaintenanceClass::RequestLogsGc).count,
        0
    );
}

#[test]
fn maintenance_progress_counts_only_explicit_committed_results() {
    let metrics = Arc::new(SqliteMaintenanceMetrics::default());

    let canceled = metrics.start_permit(SqliteMaintenanceClass::ReconciliationProjection);
    drop(canceled);
    let rolled_back = metrics.start_permit(SqliteMaintenanceClass::ReconciliationProjection);
    drop(rolled_back);
    let stale_claim = metrics.start_permit(SqliteMaintenanceClass::ReconciliationProjection);
    drop(stale_claim);

    let before_commit = metrics.snapshot();
    assert_eq!(before_commit.cursor_progress, 0);
    assert_eq!(before_commit.range_progress, 0);
    assert_eq!(before_commit.completed, 0);

    metrics.record_committed_progress(true, 3, false);
    metrics.record_committed_progress(false, 0, true);

    let after_commit = metrics.snapshot();
    assert_eq!(after_commit.cursor_progress, 1);
    assert_eq!(after_commit.range_progress, 3);
    assert_eq!(after_commit.completed, 1);
}
