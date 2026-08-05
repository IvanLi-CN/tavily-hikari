use super::{ImmediateSqliteTransaction, ProxyError, is_transient_sqlite_write_error};
use sqlx::{Sqlite, SqlitePool};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SqliteDeferredReason {
    Cancelled,
    Busy,
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum SqliteOperationOutcome<T> {
    Completed(T),
    Deferred(SqliteDeferredReason),
    Failed(ProxyError),
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct SqliteReadConnection(sqlx::pool::PoolConnection<Sqlite>);

impl Deref for SqliteReadConnection {
    type Target = sqlx::SqliteConnection;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

impl DerefMut for SqliteReadConnection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut()
    }
}

#[allow(dead_code)]
pub(crate) type SqliteAdmission = OwnedSemaphorePermit;

#[derive(Debug)]
pub(crate) struct SqliteRuntime {
    primary_pool: SqlitePool,
    read_flush_pool: SqlitePool,
    admission: Arc<Semaphore>,
    operation_budget: Duration,
}

impl SqliteRuntime {
    pub(crate) const DEFAULT_OPERATION_BUDGET: Duration = Duration::from_millis(250);

    pub(crate) fn new(
        primary_pool: SqlitePool,
        read_flush_pool: SqlitePool,
        admission_limit: usize,
    ) -> Self {
        Self {
            primary_pool,
            read_flush_pool,
            admission: Arc::new(Semaphore::new(admission_limit)),
            operation_budget: Self::DEFAULT_OPERATION_BUDGET,
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn read(
        &self,
        cancellation: &CancellationToken,
    ) -> SqliteOperationOutcome<SqliteReadConnection> {
        let acquire = tokio::time::timeout(self.operation_budget, self.primary_pool.acquire());
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                SqliteOperationOutcome::Deferred(SqliteDeferredReason::Cancelled)
            }
            result = acquire => match result {
                Ok(Ok(connection)) => {
                    SqliteOperationOutcome::Completed(SqliteReadConnection(connection))
                }
                Ok(Err(err)) => SqliteOperationOutcome::Failed(ProxyError::Database(err)),
                Err(_) => SqliteOperationOutcome::Deferred(SqliteDeferredReason::Busy),
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn begin_immediate(
        &self,
        cancellation: &CancellationToken,
    ) -> SqliteOperationOutcome<ImmediateSqliteTransaction> {
        let begin = async {
            let connection = self.primary_pool.acquire().await?;
            ImmediateSqliteTransaction::begin(connection).await
        };
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                SqliteOperationOutcome::Deferred(SqliteDeferredReason::Cancelled)
            }
            result = tokio::time::timeout(self.operation_budget, begin) => match result {
                Ok(Ok(transaction)) => SqliteOperationOutcome::Completed(transaction),
                Ok(Err(err)) if is_transient_sqlite_write_error(&err) => {
                    SqliteOperationOutcome::Deferred(SqliteDeferredReason::Busy)
                }
                Ok(Err(err)) => SqliteOperationOutcome::Failed(err),
                Err(_) => SqliteOperationOutcome::Deferred(SqliteDeferredReason::Busy),
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn admit(
        &self,
        cancellation: &CancellationToken,
    ) -> SqliteOperationOutcome<SqliteAdmission> {
        let acquire = tokio::time::timeout(
            self.operation_budget,
            Arc::clone(&self.admission).acquire_owned(),
        );
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                SqliteOperationOutcome::Deferred(SqliteDeferredReason::Cancelled)
            }
            result = acquire => match result {
                Ok(Ok(permit)) => SqliteOperationOutcome::Completed(permit),
                Ok(Err(err)) => SqliteOperationOutcome::Failed(ProxyError::Other(err.to_string())),
                Err(_) => SqliteOperationOutcome::Deferred(SqliteDeferredReason::Busy),
            }
        }
    }

    pub(crate) fn compatibility_primary_pool(&self) -> SqlitePool {
        self.primary_pool.clone()
    }

    pub(crate) fn compatibility_read_flush_pool(&self) -> SqlitePool {
        self.read_flush_pool.clone()
    }

    pub(crate) fn compatibility_admission(&self) -> Arc<Semaphore> {
        Arc::clone(&self.admission)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;
    use tempfile::TempDir;
    use tokio_util::sync::CancellationToken;

    async fn test_runtime() -> (SqliteRuntime, sqlx::SqlitePool, TempDir) {
        let temp_dir = TempDir::new().expect("sqlite temp directory");
        let path = temp_dir
            .path()
            .join("runtime.db")
            .to_string_lossy()
            .into_owned();
        let options = SqliteConnectOptions::from_str(&format!("sqlite://{path}"))
            .expect("sqlite options")
            .create_if_missing(true)
            .busy_timeout(std::time::Duration::from_secs(5));
        let primary = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options.clone())
            .await
            .expect("primary pool");
        let lock_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("lock pool");
        let runtime = SqliteRuntime::new(primary.clone(), primary, 1);
        (runtime, lock_pool, temp_dir)
    }

    #[test]
    fn default_operation_budget_is_250ms() {
        assert_eq!(
            SqliteRuntime::DEFAULT_OPERATION_BUDGET,
            std::time::Duration::from_millis(250)
        );
    }

    #[tokio::test]
    async fn cancelled_read_returns_typed_outcome() {
        let (runtime, _lock_pool, _temp_dir) = test_runtime().await;
        let cancelled = CancellationToken::new();
        cancelled.cancel();

        let outcome = runtime.read(&cancelled).await;

        assert!(matches!(
            outcome,
            SqliteOperationOutcome::Deferred(SqliteDeferredReason::Cancelled)
        ));
    }

    #[tokio::test]
    async fn busy_immediate_write_is_deferred_within_operation_budget() {
        let (runtime, lock_pool, _temp_dir) = test_runtime().await;
        let mut lock = lock_pool.acquire().await.expect("lock connection");
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *lock)
            .await
            .expect("hold writer lock");
        let started = std::time::Instant::now();

        let outcome = runtime.begin_immediate(&CancellationToken::new()).await;

        assert!(matches!(
            outcome,
            SqliteOperationOutcome::Deferred(SqliteDeferredReason::Busy)
        ));
        assert!(started.elapsed() < std::time::Duration::from_millis(750));
        sqlx::query("ROLLBACK")
            .execute(&mut *lock)
            .await
            .expect("release writer lock");
    }
}
