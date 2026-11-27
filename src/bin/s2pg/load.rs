//! `load`: apply the PG migrations, then stream every snapshot table over `COPY`.
//!
//! Column order and target types come from `PRAGMA table_info` on the snapshot and are matched
//! by name against `information_schema.columns`; there is no second DDL copy in the tool.
//!
//! Failure handling: the first row that cannot be represented exactly aborts the COPY, rolls
//! back the load transaction and reports the table plus primary key. Tables are loaded in
//! foreign key order, so constraints stay enabled for the whole load.
//!
//! User triggers are suspended for the duration of the load. The snapshot already contains the
//! rows those triggers maintain — `core.upstream_reconciliation_work` is derived from
//! `core.upstream_reconciliation_usage`, and the observability revision tables are derived from
//! `observability.request_logs` — so replaying them would insert the same derived rows twice and
//! abort the COPY on a primary-key conflict. Suspension is transactional DDL: a failed load
//! rolls back and leaves every trigger enabled.
//!
//! `--reset` drops `core` and `observability` and clears this tool's `_sqlx_migrations`
//! bookkeeping; without the latter `sqlx::migrate!` would consider the baseline already
//! applied and leave the schemas empty.

use std::collections::BTreeMap;
use std::path::PathBuf;

use futures_util::TryStreamExt;
use sqlx::{Connection, PgConnection};

use crate::catalog::{self, Catalog, TargetTable};
use crate::cli::LoadArgs;
use crate::error::{Error, Result, postgres_error, sqlite_error};
use crate::sqlite::{self, SqliteTable, quote_ident};
use crate::types::{ColumnKind, SqlValue, copy_field};

const COPY_BUFFER_BYTES: usize = 64 * 1024;
const NON_EMPTY_HINT_ROWS: i64 = 10;

#[derive(Debug)]
pub struct LoadSummary {
    pub tables: usize,
    pub rows: u64,
    pub sequences: usize,
}

#[derive(Debug)]
struct CopyPlan {
    table: SqliteTable,
    target: TargetTable,
    kinds: Vec<ColumnKind>,
    column_indexes: BTreeMap<String, usize>,
}

pub async fn run(args: &LoadArgs) -> Result<LoadSummary> {
    let snapshot = canonical_snapshot(&args.snapshot)?;
    let mut sqlite = sqlite::open_read_only(&snapshot).await?;
    let mut snapshot_tables = Vec::new();
    for name in sqlite::list_tables(&mut sqlite, "main").await? {
        snapshot_tables.push(sqlite::read_table(&mut sqlite, "main", &name).await?);
    }

    let mut pg = PgConnection::connect(&args.database_url)
        .await
        .map_err(|err| Error::msg(format!("connect to PostgreSQL: {err}")))?;

    preflight_target(&mut pg, args.reset).await?;
    if args.reset {
        catalog::reset_target(&mut pg).await?;
    }
    apply_migrations(&mut pg).await?;

    let target_catalog = catalog::load_catalog(&mut pg).await?;
    let plans = build_plans(&snapshot_tables, &target_catalog)?;

    let names: Vec<String> = plans.iter().map(|plan| plan.table.name.clone()).collect();
    let ordered = catalog::foreign_key_order(&mut pg, &names).await?;
    let plan_by_name: BTreeMap<&str, &CopyPlan> = plans
        .iter()
        .map(|plan| (plan.table.name.as_str(), plan))
        .collect();

    sqlx::query("BEGIN")
        .execute(&mut pg)
        .await
        .map_err(|err| postgres_error("begin load transaction", err))?;

    let outcome = async {
        let suspended = suspend_triggers(&mut pg).await?;
        let rows = copy_all(&mut pg, &mut sqlite, &ordered, &plan_by_name).await?;
        resume_triggers(&mut pg, &suspended).await?;
        Ok::<u64, Error>(rows)
    }
    .await;
    let rows = match outcome {
        Ok(rows) => {
            sqlx::query("COMMIT")
                .execute(&mut pg)
                .await
                .map_err(|err| postgres_error("commit load transaction", err))?;
            rows
        }
        Err(err) => {
            let _ = sqlx::query("ROLLBACK").execute(&mut pg).await;
            return Err(err);
        }
    };

    let sequences = align_sequences(&mut pg, &target_catalog).await?;
    Ok(LoadSummary {
        tables: plans.len(),
        rows,
        sequences,
    })
}

