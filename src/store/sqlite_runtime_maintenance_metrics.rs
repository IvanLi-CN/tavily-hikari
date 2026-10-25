use super::{
    AdminAlertsWarmWindow, OperationWindow, ReconciliationReadKind, ReconciliationReadWindow,
    SqliteFileStatePaths, SqliteMaintenanceClass, SqliteOperation,
    TRANSACTION_HOLD_BUCKET_UPPER_MS,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum SqliteMaintenanceInternalMetric {
    PhysicalBulkBusy,
    CoordinatorWait,
    PoolAcquire,
    Begin,
    NativeDeadline,
}

impl SqliteMaintenanceInternalMetric {
    pub(crate) const ALL: [Self; 5] = [
        Self::PhysicalBulkBusy,
        Self::CoordinatorWait,
        Self::PoolAcquire,
        Self::Begin,
        Self::NativeDeadline,
    ];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::PhysicalBulkBusy => "physical_bulk_busy",
            Self::CoordinatorWait => "coordinator_wait",
            Self::PoolAcquire => "pool_acquire",
            Self::Begin => "begin",
            Self::NativeDeadline => "native_deadline",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct SqliteMaintenanceMetricStats {
    pub(crate) count: u64,
    pub(crate) elapsed_ms: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct SqliteMaintenancePermitStats {
    pub(crate) count: u64,
    pub(crate) hold_ms: u64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SqliteMaintenanceMetricsSnapshot {
    internal: BTreeMap<SqliteMaintenanceInternalMetric, SqliteMaintenanceMetricStats>,
    permits: BTreeMap<SqliteMaintenanceClass, SqliteMaintenancePermitStats>,
    pub(crate) cursor_progress: u64,
    pub(crate) range_progress: u64,
    pub(crate) completed: u64,
}

impl SqliteMaintenanceMetricsSnapshot {
    pub(crate) fn internal(
        &self,
        metric: SqliteMaintenanceInternalMetric,
    ) -> SqliteMaintenanceMetricStats {
        self.internal.get(&metric).copied().unwrap_or_default()
    }

    pub(crate) fn permit(&self, class: SqliteMaintenanceClass) -> SqliteMaintenancePermitStats {
        self.permits.get(&class).copied().unwrap_or_default()
    }
}

#[derive(Debug, Default)]
struct SqliteMaintenanceMetricsWindow {
    internal: BTreeMap<SqliteMaintenanceInternalMetric, SqliteMaintenanceMetricStats>,
    permits: BTreeMap<SqliteMaintenanceClass, SqliteMaintenancePermitStats>,
    cursor_progress: u64,
    range_progress: u64,
    completed: u64,
}

#[derive(Debug)]
pub(crate) struct SqliteMaintenanceMetrics {
    window: Mutex<SqliteMaintenanceMetricsWindow>,
    recording_enabled: AtomicBool,
}

impl Default for SqliteMaintenanceMetrics {
    fn default() -> Self {
        Self {
            window: Mutex::new(SqliteMaintenanceMetricsWindow::default()),
            recording_enabled: AtomicBool::new(true),
        }
    }
}

impl SqliteMaintenanceMetrics {
    pub(crate) fn record(&self, metric: SqliteMaintenanceInternalMetric, elapsed: Duration) {
        if !self.recording_enabled.load(Ordering::Acquire) {
            return;
        }
        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let stats = window.internal.entry(metric).or_default();
        stats.count = stats.count.saturating_add(1);
        stats.elapsed_ms = stats
            .elapsed_ms
            .saturating_add(elapsed.as_millis().min(u64::MAX as u128) as u64);
    }

    pub(crate) fn start_permit(
        self: &Arc<Self>,
        class: SqliteMaintenanceClass,
    ) -> SqliteMaintenancePermitEvidence {
        let recording_enabled = self.recording_enabled.load(Ordering::Acquire);
        SqliteMaintenancePermitEvidence {
            metrics: self.clone(),
            class,
            started_at: Instant::now(),
            recording_enabled,
        }
    }

    pub(crate) fn record_committed_progress(
        &self,
        cursor_advanced: bool,
        range_rows: u64,
        completed: bool,
    ) {
        if !self.recording_enabled.load(Ordering::Acquire) {
            return;
        }
        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        window.cursor_progress = window
            .cursor_progress
            .saturating_add(u64::from(cursor_advanced));
        window.range_progress = window.range_progress.saturating_add(range_rows);
        window.completed = window.completed.saturating_add(u64::from(completed));
    }

    pub(crate) fn snapshot(&self) -> SqliteMaintenanceMetricsSnapshot {
        let window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        SqliteMaintenanceMetricsSnapshot {
            internal: window.internal.clone(),
            permits: window.permits.clone(),
            cursor_progress: window.cursor_progress,
            range_progress: window.range_progress,
            completed: window.completed,
        }
    }

    pub(crate) fn format(&self) -> String {
        let snapshot = self.snapshot();
        let internal = SqliteMaintenanceInternalMetric::ALL
            .into_iter()
            .map(|metric| {
                let stats = snapshot.internal(metric);
                format!(
                    "{}:count={},elapsed_ms={}",
                    metric.as_str(),
                    stats.count,
                    stats.elapsed_ms
                )
            })
            .collect::<Vec<_>>()
            .join("|");
        let permits = SqliteMaintenanceClass::ALL
            .into_iter()
            .map(|class| {
                let stats = snapshot.permit(class);
                format!(
                    "{}:count={},hold_ms={}",
                    class.as_str(),
                    stats.count,
                    stats.hold_ms
                )
            })
            .collect::<Vec<_>>()
            .join("|");
        format!(
            "internal={internal},permits={permits},progress=committed_cursor={},committed_range={},completed={}",
            snapshot.cursor_progress, snapshot.range_progress, snapshot.completed
        )
    }

    pub(crate) fn rotate(&self) {
        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *window = SqliteMaintenanceMetricsWindow::default();
    }

    #[cfg(test)]
    pub(crate) fn set_recording_enabled_for_test(&self, enabled: bool) {
        self.recording_enabled.store(enabled, Ordering::Release);
    }
}

#[derive(Debug)]
pub(crate) struct SqliteMaintenancePermitEvidence {
    metrics: Arc<SqliteMaintenanceMetrics>,
    class: SqliteMaintenanceClass,
    started_at: Instant,
    recording_enabled: bool,
}

impl Drop for SqliteMaintenancePermitEvidence {
    fn drop(&mut self) {
        if !self.recording_enabled {
            return;
        }
        let hold_ms = self.started_at.elapsed().as_millis().min(u64::MAX as u128) as u64;
        let mut window = self
            .metrics
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let stats = window.permits.entry(self.class).or_default();
        stats.count = stats.count.saturating_add(1);
        stats.hold_ms = stats.hold_ms.saturating_add(hold_ms);
    }
}

pub(super) fn monotonic_delta(current: Option<u64>, previous: Option<u64>) -> Option<u64> {
    current
        .zip(previous)
        .map(|(current, previous)| current.saturating_sub(previous))
}

pub(super) fn format_operation_window(
    operations: &BTreeMap<SqliteOperation, OperationWindow>,
    reconciliation_reads: &BTreeMap<ReconciliationReadKind, ReconciliationReadWindow>,
) -> String {
    operations
        .iter()
        .map(|(operation, metrics)| {
            let deferred_reasons = metrics
                .deferred_by_reason
                .iter()
                .map(|(reason, count)| format!("{}={count}", reason.as_str()))
                .collect::<Vec<_>>()
                .join("|");
            let cache_write_pages = if metrics.connection_cache_write_sampled
                && !metrics.connection_cache_write_sample_failed
            {
                metrics.connection_cache_write_pages.to_string()
            } else {
                "unknown".to_string()
            };
            format!(
                "{}/{}:calls={},deferred={},defer_reasons={},errors={},retries={},discarded={},pool_wait_ms={},begin_wait_ms={},hold_ms={},hold_p95_ms={},rows={},connection_cache_write_pages={},cooperative_read_elapsed_ms={},cooperative_read_deadlines={}",
                operation.workload_class(),
                operation.as_str(),
                metrics.calls,
                metrics.deferred,
                deferred_reasons,
                metrics.errors,
                metrics.retries,
                metrics.discarded_connections,
                metrics.pool_wait_ms,
                metrics.begin_wait_ms,
                metrics.hold_ms,
                transaction_hold_p95_ms(&metrics.hold_histogram),
                metrics.rows_affected,
                cache_write_pages,
                metrics.cooperative_read_elapsed_ms,
                metrics.cooperative_read_deadlines,
            )
        })
        .chain(reconciliation_reads.iter().map(|(kind, metrics)| {
            let cache_write_pages = if metrics.connection_cache_write_sampled
                && !metrics.connection_cache_write_sample_failed
            {
                metrics.connection_cache_write_pages.to_string()
            } else {
                "unknown".to_string()
            };
            format!(
                "reconciliation_read/{}:calls={},elapsed_ms={},deadlines={},deferred={},discarded={},connection_cache_write_pages={},key_observation_reuses={},key_observation_identity_misses={}",
                kind.as_str(),
                metrics.calls,
                metrics.elapsed_ms,
                metrics.deadlines,
                metrics.deferred,
                metrics.discarded_connections,
                cache_write_pages,
                metrics.key_observation_reuses,
                metrics.key_observation_identity_misses,
            )
        }))
        .collect::<Vec<_>>()
        .join(";")
}

pub(super) fn format_admin_alerts_warm_window(metrics: AdminAlertsWarmWindow) -> String {
    format!(
        "slices={},publishes={},generation_discards={},defers={},cold_misses={},canonical_events_indexed_reads={},canonical_catalog_payload_slices={},canonical_group_build_slices={},canonical_group_reduction_slices={},canonical_group_publishes={},canonical_group_defers={}",
        metrics.slices,
        metrics.publishes,
        metrics.generation_discards,
        metrics.defers,
        metrics.cold_misses,
        metrics.canonical_events_indexed_reads,
        metrics.canonical_catalog_payload_slices,
        metrics.canonical_group_build_slices,
        metrics.canonical_group_reduction_slices,
        metrics.canonical_group_publishes,
        metrics.canonical_group_defers,
    )
}

pub(super) fn format_sqlite_file_state(paths: &SqliteFileStatePaths) -> String {
    let mut fields = Vec::with_capacity(4);
    for (label, path) in [
        ("core", paths.core.as_ref()),
        ("observability", paths.observability.as_ref()),
    ] {
        let Some(path) = path else {
            continue;
        };
        fields.push(format!("{label}_db_bytes={}", sqlite_file_size(path)));
        let wal_path = PathBuf::from(format!("{}-wal", path.display()));
        fields.push(format!("{label}_wal_bytes={}", sqlite_file_size(&wal_path)));
    }
    if fields.is_empty() {
        "unconfigured".to_string()
    } else {
        fields.join(",")
    }
}

fn sqlite_file_size(path: &std::path::Path) -> String {
    std::fs::metadata(path)
        .map(|metadata| metadata.len().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

pub(super) fn transaction_hold_p95_ms(
    histogram: &[u64; TRANSACTION_HOLD_BUCKET_UPPER_MS.len()],
) -> u64 {
    let samples = histogram.iter().sum::<u64>();
    if samples == 0 {
        return 0;
    }
    let target = samples.saturating_mul(95).saturating_add(99) / 100;
    let mut cumulative = 0_u64;
    for (index, count) in histogram.iter().enumerate() {
        cumulative = cumulative.saturating_add(*count);
        if cumulative >= target {
            return TRANSACTION_HOLD_BUCKET_UPPER_MS[index];
        }
    }
    TRANSACTION_HOLD_BUCKET_UPPER_MS[TRANSACTION_HOLD_BUCKET_UPPER_MS.len() - 1]
}
