use super::{ImmediateSqliteTransaction, KeyStore, ProxyError, is_transient_sqlite_write_error};
use sqlx::{Connection, Sqlite, SqliteConnection, SqlitePool};
use std::collections::BTreeMap;
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::{debug, error, info, warn};

const SQLITE_WORKLOAD_LOG_INTERVAL: Duration = Duration::from_secs(60);
const DEFAULT_BUSY_TIMEOUT_MS: i64 = 5_000;

const MAINTENANCE_BULK_MAX_FOREGROUND_RPS: i64 = 5;
const MAINTENANCE_BULK_CONTENTION_COOLDOWN: Duration = Duration::from_secs(5);
const MAINTENANCE_BULK_RESERVED_FOREGROUND_CONNECTIONS: u32 = 2;
const MAINTENANCE_BULK_HEAP_TRIM_INTERVAL: Duration = Duration::from_secs(5 * 60);
const FOREGROUND_ACTIVITY_BUCKETS: usize = 10;
const FOREGROUND_ACTIVITY_BUCKET_MS: u64 = 100;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum SqliteAdmissionDeferReason {
    BulkBusy,
    ForegroundPressure,
    PoolPressure,
    RecentContention,
}

impl SqliteAdmissionDeferReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::BulkBusy => "bulk_busy",
            Self::ForegroundPressure => "foreground_pressure",
            Self::PoolPressure => "pool_pressure",
            Self::RecentContention => "recent_contention",
        }
    }
}

#[derive(Debug)]
pub(crate) struct SqliteMaintenanceBulkPermit {
    _permit: OwnedSemaphorePermit,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum SqliteOperation {
    BillingLedgerAuditRead,
    DashboardIntegrityWrite,
    ForegroundJobTrigger,
    HaBaselineRead,
    HaEventsRead,
    HaOutboxGc,
    HaOutboxGcWatchdog,
    RequestLogsGc,
    RequestStatsFlush,
    ServerPressureRebuild,
    ReconciliationProjection,
    ScheduledJobControl,
}

impl SqliteOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::BillingLedgerAuditRead => "billing_ledger_audit_read",
            Self::DashboardIntegrityWrite => "dashboard_integrity_write",
            Self::ForegroundJobTrigger => "foreground_job_trigger",
            Self::HaBaselineRead => "ha_baseline_read",
            Self::HaEventsRead => "ha_events_read",
            Self::HaOutboxGc => "ha_outbox_gc",
            Self::HaOutboxGcWatchdog => "ha_outbox_gc_watchdog",
            Self::RequestLogsGc => "request_logs_gc",
            Self::RequestStatsFlush => "request_stats_flush",
            Self::ServerPressureRebuild => "server_pressure_rebuild",
            Self::ReconciliationProjection => "reconciliation_projection",
            Self::ScheduledJobControl => "scheduled_job_control",
        }
    }

    fn workload_class(self) -> &'static str {
        match self {
            Self::ForegroundJobTrigger => "foreground_work",
            Self::BillingLedgerAuditRead | Self::HaBaselineRead | Self::HaEventsRead => {
                "maintenance_read"
            }
            Self::ScheduledJobControl | Self::HaOutboxGcWatchdog => "maintenance_control",
            Self::DashboardIntegrityWrite
            | Self::HaOutboxGc
            | Self::RequestLogsGc
            | Self::RequestStatsFlush
            | Self::ServerPressureRebuild
            | Self::ReconciliationProjection => "maintenance_bulk",
        }
    }

    fn acquire_budget(self) -> Duration {
        match self {
            Self::DashboardIntegrityWrite
            | Self::ScheduledJobControl
            | Self::HaOutboxGcWatchdog => Duration::from_millis(100),
            Self::ForegroundJobTrigger => Duration::from_millis(250),
            Self::HaOutboxGc
            | Self::RequestLogsGc
            | Self::RequestStatsFlush
            | Self::ServerPressureRebuild
            | Self::ReconciliationProjection => Duration::from_millis(100),
            _ => Duration::from_secs(5),
        }
    }

    fn begin_budget(self) -> Duration {
        match self {
            Self::DashboardIntegrityWrite
            | Self::RequestStatsFlush
            | Self::ScheduledJobControl
            | Self::HaOutboxGcWatchdog => Duration::from_millis(100),
            Self::ForegroundJobTrigger => Duration::from_millis(100),
            Self::HaOutboxGc
            | Self::RequestLogsGc
            | Self::ServerPressureRebuild
            | Self::ReconciliationProjection => Duration::from_millis(250),
            _ => Duration::from_secs(5),
        }
    }

    fn busy_timeout_override_ms(self) -> Option<i64> {
        match self {
            // Dashboard integrity already used this per-connection short
            // timeout before workload admission existed. All new admission
            // paths retain the database's configured busy timeout and use an
            // explicit operation budget instead.
            Self::DashboardIntegrityWrite => Some(100),
            _ => None,
        }
    }

    fn is_maintenance_bulk(self) -> bool {
        matches!(
            self,
            Self::DashboardIntegrityWrite
                | Self::HaOutboxGc
                | Self::RequestLogsGc
                | Self::RequestStatsFlush
                | Self::ServerPressureRebuild
                | Self::ReconciliationProjection
        )
    }

    fn probes_recent_contention(self) -> bool {
        // The coalescer can atomically restore an uncommitted batch. Let it
        // make one bounded attempt on its nominal wake instead of extending a
        // prior writer conflict into a multi-second metrics backlog. Other
        // bulk work keeps the full contention cooldown.
        matches!(self, Self::RequestStatsFlush)
    }
}

impl fmt::Display for SqliteOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Default)]
struct OperationWindow {
    calls: u64,
    deferred: u64,
    errors: u64,
    retries: u64,
    discarded_connections: u64,
    pool_wait_ms: u64,
    begin_wait_ms: u64,
    hold_ms: u64,
    rows_affected: u64,
    deferred_by_reason: BTreeMap<SqliteAdmissionDeferReason, u64>,
}

