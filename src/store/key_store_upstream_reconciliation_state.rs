// Reconciliation admission, backoff and research bookkeeping.
//
// Split out of `key_store_upstream_reconciliation.rs` so the reconciliation host stays
// inside its line budget while both backends live side by side.

impl KeyStore {
    pub(crate) async fn upstream_reconciliation_run_admission_state(
        &self,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<UpstreamReconciliationRunAdmissionState, ProxyError> {
        if self.pg_pool.is_some() {
            let pool = self.pg()?;
            let claim_current = if let Some((job_id, claim_generation)) = claimed_job {
                sqlx::query_scalar::<_, i64>(
                    "SELECT EXISTS(SELECT 1 FROM core.scheduled_jobs WHERE id = $1 AND status = 'running' AND claim_generation = $2)::int::bigint",
                )
                .bind(job_id)
                .bind(claim_generation)
                .fetch_one(pool)
                .await?
                    != 0
            } else {
                true
            };
            let rows: Vec<(String, String)> = sqlx::query_as(
                r#"
                SELECT key, value
                FROM core.meta
                WHERE key IN ($1, $2, $3, $4, $5)
                "#,
            )
            .bind(META_KEY_UPSTREAM_PROJECT_ID_MODE_V1)
            .bind(META_KEY_API_REBALANCE_ENABLED_V1)
            .bind(META_KEY_REBALANCE_MCP_ENABLED_V1)
            .bind(META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_LEVEL_V1)
            .bind(META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_UNTIL_V1)
            .fetch_all(pool)
            .await?;
            let mode: String = sqlx::query_scalar(
                "SELECT mode FROM core.upstream_reconciliation_control_state WHERE id = 'local'",
            )
            .fetch_one(pool)
            .await?;
            let values = rows
                .into_iter()
                .collect::<std::collections::HashMap<_, _>>();
            let value_i64 = |key: &str| {
                values
                    .get(key)
                    .and_then(|value| value.parse::<i64>().ok())
                    .unwrap_or(0)
            };
            let shadow_ready = values
                .get(META_KEY_UPSTREAM_PROJECT_ID_MODE_V1)
                .and_then(|value| UpstreamProjectIdMode::from_meta_value(value))
                .unwrap_or_default()
                == UpstreamProjectIdMode::AccessToken
                && value_i64(META_KEY_API_REBALANCE_ENABLED_V1) != 0
                && value_i64(META_KEY_REBALANCE_MCP_ENABLED_V1) != 0;
            return Ok(UpstreamReconciliationRunAdmissionState {
                claim_current,
                shadow_ready,
                mode: ReconciliationMode::parse(&mode).ok_or_else(|| {
                    ProxyError::Other(
                        "invalid persisted upstream reconciliation mode".to_string(),
                    )
                })?,
                local_backoff_level: value_i64(
                    META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_LEVEL_V1,
                ),
                local_backoff_until: value_i64(
                    META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_UNTIL_V1,
                ),
            });
        }
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::ScheduledJobControl)
            .await?;
        #[allow(clippy::type_complexity)]
        let read_result: Result<(bool, Vec<(String, String)>, String), sqlx::Error> = async {
            let claim_current = if let Some((job_id, claim_generation)) = claimed_job {
                sqlx::query_scalar::<_, i64>(
                "SELECT EXISTS(SELECT 1 FROM scheduled_jobs WHERE id = ? AND status = 'running' AND claim_generation = ?)",
            )
            .bind(job_id)
            .bind(claim_generation)
            .fetch_one(&mut *conn)
                .await?
                    != 0
            } else {
                true
            };
            let rows: Vec<(String, String)> = sqlx::query_as(
                r#"
            SELECT key, value
            FROM meta
            WHERE key IN (?, ?, ?, ?, ?)
            "#,
        )
        .bind(META_KEY_UPSTREAM_PROJECT_ID_MODE_V1)
        .bind(META_KEY_API_REBALANCE_ENABLED_V1)
        .bind(META_KEY_REBALANCE_MCP_ENABLED_V1)
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_LEVEL_V1)
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_UNTIL_V1)
            .fetch_all(&mut *conn)
            .await?;
            let mode: String = sqlx::query_scalar(
                "SELECT mode FROM upstream_reconciliation_control_state WHERE id = 'local'",
            )
            .fetch_one(&mut *conn)
            .await?;
            Ok((claim_current, rows, mode))
        }
        .await;
        let (claim_current, rows, mode) = conn.complete_query(read_result).await?;
        let values = rows
            .into_iter()
            .collect::<std::collections::HashMap<_, _>>();
        let value_i64 = |key: &str| {
            values
                .get(key)
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(0)
        };
        let shadow_ready = values
            .get(META_KEY_UPSTREAM_PROJECT_ID_MODE_V1)
            .and_then(|value| UpstreamProjectIdMode::from_meta_value(value))
            .unwrap_or_default()
            == UpstreamProjectIdMode::AccessToken
            && value_i64(META_KEY_API_REBALANCE_ENABLED_V1) != 0
            && value_i64(META_KEY_REBALANCE_MCP_ENABLED_V1) != 0;
        Ok(UpstreamReconciliationRunAdmissionState {
            claim_current,
            shadow_ready,
            mode: ReconciliationMode::parse(&mode).ok_or_else(|| {
                ProxyError::Other("invalid persisted upstream reconciliation mode".to_string())
            })?,
            local_backoff_level: value_i64(META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_LEVEL_V1),
            local_backoff_until: value_i64(META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_UNTIL_V1),
        })
    }

    pub(crate) async fn api_key_transient_backoff_state(
        &self,
        key_id: &str,
        scope: &str,
    ) -> Result<Option<ApiKeyTransientBackoffState>, ProxyError> {
        if self.pg_pool.is_some() {
            let state = sqlx::query_as::<_, (i64, i64)>(
                r#"
                SELECT cooldown_until, retry_after_secs
                FROM core.api_key_transient_backoffs
                WHERE key_id = $1 AND scope = $2
                LIMIT 1
                "#,
            )
            .bind(key_id)
            .bind(scope)
            .fetch_optional(self.pg()?)
            .await?;
            return Ok(state.map(
                |(cooldown_until, retry_after_secs)| ApiKeyTransientBackoffState {
                    cooldown_until,
                    retry_after_secs,
                },
            ));
        }
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::ScheduledJobControl)
            .await?;
        let state = sqlx::query_as::<_, (i64, i64)>(
            r#"
            SELECT cooldown_until, retry_after_secs
            FROM api_key_transient_backoffs
            WHERE key_id = ? AND scope = ?
            LIMIT 1
            "#,
        )
        .bind(key_id)
        .bind(scope)
        .fetch_optional(&mut *conn)
        .await?;
        conn.close().await?;
        Ok(state.map(
            |(cooldown_until, retry_after_secs)| ApiKeyTransientBackoffState {
                cooldown_until,
                retry_after_secs,
            },
        ))
    }

    pub(crate) async fn mark_upstream_reconciliation_run_completed_at(
        &self,
        timestamp: i64,
    ) -> Result<(), ProxyError> {
        if self.pg_pool.is_some() {
            sqlx::query(
                r#"
                INSERT INTO core.meta (key, value)
                VALUES ($1, $2)
                ON CONFLICT (key) DO UPDATE SET value = excluded.value
                "#,
            )
            .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_RUN_AT_V1)
            .bind(timestamp.to_string())
            .execute(self.pg()?)
            .await?;
            return Ok(());
        }
        let mut transaction = self.sqlite_runtime.begin_scheduled_job_control().await?;
        let result = sqlx::query(
            r#"
            INSERT INTO meta (key, value)
            VALUES (?, ?)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            "#,
        )
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_RUN_AT_V1)
        .bind(timestamp.to_string())
        .execute(&mut *transaction)
        .await;
        match result {
            Ok(_) => transaction.finish(Ok(())).await,
            Err(err) => transaction.finish(Err(ProxyError::Database(err))).await,
        }
    }

    async fn upstream_reconciliation_shadow_ready_for_control(&self) -> Result<bool, ProxyError> {
        if self.pg_pool.is_some() {
            let rows: Vec<(String, String)> =
                sqlx::query_as("SELECT key, value FROM core.meta WHERE key IN ($1, $2, $3)")
                    .bind(META_KEY_UPSTREAM_PROJECT_ID_MODE_V1)
                    .bind(META_KEY_API_REBALANCE_ENABLED_V1)
                    .bind(META_KEY_REBALANCE_MCP_ENABLED_V1)
                    .fetch_all(self.pg()?)
                    .await?;
            let values = rows
                .into_iter()
                .collect::<std::collections::HashMap<_, _>>();
            let value_i64 = |key: &str| {
                values
                    .get(key)
                    .and_then(|value| value.parse::<i64>().ok())
                    .unwrap_or(0)
            };
            return Ok(values
                .get(META_KEY_UPSTREAM_PROJECT_ID_MODE_V1)
                .and_then(|value| UpstreamProjectIdMode::from_meta_value(value))
                .unwrap_or_default()
                == UpstreamProjectIdMode::AccessToken
                && value_i64(META_KEY_API_REBALANCE_ENABLED_V1) != 0
                && value_i64(META_KEY_REBALANCE_MCP_ENABLED_V1) != 0);
        }
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::ScheduledJobControl)
            .await?;
        let result: Result<_, ProxyError> = async {
            let rows: Vec<(String, String)> =
                sqlx::query_as("SELECT key, value FROM meta WHERE key IN (?, ?, ?)")
                    .bind(META_KEY_UPSTREAM_PROJECT_ID_MODE_V1)
                    .bind(META_KEY_API_REBALANCE_ENABLED_V1)
                    .bind(META_KEY_REBALANCE_MCP_ENABLED_V1)
                    .fetch_all(&mut *conn)
                    .await?;
            let values = rows
                .into_iter()
                .collect::<std::collections::HashMap<_, _>>();
            let value_i64 = |key: &str| {
                values
                    .get(key)
                    .and_then(|value| value.parse::<i64>().ok())
                    .unwrap_or(0)
            };
            Ok(values
                .get(META_KEY_UPSTREAM_PROJECT_ID_MODE_V1)
                .and_then(|value| UpstreamProjectIdMode::from_meta_value(value))
                .unwrap_or_default()
                == UpstreamProjectIdMode::AccessToken
                && value_i64(META_KEY_API_REBALANCE_ENABLED_V1) != 0
                && value_i64(META_KEY_REBALANCE_MCP_ENABLED_V1) != 0)
        }
        .await;
        let close = conn.close().await;
        match (result, close) {
            (Ok(shadow_ready), Ok(())) => Ok(shadow_ready),
            (Err(err), _) | (_, Err(err)) => Err(err),
        }
    }

    pub(crate) async fn record_upstream_reconciliation_run_stats(
        &self,
        duration_ms: i64,
        attempted: i64,
        settled: i64,
        no_adjustment: i64,
        upstream_429: i64,
        budget_exhausted: bool,
    ) -> Result<(), ProxyError> {
        if self.pg_pool.is_some() {
            sqlx::query(
                r#"INSERT INTO core.meta (key, value) VALUES
                       ($1, $2), ($3, $4), ($5, $6), ($7, $8), ($9, $10), ($11, $12)
                   ON CONFLICT (key) DO UPDATE SET value = excluded.value"#,
            )
            .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_DURATION_MS_V1)
            .bind(duration_ms.to_string())
            .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_ATTEMPTED_V1)
            .bind(attempted.to_string())
            .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_SETTLED_V1)
            .bind(settled.to_string())
            .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_NO_ADJUSTMENT_V1)
            .bind(no_adjustment.to_string())
            .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_429_V1)
            .bind(upstream_429.to_string())
            .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_BUDGET_EXHAUSTED_V1)
            .bind(if budget_exhausted { "1" } else { "0" })
            .execute(self.pg()?)
            .await?;
            return Ok(());
        }
        let mut transaction = self.begin_reconciliation_control().await?;
        let result = sqlx::query(
            r#"INSERT INTO meta (key, value) VALUES
                   (?, ?), (?, ?), (?, ?), (?, ?), (?, ?), (?, ?)
               ON CONFLICT(key) DO UPDATE SET value = excluded.value"#,
        )
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_DURATION_MS_V1)
        .bind(duration_ms.to_string())
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_ATTEMPTED_V1)
        .bind(attempted.to_string())
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_SETTLED_V1)
        .bind(settled.to_string())
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_NO_ADJUSTMENT_V1)
        .bind(no_adjustment.to_string())
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_429_V1)
        .bind(upstream_429.to_string())
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_BUDGET_EXHAUSTED_V1)
        .bind(if budget_exhausted { "1" } else { "0" })
        .execute(&mut *transaction)
        .await;
        match result {
            Ok(_) => transaction.finish(Ok(())).await,
            Err(err) => transaction.finish(Err(ProxyError::Database(err))).await,
        }
    }

    #[cfg(test)]
    async fn mark_upstream_reconciliation_research_sweep_at_inner(
        &self,
        timestamp: i64,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<(), ProxyError> {
        if self.pg_pool.is_some() {
            let pool = self.pg()?;
            let mut tx = pool.begin().await?;
            if !Self::reconciliation_claim_is_current_locked_pg(&mut tx, claimed_job).await? {
                let (job_id, claim_generation) = claimed_job.expect("claimed job was checked");
                tx.rollback().await?;
                return Err(ProxyError::StaleClaim {
                    job_id,
                    claim_generation,
                });
            }
            sqlx::query("INSERT INTO core.meta (key, value) VALUES ($1, $2) ON CONFLICT (key) DO UPDATE SET value = excluded.value")
                .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_RESEARCH_SWEEP_AT_V1)
                .bind(timestamp.to_string())
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(());
        }
        let mut transaction = self.begin_reconciliation_control().await?;
        if !Self::reconciliation_claim_is_current_locked(&mut transaction, claimed_job).await? {
            let (job_id, claim_generation) = claimed_job.expect("claimed job was checked");
            transaction.rollback().await?;
            return Err(ProxyError::StaleClaim {
                job_id,
                claim_generation,
            });
        }
        sqlx::query("INSERT INTO meta (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
            .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_RESEARCH_SWEEP_AT_V1)
            .bind(timestamp.to_string())
            .execute(&mut *transaction)
            .await?;
        transaction.finish(Ok(())).await
    }

    async fn mark_upstream_reconciliation_research_terminal_inner(
        &self,
        request_id: &str,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<bool, ProxyError> {
        let now = self.backend_time.now_ts();
        if self.pg_pool.is_some() {
            let pool = self.pg()?;
            let mut tx = pool.begin().await?;
            if !Self::reconciliation_claim_is_current_locked_pg(&mut tx, claimed_job).await? {
                let (job_id, claim_generation) = claimed_job.expect("claimed job was checked");
                tx.rollback().await?;
                return Err(ProxyError::StaleClaim {
                    job_id,
                    claim_generation,
                });
            }
            let changed = sqlx::query(
                r#"
                UPDATE core.upstream_reconciliation_research
                SET terminal_at = $1,
                    last_polled_at = $2,
                    next_poll_at = 0,
                    poll_attempt_count = poll_attempt_count + 1,
                    last_poll_outcome = 'terminal',
                    last_poll_error_kind = NULL,
                    updated_at = $3
                WHERE request_id = $4 AND terminal_at IS NULL
                "#,
            )
            .bind(now)
            .bind(now)
            .bind(now)
            .bind(request_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if changed > 0 {
                sqlx::query("INSERT INTO core.meta (key, value) VALUES ($1, $2) ON CONFLICT (key) DO UPDATE SET value = excluded.value")
                    .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_RESEARCH_TERMINAL_AT_V1)
                    .bind(now.to_string())
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
            return Ok(changed > 0);
        }
        let mut transaction = self.begin_reconciliation_control().await?;
        if !Self::reconciliation_claim_is_current_locked(&mut transaction, claimed_job).await? {
            let (job_id, claim_generation) = claimed_job.expect("claimed job was checked");
            transaction.rollback().await?;
            return Err(ProxyError::StaleClaim {
                job_id,
                claim_generation,
            });
        }
        let changed = sqlx::query(
            r#"
            UPDATE upstream_reconciliation_research
            SET terminal_at = ?,
                last_polled_at = ?,
                next_poll_at = 0,
                poll_attempt_count = poll_attempt_count + 1,
                last_poll_outcome = 'terminal',
                last_poll_error_kind = NULL,
                updated_at = ?
            WHERE request_id = ? AND terminal_at IS NULL
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(request_id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed > 0 {
            sqlx::query("INSERT INTO meta (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
                .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_RESEARCH_TERMINAL_AT_V1)
                .bind(now.to_string())
                .execute(&mut *transaction)
                .await?;
        }
        transaction.finish(Ok(())).await?;
        Ok(changed > 0)
    }

    async fn record_upstream_reconciliation_research_poll_inner(
        &self,
        request_id: &str,
        next_poll_at: i64,
        outcome: &str,
        error_kind: Option<&str>,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        if self.pg_pool.is_some() {
            let pool = self.pg()?;
            let mut tx = pool.begin().await?;
            if !Self::reconciliation_claim_is_current_locked_pg(&mut tx, claimed_job).await? {
                let (job_id, claim_generation) = claimed_job.expect("claimed job was checked");
                tx.rollback().await?;
                return Err(ProxyError::StaleClaim {
                    job_id,
                    claim_generation,
                });
            }
            sqlx::query(
                r#"
                UPDATE core.upstream_reconciliation_research
                SET last_polled_at = $1, next_poll_at = $2, poll_attempt_count = poll_attempt_count + 1,
                    last_poll_outcome = $3, last_poll_error_kind = $4, updated_at = $5
                WHERE request_id = $6 AND terminal_at IS NULL
                "#,
            )
            .bind(now)
            .bind(next_poll_at)
            .bind(outcome)
            .bind(error_kind)
            .bind(now)
            .bind(request_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok(());
        }
        let mut transaction = self.begin_reconciliation_control().await?;
        if !Self::reconciliation_claim_is_current_locked(&mut transaction, claimed_job).await? {
            let (job_id, claim_generation) = claimed_job.expect("claimed job was checked");
            transaction.rollback().await?;
            return Err(ProxyError::StaleClaim {
                job_id,
                claim_generation,
            });
        }
        sqlx::query(
            r#"
            UPDATE upstream_reconciliation_research
            SET last_polled_at = ?, next_poll_at = ?, poll_attempt_count = poll_attempt_count + 1,
                last_poll_outcome = ?, last_poll_error_kind = ?, updated_at = ?
            WHERE request_id = ? AND terminal_at IS NULL
            "#,
        )
        .bind(now)
        .bind(next_poll_at)
        .bind(outcome)
        .bind(error_kind)
        .bind(now)
        .bind(request_id)
        .execute(&mut *transaction)
        .await?;
        transaction.finish(Ok(())).await
    }
}
