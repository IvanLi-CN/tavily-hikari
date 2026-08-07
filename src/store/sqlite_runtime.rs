use super::{ImmediateSqliteTransaction, ProxyError, is_transient_sqlite_write_error};
use sqlx::{Sqlite, SqlitePool};
use std::future::Future;
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

impl<T> SqliteOperationOutcome<T> {
    fn map<U>(self, map: impl FnOnce(T) -> U) -> SqliteOperationOutcome<U> {
        match self {
            Self::Completed(value) => SqliteOperationOutcome::Completed(map(value)),
            Self::Deferred(reason) => SqliteOperationOutcome::Deferred(reason),
            Self::Failed(error) => SqliteOperationOutcome::Failed(error),
        }
    }

    pub(crate) fn into_result(self) -> Result<T, ProxyError> {
        match self {
            Self::Completed(value) => Ok(value),
            Self::Deferred(SqliteDeferredReason::Busy) => {
                Err(ProxyError::Database(sqlx::Error::PoolTimedOut))
            }
            Self::Deferred(SqliteDeferredReason::Cancelled) => {
                Err(ProxyError::Other("SQLite operation cancelled".to_string()))
            }
            Self::Failed(error) => Err(error),
        }
    }
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

#[derive(Debug)]
pub(crate) struct SqliteRequestStatsConnection {
    connection: Option<sqlx::pool::PoolConnection<Sqlite>>,
    operation_budget: Duration,
    transaction_active: bool,
}

impl Deref for SqliteRequestStatsConnection {
    type Target = sqlx::SqliteConnection;

    fn deref(&self) -> &Self::Target {
        self.connection
            .as_ref()
            .expect("request stats connection")
            .as_ref()
    }
}

impl DerefMut for SqliteRequestStatsConnection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.connection
            .as_mut()
            .expect("request stats connection")
            .as_mut()
    }
}

impl SqliteRequestStatsConnection {
    pub(crate) fn operation_budget(&self) -> Duration {
        self.operation_budget
    }

    pub(crate) fn mark_transaction_active(&mut self) {
        self.transaction_active = true;
    }

    pub(crate) fn mark_transaction_inactive(&mut self) {
        self.transaction_active = false;
    }

    pub(crate) fn discard(&mut self) {
        self.transaction_active = false;
        if let Some(connection) = self.connection.take() {
            drop(connection.detach());
        }
    }

    pub(crate) async fn run_bounded_operation<F, T>(
        operation_budget: Duration,
        operation: F,
    ) -> Result<T, ProxyError>
    where
        F: Future<Output = Result<T, ProxyError>>,
    {
        run_bounded_request_stats_operation(operation_budget, operation)
            .await
            .into_result()
    }
}

impl Drop for SqliteRequestStatsConnection {
    fn drop(&mut self) {
        if self.transaction_active
            && let Some(connection) = self.connection.take()
        {
            drop(connection.detach());
        }
    }
}

#[derive(Debug)]
pub(crate) struct SqliteRequestStatsTransaction<'c> {
    transaction: sqlx::Transaction<'c, Sqlite>,
    operation_budget: Duration,
}

impl<'c> SqliteRequestStatsTransaction<'c> {
    pub(crate) async fn commit(self) -> Result<(), ProxyError> {
        run_bounded_request_stats_operation(self.operation_budget, self.transaction.commit())
            .await
            .into_result()
    }

    pub(crate) fn operation_budget(&self) -> Duration {
        self.operation_budget
    }

    pub(crate) async fn run_bounded_operation<F, T>(
        operation_budget: Duration,
        operation: F,
    ) -> Result<T, ProxyError>
    where
        F: Future<Output = Result<T, ProxyError>>,
    {
        run_bounded_request_stats_operation(operation_budget, operation)
            .await
            .into_result()
    }
}

impl<'c> Deref for SqliteRequestStatsTransaction<'c> {
    type Target = sqlx::Transaction<'c, Sqlite>;

    fn deref(&self) -> &Self::Target {
        &self.transaction
    }
}