/// Refuses to write into a target that already contains managed tables.
pub async fn preflight_target(pg: &mut PgConnection, reset: bool) -> Result<()> {
    let count = catalog::target_table_count(pg).await?;
    if require_empty_target(count, reset).is_ok() {
        return Ok(());
    }
    let existing: Vec<String> = sqlx::query_scalar(
        "SELECT table_schema || '.' || table_name FROM information_schema.tables \
         WHERE table_schema = ANY($1) AND table_type = 'BASE TABLE' \
         ORDER BY 1 LIMIT $2",
    )
    .bind(&catalog::SCHEMAS[..])
    .bind(NON_EMPTY_HINT_ROWS)
    .fetch_all(&mut *pg)
    .await
    .map_err(|err| postgres_error("list existing target tables", err))?;
    Err(Error::msg(format!(
        "target database is not empty ({count} tables in core/observability: {}); \
         pass --reset to drop both schemas first",
        existing.join(", ")
    )))
}

/// Pure policy kept separate from the catalog probe so it is unit-testable without a server.
pub fn require_empty_target(table_count: i64, reset: bool) -> Result<()> {
    if table_count > 0 && !reset {
        return Err(Error::msg(format!(
            "target database is not empty ({table_count} tables in core/observability)"
        )));
    }
    Ok(())
}

async fn apply_migrations(pg: &mut PgConnection) -> Result<()> {
    sqlx::migrate!("src/store/pg/migrations")
        .run(&mut *pg)
        .await
        .map_err(|err| Error::msg(format!("apply PostgreSQL migrations: {err}")))
}

/// Suspends every user trigger in the managed schemas and returns the relations it touched.
async fn suspend_triggers(pg: &mut PgConnection) -> Result<Vec<String>> {
    let relations: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT n.nspname || '.' || c.relname \
           FROM pg_trigger t \
           JOIN pg_class c ON c.oid = t.tgrelid \
           JOIN pg_namespace n ON n.oid = c.relnamespace \
          WHERE NOT t.tgisinternal AND n.nspname = ANY($1) \
          ORDER BY 1",
    )
    .bind(&catalog::SCHEMAS[..])
    .fetch_all(&mut *pg)
    .await
    .map_err(|err| postgres_error("list trigger relations", err))?;
    for relation in &relations {
        sqlx::query(&format!("ALTER TABLE {relation} DISABLE TRIGGER USER"))
            .execute(&mut *pg)
            .await
            .map_err(|err| postgres_error(format!("suspend triggers on {relation}"), err))?;
    }
    Ok(relations)
}

async fn resume_triggers(pg: &mut PgConnection, relations: &[String]) -> Result<()> {
    for relation in relations {
        sqlx::query(&format!("ALTER TABLE {relation} ENABLE TRIGGER USER"))
            .execute(&mut *pg)
            .await
            .map_err(|err| postgres_error(format!("resume triggers on {relation}"), err))?;
    }
    Ok(())
}

fn build_plans(tables: &[SqliteTable], catalog: &Catalog) -> Result<Vec<CopyPlan>> {
    let mut plans = Vec::with_capacity(tables.len());
    for table in tables {
        let target = catalog.table(&table.name).ok_or_else(|| {
            Error::msg(format!(
                "snapshot table {} has no table in core/observability; \
                 migrate the target or rerun with --reset",
                table.name
            ))
        })?;
        plans.push(build_plan(table, target)?);
    }
    Ok(plans)
}

fn build_plan(table: &SqliteTable, target: &TargetTable) -> Result<CopyPlan> {
    let mut kinds = Vec::with_capacity(table.columns.len());
    let mut indexes = BTreeMap::new();
    for (index, column) in table.columns.iter().enumerate() {
        let target_column = target.column(&column.name).ok_or_else(|| {
            Error::msg(format!(
                "table {}.{} column {} does not exist in the target; \
                 SQLite declared type {}",
                target.schema, target.name, column.name, column.declared_type
            ))
        })?;
        if target_column.kind != column.kind {
            return Err(Error::msg(format!(
                "table {}.{} column {}: SQLite type {} maps to {} but the target column is {}",
                target.schema,
                target.name,
                column.name,
                column.declared_type,
                column.kind.pg_type_name(),
                target_column.kind.pg_type_name()
            )));
        }
        kinds.push(column.kind);
        indexes.insert(column.name.clone(), index);
    }
    for target_column in &target.columns {
        if !indexes.contains_key(&target_column.name) {
            return Err(Error::msg(format!(
                "table {}.{} target column {} has no SQLite source column",
                target.schema, target.name, target_column.name
            )));
        }
    }
    Ok(CopyPlan {
        table: table.clone(),
        target: target.clone(),
        kinds,
        column_indexes: indexes,
    })
}

async fn copy_all(
    pg: &mut PgConnection,
    sqlite: &mut sqlx::SqliteConnection,
    ordered: &[String],
    plans: &BTreeMap<&str, &CopyPlan>,
) -> Result<u64> {
    let mut rows = 0u64;
    for name in ordered {
        let plan = plans
            .get(name.as_str())
            .ok_or_else(|| Error::msg(format!("internal: missing copy plan for {name}")))?;
        let copied = copy_table(pg, sqlite, plan).await?;
        println!(
            "load: table {}.{} rows={copied}",
            plan.target.schema, plan.target.name
        );
        rows += copied;
    }
    Ok(rows)
}

