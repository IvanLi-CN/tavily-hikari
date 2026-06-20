use crate::analysis::*;
use crate::backend_time::BackendTime;
use crate::models::*;
use crate::*;
use sqlx::ConnectOptions;
use sqlx::Connection;
use sqlx::Row;
use sqlx::SqliteConnection;
use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use tracing::log::LevelFilter;
use tracing::{error, warn};

pub(crate) struct ObservabilityOfflineGuard {
    _lock_file: File,
}

pub(crate) const SQLITE_SLOW_STATEMENT_THRESHOLD: Duration = Duration::from_millis(250);
pub(crate) const SQLITE_SLOW_OPERATION_THRESHOLD: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservabilityOfflineProbe {
    pub sibling_lock_path: String,
    pub service_lock_held_exclusively: bool,
    pub sqlite_write_probe_ok: bool,
}

pub(crate) fn sqlite_lock_sidecar_path(database_path: &str) -> String {
    sqlite_sidecar_path(database_path, "observability-migrate.lock")
}

fn flock_nonblocking(file: &File, operation: libc::c_int) -> std::io::Result<()> {
    let rc = unsafe { libc::flock(file.as_raw_fd(), operation | libc::LOCK_NB) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn open_observability_lock_file(lock_path: &str, create_if_missing: bool) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    if create_if_missing {
        options.create(true).truncate(false);
    }
    options.open(lock_path)
}

pub(crate) fn acquire_observability_service_shared_lock(
    database_path: &str,
) -> Result<File, ProxyError> {
    let lock_path = sqlite_lock_sidecar_path(database_path);
    let file = open_observability_lock_file(&lock_path, true).map_err(|err| {
        ProxyError::Other(format!(
            "failed to open observability service lock file {lock_path}: {err}"
        ))
    })?;
    flock_nonblocking(&file, libc::LOCK_SH).map_err(|err| {
        ProxyError::Other(format!(
            "failed to acquire shared observability service lock {lock_path}: {err}"
        ))
    })?;
    Ok(file)
}

pub(crate) fn acquire_observability_offline_guard(
    database_path: &str,
) -> Result<ObservabilityOfflineGuard, ProxyError> {
    let lock_path = sqlite_lock_sidecar_path(database_path);
    let lock_file = open_observability_lock_file(&lock_path, true).map_err(|err| {
        ProxyError::Other(format!(
            "failed to open observability offline lock file {lock_path}: {err}"
        ))
    })?;
    flock_nonblocking(&lock_file, libc::LOCK_EX).map_err(|err| {
        ProxyError::Other(format!(
            "offline migration requires the service to be stopped; could not acquire exclusive observability service lock {lock_path}: {err}"
        ))
    })?;
    Ok(ObservabilityOfflineGuard {
        _lock_file: lock_file,
    })
}

pub(crate) fn is_transient_sqlite_write_error(err: &ProxyError) -> bool {
    let ProxyError::Database(db_err) = err else {
        return false;
    };
    let sqlx::Error::Database(db_err) = db_err else {
        return false;
    };

    if let Some(code) = db_err.code() {
        match code.as_ref() {
            // SQLite primary and extended codes for lock/busy states.
            "5" | "6" | "261" | "262" | "517" | "518" | "SQLITE_BUSY" | "SQLITE_LOCKED" => {
                return true;
            }
            _ => {}
        }
    }

    let message = db_err.message().to_ascii_lowercase();
    message.contains("database is locked")
        || message.contains("database table is locked")
        || message.contains("database schema is locked")
        || message.contains("database is busy")
}

pub(crate) fn sqlite_transient_write_retry_delay(attempt: usize) -> Duration {
    const BACKOFF_MS: [u64; 5] = [20, 50, 100, 200, 500];
    Duration::from_millis(
        BACKOFF_MS
            .get(attempt)
            .copied()
            .unwrap_or(*BACKOFF_MS.last().expect("backoff is non-empty")),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DbLogStatus {
    Slow,
    Error,
}

impl DbLogStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Slow => "slow",
            Self::Error => "error",
        }
    }
}

pub(crate) fn format_db_operation_log(
    status: DbLogStatus,
    operation: &str,
    elapsed: Duration,
    context: Option<&str>,
    err: Option<&str>,
) -> String {
    let mut message = format!(
        "db operation {}: operation={}, elapsed_ms={}",
        status.as_str(),
        operation,
        elapsed.as_millis()
    );
    if let Some(context) = context.map(str::trim).filter(|value| !value.is_empty()) {
        message.push_str(", context=");
        message.push_str(context);
    }
    if let Some(err) = err.map(str::trim).filter(|value| !value.is_empty()) {
        message.push_str(", err=");
        message.push_str(err);
    }
    message
}

pub(crate) fn log_slow_db_operation(operation: &str, elapsed: Duration, context: Option<&str>) {
    if elapsed < SQLITE_SLOW_OPERATION_THRESHOLD {
        return;
    }
    warn!(
        component = "db",
        event = "operation_slow",
        operation,
        elapsed_ms = elapsed.as_millis() as u64,
        context = context.unwrap_or(""),
        "{}",
        format_db_operation_log(DbLogStatus::Slow, operation, elapsed, context, None)
    );
}

pub(crate) fn log_db_operation_error(
    operation: &str,
    elapsed: Duration,
    context: Option<&str>,
    err: &ProxyError,
) {
    error!(
        component = "db",
        event = "operation_error",
        operation,
        elapsed_ms = elapsed.as_millis() as u64,
        context = context.unwrap_or(""),
        err = %err,
        "{}",
        format_db_operation_log(
            DbLogStatus::Error,
            operation,
            elapsed,
            context,
            Some(&err.to_string()),
        )
    );
}

pub(crate) async fn instrument_db_operation<T, F>(
    operation: &str,
    context: Option<&str>,
    future: F,
) -> Result<T, ProxyError>
where
    F: std::future::Future<Output = Result<T, ProxyError>>,
{
    let started_at = Instant::now();
    match future.await {
        Ok(value) => {
            log_slow_db_operation(operation, started_at.elapsed(), context);
            Ok(value)
        }
        Err(err) => {
            log_db_operation_error(operation, started_at.elapsed(), context, &err);
            Err(err)
        }
    }
}

pub(crate) fn sqlite_runtime_log_context(
    database_path: &str,
    mode: &str,
    read_only: bool,
    create_if_missing: bool,
) -> String {
    format!(
        "path={}, mode={}, read_only={}, create_if_missing={}",
        database_path, mode, read_only, create_if_missing
    )
}

pub(crate) async fn sleep_before_sqlite_transient_write_retry(
    backend_time: &BackendTime,
    operation: &str,
    attempt: usize,
    deadline: Instant,
    err: &ProxyError,
) -> bool {
    if !is_transient_sqlite_write_error(err) {
        return false;
    }

    let now = backend_time.instant_now();
    if now >= deadline {
        return false;
    }

    let remaining = deadline.saturating_duration_since(now);
    let backoff = sqlite_transient_write_retry_delay(attempt).min(remaining);
    warn!(
        component = "db",
        event = "sqlite_transient_write_retry",
        operation,
        attempt = attempt + 1,
        backoff_ms = backoff.as_millis() as u64,
        err = %err,
        "{operation}: transient sqlite write error (attempt={}, backoff={}ms): {err}",
        attempt + 1,
        backoff.as_millis(),
    );
    backend_time.sleep(backoff).await;
    true
}

pub(crate) fn is_invalid_current_month_billing_subject_error(err: &ProxyError) -> bool {
    match err {
        ProxyError::QuotaDataMissing { reason } => {
            reason.contains("charged auth_token_logs rows with invalid billing_subject")
        }
        _ => false,
    }
}

fn add_summary_window_metrics(target: &mut SummaryWindowMetrics, delta: &SummaryWindowMetrics) {
    target.total_requests += delta.total_requests;
    target.success_count += delta.success_count;
    target.error_count += delta.error_count;
    target.quota_exhausted_count += delta.quota_exhausted_count;
    target.valuable_success_count += delta.valuable_success_count;
    target.valuable_failure_count += delta.valuable_failure_count;
    target.other_success_count += delta.other_success_count;
    target.other_failure_count += delta.other_failure_count;
    target.unknown_count += delta.unknown_count;
    target.upstream_exhausted_key_count += delta.upstream_exhausted_key_count;
    target.new_keys += delta.new_keys;
    target.new_quarantines += delta.new_quarantines;
    target.quota_charge.local_estimated_credits += delta.quota_charge.local_estimated_credits;
}

fn summary_window_metrics_from_dashboard_counts(
    counts: DashboardRequestRollupCounts,
) -> SummaryWindowMetrics {
    SummaryWindowMetrics {
        total_requests: counts.total_requests,
        success_count: counts.success_count,
        error_count: counts.error_count,
        quota_exhausted_count: counts.quota_exhausted_count,
        valuable_success_count: counts.valuable_success_count,
        valuable_failure_count: counts.valuable_failure_count,
        other_success_count: counts.other_success_count,
        other_failure_count: counts.other_failure_count,
        unknown_count: counts.unknown_count,
        upstream_exhausted_key_count: 0,
        new_keys: 0,
        new_quarantines: 0,
        quota_charge: SummaryQuotaCharge {
            local_estimated_credits: counts.local_estimated_credits,
            ..SummaryQuotaCharge::default()
        },
    }
}

fn subtract_nonnegative(total: i64, subtract: i64) -> i64 {
    total.saturating_sub(subtract).max(0)
}

