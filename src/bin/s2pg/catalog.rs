//! PostgreSQL target introspection.
//!
//! Table names, column order, column kinds, primary keys and foreign keys are read from the
//! target catalog. The tool never carries a second copy of the DDL, so load and verify always
//! agree with whatever migrations the target actually has.

use std::collections::{BTreeMap, BTreeSet};

use sqlx::{PgConnection, Row};

use crate::error::{Error, Result, postgres_error};
use crate::types::ColumnKind;

pub const SCHEMAS: [&str; 2] = ["core", "observability"];

#[derive(Debug, Clone)]
pub struct TargetColumn {
    pub name: String,
    pub kind: ColumnKind,
    pub is_identity: bool,
}

#[derive(Debug, Clone)]
pub struct TargetTable {
    pub schema: String,
    pub name: String,
    pub columns: Vec<TargetColumn>,
    pub primary_key: Vec<String>,
}

impl TargetTable {
    pub fn qualified(&self) -> String {
        format!(
            "{}.{}",
            crate::sqlite::quote_ident(&self.schema),
            crate::sqlite::quote_ident(&self.name)
        )
    }

    pub fn column(&self, name: &str) -> Option<&TargetColumn> {
        self.columns.iter().find(|column| column.name == name)
    }
}

#[derive(Debug, Clone)]
pub struct Catalog {
    tables: BTreeMap<String, TargetTable>,
}

impl Catalog {
    pub fn table(&self, name: &str) -> Option<&TargetTable> {
        self.tables.get(name)
    }

    pub fn tables(&self) -> impl Iterator<Item = &TargetTable> {
        self.tables.values()
    }
}

pub async fn load_catalog(conn: &mut PgConnection) -> Result<Catalog> {
    let rows = sqlx::query(
        "SELECT table_schema, table_name, column_name, data_type, is_identity \
         FROM information_schema.columns \
         WHERE table_schema = ANY($1) \
         ORDER BY table_schema, table_name, ordinal_position",
    )
    .bind(&SCHEMAS[..])
    .fetch_all(&mut *conn)
    .await
    .map_err(|err| postgres_error("read information_schema.columns", err))?;

    let mut tables: BTreeMap<(String, String), TargetTable> = BTreeMap::new();
    for row in rows {
        let schema: String = row.try_get("table_schema").map_err(pg_row_error)?;
        let table: String = row.try_get("table_name").map_err(pg_row_error)?;
        let column: String = row.try_get("column_name").map_err(pg_row_error)?;
        let data_type: String = row.try_get("data_type").map_err(pg_row_error)?;
        let is_identity: String = row.try_get("is_identity").map_err(pg_row_error)?;
        let kind = ColumnKind::from_pg_data_type(&data_type)
            .map_err(|err| Error::msg(format!("target column {schema}.{table}.{column}: {err}")))?;
        tables
            .entry((schema.clone(), table.clone()))
            .or_insert_with(|| TargetTable {
                schema,
                name: table,
                columns: Vec::new(),
                primary_key: Vec::new(),
            })
            .columns
            .push(TargetColumn {
                name: column,
                kind,
                is_identity: is_identity == "YES",
            });
    }

    let primary_key_rows = sqlx::query(
        "SELECT n.nspname AS schema_name, c.relname AS table_name, a.attname AS column_name \
         FROM pg_index i \
         JOIN pg_class c ON c.oid = i.indrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         CROSS JOIN LATERAL generate_series(0, i.indnkeyatts - 1) AS key(ord) \
         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = i.indkey[key.ord] \
         WHERE i.indisprimary AND n.nspname = ANY($1) \
         ORDER BY n.nspname, c.relname, key.ord",
    )
    .bind(&SCHEMAS[..])
    .fetch_all(&mut *conn)
    .await
    .map_err(|err| postgres_error("read primary keys", err))?;
    for row in primary_key_rows {
        let schema: String = row.try_get("schema_name").map_err(pg_row_error)?;
        let table: String = row.try_get("table_name").map_err(pg_row_error)?;
        let column: String = row.try_get("column_name").map_err(pg_row_error)?;
        if let Some(target) = tables.get_mut(&(schema, table)) {
            target.primary_key.push(column);
        }
    }

    let mut by_name: BTreeMap<String, TargetTable> = BTreeMap::new();
    for table in tables.into_values() {
        if let Some(existing) = by_name.get(&table.name) {
            return Err(Error::msg(format!(
                "table name {} exists in both {}.{} and {}.{}; snapshot tables cannot be mapped unambiguously",
                table.name, existing.schema, existing.name, table.schema, table.name
            )));
        }
        by_name.insert(table.name.clone(), table);
    }
    Ok(Catalog { tables: by_name })
}

