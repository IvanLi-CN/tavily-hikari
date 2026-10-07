use super::ProxyError;
use sqlx::Connection;
use sqlx::Sqlite;
use sqlx::SqliteConnection;
use std::ops::{Deref, DerefMut};

/// A raw `BEGIN IMMEDIATE` transaction that cannot return an open transaction
/// to the pool when its future is cancelled.
#[derive(Debug)]
pub(crate) struct ImmediateSqliteTransaction {
    conn: Option<sqlx::pool::PoolConnection<Sqlite>>,
}

impl ImmediateSqliteTransaction {
    pub(crate) async fn begin(
        conn: sqlx::pool::PoolConnection<Sqlite>,
    ) -> Result<Self, ProxyError> {
        let mut transaction = Self { conn: Some(conn) };
        if let Err(err) = sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *transaction)
            .await
        {
            let conn = transaction
                .conn
                .take()
                .expect("immediate transaction connection");
            // A failed BEGIN has not opened a transaction. Sending ROLLBACK
            // here can wait behind the same writer that rejected BEGIN, which
            // turns a bounded control admission into multiple busy windows.
            // Detach instead so this physical connection never returns to the
            // pool with unknown state.
            drop(conn.detach());
            return Err(ProxyError::Database(err));
        }
        Ok(transaction)
    }

    pub(crate) async fn commit(self) -> Result<(), ProxyError> {
        self.commit_connection().await.map(drop)
    }

    pub(crate) async fn commit_connection(
        mut self,
    ) -> Result<sqlx::pool::PoolConnection<Sqlite>, ProxyError> {
        let commit_result = sqlx::query("COMMIT").execute(&mut *self).await;
        if let Err(err) = commit_result {
            let _ = sqlx::query("ROLLBACK").execute(&mut *self).await;
            let conn = self.conn.take().expect("immediate transaction connection");
            conn.detach().close().await.ok();
            return Err(ProxyError::Database(err));
        }
        Ok(self.conn.take().expect("immediate transaction connection"))
    }

    pub(crate) async fn rollback(self) -> Result<(), ProxyError> {
        self.rollback_connection().await.map(drop)
    }

    pub(crate) async fn rollback_connection(
        mut self,
    ) -> Result<sqlx::pool::PoolConnection<Sqlite>, ProxyError> {
        let result = sqlx::query("ROLLBACK").execute(&mut *self).await;
        match result {
            Ok(_) => Ok(self.conn.take().expect("immediate transaction connection")),
            Err(err) => {
                let conn = self.conn.take().expect("immediate transaction connection");
                conn.detach().close().await.ok();
                Err(ProxyError::from(err))
            }
        }
    }

    /// Restores a connection-level foreign-key override after a successful
    /// rollback. If the pragma cannot be restored, the connection is removed
    /// from the pool instead of leaking the override to an unrelated request.
    pub(crate) async fn rollback_and_reenable_foreign_keys(self) -> Result<(), ProxyError> {
        let mut conn = self.rollback_connection().await?;
        match sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await
        {
            Ok(_) => Ok(()),
            Err(err) => {
                conn.detach().close().await.ok();
                Err(ProxyError::Database(err))
            }
        }
    }
}

impl Deref for ImmediateSqliteTransaction {
    type Target = SqliteConnection;

    fn deref(&self) -> &Self::Target {
        self.conn
            .as_ref()
            .expect("immediate transaction connection")
            .as_ref()
    }
}

impl DerefMut for ImmediateSqliteTransaction {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.conn
            .as_mut()
            .expect("immediate transaction connection")
            .as_mut()
    }
}

impl Drop for ImmediateSqliteTransaction {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            // Detaching prevents PoolConnection::drop from returning a possibly
            // transaction-polluted physical connection to the shared pool.
            drop(conn.detach());
        }
    }
}

/// A connection-owned savepoint that cannot return an open savepoint to the pool
/// when its future is cancelled.
#[derive(Debug)]
pub(crate) struct SavepointSqliteTransaction {
    conn: Option<sqlx::pool::PoolConnection<Sqlite>>,
}

impl SavepointSqliteTransaction {
    const NAME: &'static str = "dashboard_rollup_trigger_replace";