#[derive(Debug)]
struct WorkloadWindow {
    started_at: Instant,
    operations: BTreeMap<SqliteOperation, OperationWindow>,
    last_process_write_bytes: Option<u64>,
    last_cgroup_write_bytes: Option<u64>,
    minimum_idle_connections: Option<u32>,
    maximum_in_use_connections: u32,
    maximum_acquire_waiters: u32,
}

impl Default for WorkloadWindow {
    fn default() -> Self {
        Self {
            started_at: Instant::now(),
            operations: BTreeMap::new(),
            last_process_write_bytes: None,
            last_cgroup_write_bytes: None,
            minimum_idle_connections: None,
            maximum_in_use_connections: 0,
            maximum_acquire_waiters: 0,
        }
    }
}

#[derive(Debug)]
struct SqliteRuntimeInner {
    pool: SqlitePool,
    maximum_connections: u32,
    maintenance_bulk: Arc<Semaphore>,
    last_bulk_heap_trim_at: Mutex<Option<Instant>>,
    last_contention_at: Mutex<Option<Instant>>,
    contention_warning_active: AtomicBool,
    foreground_activity: ForegroundActivityMeter,
    acquire_waiters: AtomicU32,
    peak_acquire_waiters: AtomicU32,
    workload: Mutex<WorkloadWindow>,
}

#[derive(Debug)]
struct ForegroundActivityMeter {
    buckets: [AtomicU64; FOREGROUND_ACTIVITY_BUCKETS],
    started_slot: u64,
    last_high_pressure_slot: AtomicU64,
}

impl ForegroundActivityMeter {
    fn new() -> Self {
        Self {
            buckets: std::array::from_fn(|_| AtomicU64::new(0)),
            started_slot: foreground_activity_slot(),
            last_high_pressure_slot: AtomicU64::new(0),
        }
    }

    fn record_at(&self, slot: u64) {
        const COUNT_BITS: u32 = 16;
        const COUNT_MASK: u64 = (1 << COUNT_BITS) - 1;
        let bucket = &self.buckets[(slot as usize) % FOREGROUND_ACTIVITY_BUCKETS];
        loop {
            let current = bucket.load(AtomicOrdering::Acquire);
            let epoch = current >> COUNT_BITS;
            let count = current & COUNT_MASK;
            let next_count = if epoch == slot {
                count.saturating_add(1).min(COUNT_MASK)
            } else {
                1
            };
            let next = (slot << COUNT_BITS) | next_count;
            if bucket
                .compare_exchange(
                    current,
                    next,
                    AtomicOrdering::AcqRel,
                    AtomicOrdering::Acquire,
                )
                .is_ok()
            {
                break;
            }
        }
        if self.rps_at(slot) > MAINTENANCE_BULK_MAX_FOREGROUND_RPS {
            self.last_high_pressure_slot
                .fetch_max(slot, AtomicOrdering::AcqRel);
        }
    }

    fn rps_at(&self, current_slot: u64) -> i64 {
        let arrivals = self
            .buckets
            .iter()
            .filter_map(|bucket| {
                const COUNT_BITS: u32 = 16;
                const COUNT_MASK: u64 = (1 << COUNT_BITS) - 1;
                let value = bucket.load(AtomicOrdering::Acquire);
                let epoch = value >> COUNT_BITS;
                (current_slot >= epoch
                    && current_slot.saturating_sub(epoch) < FOREGROUND_ACTIVITY_BUCKETS as u64)
                    .then_some(value & COUNT_MASK)
            })
            .sum::<u64>();
        arrivals.min(i64::MAX as u64) as i64
    }

    fn low_pressure_since_floor_at(&self, current_slot: u64) -> i64 {
        let last_high = self.last_high_pressure_slot.load(AtomicOrdering::Acquire);
        self.started_slot
            .max(last_high)
            .min(current_slot)
            .saturating_mul(FOREGROUND_ACTIVITY_BUCKET_MS)
            .saturating_div(1_000)
            .min(i64::MAX as u64) as i64
    }
}

fn foreground_activity_slot() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .saturating_div(FOREGROUND_ACTIVITY_BUCKET_MS as u128) as u64
}

#[derive(Debug)]
pub struct SqliteForegroundSubscription;

impl Drop for SqliteForegroundSubscription {
    fn drop(&mut self) {}
}

struct PoolAcquireWaiter {
    inner: Arc<SqliteRuntimeInner>,
}

