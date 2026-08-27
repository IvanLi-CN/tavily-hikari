use super::*;

#[derive(Debug)]
pub(crate) enum SqliteCooperativeQueryOutcome<T> {
    Completed(T),
    DeadlineExceeded,
}

impl SqliteReadSnapshot {
    pub(crate) async fn complete_reconciliation_read<T>(
        self,
        kind: ReconciliationReadKind,
        query_result: Result<T, sqlx::Error>,
    ) -> Result<SqliteCooperativeQueryOutcome<T>, ProxyError> {
        self.complete_cooperative_query_inner(Some(kind), query_result)
            .await
    }

    async fn complete_cooperative_query_inner<T>(
        mut self,
        reconciliation_read_kind: Option<ReconciliationReadKind>,
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
            if let Some(kind) = reconciliation_read_kind {
                self.runtime.record_reconciliation_read(
                    kind,
                    self.started_at.elapsed(),
                    false,
                    false,
                    true,
                    None,
                );
            }
            return Err(error);
        }

        let rollback_result = sqlx::query("ROLLBACK").execute(&mut *self).await;
        match (query_result, rollback_result) {
            // A statement can finish between progress-handler callbacks. Its result is still
            // beyond the read session's contract, so discard it before preparation can advance.
            (Ok(_), Ok(_)) if deadline_expired => {
                let mut conn = self.conn.take().expect("SQLite read snapshot connection");
                let cache_write_pages = record_connection_cache_write_delta(
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
                    if let Some(kind) = reconciliation_read_kind {
                        self.runtime.record_reconciliation_read(
                            kind,
                            self.started_at.elapsed(),
                            true,
                            true,
                            true,
                            cache_write_pages,
                        );
                    }
                    return Err(err);
                }
                self.runtime.record_cooperative_read(
                    self.operation,
                    self.started_at.elapsed(),
                    true,
                );
                self.runtime
                    .record_deferred(self.operation, SqliteAdmissionDeferReason::QueryDeadline);
                if let Some(kind) = reconciliation_read_kind {
                    self.runtime.record_reconciliation_read(
                        kind,
                        self.started_at.elapsed(),
                        true,
                        true,
                        false,
                        cache_write_pages,
                    );
                }
                Ok(SqliteCooperativeQueryOutcome::DeadlineExceeded)
            }
            (Ok(value), Ok(_)) => {
                let mut conn = self.conn.take().expect("SQLite read snapshot connection");
                let cache_write_pages = record_connection_cache_write_delta(
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
                    if let Some(kind) = reconciliation_read_kind {
                        self.runtime.record_reconciliation_read(
                            kind,
                            self.started_at.elapsed(),
                            false,
                            false,
                            true,
                            cache_write_pages,
                        );
                    }
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
                if let Some(kind) = reconciliation_read_kind {
                    self.runtime.record_reconciliation_read(
                        kind,
                        self.started_at.elapsed(),
                        false,
                        false,
                        false,
                        cache_write_pages,
                    );
                }
                Ok(SqliteCooperativeQueryOutcome::Completed(value))
            }
            (Err(query_err), Ok(_)) if deadline_expired && sqlite_query_interrupted(&query_err) => {
                let mut conn = self.conn.take().expect("SQLite read snapshot connection");
                let cache_write_pages = record_connection_cache_write_delta(
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
                    if let Some(kind) = reconciliation_read_kind {
                        self.runtime.record_reconciliation_read(
                            kind,
                            self.started_at.elapsed(),
                            true,
                            true,
                            true,
                            cache_write_pages,
                        );
                    }
                    return Err(err);
                }
                self.runtime.record_cooperative_read(
                    self.operation,
                    self.started_at.elapsed(),
                    true,
                );
                self.runtime
                    .record_deferred(self.operation, SqliteAdmissionDeferReason::QueryDeadline);
                if let Some(kind) = reconciliation_read_kind {
                    self.runtime.record_reconciliation_read(
                        kind,
                        self.started_at.elapsed(),
                        true,
                        true,
                        false,
                        cache_write_pages,
                    );
                }
                Ok(SqliteCooperativeQueryOutcome::DeadlineExceeded)
            }
            (Err(query_err), _) => {
                let conn = self.conn.take().expect("SQLite read snapshot connection");
                conn.detach().close().await.ok();
                let err = ProxyError::Database(query_err);
                self.runtime
                    .record_error(self.operation, self.pool_wait, self.begin_wait, &err);
                if let Some(kind) = reconciliation_read_kind {
                    self.runtime.record_reconciliation_read(
                        kind,
                        self.started_at.elapsed(),
                        false,
                        false,
                        true,
                        None,
                    );
                }
                Err(err)
            }
            (Ok(_), Err(rollback_err)) => {
                let conn = self.conn.take().expect("SQLite read snapshot connection");
                conn.detach().close().await.ok();
                let err = ProxyError::Database(rollback_err);
                self.runtime
                    .record_error(self.operation, self.pool_wait, self.begin_wait, &err);
                if let Some(kind) = reconciliation_read_kind {
                    self.runtime.record_reconciliation_read(
                        kind,
                        self.started_at.elapsed(),
                        false,
                        false,
                        true,
                        None,
                    );
                }
                Err(err)
            }
        }
    }
}
