//! `snapshot`: one consistent copy of the SQLite core database (plus its observability
//! sidecar when present) with a manifest that identifies it by sha256.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sqlx::Connection;

use crate::cli::SnapshotArgs;
use crate::error::{Error, Result};
use crate::hash::sha256_file;
use crate::manifest::{ManifestTable, SnapshotManifest, SnapshotSource};
use crate::sqlite;

const OBSERVABILITY_ALIAS: &str = "s2pg_observability";

#[derive(Debug)]
pub struct SnapshotSummary {
    pub manifest: SnapshotManifest,
    pub manifest_path: PathBuf,
}

pub async fn run(args: &SnapshotArgs) -> Result<SnapshotSummary> {
    let core_path = canonical_source(&args.sqlite)?;
    let out_path = reserve_output(&args.out)?;
    let manifest_path = SnapshotManifest::manifest_path(&out_path);
    if manifest_path.exists() {
        return Err(Error::msg(format!(
            "refusing to overwrite existing manifest {}",
            manifest_path.display()
        )));
    }
    let observability_path = sqlite::resolve_observability_path(&core_path)?;

    let workdir = tempfile::Builder::new()
        .prefix(".s2pg-snapshot-")
        .tempdir_in(out_path.parent().unwrap_or_else(|| Path::new(".")))
        .map_err(|err| {
            Error::msg(format!(
                "create work directory next to {}: {err}",
                out_path.display()
            ))
        })?;

    let mut core = sqlite::open_read_only(&core_path).await?;
    let user_version = sqlite::user_version(&mut core).await?;
    let mut observability_tables = Vec::new();
    let mut observability_user_version = None;
    if let Some(sidecar) = observability_path.as_deref() {
        let mut observability = sqlite::open_read_only(sidecar).await?;
        observability_user_version = Some(sqlite::user_version(&mut observability).await?);
        observability_tables = sqlite::list_tables(&mut observability, "main").await?;
        observability_tables.sort();
        let observability_copy = workdir.path().join("observability-copy.db");
        sqlite::vacuum_into(&mut observability, &observability_copy).await?;
        observability.close().await.ok();

        let merged = workdir.path().join("merged.db");
        sqlite::vacuum_into(&mut core, &merged).await?;
        core.close().await.ok();
        let mut merged_conn = sqlite::open_read_write(&merged).await?;
        sqlite::attach(&mut merged_conn, &observability_copy, OBSERVABILITY_ALIAS).await?;
        merge_tables(&mut merged_conn).await?;
        sqlite::detach(&mut merged_conn, OBSERVABILITY_ALIAS).await?;
        let final_copy = workdir.path().join("final.db");
        sqlite::vacuum_into(&mut merged_conn, &final_copy).await?;
        merged_conn.close().await.ok();
        publish(&final_copy, &out_path)?;
    } else {
        let final_copy = workdir.path().join("final.db");
        sqlite::vacuum_into(&mut core, &final_copy).await?;
        core.close().await.ok();
        publish(&final_copy, &out_path)?;
    }

    let (bytes, snapshot_id) = sha256_file(&out_path)?;
    let mut snapshot = sqlite::open_read_only(&out_path).await?;
    let tables = sqlite::list_tables(&mut snapshot, "main").await?;
    let mut manifest_tables = Vec::with_capacity(tables.len());
    let mut total_rows = 0u64;
    for table in tables {
        let rows = sqlite::count_rows(&mut snapshot, "main", &table).await?;
        total_rows += rows;
        manifest_tables.push(ManifestTable {
            source: if observability_tables.contains(&table) {
                "observability".to_string()
            } else {
                "core".to_string()
            },
            name: table,
            rows,
        });
    }
    snapshot.close().await.ok();

    let manifest = SnapshotManifest {
        snapshot_id,
        path: display_path(&out_path),
        bytes,
        created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        source: SnapshotSource {
            sqlite_path: display_path(&core_path),
            observability_path: observability_path.as_deref().map(display_path),
            user_version,
            observability_user_version,
        },
        tables: manifest_tables,
        total_rows,
    };
    let manifest_path = manifest.write(&out_path)?;
    Ok(SnapshotSummary {
        manifest,
        manifest_path,
    })
}

