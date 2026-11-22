//! SQLite access for `snapshot`, `load` and `verify`.
//!
//! All reads go through read-only connections. Snapshots use `VACUUM INTO`, which is the
//! supported equivalent of the backup API: it produces one consistent copy including
//! committed WAL frames without ever copying the live database file.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection, Row, SqliteConnection};

use crate::error::{Error, Result, sqlite_error};
use crate::types::{ColumnKind, SqlValue};

#[derive(Debug, Clone)]
pub struct SqliteColumn {
    pub name: String,
    pub declared_type: String,
    pub kind: ColumnKind,
    /// 1-based ordering inside the primary key, `None` when the column is not part of it.
    pub primary_key_ordinal: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct SqliteTable {
    pub name: String,
    pub columns: Vec<SqliteColumn>,
}

impl SqliteTable {
    pub fn primary_key(&self) -> Vec<&SqliteColumn> {
        let mut columns: Vec<&SqliteColumn> = self
            .columns
            .iter()
            .filter(|column| column.primary_key_ordinal.is_some())
            .collect();
        columns.sort_by_key(|column| column.primary_key_ordinal);
        columns
    }

    pub fn column_names(&self) -> Vec<&str> {
        self.columns
            .iter()
            .map(|column| column.name.as_str())
            .collect()
    }
}

pub async fn open_read_only(path: &Path) -> Result<SqliteConnection> {
    if !path.exists() {
        return Err(Error::msg(format!(
            "SQLite database {} does not exist",
            path.display()
        )));
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .busy_timeout(Duration::from_secs(30));
    SqliteConnection::connect_with(&options)
        .await
        .map_err(|err| sqlite_error(format!("open {} read-only", path.display()), err))
}

pub async fn open_read_write(path: &Path) -> Result<SqliteConnection> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .busy_timeout(Duration::from_secs(30));
    SqliteConnection::connect_with(&options)
        .await
        .map_err(|err| sqlite_error(format!("open {}", path.display()), err))
}

pub async fn vacuum_into(conn: &mut SqliteConnection, target: &Path) -> Result<()> {
    if target.exists() {
        return Err(Error::msg(format!(
            "refusing to overwrite existing SQLite file {}",
            target.display()
        )));
    }
    let sql = format!("VACUUM INTO {}", sqlite_literal(&target.to_string_lossy()));
    sqlx::query(&sql)
        .execute(&mut *conn)
        .await
        .map_err(|err| sqlite_error(format!("VACUUM INTO {}", target.display()), err))?;
    Ok(())
}

pub async fn attach(conn: &mut SqliteConnection, path: &Path, alias: &str) -> Result<()> {
    let sql = format!(
        "ATTACH DATABASE {} AS {}",
        sqlite_literal(&path.to_string_lossy()),
        quote_ident(alias)
    );
    sqlx::query(&sql)
        .execute(&mut *conn)
        .await
        .map_err(|err| sqlite_error(format!("ATTACH {}", path.display()), err))?;
    Ok(())
}

pub async fn detach(conn: &mut SqliteConnection, alias: &str) -> Result<()> {
    let sql = format!("DETACH DATABASE {}", quote_ident(alias));
    sqlx::query(&sql)
        .execute(&mut *conn)
        .await
        .map_err(|err| sqlite_error(format!("DETACH {alias}"), err))?;
    Ok(())
}

/// Lists user tables (`sqlite_%` internals excluded) in schema `schema` (e.g. `main`).
pub async fn list_tables(conn: &mut SqliteConnection, schema: &str) -> Result<Vec<String>> {
    let sql = format!(
        "SELECT name FROM {}.sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY name",
        quote_ident(schema)
    );
    let rows = sqlx::query_scalar::<_, String>(&sql)
        .fetch_all(&mut *conn)
        .await
        .map_err(|err| sqlite_error(format!("list tables in {schema}"), err))?;
    Ok(rows)
}

pub async fn read_table(
    conn: &mut SqliteConnection,
    schema: &str,
    table: &str,
) -> Result<SqliteTable> {
    let sql = format!(
        "PRAGMA {}.table_info({})",
        quote_ident(schema),
        sqlite_literal(table)
    );
    let rows = sqlx::query(&sql)
        .fetch_all(&mut *conn)
        .await
        .map_err(|err| sqlite_error(format!("PRAGMA table_info({table})"), err))?;
    let mut columns = Vec::with_capacity(rows.len());
    for row in rows {
        let name: String = row
            .try_get("name")
            .map_err(|err| sqlite_error(format!("read column name of {table}"), err))?;
        let declared_type: String = row
            .try_get("type")
            .map_err(|err| sqlite_error(format!("read column type of {table}.{name}"), err))?;
        let primary_key: i64 = row
            .try_get("pk")
            .map_err(|err| sqlite_error(format!("read primary key of {table}.{name}"), err))?;
        let kind = ColumnKind::from_sqlite_declared(&declared_type)
            .map_err(|err| Error::msg(format!("table {table} column {name}: {err}")))?;
        columns.push(SqliteColumn {
            name,
            declared_type,
            kind,
            primary_key_ordinal: (primary_key > 0).then_some(primary_key as usize),
        });
    }
    if columns.is_empty() {
        return Err(Error::msg(format!(
            "table {table} in schema {schema} has no columns"
        )));
    }
    Ok(SqliteTable {
        name: table.to_string(),
        columns,
    })
}

pub async fn count_rows(conn: &mut SqliteConnection, schema: &str, table: &str) -> Result<u64> {
    let sql = format!(
        "SELECT count(*) FROM {}.{}",
        quote_ident(schema),
        quote_ident(table)
    );
    let count: i64 = sqlx::query_scalar(&sql)
        .fetch_one(&mut *conn)
        .await
        .map_err(|err| sqlite_error(format!("count rows of {table}"), err))?;
    Ok(count.max(0) as u64)
}

pub async fn user_version(conn: &mut SqliteConnection) -> Result<i64> {
    sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await
        .map_err(|err| sqlite_error("PRAGMA user_version", err))
}

/// Decodes one runtime SQLite value without coercing its storage class.
pub fn row_value(row: &sqlx::sqlite::SqliteRow, index: usize) -> Result<SqlValue> {
    use sqlx::{TypeInfo, ValueRef};

    let raw = row
        .try_get_raw(index)
        .map_err(|err| sqlite_error(format!("read column {index}"), err))?;
    if raw.is_null() {
        return Ok(SqlValue::Null);
    }
    match raw.type_info().name() {
        "INTEGER" => row
            .try_get::<i64, _>(index)
            .map(SqlValue::Integer)
            .map_err(|err| sqlite_error(format!("decode integer column {index}"), err)),
        "REAL" => row
            .try_get::<f64, _>(index)
            .map(SqlValue::Real)
            .map_err(|err| sqlite_error(format!("decode real column {index}"), err)),
        "TEXT" => row
            .try_get::<String, _>(index)
            .map(SqlValue::Text)
            .map_err(|err| sqlite_error(format!("decode text column {index}"), err)),
        "BLOB" => row
            .try_get::<Vec<u8>, _>(index)
            .map(SqlValue::Bytes)
            .map_err(|err| sqlite_error(format!("decode blob column {index}"), err)),
        other => Err(Error::msg(format!(
            "SQLite returned unsupported storage class {other} for column {index}"
        ))),
    }
}

/// Builds a SELECT in the declared column order, optionally ordered by the primary key.
pub fn select_sql(schema: &str, table: &SqliteTable, order_by_primary_key: bool) -> String {
    let columns = table
        .columns
        .iter()
        .map(|column| quote_ident(&column.name))
        .collect::<Vec<_>>()
        .join(", ");
    let mut sql = format!(
        "SELECT {columns} FROM {}.{}",
        quote_ident(schema),
        quote_ident(&table.name)
    );
    if order_by_primary_key {
        let order = table
            .primary_key()
            .iter()
            .map(|column| quote_ident(&column.name))
            .collect::<Vec<_>>()
            .join(", ");
        if !order.is_empty() {
            sql.push_str(" ORDER BY ");
            sql.push_str(&order);
        }
    }
    sql
}

pub fn sqlite_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub fn quote_ident(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

/// Resolves the observability sidecar that belongs to `core_path`.
///
/// The server keeps observability in a sibling file: `<stem>-observability.db` for an explicit
/// `*.db` path, or `observability.db` next to a `core.db` file. Returns `None` when no distinct
/// sidecar exists.
pub fn resolve_observability_path(core_path: &Path) -> Result<Option<PathBuf>> {
    let parent = core_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = core_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("sqlite");
    let mut candidates = vec![parent.join(format!("{stem}-observability.db"))];
    if stem == "core" {
        candidates.push(parent.join("observability.db"));
    }
    let mut found: Vec<PathBuf> = Vec::new();
    for candidate in candidates {
        if !candidate.exists() {
            continue;
        }
        let same_as_core = std::fs::canonicalize(&candidate)
            .ok()
            .zip(std::fs::canonicalize(core_path).ok())
            .is_some_and(|(candidate, core)| candidate == core);
        if !same_as_core {
            found.push(candidate);
        }
    }
    found.sort();
    found.dedup();
    match found.len() {
        0 => Ok(None),
        1 => Ok(found.pop()),
        _ => Err(Error::msg(format!(
            "ambiguous observability sidecar for {}: {}",
            core_path.display(),
            found
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}