fn pg_row_error(err: sqlx::Error) -> Error {
    Error::msg(format!("decode catalog row: {err}"))
}

/// Returns the number of user tables currently present in `core` and `observability`.
pub async fn target_table_count(conn: &mut PgConnection) -> Result<i64> {
    sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.tables \
         WHERE table_schema = ANY($1) AND table_type = 'BASE TABLE'",
    )
    .bind(&SCHEMAS[..])
    .fetch_one(&mut *conn)
    .await
    .map_err(|err| postgres_error("count target tables", err))
}

/// Drops both managed schemas and forgets applied migrations so `migrate` can rebuild them.
pub async fn reset_target(conn: &mut PgConnection) -> Result<()> {
    for schema in SCHEMAS {
        let sql = format!(
            "DROP SCHEMA IF EXISTS {} CASCADE",
            crate::sqlite::quote_ident(schema)
        );
        sqlx::query(&sql)
            .execute(&mut *conn)
            .await
            .map_err(|err| postgres_error(format!("drop schema {schema}"), err))?;
    }
    let migration_table_exists: bool =
        sqlx::query_scalar("SELECT to_regclass('public._sqlx_migrations') IS NOT NULL")
            .fetch_one(&mut *conn)
            .await
            .map_err(|err| postgres_error("probe _sqlx_migrations", err))?;
    if migration_table_exists {
        sqlx::query("DELETE FROM _sqlx_migrations")
            .execute(&mut *conn)
            .await
            .map_err(|err| postgres_error("clear _sqlx_migrations", err))?;
    }
    Ok(())
}

/// Orders snapshot tables so foreign key parents load first. Self references are ignored; a
/// cycle across distinct tables is reported instead of being silently broken.
pub async fn foreign_key_order(
    conn: &mut PgConnection,
    snapshot_tables: &[String],
) -> Result<Vec<String>> {
    let rows = sqlx::query(
        "SELECT cn.nspname AS child_schema, c.relname AS child_table, \
                pn.nspname AS parent_schema, p.relname AS parent_table \
         FROM pg_constraint con \
         JOIN pg_class c ON c.oid = con.conrelid \
         JOIN pg_namespace cn ON cn.oid = c.relnamespace \
         JOIN pg_class p ON p.oid = con.confrelid \
         JOIN pg_namespace pn ON pn.oid = p.relnamespace \
         WHERE con.contype = 'f' AND cn.nspname = ANY($1) AND pn.nspname = ANY($1)",
    )
    .bind(&SCHEMAS[..])
    .fetch_all(&mut *conn)
    .await
    .map_err(|err| postgres_error("read foreign keys", err))?;

    let present: BTreeSet<&str> = snapshot_tables.iter().map(String::as_str).collect();
    let mut children: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for table in snapshot_tables {
        children.insert(table.clone(), BTreeSet::new());
    }
    for row in rows {
        let child: String = row.try_get("child_table").map_err(pg_row_error)?;
        let parent: String = row.try_get("parent_table").map_err(pg_row_error)?;
        if child == parent
            || !present.contains(child.as_str())
            || !present.contains(parent.as_str())
        {
            continue;
        }
        children.entry(child).or_default().insert(parent);
    }

    let mut ordered = Vec::with_capacity(snapshot_tables.len());
    let mut remaining: BTreeSet<String> = snapshot_tables.iter().cloned().collect();
    while !remaining.is_empty() {
        let ready: Vec<String> = remaining
            .iter()
            .filter(|table| {
                children
                    .get(*table)
                    .is_none_or(|parents| parents.iter().all(|parent| !remaining.contains(parent)))
            })
            .cloned()
            .collect();
        if ready.is_empty() {
            let cycle: Vec<&str> = remaining.iter().map(String::as_str).collect();
            return Err(Error::msg(format!(
                "foreign key cycle between snapshot tables: {}",
                cycle.join(", ")
            )));
        }
        for table in ready {
            remaining.remove(&table);
            ordered.push(table);
        }
    }
    Ok(ordered)
}
