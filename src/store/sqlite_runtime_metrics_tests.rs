use super::*;

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
async fn workload_window_reports_canonical_alerts_warm_events_without_sensitive_fields() {
    let runtime = SqliteRuntime::new(SqlitePool::connect_lazy("sqlite::memory:").unwrap());
    runtime.record_admin_alerts_warm_slice();
    runtime.record_admin_alerts_warm_publish();
    runtime.record_admin_alerts_warm_generation_discard();
    runtime.record_admin_alerts_warm_defer();
    runtime.record_admin_alerts_warm_cold_miss();

    let window = runtime
        .inner
        .workload
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let formatted = format_admin_alerts_warm_window(window.admin_alerts_warm);
    assert_eq!(
        formatted,
        "slices=1,publishes=1,generation_discards=1,defers=1,cold_misses=1"
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

    let telemetry = runtime.operation_telemetry(SqliteOperation::ReconciliationProjection);
    assert_eq!(telemetry.connection_cache_write_pages, 7);
    assert_eq!(telemetry.cooperative_read_elapsed_ms, 42);
    assert_eq!(telemetry.cooperative_read_deadlines, 1);

    let window = runtime.inner.workload.lock().unwrap();
    let formatted = format_operation_window(&window.operations, &window.reconciliation_reads);
    assert!(formatted.contains("connection_cache_write_pages=7"));
    assert!(formatted.contains("cooperative_read_elapsed_ms=42"));
    assert!(formatted.contains("cooperative_read_deadlines=1"));
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