impl Drop for PoolAcquireWaiter {
    fn drop(&mut self) {
        self.inner
            .acquire_waiters
            .fetch_sub(1, AtomicOrdering::AcqRel);
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SqliteRuntime {
    inner: Arc<SqliteRuntimeInner>,
}

impl SqliteRuntime {
    pub(crate) fn new(pool: SqlitePool) -> Self {
        Self::with_max_connections(pool, crate::SQLITE_POOL_MAX_CONNECTIONS_DEFAULT)
    }

    pub(crate) fn with_max_connections(pool: SqlitePool, maximum_connections: u32) -> Self {
        Self {
            inner: Arc::new(SqliteRuntimeInner {
                pool,
                maximum_connections: maximum_connections.max(1),
                maintenance_bulk: Arc::new(Semaphore::new(1)),
                last_bulk_heap_trim_at: Mutex::new(None),
                last_contention_at: Mutex::new(None),
                contention_warning_active: AtomicBool::new(false),
                foreground_activity: ForegroundActivityMeter::new(),
                acquire_waiters: AtomicU32::new(0),
                peak_acquire_waiters: AtomicU32::new(0),
                workload: Mutex::new(WorkloadWindow::default()),
            }),
        }
    }

    pub(crate) fn record_foreground_activity(&self) {
        self.inner
            .foreground_activity
            .record_at(foreground_activity_slot());
    }

    pub(crate) fn foreground_activity_rps(&self) -> i64 {
        self.inner
            .foreground_activity
            .rps_at(foreground_activity_slot())
    }

    pub(crate) fn foreground_activity_low_pressure_since_floor(&self) -> i64 {
        self.inner
            .foreground_activity
            .low_pressure_since_floor_at(foreground_activity_slot())
    }

    #[cfg(test)]
    pub(crate) fn mark_recent_contention_for_test(&self) {
        *self
            .inner
            .last_contention_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Instant::now());
    }

    pub(crate) fn release_bulk_heap_after_connection_close(&self) {
        // SQLite and SQLx release their allocations when the short-lived bulk
        // connection closes, but glibc may retain those free pages in the
        // process heap after a large retention delete. Only ask the allocator
        // to return them while the same foreground checks that admitted bulk
        // work still show an idle process.
        let can_trim = self.foreground_activity_rps() <= MAINTENANCE_BULK_MAX_FOREGROUND_RPS
            && self.inner.acquire_waiters.load(AtomicOrdering::Acquire) == 0;

        if !can_trim || !self.bulk_heap_trim_due() {
            return;
        }

        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        {
            // SAFETY: `malloc_trim` is process-global but does not require an
            // allocator-owned pointer. The caller has already closed its bulk
            // SQLite connection; admission excludes foreground waits and the
            // five-minute rate limit prevents recovery micro-slices from
            // repeatedly taking the allocator lock.
            let started_at = Instant::now();
            unsafe {
                libc::malloc_trim(0);
            }
            debug!(
                component = "sqlite_runtime",
                event = "bulk_heap_trim",
                elapsed_ms = started_at.elapsed().as_millis() as u64,
                "released bulk SQLite heap pages after connection close"
            );
        }

        #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
        let _ = can_trim;
    }

    fn bulk_heap_trim_due(&self) -> bool {
        let now = Instant::now();
        let mut last_trim_at = self
            .inner
            .last_bulk_heap_trim_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if last_trim_at
            .is_some_and(|last| now.duration_since(last) < MAINTENANCE_BULK_HEAP_TRIM_INTERVAL)
        {
            return false;
        }
        *last_trim_at = Some(now);
        true
    }

    pub(crate) fn subscribe_dashboard_sse(&self) -> impl Drop {
        // A dashboard SSE connection is a foreground arrival, but it is not a
        // sustained request rate. Counting its lifetime as RPS would freeze
        // recovery whenever dashboards stay open.
        self.record_foreground_activity();
        SqliteForegroundSubscription
    }

    pub(crate) fn try_admit_maintenance_bulk(
        &self,
        operation: SqliteOperation,
    ) -> Result<SqliteMaintenanceBulkPermit, SqliteAdmissionDeferReason> {
        debug_assert!(operation.is_maintenance_bulk());
        let reason = self.maintenance_bulk_defer_reason_for(operation);
        if let Some(reason) = reason {
            self.record_deferred(operation, reason);
            return Err(reason);
        }
        match self.inner.maintenance_bulk.clone().try_acquire_owned() {
            Ok(permit) => Ok(SqliteMaintenanceBulkPermit { _permit: permit }),
            Err(_) => {
                self.record_deferred(operation, SqliteAdmissionDeferReason::BulkBusy);
                Err(SqliteAdmissionDeferReason::BulkBusy)
            }
        }
    }

    pub(crate) fn maintenance_bulk_continue_reason(&self) -> Option<SqliteAdmissionDeferReason> {
        let foreground_rps = self.foreground_activity_rps();
        if foreground_rps > MAINTENANCE_BULK_MAX_FOREGROUND_RPS {
            Some(SqliteAdmissionDeferReason::ForegroundPressure)
        } else if self.recent_contention_active() {
            Some(SqliteAdmissionDeferReason::RecentContention)
        } else if !self.has_foreground_pool_capacity() {
            Some(SqliteAdmissionDeferReason::PoolPressure)
        } else {
            None
        }
    }

    pub(crate) fn dashboard_read_defer_reason(&self) -> Option<SqliteAdmissionDeferReason> {
        // Dashboard snapshot construction is a foreground read, not bulk
        // maintenance. It must contain itself under real pressure without
        // requiring two already-open idle connections: a lazy three-connection
        // pool is intentionally allowed to stay at one connection while idle.
        if self.foreground_activity_rps() > MAINTENANCE_BULK_MAX_FOREGROUND_RPS {
            Some(SqliteAdmissionDeferReason::ForegroundPressure)
        } else if self.recent_contention_active() {
            Some(SqliteAdmissionDeferReason::RecentContention)
        } else if self.inner.pool.num_idle() == 0
            || self.inner.acquire_waiters.load(AtomicOrdering::Acquire) > 0
        {
            Some(SqliteAdmissionDeferReason::PoolPressure)
        } else {
            None
        }
    }

    fn maintenance_bulk_defer_reason_for(
        &self,
        operation: SqliteOperation,
    ) -> Option<SqliteAdmissionDeferReason> {
        let foreground_rps = self.foreground_activity_rps();
        if foreground_rps > MAINTENANCE_BULK_MAX_FOREGROUND_RPS {
            Some(SqliteAdmissionDeferReason::ForegroundPressure)
        } else if self.recent_contention_active() && !operation.probes_recent_contention() {
            Some(SqliteAdmissionDeferReason::RecentContention)
        } else if !self.has_foreground_pool_capacity() {
            Some(SqliteAdmissionDeferReason::PoolPressure)
        } else if self.inner.maintenance_bulk.available_permits() == 0 {
            Some(SqliteAdmissionDeferReason::BulkBusy)
        } else {
            None
        }
    }

    pub(crate) fn record_retry(&self, operation: SqliteOperation) {
        let mut window = self
            .inner
            .workload
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let metrics = window.operations.entry(operation).or_default();
        metrics.retries = metrics.retries.saturating_add(1);
    }

    fn has_foreground_pool_capacity(&self) -> bool {
        // Keep two foreground slots available. A lazy pool begins with one
        // open connection, so unopened capacity is usable for this admission
        // decision as long as it still covers both reserved foreground slots
        // and the single bulk permit. This does not acquire a connection: the
        // eventual operation remains bounded by its 100ms pool budget.
        let idle = self.inner.pool.num_idle().min(u32::MAX as usize) as u32;
        let unopened = self
            .inner
            .maximum_connections
            .saturating_sub(self.inner.pool.size());
        // Once a pool has reached its configured size, two returned slots are
        // sufficient: any already checked-out connection is part of the
        // foreground capacity currently in use. Before the pool grows, reserve
        // the two allocatable foreground slots plus the one pending bulk slot.
        idle >= MAINTENANCE_BULK_RESERVED_FOREGROUND_CONNECTIONS
            || idle.saturating_add(unopened)
                >= MAINTENANCE_BULK_RESERVED_FOREGROUND_CONNECTIONS.saturating_add(1)
    }

    fn recent_contention_active(&self) -> bool {
        self.inner
            .last_contention_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some_and(|at| at.elapsed() < MAINTENANCE_BULK_CONTENTION_COOLDOWN)
    }

    fn pool_acquire_waiter(&self) -> PoolAcquireWaiter {
        let current = self
            .inner
            .acquire_waiters
            .fetch_add(1, AtomicOrdering::AcqRel)
            .saturating_add(1);
        self.inner
            .peak_acquire_waiters
            .fetch_max(current, AtomicOrdering::AcqRel);
        PoolAcquireWaiter {
            inner: self.inner.clone(),
        }
    }

    async fn acquire_pool_connection(
        &self,
        operation: SqliteOperation,
    ) -> Result<(sqlx::pool::PoolConnection<Sqlite>, Duration), ProxyError> {
        let acquire_started = Instant::now();
        let acquire_waiter = self.pool_acquire_waiter();
        match tokio::time::timeout(operation.acquire_budget(), self.inner.pool.acquire()).await {
            Ok(Ok(conn)) => {
                drop(acquire_waiter);
                Ok((conn, acquire_started.elapsed()))
            }
            Ok(Err(err)) => {
                let err = ProxyError::Database(err);
                self.record_error(operation, acquire_started.elapsed(), Duration::ZERO, &err);
                Err(err)
            }
            Err(_) => {
                // Keep the timeout typed so every caller can make the same
                // bounded-defer decision as a native sqlx pool timeout.
                // Wrapping it in `Other` turns expected contention into an
                // apparent scheduler failure and defeats foreground admission.
                let err = ProxyError::Database(sqlx::Error::PoolTimedOut);
                self.record_error(operation, acquire_started.elapsed(), Duration::ZERO, &err);
                Err(err)
            }
        }
    }

    pub(crate) async fn acquire_operation_connection(
        &self,
        operation: SqliteOperation,
    ) -> Result<SqliteOperationConnection, ProxyError> {
        let (conn, pool_wait) = self.acquire_pool_connection(operation).await?;
        let (conn, restore_busy_timeout) =
            match configure_operation_connection(conn, operation).await {
                Ok(configured) => configured,
                Err(err) => {
                    let err = ProxyError::Database(err);
                    self.record_error(operation, pool_wait, Duration::ZERO, &err);
                    return Err(err);
                }
            };
        Ok(SqliteOperationConnection {
            conn: Some(conn),
            runtime: self.clone(),
            operation,
            pool_wait,
            started_at: Instant::now(),
            restore_busy_timeout,
        })
    }

    pub(crate) async fn begin_read_snapshot(
        &self,
        operation: SqliteOperation,
    ) -> Result<SqliteReadSnapshot, ProxyError> {
        let (conn, pool_wait) = self.acquire_pool_connection(operation).await?;
        let begin_started = Instant::now();
        let mut snapshot = SqliteReadSnapshot {
            conn: Some(conn),
            runtime: self.clone(),
            operation,
            pool_wait,
            begin_wait: Duration::ZERO,
            started_at: Instant::now(),
        };
        if let Err(err) = sqlx::query("BEGIN").execute(&mut *snapshot).await {
            let conn = snapshot
                .conn
                .take()
                .expect("SQLite read snapshot connection");
            conn.detach().close().await.ok();
            let err = ProxyError::Database(err);
            self.record_error(operation, pool_wait, begin_started.elapsed(), &err);
            return Err(err);
        }
        snapshot.begin_wait = begin_started.elapsed();
        snapshot.started_at = Instant::now();
        Ok(snapshot)
    }

    pub(crate) async fn begin_immediate(
        &self,
        operation: SqliteOperation,
    ) -> Result<SqliteImmediateTransaction, ProxyError> {
        let (conn, pool_wait) = self.acquire_pool_connection(operation).await?;
        let (conn, restore_busy_timeout) =
            match configure_operation_connection(conn, operation).await {
                Ok(configured) => configured,
                Err(err) => {
                    let err = ProxyError::Database(err);
                    self.record_error(operation, pool_wait, Duration::ZERO, &err);
                    return Err(err);
                }
            };
        let begin_started = Instant::now();
        let mut transaction = match tokio::time::timeout(
            operation.begin_budget(),
            ImmediateSqliteTransaction::begin(conn),
        )
        .await
        {
            Ok(Ok(transaction)) => transaction,
            Ok(Err(err)) => {
                self.record_error(operation, pool_wait, begin_started.elapsed(), &err);
                return Err(err);
            }
            Err(_) => {
                let err = ProxyError::Database(sqlx::Error::PoolTimedOut);
                self.record_error(operation, pool_wait, begin_started.elapsed(), &err);
                return Err(err);
            }
        };
        let start_total_changes = match sqlx::query_scalar::<_, i64>("SELECT total_changes()")
            .fetch_one(&mut *transaction)
            .await
        {
            Ok(total) => total.max(0) as u64,
            Err(err) => {
                let _ = transaction.rollback().await;
                let err = ProxyError::Database(err);
                self.record_error(operation, pool_wait, begin_started.elapsed(), &err);
                return Err(err);
            }
        };
        Ok(SqliteImmediateTransaction {
            transaction: Some(transaction),
            runtime: self.clone(),
            operation,
            pool_wait,
            begin_wait: begin_started.elapsed(),
            started_at: Instant::now(),
            start_total_changes,
            restore_busy_timeout,
        })
    }

    pub(crate) async fn begin_immediate_before(
        &self,
        operation: SqliteOperation,
        deadline: Instant,
    ) -> Result<SqliteImmediateTransaction, ProxyError> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            self.record_deferred(operation, SqliteAdmissionDeferReason::RecentContention);
            return Err(ProxyError::Database(sqlx::Error::PoolTimedOut));
        }
        match tokio::time::timeout(remaining, self.begin_immediate(operation)).await {
            Ok(result) => result,
            Err(_) => {
                self.record_deferred(operation, SqliteAdmissionDeferReason::RecentContention);
                Err(ProxyError::Database(sqlx::Error::PoolTimedOut))
            }
        }
    }

    fn record_success(
        &self,
        operation: SqliteOperation,
        pool_wait: Duration,
        begin_wait: Duration,
        hold: Duration,
        rows_affected: u64,
    ) {
        self.record(
            operation,
            pool_wait,
            begin_wait,
            hold,
            rows_affected,
            false,
            false,
            None,
        );
    }

    pub(crate) fn record_deferred(
        &self,
        operation: SqliteOperation,
        reason: SqliteAdmissionDeferReason,
    ) {
        self.record(
            operation,
            Duration::ZERO,
            Duration::ZERO,
            Duration::ZERO,
            0,
            false,
            false,
            Some(reason),
        );
    }

    fn record_error(
        &self,
        operation: SqliteOperation,
        pool_wait: Duration,
        begin_wait: Duration,
        err: &ProxyError,
    ) {
        let transient = is_transient_sqlite_write_error(err);
        let contention_entered = if transient {
            *self
                .inner
                .last_contention_at
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Instant::now());
            self.inner
                .contention_warning_active
                .compare_exchange(false, true, AtomicOrdering::AcqRel, AtomicOrdering::Acquire)
                .is_ok()
        } else {
            false
        };
        let error_category = if transient { "sqlite_busy" } else { "database" };
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
                    process_write_bytes = process_write_bytes.unwrap_or_default(),
                    cgroup_write_bytes = cgroup_write_bytes.unwrap_or_default(),
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
                    process_write_bytes = process_write_bytes.unwrap_or_default(),
                    cgroup_write_bytes = cgroup_write_bytes.unwrap_or_default(),
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
        self.record(
            operation,
            pool_wait,
            begin_wait,
            Duration::ZERO,
            0,
            !maintenance_transient_defer,
            false,
            maintenance_transient_defer.then_some(SqliteAdmissionDeferReason::RecentContention),
        );
    }

    fn record_discard(
        &self,
        operation: SqliteOperation,
        pool_wait: Duration,
        begin_wait: Duration,
        hold: Duration,
    ) {
        warn!(
            component = "db",
            event = "sqlite_transaction_connection_discarded",
            operation = operation.as_str(),
            workload_class = operation.workload_class(),
            pool_wait_ms = pool_wait.as_millis() as u64,
            begin_wait_ms = begin_wait.as_millis() as u64,
            hold_ms = hold.as_millis() as u64,
            "discarded a physical SQLite connection with an unfinished transaction"
        );
        self.record(operation, pool_wait, begin_wait, hold, 0, false, true, None);
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
        &self,
        operation: SqliteOperation,
        pool_wait: Duration,
        begin_wait: Duration,
        hold: Duration,
        rows_affected: u64,
        error: bool,
        discarded: bool,
        deferred: Option<SqliteAdmissionDeferReason>,
    ) {
        let now = Instant::now();
        self.emit_contention_recovery_if_ready(now);
        let mut window = self
            .inner
            .workload
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let metrics = window.operations.entry(operation).or_default();
        metrics.calls = metrics.calls.saturating_add(1);
        metrics.deferred = metrics
            .deferred
            .saturating_add(u64::from(deferred.is_some()));
        if let Some(reason) = deferred {
            *metrics.deferred_by_reason.entry(reason).or_default() += 1;
        }
        metrics.errors = metrics.errors.saturating_add(u64::from(error));
        metrics.discarded_connections = metrics
            .discarded_connections
            .saturating_add(u64::from(discarded));
        metrics.pool_wait_ms = metrics
            .pool_wait_ms
            .saturating_add(pool_wait.as_millis() as u64);
        metrics.begin_wait_ms = metrics
            .begin_wait_ms
            .saturating_add(begin_wait.as_millis() as u64);
        metrics.hold_ms = metrics.hold_ms.saturating_add(hold.as_millis() as u64);
        metrics.rows_affected = metrics.rows_affected.saturating_add(rows_affected);
        let idle_connections = self.inner.pool.num_idle().min(u32::MAX as usize) as u32;
        let in_use_connections = self.inner.pool.size().saturating_sub(idle_connections);
        window.minimum_idle_connections = Some(
            window
                .minimum_idle_connections
                .map_or(idle_connections, |minimum| minimum.min(idle_connections)),
        );
        window.maximum_in_use_connections =
            window.maximum_in_use_connections.max(in_use_connections);
        window.maximum_acquire_waiters = window.maximum_acquire_waiters.max(
            self.inner
                .peak_acquire_waiters
                .load(AtomicOrdering::Acquire),
        );
        if now.saturating_duration_since(window.started_at) < SQLITE_WORKLOAD_LOG_INTERVAL {
            return;
        }

        let process_write_bytes = read_process_write_bytes();
        let cgroup_write_bytes = read_cgroup_write_bytes();
        let process_write_bytes_delta =
            monotonic_delta(process_write_bytes, window.last_process_write_bytes);
        let cgroup_write_bytes_delta =
            monotonic_delta(cgroup_write_bytes, window.last_cgroup_write_bytes);
        let top_operations = format_operation_window(&window.operations);
        info!(
            component = "db",
            event = "sqlite_workload_window",
            window_secs = now.saturating_duration_since(window.started_at).as_secs(),
            process_write_bytes_delta = process_write_bytes_delta.unwrap_or_default(),
            cgroup_write_bytes_delta = cgroup_write_bytes_delta.unwrap_or_default(),
            pool_size = self.inner.pool.size(),
            minimum_idle_connections = window.minimum_idle_connections.unwrap_or_default(),
            maximum_in_use_connections = window.maximum_in_use_connections,
            current_acquire_waiters = self.inner.acquire_waiters.load(AtomicOrdering::Acquire),
            peak_acquire_waiters = window.maximum_acquire_waiters,
            top_operations,
            "SQLite workload summary"
        );
        window.started_at = now;
        window.operations.clear();
        window.last_process_write_bytes = process_write_bytes;
        window.last_cgroup_write_bytes = cgroup_write_bytes;
        window.minimum_idle_connections = None;
        window.maximum_in_use_connections = 0;
        window.maximum_acquire_waiters = self.inner.acquire_waiters.load(AtomicOrdering::Acquire);
        self.inner.peak_acquire_waiters.store(
            self.inner.acquire_waiters.load(AtomicOrdering::Acquire),
            AtomicOrdering::Release,
        );
    }

    fn emit_contention_recovery_if_ready(&self, now: Instant) {
        let recovered = {
            let contention = self
                .inner
                .last_contention_at
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            contention.is_some_and(|at| {
                now.saturating_duration_since(at) >= MAINTENANCE_BULK_CONTENTION_COOLDOWN
            })
        };
        if recovered
            && self
                .inner
                .contention_warning_active
                .compare_exchange(true, false, AtomicOrdering::AcqRel, AtomicOrdering::Acquire)
                .is_ok()
        {
            info!(
                component = "db",
                event = "sqlite_runtime_contention_recovered",
                process_write_bytes = read_process_write_bytes().unwrap_or_default(),
                cgroup_write_bytes = read_cgroup_write_bytes().unwrap_or_default(),
                "SQLite workload contention recovered"
            );
        }
    }
}

