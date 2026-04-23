use crate::analysis::*;
use crate::models::*;
use crate::tavily_proxy::QuotaSubjectDbLease;
use crate::*;
use sqlx::Row;

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

fn subtract_summary_window_metrics(
    total: &SummaryWindowMetrics,
    subtract: &SummaryWindowMetrics,
) -> SummaryWindowMetrics {
    SummaryWindowMetrics {
        total_requests: total.total_requests.saturating_sub(subtract.total_requests),
        success_count: total.success_count.saturating_sub(subtract.success_count),
        error_count: total.error_count.saturating_sub(subtract.error_count),
        quota_exhausted_count: total
            .quota_exhausted_count
            .saturating_sub(subtract.quota_exhausted_count),
        valuable_success_count: total
            .valuable_success_count
            .saturating_sub(subtract.valuable_success_count),
        valuable_failure_count: total
            .valuable_failure_count
            .saturating_sub(subtract.valuable_failure_count),
        other_success_count: total
            .other_success_count
            .saturating_sub(subtract.other_success_count),
        other_failure_count: total
            .other_failure_count
            .saturating_sub(subtract.other_failure_count),
        unknown_count: total.unknown_count.saturating_sub(subtract.unknown_count),
        upstream_exhausted_key_count: total
            .upstream_exhausted_key_count
            .saturating_sub(subtract.upstream_exhausted_key_count),
        new_keys: total.new_keys.saturating_sub(subtract.new_keys),
        new_quarantines: total
            .new_quarantines
            .saturating_sub(subtract.new_quarantines),
        quota_charge: SummaryQuotaCharge {
            local_estimated_credits: total
                .quota_charge
                .local_estimated_credits
                .saturating_sub(subtract.quota_charge.local_estimated_credits),
            ..SummaryQuotaCharge::default()
        },
    }
}

#[derive(Clone, Copy, Default)]
struct DashboardRequestRollupCounts {
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
    let mut options = SqliteConnectOptions::new()
        .filename(database_path)
        .create_if_missing(create_if_missing)
        .read_only(read_only)
        .busy_timeout(Duration::from_secs(5));
    if !read_only {
        options = options.journal_mode(SqliteJournalMode::Wal);
    }

    SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(5)
        .connect_with(options)
        .await
        .map_err(ProxyError::Database)
}

pub(crate) async fn begin_immediate_sqlite_connection(
    pool: &SqlitePool,
) -> Result<sqlx::pool::PoolConnection<Sqlite>, ProxyError> {
    let mut conn = pool.acquire().await?;
    sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
    Ok(conn)
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
CREATE TABLE request_logs_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    api_key_id TEXT,
    auth_token_id TEXT,
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
    forwarded_headers TEXT,
    dropped_headers TEXT,
    visibility TEXT NOT NULL DEFAULT 'visible',
    created_at INTEGER NOT NULL,
    FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
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
    request_log_id INTEGER REFERENCES request_logs(id),
    created_at INTEGER NOT NULL
)
"#;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RequestLogDiagnosticMetadata {
    gateway_mode: Option<String>,
    experiment_variant: Option<String>,
    proxy_session_id: Option<String>,
    routing_subject_hash: Option<String>,
    upstream_operation: Option<String>,
    fallback_reason: Option<String>,
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
                            tokio::time::sleep(Duration::from_millis(
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
                                Utc::now().timestamp(),
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
                                tokio::time::sleep(Duration::from_millis(
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
                            tokio::time::sleep(Duration::from_millis(
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
                            tokio::time::sleep(Duration::from_millis(
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
                                Utc::now().timestamp(),
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
                                tokio::time::sleep(Duration::from_millis(
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
                            tokio::time::sleep(Duration::from_millis(
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
) -> Result<RequestKindCanonicalBackfillReport, ProxyError> {
    let batch_size = batch_size.max(1);
    let request_logs = backfill_request_log_request_kinds_with_pool(
        pool,
        batch_size,
        dry_run,
        migration_state_key,
        upper_bounds.map(|upper_bounds| upper_bounds.request_logs),
    )
    .await?;
    let auth_token_logs = backfill_auth_token_log_request_kinds_with_pool(
        pool,
        batch_size,
        dry_run,
        migration_state_key,
        upper_bounds.map(|upper_bounds| upper_bounds.auth_token_logs),
    )
    .await?;

    Ok(RequestKindCanonicalBackfillReport {
        dry_run,
        batch_size,
        request_logs,
        auth_token_logs,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct RequestLogsCatalogCacheEntry {
    value: RequestLogsCatalog,
    expires_at: Instant,
}

#[derive(Debug)]
pub(crate) struct KeyStore {
    pub(crate) pool: SqlitePool,
    pub(crate) token_binding_cache: RwLock<HashMap<String, TokenBindingCacheEntry>>,
    pub(crate) account_quota_resolution_cache:
        RwLock<HashMap<String, AccountQuotaResolutionCacheEntry>>,
    pub(crate) request_logs_catalog_cache: RwLock<HashMap<String, RequestLogsCatalogCacheEntry>>,
    #[cfg(test)]
    pub(crate) forced_pending_claim_miss_log_ids: Mutex<HashSet<i64>>,
    // Lightweight failpoint registry used by integration tests to simulate a lost quota
    // subject lease after precheck but before settlement.
    pub(crate) forced_quota_subject_lock_loss_subjects: std::sync::Mutex<HashSet<String>>,
}

include!("key_store_bootstrap.rs");
include!("key_store_migrations_a.rs");
include!("key_store_migrations_b.rs");
include!("key_store_keys.rs");
include!("key_store_sessions.rs");
include!("key_store_users_and_oauth.rs");
include!("key_store_token_logs.rs");
include!("key_store_alerts.rs");
include!("key_store_request_logs_and_dashboard.rs");
include!("key_store_account_limit_snapshots.rs");
include!("key_store_account_usage_rollups.rs");
