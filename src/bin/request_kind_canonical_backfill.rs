use std::io::{self, Write};
use std::time::Duration;

use clap::Parser;
use dotenvy::dotenv;
use serde::Serialize;
use sqlx::{
    Row,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use tavily_hikari::{canonicalize_request_log_request_kind, finalize_token_request_kind};

const META_KEY_REQUEST_LOGS_CURSOR: &str = "request_kind_canonical_backfill_request_logs_v1";
const META_KEY_AUTH_TOKEN_LOGS_CURSOR: &str = "request_kind_canonical_backfill_auth_token_logs_v1";

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Canonicalize request_kind fields while preserving legacy snapshots"
)]
struct Cli {
    #[arg(long, env = "PROXY_DB_PATH", default_value = "data/tavily_proxy.db")]
    db_path: String,

    #[arg(long, default_value_t = 500)]
    batch_size: i64,

    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

#[derive(Debug, Serialize)]
struct BackfillTableReport {
    table: &'static str,
    meta_key: &'static str,
    dry_run: bool,
    batch_size: i64,
    cursor_before: i64,
    cursor_after: i64,
    rows_scanned: i64,
    rows_updated: i64,
    rows_snapshotted: i64,
}

#[derive(Debug, Serialize)]
struct BackfillReport {
    dry_run: bool,
    batch_size: i64,
    request_logs: BackfillTableReport,
    auth_token_logs: BackfillTableReport,
}

#[derive(Debug, Clone)]
struct RequestLogRow {
    id: i64,
    path: String,
    request_body: Option<Vec<u8>>,
    request_kind_key: Option<String>,
    request_kind_label: Option<String>,
    request_kind_detail: Option<String>,
    legacy_request_kind_key: Option<String>,
    legacy_request_kind_label: Option<String>,
    legacy_request_kind_detail: Option<String>,
}

#[derive(Debug, Clone)]
struct TokenLogRow {
    id: i64,
    method: String,
    path: String,
    query: Option<String>,
    request_kind_key: Option<String>,
    request_kind_label: Option<String>,
    request_kind_detail: Option<String>,
    legacy_request_kind_key: Option<String>,
    legacy_request_kind_label: Option<String>,
    legacy_request_kind_detail: Option<String>,
}

#[derive(Debug, Clone)]
struct CanonicalUpdate {
    id: i64,
    request_kind_key: String,
    request_kind_label: String,
    request_kind_detail: Option<String>,
    legacy_request_kind_key: Option<String>,
    legacy_request_kind_label: Option<String>,
    legacy_request_kind_detail: Option<String>,
    snapshotted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RequestKindSnapshot {
    key: Option<String>,
    label: Option<String>,
    detail: Option<String>,
}

impl RequestKindSnapshot {
    fn has_any(&self) -> bool {
        self.key.is_some() || self.label.is_some() || self.detail.is_some()
    }
}

fn normalize_field(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn resolve_legacy_snapshot(
    current: &RequestKindSnapshot,
    legacy: &RequestKindSnapshot,
    desired: &RequestKindSnapshot,
) -> (RequestKindSnapshot, bool) {
    let already_canonical = current == desired;
    let should_snapshot = !already_canonical && !legacy.has_any() && current.has_any();
    let next_legacy = if should_snapshot {
        current.clone()
    } else {
        legacy.clone()
    };
    (next_legacy, should_snapshot)
}

async fn connect_sqlite_pool(db_path: &str) -> Result<sqlx::SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(5)
        .connect_with(options)
        .await
}

async fn table_column_exists(
    pool: &sqlx::SqlitePool,
    table: &str,
    column: &str,
) -> Result<bool, sqlx::Error> {
    let sql = format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name = ? LIMIT 1");
    Ok(sqlx::query_scalar::<_, i64>(&sql)
        .bind(column)
        .fetch_optional(pool)
        .await?
        .is_some())
}

async fn ensure_request_kind_backfill_schema(pool: &sqlx::SqlitePool) -> Result<(), sqlx::Error> {
    for (table, columns) in [
        (
            "request_logs",
            [
                (
                    "legacy_request_kind_key",
                    "ALTER TABLE request_logs ADD COLUMN legacy_request_kind_key TEXT",
                ),
                (
                    "legacy_request_kind_label",
                    "ALTER TABLE request_logs ADD COLUMN legacy_request_kind_label TEXT",
                ),
                (
                    "legacy_request_kind_detail",
                    "ALTER TABLE request_logs ADD COLUMN legacy_request_kind_detail TEXT",
                ),
            ],
        ),
        (
            "auth_token_logs",
            [
                (
                    "legacy_request_kind_key",
                    "ALTER TABLE auth_token_logs ADD COLUMN legacy_request_kind_key TEXT",
                ),
                (
                    "legacy_request_kind_label",
                    "ALTER TABLE auth_token_logs ADD COLUMN legacy_request_kind_label TEXT",
                ),
                (
                    "legacy_request_kind_detail",
                    "ALTER TABLE auth_token_logs ADD COLUMN legacy_request_kind_detail TEXT",
                ),
            ],
        ),
    ] {
        for (column, alter_sql) in columns {
            if !table_column_exists(pool, table, column).await? {
                sqlx::query(alter_sql).execute(pool).await?;
            }
        }
    }

    Ok(())
}

async fn read_meta_i64(pool: &sqlx::SqlitePool, key: &str) -> Result<i64, sqlx::Error> {
    Ok(
        sqlx::query_scalar::<_, Option<String>>("SELECT value FROM meta WHERE key = ? LIMIT 1")
            .bind(key)
            .fetch_optional(pool)
            .await?
            .flatten()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(0),
    )
}

async fn write_meta_i64(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    key: &str,
    value: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO meta (key, value)
        VALUES (?, ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
    )
    .bind(key)
    .bind(value.to_string())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn build_request_log_update(row: RequestLogRow) -> Option<CanonicalUpdate> {
    let current = RequestKindSnapshot {
        key: normalize_field(row.request_kind_key),
        label: normalize_field(row.request_kind_label),
        detail: normalize_field(row.request_kind_detail),
    };
    let legacy = RequestKindSnapshot {
        key: normalize_field(row.legacy_request_kind_key),
        label: normalize_field(row.legacy_request_kind_label),
        detail: normalize_field(row.legacy_request_kind_detail),
    };

    let kind = canonicalize_request_log_request_kind(
        row.path.as_str(),
        row.request_body.as_deref(),
        current.key.clone(),
        current.label.clone(),
        current.detail.clone(),
    );
    let desired = RequestKindSnapshot {
        key: Some(kind.key.clone()),
        label: Some(kind.label.clone()),
        detail: normalize_field(kind.detail),
    };
    let (next_legacy, snapshotted) = resolve_legacy_snapshot(&current, &legacy, &desired);

    if current == desired && legacy == next_legacy {
        return None;
    }

    Some(CanonicalUpdate {
        id: row.id,
        request_kind_key: kind.key,
        request_kind_label: kind.label,
        request_kind_detail: desired.detail,
        legacy_request_kind_key: next_legacy.key,
        legacy_request_kind_label: next_legacy.label,
        legacy_request_kind_detail: next_legacy.detail,
        snapshotted,
    })
}

fn build_token_log_update(row: TokenLogRow) -> Option<CanonicalUpdate> {
    let current = RequestKindSnapshot {
        key: normalize_field(row.request_kind_key),
        label: normalize_field(row.request_kind_label),
        detail: normalize_field(row.request_kind_detail),
    };
    let legacy = RequestKindSnapshot {
        key: normalize_field(row.legacy_request_kind_key),
        label: normalize_field(row.legacy_request_kind_label),
        detail: normalize_field(row.legacy_request_kind_detail),
    };

    let kind = finalize_token_request_kind(
        row.method.as_str(),
        row.path.as_str(),
        row.query.as_deref(),
        current.key.clone(),
        current.label.clone(),
        current.detail.clone(),
    );
    let desired = RequestKindSnapshot {
        key: Some(kind.key.clone()),
        label: Some(kind.label.clone()),
        detail: normalize_field(kind.detail),
    };
    let (next_legacy, snapshotted) = resolve_legacy_snapshot(&current, &legacy, &desired);

    if current == desired && legacy == next_legacy {
        return None;
    }

    Some(CanonicalUpdate {
        id: row.id,
        request_kind_key: kind.key,
        request_kind_label: kind.label,
        request_kind_detail: desired.detail,
        legacy_request_kind_key: next_legacy.key,
        legacy_request_kind_label: next_legacy.label,
        legacy_request_kind_detail: next_legacy.detail,
        snapshotted,
    })
}

async fn backfill_request_logs(
    pool: &sqlx::SqlitePool,
    batch_size: i64,
    dry_run: bool,
) -> Result<BackfillTableReport, sqlx::Error> {
    let cursor_before = read_meta_i64(pool, META_KEY_REQUEST_LOGS_CURSOR).await?;
    let mut cursor_after = cursor_before;
    let mut rows_scanned = 0_i64;
    let mut rows_updated = 0_i64;
    let mut rows_snapshotted = 0_i64;

    loop {
        let rows = sqlx::query(
            r#"
            SELECT
                id,
                path,
                request_body,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                legacy_request_kind_key,
                legacy_request_kind_label,
                legacy_request_kind_detail
            FROM request_logs
            WHERE id > ?
            ORDER BY id ASC
            LIMIT ?
            "#,
        )
        .bind(cursor_after)
        .bind(batch_size)
        .fetch_all(pool)
        .await?;
        if rows.is_empty() {
            break;
        }

        let parsed_rows = rows
            .into_iter()
            .map(|row| {
                Ok(RequestLogRow {
                    id: row.try_get("id")?,
                    path: row.try_get("path")?,
                    request_body: row.try_get("request_body")?,
                    request_kind_key: row.try_get("request_kind_key")?,
                    request_kind_label: row.try_get("request_kind_label")?,
                    request_kind_detail: row.try_get("request_kind_detail")?,
                    legacy_request_kind_key: row.try_get("legacy_request_kind_key")?,
                    legacy_request_kind_label: row.try_get("legacy_request_kind_label")?,
                    legacy_request_kind_detail: row.try_get("legacy_request_kind_detail")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;
        let batch_max_id = parsed_rows.last().map(|row| row.id).unwrap_or(cursor_after);
        rows_scanned += parsed_rows.len() as i64;

        let updates = parsed_rows
            .into_iter()
            .filter_map(build_request_log_update)
            .collect::<Vec<_>>();
        rows_updated += updates.len() as i64;
        rows_snapshotted += updates.iter().filter(|update| update.snapshotted).count() as i64;

        if !dry_run {
            let mut tx = pool.begin().await?;
            for update in &updates {
                sqlx::query(
                    r#"
                    UPDATE request_logs
                    SET
                        request_kind_key = ?,
                        request_kind_label = ?,
                        request_kind_detail = ?,
                        legacy_request_kind_key = ?,
                        legacy_request_kind_label = ?,
                        legacy_request_kind_detail = ?
                    WHERE id = ?
                    "#,
                )
                .bind(&update.request_kind_key)
                .bind(&update.request_kind_label)
                .bind(&update.request_kind_detail)
                .bind(&update.legacy_request_kind_key)
                .bind(&update.legacy_request_kind_label)
                .bind(&update.legacy_request_kind_detail)
                .bind(update.id)
                .execute(&mut *tx)
                .await?;
            }
            write_meta_i64(&mut tx, META_KEY_REQUEST_LOGS_CURSOR, batch_max_id).await?;
            tx.commit().await?;
        }

        cursor_after = if dry_run { cursor_before } else { batch_max_id };
        if dry_run && batch_max_id > cursor_before {
            cursor_after = batch_max_id;
        }
    }

    Ok(BackfillTableReport {
        table: "request_logs",
        meta_key: META_KEY_REQUEST_LOGS_CURSOR,
        dry_run,
        batch_size,
        cursor_before,
        cursor_after: if dry_run { cursor_before } else { cursor_after },
        rows_scanned,
        rows_updated,
        rows_snapshotted,
    })
}

async fn backfill_auth_token_logs(
    pool: &sqlx::SqlitePool,
    batch_size: i64,
    dry_run: bool,
) -> Result<BackfillTableReport, sqlx::Error> {
    let cursor_before = read_meta_i64(pool, META_KEY_AUTH_TOKEN_LOGS_CURSOR).await?;
    let mut cursor_after = cursor_before;
    let mut rows_scanned = 0_i64;
    let mut rows_updated = 0_i64;
    let mut rows_snapshotted = 0_i64;

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
                request_kind_detail,
                legacy_request_kind_key,
                legacy_request_kind_label,
                legacy_request_kind_detail
            FROM auth_token_logs
            WHERE id > ?
            ORDER BY id ASC
            LIMIT ?
            "#,
        )
        .bind(cursor_after)
        .bind(batch_size)
        .fetch_all(pool)
        .await?;
        if rows.is_empty() {
            break;
        }

        let parsed_rows = rows
            .into_iter()
            .map(|row| {
                Ok(TokenLogRow {
                    id: row.try_get("id")?,
                    method: row.try_get("method")?,
                    path: row.try_get("path")?,
                    query: row.try_get("query")?,
                    request_kind_key: row.try_get("request_kind_key")?,
                    request_kind_label: row.try_get("request_kind_label")?,
                    request_kind_detail: row.try_get("request_kind_detail")?,
                    legacy_request_kind_key: row.try_get("legacy_request_kind_key")?,
                    legacy_request_kind_label: row.try_get("legacy_request_kind_label")?,
                    legacy_request_kind_detail: row.try_get("legacy_request_kind_detail")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;
        let batch_max_id = parsed_rows.last().map(|row| row.id).unwrap_or(cursor_after);
        rows_scanned += parsed_rows.len() as i64;

        let updates = parsed_rows
            .into_iter()
            .filter_map(build_token_log_update)
            .collect::<Vec<_>>();
        rows_updated += updates.len() as i64;
        rows_snapshotted += updates.iter().filter(|update| update.snapshotted).count() as i64;

        if !dry_run {
            let mut tx = pool.begin().await?;
            for update in &updates {
                sqlx::query(
                    r#"
                    UPDATE auth_token_logs
                    SET
                        request_kind_key = ?,
                        request_kind_label = ?,
                        request_kind_detail = ?,
                        legacy_request_kind_key = ?,
                        legacy_request_kind_label = ?,
                        legacy_request_kind_detail = ?
                    WHERE id = ?
                    "#,
                )
                .bind(&update.request_kind_key)
                .bind(&update.request_kind_label)
                .bind(&update.request_kind_detail)
                .bind(&update.legacy_request_kind_key)
                .bind(&update.legacy_request_kind_label)
                .bind(&update.legacy_request_kind_detail)
                .bind(update.id)
                .execute(&mut *tx)
                .await?;
            }
            write_meta_i64(&mut tx, META_KEY_AUTH_TOKEN_LOGS_CURSOR, batch_max_id).await?;
            tx.commit().await?;
        }

        cursor_after = if dry_run { cursor_before } else { batch_max_id };
        if dry_run && batch_max_id > cursor_before {
            cursor_after = batch_max_id;
        }
    }

    Ok(BackfillTableReport {
        table: "auth_token_logs",
        meta_key: META_KEY_AUTH_TOKEN_LOGS_CURSOR,
        dry_run,
        batch_size,
        cursor_before,
        cursor_after: if dry_run { cursor_before } else { cursor_after },
        rows_scanned,
        rows_updated,
        rows_snapshotted,
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let cli = Cli::parse();
    let batch_size = cli.batch_size.max(1);

    let pool = connect_sqlite_pool(&cli.db_path).await?;
    ensure_request_kind_backfill_schema(&pool).await?;
    let request_logs = backfill_request_logs(&pool, batch_size, cli.dry_run).await?;
    let auth_token_logs = backfill_auth_token_logs(&pool, batch_size, cli.dry_run).await?;

    let report = BackfillReport {
        dry_run: cli.dry_run,
        batch_size,
        request_logs,
        auth_token_logs,
    };

    serde_json::to_writer_pretty(io::stdout(), &report)?;
    io::stdout().write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nanoid::nanoid;

    fn temp_db_path(prefix: &str) -> String {
        std::env::temp_dir()
            .join(format!("{prefix}-{}.db", nanoid!(8)))
            .to_string_lossy()
            .to_string()
    }

    async fn create_test_pool(db_path: &str) -> sqlx::SqlitePool {
        let options = SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("connect test sqlite pool")
    }

    #[tokio::test]
    async fn ensure_request_kind_backfill_schema_adds_missing_legacy_columns() {
        let db_path = temp_db_path("request-kind-backfill-schema-self-heal");
        let pool = create_test_pool(&db_path).await;

        sqlx::query(
            r#"
            CREATE TABLE meta (
                key TEXT PRIMARY KEY,
                value TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create meta");
        sqlx::query(
            r#"
            CREATE TABLE request_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL,
                request_body BLOB,
                request_kind_key TEXT,
                request_kind_label TEXT,
                request_kind_detail TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create request_logs");
        sqlx::query(
            r#"
            CREATE TABLE auth_token_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                method TEXT NOT NULL,
                path TEXT NOT NULL,
                query TEXT,
                request_kind_key TEXT,
                request_kind_label TEXT,
                request_kind_detail TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create auth_token_logs");

        ensure_request_kind_backfill_schema(&pool)
            .await
            .expect("self-heal missing legacy columns");

        for table in ["request_logs", "auth_token_logs"] {
            for column in [
                "legacy_request_kind_key",
                "legacy_request_kind_label",
                "legacy_request_kind_detail",
            ] {
                assert!(
                    table_column_exists(&pool, table, column)
                        .await
                        .expect("probe healed column"),
                    "{table} should add {column}"
                );
            }
        }

        drop(pool);
        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn backfill_request_logs_succeeds_when_legacy_columns_were_missing() {
        let db_path = temp_db_path("request-kind-backfill-request-logs-self-heal");
        let pool = create_test_pool(&db_path).await;

        sqlx::query(
            r#"
            CREATE TABLE meta (
                key TEXT PRIMARY KEY,
                value TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create meta");
        sqlx::query(
            r#"
            CREATE TABLE request_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL,
                request_body BLOB,
                request_kind_key TEXT,
                request_kind_label TEXT,
                request_kind_detail TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create request_logs");
        sqlx::query(
            r#"
            CREATE TABLE auth_token_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                method TEXT NOT NULL,
                path TEXT NOT NULL,
                query TEXT,
                request_kind_key TEXT,
                request_kind_label TEXT,
                request_kind_detail TEXT,
                legacy_request_kind_key TEXT,
                legacy_request_kind_label TEXT,
                legacy_request_kind_detail TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create auth_token_logs");
        sqlx::query(
            r#"
            INSERT INTO request_logs (
                path,
                request_body,
                request_kind_key,
                request_kind_label,
                request_kind_detail
            ) VALUES (
                '/mcp/search',
                X'7B226B6579223A2276616C7565227D',
                'mcp:raw:/mcp/search',
                'MCP | /mcp/search',
                NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("insert legacy request log");

        ensure_request_kind_backfill_schema(&pool)
            .await
            .expect("self-heal schema before backfill");
        let report = backfill_request_logs(&pool, 100, false)
            .await
            .expect("backfill request logs");

        assert_eq!(report.rows_updated, 1);
        assert_eq!(report.rows_snapshotted, 1);

        let row = sqlx::query(
            r#"
            SELECT
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                legacy_request_kind_key,
                legacy_request_kind_label,
                legacy_request_kind_detail
            FROM request_logs
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .fetch_one(&pool)
        .await
        .expect("read canonicalized request log");

        assert_eq!(
            row.try_get::<String, _>("request_kind_key").unwrap(),
            "mcp:unsupported-path"
        );
        assert_eq!(
            row.try_get::<String, _>("request_kind_label").unwrap(),
            "MCP | unsupported path"
        );
        assert_eq!(
            row.try_get::<Option<String>, _>("request_kind_detail")
                .unwrap()
                .as_deref(),
            Some("/mcp/search")
        );
        assert_eq!(
            row.try_get::<Option<String>, _>("legacy_request_kind_key")
                .unwrap()
                .as_deref(),
            Some("mcp:raw:/mcp/search")
        );
        assert_eq!(
            row.try_get::<Option<String>, _>("legacy_request_kind_label")
                .unwrap()
                .as_deref(),
            Some("MCP | /mcp/search")
        );
        assert_eq!(
            row.try_get::<Option<String>, _>("legacy_request_kind_detail")
                .unwrap()
                .as_deref(),
            None
        );

        drop(pool);
        let _ = std::fs::remove_file(db_path);
    }
}