async fn copy_table(
    pg: &mut PgConnection,
    sqlite: &mut sqlx::SqliteConnection,
    plan: &CopyPlan,
) -> Result<u64> {
    let columns = plan
        .table
        .columns
        .iter()
        .map(|column| quote_ident(&column.name))
        .collect::<Vec<_>>()
        .join(", ");
    let copy_sql = format!("COPY {} ({columns}) FROM STDIN", plan.target.qualified());
    let mut copy = pg
        .copy_in_raw(&copy_sql)
        .await
        .map_err(|err| postgres_error(format!("start COPY for {}", plan.target.name), err))?;
    let mut buffer: Vec<u8> = Vec::with_capacity(COPY_BUFFER_BYTES);

    let streamed = stream_rows(&mut copy, sqlite, plan, &mut buffer).await;
    let streamed = match streamed {
        Ok(streamed) => streamed,
        Err(err) => {
            let _ = copy.abort(err.to_string()).await;
            return Err(err);
        }
    };
    let affected = copy
        .finish()
        .await
        .map_err(|err| postgres_error(format!("finish COPY for {}", plan.target.name), err))?;
    if affected != streamed {
        return Err(Error::msg(format!(
            "table {}.{}: COPY reported {affected} rows but {streamed} rows were streamed",
            plan.target.schema, plan.target.name
        )));
    }
    Ok(streamed)
}

async fn stream_rows(
    copy: &mut sqlx::postgres::PgCopyIn<&mut PgConnection>,
    sqlite: &mut sqlx::SqliteConnection,
    plan: &CopyPlan,
    buffer: &mut Vec<u8>,
) -> Result<u64> {
    let select = sqlite::select_sql("main", &plan.table, false);
    let mut rows = sqlx::query(&select).fetch(&mut *sqlite);
    let mut streamed = 0u64;
    while let Some(row) = rows
        .try_next()
        .await
        .map_err(|err| sqlite_error(format!("stream rows of {}", plan.table.name), err))?
    {
        streamed += 1;
        let mut values = Vec::with_capacity(plan.table.columns.len());
        for index in 0..plan.table.columns.len() {
            values.push(sqlite::row_value(&row, index)?);
        }
        for (index, column) in plan.table.columns.iter().enumerate() {
            if index > 0 {
                buffer.push(b'\t');
            }
            let field = copy_field(plan.kinds[index], &values[index]).map_err(|detail| {
                Error::msg(format!(
                    "table {}.{} pk[{}] column {}: {detail}",
                    plan.target.schema,
                    plan.target.name,
                    render_primary_key(plan, &values, streamed),
                    column.name
                ))
            })?;
            buffer.extend_from_slice(field.as_bytes());
        }
        buffer.push(b'\n');
        if buffer.len() >= COPY_BUFFER_BYTES {
            copy.send(std::mem::take(buffer)).await.map_err(|err| {
                postgres_error(format!("stream COPY for {}", plan.target.name), err)
            })?;
            buffer.reserve(COPY_BUFFER_BYTES);
        }
    }
    if !buffer.is_empty() {
        copy.send(std::mem::take(buffer))
            .await
            .map_err(|err| postgres_error(format!("stream COPY for {}", plan.target.name), err))?;
    }
    Ok(streamed)
}

fn render_primary_key(plan: &CopyPlan, values: &[SqlValue], row_number: u64) -> String {
    let primary_key = plan.table.primary_key();
    if primary_key.is_empty() {
        return format!("#{row_number}");
    }
    primary_key
        .iter()
        .filter_map(|column| {
            let index = plan.column_indexes.get(&column.name)?;
            Some(format!(
                "{}={}",
                column.name,
                values.get(*index)?.render_debug()
            ))
        })
        .collect::<Vec<_>>()
        .join(",")
}

async fn align_sequences(pg: &mut PgConnection, target_catalog: &Catalog) -> Result<usize> {
    let mut aligned = 0;
    for target in target_catalog.tables() {
        for identity in target.columns.iter().filter(|column| column.is_identity) {
            align_sequence(pg, target, &identity.name).await?;
            aligned += 1;
        }
    }
    Ok(aligned)
}

