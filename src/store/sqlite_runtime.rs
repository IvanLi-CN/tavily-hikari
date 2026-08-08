use super::{ImmediateSqliteTransaction, ProxyError, is_transient_sqlite_write_error};
use sqlx::{Connection, Sqlite, SqliteConnection, SqlitePool};
use std::collections::BTreeMap;
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

const SQLITE_WORKLOAD_LOG_INTERVAL: Duration = Duration::from_secs(60);
const DEFAULT_BUSY_TIMEOUT_MS: i64 = 5_000;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum SqliteOperation {
    BillingLedgerAuditRead,
    DashboardIntegrityWrite,
    HaBaselineRead,
    HaEventsRead,
}

impl SqliteOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::BillingLedgerAuditRead => "billing_ledger_audit_read",
            Self::DashboardIntegrityWrite => "dashboard_integrity_write",
            Self::HaBaselineRead => "ha_baseline_read",
            Self::HaEventsRead => "ha_events_read",
        }
    }

    fn workload_class(self) -> &'static str {
        match self {
            Self::BillingLedgerAuditRead | Self::HaBaselineRead | Self::HaEventsRead => {
                "maintenance_read"
            }
            Self::DashboardIntegrityWrite => "maintenance_write",
        }
    }

    fn acquire_budget(self) -> Duration {
        match self {
            Self::DashboardIntegrityWrite => Duration::from_millis(100),
            _ => Duration::from_secs(5),
        }
    }

    fn busy_timeout_ms(self) -> i64 {
        match self {
            Self::DashboardIntegrityWrite => 100,
            _ => DEFAULT_BUSY_TIMEOUT_MS,
        }
    }
}

impl fmt::Display for SqliteOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct OperationWindow {
    calls: u64,
    errors: u64,
    retries: u64,
    discarded_connections: u64,
    pool_wait_ms: u64,
    begin_wait_ms: u64,
    hold_ms: u64,
    rows_affected: u64,
}

#[derive(Debug)]
struct WorkloadWindow {
    started_at: Instant,
    operations: BTreeMap<SqliteOperation, OperationWindow>,
    last_process_write_bytes: Option<u64>,
    last_cgroup_write_bytes: Option<u64>,
}

impl Default for WorkloadWindow {
    fn default() -> Self {
        Self {
            started_at: Instant::now(),
            operations: BTreeMap::new(),
            last_process_write_bytes: None,
            last_cgroup_write_bytes: None,
        }
    }
}

#[derive(Debug)]
struct SqliteRuntimeInner {
    pool: SqlitePool,
    workload: Mutex<WorkloadWindow>,
}

#[derive(Clone, Debug)]
pub(crate) struct SqliteRuntime {
    inner: Arc<SqliteRuntimeInner>,
}

impl SqliteRuntime {
    pub(crate) fn new(pool: SqlitePool) -> Self {
        Self {
            inner: Arc::new(SqliteRuntimeInner {
                pool,
                workload: Mutex::new(WorkloadWindow::default()),
            }),
        }
    }

    pub(crate) async fn begin_read_snapshot(
        &self,
        operation: SqliteOperation,
    ) -> Result<SqliteReadSnapshot, ProxyError> {
        let acquire_started = Instant::now();
        let conn = match tokio::time::timeout(operation.acquire_budget(), self.inner.pool.acquire())
            .await
        {
            Ok(Ok(conn)) => conn,
            Ok(Err(err)) => {
                let err = ProxyError::Database(err);
                self.record_error(operation, acquire_started.elapsed(), Duration::ZERO, &err);
                return Err(err);
            }
            Err(_) => {
                let err = ProxyError::Other(format!("{operation} pool acquisition timed out"));
                self.record_error(operation, acquire_started.elapsed(), Duration::ZERO, &err);
                return Err(err);
            }
        };
        let pool_wait = acquire_started.elapsed();
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
        let acquire_started = Instant::now();
        let conn = match tokio::time::timeout(operation.acquire_budget(), self.inner.pool.acquire())
            .await
        {
            Ok(Ok(conn)) => conn,
            Ok(Err(err)) => {
                let err = ProxyError::Database(err);
                self.record_error(operation, acquire_started.elapsed(), Duration::ZERO, &err);
                return Err(err);
            }
            Err(_) => {
                let err = ProxyError::Other(format!("{operation} pool acquisition timed out"));
                self.record_error(operation, acquire_started.elapsed(), Duration::ZERO, &err);
                return Err(err);
            }
        };
        let pool_wait = acquire_started.elapsed();
        let conn = match (BusyTimeoutResetGuard { conn: Some(conn) })
            .configure(operation.busy_timeout_ms())
            .await
        {
            Ok(conn) => conn,
            Err(err) => {
                let err = ProxyError::Database(err);
                self.record_error(operation, pool_wait, Duration::ZERO, &err);
                return Err(err);
            }
        };
        let begin_started = Instant::now();
        let mut transaction = match ImmediateSqliteTransaction::begin(conn).await {
            Ok(transaction) => transaction,
            Err(err) => {
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
        })
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
        );
    }

    fn record_error(
        &self,
        operation: SqliteOperation,
        pool_wait: Duration,
        begin_wait: Duration,
        err: &ProxyError,
    ) {
        let process_write_bytes = read_process_write_bytes();
        let cgroup_write_bytes = read_cgroup_write_bytes();
        error!(
            component = "db",
            event = "sqlite_runtime_error",
            operation = operation.as_str(),
            workload_class = operation.workload_class(),
            pool_wait_ms = pool_wait.as_millis() as u64,
            begin_wait_ms = begin_wait.as_millis() as u64,
            error_category = if is_transient_sqlite_write_error(err) {
                "sqlite_busy"
            } else {
                "database"
            },
            process_write_bytes = process_write_bytes.unwrap_or_default(),
            cgroup_write_bytes = cgroup_write_bytes.unwrap_or_default(),
            "sqlite runtime operation failed"
        );
        self.record(
            operation,
            pool_wait,
            begin_wait,
            Duration::ZERO,
            0,
            true,
            false,
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
        self.record(operation, pool_wait, begin_wait, hold, 0, false, true);
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
    ) {
        let now = Instant::now();
        let mut window = self
            .inner
            .workload
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let metrics = window.operations.entry(operation).or_default();
        metrics.calls = metrics.calls.saturating_add(1);
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
            top_operations,
            "SQLite workload summary"
        );
        window.started_at = now;
        window.operations.clear();
        window.last_process_write_bytes = process_write_bytes;
        window.last_cgroup_write_bytes = cgroup_write_bytes;
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
}

struct BusyTimeoutResetGuard {
    conn: Option<sqlx::pool::PoolConnection<Sqlite>>,
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
    #[cfg(test)]
    pub(crate) async fn rollback(mut self) -> Result<(), ProxyError> {
        let transaction = self
            .transaction
            .take()
            .expect("SQLite immediate transaction");
        let result = transaction.rollback().await;
        match result {
            Ok(()) => {
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
                if let Err(err) = (BusyTimeoutResetGuard { conn: Some(conn) }).restore().await {
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
                if let Err(rollback_err) = transaction.rollback().await {
                    self.runtime.record_error(
                        self.operation,
                        self.pool_wait,
                        self.begin_wait,
                        &rollback_err,
                    );
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
            format!(
                "{}/{}:calls={},errors={},retries={},discarded={},pool_wait_ms={},begin_wait_ms={},hold_ms={},rows={}",
                operation.workload_class(),
                operation.as_str(),
                metrics.calls,
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
        SqliteRuntime::new(pool)
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
        let metrics = window.operations[&SqliteOperation::HaEventsRead];
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
}