fn subtract_summary_window_metrics(
    total: &SummaryWindowMetrics,
    subtract: &SummaryWindowMetrics,
) -> SummaryWindowMetrics {
    SummaryWindowMetrics {
        total_requests: subtract_nonnegative(total.total_requests, subtract.total_requests),
        success_count: subtract_nonnegative(total.success_count, subtract.success_count),
        error_count: subtract_nonnegative(total.error_count, subtract.error_count),
        quota_exhausted_count: subtract_nonnegative(
            total.quota_exhausted_count,
            subtract.quota_exhausted_count,
        ),
        valuable_success_count: subtract_nonnegative(
            total.valuable_success_count,
            subtract.valuable_success_count,
        ),
        valuable_failure_count: subtract_nonnegative(
            total.valuable_failure_count,
            subtract.valuable_failure_count,
        ),
        other_success_count: subtract_nonnegative(
            total.other_success_count,
            subtract.other_success_count,
        ),
        other_failure_count: subtract_nonnegative(
            total.other_failure_count,
            subtract.other_failure_count,
        ),
        unknown_count: subtract_nonnegative(total.unknown_count, subtract.unknown_count),
        upstream_exhausted_key_count: subtract_nonnegative(
            total.upstream_exhausted_key_count,
            subtract.upstream_exhausted_key_count,
        ),
        new_keys: subtract_nonnegative(total.new_keys, subtract.new_keys),
        new_quarantines: subtract_nonnegative(total.new_quarantines, subtract.new_quarantines),
        quota_charge: SummaryQuotaCharge {
            local_estimated_credits: subtract_nonnegative(
                total.quota_charge.local_estimated_credits,
                subtract.quota_charge.local_estimated_credits,
            ),
            ..SummaryQuotaCharge::default()
        },
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DashboardRequestRollupCounts {
    total_requests: i64,
    success_count: i64,
    error_count: i64,
    quota_exhausted_count: i64,
    valuable_success_count: i64,
    valuable_failure_count: i64,
    valuable_failure_429_count: i64,
    other_success_count: i64,
    other_failure_count: i64,
    unknown_count: i64,
    mcp_non_billable: i64,
    mcp_billable: i64,
    api_non_billable: i64,
    api_billable: i64,
    local_estimated_credits: i64,
}

impl DashboardRequestRollupCounts {
    fn add(&mut self, delta: Self) {
        self.total_requests += delta.total_requests;
        self.success_count += delta.success_count;
        self.error_count += delta.error_count;
        self.quota_exhausted_count += delta.quota_exhausted_count;
        self.valuable_success_count += delta.valuable_success_count;
        self.valuable_failure_count += delta.valuable_failure_count;
        self.valuable_failure_429_count += delta.valuable_failure_429_count;
        self.other_success_count += delta.other_success_count;
        self.other_failure_count += delta.other_failure_count;
        self.unknown_count += delta.unknown_count;
        self.mcp_non_billable += delta.mcp_non_billable;
        self.mcp_billable += delta.mcp_billable;
        self.api_non_billable += delta.api_non_billable;
        self.api_billable += delta.api_billable;
        self.local_estimated_credits += delta.local_estimated_credits;
    }
}

pub(crate) async fn open_sqlite_pool(
    database_path: &str,
    create_if_missing: bool,
    read_only: bool,
) -> Result<SqlitePool, ProxyError> {
    let layout = SqliteDatabaseLayout::from_database_path(database_path);
    open_sqlite_pool_with_observability(
        &layout.core_database_path,
        layout.observability_database_path.as_deref(),
        create_if_missing,
        read_only,
    )
    .await
}

pub(crate) async fn open_sqlite_pool_with_observability(
    database_path: &str,
    observability_database_path: Option<&str>,
    create_if_missing: bool,
    read_only: bool,
) -> Result<SqlitePool, ProxyError> {
    let mut options = SqliteConnectOptions::new()
        .filename(database_path)
        .create_if_missing(create_if_missing)
        .read_only(read_only)
        .busy_timeout(Duration::from_secs(5))
        .log_slow_statements(LevelFilter::Warn, SQLITE_SLOW_STATEMENT_THRESHOLD);
    if !read_only {
        options = options.journal_mode(SqliteJournalMode::Wal);
    }

    let attach_context =
        sqlite_runtime_log_context(database_path, "runtime", read_only, create_if_missing);
    let attach_plan = instrument_db_operation(
        "sqlite resolve observability attach plan",
        Some(attach_context.as_str()),
        resolve_observability_attach_plan(
            database_path,
            observability_database_path,
            create_if_missing,
            read_only,
            SQLITE_POOL_MAX_CONNECTIONS_DEFAULT,
        ),
    )
    .await?;
    let mut pool_options = SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(attach_plan.max_connections);
    if let Some(observability_database_path) = attach_plan.target_path {
        pool_options = pool_options.after_connect(move |conn, _meta| {
            let observability_database_path = observability_database_path.clone();
            Box::pin(async move {
                attach_observability_database(conn, &observability_database_path).await?;
                Ok(())
            })
        });
    }

    let open_context =
        sqlite_runtime_log_context(database_path, "runtime", read_only, create_if_missing);
    instrument_db_operation(
        "sqlite open pool",
        Some(open_context.as_str()),
        async move {
            pool_options
                .connect_with(options)
                .await
                .map_err(ProxyError::Database)
        },
    )
    .await
}

pub(crate) const LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES: u64 = 32 * 1024 * 1024;

struct ObservabilityAttachPlan {
    target_path: Option<String>,
    max_connections: u32,
}

async fn resolve_observability_attach_plan(
    core_database_path: &str,
    observability_database_path: Option<&str>,
    create_if_missing: bool,
    read_only: bool,
    default_max_connections: u32,
) -> Result<ObservabilityAttachPlan, ProxyError> {
    let Some(observability_database_path) = observability_database_path else {
        return Ok(ObservabilityAttachPlan {
            target_path: None,
            max_connections: default_max_connections,
        });
    };

    let mut options = SqliteConnectOptions::new()
        .filename(core_database_path)
        .create_if_missing(create_if_missing)
        .read_only(read_only)
        .busy_timeout(Duration::from_secs(5))
        .log_slow_statements(LevelFilter::Warn, SQLITE_SLOW_STATEMENT_THRESHOLD);
    if !read_only {
        options = options.journal_mode(SqliteJournalMode::Wal);
    }

    let probe_context = sqlite_runtime_log_context(
        core_database_path,
        "observability-probe",
        read_only,
        create_if_missing,
    );
    let mut probe = instrument_db_operation(
        "sqlite open observability attach probe",
        Some(probe_context.as_str()),
        async {
            SqliteConnection::connect_with(&options)
                .await
                .map_err(ProxyError::Database)
        },
    )
    .await?;
    let select_context = sqlite_runtime_log_context(
        core_database_path,
        "observability-probe",
        read_only,
        create_if_missing,
    );
    let target_path = instrument_db_operation(
        "sqlite select observability attach path",
        Some(select_context.as_str()),
        async {
            select_observability_attach_path(
                &mut probe,
                core_database_path,
                observability_database_path,
                create_if_missing,
                read_only,
            )
            .await
            .map_err(ProxyError::Database)
        },
    )
    .await?;
    Ok(ObservabilityAttachPlan {
        target_path,
        max_connections: default_max_connections,
    })
}

pub(crate) async fn open_sqlite_pool_forced_observability(
    core_database_path: &str,
    observability_database_path: Option<&str>,
    create_if_missing: bool,
    read_only: bool,
    max_connections: u32,
) -> Result<SqlitePool, ProxyError> {
    let mut options = SqliteConnectOptions::new()
        .filename(core_database_path)
        .create_if_missing(create_if_missing)
        .read_only(read_only)
        .busy_timeout(Duration::from_secs(5))
        .log_slow_statements(LevelFilter::Warn, SQLITE_SLOW_STATEMENT_THRESHOLD);
    if !read_only {
        options = options.journal_mode(SqliteJournalMode::Wal);
    }

    let mut pool_options = SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(max_connections);
    if let Some(observability_database_path) = observability_database_path {
        let observability_database_path = observability_database_path.to_string();
        pool_options = pool_options.after_connect(move |conn, _meta| {
            let observability_database_path = observability_database_path.clone();
            Box::pin(async move {
                attach_observability_database(conn, &observability_database_path).await?;
                Ok(())
            })
        });
    }

    let forced_context = sqlite_runtime_log_context(
        core_database_path,
        "forced-observability",
        read_only,
        create_if_missing,
    );
    instrument_db_operation(
        "sqlite open forced observability pool",
        Some(forced_context.as_str()),
        async move {
            pool_options
                .connect_with(options)
                .await
                .map_err(ProxyError::Database)
        },
    )
    .await
}

async fn select_observability_attach_path(
    conn: &mut sqlx::SqliteConnection,
    core_database_path: &str,
    observability_database_path: &str,
    create_if_missing: bool,
    read_only: bool,
) -> Result<Option<String>, sqlx::Error> {
    let legacy_request_logs_exists = connection_main_table_exists(conn, "request_logs").await?;
    Ok(planned_observability_attach_path(
        core_database_path,
        Some(observability_database_path),
        legacy_request_logs_exists,
        create_if_missing,
        read_only,
    ))
}

pub(crate) fn planned_observability_attach_path(
    core_database_path: &str,
    observability_database_path: Option<&str>,
    legacy_request_logs_exists: bool,
    create_if_missing: bool,
    read_only: bool,
) -> Option<String> {
    let observability_database_path = observability_database_path?;
    let sidecar_exists = std::path::Path::new(observability_database_path).exists();
    if legacy_request_logs_exists
        && (read_only || !legacy_request_logs_inline_sidecar_migration_allowed(core_database_path))
    {
        return Some(core_database_path.to_string());
    }

    if !read_only || create_if_missing || sidecar_exists {
        return Some(observability_database_path.to_string());
    }

    None
}

async fn connection_main_table_exists(
    conn: &mut sqlx::SqliteConnection,
    table: &str,
) -> Result<bool, sqlx::Error> {
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ? LIMIT 1",
    )
    .bind(table)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(exists.is_some())
}

fn legacy_request_logs_inline_sidecar_migration_allowed(database_path: &str) -> bool {
    match std::fs::metadata(database_path) {
        Ok(metadata) => metadata.len() <= LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES,
        Err(err) => {
            eprintln!(
                "observability startup migration: failed to read core database size for {database_path}: {err}"
            );
            false
        }
    }
}

pub(crate) fn core_database_file_size(database_path: &str) -> Result<u64, ProxyError> {
    std::fs::metadata(database_path)
        .map(|metadata| metadata.len())
        .map_err(|err| {
            ProxyError::Other(format!(
                "failed to stat core database {database_path}: {err}"
            ))
        })
}

pub(crate) fn available_disk_bytes_for_path(path: &str) -> Result<u64, ProxyError> {
    let output = std::process::Command::new("df")
        .arg("-k")
        .arg(path)
        .output()
        .map_err(|err| ProxyError::Other(format!("failed to execute df for {path}: {err}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ProxyError::Other(format!(
            "df -k {path} failed with status {}: {}",
            output.status,
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| ProxyError::Other(format!("df output for {path} was empty")))?;
    let available_kib = line
        .split_whitespace()
        .nth(3)
        .ok_or_else(|| ProxyError::Other(format!("unable to parse df output for {path}: {line}")))?
        .parse::<u64>()
        .map_err(|err| {
            ProxyError::Other(format!(
                "unable to parse df available bytes for {path}: {err}"
            ))
        })?;
    Ok(available_kib.saturating_mul(1024))
}

pub(crate) async fn probe_observability_offline_state(
    database_path: &str,
) -> Result<ObservabilityOfflineProbe, ProxyError> {
    let sibling_lock_path = sqlite_lock_sidecar_path(database_path);
    let service_lock_held_exclusively = match open_observability_lock_file(
        &sibling_lock_path,
        false,
    ) {
        Ok(lock_file) => match flock_nonblocking(&lock_file, libc::LOCK_EX) {
            Ok(()) => true,
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => false,
            Err(err) => {
                return Err(ProxyError::Other(format!(
                    "failed to probe observability offline lock file {sibling_lock_path}: {err}"
                )));
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
        Err(err) => {
            return Err(ProxyError::Other(format!(
                "failed to open observability offline probe lock file {sibling_lock_path}: {err}"
            )));
        }
    };
    let sqlite_write_probe_ok = OpenOptions::new()
        .read(true)
        .write(true)
        .open(database_path)
        .is_ok();
    Ok(ObservabilityOfflineProbe {
        sibling_lock_path,
        service_lock_held_exclusively,
        sqlite_write_probe_ok,
    })
}

async fn attach_observability_database(
    conn: &mut sqlx::SqliteConnection,
    database_path: &str,
) -> Result<(), sqlx::Error> {
    let attach_sql = format!(
        "ATTACH DATABASE '{}' AS observability",
        database_path.replace('\'', "''")
    );
    conn.execute(attach_sql.as_str()).await?;
    Ok(())
}

pub(crate) async fn attached_database_path(
    pool: &SqlitePool,
    name: &str,
) -> Result<Option<String>, ProxyError> {
    let path = sqlx::query_scalar::<_, String>(
        "SELECT file FROM pragma_database_list WHERE name = ? LIMIT 1",
    )
    .bind(name)
    .fetch_optional(pool)
    .await?;
    Ok(path.filter(|value| !value.is_empty()))
}

pub(crate) fn sqlite_paths_match(lhs: &str, rhs: &str) -> bool {
    let lhs_path = std::path::Path::new(lhs);
    let rhs_path = std::path::Path::new(rhs);
    match (lhs_path.canonicalize(), rhs_path.canonicalize()) {
        (Ok(lhs), Ok(rhs)) => lhs == rhs,
        _ => lhs == rhs,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SqliteDatabaseLayout {
    pub(crate) core_database_path: String,
    pub(crate) observability_database_path: Option<String>,
}

impl SqliteDatabaseLayout {
    pub(crate) fn from_database_path(database_path: &str) -> Self {
        let database_path = database_path.trim();
        let path = std::path::Path::new(database_path);
        let is_explicit_sqlite_file = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("db"))
            .unwrap_or(false);
        if is_explicit_sqlite_file {
            return Self {
                core_database_path: database_path.to_string(),
                observability_database_path: Some(sqlite_sidecar_path(
                    database_path,
                    "observability.db",
                )),
            };
        }

        let trimmed = database_path.trim_end_matches(std::path::MAIN_SEPARATOR);
        let base = if trimmed.is_empty() {
            database_path
        } else {
            trimmed
        };
        Self {
            core_database_path: format!("{}{}core.db", base, std::path::MAIN_SEPARATOR),
            observability_database_path: Some(format!(
                "{}{}observability.db",
                base,
                std::path::MAIN_SEPARATOR
            )),
        }
    }
}

fn sqlite_sidecar_path(database_path: &str, file_name: &str) -> String {
    let path = std::path::Path::new(database_path);
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("sqlite");
    let sidecar_name = if let Some((base, ext)) = file_name.rsplit_once('.') {
        format!("{stem}-{base}.{ext}")
    } else {
        format!("{stem}-{file_name}")
    };
    parent.join(sidecar_name).to_string_lossy().to_string()
}

pub(crate) fn is_observability_table(table: &str) -> bool {
    matches!(
        table,
        "request_logs"
            | "api_key_usage_buckets"
            | "dashboard_request_rollup_buckets"
            | "request_log_catalog_rollups"
    )
}

pub(crate) fn sqlite_qualified_table_name(table: &str) -> String {
    if is_observability_table(table) {
        format!("observability.{}", quote_sqlite_identifier(table))
    } else {
        quote_sqlite_identifier(table)
    }
}

pub(crate) async fn begin_immediate_sqlite_connection(
    pool: &SqlitePool,
) -> Result<sqlx::pool::PoolConnection<Sqlite>, ProxyError> {
    instrument_db_operation("sqlite begin immediate", None, async {
        let mut conn = pool.acquire().await?;
        if let Err(err) = sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
            return Err(ProxyError::Database(err));
        }
        Ok(conn)
    })
    .await
}

pub(crate) async fn begin_immediate_sqlite_connection_with_retry(
    pool: &SqlitePool,
    backend_time: &BackendTime,
    operation: &str,
    retry_window: Duration,
) -> Result<sqlx::pool::PoolConnection<Sqlite>, ProxyError> {
    let deadline = backend_time.instant_now() + retry_window;
    let mut attempt = 0;
    loop {
        match begin_immediate_sqlite_connection(pool).await {
            Ok(conn) => return Ok(conn),
            Err(err)
                if sleep_before_sqlite_transient_write_retry(
                    backend_time,
                    operation,
                    attempt,
                    deadline,
                    &err,
                )
                .await =>
            {
                attempt += 1;
            }
            Err(err) => return Err(err),
        }
    }
}

pub(crate) async fn begin_immediate_sqlite_connection_for_monthly_quota_rebase(
    pool: &SqlitePool,
) -> Result<sqlx::pool::PoolConnection<Sqlite>, ProxyError> {
    begin_immediate_sqlite_connection_with_retry(
        pool,
        &BackendTime::system(),
        "rebase_current_month_business_quota_with_pool",
        Duration::from_secs(5),
    )
    .await
}

pub(crate) async fn begin_read_snapshot_sqlite_connection(
    pool: &SqlitePool,
) -> Result<sqlx::pool::PoolConnection<Sqlite>, ProxyError> {
    let mut conn = pool.acquire().await?;
    sqlx::query("BEGIN").execute(&mut *conn).await?;
    Ok(conn)
}

#[derive(Debug, Clone, Copy)]
struct QuotaSyncSampleRow {
    quota_remaining: i64,
    captured_at: i64,
}

#[derive(Debug, Clone, Copy, Default)]
struct QuotaChargeAccumulator {
    upstream_actual_credits: i64,
    sampled_key_count: i64,
    stale_key_count: i64,
    latest_sync_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ApiKeyTransientBackoffState {
    pub(crate) cooldown_until: i64,
    pub(crate) retry_after_secs: i64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ApiKeyTransientBackoffArm<'a> {
    pub(crate) key_id: &'a str,
    pub(crate) scope: &'a str,
    pub(crate) cooldown_until: i64,
    pub(crate) retry_after_secs: i64,
    pub(crate) reason_code: Option<&'a str>,
    pub(crate) source_request_log_id: Option<i64>,
    pub(crate) now: i64,
}

const REQUEST_LOGS_REBUILT_SCHEMA_SQL: &str = r#"
CREATE TABLE observability.request_logs_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    api_key_id TEXT,
    auth_token_id TEXT,
    request_user_id TEXT,
    method TEXT NOT NULL,
    path TEXT NOT NULL,
    query TEXT,
    status_code INTEGER,
    tavily_status_code INTEGER,
    error_message TEXT,
    result_status TEXT NOT NULL DEFAULT 'unknown',
    request_kind_key TEXT,
    request_kind_label TEXT,
    request_kind_detail TEXT,
    counts_business_quota INTEGER,
    business_credits INTEGER,
    failure_kind TEXT,
    key_effect_code TEXT NOT NULL DEFAULT 'none',
    key_effect_summary TEXT,
    binding_effect_code TEXT NOT NULL DEFAULT 'none',
    binding_effect_summary TEXT,
    selection_effect_code TEXT NOT NULL DEFAULT 'none',
    selection_effect_summary TEXT,
    gateway_mode TEXT,
    experiment_variant TEXT,
    proxy_session_id TEXT,
    routing_subject_hash TEXT,
    upstream_operation TEXT,
    fallback_reason TEXT,
    request_body BLOB,
    response_body BLOB,
    request_body_bytes INTEGER,
    response_body_bytes INTEGER,
    request_body_sha256 TEXT,
    response_body_sha256 TEXT,
    body_retention_days INTEGER,
    body_retention_profile TEXT,
    body_cleaned_reason TEXT,
    body_cleaned_at INTEGER,
    forwarded_headers TEXT,
    dropped_headers TEXT,
    remote_addr TEXT,
    client_ip TEXT,
    client_ip_source TEXT,
    client_ip_trusted INTEGER NOT NULL DEFAULT 0,
    ip_headers TEXT,
    visibility TEXT NOT NULL DEFAULT 'visible',
    created_at INTEGER NOT NULL
)
"#;

const AUTH_TOKEN_LOGS_REBUILT_SCHEMA_SQL: &str = r#"
CREATE TABLE auth_token_logs_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    token_id TEXT NOT NULL,
    method TEXT NOT NULL,
    path TEXT NOT NULL,
    query TEXT,
    http_status INTEGER,
    mcp_status INTEGER,
    request_kind_key TEXT,
    request_kind_label TEXT,
    request_kind_detail TEXT,
    result_status TEXT NOT NULL,
    error_message TEXT,
    failure_kind TEXT,
    key_effect_code TEXT NOT NULL DEFAULT 'none',
    key_effect_summary TEXT,
    binding_effect_code TEXT NOT NULL DEFAULT 'none',
    binding_effect_summary TEXT,
    selection_effect_code TEXT NOT NULL DEFAULT 'none',
    selection_effect_summary TEXT,
    gateway_mode TEXT,
    experiment_variant TEXT,
    proxy_session_id TEXT,
    routing_subject_hash TEXT,
    upstream_operation TEXT,
    fallback_reason TEXT,
    counts_business_quota INTEGER NOT NULL DEFAULT 1,
    business_credits INTEGER,
    billing_subject TEXT,
    billing_state TEXT NOT NULL DEFAULT 'none',
    request_user_id TEXT,
    api_key_id TEXT,
    request_log_id INTEGER,
    created_at INTEGER NOT NULL
)
"#;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RequestLogDiagnosticMetadata {
    created_at: Option<i64>,
    request_user_id: Option<String>,
    gateway_mode: Option<String>,
    experiment_variant: Option<String>,
    proxy_session_id: Option<String>,
    routing_subject_hash: Option<String>,
    upstream_operation: Option<String>,
    fallback_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UserBusinessCallEventWrite {
    pub(crate) user_id: String,
    pub(crate) created_at: i64,
    pub(crate) result_status: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestLogsRebuildMode {
    DropLegacyApiKeyColumn,
    RelaxApiKeyIdNullability,
    DropLegacyRequestKindColumns,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthTokenLogsRebuildMode {
    DropLegacyRequestKindColumns,
}

struct RequestLogFilterParams<'a, 'b> {
    request_kinds: &'b [String],
    result_status: Option<&'b str>,
    key_effect_code: Option<&'b str>,
    binding_effect_code: Option<&'b str>,
    selection_effect_code: Option<&'b str>,
    auth_token_id: Option<&'b str>,
    key_id: Option<&'b str>,
    stored_request_kind_sql: &'a str,
    legacy_request_kind_predicate_sql: &'a str,
    legacy_request_kind_sql: &'a str,
    has_where: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct RequestLogsCatalogFilters<'a> {
    pub(crate) request_kinds: &'a [String],
    pub(crate) result_status: Option<&'a str>,
    pub(crate) key_effect_code: Option<&'a str>,
    pub(crate) binding_effect_code: Option<&'a str>,
    pub(crate) selection_effect_code: Option<&'a str>,
    pub(crate) auth_token_id: Option<&'a str>,
    pub(crate) key_id: Option<&'a str>,
    pub(crate) operational_class: Option<&'a str>,
}

#[derive(Clone, Copy)]
pub(crate) struct TokenLogsCatalogFilters<'a> {
    pub(crate) request_kinds: &'a [String],
    pub(crate) result_status: Option<&'a str>,
    pub(crate) key_effect_code: Option<&'a str>,
    pub(crate) binding_effect_code: Option<&'a str>,
    pub(crate) selection_effect_code: Option<&'a str>,
    pub(crate) key_id: Option<&'a str>,
    pub(crate) operational_class: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestKindCanonicalMigrationState {
    Running {
        heartbeat_at: i64,
        owner_pid: Option<u32>,
    },
    Failed(i64),
    Done(i64),
}

impl RequestKindCanonicalMigrationState {
    fn as_meta_value(self) -> String {
        match self {
            Self::Running {
                heartbeat_at,
                owner_pid: Some(owner_pid),
            } => format!("running:{heartbeat_at}:{owner_pid}"),
            Self::Running {
                heartbeat_at,
                owner_pid: None,
            } => format!("running:{heartbeat_at}"),
            Self::Failed(ts) => format!("failed:{ts}"),
            Self::Done(ts) => format!("done:{ts}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestKindCanonicalMigrationClaim {
    Claimed,
    RunningElsewhere(i64),
    AlreadyDone(i64),
    RetryLater,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RequestKindCanonicalBackfillUpperBounds {
    pub(crate) request_logs: i64,
    pub(crate) auth_token_logs: i64,
}

#[derive(Debug, Clone)]
struct RequestKindCanonicalUpdate {
    id: i64,
    request_kind_key: String,
    request_kind_label: String,
    request_kind_detail: Option<String>,
}

#[derive(Debug, Clone)]
struct RequestKindBackfillRequestLogRow {
    id: i64,
    path: String,
    request_body: Option<Vec<u8>>,
    request_kind_key: Option<String>,
    request_kind_label: Option<String>,
    request_kind_detail: Option<String>,
}

#[derive(Debug, Clone)]
struct RequestKindBackfillTokenLogRow {
    id: i64,
    method: String,
    path: String,
    query: Option<String>,
    request_kind_key: Option<String>,
    request_kind_label: Option<String>,
    request_kind_detail: Option<String>,
}

fn normalize_request_kind_backfill_field(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

async fn read_request_kind_backfill_meta_i64(
    pool: &SqlitePool,
    key: &str,
) -> Result<i64, ProxyError> {
    Ok(read_request_kind_backfill_meta_i64_optional(pool, key)
        .await?
        .unwrap_or(0))
}

async fn read_request_kind_backfill_meta_i64_optional(
    pool: &SqlitePool,
    key: &str,
) -> Result<Option<i64>, ProxyError> {
    Ok(
        sqlx::query_scalar::<_, Option<String>>("SELECT value FROM meta WHERE key = ? LIMIT 1")
            .bind(key)
            .fetch_optional(pool)
            .await?
            .flatten()
            .and_then(|value| value.parse::<i64>().ok()),
    )
}

async fn write_request_kind_backfill_meta_i64(
    tx: &mut Transaction<'_, Sqlite>,
    key: &str,
    value: i64,
) -> Result<(), ProxyError> {
    write_request_kind_backfill_meta_string(tx, key, &value.to_string()).await
}

async fn write_request_kind_backfill_meta_string(
    tx: &mut Transaction<'_, Sqlite>,
    key: &str,
    value: &str,
) -> Result<(), ProxyError> {
    sqlx::query(
        r#"
        INSERT INTO meta (key, value)
        VALUES (?, ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
    )
    .bind(key)
    .bind(value)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn parse_request_kind_canonical_migration_state(
    value: Option<String>,
) -> Option<RequestKindCanonicalMigrationState> {
    let value = value?;
    let mut parts = value.split(':');
    let kind = parts.next()?;
    let ts = parts.next()?.parse::<i64>().ok()?;
    match kind {
        "running" => {
            let owner_pid = match parts.next() {
                Some(pid) => Some(pid.parse::<u32>().ok()?),
                None => None,
            };
            if parts.next().is_some() {
                return None;
            }
            Some(RequestKindCanonicalMigrationState::Running {
                heartbeat_at: ts,
                owner_pid,
            })
        }
        "failed" if parts.next().is_none() => Some(RequestKindCanonicalMigrationState::Failed(ts)),
        "done" if parts.next().is_none() => Some(RequestKindCanonicalMigrationState::Done(ts)),
        _ => None,
    }
}

fn request_kind_canonical_migration_is_fresh(now_ts: i64, started_at: i64) -> bool {
    now_ts.saturating_sub(started_at) < REQUEST_KIND_CANONICAL_MIGRATION_STALE_SECS
}

fn current_request_kind_canonical_migration_running_state(
    now_ts: i64,
) -> RequestKindCanonicalMigrationState {
    RequestKindCanonicalMigrationState::Running {
        heartbeat_at: now_ts,
        owner_pid: Some(std::process::id()),
    }
}

#[cfg(unix)]
pub(crate) fn request_kind_canonical_migration_owner_pid_is_live(owner_pid: u32) -> bool {
    let result = unsafe { libc::kill(owner_pid as i32, 0) };
    if result == 0 {
        return true;
    }

    matches!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EPERM)
    )
}

#[cfg(not(unix))]
pub(crate) fn request_kind_canonical_migration_owner_pid_is_live(owner_pid: u32) -> bool {
    let _ = owner_pid;
    true
}

fn request_kind_canonical_migration_state_blocks_reentry(
    now_ts: i64,
    state: RequestKindCanonicalMigrationState,
) -> Option<i64> {
    match state {
        RequestKindCanonicalMigrationState::Running {
            heartbeat_at,
            owner_pid: Some(owner_pid),
        } if request_kind_canonical_migration_is_fresh(now_ts, heartbeat_at)
            && request_kind_canonical_migration_owner_pid_is_live(owner_pid) =>
        {
            Some(heartbeat_at)
        }
        RequestKindCanonicalMigrationState::Running {
            heartbeat_at,
            owner_pid: None,
        } if request_kind_canonical_migration_is_fresh(now_ts, heartbeat_at) => Some(heartbeat_at),
        _ => None,
    }
}

async fn read_meta_string_with_connection(
    conn: &mut sqlx::pool::PoolConnection<Sqlite>,
    key: &str,
) -> Result<Option<String>, ProxyError> {
    sqlx::query_scalar::<_, String>("SELECT value FROM meta WHERE key = ? LIMIT 1")
        .bind(key)
        .fetch_optional(&mut **conn)
        .await
        .map_err(ProxyError::Database)
}

async fn write_meta_string_with_connection(
    conn: &mut sqlx::pool::PoolConnection<Sqlite>,
    key: &str,
    value: &str,
) -> Result<(), ProxyError> {
    sqlx::query(
        r#"
        INSERT INTO meta (key, value)
        VALUES (?, ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
    )
    .bind(key)
    .bind(value)
    .execute(&mut **conn)
    .await?;
    Ok(())
}

async fn read_meta_i64_with_connection(
    conn: &mut sqlx::pool::PoolConnection<Sqlite>,
    key: &str,
) -> Result<Option<i64>, ProxyError> {
    read_meta_string_with_connection(conn, key)
        .await
        .map(|value| value.and_then(|value| value.parse::<i64>().ok()))
}

async fn delete_meta_key_with_connection(
    conn: &mut sqlx::pool::PoolConnection<Sqlite>,
    key: &str,
) -> Result<(), ProxyError> {
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(key)
        .execute(&mut **conn)
        .await?;
    Ok(())
}

async fn read_request_kind_canonical_migration_status(
    pool: &SqlitePool,
) -> Result<Option<RequestKindCanonicalMigrationState>, ProxyError> {
    if let Some(done_at) = read_request_kind_backfill_meta_i64_optional(
        pool,
        META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_DONE,
    )
    .await?
    {
        return Ok(Some(RequestKindCanonicalMigrationState::Done(done_at)));
    }

    Ok(parse_request_kind_canonical_migration_state(
        sqlx::query_scalar::<_, String>("SELECT value FROM meta WHERE key = ? LIMIT 1")
            .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE)
            .fetch_optional(pool)
            .await
            .map_err(ProxyError::Database)?,
    ))
}

async fn read_request_kind_canonical_migration_status_with_connection(
    conn: &mut sqlx::pool::PoolConnection<Sqlite>,
) -> Result<Option<RequestKindCanonicalMigrationState>, ProxyError> {
    if let Some(done_at) =
        read_meta_i64_with_connection(conn, META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_DONE)
            .await?
    {
        return Ok(Some(RequestKindCanonicalMigrationState::Done(done_at)));
    }

    Ok(parse_request_kind_canonical_migration_state(
        read_meta_string_with_connection(conn, META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE)
            .await?,
    ))
}

async fn read_request_kind_canonical_backfill_upper_bounds(
    pool: &SqlitePool,
) -> Result<Option<RequestKindCanonicalBackfillUpperBounds>, ProxyError> {
    let request_logs = read_request_kind_backfill_meta_i64_optional(
        pool,
        META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_REQUEST_LOGS_UPPER_BOUND,
    )
    .await?;
    let auth_token_logs = read_request_kind_backfill_meta_i64_optional(
        pool,
        META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_AUTH_TOKEN_LOGS_UPPER_BOUND,
    )
    .await?;
    Ok(match (request_logs, auth_token_logs) {
        (Some(request_logs), Some(auth_token_logs)) => {
            Some(RequestKindCanonicalBackfillUpperBounds {
                request_logs,
                auth_token_logs,
            })
        }
        _ => None,
    })
}

async fn read_request_kind_canonical_backfill_upper_bounds_with_connection(
    conn: &mut sqlx::pool::PoolConnection<Sqlite>,
) -> Result<Option<RequestKindCanonicalBackfillUpperBounds>, ProxyError> {
    let request_logs = read_meta_i64_with_connection(
        conn,
        META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_REQUEST_LOGS_UPPER_BOUND,
    )
    .await?;
    let auth_token_logs = read_meta_i64_with_connection(
        conn,
        META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_AUTH_TOKEN_LOGS_UPPER_BOUND,
    )
    .await?;
    Ok(match (request_logs, auth_token_logs) {
        (Some(request_logs), Some(auth_token_logs)) => {
            Some(RequestKindCanonicalBackfillUpperBounds {
                request_logs,
                auth_token_logs,
            })
        }
        _ => None,
    })
}

async fn fetch_table_max_id_with_connection(
    conn: &mut sqlx::pool::PoolConnection<Sqlite>,
    table: &str,
) -> Result<i64, ProxyError> {
    let sql = format!("SELECT COALESCE(MAX(id), 0) FROM {table}");
    sqlx::query_scalar::<_, i64>(&sql)
        .fetch_one(&mut **conn)
        .await
        .map_err(ProxyError::Database)
}

async fn capture_request_kind_canonical_backfill_upper_bounds_with_connection(
    conn: &mut sqlx::pool::PoolConnection<Sqlite>,
) -> Result<RequestKindCanonicalBackfillUpperBounds, ProxyError> {
    Ok(RequestKindCanonicalBackfillUpperBounds {
        request_logs: fetch_table_max_id_with_connection(conn, "request_logs").await?,
        auth_token_logs: fetch_table_max_id_with_connection(conn, "auth_token_logs").await?,
    })
}

async fn write_request_kind_canonical_backfill_upper_bounds_with_connection(
    conn: &mut sqlx::pool::PoolConnection<Sqlite>,
    upper_bounds: RequestKindCanonicalBackfillUpperBounds,
) -> Result<(), ProxyError> {
    write_meta_string_with_connection(
        conn,
        META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_REQUEST_LOGS_UPPER_BOUND,
        &upper_bounds.request_logs.to_string(),
    )
    .await?;
    write_meta_string_with_connection(
        conn,
        META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_AUTH_TOKEN_LOGS_UPPER_BOUND,
        &upper_bounds.auth_token_logs.to_string(),
    )
    .await?;
    Ok(())
}

fn build_request_kind_backfill_request_log_update(
    row: RequestKindBackfillRequestLogRow,
) -> Option<RequestKindCanonicalUpdate> {
    let current_key = normalize_request_kind_backfill_field(row.request_kind_key);
    let current_label = normalize_request_kind_backfill_field(row.request_kind_label);
    let current_detail = normalize_request_kind_backfill_field(row.request_kind_detail);
    let kind = canonicalize_request_log_request_kind(
        row.path.as_str(),
        row.request_body.as_deref(),
        current_key.clone(),
        current_label.clone(),
        current_detail.clone(),
    );
    let desired_detail = normalize_request_kind_backfill_field(kind.detail);

    if current_key.as_deref() == Some(kind.key.as_str())
        && current_label.as_deref() == Some(kind.label.as_str())
        && current_detail == desired_detail
    {
        return None;
    }

    Some(RequestKindCanonicalUpdate {
        id: row.id,
        request_kind_key: kind.key,
        request_kind_label: kind.label,
        request_kind_detail: desired_detail,
    })
}

fn build_request_kind_backfill_token_log_update(
    row: RequestKindBackfillTokenLogRow,
) -> Option<RequestKindCanonicalUpdate> {
    let current_key = normalize_request_kind_backfill_field(row.request_kind_key);
    let current_label = normalize_request_kind_backfill_field(row.request_kind_label);
    let current_detail = normalize_request_kind_backfill_field(row.request_kind_detail);
    let kind = finalize_token_request_kind(
        row.method.as_str(),
        row.path.as_str(),
        row.query.as_deref(),
        current_key.clone(),
        current_label.clone(),
        current_detail.clone(),
    );
    let desired_detail = normalize_request_kind_backfill_field(kind.detail);

    if current_key.as_deref() == Some(kind.key.as_str())
        && current_label.as_deref() == Some(kind.label.as_str())
        && current_detail == desired_detail
    {
        return None;
    }

    Some(RequestKindCanonicalUpdate {
        id: row.id,
        request_kind_key: kind.key,
        request_kind_label: kind.label,
        request_kind_detail: desired_detail,
    })
}

async fn backfill_request_log_request_kinds_with_pool(
    pool: &SqlitePool,
    batch_size: i64,
    dry_run: bool,
    migration_state_key: Option<&str>,
    upper_bound_id: Option<i64>,
    backend_time: &BackendTime,
) -> Result<RequestKindCanonicalBackfillTableReport, ProxyError> {
    let cursor_before = read_request_kind_backfill_meta_i64(
        pool,
        META_KEY_REQUEST_KIND_CANONICAL_BACKFILL_REQUEST_LOGS_CURSOR_V1,
    )
    .await?;
    let upper_bound_id = upper_bound_id.unwrap_or(i64::MAX);
    let mut cursor_after = cursor_before;
    let mut rows_scanned = 0_i64;
    let mut rows_updated = 0_i64;

    loop {
        let rows = sqlx::query(
            r#"
            SELECT
                id,
                path,
                request_body,
                request_kind_key,
                request_kind_label,
                request_kind_detail
            FROM request_logs
            WHERE id > ?
              AND id <= ?
            ORDER BY id ASC
            LIMIT ?
            "#,
        )
        .bind(cursor_after)
        .bind(upper_bound_id)
        .bind(batch_size)
        .fetch_all(pool)
        .await?;
        if rows.is_empty() {
            break;
        }

        let parsed_rows = rows
            .into_iter()
            .map(|row| {
                Ok(RequestKindBackfillRequestLogRow {
                    id: row.try_get("id")?,
                    path: row.try_get("path")?,
                    request_body: row.try_get("request_body")?,
                    request_kind_key: row.try_get("request_kind_key")?,
                    request_kind_label: row.try_get("request_kind_label")?,
                    request_kind_detail: row.try_get("request_kind_detail")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;
        let batch_max_id = parsed_rows.last().map(|row| row.id).unwrap_or(cursor_after);
        rows_scanned += parsed_rows.len() as i64;

        let updates = parsed_rows
            .into_iter()
            .filter_map(build_request_kind_backfill_request_log_update)
            .collect::<Vec<_>>();
        rows_updated += updates.len() as i64;

        if !dry_run {
            loop {
                let mut tx = match pool.begin().await {
                    Ok(tx) => tx,
                    Err(err) => {
                        let err = ProxyError::Database(err);
                        if is_transient_sqlite_write_error(&err) {
                            backend_time
                                .sleep(Duration::from_millis(
                                    REQUEST_KIND_CANONICAL_MIGRATION_WAIT_POLL_MS,
                                ))
                                .await;
                            continue;
                        }
                        return Err(err);
                    }
                };

                let batch_result: Result<(), ProxyError> = async {
                    for update in &updates {
                        sqlx::query(
                            r#"
                            UPDATE request_logs
                            SET
                                request_kind_key = ?,
                                request_kind_label = ?,
                                request_kind_detail = ?
                            WHERE id = ?
                            "#,
                        )
                        .bind(&update.request_kind_key)
                        .bind(&update.request_kind_label)
                        .bind(&update.request_kind_detail)
                        .bind(update.id)
                        .execute(&mut *tx)
                        .await?;
                    }
                    write_request_kind_backfill_meta_i64(
                        &mut tx,
                        META_KEY_REQUEST_KIND_CANONICAL_BACKFILL_REQUEST_LOGS_CURSOR_V1,
                        batch_max_id,
                    )
                    .await?;
                    if let Some(migration_state_key) = migration_state_key {
                        write_request_kind_backfill_meta_string(
                            &mut tx,
                            migration_state_key,
                            &current_request_kind_canonical_migration_running_state(
                                backend_time.now_ts(),
                            )
                            .as_meta_value(),
                        )
                        .await?;
                    }
                    Ok(())
                }
                .await;

                match batch_result {
                    Ok(()) => match tx.commit().await {
                        Ok(()) => break,
                        Err(err) => {
                            let err = ProxyError::Database(err);
                            if is_transient_sqlite_write_error(&err) {
                                backend_time
                                    .sleep(Duration::from_millis(
                                        REQUEST_KIND_CANONICAL_MIGRATION_WAIT_POLL_MS,
                                    ))
                                    .await;
                                continue;
                            }
                            return Err(err);
                        }
                    },
                    Err(err) => {
                        let retry = is_transient_sqlite_write_error(&err);
                        let _ = tx.rollback().await;
                        if retry {
                            backend_time
                                .sleep(Duration::from_millis(
                                    REQUEST_KIND_CANONICAL_MIGRATION_WAIT_POLL_MS,
                                ))
                                .await;
                            continue;
                        }
                        return Err(err);
                    }
                }
            }
        }

        cursor_after = if dry_run { cursor_before } else { batch_max_id };
        if dry_run && batch_max_id > cursor_before {
            cursor_after = batch_max_id;
        }
    }

    Ok(RequestKindCanonicalBackfillTableReport {
        table: "request_logs",
        meta_key: META_KEY_REQUEST_KIND_CANONICAL_BACKFILL_REQUEST_LOGS_CURSOR_V1,
        dry_run,
        batch_size,
        cursor_before,
        cursor_after: if dry_run { cursor_before } else { cursor_after },
        rows_scanned,
        rows_updated,
    })
}

async fn backfill_auth_token_log_request_kinds_with_pool(
    pool: &SqlitePool,
    batch_size: i64,
    dry_run: bool,
    migration_state_key: Option<&str>,
    upper_bound_id: Option<i64>,
    backend_time: &BackendTime,
) -> Result<RequestKindCanonicalBackfillTableReport, ProxyError> {
    let cursor_before = read_request_kind_backfill_meta_i64(
        pool,
        META_KEY_REQUEST_KIND_CANONICAL_BACKFILL_AUTH_TOKEN_LOGS_CURSOR_V1,
    )
    .await?;
    let upper_bound_id = upper_bound_id.unwrap_or(i64::MAX);
    let mut cursor_after = cursor_before;
    let mut rows_scanned = 0_i64;
    let mut rows_updated = 0_i64;

    loop {
        let rows = sqlx::query(
            r#"
            SELECT
                id,
                method,
                path,
                query,
                request_kind_key,
                request_kind_label,
                request_kind_detail
            FROM auth_token_logs
            WHERE id > ?
              AND id <= ?
            ORDER BY id ASC
            LIMIT ?
            "#,
        )
        .bind(cursor_after)
        .bind(upper_bound_id)
        .bind(batch_size)
        .fetch_all(pool)
        .await?;
        if rows.is_empty() {
            break;
        }

        let parsed_rows = rows
            .into_iter()
            .map(|row| {
                Ok(RequestKindBackfillTokenLogRow {
                    id: row.try_get("id")?,
                    method: row.try_get("method")?,
                    path: row.try_get("path")?,
                    query: row.try_get("query")?,
                    request_kind_key: row.try_get("request_kind_key")?,
                    request_kind_label: row.try_get("request_kind_label")?,
                    request_kind_detail: row.try_get("request_kind_detail")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;
        let batch_max_id = parsed_rows.last().map(|row| row.id).unwrap_or(cursor_after);
        rows_scanned += parsed_rows.len() as i64;

        let updates = parsed_rows
            .into_iter()
            .filter_map(build_request_kind_backfill_token_log_update)
            .collect::<Vec<_>>();
        rows_updated += updates.len() as i64;

        if !dry_run {
            loop {
                let mut tx = match pool.begin().await {
                    Ok(tx) => tx,
                    Err(err) => {
                        let err = ProxyError::Database(err);
                        if is_transient_sqlite_write_error(&err) {
                            backend_time
                                .sleep(Duration::from_millis(
                                    REQUEST_KIND_CANONICAL_MIGRATION_WAIT_POLL_MS,
                                ))
                                .await;
                            continue;
                        }
                        return Err(err);
                    }
                };

                let batch_result: Result<(), ProxyError> = async {
                    for update in &updates {
                        sqlx::query(
                            r#"
                            UPDATE auth_token_logs
                            SET
                                request_kind_key = ?,
                                request_kind_label = ?,
                                request_kind_detail = ?
                            WHERE id = ?
                            "#,
                        )
                        .bind(&update.request_kind_key)
                        .bind(&update.request_kind_label)
                        .bind(&update.request_kind_detail)
                        .bind(update.id)
                        .execute(&mut *tx)
                        .await?;
                    }
                    write_request_kind_backfill_meta_i64(
                        &mut tx,
                        META_KEY_REQUEST_KIND_CANONICAL_BACKFILL_AUTH_TOKEN_LOGS_CURSOR_V1,
                        batch_max_id,
                    )
                    .await?;
                    if let Some(migration_state_key) = migration_state_key {
                        write_request_kind_backfill_meta_string(
                            &mut tx,
                            migration_state_key,
                            &current_request_kind_canonical_migration_running_state(
                                backend_time.now_ts(),
                            )
                            .as_meta_value(),
                        )
                        .await?;
                    }
                    Ok(())
                }
                .await;

                match batch_result {
                    Ok(()) => match tx.commit().await {
                        Ok(()) => break,
                        Err(err) => {
                            let err = ProxyError::Database(err);
                            if is_transient_sqlite_write_error(&err) {
                                backend_time
                                    .sleep(Duration::from_millis(
                                        REQUEST_KIND_CANONICAL_MIGRATION_WAIT_POLL_MS,
                                    ))
                                    .await;
                                continue;
                            }
                            return Err(err);
                        }
                    },
                    Err(err) => {
                        let retry = is_transient_sqlite_write_error(&err);
                        let _ = tx.rollback().await;
                        if retry {
                            backend_time
                                .sleep(Duration::from_millis(
                                    REQUEST_KIND_CANONICAL_MIGRATION_WAIT_POLL_MS,
                                ))
                                .await;
                            continue;
                        }
                        return Err(err);
                    }
                }
            }
        }

        cursor_after = if dry_run { cursor_before } else { batch_max_id };
        if dry_run && batch_max_id > cursor_before {
            cursor_after = batch_max_id;
        }
    }

    Ok(RequestKindCanonicalBackfillTableReport {
        table: "auth_token_logs",
        meta_key: META_KEY_REQUEST_KIND_CANONICAL_BACKFILL_AUTH_TOKEN_LOGS_CURSOR_V1,
        dry_run,
        batch_size,
        cursor_before,
        cursor_after: if dry_run { cursor_before } else { cursor_after },
        rows_scanned,
        rows_updated,
    })
}

pub(crate) async fn run_request_kind_canonical_backfill_with_pool(
    pool: &SqlitePool,
    batch_size: i64,
    dry_run: bool,
    migration_state_key: Option<&str>,
    upper_bounds: Option<RequestKindCanonicalBackfillUpperBounds>,
    backend_time: &BackendTime,
) -> Result<RequestKindCanonicalBackfillReport, ProxyError> {
    let batch_size = batch_size.max(1);
    let request_logs = backfill_request_log_request_kinds_with_pool(
        pool,
        batch_size,
        dry_run,
        migration_state_key,
        upper_bounds.map(|upper_bounds| upper_bounds.request_logs),
        backend_time,
    )
    .await?;
    let auth_token_logs = backfill_auth_token_log_request_kinds_with_pool(
        pool,
        batch_size,
        dry_run,
        migration_state_key,
        upper_bounds.map(|upper_bounds| upper_bounds.auth_token_logs),
        backend_time,
    )
    .await?;

    Ok(RequestKindCanonicalBackfillReport {
        dry_run,
        batch_size,
        request_logs,
        auth_token_logs,
    })
}

pub(crate) async fn run_request_user_id_backfill_with_pool(
    pool: &SqlitePool,
    batch_size: i64,
    backend_time: &BackendTime,
) -> Result<RequestUserIdBackfillReport, ProxyError> {
    let batch_size = batch_size.max(1);
    let cursor_before =
        read_request_kind_backfill_meta_i64(pool, META_KEY_REQUEST_USER_ID_BACKFILL_CURSOR_V1)
            .await?;
    let upper_bound = sqlx::query_scalar::<_, i64>("SELECT COALESCE(MAX(id), 0) FROM request_logs")
        .fetch_one(pool)
        .await?;
    let stable_before = backend_time
        .now_utc()
        .timestamp()
        .saturating_sub(REQUEST_USER_ID_BACKFILL_STABILITY_GRACE_SECS);

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_auth_token_logs_request_log_user_id
        ON auth_token_logs(request_log_id, request_user_id, id)
        "#,
    )
    .execute(pool)
    .await?;

    let rows = sqlx::query(
        r#"
        SELECT
            request_logs.id AS id,
            (
                SELECT atl.request_user_id
                FROM auth_token_logs atl
                WHERE atl.request_log_id = request_logs.id
                  AND atl.request_user_id IS NOT NULL
                ORDER BY atl.id DESC
                LIMIT 1
            ) AS request_user_id
        FROM request_logs
        WHERE request_logs.id > ?
          AND request_logs.id <= ?
          AND request_logs.created_at <= ?
          AND request_logs.request_user_id IS NULL
        ORDER BY id ASC
        LIMIT ?
        "#,
    )
    .bind(cursor_before)
    .bind(upper_bound)
    .bind(stable_before)
    .bind(batch_size)
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        return Ok(RequestUserIdBackfillReport {
            batch_size,
            cursor_before,
            cursor_after: cursor_before,
            upper_bound,
            rows_scanned: 0,
            rows_updated: 0,
        });
    }

    let updates = rows
        .into_iter()
        .map(|row| {
            Ok((
                row.try_get::<i64, _>("id")?,
                row.try_get::<Option<String>, _>("request_user_id")?,
            ))
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    let batch_max_id = updates
        .last()
        .map(|(id, _)| *id)
        .unwrap_or(cursor_before)
        .max(cursor_before);
    let rows_scanned = updates.len() as i64;
    let rows_updated: i64;

    loop {
        let mut tx = match pool.begin().await {
            Ok(tx) => tx,
            Err(err) => {
                let err = ProxyError::Database(err);
                if is_transient_sqlite_write_error(&err) {
                    backend_time
                        .sleep(Duration::from_millis(
                            REQUEST_KIND_CANONICAL_MIGRATION_WAIT_POLL_MS,
                        ))
                        .await;
                    continue;
                }
                return Err(err);
            }
        };

        let batch_result: Result<u64, ProxyError> = async {
            let mut updated = 0_u64;
            for (id, request_user_id) in &updates {
                let Some(request_user_id) = request_user_id else {
                    continue;
                };
                let result = sqlx::query(
                    r#"
                    UPDATE request_logs
                    SET request_user_id = ?
                    WHERE id = ?
                      AND request_user_id IS NULL
                    "#,
                )
                .bind(request_user_id)
                .bind(id)
                .execute(&mut *tx)
                .await?;
                updated += result.rows_affected();
            }

            write_request_kind_backfill_meta_i64(
                &mut tx,
                META_KEY_REQUEST_USER_ID_BACKFILL_CURSOR_V1,
                batch_max_id,
            )
            .await?;
            Ok(updated)
        }
        .await;

        match batch_result {
            Ok(updated) => match tx.commit().await {
                Ok(()) => {
                    rows_updated = updated as i64;
                    break;
                }
                Err(err) => {
                    let err = ProxyError::Database(err);
                    if is_transient_sqlite_write_error(&err) {
                        backend_time
                            .sleep(Duration::from_millis(
                                REQUEST_KIND_CANONICAL_MIGRATION_WAIT_POLL_MS,
                            ))
                            .await;
                        continue;
                    }
                    return Err(err);
                }
            },
            Err(err) => {
                let retry = is_transient_sqlite_write_error(&err);
                let _ = tx.rollback().await;
                if retry {
                    backend_time
                        .sleep(Duration::from_millis(
                            REQUEST_KIND_CANONICAL_MIGRATION_WAIT_POLL_MS,
                        ))
                        .await;
                    continue;
                }
                return Err(err);
            }
        }
    }

    Ok(RequestUserIdBackfillReport {
        batch_size,
        cursor_before,
        cursor_after: batch_max_id,
        upper_bound,
        rows_scanned,
        rows_updated,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct RequestLogsCatalogCacheEntry {
    value: RequestLogsCatalog,
    expires_at: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct UserDebugInfoSharedCacheEntry {
    shared: bool,
    expires_at: i64,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ApiKeyUsageBucketDelta {
    pub(crate) total_requests: i64,
    pub(crate) success_count: i64,
    pub(crate) error_count: i64,
    pub(crate) quota_exhausted_count: i64,
    pub(crate) valuable_success_count: i64,
    pub(crate) valuable_failure_count: i64,
    pub(crate) other_success_count: i64,
    pub(crate) other_failure_count: i64,
    pub(crate) unknown_count: i64,
}

impl ApiKeyUsageBucketDelta {
    pub(crate) fn add(&mut self, other: Self) {
        self.total_requests += other.total_requests;
        self.success_count += other.success_count;
        self.error_count += other.error_count;
        self.quota_exhausted_count += other.quota_exhausted_count;
        self.valuable_success_count += other.valuable_success_count;
        self.valuable_failure_count += other.valuable_failure_count;
        self.other_success_count += other.other_success_count;
        self.other_failure_count += other.other_failure_count;
        self.unknown_count += other.unknown_count;
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.total_requests == 0
            && self.success_count == 0
            && self.error_count == 0
            && self.quota_exhausted_count == 0
            && self.valuable_success_count == 0
            && self.valuable_failure_count == 0
            && self.other_success_count == 0
            && self.other_failure_count == 0
            && self.unknown_count == 0
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AuthTokenActivityDelta {
    pub(crate) total_requests_delta: i64,
    pub(crate) last_used_at: Option<i64>,
}

impl AuthTokenActivityDelta {
    pub(crate) fn add_request(&mut self, created_at: i64) {
        self.total_requests_delta += 1;
        self.last_used_at = Some(
            self.last_used_at
                .map_or(created_at, |current| current.max(created_at)),
        );
    }

    pub(crate) fn add(&mut self, other: Self) {
        self.total_requests_delta += other.total_requests_delta;
        if let Some(last_used_at) = other.last_used_at {
            self.last_used_at = Some(
                self.last_used_at
                    .map_or(last_used_at, |current| current.max(last_used_at)),
            );
        }
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.total_requests_delta == 0 && self.last_used_at.is_none()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct AccountUsageRollupDelta {
    pub(crate) request_count: i64,
    pub(crate) primary_success: i64,
    pub(crate) secondary_success: i64,
}

impl AccountUsageRollupDelta {
    pub(crate) fn add(&mut self, other: Self) {
        self.request_count += other.request_count;
        self.primary_success += other.primary_success;
        self.secondary_success += other.secondary_success;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct AccountRequestRollupKey {
    pub(crate) user_id: String,
    pub(crate) five_minute_bucket_start: i64,
    pub(crate) day_bucket_start: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct RequestLogCatalogRollupKey {
    pub(crate) bucket_start: i64,
    pub(crate) request_kind_key: String,
    pub(crate) request_kind_label: String,
    pub(crate) result_bucket: String,
    pub(crate) key_effect_code: String,
    pub(crate) binding_effect_code: String,
    pub(crate) selection_effect_code: String,
    pub(crate) auth_token_id: String,
    pub(crate) api_key_id: String,
    pub(crate) operational_class: String,
}

#[derive(Debug, Default)]
pub(crate) struct RequestStatsCoalescerState {
    pub(crate) pending_dashboard_rollups: HashMap<(i64, i64), DashboardRequestRollupCounts>,
    pub(crate) pending_api_key_usage: HashMap<(String, i64), ApiKeyUsageBucketDelta>,
    pub(crate) pending_auth_token_activity: HashMap<String, AuthTokenActivityDelta>,
    pub(crate) pending_account_request_rollups:
        HashMap<AccountRequestRollupKey, AccountUsageRollupDelta>,
    pub(crate) pending_request_log_catalog: HashMap<RequestLogCatalogRollupKey, i64>,
    pub(crate) flush_deadline: Option<Instant>,
    pub(crate) flushing: bool,
    pub(crate) shutdown: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct RequestStatsCoalescer {
    pub(crate) state: Arc<Mutex<RequestStatsCoalescerState>>,
    pub(crate) wake: Arc<Notify>,
    pub(crate) flushed: Arc<Notify>,
}

impl Default for RequestStatsCoalescer {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(RequestStatsCoalescerState::default())),
            wake: Arc::new(Notify::new()),
            flushed: Arc::new(Notify::new()),
        }
    }
}

impl RequestStatsCoalescer {
    pub(crate) const MAX_PENDING_KEYS: usize = 100;
    pub(crate) const FLUSH_INTERVAL: Duration = Duration::from_secs(1);

    pub(crate) fn pending_key_count(state: &RequestStatsCoalescerState) -> usize {
        state.pending_dashboard_rollups.len()
            + state.pending_api_key_usage.len()
            + state.pending_auth_token_activity.len()
            + state.pending_account_request_rollups.len()
            + state.pending_request_log_catalog.len()
    }

    fn mark_flush_deadline_if_pending(state: &mut RequestStatsCoalescerState) {
        if Self::pending_key_count(state) > 0 && state.flush_deadline.is_none() {
            state.flush_deadline = Some(Instant::now() + Self::FLUSH_INTERVAL);
        }
    }

    pub(crate) async fn enqueue_request_log_rollups(
        &self,
        api_key_id: Option<&str>,
        auth_token_id: &str,
        request_user_id: Option<&str>,
        created_at: i64,
        dashboard_counts: DashboardRequestRollupCounts,
        request_log_catalog_key: Option<RequestLogCatalogRollupKey>,
    ) {
        {
            let mut state = self.state.lock().await;
            let minute_bucket_start = created_at.div_euclid(SECS_PER_MINUTE) * SECS_PER_MINUTE;
            let day_bucket_start = local_day_bucket_start_utc_ts(created_at);
            state
                .pending_dashboard_rollups
                .entry((minute_bucket_start, SECS_PER_MINUTE))
                .or_default()
                .add(dashboard_counts);
            state
                .pending_dashboard_rollups
                .entry((day_bucket_start, SECS_PER_DAY))
                .or_default()
                .add(dashboard_counts);
            if let Some(api_key_id) = api_key_id {
                state
                    .pending_api_key_usage
                    .entry((api_key_id.to_string(), day_bucket_start))
                    .or_default()
                    .add(ApiKeyUsageBucketDelta {
                        total_requests: dashboard_counts.total_requests,
                        success_count: dashboard_counts.success_count,
                        error_count: dashboard_counts.error_count,
                        quota_exhausted_count: dashboard_counts.quota_exhausted_count,
                        valuable_success_count: dashboard_counts.valuable_success_count,
                        valuable_failure_count: dashboard_counts.valuable_failure_count,
                        other_success_count: dashboard_counts.other_success_count,
                        other_failure_count: dashboard_counts.other_failure_count,
                        unknown_count: dashboard_counts.unknown_count,
                    });
            }
            Self::enqueue_auth_token_activity_locked(
                &mut state,
                auth_token_id,
                request_user_id,
                created_at,
                dashboard_counts.valuable_success_count,
                dashboard_counts.other_success_count,
            );
            if let Some(request_log_catalog_key) = request_log_catalog_key {
                *state
                    .pending_request_log_catalog
                    .entry(request_log_catalog_key)
                    .or_default() += 1;
            }
            Self::mark_flush_deadline_if_pending(&mut state);
        }
        self.wake.notify_one();
    }

    pub(crate) async fn enqueue_auth_token_activity(
        &self,
        auth_token_id: &str,
        request_user_id: Option<&str>,
        created_at: i64,
    ) {
        {
            let mut state = self.state.lock().await;
            Self::enqueue_auth_token_activity_locked(
                &mut state,
                auth_token_id,
                request_user_id,
                created_at,
                0,
                0,
            );
            Self::mark_flush_deadline_if_pending(&mut state);
        }
        self.wake.notify_one();
    }

    fn enqueue_auth_token_activity_locked(
        state: &mut RequestStatsCoalescerState,
        auth_token_id: &str,
        request_user_id: Option<&str>,
        created_at: i64,
        primary_success_delta: i64,
        secondary_success_delta: i64,
    ) {
        state
            .pending_auth_token_activity
            .entry(auth_token_id.to_string())
            .or_default()
            .add_request(created_at);
        if let Some(user_id) = request_user_id {
            let five_minute_bucket_start =
                created_at - created_at.rem_euclid(SECS_PER_FIVE_MINUTES);
            let day_bucket_start = local_day_bucket_start_utc_ts(created_at);
            let entry = state
                .pending_account_request_rollups
                .entry(AccountRequestRollupKey {
                    user_id: user_id.to_string(),
                    five_minute_bucket_start,
                    day_bucket_start,
                })
                .or_default();
            entry.request_count += 1;
            entry.primary_success += primary_success_delta.max(0);
            entry.secondary_success += secondary_success_delta.max(0);
        }
    }

    pub(crate) async fn enqueue_dashboard_credit_rollups(&self, created_at: i64, credits: i64) {
        if credits <= 0 {
            return;
        }
        {
            let mut state = self.state.lock().await;
            let minute_bucket_start = created_at.div_euclid(SECS_PER_MINUTE) * SECS_PER_MINUTE;
            let day_bucket_start = local_day_bucket_start_utc_ts(created_at);
            for key in [
                (minute_bucket_start, SECS_PER_MINUTE),
                (day_bucket_start, SECS_PER_DAY),
            ] {
                state
                    .pending_dashboard_rollups
                    .entry(key)
                    .or_default()
                    .local_estimated_credits += credits;
            }
            Self::mark_flush_deadline_if_pending(&mut state);
        }
        self.wake.notify_one();
    }

    pub(crate) async fn wait_until_flushed(&self) {
        loop {
            let notified = {
                let state = self.state.lock().await;
                if !state.flushing
                    && state.pending_dashboard_rollups.is_empty()
                    && state.pending_api_key_usage.is_empty()
                    && state.pending_auth_token_activity.is_empty()
                    && state.pending_account_request_rollups.is_empty()
                    && state.pending_request_log_catalog.is_empty()
                    && !state.shutdown
                {
                    return;
                }
                self.flushed.clone().notified_owned()
            };
            notified.await;
        }
    }
}

#[derive(Debug)]
pub(crate) struct KeyStore {
    pub(crate) database_path: String,
    pub(crate) observability_database_path: Option<String>,
    pub(crate) _observability_lock: Option<File>,
    pub(crate) pool: SqlitePool,
    pub(crate) backend_time: BackendTime,
    pub(crate) token_binding_cache: RwLock<HashMap<String, TokenBindingCacheEntry>>,
    pub(crate) account_quota_resolution_cache:
        RwLock<HashMap<String, AccountQuotaResolutionCacheEntry>>,
    pub(crate) request_logs_catalog_cache: RwLock<HashMap<String, RequestLogsCatalogCacheEntry>>,
    pub(crate) request_log_retention_cache: RwLock<Option<RequestLogRetentionSettings>>,
    pub(crate) user_debug_info_shared_cache: RwLock<HashMap<String, UserDebugInfoSharedCacheEntry>>,
    pub(crate) request_stats_coalescer: RequestStatsCoalescer,
    pub(crate) admin_heavy_read_semaphore: Semaphore,
    #[cfg(test)]
    pub(crate) forced_pending_claim_miss_log_ids: Mutex<HashSet<i64>>,
    // Lightweight failpoint registry used by integration tests to simulate a lost quota
    // subject lease after precheck but before settlement.
    pub(crate) forced_quota_subject_lock_loss_subjects: std::sync::Mutex<HashSet<String>>,
}

include!("key_store_bootstrap.rs");
include!("key_store_observability_sidecar.rs");
include!("key_store_request_logs_gc.rs");
include!("key_store_migrations_a.rs");
include!("key_store_migrations_b.rs");
include!("key_store_admin_user_listing.rs");
include!("key_store_admin_tokens.rs");
include!("key_store_keys.rs");
include!("key_store_sessions.rs");
include!("key_store_system_settings.rs");
include!("key_store_users_and_oauth.rs");
include!("key_store_linuxdo_credit_recharge.rs");
include!("key_store_request_log_body_retention.rs");
include!("key_store_token_logs.rs");
include!("key_store_alerts.rs");
include!("key_store_announcements.rs");
include!("key_store_dashboard_window_metrics.rs");
include!("key_store_dashboard_month_series.rs");
include!("key_store_request_logs_and_dashboard.rs");
include!("key_store_request_logs_summary_windows.rs");
include!("key_store_user_rankings.rs");
include!("key_store_meta.rs");
include!("key_store_token_success_metrics.rs");
include!("key_store_jobs.rs");
include!("key_store_account_limit_snapshots.rs");
include!("key_store_account_usage_rollups.rs");
include!("key_store_ha.rs");

#[cfg(test)]
mod tests {
    use super::*;

    fn as_account_usage_rollup_tuples(
        records: &[AccountUsageRollupRecord],
    ) -> Vec<(String, i64, i64)> {
        records
            .iter()
            .map(|record| (record.user_id.clone(), record.bucket_start, record.value))
            .collect()
    }

    #[test]
    fn db_operation_log_format_includes_operation_context_and_error() {
        let rendered = format_db_operation_log(
            DbLogStatus::Error,
            "sqlite begin immediate",
            Duration::from_millis(1534),
            Some("path=data/core.db, mode=startup"),
            Some("database is locked"),
        );
        assert_eq!(
            rendered,
            "db operation error: operation=sqlite begin immediate, elapsed_ms=1534, context=path=data/core.db, mode=startup, err=database is locked"
        );
    }

    #[test]
    fn sqlite_runtime_log_context_is_stable_and_grep_friendly() {
        assert_eq!(
            sqlite_runtime_log_context("data/core.db", "startup", false, true),
            "path=data/core.db, mode=startup, read_only=false, create_if_missing=true"
        );
    }

    #[test]
    fn fold_request_rollup_rows_preserves_day_buckets_across_shared_five_minute_bucket() {
        let rows = vec![
            ("user-1".to_string(), 300, 0, 1, 1, 0),
            ("user-1".to_string(), 300, 86_400, 1, 0, 1),
        ];

        let FoldedRequestRollupRecords {
            request_records,
            primary_success_records,
            secondary_success_records,
            request_day_records,
            primary_success_day_records,
            secondary_success_day_records,
        } = fold_request_rollup_rows(rows, 0, 0);

        assert_eq!(
            as_account_usage_rollup_tuples(&request_records),
            vec![("user-1".to_string(), 300, 2)]
        );
        assert_eq!(
            as_account_usage_rollup_tuples(&primary_success_records),
            vec![("user-1".to_string(), 300, 1)]
        );
        assert_eq!(
            as_account_usage_rollup_tuples(&secondary_success_records),
            vec![("user-1".to_string(), 300, 1)]
        );
        assert_eq!(
            as_account_usage_rollup_tuples(&request_day_records),
            vec![
                ("user-1".to_string(), 0, 1),
                ("user-1".to_string(), 86_400, 1),
            ]
        );
        assert_eq!(
            as_account_usage_rollup_tuples(&primary_success_day_records),
            vec![("user-1".to_string(), 0, 1)]
        );
        assert_eq!(
            as_account_usage_rollup_tuples(&secondary_success_day_records),
            vec![("user-1".to_string(), 86_400, 1)]
        );
    }
}