async fn align_sequence(pg: &mut PgConnection, target: &TargetTable, column: &str) -> Result<()> {
    let schema = target.schema.as_str();
    let table = target.name.as_str();
    let max: Option<i64> = sqlx::query_scalar(&format!(
        "SELECT max({}) FROM {}.{}",
        quote_ident(column),
        quote_ident(schema),
        quote_ident(table)
    ))
    .fetch_one(&mut *pg)
    .await
    .map_err(|err| postgres_error(format!("read max({column}) of {schema}.{table}"), err))?;
    let sequence: Option<String> = sqlx::query_scalar("SELECT pg_get_serial_sequence($1, $2)")
        .bind(format!("{}.{}", quote_ident(schema), quote_ident(table)))
        .bind(column)
        .fetch_one(&mut *pg)
        .await
        .map_err(|err| {
            postgres_error(
                format!("resolve sequence of {schema}.{table}.{column}"),
                err,
            )
        })?;
    let sequence = sequence.ok_or_else(|| {
        Error::msg(format!(
            "identity column {schema}.{table}.{column} has no sequence"
        ))
    })?;
    // `setval(seq, 0, true)` is out of bounds, so an empty table (or one whose ids are all
    // non-positive) restarts the sequence at 1 with `is_called = false`.
    let (value, is_called) = match max {
        Some(max) if max >= 1 => (max, true),
        _ => (1, false),
    };
    sqlx::query("SELECT setval($1::regclass, $2, $3)")
        .bind(&sequence)
        .bind(value)
        .bind(is_called)
        .execute(&mut *pg)
        .await
        .map_err(|err| postgres_error(format!("setval {sequence}"), err))?;
    Ok(())
}

fn canonical_snapshot(path: &std::path::Path) -> Result<PathBuf> {
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
    use std::collections::BTreeMap;

    use super::{CopyPlan, build_plan, render_primary_key, require_empty_target};
    use crate::catalog::{TargetColumn, TargetTable};
    use crate::sqlite::{SqliteColumn, SqliteTable};
    use crate::types::{ColumnKind, SqlValue};

    fn sqlite_table() -> SqliteTable {
        SqliteTable {
            name: "api_keys".to_string(),
            columns: vec![
                SqliteColumn {
                    name: "id".to_string(),
                    declared_type: "TEXT".to_string(),
                    kind: ColumnKind::Text,
                    primary_key_ordinal: Some(1),
                },
                SqliteColumn {
                    name: "quota_remaining".to_string(),
                    declared_type: "INTEGER".to_string(),
                    kind: ColumnKind::Integer,
                    primary_key_ordinal: None,
                },
            ],
        }
    }

    fn target_table(quota_kind: ColumnKind) -> TargetTable {
        TargetTable {
            schema: "core".to_string(),
            name: "api_keys".to_string(),
            columns: vec![
                TargetColumn {
                    name: "id".to_string(),
                    kind: ColumnKind::Text,
                    is_identity: false,
                },
                TargetColumn {
                    name: "quota_remaining".to_string(),
                    kind: quota_kind,
                    is_identity: false,
                },
            ],
            primary_key: vec!["id".to_string()],
        }
    }

    #[test]
    fn non_empty_target_is_rejected_without_reset() {
        assert!(require_empty_target(0, false).is_ok());
        let error = require_empty_target(3, false).unwrap_err();
        assert!(error.to_string().contains("not empty (3 tables"));
        assert!(require_empty_target(3, true).is_ok());
    }

    #[test]
    fn target_type_mismatch_is_rejected() {
        let error = build_plan(&sqlite_table(), &target_table(ColumnKind::Text)).unwrap_err();
        assert!(error.to_string().contains(
            "quota_remaining: SQLite type INTEGER maps to bigint but the target column is text"
        ));
        assert!(build_plan(&sqlite_table(), &target_table(ColumnKind::Integer)).is_ok());
    }

    #[test]
    fn missing_target_or_source_columns_are_rejected() {
        let mut target = target_table(ColumnKind::Integer);
        target.columns.pop();
        let error = build_plan(&sqlite_table(), &target).unwrap_err();
        assert!(error.to_string().contains("does not exist in the target"));

        let mut target = target_table(ColumnKind::Integer);
        target.columns.push(TargetColumn {
            name: "extra".to_string(),
            kind: ColumnKind::Text,
            is_identity: false,
        });
        let error = build_plan(&sqlite_table(), &target).unwrap_err();
        assert!(error.to_string().contains("has no SQLite source column"));
    }

    #[test]
    fn primary_key_is_rendered_for_failures() {
        let plan: CopyPlan =
            build_plan(&sqlite_table(), &target_table(ColumnKind::Integer)).unwrap();
        let values = vec![SqlValue::Text("key-1".to_string()), SqlValue::Integer(4)];
        let mut indexes = BTreeMap::new();
        indexes.insert("id".to_string(), 0usize);
        let mut plan = plan;
        plan.column_indexes = indexes;
        assert_eq!(render_primary_key(&plan, &values, 1), "id=\"key-1\"");
    }
}