impl KeyStore {
    pub(crate) fn record_foreground_activity(&self) {
        self.sqlite_runtime.record_foreground_activity();
    }

    pub(crate) fn foreground_activity_rps(&self) -> i64 {
        self.sqlite_runtime.foreground_activity_rps()
    }

    pub(crate) fn foreground_activity_low_pressure_since_floor(&self) -> i64 {
        self.sqlite_runtime
            .foreground_activity_low_pressure_since_floor()
    }

    pub(crate) fn subscribe_dashboard_sse(&self) -> impl Drop {
        self.sqlite_runtime.subscribe_dashboard_sse()
    }
}

#[derive(Debug)]
pub(crate) struct SqliteReadSnapshot {
    conn: Option<sqlx::pool::PoolConnection<Sqlite>>,
    runtime: SqliteRuntime,
    operation: SqliteOperation,
    pool_wait: Duration,
    begin_wait: Duration,
    started_at: Instant,
}

#[derive(Debug)]
pub(crate) struct SqliteOperationConnection {
    conn: Option<sqlx::pool::PoolConnection<Sqlite>>,
    runtime: SqliteRuntime,
    operation: SqliteOperation,
    pool_wait: Duration,
    started_at: Instant,
    restore_busy_timeout: bool,
}

impl SqliteOperationConnection {
    pub(crate) async fn begin_immediate(
        &mut self,
    ) -> Result<SqliteOperationTransaction<'_>, ProxyError> {
        let begin_started = Instant::now();
        let conn = self.conn.take().expect("SQLite operation connection");
        match tokio::time::timeout(
            self.operation.begin_budget(),
            ImmediateSqliteTransaction::begin(conn),
        )
        .await
        {
            Ok(Ok(transaction)) => Ok(SqliteOperationTransaction {
                transaction: Some(transaction),
                connection: self,
            }),
            Ok(Err(err)) => {
                self.runtime.record_error(
                    self.operation,
                    self.pool_wait,
                    begin_started.elapsed(),
                    &err,
                );
                Err(err)
            }
            Err(_) => {
                let err = ProxyError::Database(sqlx::Error::PoolTimedOut);
                self.runtime.record_error(
                    self.operation,
                    self.pool_wait,
                    begin_started.elapsed(),
                    &err,
                );
                Err(err)
            }
        }
    }

    pub(crate) async fn close(mut self) -> Result<(), ProxyError> {
        let Some(conn) = self.conn.take() else {
            // `BEGIN IMMEDIATE` already detached a failed physical connection.
            // There is no pool state left to restore on this error path.
            return Ok(());
        };
        if let Err(err) = restore_operation_connection(conn, self.restore_busy_timeout).await {
            let err = ProxyError::Database(err);
            self.runtime
                .record_error(self.operation, self.pool_wait, Duration::ZERO, &err);
            return Err(err);
        }
        self.runtime.record_success(
            self.operation,
            self.pool_wait,
            Duration::ZERO,
            self.started_at.elapsed(),
            0,
        );
        Ok(())
    }
}