impl<'c> DerefMut for SqliteRequestStatsTransaction<'c> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.transaction
    }
}

async fn run_bounded_request_stats_operation<F, T, E>(
    operation_budget: Duration,
    operation: F,
) -> SqliteOperationOutcome<T>
where
    F: Future<Output = Result<T, E>>,
    E: Into<ProxyError>,
{
    match tokio::time::timeout(operation_budget, operation).await {
        Ok(Ok(value)) => SqliteOperationOutcome::Completed(value),
        Ok(Err(error)) => {
            let error = error.into();
            if is_transient_sqlite_write_error(&error) {
                SqliteOperationOutcome::Deferred(SqliteDeferredReason::Busy)
            } else {
                SqliteOperationOutcome::Failed(error)
            }
        }
        Err(_) => SqliteOperationOutcome::Deferred(SqliteDeferredReason::Busy),
    }
}

#[allow(dead_code)]
pub(crate) type SqliteAdmission = OwnedSemaphorePermit;

#[derive(Debug, Clone)]
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

    pub(crate) async fn fetch_request_stats_one<'q>(
        &self,
        query: sqlx::query::Query<'q, Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    ) -> SqliteOperationOutcome<sqlx::sqlite::SqliteRow> {
        run_bounded_request_stats_operation(
            self.operation_budget,
            query.fetch_one(&self.primary_pool),
        )
        .await
    }

    pub(crate) async fn fetch_request_stats_scalar_one<'q, O>(
        &self,
        query: sqlx::query::QueryScalar<'q, Sqlite, O, sqlx::sqlite::SqliteArguments<'q>>,
    ) -> SqliteOperationOutcome<O>
    where
        O: Send + Unpin + 'q,
        (O,): Send + Unpin + for<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow>,
    {
        run_bounded_request_stats_operation(
            self.operation_budget,
            query.fetch_one(&self.primary_pool),
        )
        .await
    }

    pub(crate) async fn fetch_request_stats_optional<'q>(
        &self,
        query: sqlx::query::Query<'q, Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    ) -> SqliteOperationOutcome<Option<sqlx::sqlite::SqliteRow>> {
        run_bounded_request_stats_operation(
            self.operation_budget,
            query.fetch_optional(&self.primary_pool),
        )
        .await
    }

    pub(crate) async fn fetch_request_stats_scalar_optional<'q, O>(
        &self,
        query: sqlx::query::QueryScalar<'q, Sqlite, O, sqlx::sqlite::SqliteArguments<'q>>,
    ) -> SqliteOperationOutcome<Option<O>>
    where
        O: Send + Unpin + 'q,
        (O,): Send + Unpin + for<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow>,
    {
        run_bounded_request_stats_operation(
            self.operation_budget,
            query.fetch_optional(&self.primary_pool),
        )
        .await
    }

    pub(crate) async fn fetch_request_stats_all<'q>(
        &self,
        query: sqlx::query::Query<'q, Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    ) -> SqliteOperationOutcome<Vec<sqlx::sqlite::SqliteRow>> {
        run_bounded_request_stats_operation(
            self.operation_budget,
            query.fetch_all(&self.primary_pool),
        )
        .await
    }

    pub(crate) async fn execute_request_stats<'q>(
        &self,
        query: sqlx::query::Query<'q, Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    ) -> SqliteOperationOutcome<sqlx::sqlite::SqliteQueryResult> {
        run_bounded_request_stats_operation(
            self.operation_budget,
            query.execute(&self.primary_pool),
        )
        .await
    }

    pub(crate) async fn begin_primary_transaction(
        &self,
    ) -> SqliteOperationOutcome<SqliteRequestStatsTransaction<'_>> {
        run_bounded_request_stats_operation(self.operation_budget, self.primary_pool.begin())
            .await
            .map(|transaction| SqliteRequestStatsTransaction {
                transaction,
                operation_budget: self.operation_budget,
            })
    }

    pub(crate) async fn begin_read_flush_transaction(
        &self,
    ) -> SqliteOperationOutcome<SqliteRequestStatsTransaction<'_>> {
        run_bounded_request_stats_operation(self.operation_budget, self.read_flush_pool.begin())
            .await
            .map(|transaction| SqliteRequestStatsTransaction {
                transaction,
                operation_budget: self.operation_budget,
            })
    }

    pub(crate) async fn acquire_primary_connection(
        &self,
    ) -> SqliteOperationOutcome<SqliteRequestStatsConnection> {
        run_bounded_request_stats_operation(self.operation_budget, self.primary_pool.acquire())
            .await
            .map(|connection| SqliteRequestStatsConnection {
                connection: Some(connection),
                operation_budget: self.operation_budget,
                transaction_active: false,
            })
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

    #[tokio::test]
    async fn request_stats_write_is_deferred_within_operation_budget() {
        let (runtime, lock_pool, _temp_dir) = test_runtime().await;
        let primary = runtime.compatibility_primary_pool();
        sqlx::query("CREATE TABLE request_stats_runtime_probe (value INTEGER NOT NULL)")
            .execute(&primary)
            .await
            .expect("create request stats probe table");

        let mut lock = lock_pool.acquire().await.expect("lock connection");
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *lock)
            .await
            .expect("hold writer lock");
        let started = std::time::Instant::now();

        let outcome = runtime
            .execute_request_stats(sqlx::query(
                "INSERT INTO request_stats_runtime_probe (value) VALUES (1)",
            ))
            .await;

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

    #[tokio::test]
    async fn request_stats_transaction_statement_is_deferred_within_operation_budget() {
        let (runtime, lock_pool, _temp_dir) = test_runtime().await;
        let primary = runtime.compatibility_primary_pool();
        sqlx::query("CREATE TABLE request_stats_transaction_probe (value INTEGER NOT NULL)")
            .execute(&primary)
            .await
            .expect("create request stats transaction probe table");

        let mut lock = lock_pool.acquire().await.expect("lock connection");
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *lock)
            .await
            .expect("hold writer lock");

        let mut transaction = match runtime.begin_primary_transaction().await {
            SqliteOperationOutcome::Completed(transaction) => transaction,
            other => panic!("request stats transaction did not begin: {other:?}"),
        };
        let started = std::time::Instant::now();
        let outcome = SqliteRequestStatsTransaction::run_bounded_operation(
            transaction.operation_budget(),
            async {
                sqlx::query("INSERT INTO request_stats_transaction_probe (value) VALUES (1)")
                    .execute(&mut **transaction)
                    .await
                    .map(|_| ())
                    .map_err(ProxyError::Database)
            },
        )
        .await;

        assert!(matches!(
            outcome,
            Err(ProxyError::Database(sqlx::Error::PoolTimedOut))
        ));
        assert!(started.elapsed() < std::time::Duration::from_millis(750));
        drop(transaction);
        sqlx::query("ROLLBACK")
            .execute(&mut *lock)
            .await
            .expect("release writer lock");
    }

    #[tokio::test]
    async fn request_stats_transaction_commit_is_deferred_within_operation_budget() {
        let (runtime, lock_pool, _temp_dir) = test_runtime().await;
        let primary = runtime.compatibility_primary_pool();
        sqlx::query("CREATE TABLE request_stats_commit_probe (value INTEGER NOT NULL)")
            .execute(&primary)
            .await
            .expect("create request stats commit probe table");
        sqlx::query("INSERT INTO request_stats_commit_probe (value) VALUES (0)")
            .execute(&primary)
            .await
            .expect("seed request stats commit probe");

        let mut transaction = match runtime.begin_primary_transaction().await {
            SqliteOperationOutcome::Completed(transaction) => transaction,
            other => panic!("request stats transaction did not begin: {other:?}"),
        };
        sqlx::query("UPDATE request_stats_commit_probe SET value = 1")
            .execute(&mut **transaction)
            .await
            .expect("write request stats commit probe");

        let mut read_lock = lock_pool.acquire().await.expect("read lock connection");
        sqlx::query("BEGIN")
            .execute(&mut *read_lock)
            .await
            .expect("begin read lock transaction");
        sqlx::query("SELECT value FROM request_stats_commit_probe")
            .fetch_one(&mut *read_lock)
            .await
            .expect("hold read lock");

        let started = std::time::Instant::now();
        let outcome = transaction.commit().await;

        assert!(matches!(
            outcome,
            Err(ProxyError::Database(sqlx::Error::PoolTimedOut))
        ));
        assert!(started.elapsed() < std::time::Duration::from_millis(750));
        sqlx::query("ROLLBACK")
            .execute(&mut *read_lock)
            .await
            .expect("release read lock");
    }

    #[tokio::test]
    async fn dropped_request_stats_connection_detaches_active_transaction() {
        let (runtime, _lock_pool, _temp_dir) = test_runtime().await;
        let primary = runtime.compatibility_primary_pool();
        let mut connection = match runtime.acquire_primary_connection().await {
            SqliteOperationOutcome::Completed(connection) => connection,
            other => panic!("request stats connection did not acquire: {other:?}"),
        };
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *connection)
            .await
            .expect("begin request stats transaction");
        connection.mark_transaction_active();
        drop(connection);

        let mut replacement =
            tokio::time::timeout(std::time::Duration::from_secs(1), primary.acquire())
                .await
                .expect("pool should replace detached connection")
                .expect("acquire replacement connection");
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *replacement)
            .await
            .expect("replacement connection is not inside a transaction");
        sqlx::query("ROLLBACK")
            .execute(&mut *replacement)
            .await
            .expect("rollback replacement transaction");
    }

    #[tokio::test]
    async fn cancelled_request_stats_cleanup_detaches_active_transaction() {
        let (runtime, _lock_pool, _temp_dir) = test_runtime().await;
        let primary = runtime.compatibility_primary_pool();
        let mut connection = match runtime.acquire_primary_connection().await {
            SqliteOperationOutcome::Completed(connection) => connection,
            other => panic!("request stats connection did not acquire: {other:?}"),
        };
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *connection)
            .await
            .expect("begin request stats transaction");
        connection.mark_transaction_active();
        let (cleanup_started, cleanup_started_rx) = tokio::sync::oneshot::channel();
        let cleanup = tokio::spawn(async move {
            let _ = SqliteRequestStatsConnection::run_bounded_operation(
                connection.operation_budget(),
                async {
                    let _ = cleanup_started.send(());
                    std::future::pending::<Result<(), ProxyError>>().await
                },
            )
            .await;
            connection.mark_transaction_inactive();
        });
        cleanup_started_rx
            .await
            .expect("cleanup operation should start");
        cleanup.abort();
        assert!(
            cleanup
                .await
                .expect_err("cancelled cleanup should abort its task")
                .is_cancelled()
        );

        let mut replacement =
            tokio::time::timeout(std::time::Duration::from_secs(1), primary.acquire())
                .await
                .expect("pool should replace detached connection")
                .expect("acquire replacement connection");
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *replacement)
            .await
            .expect("replacement connection is not inside a transaction");
        sqlx::query("ROLLBACK")
            .execute(&mut *replacement)
            .await
            .expect("rollback replacement transaction");
    }

    #[tokio::test]
    async fn exhausted_primary_pool_read_is_deferred_within_operation_budget() {
        let (runtime, _lock_pool, _temp_dir) = test_runtime().await;
        let primary = runtime.compatibility_primary_pool();
        let first = primary.acquire().await.expect("first primary connection");
        let second = primary.acquire().await.expect("second primary connection");
        let started = std::time::Instant::now();

        let outcome = runtime.read(&CancellationToken::new()).await;

        assert!(matches!(
            outcome,
            SqliteOperationOutcome::Deferred(SqliteDeferredReason::Busy)
        ));
        assert!(started.elapsed() < std::time::Duration::from_millis(750));
        drop((first, second));
    }
}
