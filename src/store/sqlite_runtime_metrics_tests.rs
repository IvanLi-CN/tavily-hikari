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
    let formatted = format_operation_window(&window.operations);
    assert!(formatted.contains("maintenance_bulk/request_stats_flush"));
    assert!(formatted.contains("defer_reasons=pool_pressure=1"));
    assert!(!formatted.contains("SELECT"));
    assert!(!formatted.contains("INSERT"));
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
    let formatted = format_operation_window(&window.operations);
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
        format_operation_window(&window.operations)
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