pub(crate) struct SqliteOperationTransaction<'connection> {
    transaction: Option<ImmediateSqliteTransaction>,
    connection: &'connection mut SqliteOperationConnection,
}

impl SqliteOperationTransaction<'_> {
    pub(crate) async fn commit(mut self) -> Result<(), ProxyError> {
        let transaction = self
            .transaction
            .take()
            .expect("SQLite operation transaction");
        self.connection.conn = Some(transaction.commit_connection().await?);
        Ok(())
    }

    pub(crate) async fn rollback(mut self) -> Result<(), ProxyError> {
        let transaction = self
            .transaction
            .take()
            .expect("SQLite operation transaction");
        self.connection.conn = Some(transaction.rollback_connection().await?);
        Ok(())
    }
}

impl Deref for SqliteOperationTransaction<'_> {
    type Target = SqliteConnection;

    fn deref(&self) -> &Self::Target {
        self.transaction
            .as_ref()
            .expect("SQLite operation transaction")
            .deref()
    }
}

impl DerefMut for SqliteOperationTransaction<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.transaction
            .as_mut()
            .expect("SQLite operation transaction")
            .deref_mut()
    }
}

impl Drop for SqliteOperationTransaction<'_> {
    fn drop(&mut self) {
        if self.transaction.is_some() {
            self.connection.runtime.record_discard(
                self.connection.operation,
                self.connection.pool_wait,
                Duration::ZERO,
                self.connection.started_at.elapsed(),
            );
        }
    }
}

