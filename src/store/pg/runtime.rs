//! PostgreSQL pool, embedded migrations and the connection health probe.
//!
//! Nothing here serves application queries yet: the request path keeps using the SQLite
//! store until the storage-migration waves port it. This module owns the startup gate
//! (connect, migrate, assert schema) that those waves build on.

use std::str::FromStr;
use std::time::{Duration, Instant};

use sqlx::Connection;
use sqlx::migrate::{MigrateError, Migrator};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use tracing::info;

use super::{
    config::{PgConfig, PgConfigError},
    schema::{self, PgSchemaError},
};

/// Embedded baseline schema; the path is relative to `CARGO_MANIFEST_DIR`.
pub static MIGRATOR: Migrator = sqlx::migrate!("src/store/pg/migrations");

const STARTUP_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const POOL_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(10);
const HEALTH_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
pub enum PgConnectError {
    #[error("postgresql connection settings are invalid: {0}")]
    Config(#[from] PgConfigError),
    #[error("postgresql connection options are invalid: {0}")]
    Options(#[source] sqlx::Error),
    #[error("failed to connect to postgresql: {0}")]
    Connect(#[source] sqlx::Error),
    #[error("postgresql health probe failed: {0}")]
    Probe(#[source] sqlx::Error),
    #[error("postgresql startup connection timed out after {0:?}")]
    Timeout(Duration),
}

#[derive(Debug, thiserror::Error)]
pub enum PgMigrateError {
    #[error("failed to apply postgresql migrations: {0}")]
    Run(#[source] MigrateError),
    #[error("failed to inspect applied postgresql migrations: {0}")]
    Inspect(#[source] sqlx::Error),
}

/// Migrations applied by one [`PgStore::run_migrations`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgMigrationReport {
    applied_versions: Vec<i64>,
}

impl PgMigrationReport {
    pub fn applied_versions(&self) -> &[i64] {
        &self.applied_versions
    }

    pub fn applied_count(&self) -> usize {
        self.applied_versions.len()
    }
}

/// Connection state shaped for `/health`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PgHealth {
    pub connected: bool,
    pub pool_size: u32,
    pub pool_idle: u32,
    /// Probe failure detail when `connected` is false.
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct PgStore {
    pool: PgPool,
    config: PgConfig,
}

impl PgStore {
    /// Connects with conservative defaults.
    pub async fn connect(database_url: &str) -> Result<Self, PgConnectError> {
        Self::connect_with(PgConfig::new(database_url, true)?).await
    }

    pub async fn connect_with(config: PgConfig) -> Result<Self, PgConnectError> {
        let options =
            PgConnectOptions::from_str(config.database_url()).map_err(PgConnectError::Options)?;
        let started = Instant::now();

        // One direct connection proves reachability and surfaces the real cause (DNS, TLS,
        // auth) instead of the opaque pool timeout; the pool itself stays lazy until the
        // startup gate or a caller acquires from it.
        let mut connection = tokio::time::timeout(
            STARTUP_CONNECT_TIMEOUT,
            PgConnection::connect_with(&options),
        )
        .await
        .map_err(|_| PgConnectError::Timeout(STARTUP_CONNECT_TIMEOUT))?
        .map_err(PgConnectError::Connect)?;

        tokio::time::timeout(
            HEALTH_PROBE_TIMEOUT,
            sqlx::query("SELECT 1").execute(&mut connection),
        )
        .await
        .map_err(|_| PgConnectError::Timeout(HEALTH_PROBE_TIMEOUT))?
        .map_err(PgConnectError::Probe)?;
        connection.close().await.ok();

        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections())
            .acquire_timeout(POOL_ACQUIRE_TIMEOUT)
            .connect_lazy_with(options);
        let store = Self { pool, config };

        info!(
            component = "startup",
            event = "pg_pool_connected",
            database = %store.config.redacted_url(),
            pool_max_connections = store.config.max_connections(),
            probe_latency_ms = %started.elapsed().as_millis(),
            "postgresql pool established"
        );
        Ok(store)
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn config(&self) -> &PgConfig {
        &self.config
    }

    /// Applies every pending embedded migration and logs the applied versions.
    pub async fn run_migrations(pool: &PgPool) -> Result<PgMigrationReport, PgMigrateError> {
        let before = applied_versions(pool)
            .await
            .map_err(PgMigrateError::Inspect)?;
        MIGRATOR.run(pool).await.map_err(PgMigrateError::Run)?;
        let after = applied_versions(pool)
            .await
            .map_err(PgMigrateError::Inspect)?;
        let applied_versions = after
            .into_iter()
            .filter(|version| !before.contains(version))
            .collect::<Vec<_>>();

        info!(
            component = "startup",
            event = "pg_migrations_applied",
            applied_count = applied_versions.len(),
            applied_versions = %join_versions(&applied_versions),
            embedded_count = MIGRATOR.migrations.len(),
            "applied postgresql schema migrations"
        );
        Ok(PgMigrationReport { applied_versions })
    }

    /// Fails when a required baseline relation is absent.
    pub async fn assert_schema_present(pool: &PgPool) -> Result<(), PgSchemaError> {
        schema::assert_schema_present(pool).await
    }

    /// Connection state for `/health`. Not wired into the router yet: the storage wave
    /// that owns `/health` consumes it.
    pub async fn health(&self) -> PgHealth {
        let connected = sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(&self.pool)
            .await;
        PgHealth {
            connected: connected.is_ok(),
            pool_size: self.pool.size(),
            pool_idle: self.pool.num_idle() as u32,
            error: connected.err().map(|err| err.to_string()),
        }
    }
}

async fn applied_versions(pool: &PgPool) -> Result<Vec<i64>, sqlx::Error> {
    let journal_exists: bool =
        sqlx::query_scalar("SELECT to_regclass('_sqlx_migrations') IS NOT NULL")
            .fetch_one(pool)
            .await?;
    if !journal_exists {
        return Ok(Vec::new());
    }
    sqlx::query_scalar("SELECT version FROM _sqlx_migrations WHERE success ORDER BY version")
        .fetch_all(pool)
        .await
}

fn join_versions(versions: &[i64]) -> String {
    if versions.is_empty() {
        return "none".to_owned();
    }
    versions
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(",")
}
