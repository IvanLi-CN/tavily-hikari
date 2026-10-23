use super::*;

#[derive(Debug)]
pub(crate) enum SqliteCooperativeQueryOutcome<T> {
    Completed(T),
    DeadlineExceeded,
}

impl SqliteReadSnapshot {
    pub(crate) async fn complete_native_read<T>(
        self,
        query_result: Result<T, sqlx::Error>,
    ) -> Result<SqliteCooperativeQueryOutcome<T>, ProxyError> {
        self.complete_cooperative_query_inner(None, query_result)
            .await
    }

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

impl SqliteRuntime {
    pub(crate) async fn begin_read_snapshot_before_deadline(
        &self,
        operation: SqliteOperation,
        deadline: Instant,
        max_run_budget: Duration,
    ) -> Result<SqliteReadSnapshot, ProxyError> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let error = ProxyError::Deferred {
                operation: operation.as_str(),
                reason: "recovery_budget".to_string(),
            };
            self.record_deferred_error(operation, &error);
            return Err(error);
        }
        let (conn, pool_wait) = self
            .acquire_pool_connection_with_budget(
                operation,
                operation.acquire_budget().min(remaining),
            )
            .await?;
        let (conn, restore_busy_timeout) =
            match configure_operation_connection(conn, operation).await {
                Ok(configured) => configured,
                Err(err) => {
                    let error = ProxyError::Database(err);
                    self.record_error(operation, pool_wait, Duration::ZERO, &error);
                    return Err(error);
                }
            };
        let begin_started = Instant::now();
        let mut snapshot = SqliteReadSnapshot {
            conn: Some(conn),
            runtime: self.clone(),
            operation,
            pool_wait,
            begin_wait: Duration::ZERO,
            started_at: Instant::now(),
            restore_busy_timeout,
            cache_write_pages_start: None,
            cooperative_run_deadline: None,
            #[cfg(any(test, debug_assertions))]
            cooperative_run_budget_for_test: None,
            #[cfg(test)]
            cooperative_run_budget_checks_remaining_for_test: None,
        };
        let begin_budget = operation
            .begin_budget()
            .min(deadline.saturating_duration_since(Instant::now()));
        if begin_budget.is_zero() {
            let error = ProxyError::Deferred {
                operation: operation.as_str(),
                reason: "recovery_budget".to_string(),
            };
            return match snapshot.close_after_query(Some(&error)).await {
                Ok(()) => Err(error),
                Err(close_error) => Err(close_error),
            };
        }
        let begin_result =
            tokio::time::timeout(begin_budget, sqlx::query("BEGIN").execute(&mut *snapshot)).await;
        match begin_result {
            Ok(Ok(_)) => {}
            Ok(Err(err)) => {
                let conn = snapshot
                    .conn
                    .take()
                    .expect("SQLite read snapshot connection");
                if let Err(restore_err) =
                    restore_operation_connection(conn, restore_busy_timeout).await
                {
                    let error = ProxyError::Database(restore_err);
                    self.record_error(operation, pool_wait, begin_started.elapsed(), &error);
                    return Err(error);
                }
                let error = ProxyError::Database(err);
                self.record_error(operation, pool_wait, begin_started.elapsed(), &error);
                return Err(error);
            }
            Err(_) => {
                let conn = snapshot
                    .conn
                    .take()
                    .expect("SQLite read snapshot connection");
                conn.detach().close().await.ok();
                let error = ProxyError::Database(sqlx::Error::PoolTimedOut);
                self.record_error(operation, pool_wait, begin_started.elapsed(), &error);
                return Err(error);
            }
        }
        snapshot.begin_wait = begin_started.elapsed();
        snapshot.started_at = Instant::now();
        snapshot.cache_write_pages_start = connection_cache_write_pages(&mut snapshot).await;
        let read_budget = max_run_budget.min(deadline.saturating_duration_since(Instant::now()));
        if read_budget.is_zero() {
            let error = ProxyError::Deferred {
                operation: operation.as_str(),
                reason: "recovery_budget".to_string(),
            };
            return match snapshot.close_after_query(Some(&error)).await {
                Ok(()) => Err(error),
                Err(close_error) => Err(close_error),
            };
        }
        if let Err(error) = snapshot
            .arm_cooperative_run_budget_with_cadence(read_budget, 1_000)
            .await
        {
            let close = snapshot.close_after_query(Some(&error)).await;
            return match close {
                Ok(()) => Err(error),
                Err(close_error) => Err(close_error),
            };
        }
        Ok(snapshot)
    }
}