async fn merge_tables(conn: &mut sqlx::SqliteConnection) -> Result<()> {
    use sqlx::Row;

    let rows = sqlx::query(
        "SELECT name, sql FROM s2pg_observability.sqlite_master \
         WHERE type = 'table' AND sql IS NOT NULL AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
         ORDER BY name",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(|err| crate::error::sqlite_error("read observability schema", err))?;

    let mut seen = BTreeSet::new();
    for row in rows {
        let name: String = row
            .try_get("name")
            .map_err(|err| crate::error::sqlite_error("decode observability table name", err))?;
        let ddl: String = row
            .try_get("sql")
            .map_err(|err| crate::error::sqlite_error("decode observability DDL", err))?;
        if !seen.insert(name.clone()) {
            return Err(Error::msg(format!(
                "duplicate observability table {name} in snapshot merge"
            )));
        }
        let exists: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM main.sqlite_master WHERE type = 'table' AND name = ?",
        )
        .bind(&name)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|err| crate::error::sqlite_error(format!("probe table {name}"), err))?;
        if exists.is_some() {
            return Err(Error::msg(format!(
                "table {name} exists in both the core and observability SQLite files; \
                 cannot build an unambiguous snapshot"
            )));
        }
        sqlx::query(&ddl).execute(&mut *conn).await.map_err(|err| {
            crate::error::sqlite_error(format!("create snapshot table {name}"), err)
        })?;
        let copy = format!(
            "INSERT INTO main.{} SELECT * FROM s2pg_observability.{}",
            sqlite::quote_ident(&name),
            sqlite::quote_ident(&name)
        );
        sqlx::query(&copy)
            .execute(&mut *conn)
            .await
            .map_err(|err| {
                crate::error::sqlite_error(format!("copy observability table {name}"), err)
            })?;
    }
    Ok(())
}

fn canonical_source(path: &Path) -> Result<PathBuf> {
    if !path.exists() {
        return Err(Error::msg(format!(
            "SQLite database {} does not exist",
            path.display()
        )));
    }
    std::fs::canonicalize(path)
        .map_err(|err| Error::msg(format!("resolve {}: {err}", path.display())))
}

/// Resolves the output path without creating it, refusing an existing file.
fn reserve_output(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return Err(Error::msg(format!(
            "refusing to overwrite existing snapshot {}",
            path.display()
        )));
    }
    let file_name = path
        .file_name()
        .ok_or_else(|| Error::msg(format!("invalid snapshot path {}", path.display())))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(Error::msg(format!(
            "snapshot directory {} does not exist",
            parent.display()
        )));
    }
    let parent = std::fs::canonicalize(parent)
        .map_err(|err| Error::msg(format!("resolve {}: {err}", parent.display())))?;
    Ok(parent.join(file_name))
}

