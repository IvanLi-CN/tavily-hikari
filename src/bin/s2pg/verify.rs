//! `verify`: three checks per table against the loaded PostgreSQL target.
//!
//! 1. row count
//! 2. primary key set (ordered, hashed in chunks)
//! 3. column aggregates: exact integer sums/extremes, relative-tolerance double sums/extremes,
//!    text lengths plus an order-independent value digest, bytea byte lengths, exact numerics
//!
//! A single repeatable-read snapshot holds the PostgreSQL view stable for the whole run. Any
//! mismatch makes the command exit non-zero; the JSON report keeps the evidence.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use futures_util::TryStreamExt;
use sqlx::{Connection, PgConnection, Row, SqliteConnection};

use crate::aggregate::AggregateState;
use crate::catalog::{self, Catalog, TargetTable};
use crate::cli::VerifyArgs;
use crate::error::{Error, Result, postgres_error, sqlite_error};
use crate::hash::{ChunkedDigest, encode_primary_key_value, sha256_file};
use crate::report::{
    ColumnReport, Failure, PrimaryKeyReport, RowReport, TableReport, VerificationReport,
};
use crate::sqlite::{self, SqliteTable, quote_ident};
use crate::types::{ColumnKind, SqlValue};

struct TableScan {
    rows: u64,
    pk_digest: String,
    columns: BTreeMap<String, AggregateState>,
}

