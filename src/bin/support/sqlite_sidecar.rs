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

    let observability_database_path =
        attachable_observability_path(&layout, create_if_missing, read_only);
    let mut pool_options = SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(max_connections);
    if let Some(observability_database_path) = observability_database_path {
        pool_options = pool_options.after_connect(move |conn, _meta| {
            let observability_database_path = observability_database_path.clone();
            Box::pin(async move { attach_observability(conn, &observability_database_path).await })
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
    if let Some(observability_database_path) =
        attachable_observability_path(&layout, create_if_missing, false)
    {
        attach_observability(&mut connection, &observability_database_path).await?;
    }
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut connection)
        .await?;
    Ok(connection)
}

fn attachable_observability_path(
    layout: &SqliteDatabaseLayout,
    create_if_missing: bool,
    read_only: bool,
) -> Option<String> {
    let path = layout.observability_database_path.as_deref()?;
    if !read_only || create_if_missing || Path::new(path).exists() {
        Some(path.to_string())
    } else {
        None
    }
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