    pub(crate) async fn begin(
        conn: sqlx::pool::PoolConnection<Sqlite>,
    ) -> Result<Self, ProxyError> {
        let mut transaction = Self { conn: Some(conn) };
        let statement = format!("SAVEPOINT {}", Self::NAME);
        if let Err(err) = sqlx::query(&statement).execute(&mut *transaction).await {
            let conn = transaction
                .conn
                .take()
                .expect("savepoint transaction connection");
            drop(conn.detach());
            return Err(ProxyError::Database(err));
        }
        Ok(transaction)
    }

    pub(crate) async fn commit(self) -> Result<(), ProxyError> {
        self.commit_connection().await.map(drop)
    }

    pub(crate) async fn commit_connection(
        mut self,
    ) -> Result<sqlx::pool::PoolConnection<Sqlite>, ProxyError> {
        let statement = format!("RELEASE SAVEPOINT {}", Self::NAME);
        if let Err(err) = sqlx::query(&statement).execute(&mut *self).await {
            let conn = self.conn.take().expect("savepoint transaction connection");
            conn.detach().close().await.ok();
            return Err(ProxyError::Database(err));
        }
        Ok(self.conn.take().expect("savepoint transaction connection"))
    }

    pub(crate) async fn rollback(self) -> Result<(), ProxyError> {
        self.rollback_connection().await.map(drop)
    }

    pub(crate) async fn rollback_connection(
        mut self,
    ) -> Result<sqlx::pool::PoolConnection<Sqlite>, ProxyError> {
        let rollback_statement = format!("ROLLBACK TO SAVEPOINT {}", Self::NAME);
        let release_statement = format!("RELEASE SAVEPOINT {}", Self::NAME);
        if let Err(err) = sqlx::query(&rollback_statement).execute(&mut *self).await {
            let conn = self.conn.take().expect("savepoint transaction connection");
            conn.detach().close().await.ok();
            return Err(ProxyError::Database(err));
        }
        if let Err(err) = sqlx::query(&release_statement).execute(&mut *self).await {
            let conn = self.conn.take().expect("savepoint transaction connection");
            conn.detach().close().await.ok();
            return Err(ProxyError::Database(err));
        }
        Ok(self.conn.take().expect("savepoint transaction connection"))
    }
}

impl Deref for SavepointSqliteTransaction {
    type Target = SqliteConnection;

    fn deref(&self) -> &Self::Target {
        self.conn
            .as_ref()
            .expect("savepoint transaction connection")
            .as_ref()
    }
}

impl DerefMut for SavepointSqliteTransaction {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.conn
            .as_mut()
            .expect("savepoint transaction connection")
            .as_mut()
    }
}

impl Drop for SavepointSqliteTransaction {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            drop(conn.detach());
        }
    }
}

/// A transaction boundary that works for both separate databases and the
/// legacy layout where `main` and `observability` attach the same file.
#[derive(Debug)]
pub(crate) enum SqliteTransaction {
    Immediate(ImmediateSqliteTransaction),
    Savepoint(SavepointSqliteTransaction),
}

impl SqliteTransaction {
    pub(crate) async fn begin(
        conn: sqlx::pool::PoolConnection<Sqlite>,
        use_savepoint: bool,
    ) -> Result<Self, ProxyError> {
        if use_savepoint {
            SavepointSqliteTransaction::begin(conn)
                .await
                .map(Self::Savepoint)
        } else {
            ImmediateSqliteTransaction::begin(conn)
                .await
                .map(Self::Immediate)
        }
    }

    pub(crate) async fn commit_connection(
        self,
    ) -> Result<sqlx::pool::PoolConnection<Sqlite>, ProxyError> {
        match self {
            Self::Immediate(transaction) => transaction.commit_connection().await,
            Self::Savepoint(transaction) => transaction.commit_connection().await,
        }
    }

    pub(crate) async fn rollback(self) -> Result<(), ProxyError> {
        self.rollback_connection().await.map(drop)
    }

    pub(crate) async fn rollback_connection(
        self,
    ) -> Result<sqlx::pool::PoolConnection<Sqlite>, ProxyError> {
        match self {
            Self::Immediate(transaction) => transaction.rollback_connection().await,
            Self::Savepoint(transaction) => transaction.rollback_connection().await,
        }
    }
}

impl Deref for SqliteTransaction {
    type Target = SqliteConnection;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Immediate(transaction) => transaction,
            Self::Savepoint(transaction) => transaction,
        }
    }
}

impl DerefMut for SqliteTransaction {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Immediate(transaction) => transaction,
            Self::Savepoint(transaction) => transaction,
        }
    }
}
