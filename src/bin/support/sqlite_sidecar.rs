#![allow(dead_code)]

use std::{path::Path, time::Duration};

use sqlx::{
    Connection, Executor, SqliteConnection,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct SqliteDatabaseLayout {
    core_database_path: String,
    observability_database_path: Option<String>,
}

impl SqliteDatabaseLayout {
    fn from_database_path(database_path: &str) -> Self {
        let database_path = database_path.trim();
        let path = Path::new(database_path);
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

pub fn observability_database_path(database_path: &str) -> Option<String> {
    SqliteDatabaseLayout::from_database_path(database_path).observability_database_path
}

pub async fn connect_sqlite_pool(
    database_path: &str,
    create_if_missing: bool,
    read_only: bool,
    max_connections: u32,
) -> Result<sqlx::SqlitePool, sqlx::Error> {
    let layout = SqliteDatabaseLayout::from_database_path(database_path);
    let mut options = SqliteConnectOptions::new()
        .filename(&layout.core_database_path)
        .create_if_missing(create_if_missing)
        .read_only(read_only)
        .busy_timeout(Duration::from_secs(5));
    if !read_only {
        options = options.journal_mode(SqliteJournalMode::Wal);
    }

    let mut setup_connection = SqliteConnection::connect_with(&options).await?;
    let target_path =
        if let Some(observability_database_path) = layout.observability_database_path.as_deref() {
            select_observability_attach_path(
                &mut setup_connection,
                &layout.core_database_path,
                observability_database_path,
                create_if_missing,
                read_only,
            )
            .await?
        } else {
            None
        };
    setup_connection.close().await?;
    if let Some(target_path) = target_path.as_deref() {
        configure_observability_journal_mode(target_path, &layout.core_database_path, read_only)
            .await?;
    }

    let observability_database_path = target_path;
    let mut pool_options = SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(max_connections);
    if let Some(observability_database_path) = observability_database_path {
        pool_options = pool_options.after_connect(move |conn, _meta| {
            let observability_database_path = observability_database_path.clone();
            Box::pin(async move {
                attach_observability(conn, &observability_database_path).await?;
                Ok(())
            })
        });
    }

    pool_options.connect_with(options).await
}

pub async fn connect_immediate_sqlite_connection(
    database_path: &str,
    create_if_missing: bool,
) -> Result<SqliteConnection, sqlx::Error> {
    let layout = SqliteDatabaseLayout::from_database_path(database_path);
    let options = SqliteConnectOptions::new()
        .filename(&layout.core_database_path)
        .create_if_missing(create_if_missing)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let mut connection = SqliteConnection::connect_with(&options).await?;
    if let Some(observability_database_path) = layout.observability_database_path.as_deref()
        && let Some(target_path) = select_observability_attach_path(
            &mut connection,
            &layout.core_database_path,
            observability_database_path,
            create_if_missing,
            false,
        )
        .await?
    {
        configure_observability_journal_mode(&target_path, &layout.core_database_path, false)
            .await?;
        attach_observability(&mut connection, &target_path).await?;
    }
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut connection)
        .await?;
    Ok(connection)
}

const LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES: u64 = 32 * 1024 * 1024;

async fn select_observability_attach_path(
    connection: &mut SqliteConnection,
    core_database_path: &str,
    observability_database_path: &str,
    create_if_missing: bool,
    read_only: bool,
) -> Result<Option<String>, sqlx::Error> {
    let sidecar_exists = Path::new(observability_database_path).exists();
    let legacy_request_logs_exists = sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'request_logs' LIMIT 1",
    )
    .fetch_optional(&mut *connection)
    .await?
    .is_some();
    if legacy_request_logs_exists
        && (read_only || !legacy_request_logs_inline_sidecar_migration_allowed(core_database_path))
    {
        return Ok(Some(core_database_path.to_string()));
    }

    if !read_only || create_if_missing || sidecar_exists {
        return Ok(Some(observability_database_path.to_string()));
    }

    Ok(None)
}

fn legacy_request_logs_inline_sidecar_migration_allowed(database_path: &str) -> bool {
    std::fs::metadata(database_path)
        .map(|metadata| metadata.len() <= LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES)
        .unwrap_or(false)
}

async fn attach_observability(
    connection: &mut SqliteConnection,
    database_path: &str,
) -> Result<(), sqlx::Error> {
    let attach_sql = format!(
        "ATTACH DATABASE '{}' AS observability",
        database_path.replace('\'', "''")
    );
    connection.execute(attach_sql.as_str()).await?;
    Ok(())
}

async fn configure_observability_journal_mode(
    observability_database_path: &str,
    core_database_path: &str,
    read_only: bool,
) -> Result<(), sqlx::Error> {
    if read_only || sqlite_paths_match(core_database_path, observability_database_path) {
        return Ok(());
    }

    let options = SqliteConnectOptions::new()
        .filename(observability_database_path)
        .create_if_missing(!read_only)
        .read_only(false)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let mut sidecar = SqliteConnection::connect_with(&options).await?;
    let current_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut sidecar)
        .await?;
    if current_mode.eq_ignore_ascii_case("wal") {
        sidecar.close().await?;
        return Ok(());
    }

    let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode = WAL")
        .fetch_one(&mut sidecar)
        .await?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        sidecar.close().await?;
        return Err(sqlx::Error::Protocol(format!(
            "observability sidecar journal mode is {journal_mode:?}, expected WAL"
        )));
    }
    sidecar.close().await?;
    Ok(())
}

fn sqlite_paths_match(lhs: &str, rhs: &str) -> bool {
    let lhs_path = Path::new(lhs);
    let rhs_path = Path::new(rhs);
    match (lhs_path.canonicalize(), rhs_path.canonicalize()) {
        (Ok(lhs), Ok(rhs)) => lhs == rhs,
        _ => lhs == rhs,
    }
}

fn sqlite_sidecar_path(database_path: &str, file_name: &str) -> String {
    let path = Path::new(database_path);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
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