impl Deref for SqliteOperationConnection {
    type Target = SqliteConnection;

    fn deref(&self) -> &Self::Target {
        self.conn
            .as_ref()
            .expect("SQLite operation connection")
            .as_ref()
    }
}

impl DerefMut for SqliteOperationConnection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.conn
            .as_mut()
            .expect("SQLite operation connection")
            .as_mut()
    }
}

impl Drop for SqliteOperationConnection {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            drop(conn.detach());
            self.runtime.record_discard(
                self.operation,
                self.pool_wait,
                Duration::ZERO,
                self.started_at.elapsed(),
            );
        }
    }
}

impl SqliteReadSnapshot {
    pub(crate) async fn close(mut self) -> Result<(), ProxyError> {
        let result = sqlx::query("ROLLBACK").execute(&mut *self).await;
        match result {
            Ok(_) => {
                drop(self.conn.take().expect("SQLite read snapshot connection"));
                self.runtime.record_success(
                    self.operation,
                    self.pool_wait,
                    self.begin_wait,
                    self.started_at.elapsed(),
                    0,
                );
                Ok(())
            }
            Err(err) => {
                let conn = self.conn.take().expect("SQLite read snapshot connection");
                conn.detach().close().await.ok();
                let err = ProxyError::Database(err);
                self.runtime
                    .record_error(self.operation, self.pool_wait, self.begin_wait, &err);
                Err(err)
            }
        }
    }
}

