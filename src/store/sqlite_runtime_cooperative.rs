use super::*;

#[derive(Debug)]
pub(crate) enum SqliteCooperativeQueryOutcome<T> {
    Completed(T),
    DeadlineExceeded,
}

impl SqliteReadSnapshot {
    pub(crate) async fn complete_cooperative_query<T>(
        mut self,
        query_result: Result<T, sqlx::Error>,
    ) -> Result<SqliteCooperativeQueryOutcome<T>, ProxyError> {
        let deadline_expired = self
            .cooperative_run_deadline
            .is_some_and(|deadline| Instant::now() >= deadline);
        if let Err(error) = self.clear_cooperative_run_budget().await {
            let conn = self.conn.take().expect("SQLite read snapshot connection");
            conn.detach().close().await.ok();
            self.runtime
                .record_error(self.operation, self.pool_wait, self.begin_wait, &error);
            return Err(error);
        }

        let rollback_result = sqlx::query("ROLLBACK").execute(&mut *self).await;
        match (query_result, rollback_result) {
            (Ok(value), Ok(_)) => {
                let mut conn = self.conn.take().expect("SQLite read snapshot connection");
                record_connection_cache_write_delta(
                    &self.runtime,
                    self.operation,
                    self.cache_write_pages_start,
                    &mut conn,
                )
                .await;
                if let Err(restore_err) =
                    restore_operation_connection(conn, self.restore_busy_timeout).await
                {
                    let err = ProxyError::Database(restore_err);
                    self.runtime.record_error(
                        self.operation,
                        self.pool_wait,
                        self.begin_wait,
                        &err,
                    );
                    return Err(err);
                }
                self.runtime.record_success(
                    self.operation,
                    self.pool_wait,
                    self.begin_wait,
                    self.started_at.elapsed(),
                    0,
                );
                self.runtime.record_cooperative_read(
                    self.operation,
                    self.started_at.elapsed(),
                    false,
                );
                Ok(SqliteCooperativeQueryOutcome::Completed(value))
            }
            (Err(query_err), Ok(_)) if deadline_expired && sqlite_query_interrupted(&query_err) => {
                let mut conn = self.conn.take().expect("SQLite read snapshot connection");
                record_connection_cache_write_delta(
                    &self.runtime,
                    self.operation,
                    self.cache_write_pages_start,
                    &mut conn,
                )
                .await;
                if let Err(restore_err) =
                    restore_operation_connection(conn, self.restore_busy_timeout).await
                {
                    let err = ProxyError::Database(restore_err);
                    self.runtime.record_error(
                        self.operation,
                        self.pool_wait,
                        self.begin_wait,
                        &err,
                    );
                    return Err(err);
                }
                self.runtime.record_cooperative_read(
                    self.operation,
                    self.started_at.elapsed(),
                    true,
                );
                self.runtime
                    .record_deferred(self.operation, SqliteAdmissionDeferReason::QueryDeadline);
                Ok(SqliteCooperativeQueryOutcome::DeadlineExceeded)
            }
            (Err(query_err), _) => {
                let conn = self.conn.take().expect("SQLite read snapshot connection");
                conn.detach().close().await.ok();
                let err = ProxyError::Database(query_err);
                self.runtime
                    .record_error(self.operation, self.pool_wait, self.begin_wait, &err);
                Err(err)
            }
            (Ok(_), Err(rollback_err)) => {
                let conn = self.conn.take().expect("SQLite read snapshot connection");
                conn.detach().close().await.ok();
                let err = ProxyError::Database(rollback_err);
                self.runtime
                    .record_error(self.operation, self.pool_wait, self.begin_wait, &err);
                Err(err)
            }
        }
    }
}
