use super::{
    ProxyError, SqliteAdmissionDeferReason, SqliteOperation, SqliteRuntime,
    is_transient_sqlite_write_error, read_cgroup_write_bytes, read_process_write_bytes,
};
use std::sync::atomic::Ordering as AtomicOrdering;
use std::time::{Duration, Instant};
use tracing::{debug, error, warn};

pub(super) fn record_error(
    runtime: &SqliteRuntime,
    operation: SqliteOperation,
    pool_wait: Duration,
    begin_wait: Duration,
    err: &ProxyError,
) {
    let transient = is_transient_sqlite_write_error(err);
    let pool_timeout = matches!(err, ProxyError::Database(sqlx::Error::PoolTimedOut));
    let contention_entered = if transient && !pool_timeout {
        *runtime
            .inner
            .last_contention_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Instant::now());
        runtime
            .inner
            .contention_warning_active
            .compare_exchange(false, true, AtomicOrdering::AcqRel, AtomicOrdering::Acquire)
            .is_ok()
    } else {
        false
    };
    let error_category = if pool_timeout {
        "pool_timeout"
    } else if transient {
        "sqlite_busy"
    } else {
        "database"
    };
    if !transient || contention_entered {
        let process_write_bytes = read_process_write_bytes();
        let cgroup_write_bytes = read_cgroup_write_bytes();
        if transient {
            warn!(
                component = "db",
                event = "sqlite_runtime_contention_entered",
                operation = operation.as_str(),
                workload_class = operation.workload_class(),
                pool_wait_ms = pool_wait.as_millis() as u64,
                begin_wait_ms = begin_wait.as_millis() as u64,
                error_category,
                process_write_bytes_aggregate = process_write_bytes.unwrap_or_default(),
                cgroup_write_bytes_aggregate = cgroup_write_bytes.unwrap_or_default(),
                "SQLite workload contention entered"
            );
        } else {
            error!(
                component = "db",
                event = "sqlite_runtime_error",
                operation = operation.as_str(),
                workload_class = operation.workload_class(),
                pool_wait_ms = pool_wait.as_millis() as u64,
                begin_wait_ms = begin_wait.as_millis() as u64,
                error_category,
                process_write_bytes_aggregate = process_write_bytes.unwrap_or_default(),
                cgroup_write_bytes_aggregate = cgroup_write_bytes.unwrap_or_default(),
                "sqlite runtime operation failed"
            );
        }
    } else {
        debug!(
            component = "db",
            event = "sqlite_runtime_contention",
            operation = operation.as_str(),
            workload_class = operation.workload_class(),
            pool_wait_ms = pool_wait.as_millis() as u64,
            begin_wait_ms = begin_wait.as_millis() as u64,
            error_category,
            "SQLite workload contention remains active"
        );
    }
    let maintenance_transient_defer = transient
        && (operation.is_maintenance_bulk()
            || matches!(
                operation,
                SqliteOperation::ScheduledJobControl | SqliteOperation::HaOutboxGcWatchdog
            ));
    let maintenance_defer_reason = maintenance_transient_defer.then_some(
        if matches!(operation, SqliteOperation::MaintenanceCapacityWarm) {
            SqliteAdmissionDeferReason::PoolPressure
        } else {
            SqliteAdmissionDeferReason::RecentContention
        },
    );
    runtime.record(
        operation,
        pool_wait,
        begin_wait,
        Duration::ZERO,
        0,
        !maintenance_transient_defer,
        false,
        maintenance_defer_reason,
    );
}