impl Deref for SqliteReadSnapshot {
    type Target = SqliteConnection;

    fn deref(&self) -> &Self::Target {
        self.conn
            .as_ref()
            .expect("SQLite read snapshot connection")
            .as_ref()
    }
}

impl DerefMut for SqliteReadSnapshot {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.conn
            .as_mut()
            .expect("SQLite read snapshot connection")
            .as_mut()
    }
}

impl Drop for SqliteReadSnapshot {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            drop(conn.detach());
            self.runtime.record_discard(
                self.operation,
                self.pool_wait,
                self.begin_wait,
                self.started_at.elapsed(),
            );
        }
    }
}

#[derive(Debug)]
pub(crate) struct SqliteImmediateTransaction {
    transaction: Option<ImmediateSqliteTransaction>,
    runtime: SqliteRuntime,
    operation: SqliteOperation,
    pool_wait: Duration,
    begin_wait: Duration,
    started_at: Instant,
    start_total_changes: u64,
    restore_busy_timeout: bool,
}

struct BusyTimeoutResetGuard {
    conn: Option<sqlx::pool::PoolConnection<Sqlite>>,
}

async fn configure_operation_connection(
    conn: sqlx::pool::PoolConnection<Sqlite>,
    operation: SqliteOperation,
) -> Result<(sqlx::pool::PoolConnection<Sqlite>, bool), sqlx::Error> {
    let Some(busy_timeout_ms) = operation.busy_timeout_override_ms() else {
        return Ok((conn, false));
    };
    (BusyTimeoutResetGuard { conn: Some(conn) })
        .configure(busy_timeout_ms)
        .await
        .map(|conn| (conn, true))
}

async fn restore_operation_connection(
    conn: sqlx::pool::PoolConnection<Sqlite>,
    restore_busy_timeout: bool,
) -> Result<(), sqlx::Error> {
    if restore_busy_timeout {
        (BusyTimeoutResetGuard { conn: Some(conn) }).restore().await
    } else {
        drop(conn);
        Ok(())
    }
}

impl BusyTimeoutResetGuard {
    async fn configure(
        mut self,
        busy_timeout_ms: i64,
    ) -> Result<sqlx::pool::PoolConnection<Sqlite>, sqlx::Error> {
        sqlx::query(&format!("PRAGMA busy_timeout = {busy_timeout_ms}"))
            .execute(
                self.conn
                    .as_mut()
                    .expect("SQLite busy-timeout configuration connection")
                    .as_mut(),
            )
            .await?;
        Ok(self
            .conn
            .take()
            .expect("SQLite busy-timeout configuration connection"))
    }

    async fn restore(mut self) -> Result<(), sqlx::Error> {
        sqlx::query(&format!("PRAGMA busy_timeout = {DEFAULT_BUSY_TIMEOUT_MS}"))
            .execute(
                self.conn
                    .as_mut()
                    .expect("SQLite busy-timeout reset connection")
                    .as_mut(),
            )
            .await?;
        drop(
            self.conn
                .take()
                .expect("SQLite busy-timeout reset connection"),
        );
        Ok(())
    }
}

impl Drop for BusyTimeoutResetGuard {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            drop(conn.detach());
        }
    }
}

impl SqliteImmediateTransaction {
    pub(crate) async fn rollback(mut self) -> Result<(), ProxyError> {
        let transaction = self
            .transaction
            .take()
            .expect("SQLite immediate transaction");
        match transaction.rollback_connection().await {
            Ok(conn) => {
                if let Err(err) =
                    restore_operation_connection(conn, self.restore_busy_timeout).await
                {
                    let err = ProxyError::Database(err);
                    self.runtime.record_error(
                        self.operation,
                        self.pool_wait,
                        self.begin_wait,
                        &err,
                    );
                    return Err(err);
                }
                self.runtime.record_success(
                    self.operation,
                    self.pool_wait,
                    self.begin_wait,
                    self.started_at.elapsed(),
                    0,
                );
                Ok(())
            }
            Err(err) => {
                self.runtime
                    .record_error(self.operation, self.pool_wait, self.begin_wait, &err);
                Err(err)
            }
        }
    }