pub async fn run(args: &VerifyArgs) -> Result<VerificationReport> {
    let snapshot_path = canonical_snapshot(&args.snapshot)?;
    let (_, snapshot_id) = sha256_file(&snapshot_path)?;
    let mut sqlite = sqlite::open_read_only(&snapshot_path).await?;
    let mut tables = Vec::new();
    for name in sqlite::list_tables(&mut sqlite, "main").await? {
        tables.push(sqlite::read_table(&mut sqlite, "main", &name).await?);
    }

    let mut pg = PgConnection::connect(&args.database_url)
        .await
        .map_err(|err| Error::msg(format!("connect to PostgreSQL: {err}")))?;
    sqlx::query("BEGIN ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut pg)
        .await
        .map_err(|err| postgres_error("begin verification snapshot", err))?;
    let outcome = verify_all(&mut pg, &mut sqlite, &tables, snapshot_id, snapshot_path).await;
    let _ = sqlx::query("COMMIT").execute(&mut pg).await;
    let report = outcome?;
    report.write(&args.report)?;
    Ok(report)
}

async fn verify_all(
    pg: &mut PgConnection,
    sqlite: &mut SqliteConnection,
    tables: &[SqliteTable],
    snapshot_id: String,
    snapshot_path: PathBuf,
) -> Result<VerificationReport> {
    let catalog = catalog::load_catalog(pg).await?;
    let mut report =
        VerificationReport::new(snapshot_id, snapshot_path.to_string_lossy().to_string());
    for table in tables {
        let table_report = verify_table(pg, sqlite, table, &catalog, &mut report.failures).await?;
        report.tables.push(table_report);
    }
    report.finalize();
    Ok(report)
}

async fn verify_table(
    pg: &mut PgConnection,
    sqlite: &mut SqliteConnection,
    table: &SqliteTable,
    catalog: &Catalog,
    failures: &mut Vec<Failure>,
) -> Result<TableReport> {
    let sqlite_rows = sqlite::count_rows(sqlite, "main", &table.name).await?;
    let pk_columns: Vec<String> = table
        .primary_key()
        .iter()
        .map(|column| column.name.clone())
        .collect();

    let Some(target) = catalog.table(&table.name) else {
        failures.push(Failure {
            schema: "?".to_string(),
            table: table.name.clone(),
            check: "table".to_string(),
            column: None,
            detail: "table exists in the snapshot but not in core/observability".to_string(),
        });
        return Ok(TableReport {
            schema: "?".to_string(),
            table: table.name.clone(),
            rows: RowReport {
                sqlite: sqlite_rows,
                postgres: 0,
                ok: false,
            },
            primary_key: PrimaryKeyReport {
                columns: pk_columns,
                sqlite: String::new(),
                postgres: String::new(),
                ok: false,
            },
            columns: Vec::new(),
            ok: false,
        });
    };

    let postgres_rows = pg_count(pg, target).await?;
    let mut table_report = TableReport {
        schema: target.schema.clone(),
        table: target.name.clone(),
        rows: RowReport {
            sqlite: sqlite_rows,
            postgres: postgres_rows,
            ok: sqlite_rows == postgres_rows,
        },
        primary_key: PrimaryKeyReport {
            columns: pk_columns.clone(),
            sqlite: String::new(),
            postgres: String::new(),
            ok: true,
        },
        columns: Vec::new(),
        ok: true,
    };
    if !table_report.rows.ok {
        failures.push(Failure {
            schema: target.schema.clone(),
            table: target.name.clone(),
            check: "rows".to_string(),
            column: None,
            detail: format!("sqlite={sqlite_rows} postgres={postgres_rows}"),
        });
    }

    if let Err(detail) = check_column_sets(table, target) {
        failures.push(Failure {
            schema: target.schema.clone(),
            table: target.name.clone(),
            check: "column".to_string(),
            column: None,
            detail: detail.clone(),
        });
        table_report.ok = false;
        table_report.primary_key.ok = false;
        table_report.primary_key.sqlite = format!("unavailable: {detail}");
        return Ok(table_report);
    }

    let sqlite_pk = pk_columns.clone();
    let target_pk = target.primary_key.clone();
    if sqlite_pk != target_pk {
        table_report.primary_key.ok = false;
        failures.push(Failure {
            schema: target.schema.clone(),
            table: target.name.clone(),
            check: "primaryKey".to_string(),
            column: None,
            detail: format!("sqlite pk={sqlite_pk:?} postgres pk={target_pk:?}"),
        });
    }
    if sqlite_pk.is_empty() {
        table_report.primary_key.ok = false;
        failures.push(Failure {
            schema: target.schema.clone(),
            table: target.name.clone(),
            check: "primaryKey".to_string(),
            column: None,
            detail: "snapshot table has no primary key".to_string(),
        });
    }

    let sqlite_scan = match scan_sqlite(sqlite, table).await {
        Ok(scan) => scan,
        Err(err) => {
            failures.push(Failure {
                schema: target.schema.clone(),
                table: target.name.clone(),
                check: "column".to_string(),
                column: None,
                detail: err.to_string(),
            });
            table_report.ok = false;
            return Ok(table_report);
        }
    };
    let postgres_scan = match scan_postgres(pg, target).await {
        Ok(scan) => scan,
        Err(err) => {
            failures.push(Failure {
                schema: target.schema.clone(),
                table: target.name.clone(),
                check: "column".to_string(),
                column: None,
                detail: err.to_string(),
            });
            table_report.ok = false;
            return Ok(table_report);
        }
    };

    if table_report.rows.ok
        && (sqlite_scan.rows != table_report.rows.sqlite
            || postgres_scan.rows != table_report.rows.postgres)
    {
        table_report.rows.ok = false;
        failures.push(Failure {
            schema: target.schema.clone(),
            table: target.name.clone(),
            check: "rows".to_string(),
            column: None,
            detail: format!(
                "scan counts sqlite={} postgres={} differ from count(*) sqlite={} postgres={}",
                sqlite_scan.rows,
                postgres_scan.rows,
                table_report.rows.sqlite,
                table_report.rows.postgres
            ),
        });
    }

    table_report.primary_key.sqlite = sqlite_scan.pk_digest.clone();
    table_report.primary_key.postgres = postgres_scan.pk_digest.clone();
    if table_report.primary_key.ok && sqlite_scan.pk_digest != postgres_scan.pk_digest {
        table_report.primary_key.ok = false;
        failures.push(Failure {
            schema: target.schema.clone(),
            table: target.name.clone(),
            check: "primaryKey".to_string(),
            column: None,
            detail: format!(
                "digest sqlite={} postgres={}",
                sqlite_scan.pk_digest, postgres_scan.pk_digest
            ),
        });
    }

    for column in &table.columns {
        let kind = column.kind;
        let left = sqlite_scan.columns.get(&column.name).ok_or_else(|| {
            Error::msg(format!(
                "internal: missing sqlite aggregate {}",
                column.name
            ))
        })?;
        let right = postgres_scan.columns.get(&column.name).ok_or_else(|| {
            Error::msg(format!(
                "internal: missing postgres aggregate {}",
                column.name
            ))
        })?;
        let comparison = left.compare(right);
        if !comparison.ok {
            failures.push(Failure {
                schema: target.schema.clone(),
                table: target.name.clone(),
                check: "column".to_string(),
                column: Some(column.name.clone()),
                detail: comparison
                    .detail
                    .clone()
                    .unwrap_or_else(|| format!("{} aggregate mismatch", kind.as_str())),
            });
        }
        table_report.columns.push(ColumnReport {
            column: column.name.clone(),
            kind,
            ok: comparison.ok,
            detail: comparison.detail,
            tolerance: comparison.tolerance,
            sqlite: left.view(),
            postgres: right.view(),
        });
    }

    table_report.ok = table_report.rows.ok
        && table_report.primary_key.ok
        && table_report.columns.iter().all(|column| column.ok);
    Ok(table_report)
}

fn check_column_sets(table: &SqliteTable, target: &TargetTable) -> std::result::Result<(), String> {
    let sqlite_names: Vec<&str> = table.column_names();
    let target_names: Vec<&str> = target
        .columns
        .iter()
        .map(|column| column.name.as_str())
        .collect();
    if sqlite_names != target_names {
        return Err(format!(
            "column order/set differs: sqlite={sqlite_names:?} postgres={target_names:?}"
        ));
    }
    for column in &table.columns {
        if let Some(target_column) = target.column(&column.name)
            && target_column.kind != column.kind
        {
            return Err(format!(
                "column {}: SQLite {} maps to {} but the target column is {}",
                column.name,
                column.declared_type,
                column.kind.pg_type_name(),
                target_column.kind.pg_type_name()
            ));
        }
    }
    Ok(())
}

async fn pg_count(pg: &mut PgConnection, target: &TargetTable) -> Result<u64> {
    let sql = format!("SELECT count(*) FROM {}", target.qualified());
    let count: i64 = sqlx::query_scalar(&sql)
        .fetch_one(&mut *pg)
        .await
        .map_err(|err| postgres_error(format!("count rows of {}", target.name), err))?;
    Ok(count.max(0) as u64)
}

async fn scan_sqlite(sqlite: &mut SqliteConnection, table: &SqliteTable) -> Result<TableScan> {
    let index_by_name: BTreeMap<String, usize> = table
        .columns
        .iter()
        .enumerate()
        .map(|(index, column)| (column.name.clone(), index))
        .collect();
    let pk_indexes: Vec<usize> = table
        .primary_key()
        .iter()
        .filter_map(|column| index_by_name.get(&column.name).copied())
        .collect();
    let mut states: BTreeMap<String, AggregateState> = table
        .columns
        .iter()
        .map(|column| (column.name.clone(), AggregateState::new(column.kind)))
        .collect();

    let select = sqlite::select_sql("main", table, true);
    let mut reader = sqlx::query(&select).fetch(&mut *sqlite);
    let mut digest = ChunkedDigest::new();
    let mut rows = 0u64;
    while let Some(row) = reader
        .try_next()
        .await
        .map_err(|err| sqlite_error(format!("stream rows of {}", table.name), err))?
    {
        rows += 1;
        let mut values = Vec::with_capacity(table.columns.len());
        for index in 0..table.columns.len() {
            values.push(sqlite::row_value(&row, index)?);
        }
        let mut key = Vec::new();
        for index in &pk_indexes {
            key.extend(encode_primary_key_value(&values[*index]));
        }
        digest.push_row(&key);
        for (name, state) in states.iter_mut() {
            let index = index_by_name
                .get(name)
                .ok_or_else(|| Error::msg(format!("internal: unknown column {name}")))?;
            state.observe(name, &values[*index]).map_err(|detail| {
                Error::msg(format!("table {} column {name}: {detail}", table.name))
            })?;
        }
    }
    Ok(TableScan {
        rows,
        pk_digest: digest.finish(),
        columns: states,
    })
}

async fn scan_postgres(pg: &mut PgConnection, target: &TargetTable) -> Result<TableScan> {
    let index_by_name: BTreeMap<String, usize> = target
        .columns
        .iter()
        .enumerate()
        .map(|(index, column)| (column.name.clone(), index))
        .collect();
    let pk_indexes: Vec<usize> = target
        .primary_key
        .iter()
        .filter_map(|name| index_by_name.get(name).copied())
        .collect();
    let mut states: BTreeMap<String, AggregateState> = target
        .columns
        .iter()
        .map(|column| (column.name.clone(), AggregateState::new(column.kind)))
        .collect();

    let select_list = target
        .columns
        .iter()
        .map(|column| match column.kind {
            ColumnKind::Numeric => format!("{}::text", quote_ident(&column.name)),
            _ => quote_ident(&column.name),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let order_list = target
        .primary_key
        .iter()
        .map(|name| match target.column(name).map(|column| column.kind) {
            Some(ColumnKind::Text) => format!("{} COLLATE \"C\"", quote_ident(name)),
            _ => quote_ident(name),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let mut sql = format!("SELECT {select_list} FROM {}", target.qualified());
    if !order_list.is_empty() {
        sql.push_str(" ORDER BY ");
        sql.push_str(&order_list);
    }

    let mut digest = ChunkedDigest::new();
    let mut rows = 0u64;
    let mut stream = sqlx::query(&sql).fetch(&mut *pg);
    while let Some(row) = stream
        .try_next()
        .await
        .map_err(|err| postgres_error(format!("stream rows of {}", target.name), err))?
    {
        rows += 1;
        let mut values = Vec::with_capacity(target.columns.len());
        for index in 0..target.columns.len() {
            values.push(pg_value(&row, index)?);
        }
        let mut key = Vec::new();
        for index in &pk_indexes {
            key.extend(encode_primary_key_value(&values[*index]));
        }
        digest.push_row(&key);
        for (name, state) in states.iter_mut() {
            let index = index_by_name
                .get(name)
                .ok_or_else(|| Error::msg(format!("internal: unknown column {name}")))?;
            state.observe(name, &values[*index]).map_err(|detail| {
                Error::msg(format!("table {} column {name}: {detail}", target.name))
            })?;
        }
    }
    Ok(TableScan {
        rows,
        pk_digest: digest.finish(),
        columns: states,
    })
}

fn pg_value(row: &sqlx::postgres::PgRow, index: usize) -> Result<SqlValue> {
    use sqlx::{TypeInfo, ValueRef};

    let raw = row
        .try_get_raw(index)
        .map_err(|err| postgres_error(format!("read column {index}"), err))?;
    if raw.is_null() {
        return Ok(SqlValue::Null);
    }
    match raw.type_info().name() {
        "INT8" | "INT4" | "INT2" => row
            .try_get::<i64, _>(index)
            .map(SqlValue::Integer)
            .map_err(|err| postgres_error(format!("decode integer column {index}"), err)),
        "FLOAT8" | "FLOAT4" => row
            .try_get::<f64, _>(index)
            .map(SqlValue::Real)
            .map_err(|err| postgres_error(format!("decode float column {index}"), err)),
        "TEXT" | "VARCHAR" | "BPCHAR" | "NAME" => row
            .try_get::<String, _>(index)
            .map(SqlValue::Text)
            .map_err(|err| postgres_error(format!("decode text column {index}"), err)),
        "BYTEA" => row
            .try_get::<Vec<u8>, _>(index)
            .map(SqlValue::Bytes)
            .map_err(|err| postgres_error(format!("decode bytea column {index}"), err)),
        other => Err(Error::msg(format!(
            "PostgreSQL returned unsupported type {other} for column {index}"
        ))),
    }
}

fn canonical_snapshot(path: &Path) -> Result<PathBuf> {
    if !path.exists() {
        return Err(Error::msg(format!(
            "snapshot {} does not exist",
            path.display()
        )));
    }
    std::fs::canonicalize(path)
        .map_err(|err| Error::msg(format!("resolve {}: {err}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::check_column_sets;
    use crate::sqlite::{SqliteColumn, SqliteTable};
    use crate::types::ColumnKind;

    fn table() -> SqliteTable {
        SqliteTable {
            name: "api_keys".to_string(),
            columns: vec![SqliteColumn {
                name: "id".to_string(),
                declared_type: "TEXT".to_string(),
                kind: ColumnKind::Text,
                primary_key_ordinal: Some(1),
            }],
        }
    }

    fn target(kind: ColumnKind) -> crate::catalog::TargetTable {
        crate::catalog::TargetTable {
            schema: "core".to_string(),
            name: "api_keys".to_string(),
            columns: vec![crate::catalog::TargetColumn {
                name: "id".to_string(),
                kind,
                is_identity: false,
            }],
            primary_key: vec!["id".to_string()],
        }
    }

    #[test]
    fn column_set_checks_accept_matching_schema() {
        assert!(check_column_sets(&table(), &target(ColumnKind::Text)).is_ok());
    }

    #[test]
    fn column_set_checks_reject_kind_drift() {
        let error = check_column_sets(&table(), &target(ColumnKind::Integer)).unwrap_err();
        assert!(error.contains("maps to text but the target column is bigint"));
    }

    #[test]
    fn column_set_checks_reject_reordering() {
        let mut sqlite = table();
        sqlite.columns.push(SqliteColumn {
            name: "name".to_string(),
            declared_type: "TEXT".to_string(),
            kind: ColumnKind::Text,
            primary_key_ordinal: None,
        });
        let error = check_column_sets(&sqlite, &target(ColumnKind::Text)).unwrap_err();
        assert!(error.contains("column order/set differs"));
    }
}
