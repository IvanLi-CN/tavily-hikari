//! Read-only PostgreSQL snapshot shared by the storage modules being ported from SQLite.
//!
//! Several readers hold the contract "one response, one consistent snapshot": the alert pages,
//! the admin privacy status, and the reconciliation progress views used to mix generations when
//! they issued each statement against the pool. SQLite satisfied that with a read session that
//! opens a deferred transaction; PostgreSQL satisfies it with a repeatable-read, read-only
//! transaction on one pooled connection.
//!
//! Unlike the SQLite session there is no cooperative run budget, progress handler, or pool
//! pressure defer: those exist to bound native SQLite work that cannot be cancelled. PostgreSQL
//! statements are cancellable and the pool already enforces its own wait limits.

use sqlx::{Postgres, Transaction};

use crate::models::ProxyError;

/// One repeatable-read, read-only transaction. Dropping it rolls the transaction back.
pub(crate) struct PgReadSession {
    transaction: Option<Transaction<'static, Postgres>>,
}

impl PgReadSession {
    pub(crate) async fn begin(pool: &sqlx::PgPool) -> Result<Self, ProxyError> {
        let mut transaction = pool.begin().await.map_err(ProxyError::Database)?;
        // The isolation level must be set before the first statement of the transaction.
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(ProxyError::Database)?;
        // Readers that reproduce the SQLite `localtime` day and month buckets must resolve the
        // same zone the process runs in. The server reports local time through `chrono::Local`,
        // which carries an offset but not a zone name, so the zone is resolved here instead of
        // pinning a fixed offset that would be wrong across daylight-saving transitions.
        if let Ok(timezone) = iana_time_zone::get_timezone() {
            let applied = sqlx::query("SELECT set_config('TimeZone', $1, true)")
                .bind(timezone)
                .execute(&mut *transaction)
                .await;
            if applied.is_err() {
                // An unrecognised zone name must not fail the read: the server default stays in
                // effect and the caller keeps its own zone contract.
                tracing::warn!(
                    "postgres read session kept its default time zone; local zone was rejected"
                );
            }
        }
        Ok(Self {
            transaction: Some(transaction),
        })
    }

    pub(crate) fn connection(&mut self) -> &mut sqlx::PgConnection {
        self.transaction
            .as_mut()
            .expect("pg read session is open until finish")
            .as_mut()
    }

    pub(crate) async fn finish(mut self) -> Result<(), ProxyError> {
        let transaction = self
            .transaction
            .take()
            .expect("pg read session is open until finish");
        transaction.commit().await.map_err(ProxyError::Database)
    }
}

impl Drop for PgReadSession {
    fn drop(&mut self) {
        // `sqlx::Transaction` queues its rollback when dropped, so an unfinished session cannot
        // return an open transaction to the pool.
        self.transaction.take();
    }
}