    pub(crate) async fn finish(
        &mut self,
        write_result: Result<(), ProxyError>,
    ) -> Result<(), ProxyError> {
        match write_result {
            Ok(()) => {
                let total_changes = sqlx::query_scalar::<_, i64>("SELECT total_changes()")
                    .fetch_one(&mut **self)
                    .await
                    .unwrap_or_default()
                    .max(0) as u64;
                let rows_affected = total_changes.saturating_sub(self.start_total_changes);
                let transaction = self
                    .transaction
                    .take()
                    .expect("SQLite immediate transaction");
                let conn = match transaction.commit_connection().await {
                    Ok(conn) => conn,
                    Err(err) => {
                        self.runtime.record_error(
                            self.operation,
                            self.pool_wait,
                            self.begin_wait,
                            &err,
                        );
                        return Err(err);
                    }
                };
                if let Err(err) =
                    restore_operation_connection(conn, self.restore_busy_timeout).await
                {
                    let err = ProxyError::Database(err);
                    self.runtime.record_error(
                        self.operation,
                        self.pool_wait,
                        self.begin_wait,
                        &err,
                    );
                    return Err(err);
                }
                self.runtime.record_success(
                    self.operation,
                    self.pool_wait,
                    self.begin_wait,
                    self.started_at.elapsed(),
                    rows_affected,
                );
                Ok(())
            }
            Err(err) => {
                let transaction = self
                    .transaction
                    .take()
                    .expect("SQLite immediate transaction");
                match transaction.rollback_connection().await {
                    Ok(conn) => {
                        if let Err(restore_err) =
                            restore_operation_connection(conn, self.restore_busy_timeout).await
                        {
                            let restore_err = ProxyError::Database(restore_err);
                            self.runtime.record_error(
                                self.operation,
                                self.pool_wait,
                                self.begin_wait,
                                &restore_err,
                            );
                            return Err(restore_err);
                        }
                    }
                    Err(rollback_err) => {
                        self.runtime.record_error(
                            self.operation,
                            self.pool_wait,
                            self.begin_wait,
                            &rollback_err,
                        );
                    }
                }
                self.runtime
                    .record_error(self.operation, self.pool_wait, self.begin_wait, &err);
                Err(err)
            }
        }
    }
}

impl Deref for SqliteImmediateTransaction {
    type Target = SqliteConnection;

    fn deref(&self) -> &Self::Target {
        self.transaction
            .as_ref()
            .expect("SQLite immediate transaction")
            .deref()
    }
}

impl DerefMut for SqliteImmediateTransaction {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.transaction
            .as_mut()
            .expect("SQLite immediate transaction")
            .deref_mut()
    }
}

impl Drop for SqliteImmediateTransaction {
    fn drop(&mut self) {
        if self.transaction.take().is_some() {
            self.runtime.record_discard(
                self.operation,
                self.pool_wait,
                self.begin_wait,
                self.started_at.elapsed(),
            );
        }
    }
}

fn monotonic_delta(current: Option<u64>, previous: Option<u64>) -> Option<u64> {
    current
        .zip(previous)
        .map(|(current, previous)| current.saturating_sub(previous))
}

fn format_operation_window(operations: &BTreeMap<SqliteOperation, OperationWindow>) -> String {
    operations
        .iter()
        .map(|(operation, metrics)| {
            let deferred_reasons = metrics
                .deferred_by_reason
                .iter()
                .map(|(reason, count)| format!("{}={count}", reason.as_str()))
                .collect::<Vec<_>>()
                .join("|");
            format!(
                "{}/{}:calls={},deferred={},defer_reasons={},errors={},retries={},discarded={},pool_wait_ms={},begin_wait_ms={},hold_ms={},rows={}",
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
                metrics.rows_affected,
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

fn read_process_write_bytes() -> Option<u64> {
    parse_process_write_bytes(&std::fs::read_to_string("/proc/self/io").ok()?)
}

fn parse_process_write_bytes(content: &str) -> Option<u64> {
    content
        .lines()
        .find_map(|line| line.strip_prefix("write_bytes:")?.trim().parse().ok())
}

fn read_cgroup_write_bytes() -> Option<u64> {
    let content = std::fs::read_to_string("/sys/fs/cgroup/io.stat").ok()?;
    parse_cgroup_write_bytes(&content)
}

fn parse_cgroup_write_bytes(content: &str) -> Option<u64> {
    let mut total = 0_u64;
    let mut found = false;
    for field in content.split_whitespace() {
        if let Some(value) = field.strip_prefix("wbytes=") {
            total = total.saturating_add(value.parse::<u64>().ok()?);
            found = true;
        }
    }
    found.then_some(total)
}

#[cfg(test)]
mod tests {
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
                    || relative == "src/forward_proxy/tests.rs"
                    || relative.starts_with("src/bin/")
                    || matches!(
                        relative.as_str(),
                        "src/store/sqlite_runtime.rs"
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
    async fn cancelled_immediate_transaction_never_returns_an_open_transaction_to_pool() {
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

        let next = tokio::time::timeout(
            Duration::from_secs(1),
            runtime.begin_immediate(SqliteOperation::DashboardIntegrityWrite),
        )
        .await
        .expect("pool should replace discarded connection")
        .expect("next immediate transaction");
        next.rollback().await.expect("rollback");
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

        let foreground =
            tokio::time::timeout(Duration::from_millis(250), runtime.inner.pool.acquire())
                .await
                .expect("foreground request must not wait behind deferred bulk work")
                .expect("foreground pool acquisition");
        drop(foreground);
        drop(first);
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
            runtime.begin_immediate(SqliteOperation::ScheduledJobControl),
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
}