fn publish(work_copy: &Path, out_path: &Path) -> Result<()> {
    std::fs::rename(work_copy, out_path).map_err(|err| {
        Error::msg(format!(
            "publish snapshot {} -> {}: {err}",
            work_copy.display(),
            out_path.display()
        ))
    })
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use sqlx::sqlite::SqliteConnectOptions;
    use sqlx::{Connection, SqliteConnection};

    use super::run;
    use crate::cli::SnapshotArgs;
    use crate::sqlite;

    async fn open_fixture(path: &Path) -> SqliteConnection {
        SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true),
        )
        .await
        .unwrap()
    }

    async fn create_fixture(path: &Path) {
        let mut conn = open_fixture(path).await;
        sqlx::query(
            "CREATE TABLE api_keys (id TEXT PRIMARY KEY, quota_remaining INTEGER, weight REAL, \
             payload BLOB, created_at INTEGER NOT NULL)",
        )
        .execute(&mut conn)
        .await
        .unwrap();
        for index in 0..5 {
            sqlx::query(
                "INSERT INTO api_keys (id, quota_remaining, weight, payload, created_at) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(format!("key-{index}"))
            .bind(index * 10)
            .bind(index as f64 + 0.5)
            .bind(vec![index as u8])
            .bind(1_700_000_000i64 + index)
            .execute(&mut conn)
            .await
            .unwrap();
        }
        sqlx::query("PRAGMA user_version = 7")
            .execute(&mut conn)
            .await
            .unwrap();
        conn.close().await.unwrap();
    }

    #[tokio::test]
    async fn snapshot_is_reproducible_and_preserves_rows() {
        let dir = tempfile::tempdir().unwrap();
        let core = dir.path().join("core.db");
        create_fixture(&core).await;

        let first = run(&SnapshotArgs {
            sqlite: core.clone(),
            out: dir.path().join("first.db"),
        })
        .await
        .unwrap();
        let second = run(&SnapshotArgs {
            sqlite: core.clone(),
            out: dir.path().join("second.db"),
        })
        .await
        .unwrap();

        assert_eq!(first.manifest.snapshot_id, second.manifest.snapshot_id);
        assert_eq!(first.manifest.tables.len(), 1);
        assert_eq!(first.manifest.tables[0].name, "api_keys");
        assert_eq!(first.manifest.tables[0].rows, 5);
        assert_eq!(first.manifest.total_rows, 5);
        assert_eq!(first.manifest.source.user_version, 7);
        assert_eq!(first.manifest.source.observability_path, None);
        assert!(first.manifest_path.exists());

        let mut snapshot = sqlite::open_read_only(dir.path().join("first.db").as_path())
            .await
            .unwrap();
        assert_eq!(
            sqlite::count_rows(&mut snapshot, "main", "api_keys")
                .await
                .unwrap(),
            5
        );
        let table = sqlite::read_table(&mut snapshot, "main", "api_keys")
            .await
            .unwrap();
        assert_eq!(table.primary_key().len(), 1);
        snapshot.close().await.unwrap();
    }

    #[tokio::test]
    async fn snapshot_merges_observability_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let core = dir.path().join("tavily.db");
        create_fixture(&core).await;
        let sidecar = dir.path().join("tavily-observability.db");
        let mut obs = open_fixture(&sidecar).await;
        sqlx::query(
            "CREATE TABLE request_logs (id INTEGER PRIMARY KEY, path TEXT, created_at INTEGER)",
        )
        .execute(&mut obs)
        .await
        .unwrap();
        sqlx::query("INSERT INTO request_logs (path, created_at) VALUES ('/mcp', 1), ('/api', 2)")
            .execute(&mut obs)
            .await
            .unwrap();
        sqlx::query("PRAGMA user_version = 3")
            .execute(&mut obs)
            .await
            .unwrap();
        obs.close().await.unwrap();

        let summary = run(&SnapshotArgs {
            sqlite: core,
            out: dir.path().join("merged.db"),
        })
        .await
        .unwrap();

        assert_eq!(summary.manifest.tables.len(), 2);
        let logs = summary
            .manifest
            .tables
            .iter()
            .find(|table| table.name == "request_logs")
            .unwrap();
        assert_eq!(logs.rows, 2);
        assert_eq!(logs.source, "observability");
        assert_eq!(summary.manifest.source.user_version, 7);
        assert_eq!(summary.manifest.source.observability_user_version, Some(3));
    }

    #[tokio::test]
    async fn snapshot_refuses_existing_output() {
        let dir = tempfile::tempdir().unwrap();
        let core = dir.path().join("core.db");
        create_fixture(&core).await;
        let out = dir.path().join("taken.db");
        std::fs::write(&out, b"occupied").unwrap();
        let error = run(&SnapshotArgs {
            sqlite: core,
            out: out.clone(),
        })
        .await
        .unwrap_err();
        assert!(error.to_string().contains("refusing to overwrite"));
        assert_eq!(std::fs::read(&out).unwrap(), b"occupied");
    }
}
